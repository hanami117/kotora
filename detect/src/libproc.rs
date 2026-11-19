//! macOS's process table for the process source (`procfs.rs`), through libproc: the same calls
//! `lsof` makes. No permission is needed for the user's own processes.
//!
//! - `proc_listpids(PROC_UID_ONLY, uid)`: the user's processes.
//! - `proc_name`: the process name ("mpv", "IINA", "VLC").
//! - `proc_pidinfo(PROC_PIDLISTFDS)`, then `proc_pidfdinfo(PROC_PIDFDVNODEPATHINFO)` for each
//!   vnode fd: the open files' paths.
//!
//! `libc` has the functions but not `struct vnode_fdinfowithpath`; it is declared here from
//! `<sys/proc_info.h>`.

use std::ffi::{c_int, c_void};
use std::mem::size_of;

use crate::procfs::Procs;

/// `<sys/proc_info.h>`: list by real and effective uid.
const PROC_UID_ONLY: u32 = 4;
/// `<sys/proc_info.h>`: `proc_pidfdinfo` flavour returning `vnode_fdinfowithpath`.
const PROC_PIDFDVNODEPATHINFO: c_int = 2;

/// `struct proc_fileinfo`.
#[repr(C)]
#[derive(Clone, Copy)]
struct ProcFileInfo {
    fi_openflags: u32,
    fi_status: u32,
    fi_offset: i64,
    fi_type: i32,
    fi_guardflags: u32,
}

/// `struct vnode_fdinfowithpath`.
#[repr(C)]
#[derive(Clone, Copy)]
struct VnodeFdInfoWithPath {
    pfi: ProcFileInfo,
    pvip: libc::vnode_info_path,
}

pub struct LibProc {
    uid: u32,
}

impl LibProc {
    pub fn new() -> LibProc {
        // SAFETY: getuid has no preconditions and can't fail.
        LibProc {
            uid: unsafe { libc::getuid() },
        }
    }
}

impl Default for LibProc {
    fn default() -> Self {
        LibProc::new()
    }
}

impl Procs for LibProc {
    fn pids(&mut self) -> Vec<u32> {
        // SAFETY: a null buffer asks for the size in bytes.
        let need = unsafe { libc::proc_listpids(PROC_UID_ONLY, self.uid, std::ptr::null_mut(), 0) };
        if need <= 0 {
            return Vec::new();
        }
        // Room for processes started in between.
        let cap = need as usize / size_of::<c_int>() + 32;
        let mut buf = vec![0 as c_int; cap];
        // SAFETY: the buffer holds `cap` ints and its size in bytes is passed.
        let got = unsafe {
            libc::proc_listpids(
                PROC_UID_ONLY,
                self.uid,
                buf.as_mut_ptr().cast::<c_void>(),
                (cap * size_of::<c_int>()) as c_int,
            )
        };
        if got <= 0 {
            return Vec::new();
        }
        buf.truncate((got as usize / size_of::<c_int>()).min(cap));
        buf.into_iter()
            .filter(|p| *p > 0)
            .map(|p| p as u32)
            .collect()
    }

    fn name(&self, pid: u32) -> Option<String> {
        let mut b = [0u8; 256];
        // SAFETY: the buffer and its size match.
        let n = unsafe {
            libc::proc_name(
                pid as c_int,
                b.as_mut_ptr().cast::<c_void>(),
                b.len() as u32,
            )
        };
        (n > 0).then(|| String::from_utf8_lossy(&b[..(n as usize).min(b.len())]).into_owned())
    }

    fn mine(&self, _pid: u32) -> bool {
        // `pids` lists only the user's own processes.
        true
    }

    fn args(&self, pid: u32) -> Vec<String> {
        LibProc::procargs(pid)
    }

