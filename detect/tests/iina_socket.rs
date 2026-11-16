//! IINA's mpv IPC socket, with a fake player on a temp socket (no real player involved).

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use kotora_detect::run::{Settings, start};
use kotora_detect::watch::Report;
use kotora_detect::{Led, Switch, Via};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kot-iina-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Serves one connection like IINA's mpv: setup lines in, the file's properties and the position out.
fn serve(listener: UnixListener) {
    let (conn, _) = listener.accept().unwrap();
    let mut w = conn.try_clone().unwrap();
    let mut lines = BufReader::new(conn).lines();
    for _ in 0..7 {
        lines.next().unwrap().unwrap();
    }
    for l in [
        r#"{"event":"property-change","id":1,"name":"path","data":"/media/Lanterns of Kirigaoka - 06.mkv"}"#,
        r#"{"event":"property-change","id":3,"name":"duration","data":1420.5}"#,
        r#"{"event":"property-change","id":4,"name":"pause","data":false}"#,
        r#"{"event":"property-change","id":6,"name":"idle-active","data":false}"#,
    ] {
        writeln!(w, "{l}").unwrap();
    }
    for l in lines.map_while(Result::ok) {
        if l.contains("time-pos") {
            writeln!(w, r#"{{"data":42.5,"request_id":7101,"error":"success"}}"#).unwrap();
        }
    }
}

fn settings(sockets: Vec<PathBuf>) -> Settings {
    Settings { mpv_sockets: sockets, processes: false, ..Settings::default() }
}

/// Waits for a report that `want` accepts.
fn wait_for<T>(rx: &mpsc::Receiver<Report>, mut want: impl FnMut(&Report) -> Option<T>) -> T {
    loop {
        let r = rx.recv_timeout(Duration::from_secs(5)).expect("the report did not come");
        if let Some(t) = want(&r) {
            return t;
        }
    }
}

fn plays_with_position(r: &Report) -> Option<(Switch, Via, Option<f64>)> {
    match r {
        Report::Now(Some(t)) if t.playing.position.is_some() => Some((t.playing.switch, t.playing.via, t.playing.position)),
        _ => None,
    }
}

#[test]
fn an_iina_socket_that_is_already_there_gives_the_position() {
    let dir = scratch("there");
    let sock = dir.join("iina.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    std::thread::spawn(move || serve(listener));
    let (tx, rx) = mpsc::channel();
    let _d = start(settings(vec![sock]), move |r| {
        let _ = tx.send(r);
    });
    assert_eq!(wait_for(&rx, plays_with_position), (Switch::Iina, Via::MpvIpc, Some(42.5)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_iina_socket_that_appears_later_is_found() {
    let dir = scratch("later");
    let sock = dir.join("iina.sock");
    let (tx, rx) = mpsc::channel();
    let _d = start(settings(vec![sock.clone()]), move |r| {
        let _ = tx.send(r);
    });
    wait_for(&rx, |r| matches!(r, Report::Sources(s) if s.iter().any(|s| s.switch == Switch::Iina && s.led == Led::Off)).then_some(()));
    // mpv binds the socket, and listens a moment later.
    let s2 = sock.clone();
    std::thread::spawn(move || serve(bind_slowly(&s2)));
    assert_eq!(wait_for(&rx, plays_with_position), (Switch::Iina, Via::MpvIpc, Some(42.5)));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `bind` and `listen` apart, as mpv does them: a connect in between is refused.
fn bind_slowly(sock: &Path) -> UnixListener {
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: plain syscalls on a descriptor we own; the address is filled in before `bind`.
    unsafe {
        let fd = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0);
        assert!(fd >= 0);
        let mut addr: libc::sockaddr_un = std::mem::zeroed();
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (d, b) in addr.sun_path.iter_mut().zip(sock.as_os_str().as_bytes()) {
            *d = *b as libc::c_char;
        }
        assert_eq!(libc::bind(fd, (&raw const addr).cast(), std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t), 0);
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(libc::listen(fd, 8), 0);
        UnixListener::from_raw_fd(fd)
    }
}

#[test]
fn a_socket_added_while_running_is_watched_at_once() {
    let dir = scratch("added");
    let sock = dir.join("custom-iina-ipc");
    let listener = UnixListener::bind(&sock).unwrap();
    std::thread::spawn(move || serve(listener));
    let (tx, rx) = mpsc::channel();
    let d = start(settings(Vec::new()), move |r| {
        let _ = tx.send(r);
    });
    assert!(!d.watches(&sock));
    assert!(d.watch_socket(&sock));
    assert!(d.watches(&sock) && !d.watch_socket(&sock), "once only");
    assert_eq!(wait_for(&rx, plays_with_position), (Switch::Iina, Via::MpvIpc, Some(42.5)));
    // New settings that name a socket start it too.
    let other = dir.join("mpv.sock");
    d.set(settings(vec![other.clone()]));
    assert!(d.watches(&other));
    let _ = std::fs::remove_dir_all(&dir);
}
