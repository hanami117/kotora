//! Discord Rich Presence over the Discord app's local IPC: no network of our own, no crate.
//!
//! The Discord app on the same computer listens on a Unix socket (`discord-ipc-0` … `9`, in
//! `$XDG_RUNTIME_DIR`, `$TMPDIR`, or a Flatpak or Snap folder under the runtime dir) or, on
//! Windows, a named pipe (`\\.\pipe\discord-ipc-0` … `9`). A message is a frame: opcode (u32
//! little endian), length (u32 little endian), JSON. We send a handshake (opcode 0), wait for
//! `READY`, then `SET_ACTIVITY` (opcode 1). Pings (opcode 3) are answered with pongs (opcode 4).
//!
//! [`Worker`] is the one thread, started only while the user turns the switch on. It keeps the
//! latest wanted [`Activity`], connects when Discord is there, retries quietly every
//! [`RETRY`] when it isn't, sends at most one update per [`MIN_GAP`] (Discord rate-limits
//! activity updates), and clears the status when told to or at [`Worker::stop`].

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// How often the worker looks for Discord again when it isn't running.
pub const RETRY: Duration = Duration::from_secs(30);
/// Discord allows about one activity update per 15 seconds.
pub const MIN_GAP: Duration = Duration::from_secs(15);

/// The Discord Application ID built in (a placeholder). TODO(release): create the Application at
/// discord.com/developers, upload the assets named in `docs/DISCORD.md`, paste its ID here.
/// `[discord] client_id` in `config.toml` wins over it.
pub const DEFAULT_CLIENT_ID: &str = "1572355618591871087";

/// The asset key of Kotora's icon, uploaded to the Application (Rich Presence → Art Assets).
pub const ICON_ASSET: &str = "kotora";

/// The id the user's `config.toml` or the built-in one gives: digits only, else the built-in.
pub fn client_id(configured: &str) -> String {
    let c = configured.trim();
    if !c.is_empty() && c.bytes().all(|b| b.is_ascii_digit()) { c.to_string() } else { DEFAULT_CLIENT_ID.to_string() }
}

/// Whether the built-in id is still the placeholder (nothing to show until a real one is set).
pub fn is_placeholder(id: &str) -> bool {
    id.is_empty() || id.bytes().all(|b| b == b'0')
}

/// What the status shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    /// Discord's activity type: 0 Playing (Reading uses it: there is no reading type), 3 Watching.
    pub kind: u8,
    /// The first line (the title).
    pub details: String,
    /// The second line ("Episode 7 of 24").
    pub state: Option<String>,
    /// Unix seconds. With both, Discord draws a progress bar; with the start only, elapsed time.
    pub start: Option<i64>,
    pub end: Option<i64>,
    /// The large image: a key of the Application's assets or an https URL, with its hover text.
    pub large: Option<(String, String)>,
    pub small: Option<(String, String)>,
    /// Discord's `status_display_type`: what follows the activity icon in the member list and
    /// the mini profile. 0 the application's name, 1 `state`, 2 `details`.
    pub status_display: u8,
    /// One button: label and https URL.
    pub button: Option<(String, String)>,
}

/// Discord wants 2 to 128 characters per line.
fn line(s: &str) -> String {
    let mut t: String = s.trim().chars().take(128).collect();
    if s.trim().chars().count() > 128 {
        t.pop();
        t.push('\u{2026}');
    }
    // One character is refused: a zero width space makes it two.
    if t.chars().count() < 2 {
        t.push('\u{200b}');
        if t.chars().count() < 2 {
            t = "\u{200b}\u{200b}".into();
        }
    }
    t
}

