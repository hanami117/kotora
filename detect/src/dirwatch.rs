//! Waits for a change in one folder: inotify on Linux, kqueue on macOS. Used while an mpv socket
//! is missing, so finding it needs no poll: the thread sleeps in the kernel until something in
//! the folder is created, renamed or removed.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;

pub struct DirWatch {
    fd: OwnedFd,
    /// macOS: the folder's own descriptor, kept open while kqueue watches it.
    #[cfg(target_os = "macos")]
    _dir: OwnedFd,
}

fn cpath(p: &Path) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(p.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path holds a NUL byte"))
}

fn check(r: libc::c_int) -> io::Result<libc::c_int> {
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

impl DirWatch {
    #[cfg(target_os = "linux")]
    pub fn new(dir: &Path) -> io::Result<DirWatch> {
        let path = cpath(dir)?;
        // SAFETY: plain syscalls; the descriptor is owned right away.
        let fd = unsafe { OwnedFd::from_raw_fd(check(libc::inotify_init1(libc::IN_CLOEXEC))?) };
        let mask = libc::IN_CREATE | libc::IN_MOVED_TO | libc::IN_ATTRIB | libc::IN_DELETE_SELF;
        // SAFETY: `path` is a valid C string for the call.
        check(unsafe { libc::inotify_add_watch(fd.as_raw_fd(), path.as_ptr(), mask) })?;
        Ok(DirWatch { fd })
    }

    #[cfg(target_os = "macos")]
    pub fn new(dir: &Path) -> io::Result<DirWatch> {
        let path = cpath(dir)?;
        // SAFETY: plain syscalls; each descriptor is owned right away.
        let dirfd = unsafe {
            OwnedFd::from_raw_fd(check(libc::open(
                path.as_ptr(),
                libc::O_EVTONLY | libc::O_CLOEXEC,
            ))?)
        };
        let kq = unsafe { OwnedFd::from_raw_fd(check(libc::kqueue())?) };
        let ev = libc::kevent {
            ident: dirfd.as_raw_fd() as usize,
            filter: libc::EVFILT_VNODE,
            flags: libc::EV_ADD | libc::EV_CLEAR,
            fflags: libc::NOTE_WRITE | libc::NOTE_DELETE | libc::NOTE_RENAME,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: one valid change, no events out.
        check(unsafe {
            libc::kevent(
                kq.as_raw_fd(),
                &ev,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        })?;
        Ok(DirWatch {
            fd: kq,
            _dir: dirfd,
        })
    }

    /// Blocks until something in the folder changes.
    #[cfg(target_os = "linux")]
    pub fn wait(&self) -> io::Result<()> {
        let mut buf = [0u8; 4096];
        loop {
            // SAFETY: `buf` is writable for its length.
            let n = unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n > 0 {
                return Ok(());
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub fn wait(&self) -> io::Result<()> {
        loop {
            // SAFETY: an all-zero kevent is a valid out value.
            let mut ev: libc::kevent = unsafe { std::mem::zeroed() };
            // SAFETY: no changes in, one event out, no timeout.
            let n = unsafe {
                libc::kevent(
                    self.fd.as_raw_fd(),
                    std::ptr::null(),
                    0,
                    &mut ev,
                    1,
                    std::ptr::null(),
                )
            };
            if n > 0 {
                return Ok(());
            }
            let e = io::Error::last_os_error();
            if n < 0 && e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn wakes_when_a_file_appears() {
        let dir = std::env::temp_dir().join(format!("kot-dirwatch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let w = DirWatch::new(&dir).unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(w.wait().is_ok());
        });
        // Nothing happens: no wake.
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
        std::fs::write(dir.join("mpv.sock"), b"").unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(true));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_folder_is_an_error() {
        assert!(DirWatch::new(Path::new("/nonexistent/kotora-test")).is_err());
    }
}
