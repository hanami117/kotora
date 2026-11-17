//! Windows-only glue: where the app's text output goes, opening files and links, the folder
//! picker, letting the running copy come to the front, the app id (taskbar and toasts), the
//! screen's usable area and scale, and a few registry calls (`reg`).

use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::IntoRawHandle;

use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};
use windows_sys::Win32::UI::Shell::{SetCurrentProcessExplicitAppUserModelID, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow, SW_SHOWNORMAL};

/// The log is started again when it is larger than this at start.
const LOG_MAX: u64 = 1024 * 1024;

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn has(std_handle: u32) -> bool {
    // SAFETY: no preconditions.
    let h: HANDLE = unsafe { GetStdHandle(std_handle) };
    !h.is_null() && h != INVALID_HANDLE_VALUE
}

fn point(std_handle: u32, file: std::fs::File) {
    // SAFETY: the handle stays open for the life of the process (it is never closed).
    unsafe { SetStdHandle(std_handle, file.into_raw_handle()) };
}

/// Kotora is a windowed program, so it starts with no console. This gives its messages a place:
/// - output that is already redirected (`kotora --list-scenes > file`, a pipe) is left alone;
/// - started from a terminal, it writes to that terminal;
/// - started from Explorer, the Start menu or at login, it appends to
///   `%LOCALAPPDATA%\kotora\Logs\kotora.log`.
pub fn attach_output() {
    let (out, err) = (has(STD_OUTPUT_HANDLE), has(STD_ERROR_HANDLE));
    if out && err {
        return;
    }
    let console = || std::fs::OpenOptions::new().read(true).write(true).open("CONOUT$").ok();
    // SAFETY: no preconditions; fails when the parent has no console.
    let attached = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } != 0;
    for (present, which) in [(out, STD_OUTPUT_HANDLE), (err, STD_ERROR_HANDLE)] {
        if present {
            continue;
        }
        let target = if attached { console() } else { None }.or_else(log_file);
        if let Some(f) = target {
            point(which, f);
        }
    }
    if !attached {
        eprintln!("--- kotora {} started (unix time {})", kotora_ctl::version_line(), kotora_core::time::now_unix());
    }
}

/// Where the log is.
pub fn log_path() -> std::path::PathBuf {
    kotora_core::paths::log_dir().join("kotora.log")
}

fn log_file() -> Option<std::fs::File> {
    let path = log_path();
    std::fs::create_dir_all(path.parent()?).ok()?;
    let big = std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_MAX);
    let mut o = std::fs::OpenOptions::new();
    o.create(true);
    if big { o.write(true).truncate(true) } else { o.append(true) };
    o.open(&path).ok()
}

/// Opens a file with its default program, or a link in the browser (`ShellExecuteW`, "open").
pub fn open(target: &str) -> Result<(), String> {
    let (verb, target_w) = (wide("open"), wide(target));
    // SAFETY: both strings are NUL-terminated and live across the call.
    let r = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), target_w.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
    // A value above 32 is success (the call's old contract).
    if r as usize > 32 { Ok(()) } else { Err(format!("Windows can't open {target} (code {}).", r as usize)) }
}

/// The second copy calls this before it asks the running one to show its window: Windows only
/// lets a background program come to the front when the foreground one allows it.
pub fn allow_focus() {
    // SAFETY: no preconditions.
    unsafe { AllowSetForegroundWindow(ASFW_ANY) };
}

