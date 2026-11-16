//! The control protocol between a running Kotora and `kot`, its command line.
//!
//! The app listens on a Unix stream socket ([`socket_path`]). `kot` connects, writes one
//! [`Request`] per line as JSON, and reads one [`Reply`] per line back. A connection may carry any
//! number of requests; they are answered in order. The same socket gives single instance: a second
//! `kotora` finds the first one here, sends [`Request::Show`] and exits.
//!
//! Both sides use this crate, so they can't drift apart: the app calls [`serve`] with a handler,
//! `kot` uses [`Client`]. Adapted from Uzumaki's `ctl/src/lib.rs`.
//!
//! Compatibility rule: new fields are `#[serde(default)]` (and skipped when empty where that keeps
//! old bytes), so an old `kot` reads a new app and a new `kot` reads an old app. A request an app
//! doesn't know is answered with "bad request".

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The build number: a whole number that goes up with every package built from the same version.
/// Its one source is the `BUILD` file at the top of the repo; the packaging scripts read it too.
/// 0.1.0 is build 1.
pub const BUILD: u32 = parse_build(include_str!("../../BUILD"));

/// `BUILD`'s number, at compile time: digits, optional spaces and newlines around them, above 0.
/// Anything else stops the build.
const fn parse_build(s: &str) -> u32 {
    let b = s.as_bytes();
    let (mut i, mut n, mut digits) = (0, 0u32, 0);
    while i < b.len() {
        match b[i] {
            b'0'..=b'9' if digits == 0 || i == 0 || b[i - 1].is_ascii_digit() => {
                n = match n.checked_mul(10) {
                    Some(m) => m + (b[i] - b'0') as u32,
                    None => panic!("BUILD is too large"),
                };
                digits += 1;
            }
            b' ' | b'\t' | b'\n' | b'\r' => {}
            _ => panic!("BUILD must hold one whole number"),
        }
        i += 1;
    }
    assert!(digits > 0 && n > 0, "BUILD must hold a number above 0");
    n
}

/// The version and build as `--version` prints them: `0.1.0 (build 1)`, plus the dev suffix.
pub fn version_line() -> String {
    format!("{} (build {BUILD}){}", env!("CARGO_PKG_VERSION"), dev_suffix())
}

/// What `build.rs` found in git: the branch, short hash, whether the tree has changes, and whether
/// HEAD is exactly the release tag for this version and build.
#[derive(Debug, PartialEq)]
pub struct GitFacts<'a> {
    pub branch: &'a str,
    pub hash: &'a str,
    pub dirty: bool,
    pub at_release_tag: bool,
}

/// The text a dev build adds after the version, for display only. Empty without git facts
/// (tarball, AUR source build) and for a clean checkout of the release tag. Otherwise
/// ` · main 20c1fe9`, with ` dirty` if the tree has changes.
pub fn git_suffix(facts: Option<GitFacts>) -> String {
    match facts {
        Some(f) if !(f.at_release_tag && !f.dirty) => {
            format!(" \u{b7} {} {}{}", f.branch, f.hash, if f.dirty { " dirty" } else { "" })
        }
        _ => String::new(),
    }
}

/// `build.rs`'s `KOTORA_GIT` (tab separated, or empty), as facts.
fn parse_facts(s: &str) -> Option<GitFacts<'_>> {
    let mut p = s.split('\t');
    let (branch, hash, dirty, tag) = (p.next()?, p.next()?, p.next()?, p.next()?);
    (!branch.is_empty() && !hash.is_empty()).then_some(GitFacts { branch, hash, dirty: dirty == "1", at_release_tag: tag == "1" })
}

/// [`git_suffix`] for this build.
pub fn dev_suffix() -> String {
    git_suffix(parse_facts(env!("KOTORA_GIT")))
}

// --------------------------------------------------------------------------------------- paths

/// `$KOTORA_CTL_SOCKET`, else `<runtime dir>/kotora.sock` (see [`runtime_dir`]).
pub fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("KOTORA_CTL_SOCKET").filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    runtime_dir().join("kotora.sock")
}

/// Linux: `$XDG_RUNTIME_DIR`, else `/tmp/kotora-<uid>`.
#[cfg(not(target_os = "macos"))]
pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("kotora-{}", uid())))
}

/// macOS: `kotora-<uid>` in the per-user temp folder (`/var/folders/…/T/`), asked from the system
/// instead of read from `$TMPDIR`: an ssh login doesn't set `$TMPDIR` (it comes with the GUI
/// login), and the app started from the Dock and `kot` over ssh must find the same socket.
#[cfg(target_os = "macos")]
pub fn runtime_dir() -> PathBuf {
    darwin_user_temp_dir().unwrap_or_else(std::env::temp_dir).join(format!("kotora-{}", uid()))
}