    fn files(&self, pid: u32) -> Vec<(i32, String)> {
        let pid = pid as c_int;
        let one = size_of::<libc::proc_fdinfo>();
        // SAFETY: a null buffer asks for the size in bytes.
        let need =
            unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
        if need <= 0 {
            return Vec::new();
        }
        let cap = need as usize / one + 16;
        let mut fds = vec![
            libc::proc_fdinfo {
                proc_fd: 0,
                proc_fdtype: 0
            };
            cap
        ];
        // SAFETY: the buffer holds `cap` entries and its size in bytes is passed.
        let got = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDLISTFDS,
                0,
                fds.as_mut_ptr().cast::<c_void>(),
                (cap * one) as c_int,
            )
        };
        if got <= 0 {
            return Vec::new();
        }
        fds.truncate((got as usize / one).min(cap));
        let mut out = Vec::new();
        for fd in fds
            .iter()
            .filter(|f| f.proc_fdtype == libc::PROX_FDTYPE_VNODE as u32)
        {
            // SAFETY: plain C data; all zero bits is a valid value.
            let mut info: VnodeFdInfoWithPath = unsafe { std::mem::zeroed() };
            let size = size_of::<VnodeFdInfoWithPath>();
            // SAFETY: `info` is `size` bytes, the layout of `struct vnode_fdinfowithpath`.
            let n = unsafe {
                libc::proc_pidfdinfo(
                    pid,
                    fd.proc_fd,
                    PROC_PIDFDVNODEPATHINFO,
                    (&mut info as *mut VnodeFdInfoWithPath).cast::<c_void>(),
                    size as c_int,
                )
            };
            if n as usize != size {
                continue;
            }
            let bytes: Vec<u8> = info
                .pvip
                .vip_path
                .iter()
                .flatten()
                .map(|c| *c as u8)
                .take_while(|b| *b != 0)
                .collect();
            if let Ok(path) = String::from_utf8(bytes)
                && !path.is_empty()
            {
                out.push((fd.proc_fd, path));
            }
        }
        out
    }
}

/// The argv in a `KERN_PROCARGS2` block: `argc` (native `int`), the executable's path, NUL
/// padding, then `argc` NUL-ended strings (the environment follows; it is not read).
pub fn parse_procargs2(b: &[u8]) -> Vec<String> {
    let Some(head) = b.get(..4) else {
        return Vec::new();
    };
    let argc = i32::from_ne_bytes([head[0], head[1], head[2], head[3]]).max(0) as usize;
    let mut rest = &b[4..];
    // The executable path, then the padding.
    let Some(end) = rest.iter().position(|c| *c == 0) else {
        return Vec::new();
    };
    rest = &rest[end..];
    let start = rest.iter().position(|c| *c != 0).unwrap_or(rest.len());
    rest = &rest[start..];
    rest.split(|c| *c == 0)
        .take(argc)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect()
}

impl LibProc {
    /// `sysctl(CTL_KERN, KERN_PROCARGS2, pid)`: works for the user's own processes.
    fn procargs(pid: u32) -> Vec<String> {
        let mut max: c_int = 0;
        let mut size = size_of::<c_int>();
        let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
        // SAFETY: `max` is an int and `size` its size.
        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                2,
                (&mut max as *mut c_int).cast::<c_void>(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } != 0
            || max <= 0
        {
            return Vec::new();
        }
        let mut buf = vec![0u8; max as usize];
        let mut size = buf.len();
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as c_int];
        // SAFETY: the buffer holds `size` bytes.
        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buf.as_mut_ptr().cast::<c_void>(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } != 0
        {
            return Vec::new();
        }
        buf.truncate(size);
        parse_procargs2(&buf)
    }
}

#[cfg(test)]
mod tests {
    //! The real process table: a copy of this test binary, named like a player, holds a dummy
    //! `.mkv` open (no player, no sound), and the scanner must find it.

    use super::*;
    use crate::Switch;
    use crate::procfs::Scanner;
    use std::io::Read;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    const HOLD: &str = "KOTORA_TEST_HOLD_FILE";

