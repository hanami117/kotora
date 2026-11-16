//! Windows-only glue: where the app's text output goes, opening files and links, the folder
//! picker, and letting the running copy come to the front.

use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::IntoRawHandle;

use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
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