#[cfg(target_os = "macos")]
fn darwin_user_temp_dir() -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut buf = vec![0u8; 1024];
    // SAFETY: `buf` is writable for `buf.len()` bytes; confstr writes at most that many,
    // NUL included, and returns the size it needs (NUL included), or 0 on failure.
    let n = unsafe { libc::confstr(libc::_CS_DARWIN_USER_TEMP_DIR, buf.as_mut_ptr().cast(), buf.len()) };
    if n <= 1 || n > buf.len() {
        return None;
    }
    buf.truncate(n - 1);
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(&buf)))
}

fn uid() -> u32 {
    // SAFETY: getuid has no preconditions and can't fail.
    unsafe { libc::getuid() }
}

// ------------------------------------------------------------------------------------ requests

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    /// The app's state in one line. Replies `Status`.
    Status,
    /// Shows the app's window (a second `kotora` sends this, then exits). Replies `Status`.
    Show,
    /// Quits the app. Replies `Status`, then the app goes away.
    Quit,
    /// The saved list: one status (`watching`, `plan`, `completed`, `on-hold`, `dropped`) or all
    /// (`None`). Replies `List`.
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// One more episode of the entry whose title matches. Replies `Edited`; an episode that
    /// hasn't aired yet (by the estimate) replies `Error` with `anyway` unless `anyway` is set.
    PlusOne {
        title: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        anyway: bool,
    },
    /// Sets the progress, status or score of the entry whose title matches. Replies `Edited`.
    Set {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        progress: Option<u32>,
        /// A status name as in `List`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        /// 1-10; 0 removes the score.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        score: Option<u8>,
        /// Mark an episode that hasn't aired yet without asking (as `PlusOne`).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        anyway: bool,
    },
    /// The sync queue. Replies `Queue`.
    Queue,
    /// Sends the queue now, then reads the list again. Replies `Status`.
    Sync,
    /// What plays and what Kotora offers. Replies `Now`.
    Now,
    /// Records the offered episode (the banner's Update). Replies `Edited`.
    Confirm,
    /// Not now: hides the offer for this file. Replies `Now`.
    Dismiss,
    /// Saves the MyAnimeList Client ID (as the Welcome page's field does). Replies `Done`.
    SetClientId { id: String },
    /// Starts a rewatch of a completed entry: a part of its title, or its MyAnimeList id.
    /// Replies `Edited`.
    Rewatch { title: String },
    /// Puts an anime on the list: a MyAnimeList id, or a title MyAnimeList is searched for
    /// (it must find one). `status` as in `List`; Watching when `None`. Replies `Edited`.
    Add {
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// Episodes of the user's list that air in the next 7 days (estimated). Replies `Airing`.
    Airing,
    /// Opens the next episode's file (progress + 1) from the library folders in the player of
    /// Settings → Library. Replies `Done`, or `Error` with why not.
    Next { title: String },
}

// ------------------------------------------------------------------------------------- replies

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case")]
pub enum Reply {
    Status(Status),
    /// `anyway`: the request was held, not refused: the same request with `anyway` set does it
    /// ("Episode 9 … hasn't aired yet").
    Error {
        message: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        anyway: bool,
    },
    List { entries: Vec<Entry> },
    /// The entry after an edit; the change waits in the queue.
    Edited { entry: Entry },
    Queue { ops: Vec<QueueItem> },
    /// `None`: nothing plays.
    Now { now: Option<NowInfo> },
    /// Done, in a sentence.
    Done { message: String },
    /// The next episodes of the list, soonest first; `note` says why there are none yet.
    Airing {
        items: Vec<AiringItem>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
}

/// One episode in `kot airing`. The app writes the day and time in its (the user's) zone.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AiringItem {
    pub title: String,
    pub episode: u32,
    /// Unix seconds (estimated).
    pub at: i64,
    /// "Today", "Tomorrow", "Saturday".
    pub day: String,
    /// "15:30".
    pub time: String,
    /// "in 1 d 6 h".
    pub countdown: String,
    /// Episodes aired (by the estimate) that the progress hasn't reached.
    #[serde(skip_serializing_if = "is_zero")]
    pub behind: u32,
}

