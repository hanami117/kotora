//! Windows: the real process list, command lines and window titles behind `winplayers.rs`.
//!
//! What a scan costs:
//! - one snapshot of the process list (`CreateToolhelp32Snapshot`), compared by name with what
//!   the last scan saw;
//! - for a **new** player process only: one `OpenProcess` with the least right there is
//!   (`PROCESS_QUERY_LIMITED_INFORMATION`), its path and its command line. No memory of another
//!   process is read and no handle table is walked;
//! - while a player runs: one pass over the top-level windows (`EnumWindows`), reading the
//!   titles of the players' windows. `GetWindowTextW` reads another process's caption without
//!   sending it a message, so a hung player can't block the scan;
//! - PotPlayer only: three `SendMessageTimeoutW` calls, each given 200 ms at most.
//!
//! The `scan_cost` test prints the measured time of a scan on the machine it runs on.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};

use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
use windows_sys::Win32::Foundation::{BOOL, CloseHandle, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, UNICODE_STRING};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_USER};

use crate::procfs::INTERVAL;
use crate::run::{Input, MpcProcess, Shared};
use crate::winplayers::{Pot, ProcInfo, Reporter, Scanner, Seen, Sys, Win};

/// PotPlayer's messages (its SDK header): `SendMessage(hwnd, WM_USER, what, 0)`.
const POT_GET_TOTAL_TIME: usize = 0x5002;
const POT_GET_CURRENT_TIME: usize = 0x5004;
const POT_GET_PLAY_STATUS: usize = 0x5006;

/// The live system.
pub struct System;

fn text(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// The full path of a process's program.
fn exe_path(h: HANDLE) -> Option<String> {
    let mut buf = vec![0u16; 1024];
    loop {
        let mut len = buf.len() as u32;
        // SAFETY: `buf` holds `len` UTF-16 units and `h` is an open process handle.
        if unsafe { QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) } != 0 {
            return Some(String::from_utf16_lossy(&buf[..len as usize]));
        }
        if buf.len() >= 32_768 {
            return None;
        }
        buf.resize(32_768, 0);
    }
}

/// The command line a process was started with (Windows 8.1 and later).
fn command_line(h: HANDLE) -> Option<String> {
    // u64 storage: the answer starts with a UNICODE_STRING, which holds a pointer.
    let mut buf = vec![0u64; 1024];
    for _ in 0..2 {
        let mut need = 0u32;
        let size = (buf.len() * 8) as u32;
        // SAFETY: `buf` is `size` bytes, aligned for UNICODE_STRING; `h` is an open handle.
        let status = unsafe { NtQueryInformationProcess(h, ProcessCommandLineInformation, buf.as_mut_ptr().cast(), size, &mut need) };
        if status >= 0 {
            // SAFETY: on success the buffer starts with a UNICODE_STRING whose `Buffer` points
            // at `Length` bytes of UTF-16 further on in the same buffer.
            let line = unsafe {
                let us = &*(buf.as_ptr() as *const UNICODE_STRING);
                if us.Buffer.is_null() {
                    return Some(String::new());
                }
                let start = us.Buffer as usize;
                let (lo, hi) = (buf.as_ptr() as usize, buf.as_ptr() as usize + size as usize);
                if start < lo || start + us.Length as usize > hi {
                    return None;
                }
                String::from_utf16_lossy(std::slice::from_raw_parts(us.Buffer, us.Length as usize / 2))
            };
            return Some(line);
        }
        if need as usize <= buf.len() * 8 {
            return None;
        }
        buf.resize((need as usize).div_ceil(8) + 8, 0);
    }
    None
}

struct Titles<'a> {
    pids: &'a HashSet<u32>,
    out: HashMap<u32, Vec<Win>>,
}

unsafe extern "system" fn each_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the `Titles` that `System::windows` passes, alive for the whole
    // `EnumWindows` call; the buffers are as long as the lengths given.
    unsafe {
        let ctx = &mut *(lparam as *mut Titles);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if !ctx.pids.contains(&pid) || IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        let mut buf = [0u16; 1024];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if n <= 0 {
            return 1;
        }
        let title = String::from_utf16_lossy(&buf[..n as usize]);
        let mut cls = [0u16; 256];
        let n = GetClassNameW(hwnd, cls.as_mut_ptr(), cls.len() as i32).max(0);
        let class = String::from_utf16_lossy(&cls[..n as usize]);
        ctx.out.entry(pid).or_default().push(Win { hwnd: hwnd as isize, title, class });
    }
    1
}