/// The system's folder picker. `Ok(None)`: cancelled. Call it on a worker thread: it blocks
/// until the dialog closes.
pub fn pick_folder(title: &str) -> Result<Option<String>, String> {
    use windows::Win32::Foundation::ERROR_CANCELLED;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize};
    use windows::Win32::UI::Shell::{FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
    use windows::core::{HRESULT, HSTRING};

    // SAFETY: COM is started for this thread and stopped again when it was started here; every
    // interface is released when its wrapper drops; the name string is freed as COM asks.
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let picked = (|| -> windows::core::Result<Option<String>> {
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(dialog.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)?;
            dialog.SetTitle(&HSTRING::from(title))?;
            if let Err(e) = dialog.Show(None) {
                return if e.code() == HRESULT::from_win32(ERROR_CANCELLED.0) { Ok(None) } else { Err(e) };
            }
            let name = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path = name.to_string().ok();
            CoTaskMemFree(Some(name.0 as *const _));
            Ok(path)
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        picked.map_err(|e| format!("The folder picker didn't open: {e}"))
    }
}

/// The 256 px app icon, written beside the database so Windows can draw it on toasts.
const ICON_PNG: &[u8] = include_bytes!("../../assets/icons/hicolor/256x256/apps/io.github.hanami117.kotora.png");

/// Where the app id's name and icon are registered for toasts, under `HKEY_CURRENT_USER`.
pub fn app_id_key() -> String {
    format!(r"Software\Classes\AppUserModelId\{}", crate::APP_ID)
}

/// Gives this process Kotora's app id (the installer's Start menu shortcut carries the same
/// one), so the taskbar groups its windows under that shortcut and toasts are Kotora's. Call
/// it before any window opens.
pub fn set_app_id() {
    let id = wide(crate::APP_ID);
    // SAFETY: a NUL-terminated string that lives across the call.
    unsafe { SetCurrentProcessExplicitAppUserModelID(id.as_ptr()) };
}

/// Registers the app id's name and icon for the current user: what a toast shows as its
/// sender, also for a copy that was only unpacked (no installer, no shortcut). The installer's
/// uninstaller removes the key. Returns false when the registry refused.
pub fn register_app_id() -> bool {
    let key = app_id_key();
    let mut ok = reg::set_string(&key, "DisplayName", "Kotora").is_ok();
    let icon = kotora_core::paths::log_dir().parent().map(|d| d.join("kotora-icon.png"));
    if let Some(icon) = icon {
        let fresh = std::fs::metadata(&icon).is_ok_and(|m| m.len() == ICON_PNG.len() as u64);
        if fresh || (icon.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && std::fs::write(&icon, ICON_PNG).is_ok()) {
            ok &= reg::set_string(&key, "IconUri", &icon.to_string_lossy()).is_ok();
        }
    }
    ok
}

/// The window shows: give it the program's own icon (the one `build.rs` put into the exe, id 1),
/// for the title bar, the taskbar and Alt+Tab. Rune sets none, and Windows would draw its
/// blank default. On the UI thread, once per window.
pub fn on_window(handle: &rune::WindowHandle) {
    use rune::raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW, WM_SETICON};

    let _ = handle.with_raw_handle(|h| {
        let RawWindowHandle::Win32(h) = h.as_raw() else { return };
        let hwnd = h.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        // SAFETY: the window is alive for this callback; resource id 1 is passed the way
        // MAKEINTRESOURCE does (a small number in place of a name); LR_SHARED icons belong
        // to the system and are never freed by us. A missing icon gives null, and the
        // window keeps the default.
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            for (which, cx, cy) in [(ICON_BIG, SM_CXICON, SM_CYICON), (ICON_SMALL, SM_CXSMICON, SM_CYSMICON)] {
                let icon = LoadImageW(module, std::ptr::without_provenance::<u16>(1), IMAGE_ICON, GetSystemMetrics(cx), GetSystemMetrics(cy), LR_SHARED);
                if !icon.is_null() {
                    SendMessageW(hwnd, WM_SETICON, which as usize, icon as isize);
                }
            }
        }
    });
}

/// The primary screen's work area (the screen less the taskbar) in points, and its scale.
/// None when Windows doesn't say.
pub fn work_area() -> Option<(f32, f32)> {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint};
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    // SAFETY: plain calls; `info` has its size set, as GetMonitorInfoW asks, and the out
    // pointers live across the calls.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        if monitor.is_null() {
            return None;
        }
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, rcMonitor: RECT { left: 0, top: 0, right: 0, bottom: 0 }, rcWork: RECT { left: 0, top: 0, right: 0, bottom: 0 }, dwFlags: 0 };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) != 0 || dx == 0 {
            dx = 96;
        }
        points((info.rcWork.right - info.rcWork.left, info.rcWork.bottom - info.rcWork.top), dx)
    }
}

/// A size in pixels at `dpi` as points (96 dpi is scale 1). None for an empty area.
fn points(px: (i32, i32), dpi: u32) -> Option<(f32, f32)> {
    let scale = dpi.max(48) as f32 / 96.0;
    (px.0 > 0 && px.1 > 0).then(|| (px.0 as f32 / scale, px.1 as f32 / scale))
}

/// The side of a notification-area icon in pixels at the system's scale: 16 at 100 %, 20 at
/// 125 %, 24 at 150 %, 32 at 200 %.
pub fn tray_icon_px() -> u32 {
    use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
    use windows_sys::Win32::UI::WindowsAndMessaging::SM_CXSMICON;
    // SAFETY: no preconditions.
    let px = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) };
    (px.clamp(16, 64)) as u32
}