/// What plays, as `kot now` prints it.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NowInfo {
    /// "mpv", "VLC media player", …
    pub player: String,
    /// "IPC socket", "MPRIS", "web interface".
    pub via: String,
    /// The file's name.
    pub file: String,
    pub playing: bool,
    /// 0–100, by position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    /// The anime: the list entry's title, a search hit, or the parsed title.
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub episode: Option<u32>,
    /// "In your list (Watching), sure: …", "Not on your list: …".
    pub matched: String,
    /// The episode Update would record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer: Option<u32>,
    /// Why the offer asks even with "Update automatically" on: "Skips episode 5", "Episode 9
    /// hasn't aired yet (expected …)".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The episode counts as watched (the banner shows).
    pub reached: bool,
    /// The episode recorded from here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded: Option<u32>,
    pub dismissed: bool,
}

/// One list entry, as `kot list` prints it.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    /// The service's id (MyAnimeList id).
    pub id: u32,
    pub title: String,
    /// "Watching", "Plan to watch", …
    pub status: String,
    pub progress: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub episodes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<u8>,
    /// "Waiting to sync", "Conflict in queue", "Sync failed"; `None` when synced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
    /// Watching it again (still Completed).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rewatching: bool,
    /// Finished rewatches.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub times_rewatched: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// One change in the sync queue.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueItem {
    pub title: String,
    /// "Progress 3 → 5", "Status → Completed, score 8".
    pub change: String,
    /// "Waiting", "Sending", "Conflict", "Failed", "Sent".
    pub state: String,
    /// The service's words for a failed change, the last error of a retried one, or what the
    /// service has in a conflict.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// What `kot status` prints. Every field has a default, so an older app's reply still reads.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Status {
    /// The app's [`version_line`].
    pub version: String,
    /// The signed-in account's user name; `None` when signed out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// "MyAnimeList" (v0.1); "" when signed out.
    pub service: String,
    /// Entries on the list, all statuses.
    pub entries: u32,
    /// Changes waiting in the sync queue (waiting, failed and conflicted).
    pub queue: u32,
    /// No network: the list works from the saved copy.
    pub offline: bool,
    /// The window is open (false for a `--hidden` start).
    pub window: bool,
}

// ------------------------------------------------------------------------------------ transport

/// How long a client waits for one reply. This only guards against a hung app.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// A connection to the running app.
pub struct Client {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Client {
    /// Connects to [`socket_path`]. Fails (NotFound / ConnectionRefused) when Kotora isn't running.
    pub fn connect() -> io::Result<Self> {
        Self::connect_to(&socket_path())
    }

    pub fn connect_to(path: &Path) -> io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(REPLY_TIMEOUT))?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self { writer: stream, reader })
    }

    /// Sends one request and waits for its reply. An app-side failure comes back as
    /// `Ok(Reply::Error { .. })`; `Err` means the connection itself failed.
    pub fn request(&mut self, req: &Request) -> io::Result<Reply> {
        let mut line = serde_json::to_string(req).map_err(io::Error::other)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes())?;
        self.writer.flush()?;
        let mut answer = String::new();
        if self.reader.read_line(&mut answer)? == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the app closed the connection"));
        }
        serde_json::from_str(&answer).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

/// The listening socket. Dropping it removes the socket file (the accept thread ends with the
/// process).
pub struct Server {
    path: PathBuf,
}

impl Server {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Listens on `path` and answers every request with `handler`, which runs on a per-connection
/// thread (it may block). A stale socket file from a crashed run is replaced; one that still
/// answers means another app owns it (`AddrInUse`). The accept thread sleeps in `accept`: no
/// timers, no polling.
pub fn serve(path: &Path, handler: impl Fn(Request) -> Reply + Send + Sync + 'static) -> io::Result<Server> {
    if let Some(dir) = path.parent()
        && !dir.exists()
    {
        std::fs::create_dir_all(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(io::ErrorKind::AddrInUse, format!("{} is in use", path.display())));
        }
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    let handler = Arc::new(handler);
    std::thread::Builder::new().name("kotora-ctl".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let handler = handler.clone();
            let _ = std::thread::Builder::new().name("kotora-ctl-conn".into()).spawn(move || connection(stream, &*handler));
        }
    })?;
    Ok(Server { path: path.to_path_buf() })
}

fn connection(stream: UnixStream, handler: &(dyn Fn(Request) -> Reply + Send + Sync)) {
    let Ok(mut writer) = stream.try_clone() else { return };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Request>(&line) {
            Ok(req) => handler(req),
            Err(e) => Reply::Error { message: format!("bad request: {e}"), anyway: false },
        };
        let Ok(mut out) = serde_json::to_string(&reply) else { return };
        out.push('\n');
        if writer.write_all(out.as_bytes()).and_then(|_| writer.flush()).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