    /// Not a test: the body of the fake player. Opens the file in `HOLD` and keeps it open until
    /// stdin closes.
    #[test]
    #[ignore = "helper process for scanner_finds_players_by_their_open_file"]
    fn hold_open_helper() {
        let Some(path) = std::env::var_os(HOLD) else {
            return;
        };
        let _f = std::fs::File::open(path).unwrap();
        let mut sink = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut sink);
    }

    struct Fake {
        child: Child,
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            drop(self.child.stdin.take());
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// Starts a copy of this test binary named `name`, holding `file` open.
    fn fake_player(dir: &Path, name: &str, file: &Path) -> Fake {
        let exe = dir.join(name);
        let me = std::env::current_exe().unwrap();
        if std::fs::hard_link(&me, &exe).is_err() {
            std::fs::copy(&me, &exe).unwrap();
        }
        let child = Command::new(&exe)
            .args([
                "--ignored",
                "--exact",
                "libproc::tests::hold_open_helper",
                "--test-threads=1",
                "-q",
            ])
            .env(HOLD, file)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Fake { child }
    }

    #[test]
    fn scanner_finds_players_by_their_open_file() {
        let dir = std::env::temp_dir().join(format!("kotora-libproc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // The kernel reports the real path: `/var/folders` is `/private/var/folders`.
        let dir: PathBuf = dir.canonicalize().unwrap();
        let cases = [
            ("mpv", "mpv", Switch::Mpv),
            ("IINA", "IINA", Switch::Iina),
            ("VLC", "VLC", Switch::Vlc),
        ];
        let mut fakes = Vec::new();
        let mut want = Vec::new();
        for (i, (name, player, switch)) in cases.iter().enumerate() {
            let file = dir.join(format!("Kotora Test Show - 0{}.mkv", i + 1));
            std::fs::write(&file, b"").unwrap();
            let f = fake_player(&dir, name, &file);
            want.push((f.child.id(), *player, *switch, file.display().to_string()));
            fakes.push(f);
        }
        let mut sc = Scanner::new(LibProc::new());
        let start = Instant::now();
        let mut found = Vec::new();
        while start.elapsed() < Duration::from_secs(10) {
            found = sc.scan();
            if want
                .iter()
                .all(|(pid, ..)| found.iter().any(|f| f.pid == *pid && f.file.is_some()))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        for (pid, player, switch, file) in &want {
            let f = found
                .iter()
                .find(|f| f.pid == *pid)
                .unwrap_or_else(|| panic!("{player} (pid {pid}) not found: {found:?}"));
            assert_eq!((f.kind.player, f.kind.switch), (*player, *switch));
            assert_eq!(f.file.as_deref(), Some(file.as_str()));
        }
        // The test binary itself has no video open and isn't a player.
        assert!(!found.iter().any(|f| f.pid == std::process::id()));
        // A player that quits is gone on the next scan.
        let (pid, ..) = want[0];
        drop(fakes.remove(0));
        assert!(!sc.scan().iter().any(|f| f.pid == pid));
        drop(fakes);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Reading detection, stage 2: a process named like a reader holding a `.cbz`, and one with
    /// another name holding a page image inside a comic folder; the real process table.
    #[test]
    fn comic_scanner_finds_open_comics() {
        let dir =
            std::env::temp_dir().join(format!("kotora-libproc-comics-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Manga/Kotora Test Comic/Ch 2")).unwrap();
        let dir: PathBuf = dir.canonicalize().unwrap();
        let cbz = dir.join("Kotora Test Comic c001.cbz");
        std::fs::write(&cbz, b"").unwrap();
        let page = dir.join("Manga/Kotora Test Comic/Ch 2/001.jpg");
        std::fs::write(&page, b"").unwrap();
        let reader = fake_player(&dir, "Simple Comic", &cbz);
        let other = fake_player(&dir, "pageviewer", &page);
        let folders = vec![dir.join("Manga").display().to_string()];
        let mut sc = crate::comics::ComicScanner::new(LibProc::new());
        let start = Instant::now();
        let mut found = Vec::new();
        while start.elapsed() < Duration::from_secs(10) {
            found = sc.scan(&folders);
            if found.len() >= 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let v: Vec<(u32, &str, &str)> = found
            .iter()
            .map(|o| (o.pid, o.app.as_str(), o.path.as_str()))
            .collect();
        let (cbz, chapter) = (
            cbz.display().to_string(),
            dir.join("Manga/Kotora Test Comic/Ch 2")
                .display()
                .to_string(),
        );
        assert!(
            v.contains(&(reader.child.id(), "Simple Comic", cbz.as_str())),
            "{v:?}"
        );
        assert!(
            v.contains(&(other.child.id(), "pageviewer", chapter.as_str())),
            "{v:?}"
        );
        // Without comic folders, the unknown process is not looked at.
        assert!(!sc.scan(&[]).iter().any(|o| o.pid == other.child.id()));
        drop((reader, other));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Lightness: one scan of the real process table, without and with a comic folder (every
    /// process's open files are read then). `cargo test -p kotora-detect comic_scan_cost --
    /// --ignored --nocapture`, release.
    #[test]
    #[ignore = "timing"]
    fn comic_scan_cost() {
        let mut sc = crate::comics::ComicScanner::new(LibProc::new());
        let t = Instant::now();
        sc.scan(&[]);
        let first = t.elapsed();
        let t = Instant::now();
        for _ in 0..20 {
            sc.scan(&[]);
        }
        let readers = t.elapsed() / 20;
        let folders = vec![std::env::temp_dir().display().to_string()];
        let t = Instant::now();
        for _ in 0..5 {
            sc.scan(&folders);
        }
        println!(
            "first scan {first:?}; readers only {readers:?}; with a comic folder {:?}",
            t.elapsed() / 5
        );
    }

    #[test]
    fn procargs2_blocks() {
        let mut b = 3i32.to_ne_bytes().to_vec();
        b.extend_from_slice(b"/Applications/YACReader.app/Contents/MacOS/YACReader\0\0\0\0");
        b.extend_from_slice(b"/Applications/YACReader.app/Contents/MacOS/YACReader\0/Volumes/Disk/Movies\0--comicId=1\0HOME=/x\0");
        assert_eq!(
            parse_procargs2(&b),
            [
                "/Applications/YACReader.app/Contents/MacOS/YACReader",
                "/Volumes/Disk/Movies",
                "--comicId=1"
            ]
        );
        assert!(parse_procargs2(&[1, 0]).is_empty());
    }

    /// The real sysctl: a copy of this binary named like the viewer, started with YACReader's
    /// arguments (after the test harness's own), is found by its command line.
    #[test]
    fn the_viewer_is_found_by_its_real_command_line() {
        let dir = std::env::temp_dir().join(format!("kotora-libproc-args-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir: PathBuf = dir.canonicalize().unwrap();
        let file = dir.join("held.txt");
        std::fs::write(&file, b"").unwrap();
        let exe = dir.join("YACReader");
        let me = std::env::current_exe().unwrap();
        if std::fs::hard_link(&me, &exe).is_err() {
            std::fs::copy(&me, &exe).unwrap();
        }
        let lib = dir.display().to_string();
        let child = Command::new(&exe)
            .args([
                "--ignored",
                "--exact",
                "libproc::tests::hold_open_helper",
                "--test-threads=1",
                "-q",
                "--",
                &lib,
                "--comicId=7",
                "--libraryId=1",
            ])
            .env(HOLD, &file)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let fake = Fake { child };
        let mut sc = crate::comics::ComicScanner::new(LibProc::new());
        let opts = crate::comics::ScanOpts {
            yac_args: true,
            ..Default::default()
        };
        let start = Instant::now();
        let mut found = Vec::new();
        while start.elapsed() < Duration::from_secs(10) && found.is_empty() {
            found = sc
                .scan_with(&opts)
                .into_iter()
                .filter(|o| o.pid == fake.child.id())
                .collect();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(
            found.first().map(|o| (o.path.as_str(), o.yac)),
            Some((lib.as_str(), Some(7))),
            "{found:?}"
        );
        drop(fake);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
