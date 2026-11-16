//! mpv's JSON IPC (also IINA's, which is mpv inside). The user turns it on with
//! `input-ipc-server=PATH` in `mpv.conf` (IINA: Settings → Advanced → additional mpv options);
//! Kotora never touches the player's config.
//!
//! On connect, Kotora asks mpv to report the file, title, duration, pause, speed and idle state
//! (`observe_property`), and reads the position only when it jumps (seek, restart, pause). mpv
//! sends nothing while a file plays on, so the connection is quiet; the watch hub works the
//! position out from the clock.
//!
//! [`Model`] is the parsing, tested on recorded messages; [`run`] is the thread.

use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::Value;

#[cfg(unix)]
use crate::dirwatch::DirWatch;
use crate::run::Input;
use crate::{Led, PlayState, Playing, SourceStatus, Switch, Via};

/// The `request_id` of our position reads.
const TIME_REQ: u64 = 7101;

/// What Kotora sends on connect.
pub const SETUP: [&str; 7] = [
    r#"{"command":["observe_property",1,"path"]}"#,
    r#"{"command":["observe_property",2,"media-title"]}"#,
    r#"{"command":["observe_property",3,"duration"]}"#,
    r#"{"command":["observe_property",4,"pause"]}"#,
    r#"{"command":["observe_property",5,"speed"]}"#,
    r#"{"command":["observe_property",6,"idle-active"]}"#,
    r#"{"command":["observe_property",7,"working-directory"]}"#,
];

/// Asks for the position now.
pub const ASK_TIME: &str = r#"{"command":["get_property","time-pos"],"request_id":7101}"#;

/// One mpv's state, from its messages.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub key: String,
    pub player: String,
    pub switch: Switch,
    path: Option<String>,
    cwd: Option<String>,
    title: Option<String>,
    duration: Option<f64>,
    pause: bool,
    idle: bool,
    speed: f64,
    position: Option<f64>,
    at: f64,
}

/// What a message asks of the connection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step {
    /// The snapshot changed: report it.
    pub changed: bool,
    /// Read the position (send [`ASK_TIME`]).
    pub ask_time: bool,
}

impl Model {
    pub fn new(socket: &Path, switch: Switch) -> Model {
        Model {
            key: format!("mpv:{}", socket.display()),
            player: switch.label().to_string(),
            switch,
            path: None,
            cwd: None,
            title: None,
            duration: None,
            pause: false,
            idle: true,
            speed: 1.0,
            position: None,
            at: 0.0,
        }
    }

    fn position_now(&self, now: f64) -> Option<f64> {
        let p = self.position?;
        Some(if self.playing() { p + (now - self.at).max(0.0) * self.speed } else { p })
    }

    fn playing(&self) -> bool {
        !self.pause && !self.idle && self.path.is_some()
    }

    /// Keeps the position right across a change of pause or speed.
    fn re_anchor(&mut self, now: f64) {
        self.position = self.position_now(now);
        self.at = now;
    }