impl Activity {
    /// The `activity` object of `SET_ACTIVITY`.
    pub fn to_json(&self) -> Value {
        let mut a = json!({ "type": self.kind, "details": line(&self.details) });
        // The line has to exist: no second line, no showing it (the first one then).
        let shown = match self.status_display {
            1 if self.state.is_none() => 2,
            n => n.min(2),
        };
        a["status_display_type"] = json!(shown);
        if let Some(s) = &self.state {
            a["state"] = json!(line(s));
        }
        let mut ts = serde_json::Map::new();
        if let Some(s) = self.start {
            ts.insert("start".into(), json!(s));
            if let Some(e) = self.end.filter(|e| *e > s) {
                ts.insert("end".into(), json!(e));
            }
        }
        if !ts.is_empty() {
            a["timestamps"] = Value::Object(ts);
        }
        let mut assets = serde_json::Map::new();
        // An image URL over 256 characters is refused: the icon is shown instead.
        if let Some((k, t)) = self.large.as_ref().filter(|(k, _)| k.len() <= 256) {
            assets.insert("large_image".into(), json!(k));
            assets.insert("large_text".into(), json!(line(t)));
        }
        if let Some((k, t)) = &self.small {
            assets.insert("small_image".into(), json!(k));
            assets.insert("small_text".into(), json!(line(t)));
        }
        if !assets.is_empty() {
            a["assets"] = Value::Object(assets);
        }
        if let Some((label, url)) = self.button.as_ref().filter(|(_, u)| u.starts_with("https://") && u.len() <= 512) {
            a["buttons"] = json!([{ "label": label.chars().take(32).collect::<String>(), "url": url }]);
        }
        a
    }

    /// The same status, give or take `tol` seconds on the timestamps (a playing file's start
    /// moves by a fraction of a second between reports).
    pub fn same_as(&self, o: &Activity, tol: i64) -> bool {
        let near = |a: Option<i64>, b: Option<i64>| match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() <= tol,
            (None, None) => true,
            _ => false,
        };
        Activity { start: None, end: None, ..self.clone() } == Activity { start: None, end: None, ..o.clone() } && near(self.start, o.start) && near(self.end, o.end)
    }
}

/// One frame.
pub fn frame(op: u32, body: &Value) -> Vec<u8> {
    let json = body.to_string();
    let mut v = Vec::with_capacity(8 + json.len());
    v.extend_from_slice(&op.to_le_bytes());
    v.extend_from_slice(&(json.len() as u32).to_le_bytes());
    v.extend_from_slice(json.as_bytes());
    v
}

/// Reads one frame: opcode and JSON (`Null` for a body that isn't JSON).
pub fn read_frame(r: &mut impl Read) -> io::Result<(u32, Value)> {
    let mut h = [0u8; 8];
    r.read_exact(&mut h)?;
    let op = u32::from_le_bytes(h[..4].try_into().unwrap());
    let len = u32::from_le_bytes(h[4..].try_into().unwrap()) as usize;
    // Discord's frames are small; anything big is not Discord.
    if len > 64 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too big"));
    }
    let mut b = vec![0u8; len];
    r.read_exact(&mut b)?;
    Ok((op, serde_json::from_slice(&b).unwrap_or(Value::Null)))
}

/// A connection to Discord's IPC.
pub trait Stream: Read + Write + Send {
    /// How long a read may wait (not at all possible on a Windows pipe: `None` there).
    fn set_read_timeout(&mut self, _d: Option<Duration>) {}
}

#[cfg(unix)]
impl Stream for std::os::unix::net::UnixStream {
    fn set_read_timeout(&mut self, d: Option<Duration>) {
        let _ = std::os::unix::net::UnixStream::set_read_timeout(self, d);
    }
}

#[cfg(windows)]
impl Stream for std::fs::File {}

/// How the worker finds Discord: each call is one try.
pub type Connector = Box<dyn FnMut() -> io::Result<Box<dyn Stream>> + Send>;

