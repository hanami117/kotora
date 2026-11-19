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

use std::io::Write;
#[cfg(any(unix, test))]
use std::io::{BufRead, BufReader};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
#[cfg(windows)]
use std::sync::atomic::Ordering;
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
        Some(if self.playing() {
            p + (now - self.at).max(0.0) * self.speed
        } else {
            p
        })
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
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Step::default();
        };
        if v.get("request_id").and_then(Value::as_u64) == Some(TIME_REQ) {
            let pos = v.get("data").and_then(Value::as_f64);
            if pos.is_some() {
                self.position = pos;
                self.at = now;
            }
            return Step {
                changed: pos.is_some(),
                ask_time: false,
            };
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
                        Step {
                            changed: true,
                            ask_time: self.path.is_some(),
                        }
                    }
                    "media-title" => self.set(|m| &mut m.title, data.as_str().map(str::to_string)),
                    "duration" => self.set(|m| &mut m.duration, data.as_f64()),
                    "working-directory" => {
                        self.cwd = data.as_str().map(str::to_string);
                        Step {
                            changed: self.path.as_deref().is_some_and(|p| !crate::is_absolute(p)),
                            ask_time: false,
                        }
                    }
                    "pause" => {
                        let pause = data.as_bool().unwrap_or(false);
                        if pause == self.pause {
                            return Step::default();
                        }
                        self.re_anchor(now);
                        self.pause = pause;
                        Step {
                            changed: true,
                            ask_time: self.path.is_some(),
                        }
                    }
                    "speed" => {
                        let speed = data.as_f64().unwrap_or(1.0);
                        self.re_anchor(now);
                        self.speed = speed;
                        Step {
                            changed: true,
                            ask_time: false,
                        }
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
                        Step {
                            changed: true,
                            ask_time: !idle,
                        }
                    }
                    _ => Step::default(),
                }
            }
            // The position jumped, or playback (re)started: read it.
            Some("seek" | "playback-restart") => Step {
                changed: false,
                ask_time: true,
            },
            _ => Step::default(),
        }
    }

    fn set<T: PartialEq>(
        &mut self,
        field: impl Fn(&mut Model) -> &mut Option<T>,
        value: Option<T>,
    ) -> Step {
        let f = field(self);
        if *f == value {
            return Step::default();
        }
        *f = value;
        Step {
            changed: true,
            ask_time: false,
        }
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
            title: if local.is_none() && self.path.is_some() {
                self.title.clone()
            } else {
                None
            },
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
    if socket.to_string_lossy().to_lowercase().contains("iina") {
        Switch::Iina
    } else {
        Switch::Mpv
    }
}

/// The socket paths to watch when `config.toml` names none.
pub fn default_sockets() -> Vec<PathBuf> {
    if cfg!(windows) {
        // mpv's manual uses the first name in its example; the second is the other common one.
        vec![
            PathBuf::from(r"\\.\pipe\mpvsocket"),
            PathBuf::from(r"\\.\pipe\mpv"),
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            PathBuf::from("/tmp/mpv.sock"),
            PathBuf::from("/tmp/iina.sock"),
        ]
    } else {
        let run = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        vec![run.join("mpv.sock")]
    }
}

fn status(switch: Switch, line: String, led: Led) -> Input {
    Input::Status(SourceStatus {
        switch,
        via: Via::MpvIpc,
        line,
        led,
    })
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
            Err(e)
                if wait > Duration::from_millis(320)
                    || std::fs::symlink_metadata(socket).is_err() =>
            {
                return Err(e);
            }
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
pub fn run(socket: PathBuf, tx: Sender<Input>, _players_gone: Arc<AtomicU64>) {
    let switch = switch_for(&socket);
    let shown = socket.display().to_string();
    let dir = socket
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"));
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
                if tx
                    .send(status(switch, format!("{shown}: not running"), Led::Off))
                    .is_err()
                {
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
                let _ = tx.send(status(
                    switch,
                    format!("folder of {shown} not found"),
                    Led::Problem,
                ));
                return;
            }
        }
    }
}