    /// One line from mpv.
    pub fn on_line(&mut self, line: &str, now: f64) -> Step {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return Step::default() };
        if v.get("request_id").and_then(Value::as_u64) == Some(TIME_REQ) {
            let pos = v.get("data").and_then(Value::as_f64);
            if pos.is_some() {
                self.position = pos;
                self.at = now;
            }
            return Step { changed: pos.is_some(), ask_time: false };
        }
        match v.get("event").and_then(Value::as_str) {
            Some("property-change") => {
                let data = v.get("data").unwrap_or(&Value::Null);
                match v.get("name").and_then(Value::as_str).unwrap_or_default() {
                    "path" => {
                        let p = data.as_str().map(str::to_string);
                        if p == self.path {
                            return Step::default();
                        }
                        self.path = p;
                        self.position = None;
                        self.duration = None;
                        self.title = None;
                        Step { changed: true, ask_time: self.path.is_some() }
                    }
                    "media-title" => self.set(|m| &mut m.title, data.as_str().map(str::to_string)),
                    "duration" => self.set(|m| &mut m.duration, data.as_f64()),
                    "working-directory" => {
                        self.cwd = data.as_str().map(str::to_string);
                        Step { changed: self.path.as_deref().is_some_and(|p| !crate::is_absolute(p)), ask_time: false }
                    }
                    "pause" => {
                        let pause = data.as_bool().unwrap_or(false);
                        if pause == self.pause {
                            return Step::default();
                        }
                        self.re_anchor(now);
                        self.pause = pause;
                        Step { changed: true, ask_time: self.path.is_some() }
                    }
                    "speed" => {
                        let speed = data.as_f64().unwrap_or(1.0);
                        self.re_anchor(now);
                        self.speed = speed;
                        Step { changed: true, ask_time: false }
                    }
                    "idle-active" => {
                        let idle = data.as_bool().unwrap_or(false);
                        if idle == self.idle {
                            return Step::default();
                        }
                        self.idle = idle;
                        if idle {
                            self.path = None;
                            self.position = None;
                        }
                        Step { changed: true, ask_time: !idle }
                    }
                    _ => Step::default(),
                }
            }
            // The position jumped, or playback (re)started: read it.
            Some("seek" | "playback-restart") => Step { changed: false, ask_time: true },
            _ => Step::default(),
        }
    }

    fn set<T: PartialEq>(&mut self, field: impl Fn(&mut Model) -> &mut Option<T>, value: Option<T>) -> Step {
        let f = field(self);
        if *f == value {
            return Step::default();
        }
        *f = value;
        Step { changed: true, ask_time: false }
    }

    /// The path as a local file: made absolute with mpv's folder, `None` for streams.
    fn local_path(&self) -> Option<String> {
        let p = self.path.as_deref()?;
        if let Some(local) = crate::file_url_path(p) {
            return Some(local);
        }
        if p.contains("://") {
            return None;
        }
        if crate::is_absolute(p) {
            return Some(p.to_string());
        }
        let cwd = self.cwd.as_deref()?;
        Some(Path::new(cwd).join(p).to_string_lossy().into_owned())
    }

    pub fn snapshot(&self) -> Playing {
        let state = if self.idle || self.path.is_none() {
            PlayState::Stopped
        } else if self.pause {
            PlayState::Paused
        } else {
            PlayState::Playing
        };
        // A stream (no local file) still has a title; a file shows its name, not the title.
        let local = self.local_path();
        Playing {
            key: self.key.clone(),
            player: self.player.clone(),
            switch: self.switch,
            via: Via::MpvIpc,
            title: if local.is_none() && self.path.is_some() { self.title.clone() } else { None },
            path: local,
            position: self.position,
            duration: self.duration,
            state,
            rate: self.speed,
            at: self.at,
        }
    }
}

/// The switch a socket belongs to: a path with "iina" in it is IINA's.
pub fn switch_for(socket: &Path) -> Switch {
    if socket.to_string_lossy().to_lowercase().contains("iina") { Switch::Iina } else { Switch::Mpv }
}

/// The socket paths to watch when `config.toml` names none.
pub fn default_sockets() -> Vec<PathBuf> {
    if cfg!(windows) {
        // mpv's manual uses the first name in its example; the second is the other common one.
        vec![PathBuf::from(r"\\.\pipe\mpvsocket"), PathBuf::from(r"\\.\pipe\mpv")]
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from("/tmp/mpv.sock"), PathBuf::from("/tmp/iina.sock")]
    } else {
        let run = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
        vec![run.join("mpv.sock")]
    }
}

fn status(switch: Switch, line: String, led: Led) -> Input {
    Input::Status(SourceStatus { switch, via: Via::MpvIpc, line, led })
}

/// Connects to the socket. The file shows up with `bind` and answers only after `listen`, so a
/// connect right after the folder woke us can be refused: when the file is there, try again a few
/// times (about 0.6 s in all, so a stale file costs little).
#[cfg(unix)]
fn connect(socket: &Path) -> std::io::Result<UnixStream> {
    let mut wait = Duration::from_millis(5);
    loop {
        match UnixStream::connect(socket) {
            Ok(s) => return Ok(s),
            Err(e) if wait > Duration::from_millis(320) || std::fs::symlink_metadata(socket).is_err() => return Err(e),
            Err(_) => {
                std::thread::sleep(wait);
                wait *= 2;
            }
        }
    }
}