/// Where Discord's socket may be, in the order to try. `env` reads an environment variable.
pub fn socket_candidates(env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut add = |d: PathBuf| {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    };
    let runtime = env("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()).map(PathBuf::from);
    for k in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
        if let Some(v) = env(k).filter(|v| !v.is_empty()) {
            add(PathBuf::from(v));
        }
    }
    add(PathBuf::from("/tmp"));
    let mut out = Vec::new();
    for d in &dirs {
        for n in 0..10 {
            out.push(d.join(format!("discord-ipc-{n}")));
        }
    }
    // Flatpak and Snap keep the socket in their own folders under the runtime dir.
    if let Some(r) = runtime {
        for sub in ["app/com.discordapp.Discord", "app/com.discordapp.DiscordCanary", "app/dev.vencord.Vesktop", "snap.discord", "snap.discord-canary"] {
            for n in 0..10 {
                out.push(r.join(sub).join(format!("discord-ipc-{n}")));
            }
        }
    }
    out
}

/// The real connector: the first socket (or pipe) that answers.
pub fn system_connector() -> Connector {
    Box::new(|| {
        #[cfg(unix)]
        {
            let paths = socket_candidates(&|k| std::env::var(k).ok());
            for p in paths {
                if let Ok(s) = std::os::unix::net::UnixStream::connect(&p) {
                    return Ok(Box::new(s) as Box<dyn Stream>);
                }
            }
        }
        #[cfg(windows)]
        {
            for n in 0..10 {
                if let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{n}")) {
                    return Ok(Box::new(f) as Box<dyn Stream>);
                }
            }
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "Discord is not running"))
    })
}

/// A live, handshaken connection.
struct Conn {
    s: Box<dyn Stream>,
    nonce: u64,
}

impl Conn {
    fn open(mut s: Box<dyn Stream>, client_id: &str) -> io::Result<Conn> {
        s.set_read_timeout(Some(Duration::from_secs(5)));
        s.write_all(&frame(0, &json!({ "v": 1, "client_id": client_id })))?;
        loop {
            let (op, v) = read_frame(&mut s)?;
            match op {
                // A frame: READY, or an ERROR event (a bad Application ID).
                1 if v["evt"] == "READY" => break,
                1 if v["evt"] == "ERROR" => return Err(io::Error::other("Discord refused the Application ID")),
                // Close.
                2 => return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "Discord closed the connection")),
                3 => s.write_all(&frame(4, &v))?,
                _ => {}
            }
        }
        Ok(Conn { s, nonce: 0 })
    }

    /// `SET_ACTIVITY`; `None` clears the status. Reads Discord's answer so its side never fills.
    fn set(&mut self, a: Option<&Activity>) -> io::Result<()> {
        self.nonce += 1;
        let nonce = self.nonce.to_string();
        let body = json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": std::process::id(), "activity": a.map(Activity::to_json) },
            "nonce": nonce,
        });
        self.s.write_all(&frame(1, &body))?;
        loop {
            let (op, v) = read_frame(&mut self.s)?;
            match op {
                1 if v["nonce"] == nonce.as_str() => return Ok(()),
                2 => return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "Discord closed the connection")),
                3 => self.s.write_all(&frame(4, &v))?,
                _ => {}
            }
        }
    }

    fn close(mut self) {
        let _ = self.s.write_all(&frame(2, &json!({})));
    }
}

enum Cmd {
    /// The status wanted now (`None`: clear it).
    Set(Option<Box<Activity>>),
    Stop,
}

/// The one thread. Dropping it without [`Worker::stop`] stops it too, without waiting.
pub struct Worker {
    tx: Sender<Cmd>,
    done: Receiver<()>,
    thread: Option<JoinHandle<()>>,
}

/// How a worker is tuned (tests make it quick).
#[derive(Clone, Copy)]
pub struct Timing {
    pub retry: Duration,
    pub min_gap: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing { retry: RETRY, min_gap: MIN_GAP }
    }
}

impl Worker {
    pub fn start(client_id: String, connect: Connector, timing: Timing) -> Worker {
        let (tx, rx) = mpsc::channel();
        let (dtx, done) = mpsc::channel();
        let thread = std::thread::Builder::new().name("kotora-discord".into()).spawn(move || {
            run(&client_id, connect, timing, &rx);
            let _ = dtx.send(());
        });
        Worker { tx, done, thread: thread.ok() }
    }

    /// The status to show (`None`: nothing). Never blocks.
    pub fn set(&self, a: Option<Activity>) {
        let _ = self.tx.send(Cmd::Set(a.map(Box::new)));
    }