/// How often the Windows thread looks for mpv's pipe while there is no mpv.
#[cfg(windows)]
const PIPE_POLL: Duration = Duration::from_secs(3);

/// Windows, while a pipe is connected and silent: how often the thread checks that the player
/// is still there.
#[cfg(windows)]
const PIPE_TICK_MS: u32 = 1000;

/// Windows: mpv's IPC is a named pipe (`input-ipc-server=\\.\pipe\mpvsocket`) with the same JSON
/// lines. A pipe has no folder to watch, so while there is no mpv the thread looks for the pipe
/// every [`PIPE_POLL`].
///
/// The player going away must never leave its last state on screen, so three things end a
/// connection (see [`pipe_session`]): the pipe says so (a failed or empty read, a failed
/// write), the pipe no longer answers a look at it, or the process scan saw the last mpv or
/// mpv.net process go (`players_gone` counts those moments).
#[cfg(windows)]
pub fn run(socket: PathBuf, tx: Sender<Input>, players_gone: Arc<AtomicU64>) {
    let switch = switch_for(&socket);
    let shown = socket.display().to_string();
    let mut said_off = false;
    loop {
        match pipe::Conn::open(&socket) {
            Ok(conn) => {
                let _ = tx.send(status(switch, format!("{shown}: running"), Led::Idle));
                let key = pipe_session(conn, &socket, switch, &tx, &players_gone);
                // The state light goes with the player, at once.
                said_off = true;
                if tx.send(Input::Gone(key)).is_err()
                    || tx
                        .send(status(switch, format!("{shown}: not running"), Led::Off))
                        .is_err()
                {
                    return;
                }
            }
            Err(_) if !said_off => {
                said_off = true;
                if tx
                    .send(status(switch, format!("{shown}: not running"), Led::Off))
                    .is_err()
                {
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
pub fn run(_socket: PathBuf, _tx: Sender<Input>, _players_gone: Arc<AtomicU64>) {}

/// One connection's state: mpv's lines in, hub inputs out.
struct Follow {
    model: Model,
    shown: String,
    switch: Switch,
    last_led: Led,
}

impl Follow {
    fn new(socket: &Path, switch: Switch) -> Follow {
        Follow {
            model: Model::new(socket, switch),
            shown: socket.display().to_string(),
            switch,
            last_led: Led::Idle,
        }
    }

    /// Asks mpv to report what Kotora follows. `false`: the connection is over.
    fn setup(&self, write: &mut impl Write) -> bool {
        SETUP.iter().all(|line| writeln!(write, "{line}").is_ok())
    }

    /// One line from mpv. `false`: the connection is over (a failed write, or the hub is gone).
    fn line(&mut self, line: &str, write: &mut impl Write, tx: &Sender<Input>) -> bool {
        let step = self.model.on_line(line, crate::clock());
        if step.ask_time && writeln!(write, "{ASK_TIME}").is_err() {
            return false;
        }
        if step.changed {
            let snap = self.model.snapshot();
            let led = if snap.state == PlayState::Playing {
                Led::Playing
            } else {
                Led::Idle
            };
            if led != self.last_led {
                self.last_led = led;
                let word = if led == Led::Playing {
                    "playing"
                } else {
                    "running"
                };
                let _ = tx.send(status(self.switch, format!("{}: {word}", self.shown), led));
            }
            if tx.send(Input::Update(snap)).is_err() {
                return false;
            }
        }
        true
    }
}

/// One connection, until mpv quits. Returns the player's key.
#[cfg(unix)]
fn session(
    read: impl std::io::Read,
    mut write: impl Write,
    socket: &Path,
    switch: Switch,
    tx: &Sender<Input>,
) -> String {
    let mut follow = Follow::new(socket, switch);
    let key = follow.model.key.clone();
    if !follow.setup(&mut write) {
        return key;
    }
    for line in BufReader::new(read).lines() {
        let Ok(line) = line else { break };
        if !follow.line(&line, &mut write, tx) {
            break;
        }
    }
    key
}

/// Windows: one connection on a named pipe, until the player is gone. Returns the player's key.
///
/// mpv says nothing while a file plays on, so a silent pipe is normal and a read may wait for
/// a long time. It must not wait past the player's end, though: a plain blocking read was seen
/// to keep waiting after mpv.net's window closed, which left the last episode on screen. So
/// the read waits at most [`PIPE_TICK_MS`], and each time it comes back empty the thread
/// checks the pipe itself and the process scan's count. One wake-up a second, and only while
/// a player is connected.
#[cfg(windows)]
fn pipe_session(
    mut conn: pipe::Conn,
    socket: &Path,
    switch: Switch,
    tx: &Sender<Input>,
    players_gone: &AtomicU64,
) -> String {
    let mut follow = Follow::new(socket, switch);
    let key = follow.model.key.clone();
    let gone_before = players_gone.load(Ordering::Relaxed);
    if !follow.setup(&mut conn) {
        return key;
    }
    let mut pending: Vec<u8> = Vec::new();
    loop {
        match conn.read(PIPE_TICK_MS) {
            pipe::Read::Data(bytes) => {
                pending.extend_from_slice(&bytes);
                while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                    let raw: Vec<u8> = pending.drain(..=end).collect();
                    let line = String::from_utf8_lossy(&raw[..end]);
                    if !follow.line(line.trim_end_matches('\r'), &mut conn, tx) {
                        return key;
                    }
                }
            }
            pipe::Read::Quiet => {
                if !conn.alive() || players_gone.load(Ordering::Relaxed) != gone_before {
                    return key;
                }
            }
            pipe::Read::Closed => return key,
        }
    }
}

/// A named pipe opened for overlapped I/O, so a read can wait with a time limit and the pipe
/// can be looked at while that read is still waiting.
#[cfg(windows)]
mod pipe {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_IO_PENDING, GetLastError, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, ReadFile, WriteFile};
    use windows_sys::Win32::System::IO::{CancelIo, GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;
    use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    pub enum Read {
        Data(Vec<u8>),
        /// Nothing came within the time given; the read is still waiting.
        Quiet,
        /// The other end is gone (a broken pipe, an empty read, any error).
        Closed,
    }

    pub struct Conn {
        file: std::fs::File,
        read_event: HANDLE,
        write_event: HANDLE,
        /// The waiting read's state and buffer: boxed, so they stay put while Windows uses them.
        read_state: Box<OVERLAPPED>,
        buf: Box<[u8; 4096]>,
        reading: bool,
    }

    // SAFETY: the events and the file belong to this value alone and are used by one thread
    // at a time; nothing in it is tied to the thread that made it.
    unsafe impl Send for Conn {}

    fn event() -> std::io::Result<HANDLE> {
        // SAFETY: a nameless manual-reset event; the handle is checked.
        let h = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if h.is_null() {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(h)
        }
    }

    impl Conn {
        pub fn open(path: &Path) -> std::io::Result<Conn> {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(FILE_FLAG_OVERLAPPED)
                .open(path)?;
            let read_event = event()?;
            let write_event = match event() {
                Ok(e) => e,
                Err(e) => {
                    // SAFETY: the event made just above.
                    unsafe { CloseHandle(read_event) };
                    return Err(e);
                }
            };
            // SAFETY: OVERLAPPED is plain data; all zero is its empty state.
            Ok(Conn {
                file,
                read_event,
                write_event,
                read_state: Box::new(unsafe { std::mem::zeroed() }),
                buf: Box::new([0; 4096]),
                reading: false,
            })
        }

        fn handle(&self) -> HANDLE {
            self.file.as_raw_handle() as HANDLE
        }

        /// Waits up to `timeout_ms` for bytes from the other end.
        pub fn read(&mut self, timeout_ms: u32) -> Read {
            // SAFETY: the buffer and the OVERLAPPED are boxed fields that live until the read
            // has finished or was cancelled (`Drop`); the lengths given are theirs.
            unsafe {
                if !self.reading {
                    *self.read_state = std::mem::zeroed();
                    self.read_state.hEvent = self.read_event;
                    if ReadFile(
                        self.handle(),
                        self.buf.as_mut_ptr(),
                        self.buf.len() as u32,
                        std::ptr::null_mut(),
                        &mut *self.read_state,
                    ) == 0
                        && GetLastError() != ERROR_IO_PENDING
                    {
                        return Read::Closed;
                    }
                    self.reading = true;
                }
                match WaitForSingleObject(self.read_event, timeout_ms) {
                    WAIT_OBJECT_0 => {
                        self.reading = false;
                        let mut n = 0u32;
                        if GetOverlappedResult(self.handle(), &*self.read_state, &mut n, 0) == 0
                            || n == 0
                        {
                            return Read::Closed;
                        }
                        Read::Data(self.buf[..n as usize].to_vec())
                    }
                    WAIT_TIMEOUT => Read::Quiet,
                    _ => Read::Closed,
                }
            }
        }

        /// Whether the other end still holds the pipe. Works while a read is waiting.
        pub fn alive(&self) -> bool {
            let mut waiting = 0u32;
            // SAFETY: only the count of waiting bytes is asked for.
            unsafe {
                PeekNamedPipe(
                    self.handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut waiting,
                    std::ptr::null_mut(),
                ) != 0
            }
        }
    }

    impl std::io::Write for Conn {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            // SAFETY: `state` and `data` outlive the write: a write that doesn't finish at
            // once is waited for before this returns.
            unsafe {
                let mut state: OVERLAPPED = std::mem::zeroed();
                state.hEvent = self.write_event;
                if WriteFile(
                    self.handle(),
                    data.as_ptr(),
                    data.len() as u32,
                    std::ptr::null_mut(),
                    &mut state,
                ) == 0
                    && GetLastError() != ERROR_IO_PENDING
                {
                    return Err(std::io::Error::last_os_error());
                }
                let mut n = 0u32;
                if GetOverlappedResult(self.handle(), &state, &mut n, 1) == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(n as usize)
            }
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Drop for Conn {
        fn drop(&mut self) {
            // SAFETY: a read that still waits is cancelled and waited for, so Windows no
            // longer uses the buffer; then the events are closed. The file closes after this.
            unsafe {
                if self.reading {
                    CancelIo(self.handle());
                    let mut n = 0u32;
                    GetOverlappedResult(self.handle(), &*self.read_state, &mut n, 1);
                }
                CloseHandle(self.read_event);
                CloseHandle(self.write_event);
            }
        }
    }
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
        assert!(
            steps[1].changed && steps[1].ask_time,
            "a new path asks for the position"
        );
        assert!(
            steps[8].ask_time && !steps[8].changed,
            "restart asks for the position"
        );
        let p = m.snapshot();
        assert_eq!(p.key, "mpv:/run/user/1000/mpv.sock");
        // mpv's folder and the file name, joined with the system's separator.
        let joined = Path::new("/media/anime/Lanterns")
            .join("[Hoshi] Lanterns of Kirigaoka - 06 (1080p).mkv");
        assert_eq!(p.path.as_deref(), joined.to_str());
        assert_eq!(p.title, None, "a local file goes by its name");
        assert_eq!(
            (p.position, p.duration, p.state, p.at),
            (Some(12.25), Some(1420.5), PlayState::Playing, 5.0)
        );
    }

    #[test]
    fn pause_keeps_the_position_by_the_clock() {
        let mut m = Model::new(Path::new("/tmp/mpv.sock"), Switch::Mpv);
        feed(&mut m, RECORDED, 0.0);
        // 100 s later the user pauses: the position is 112.25 even before mpv answers.
        let s = m.on_line(
            r#"{"event":"property-change","id":4,"name":"pause","data":true}"#,
            100.0,
        );
        assert!(s.changed && s.ask_time);
        let p = m.snapshot();
        assert_eq!(
            (p.state, p.position, p.at),
            (PlayState::Paused, Some(112.25), 100.0)
        );
        // A seek asks again, and the answer moves the anchor.
        assert!(m.on_line(r#"{"event":"seek"}"#, 101.0).ask_time);
        m.on_line(
            r#"{"request_id":7101,"error":"success","data":600.0}"#,
            101.5,
        );
        assert_eq!(
            (m.snapshot().position, m.snapshot().at),
            (Some(600.0), 101.5)
        );
    }

    #[test]
    fn idle_and_streams() {
        let mut m = Model::new(Path::new("/tmp/mpv.sock"), Switch::Mpv);
        feed(&mut m, RECORDED, 0.0);
        let s = m.on_line(
            r#"{"event":"property-change","id":6,"name":"idle-active","data":true}"#,
            1.0,
        );
        assert!(s.changed);
        assert_eq!(m.snapshot().state, PlayState::Stopped);
        assert_eq!(m.snapshot().path, None);
        // A stream: no local file, the title stays as a hint.
        m.on_line(
            r#"{"event":"property-change","id":6,"name":"idle-active","data":false}"#,
            2.0,
        );
        m.on_line(r#"{"event":"property-change","id":1,"name":"path","data":"https://example.invalid/v.m3u8"}"#, 2.0);
        m.on_line(
            r#"{"event":"property-change","id":2,"name":"media-title","data":"Some stream"}"#,
            2.0,
        );
        let p = m.snapshot();
        assert_eq!((p.path, p.title.as_deref()), (None, Some("Some stream")));
        // Junk and unknown events change nothing.
        assert_eq!(m.on_line("not json", 3.0), Step::default());
        assert_eq!(
            m.on_line(r#"{"event":"audio-reconfig"}"#, 3.0),
            Step::default()
        );
    }

    #[test]
    fn iina_sockets_are_iinas() {
        assert_eq!(switch_for(Path::new("/tmp/iina.sock")), Switch::Iina);
        assert_eq!(switch_for(Path::new("/tmp/mpv.sock")), Switch::Mpv);
        assert_eq!(
            Model::new(Path::new("/tmp/IINA-ipc"), Switch::Iina).player,
            "IINA"
        );
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
        std::thread::spawn(move || run(path, tx, Arc::default()));
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
        assert!(
            p.path
                .unwrap()
                .ends_with("Lanterns of Kirigaoka - 06 (1080p).mkv")
        );
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

    /// mpv open with nothing loaded, or between two files, is nothing playing: no file, so the
    /// hub has nothing to show for it.
    #[test]
    fn an_idle_mpv_plays_nothing() {
        let mut m = Model::new(Path::new("/tmp/mpv.sock"), Switch::Mpv);
        feed(&mut m, RECORDED, 0.0);
        assert!(m.snapshot().file().is_some());
        // The file ends and mpv stays open (`--idle`, mpv.net).
        let s = m.on_line(
            r#"{"event":"property-change","id":6,"name":"idle-active","data":true}"#,
            9.0,
        );
        assert!(s.changed);
        let p = m.snapshot();
        assert_eq!(
            (p.state, p.file(), p.position),
            (PlayState::Stopped, None, None)
        );
        // The path alone going away says the same (mpv sends it with no data on its way out).
        let mut m = Model::new(Path::new("/tmp/mpv.sock"), Switch::Mpv);
        feed(&mut m, RECORDED, 0.0);
        assert!(
            m.on_line(r#"{"event":"property-change","id":1,"name":"path"}"#, 9.0)
                .changed
        );
        assert_eq!(
            (m.snapshot().state, m.snapshot().file()),
            (PlayState::Stopped, None)
        );
        // And the hub then follows nothing.
        let mut w = crate::watch::Watch::new(0.8);
        let mut m = Model::new(Path::new("/tmp/mpv.sock"), Switch::Mpv);
        feed(&mut m, RECORDED, 0.0);
        w.update(m.snapshot(), 1.0);
        assert!(w.current(1.0).is_some());
        m.on_line(
            r#"{"event":"property-change","id":6,"name":"idle-active","data":true}"#,
            9.0,
        );
        let reports = w.update(m.snapshot(), 9.0);
        assert!(
            reports.contains(&crate::watch::Report::Now(None)),
            "{reports:?}"
        );
        assert!(w.current(9.0).is_none());
    }

    /// A fake mpv on a named pipe (Windows), for the tests below.
    #[cfg(windows)]
    mod fake {
        use super::super::*;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        use std::sync::mpsc::{self, Receiver};
        use std::time::Instant;
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
        use windows_sys::Win32::System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
            PIPE_TYPE_BYTE, PIPE_WAIT,
        };

        pub fn name(test: &str) -> PathBuf {
            PathBuf::from(format!(r"\\.\pipe\kot-mpv-{test}-{}", std::process::id()))
        }

        /// The player's end of the pipe, before anyone connected.
        pub fn serve(name: &Path) -> std::fs::File {
            let wide: Vec<u16> = name.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: a NUL-terminated name; the handle is owned by the File right away.
            unsafe {
                let h = CreateNamedPipeW(
                    wide.as_ptr(),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    1,
                    65536,
                    65536,
                    0,
                    std::ptr::null(),
                );
                assert_ne!(
                    h,
                    INVALID_HANDLE_VALUE,
                    "{}",
                    std::io::Error::last_os_error()
                );
                std::fs::File::from_raw_handle(h)
            }
        }

        /// Waits for Kotora, reads its setup lines and plays the recorded file.
        pub fn play(server: &std::fs::File) {
            // SAFETY: a valid pipe handle; a client that is already there counts as connected.
            unsafe { ConnectNamedPipe(server.as_raw_handle(), std::ptr::null_mut()) };
            let mut lines = BufReader::new(server.try_clone().unwrap()).lines();
            for want in SETUP {
                assert_eq!(lines.next().unwrap().unwrap(), want);
            }
            let mut w = server;
            for l in super::RECORDED {
                writeln!(w, "{l}").unwrap();
            }
        }

        /// Cuts the connection and keeps the handle (what a player does that closes its window
        /// and lingers).
        pub fn disconnect(server: &std::fs::File) {
            // SAFETY: a valid pipe handle.
            unsafe { DisconnectNamedPipe(server.as_raw_handle()) };
        }

        /// Kotora's thread on that pipe, and the count the process scan would move.
        pub fn kotora(name: &Path) -> (Receiver<Input>, Arc<AtomicU64>) {
            let (tx, rx) = mpsc::channel();
            let gone = Arc::new(AtomicU64::new(0));
            let (path, g) = (name.to_path_buf(), gone.clone());
            std::thread::spawn(move || run(path, tx, g));
            (rx, gone)
        }

        pub fn wait_playing(rx: &Receiver<Input>) -> Playing {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(10) {
                if let Ok(Input::Update(p)) = rx.recv_timeout(Duration::from_secs(1))
                    && p.position == Some(12.25)
                {
                    return p;
                }
            }
            panic!("no update with the position");
        }

        /// Waits for "the player is gone" and says how long it took.
        pub fn wait_gone(rx: &Receiver<Input>, name: &Path) -> Duration {
            let start = Instant::now();
            loop {
                match rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("the player was never reported gone")
                {
                    Input::Gone(k) => {
                        assert_eq!(k, format!("mpv:{}", name.display()));
                        return start.elapsed();
                    }
                    _ => continue,
                }
            }
        }
    }

    /// How long "gone" may take: one tick of the pipe check and some slack for a busy machine.
    #[cfg(windows)]
    const GONE_WITHIN: Duration = Duration::from_millis(2500);

    /// The player quits mid-session (its end of the pipe closes): gone at once, the state
    /// light with it, and a player that starts later on the same pipe is found again.
    #[test]
    #[cfg(windows)]
    fn a_player_that_quits_is_gone_and_the_next_one_is_found() {
        let name = fake::name("quit");
        let server = fake::serve(&name);
        let (rx, _) = fake::kotora(&name);
        fake::play(&server);
        let p = fake::wait_playing(&rx);
        assert!(
            p.path
                .unwrap()
                .ends_with("Lanterns of Kirigaoka - 06 (1080p).mkv")
        );
        assert_eq!(p.state, PlayState::Playing);
        drop(server);
        let took = fake::wait_gone(&rx, &name);
        assert!(took < GONE_WITHIN, "gone after {took:?}");
        let line = loop {
            if let Input::Status(s) = rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                break (s.line, s.led);
            }
        };
        assert_eq!(line, (format!("{}: not running", name.display()), Led::Off));
        // The player starts again: Kotora looks for the pipe every few seconds.
        let server = fake::serve(&name);
        fake::play(&server);
        fake::wait_playing(&rx);
        drop(server);
        fake::wait_gone(&rx, &name);
    }

    /// The player cuts the connection but keeps its end of the pipe: gone all the same.
    #[test]
    #[cfg(windows)]
    fn a_player_that_disconnects_is_gone() {
        let name = fake::name("disconnect");
        let server = fake::serve(&name);
        let (rx, _) = fake::kotora(&name);
        fake::play(&server);
        fake::wait_playing(&rx);
        fake::disconnect(&server);
        let took = fake::wait_gone(&rx, &name);
        assert!(took < GONE_WITHIN, "gone after {took:?}");
        drop(server);
    }

    /// The pipe stays open and silent (nothing tells the reader the player went), and the
    /// process scan sees the last mpv process go: gone within a tick. While the process is
    /// there, a silent pipe is a file playing on: nothing is reported.
    #[test]
    #[cfg(windows)]
    fn a_silent_pipe_ends_when_the_process_scan_says_the_player_went() {
        use std::sync::atomic::Ordering;
        let name = fake::name("process");
        let server = fake::serve(&name);
        let (rx, gone) = fake::kotora(&name);
        fake::play(&server);
        fake::wait_playing(&rx);
        // Three ticks of silence: still playing, nothing sent.
        let quiet = std::time::Instant::now();
        while quiet.elapsed() < Duration::from_secs(3) {
            if let Ok(i) = rx.recv_timeout(Duration::from_millis(200)) {
                assert!(
                    !matches!(i, Input::Gone(_)),
                    "a silent pipe was taken for a player that went"
                );
            }
        }
        gone.fetch_add(1, Ordering::Relaxed);
        let took = fake::wait_gone(&rx, &name);
        assert!(took < GONE_WITHIN, "gone after {took:?}");
        drop(server);
    }

    /// Kotora's end goes while its read still waits: the read is cancelled, nothing hangs or
    /// crashes, and the player sees its pipe close.
    #[test]
    #[cfg(windows)]
    fn dropping_a_connection_with_a_waiting_read_is_clean() {
        use std::io::Read;
        let name = fake::name("drop");
        let mut server = fake::serve(&name);
        let mut conn = pipe::Conn::open(&name).unwrap();
        assert!(matches!(conn.read(50), pipe::Read::Quiet));
        assert!(conn.alive());
        writeln!(conn, "hello").unwrap();
        let mut got = [0u8; 6];
        server.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"hello\n");
        writeln!(server, "one\ntwo").unwrap();
        let pipe::Read::Data(bytes) = conn.read(2000) else {
            panic!("no data")
        };
        assert_eq!(bytes, b"one\ntwo\n");
        assert!(matches!(conn.read(50), pipe::Read::Quiet));
        drop(conn);
        assert_eq!(server.read(&mut got).unwrap_or(0), 0, "the pipe closed");
    }
}