/// The thread for one socket path: connect, follow, and when there is no mpv, sleep until the
/// folder changes. Ends only when the folder can't be watched.
#[cfg(unix)]
pub fn run(socket: PathBuf, tx: Sender<Input>) {
    let switch = switch_for(&socket);
    let shown = socket.display().to_string();
    let dir = socket.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("/"));
    let watch = DirWatch::new(&dir).ok();
    loop {
        match connect(&socket) {
            Ok(stream) => {
                let _ = tx.send(status(switch, format!("{shown}: running"), Led::Idle));
                let key = match stream.try_clone() {
                    Ok(read) => session(read, stream, &socket, switch, &tx),
                    Err(_) => Model::new(&socket, switch).key,
                };
                if tx.send(Input::Gone(key)).is_err() {
                    return;
                }
            }
            Err(_) => {
                if tx.send(status(switch, format!("{shown}: not running"), Led::Off)).is_err() {
                    return;
                }
            }
        }
        match &watch {
            // Sleeps in the kernel until something in the folder changes.
            Some(w) => {
                if w.wait().is_err() {
                    return;
                }
            }
            None => {
                let _ = tx.send(status(switch, format!("folder of {shown} not found"), Led::Problem));
                return;
            }
        }
    }
}

/// How often the Windows thread looks for mpv's pipe while there is no mpv.
#[cfg(windows)]
const PIPE_POLL: Duration = Duration::from_secs(3);

/// Windows: mpv's IPC is a named pipe (`input-ipc-server=\\.\pipe\mpvsocket`) with the same JSON
/// lines. A pipe has no folder to watch, so while there is no mpv the thread looks for the pipe
/// every [`PIPE_POLL`]. One blocking handle is enough: every write follows a read on this thread.
#[cfg(windows)]
pub fn run(socket: PathBuf, tx: Sender<Input>) {
    let switch = switch_for(&socket);
    let shown = socket.display().to_string();
    let mut said_off = false;
    loop {
        let pipe = std::fs::OpenOptions::new().read(true).write(true).open(&socket);
        match pipe.and_then(|write| Ok((write.try_clone()?, write))) {
            Ok((read, write)) => {
                said_off = false;
                let _ = tx.send(status(switch, format!("{shown}: running"), Led::Idle));
                let key = session(read, write, &socket, switch, &tx);
                if tx.send(Input::Gone(key)).is_err() {
                    return;
                }
            }
            Err(_) if !said_off => {
                said_off = true;
                if tx.send(status(switch, format!("{shown}: not running"), Led::Off)).is_err() {
                    return;
                }
            }
            Err(_) => {}
        }
        std::thread::sleep(PIPE_POLL);
    }
}

/// Neither Unix nor Windows: no mpv IPC.
#[cfg(not(any(unix, windows)))]
pub fn run(_socket: PathBuf, _tx: Sender<Input>) {}

