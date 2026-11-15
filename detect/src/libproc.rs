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
        LibProc { uid: unsafe { libc::getuid() } }
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
        let got = unsafe { libc::proc_listpids(PROC_UID_ONLY, self.uid, buf.as_mut_ptr().cast::<c_void>(), (cap * size_of::<c_int>()) as c_int) };
        if got <= 0 {
            return Vec::new();
        }
        buf.truncate((got as usize / size_of::<c_int>()).min(cap));
        buf.into_iter().filter(|p| *p > 0).map(|p| p as u32).collect()
    }

    fn name(&self, pid: u32) -> Option<String> {
        let mut b = [0u8; 256];
        // SAFETY: the buffer and its size match.
        let n = unsafe { libc::proc_name(pid as c_int, b.as_mut_ptr().cast::<c_void>(), b.len() as u32) };
        (n > 0).then(|| String::from_utf8_lossy(&b[..(n as usize).min(b.len())]).into_owned())
    }

    fn mine(&self, _pid: u32) -> bool {
        // `pids` lists only the user's own processes.
        true
    }

    fn files(&self, pid: u32) -> Vec<(i32, String)> {
        let pid = pid as c_int;
        let one = size_of::<libc::proc_fdinfo>();
        // SAFETY: a null buffer asks for the size in bytes.
        let need = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
        if need <= 0 {
            return Vec::new();
        }
        let cap = need as usize / one + 16;
        let mut fds = vec![libc::proc_fdinfo { proc_fd: 0, proc_fdtype: 0 }; cap];
        // SAFETY: the buffer holds `cap` entries and its size in bytes is passed.
        let got = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, fds.as_mut_ptr().cast::<c_void>(), (cap * one) as c_int) };
        if got <= 0 {
            return Vec::new();
        }
        fds.truncate((got as usize / one).min(cap));
        let mut out = Vec::new();
        for fd in fds.iter().filter(|f| f.proc_fdtype == libc::PROX_FDTYPE_VNODE as u32) {
            // SAFETY: plain C data; all zero bits is a valid value.
            let mut info: VnodeFdInfoWithPath = unsafe { std::mem::zeroed() };
            let size = size_of::<VnodeFdInfoWithPath>();
            // SAFETY: `info` is `size` bytes, the layout of `struct vnode_fdinfowithpath`.
            let n = unsafe { libc::proc_pidfdinfo(pid, fd.proc_fd, PROC_PIDFDVNODEPATHINFO, (&mut info as *mut VnodeFdInfoWithPath).cast::<c_void>(), size as c_int) };
            if n as usize != size {
                continue;
            }
            let bytes: Vec<u8> = info.pvip.vip_path.iter().flatten().map(|c| *c as u8).take_while(|b| *b != 0).collect();
            if let Ok(path) = String::from_utf8(bytes)
                && !path.is_empty()
            {
                out.push((fd.proc_fd, path));
            }
        }
        out
    }
}