impl Sys for System {
    fn processes(&mut self) -> Vec<(u32, String)> {
        let mut out = Vec::with_capacity(256);
        // SAFETY: the snapshot handle is checked and closed; the entry's size field is set as
        // the API asks.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return out;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snap, &mut e);
            while more != 0 {
                out.push((e.th32ProcessID, text(&e.szExeFile)));
                more = Process32NextW(snap, &mut e);
            }
            CloseHandle(snap);
        }
        out
    }

    fn info(&mut self, pid: u32) -> Option<ProcInfo> {
        // SAFETY: the handle is checked and closed.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let exe = exe_path(h);
            let command_line = command_line(h).unwrap_or_default();
            CloseHandle(h);
            Some(ProcInfo { exe: exe?, command_line })
        }
    }

    fn windows(&mut self, pids: &HashSet<u32>) -> HashMap<u32, Vec<Win>> {
        let mut ctx = Titles { pids, out: HashMap::new() };
        // SAFETY: `ctx` outlives the call; the callback only uses it during the call.
        unsafe { EnumWindows(Some(each_window), &mut ctx as *mut Titles as LPARAM) };
        ctx.out
    }

    fn potplayer(&mut self, hwnd: isize) -> Option<Pot> {
        let ask = |what: usize| -> Option<i64> {
            let mut answer = 0usize;
            // SAFETY: a plain message to a window handle (a stale handle makes the call fail);
            // it gives up after 200 ms or at once when the window hangs.
            let ok = unsafe { SendMessageTimeoutW(hwnd as HWND, WM_USER, what, 0, SMTO_ABORTIFHUNG, 200, &mut answer) };
            (ok != 0).then_some(answer as isize as i64)
        };
        Some(Pot { status: ask(POT_GET_PLAY_STATUS)?, position_ms: ask(POT_GET_CURRENT_TIME)?, duration_ms: ask(POT_GET_TOTAL_TIME)? })
    }
}

/// The players running now, with what they show: one scan, for the "which mpv.conf" question
/// and "is MPC still open".
pub fn players() -> Vec<Seen> {
    Scanner::new(System).scan()
}