/// One connection, until mpv quits. Returns the player's key.
fn session(read: impl std::io::Read, mut write: impl Write, socket: &Path, switch: Switch, tx: &Sender<Input>) -> String {
    let mut model = Model::new(socket, switch);
    let key = model.key.clone();
    for line in SETUP {
        if writeln!(write, "{line}").is_err() {
            return key;
        }
    }
    let shown = socket.display().to_string();
    let mut last_led = Led::Idle;
    for line in BufReader::new(read).lines() {
        let Ok(line) = line else { break };
        let step = model.on_line(&line, crate::clock());
        if step.ask_time && writeln!(write, "{ASK_TIME}").is_err() {
            break;
        }
        if step.changed {
            let snap = model.snapshot();
            let led = if snap.state == PlayState::Playing { Led::Playing } else { Led::Idle };
            if led != last_led {
                last_led = led;
                let word = if led == Led::Playing { "playing" } else { "running" };
                let _ = tx.send(status(switch, format!("{shown}: {word}"), led));
            }
            if tx.send(Input::Update(snap)).is_err() {
                break;
            }
        }
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Messages as mpv 0.39 sends them after the SETUP lines (recorded shape; made-up file).
    const RECORDED: &[&str] = &[
        r#"{"request_id":0,"error":"success"}"#,
        r#"{"event":"property-change","id":1,"name":"path","data":"[Hoshi] Lanterns of Kirigaoka - 06 (1080p).mkv"}"#,
        r#"{"event":"property-change","id":2,"name":"media-title","data":"Lanterns of Kirigaoka - 06"}"#,
        r#"{"event":"property-change","id":3,"name":"duration","data":1420.5}"#,
        r#"{"event":"property-change","id":4,"name":"pause","data":false}"#,
        r#"{"event":"property-change","id":5,"name":"speed","data":1.0}"#,
        r#"{"event":"property-change","id":6,"name":"idle-active","data":false}"#,
        r#"{"event":"property-change","id":7,"name":"working-directory","data":"/media/anime/Lanterns"}"#,
        r#"{"event":"playback-restart"}"#,
        r#"{"data":12.25,"request_id":7101,"error":"success"}"#,
    ];

    fn feed(m: &mut Model, lines: &[&str], now: f64) -> Vec<Step> {
        lines.iter().map(|l| m.on_line(l, now)).collect()
    }

    #[test]
    fn recorded_session_gives_a_playing_file() {
        let mut m = Model::new(Path::new("/run/user/1000/mpv.sock"), Switch::Mpv);
        let steps = feed(&mut m, RECORDED, 5.0);
        assert!(steps[1].changed && steps[1].ask_time, "a new path asks for the position");
        assert!(steps[8].ask_time && !steps[8].changed, "restart asks for the position");
        let p = m.snapshot();
        assert_eq!(p.key, "mpv:/run/user/1000/mpv.sock");
        // mpv's folder and the file name, joined with the system's separator.
        let joined = Path::new("/media/anime/Lanterns").join("[Hoshi] Lanterns of Kirigaoka - 06 (1080p).mkv");
        assert_eq!(p.path.as_deref(), joined.to_str());
        assert_eq!(p.title, None, "a local file goes by its name");
        assert_eq!((p.position, p.duration, p.state, p.at), (Some(12.25), Some(1420.5), PlayState::Playing, 5.0));
    }

    #[test]
    fn pause_keeps_the_position_by_the_clock() {
        let mut m = Model::new(Path::new("/tmp/mpv.sock"), Switch::Mpv);
        feed(&mut m, RECORDED, 0.0);
        // 100 s later the user pauses: the position is 112.25 even before mpv answers.
        let s = m.on_line(r#"{"event":"property-change","id":4,"name":"pause","data":true}"#, 100.0);
        assert!(s.changed && s.ask_time);
        let p = m.snapshot();
        assert_eq!((p.state, p.position, p.at), (PlayState::Paused, Some(112.25), 100.0));
        // A seek asks again, and the answer moves the anchor.
        assert!(m.on_line(r#"{"event":"seek"}"#, 101.0).ask_time);
        m.on_line(r#"{"request_id":7101,"error":"success","data":600.0}"#, 101.5);
        assert_eq!((m.snapshot().position, m.snapshot().at), (Some(600.0), 101.5));
    }

    #[test]
    fn idle_and_streams() {
        let mut m = Model::new(Path::new("/tmp/mpv.sock"), Switch::Mpv);
        feed(&mut m, RECORDED, 0.0);
        let s = m.on_line(r#"{"event":"property-change","id":6,"name":"idle-active","data":true}"#, 1.0);
        assert!(s.changed);
        assert_eq!(m.snapshot().state, PlayState::Stopped);
        assert_eq!(m.snapshot().path, None);
        // A stream: no local file, the title stays as a hint.
        m.on_line(r#"{"event":"property-change","id":6,"name":"idle-active","data":false}"#, 2.0);
        m.on_line(r#"{"event":"property-change","id":1,"name":"path","data":"https://example.invalid/v.m3u8"}"#, 2.0);
        m.on_line(r#"{"event":"property-change","id":2,"name":"media-title","data":"Some stream"}"#, 2.0);
        let p = m.snapshot();
        assert_eq!((p.path, p.title.as_deref()), (None, Some("Some stream")));
        // Junk and unknown events change nothing.
        assert_eq!(m.on_line("not json", 3.0), Step::default());
        assert_eq!(m.on_line(r#"{"event":"audio-reconfig"}"#, 3.0), Step::default());
    }

    #[test]
    fn iina_sockets_are_iinas() {
        assert_eq!(switch_for(Path::new("/tmp/iina.sock")), Switch::Iina);
        assert_eq!(switch_for(Path::new("/tmp/mpv.sock")), Switch::Mpv);
        assert_eq!(Model::new(Path::new("/tmp/IINA-ipc"), Switch::Iina).player, "IINA");
    }

    /// A fake mpv on a temp socket: the thread connects, sends the setup, and reports the file;
    /// when the fake quits, the player is gone.
    #[test]
    #[cfg(unix)]
    fn talks_to_a_fake_mpv_on_a_temp_socket() {
        use std::os::unix::net::UnixListener;
        use std::sync::mpsc;
        use std::time::Duration;
        let dir = std::env::temp_dir().join(format!("kot-mpv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("mpv.sock");
        let _ = std::fs::remove_file(&sock);
        let listener = UnixListener::bind(&sock).unwrap();
        let (tx, rx) = mpsc::channel();
        let path = sock.clone();
        std::thread::spawn(move || run(path, tx));
        let (conn, _) = listener.accept().unwrap();
        let mut lines = BufReader::new(conn.try_clone().unwrap()).lines();
        for want in SETUP {
            assert_eq!(lines.next().unwrap().unwrap(), want);
        }
        let mut w = conn;
        for l in RECORDED {
            writeln!(w, "{l}").unwrap();
        }
        let mut got = None;
        while let Ok(i) = rx.recv_timeout(Duration::from_secs(5)) {
            if let Input::Update(p) = i
                && p.position == Some(12.25)
            {
                got = Some(p);
                break;
            }
        }
        let p = got.expect("an update with the position");
        assert!(p.path.unwrap().ends_with("Lanterns of Kirigaoka - 06 (1080p).mkv"));
        drop(w);
        drop(lines);
        let gone = loop {
            match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                Input::Gone(k) => break k,
                _ => continue,
            }
        };
        assert_eq!(gone, format!("mpv:{}", sock.display()));
        drop(listener);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The same on Windows: a fake mpv on a named pipe.
    #[test]
    #[cfg(windows)]
    fn talks_to_a_fake_mpv_on_a_named_pipe() {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        use std::sync::mpsc;
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
        use windows_sys::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT};
        let name = PathBuf::from(format!(r"\\.\pipe\kot-mpv-test-{}", std::process::id()));
        let wide: Vec<u16> = name.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: a NUL-terminated name; the handle is owned by the File right away.
        let server = unsafe {
            let h = CreateNamedPipeW(wide.as_ptr(), PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT, 1, 65536, 65536, 0, std::ptr::null());
            assert_ne!(h, INVALID_HANDLE_VALUE);
            std::fs::File::from_raw_handle(h)
        };
        let (tx, rx) = mpsc::channel();
        let path = name.clone();
        std::thread::spawn(move || run(path, tx));
        // SAFETY: a valid pipe handle; a client that is already there counts as connected.
        unsafe { ConnectNamedPipe(server.as_raw_handle(), std::ptr::null_mut()) };
        let mut lines = BufReader::new(server.try_clone().unwrap()).lines();
        for want in SETUP {
            assert_eq!(lines.next().unwrap().unwrap(), want);
        }
        let mut w = server;
        for l in RECORDED {
            writeln!(w, "{l}").unwrap();
        }
        let mut got = None;
        while let Ok(i) = rx.recv_timeout(Duration::from_secs(5)) {
            if let Input::Update(p) = i
                && p.position == Some(12.25)
            {
                got = Some(p);
                break;
            }
        }
        let p = got.expect("an update with the position");
        assert!(p.path.unwrap().ends_with("Lanterns of Kirigaoka - 06 (1080p).mkv"));
        drop(w);
        drop(lines);
        let gone = loop {
            match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                Input::Gone(k) => break k,
                _ => continue,
            }
        };
        assert_eq!(gone, format!("mpv:{}", name.display()));
    }
}