    /// Clears the status and says goodbye, waiting at most `wait` (Discord is a local socket;
    /// a stuck one must not hold Kotora's quit).
    pub fn stop(mut self, wait: Duration) {
        let _ = self.tx.send(Cmd::Stop);
        if self.done.recv_timeout(wait).is_ok()
            && let Some(t) = self.thread.take()
        {
            let _ = t.join();
        }
    }
}

fn run(client_id: &str, mut connect: Connector, t: Timing, rx: &Receiver<Cmd>) {
    let mut conn: Option<Conn> = None;
    // What Discord shows now (to skip a repeat), and what is wanted.
    let mut shown: Option<Activity> = None;
    let mut want: Option<Activity> = None;
    let mut last_set: Option<Instant> = None;
    let mut next_try = Instant::now();
    loop {
        // When there is something to do: connect, send the wanted status, or nothing.
        let dirty = !same(&shown, &want);
        let now = Instant::now();
        let mut wait = Duration::from_secs(3600);
        if conn.is_none() && (dirty || want.is_some()) {
            if now >= next_try {
                match connect().and_then(|s| Conn::open(s, client_id)) {
                    Ok(c) => {
                        conn = Some(c);
                        // A new connection starts with no status.
                        shown = None;
                        continue;
                    }
                    // Quiet: Discord isn't running (or isn't ready); look again later.
                    Err(_) => next_try = now + t.retry,
                }
            }
            wait = wait.min(next_try.saturating_duration_since(Instant::now()));
        }
        if let Some(c) = conn.as_mut().filter(|_| dirty) {
            // A clear is never held back; an update waits for the rate limit.
            let ready = want.is_none() || last_set.is_none_or(|l| now.duration_since(l) >= t.min_gap);
            if ready {
                match c.set(want.as_ref()) {
                    Ok(()) => {
                        shown = want.clone();
                        last_set = Some(Instant::now());
                    }
                    Err(_) => {
                        conn = None;
                        next_try = Instant::now() + t.retry.min(Duration::from_secs(2));
                    }
                }
                continue;
            }
            wait = wait.min(t.min_gap.saturating_sub(now.duration_since(last_set.unwrap_or(now))));
        }
        match rx.recv_timeout(wait.max(Duration::from_millis(1))) {
            Ok(Cmd::Set(a)) => {
                want = a.map(|a| *a);
                // Drain: only the latest matters.
                while let Ok(m) = rx.try_recv() {
                    match m {
                        Cmd::Set(a) => want = a.map(|a| *a),
                        Cmd::Stop => return finish(conn, shown.is_some()),
                    }
                }
            }
            Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => return finish(conn, shown.is_some()),
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// Clear a status that is showing, then close.
fn finish(conn: Option<Conn>, showing: bool) {
    if let Some(mut c) = conn {
        if showing {
            let _ = c.set(None);
        }
        c.close();
    }
}

fn same(a: &Option<Activity>, b: &Option<Activity>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.same_as(b, 3),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(title: &str) -> Activity {
        Activity { kind: 3, details: title.into(), state: Some("Episode 7 of 24".into()), start: Some(1000), end: Some(2400), large: Some(("https://cdn.example/a.jpg".into(), title.into())), small: Some((ICON_ASSET.into(), "Kotora".into())), status_display: 2, button: Some(("View on AniList".into(), "https://anilist.co/anime/1".into())) }
    }

    #[test]
    fn the_activity_json_has_every_part() {
        let j = act("Kagurabachi").to_json();
        assert_eq!(j["type"], 3);
        assert_eq!(j["details"], "Kagurabachi");
        assert_eq!(j["state"], "Episode 7 of 24");
        assert_eq!(j["timestamps"], json!({ "start": 1000, "end": 2400 }));
        assert_eq!(j["assets"]["large_image"], "https://cdn.example/a.jpg");
        assert_eq!(j["assets"]["small_image"], "kotora");
        assert_eq!(j["buttons"][0]["label"], "View on AniList");
        assert_eq!(j["status_display_type"], 2);
    }

    #[test]
    fn the_status_line_follows_the_choice_and_needs_its_line() {
        let mut a = act("T");
        for (want, shown) in [(0u8, 0), (1, 1), (2, 2), (9, 2)] {
            a.status_display = want;
            assert_eq!(a.to_json()["status_display_type"], shown);
        }
        a.status_display = 1;
        a.state = None;
        assert_eq!(a.to_json()["status_display_type"], 2, "no second line: the first");
    }

    #[test]
    fn lines_are_kept_to_discords_limits() {
        assert_eq!(line("K").chars().count(), 2);
        assert_eq!(line("").chars().count(), 2);
        let long = "x".repeat(300);
        assert_eq!(line(&long).chars().count(), 128);
        let mut a = act("t");
        a.large = Some(("https://x/".to_string() + &"a".repeat(300), "t".into()));
        a.button = Some(("b".into(), "http://insecure.example".into()));
        a.end = Some(5);
        let j = a.to_json();
        assert!(j["assets"].get("large_image").is_none(), "a URL over 256 characters is dropped");
        assert!(j.get("buttons").is_none(), "http is refused");
        assert_eq!(j["timestamps"], json!({ "start": 1000 }), "an end before the start is dropped");
    }

    #[test]
    fn same_status_within_a_few_seconds() {
        let a = act("T");
        let mut b = a.clone();
        b.start = Some(1002);
        b.end = Some(2401);
        assert!(a.same_as(&b, 3));
        b.start = Some(1010);
        assert!(!a.same_as(&b, 3), "a seek");
        let mut c = a.clone();
        c.state = Some("Episode 8 of 24".into());
        assert!(!a.same_as(&c, 3));
    }

    #[test]
    fn client_ids() {
        assert_eq!(client_id(" 123456789012345678 "), "123456789012345678");
        assert_eq!(client_id("abc"), DEFAULT_CLIENT_ID);
        assert_eq!(client_id(""), DEFAULT_CLIENT_ID);
        assert!(is_placeholder("0") && is_placeholder("") && !is_placeholder("12"));
    }

    #[test]
    fn socket_places() {
        let env = |k: &str| match k {
            "XDG_RUNTIME_DIR" => Some("/run/user/1000".to_string()),
            "TMPDIR" => Some("/var/tmp/x".to_string()),
            _ => None,
        };
        let c = socket_candidates(&env);
        assert_eq!(c[0], PathBuf::from("/run/user/1000/discord-ipc-0"));
        assert!(c.contains(&PathBuf::from("/var/tmp/x/discord-ipc-3")));
        assert!(c.contains(&PathBuf::from("/tmp/discord-ipc-9")));
        assert!(c.contains(&PathBuf::from("/run/user/1000/app/com.discordapp.Discord/discord-ipc-0")), "Flatpak");
        assert!(c.contains(&PathBuf::from("/run/user/1000/snap.discord/discord-ipc-1")), "Snap");
        let none = socket_candidates(&|_| None);
        assert_eq!(none.len(), 10, "only /tmp without a runtime dir");
    }

    #[test]
    fn frames_round_trip() {
        let f = frame(1, &json!({ "a": 1 }));
        assert_eq!(&f[..4], &1u32.to_le_bytes());
        let (op, v) = read_frame(&mut &f[..]).unwrap();
        assert_eq!((op, v["a"].as_i64()), (1, Some(1)));
        let mut huge = vec![1, 0, 0, 0];
        huge.extend_from_slice(&(10_000_000u32).to_le_bytes());
        assert!(read_frame(&mut &huge[..]).is_err());
    }

    /// A fake Discord: a socket in a temp folder that records the frames it gets.
    #[cfg(unix)]
    mod fake {
        use super::*;
        use std::os::unix::net::UnixListener;
        use std::sync::{Arc, Mutex};

        pub struct Server {
            pub path: PathBuf,
            pub got: Arc<Mutex<Vec<(u32, Value)>>>,
            pub accepted: Arc<Mutex<u32>>,
            /// Closes the connection after this many SET_ACTIVITY frames (0: never).
            pub drop_after: Arc<Mutex<u32>>,
        }

        pub fn serve(name: &str) -> Server {
            let dir = std::env::temp_dir().join(format!("kotora-discord-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("discord-ipc-0");
            let l = UnixListener::bind(&path).unwrap();
            let got = Arc::new(Mutex::new(Vec::new()));
            let accepted = Arc::new(Mutex::new(0));
            let drop_after = Arc::new(Mutex::new(0u32));
            let (g, a, d) = (got.clone(), accepted.clone(), drop_after.clone());
            std::thread::spawn(move || {
                for s in l.incoming() {
                    let Ok(mut s) = s else { break };
                    *a.lock().unwrap() += 1;
                    let (g, d) = (g.clone(), d.clone());
                    std::thread::spawn(move || {
                        let mut sets = 0;
                        while let Ok((op, v)) = read_frame(&mut s) {
                            g.lock().unwrap().push((op, v.clone()));
                            match op {
                                0 => {
                                    s.write_all(&frame(1, &json!({ "cmd": "DISPATCH", "evt": "READY", "data": {} }))).unwrap();
                                    // A ping first, to see it is answered.
                                    s.write_all(&frame(3, &json!({ "n": 1 }))).unwrap();
                                }
                                1 => {
                                    sets += 1;
                                    s.write_all(&frame(1, &json!({ "cmd": "SET_ACTIVITY", "evt": null, "nonce": v["nonce"] }))).unwrap();
                                    let limit = *d.lock().unwrap();
                                    if limit > 0 && sets >= limit {
                                        return;
                                    }
                                }
                                2 => return,
                                _ => {}
                            }
                        }
                    });
                }
            });
            Server { path, got, accepted, drop_after }
        }

        pub fn connector(path: PathBuf) -> Connector {
            Box::new(move || Ok(Box::new(std::os::unix::net::UnixStream::connect(&path)?) as Box<dyn Stream>))
        }

        pub fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
            for _ in 0..400 {
                if f() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("timed out waiting for {what}");
        }
    }

    #[cfg(unix)]
    fn quick() -> Timing {
        Timing { retry: Duration::from_millis(60), min_gap: Duration::from_millis(40) }
    }

    #[cfg(unix)]
    fn sets(s: &fake::Server) -> Vec<Value> {
        s.got.lock().unwrap().iter().filter(|(op, v)| *op == 1 && v["cmd"] == "SET_ACTIVITY").map(|(_, v)| v.clone()).collect()
    }

    #[cfg(unix)]
    #[test]
    fn handshake_set_clear_and_stop() {
        let s = fake::serve("basic");
        let w = Worker::start("123".into(), fake::connector(s.path.clone()), quick());
        w.set(Some(act("Kagurabachi")));
        fake::wait_for("the first activity", || sets(&s).len() == 1);
        {
            let got = s.got.lock().unwrap();
            assert_eq!(got[0].0, 0, "the handshake first");
            assert_eq!(got[0].1, json!({ "v": 1, "client_id": "123" }));
            assert_eq!(got[1].0, 1);
            assert_eq!(got[1].1["args"]["activity"]["details"], "Kagurabachi");
            assert_eq!(got[1].1["args"]["pid"], std::process::id());
            assert!(got.iter().any(|(op, v)| *op == 4 && v["n"] == 1), "the ping was answered");
        }
        // The same status again sends nothing; a changed one does.
        w.set(Some(act("Kagurabachi")));
        w.set(Some(act("Blue Lock")));
        fake::wait_for("the second activity", || sets(&s).len() == 2);
        assert_eq!(sets(&s)[1]["args"]["activity"]["details"], "Blue Lock");
        w.set(None);
        fake::wait_for("the clear", || sets(&s).len() == 3);
        assert!(sets(&s)[2]["args"]["activity"].is_null());
        w.set(Some(act("Frieren")));
        fake::wait_for("the fourth", || sets(&s).len() == 4);
        // Stop clears a status that shows, then closes.
        w.stop(Duration::from_secs(2));
        assert_eq!(sets(&s).len(), 5);
        assert!(sets(&s)[4]["args"]["activity"].is_null());
        assert!(s.got.lock().unwrap().iter().any(|(op, _)| *op == 2), "a close frame");
        assert_eq!(*s.accepted.lock().unwrap(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn updates_wait_for_the_rate_limit() {
        let s = fake::serve("gap");
        let t = Timing { retry: Duration::from_millis(60), min_gap: Duration::from_millis(300) };
        let w = Worker::start("1".into(), fake::connector(s.path.clone()), t);
        w.set(Some(act("Title A")));
        fake::wait_for("first", || sets(&s).len() == 1);
        let t0 = Instant::now();
        w.set(Some(act("Title B")));
        w.set(Some(act("Title C")));
        fake::wait_for("the latest", || sets(&s).len() == 2);
        assert!(t0.elapsed() >= Duration::from_millis(200), "held back");
        assert_eq!(sets(&s)[1]["args"]["activity"]["details"], "Title C", "B was skipped");
        w.stop(Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn it_waits_for_discord_and_sends_what_was_wanted() {
        // Nothing listens yet: the worker keeps trying, quietly, and sends the latest when Discord appears.
        let dir = std::env::temp_dir().join(format!("kotora-discord-test-{}-late", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("discord-ipc-0");
        let tries = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let (p, n) = (path.clone(), tries.clone());
        let connect: Connector = Box::new(move || {
            n.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(Box::new(std::os::unix::net::UnixStream::connect(&p)?) as Box<dyn Stream>)
        });
        let w = Worker::start("1".into(), connect, quick());
        w.set(Some(act("Early")));
        w.set(Some(act("Late")));
        fake::wait_for("two tries", || tries.load(std::sync::atomic::Ordering::Relaxed) >= 2);
        // Discord starts: rename its socket into place.
        let s = fake::serve("late-real");
        std::fs::rename(&s.path, &path).unwrap();
        fake::wait_for("the status", || sets(&s).len() == 1);
        assert_eq!(sets(&s)[0]["args"]["activity"]["details"], "Late");
        w.stop(Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn it_reconnects_after_discord_restarts() {
        let s = fake::serve("restart");
        // The server hangs up after the first SET_ACTIVITY on each connection.
        *s.drop_after.lock().unwrap() = 1;
        let w = Worker::start("1".into(), fake::connector(s.path.clone()), quick());
        w.set(Some(act("Title A")));
        fake::wait_for("first", || sets(&s).len() == 1);
        w.set(Some(act("Title B")));
        // The write may succeed into a dead socket; the next one fails, then a new connection sends the latest.
        fake::wait_for("a second connection", || *s.accepted.lock().unwrap() >= 2);
        fake::wait_for("B shown", || sets(&s).iter().any(|v| v["args"]["activity"]["details"] == "Title B"));
        w.stop(Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn a_refused_id_is_an_error_not_a_hang() {
        use std::os::unix::net::UnixListener;
        let dir = std::env::temp_dir().join(format!("kotora-discord-test-{}-bad", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("discord-ipc-0");
        let l = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let mut s = s.unwrap();
                let _ = read_frame(&mut s);
                let _ = s.write_all(&frame(1, &json!({ "evt": "ERROR", "data": { "code": 4000 } })));
            }
        });
        let stream = std::os::unix::net::UnixStream::connect(&path).unwrap();
        assert!(Conn::open(Box::new(stream), "1").is_err());
    }

    #[test]
    fn nothing_listening_is_just_an_error() {
        let c = Worker::start("1".into(), Box::new(|| Err(io::Error::new(io::ErrorKind::NotFound, "no"))), Timing { retry: Duration::from_millis(10), min_gap: Duration::from_millis(10) });
        c.set(Some(act("Title A")));
        std::thread::sleep(Duration::from_millis(50));
        c.stop(Duration::from_secs(1));
    }
}