/// The few registry calls the app needs, all under `HKEY_CURRENT_USER`.
pub mod reg {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegGetValueW, RegSetValueExW};

    use super::wide;

    fn io(code: u32) -> std::io::Error {
        std::io::Error::from_raw_os_error(code as i32)
    }

    /// Writes a text value; the key is made when it is missing.
    pub fn set_string(key: &str, name: &str, value: &str) -> std::io::Result<()> {
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: the key name is NUL-terminated; the handle is closed below.
        let r = unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, wide(key).as_ptr(), 0, std::ptr::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, std::ptr::null(), &mut h, std::ptr::null_mut()) };
        if r != ERROR_SUCCESS {
            return Err(io(r));
        }
        let data = wide(value);
        // SAFETY: `h` is open for writing; `data` is NUL-terminated UTF-16 and its size is in bytes.
        let r = unsafe { RegSetValueExW(h, wide(name).as_ptr(), 0, REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32) };
        // SAFETY: `h` was opened above.
        unsafe { RegCloseKey(h) };
        if r == ERROR_SUCCESS { Ok(()) } else { Err(io(r)) }
    }

    /// A value's bytes, when it has the type `flags` asks for. `None`: no such key or value.
    fn get(key: &str, name: &str, flags: u32) -> std::io::Result<Option<Vec<u8>>> {
        let (key, name) = (wide(key), wide(name));
        let mut buf = vec![0u8; 1024];
        loop {
            let mut size = buf.len() as u32;
            // SAFETY: the strings are NUL-terminated; `buf` is `size` bytes long.
            let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), flags, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size) };
            match r {
                ERROR_SUCCESS => {
                    buf.truncate(size as usize);
                    return Ok(Some(buf));
                }
                ERROR_MORE_DATA if buf.len() < 64 * 1024 => buf.resize(size as usize + 2, 0),
                ERROR_FILE_NOT_FOUND => return Ok(None),
                e => return Err(io(e)),
            }
        }
    }

    /// A text value.
    pub fn get_string(key: &str, name: &str) -> std::io::Result<Option<String>> {
        Ok(get(key, name, RRF_RT_REG_SZ)?.map(|b| {
            let w: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|c| *c != 0).collect();
            String::from_utf16_lossy(&w)
        }))
    }

    /// A binary value.
    pub fn get_binary(key: &str, name: &str) -> std::io::Result<Option<Vec<u8>>> {
        get(key, name, RRF_RT_REG_BINARY)
    }

    /// Removes a value; one that isn't there is fine.
    pub fn delete_value(key: &str, name: &str) -> std::io::Result<()> {
        // SAFETY: both strings are NUL-terminated.
        match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(key).as_ptr(), wide(name).as_ptr()) } {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            e => Err(io(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_become_points_at_the_screens_scale() {
        assert_eq!(points((1920, 1040), 96), Some((1920.0, 1040.0)));
        assert_eq!(points((2880, 1704), 144), Some((1920.0, 1136.0)));
        assert_eq!(points((3840, 2080), 192), Some((1920.0, 1040.0)));
        assert_eq!(points((0, 0), 96), None);
    }

    #[test]
    fn this_machine_answers_sanely() {
        // A runner has a screen of some size (or none): never nonsense.
        if let Some((w, h)) = work_area() {
            assert!((200.0..20000.0).contains(&w) && (200.0..20000.0).contains(&h), "{w}x{h}");
            eprintln!("work area {w}x{h} pt");
        }
        assert!((16..=64).contains(&tray_icon_px()));
    }

    #[test]
    fn registry_values_round_trip_on_a_scratch_key() {
        use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};
        let key = format!(r"Software\KotoraTest-app-{}", std::process::id());
        assert_eq!(reg::get_string(&key, "a").unwrap(), None);
        reg::delete_value(&key, "a").unwrap();
        reg::set_string(&key, "a", r#""C:\Program Files\Kotora\kotora.exe" --hidden"#).unwrap();
        assert_eq!(reg::get_string(&key, "a").unwrap().as_deref(), Some(r#""C:\Program Files\Kotora\kotora.exe" --hidden"#));
        assert_eq!(reg::get_binary(&key, "missing").unwrap(), None);
        reg::delete_value(&key, "a").unwrap();
        assert_eq!(reg::get_string(&key, "a").unwrap(), None);
        // SAFETY: deletes the scratch key made above; the name is NUL-terminated.
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(&key).as_ptr()) };
    }
}