/// The thread: scan, report, sleep [`INTERVAL`] (or until the settings change). While the
/// source is off it blocks until the settings change. It also tells MPC's web source whether
/// an MPC-HC or MPC-BE process runs, so that source asks nothing when none does.
pub fn run(shared: Arc<Shared>, wake: Receiver<()>, tx: Sender<Input>, mpc_wake: Option<Sender<()>>) {
    let mut scanner = Scanner::new(System);
    let mut reporter = Reporter::default();
    let mut mpc_before = None;
    loop {
        let on = shared.processes();
        let seen = if on {
            scanner.scan()
        } else {
            scanner.reset();
            Vec::new()
        };
        let mpc = match seen.iter().find(|s| s.family.is_mpc()) {
            _ if !on => MpcProcess::Unknown,
            Some(s) => MpcProcess::Running(s.kind.player),
            None => MpcProcess::NotRunning,
        };
        if mpc_before != Some(mpc) {
            mpc_before = Some(mpc);
            shared.set_mpc_process(mpc);
            if let Some(w) = &mpc_wake {
                let _ = w.send(());
            }
        }
        for input in reporter.apply(&seen, crate::clock()) {
            if tx.send(input).is_err() {
                return;
            }
        }
        let got = if on { wake.recv_timeout(INTERVAL) } else { wake.recv().map_err(|_| RecvTimeoutError::Disconnected) };
        if got == Err(RecvTimeoutError::Disconnected) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::winplayers::{Family, Shown, read_title};
    use std::time::{Duration, Instant};

    /// This test program started again as a player (see `a_real_player_process_is_found`): it
    /// only waits to be killed.
    #[test]
    fn stand_in_player() {
        if std::env::var_os("KOTORA_TEST_STAND_IN").is_some() {
            std::thread::sleep(Duration::from_secs(60));
        }
    }

    /// The real calls end to end: a real process named `mpv.exe` (a copy of this test program)
    /// started with a video file on its command line is found, with its path and that file.
    #[test]
    fn a_real_player_process_is_found() {
        let dir = std::env::temp_dir().join(format!("kotora-winproc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("mpv.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        let file = r"D:\Anime\[Hoshi] Lanterns of Kirigaoka - 06 (1080p).mkv";
        // The test harness takes the file as one more filter and runs `stand_in_player`.
        let mut child = std::process::Command::new(&exe)
            .args(["--exact", "winproc::tests::stand_in_player", file])
            .env("KOTORA_TEST_STAND_IN", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut scanner = Scanner::new(System);
        let mut found = None;
        let start = Instant::now();
        while found.is_none() && start.elapsed() < Duration::from_secs(10) {
            found = scanner.scan().into_iter().find(|s| s.pid == pid);
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        let seen = found.expect("the stand-in mpv.exe was not found");
        assert_eq!((seen.kind.player, seen.family), ("mpv", Family::Mpv));
        assert!(seen.exe.to_lowercase().ends_with(r"\mpv.exe"), "{}", seen.exe);
        // No window: the command line is all there is.
        assert_eq!(seen.file.as_deref(), Some(file));
        // It is gone at the next scan.
        assert!(!scanner.scan().iter().any(|s| s.pid == pid));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A real window of this process is listed with its title and class, and reads as a file.
    #[test]
    fn a_real_window_title_is_read() {
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_OVERLAPPEDWINDOW, WS_VISIBLE};
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let title = "[Hoshi] Lanterns of Kirigaoka - 06 (1080p).mkv - mpv";
        // SAFETY: a plain window of a system class, destroyed below; strings are NUL-terminated.
        let hwnd = unsafe { CreateWindowExW(0, wide("STATIC").as_ptr(), wide(title).as_ptr(), WS_OVERLAPPEDWINDOW | WS_VISIBLE, 0, 0, 200, 100, std::ptr::null_mut(), std::ptr::null_mut(), GetModuleHandleW(std::ptr::null()), std::ptr::null()) };
        assert!(!hwnd.is_null(), "no window: {}", std::io::Error::last_os_error());
        let me = std::process::id();
        let wins = System.windows(&HashSet::from([me]));
        // SAFETY: the window made above, on this thread.
        unsafe { DestroyWindow(hwnd) };
        let mine = wins.get(&me).cloned().unwrap_or_default();
        let w = mine.iter().find(|w| w.hwnd == hwnd as isize).unwrap_or_else(|| panic!("the window is not listed: {mine:?}"));
        assert_eq!((w.title.as_str(), w.class.to_lowercase().as_str()), (title, "static"));
        assert_eq!(read_title(Family::Mpv, w), Some(Shown::Media("[Hoshi] Lanterns of Kirigaoka - 06 (1080p).mkv".into())));
        // A window that isn't PotPlayer's answers nothing useful: no position is made up.
        assert_eq!(System.potplayer(0).and_then(Pot::pos), None);
    }

    /// What a scan costs on this machine, printed for the build log: the wall time and the
    /// processor time of the idle scan (no player running) that repeats every 5 s.
    #[test]
    fn scan_cost() {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        let cpu = || {
            let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
            let (mut c, mut e, mut k, mut u) = (z, z, z, z);
            // SAFETY: four FILETIMEs to fill, for this process.
            unsafe { GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) };
            let t = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32 | u64::from(f.dwLowDateTime)) as f64 / 10_000.0;
            t(k) + t(u)
        };
        let mut scanner = Scanner::new(System);
        let processes = System.processes().len();
        scanner.scan();
        const N: u32 = 200;
        let (cpu0, start) = (cpu(), Instant::now());
        for _ in 0..N {
            scanner.scan();
        }
        let wall_ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(N);
        let cpu_ms = (cpu() - cpu0) / f64::from(N);
        let share = cpu_ms / (INTERVAL.as_secs_f64() * 1000.0) * 100.0;
        println!("scan_cost: {processes} processes; one scan {wall_ms:.3} ms wall, {cpu_ms:.3} ms CPU; at one scan every {} s that is {share:.4} % of one core", INTERVAL.as_secs());
        // The window pass, as when a player runs (here: this process stands in for it).
        let me = HashSet::from([std::process::id()]);
        let start = Instant::now();
        for _ in 0..N {
            System.windows(&me);
        }
        println!("scan_cost: window pass {:.3} ms wall", start.elapsed().as_secs_f64() * 1000.0 / f64::from(N));
        assert!(processes > 5, "the process list is empty");
        assert!(wall_ms < 50.0, "a scan takes {wall_ms} ms");
    }
}
