//! The zero-setup source: which video file a known player has open, read from the process table.
//!
//! mpv without an IPC socket or the mpv-mpris plugin says nothing to anyone, but the kernel still
//! knows which files it has open. Every [`INTERVAL`] this source:
//!
//! 1. lists the process ids (Linux: `/proc`; macOS: `proc_listpids`, see `libproc.rs`), and reads
//!    the name (`/proc/PID/comm`) of new ones only. A process that is not a player is read once
//!    more on the next scan (a launcher that `exec`s mpv) and then never again;
//! 2. for the players of the current user (mpv, VLC, mpc-qt, Celluloid, Haruna, SMPlayer; IINA on
//!    macOS), reads the open files (`readlink /proc/PID/fd/*`) and keeps the video files (by
//!    extension: subtitles, fonts, libraries and sockets are skipped);
//! 3. picks one file per player: the one picked before while it stays open (a player may already
//!    open the next file of a playlist), else the lowest fd;
//! 4. reports changes only, as `Via::Process` snapshots keyed `proc:PID`.
//!
//! A process source knows neither position, duration nor pause, so the watch hub counts the time
//! the file stays open instead (see `watch.rs`: open for `process_minutes`, default 15, or the
//! threshold share of the duration when one is known). When an IPC or MPRIS source reports the
//! same file, the hub ignores this one: they know the position.
//!
//! The cost is a directory listing of `/proc` every 5 s plus one small `readlink` per open fd of
//! a running player; nothing at all when the source is off.

use std::collections::{HashMap, HashSet};
#[cfg(unix)]
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(unix)]
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::run::{Input, Shared};
use crate::{Led, PlayState, Playing, SourceStatus, Switch, Via};

/// Time between two scans.
pub const INTERVAL: Duration = Duration::from_secs(5);

/// File extensions counted as video (lower case).
pub const VIDEO_EXTS: &[&str] = &[
    "3gp", "asf", "avi", "divx", "f4v", "flv", "m2ts", "m2v", "m4v", "mk3d", "mkv", "mov", "mp4",
    "mpeg", "mpg", "mts", "ogm", "ogv", "rm", "rmvb", "ts", "vob", "webm", "wmv",
];

/// Whether a path looks like a video file.
pub fn is_video(path: &str) -> bool {
    if !path.starts_with('/') || path.ends_with(" (deleted)") {
        return false;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => {
            VIDEO_EXTS.iter().any(|v| ext.eq_ignore_ascii_case(v))
        }
        _ => false,
    }
}

/// A known player, by process name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    pub player: &'static str,
    pub switch: Switch,
}

/// The player a process name stands for (`comm` on Linux, `proc_name` on macOS).
pub fn kind_of(name: &str) -> Option<Kind> {
    let name = name.trim();
    let k = |player, switch| Some(Kind { player, switch });
    match name.to_ascii_lowercase().as_str() {
        "mpv" => k("mpv", Switch::Mpv),
        "vlc" => k("VLC", Switch::Vlc),
        "mpc-qt" => k("mpc-qt", Switch::MpcQt),
        "iina" => k("IINA", Switch::Iina),
        "celluloid" => k("Celluloid", Switch::OtherMpris),
        "haruna" => k("Haruna", Switch::OtherMpris),
        "smplayer" => k("SMPlayer", Switch::OtherMpris),
        _ => None,
    }
}

/// The process table, as this source needs it. Linux: [`ProcFs`]; macOS: `libproc::LibProc`.
pub trait Procs {
    /// Every process id now.
    fn pids(&mut self) -> Vec<u32>;
    /// The process name.
    fn name(&self, pid: u32) -> Option<String>;
    /// Whether the process belongs to the current user.
    fn mine(&self, pid: u32) -> bool;
    /// Open files: (fd, absolute path).
    fn files(&self, pid: u32) -> Vec<(i32, String)>;
    /// The command line (argv, the program first). Empty when it can't be read.
    fn args(&self, _pid: u32) -> Vec<String> {
        Vec::new()
    }
}

/// argv from a NUL-separated block (`/proc/PID/cmdline`).
pub fn split_nul(b: &[u8]) -> Vec<String> {
    b.split(|c| *c == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect()
}

/// `/proc` on Linux. The root is a parameter so tests use a fake tree.
#[cfg(unix)]
pub struct ProcFs {
    root: PathBuf,
    uid: u32,
}

#[cfg(unix)]
impl ProcFs {
    pub fn new(root: impl Into<PathBuf>, uid: u32) -> ProcFs {
        ProcFs {
            root: root.into(),
            uid,
        }
    }

    /// The real `/proc`, for the current user.
    pub fn system() -> ProcFs {
        // SAFETY: getuid has no preconditions and can't fail.
        ProcFs::new("/proc", unsafe { libc::getuid() })
    }
}

#[cfg(unix)]
impl Procs for ProcFs {
    fn pids(&mut self) -> Vec<u32> {
        let Ok(dir) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
            .collect()
    }

    fn name(&self, pid: u32) -> Option<String> {
        let s = std::fs::read_to_string(self.root.join(pid.to_string()).join("comm")).ok()?;
        Some(s.trim_end_matches('\n').to_string())
    }

    fn mine(&self, pid: u32) -> bool {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(self.root.join(pid.to_string())).is_ok_and(|m| m.uid() == self.uid)
    }

    fn files(&self, pid: u32) -> Vec<(i32, String)> {
        let dir = self.root.join(pid.to_string()).join("fd");
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        rd.filter_map(|e| {
            let e = e.ok()?;
            let fd: i32 = e.file_name().to_str()?.parse().ok()?;
            let target = std::fs::read_link(e.path()).ok()?;
            Some((fd, target.into_os_string().into_string().ok()?))
        })
        .collect()
    }

    fn args(&self, pid: u32) -> Vec<String> {
        std::fs::read(self.root.join(pid.to_string()).join("cmdline"))
            .map(|b| split_nul(&b))
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug)]
enum Seen {
    /// Not a player. `rechecked`: its name was read twice (once after a scan's delay).
    Other {
        rechecked: bool,
    },
    Player(Kind),
}

/// One player process and the file it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub pid: u32,
    pub kind: Kind,
    pub file: Option<String>,
}

/// Finds the players and their files; remembers what it learnt between scans.
pub struct Scanner<P: Procs> {
    procs: P,
    seen: HashMap<u32, Seen>,
    picked: HashMap<u32, String>,
}

impl<P: Procs> Scanner<P> {
    pub fn new(procs: P) -> Scanner<P> {
        Scanner {
            procs,
            seen: HashMap::new(),
            picked: HashMap::new(),
        }
    }

    /// The player processes now, by pid.
    pub fn scan(&mut self) -> Vec<Found> {
        let pids = self.procs.pids();
        let live: HashSet<u32> = pids.iter().copied().collect();
        self.seen.retain(|p, _| live.contains(p));
        self.picked.retain(|p, _| live.contains(p));
        let mut out = Vec::new();
        for pid in pids {
            let seen = match self.seen.get(&pid) {
                Some(Seen::Other { rechecked: true }) => continue,
                Some(Seen::Player(k)) => Seen::Player(*k),
                Some(Seen::Other { rechecked: false }) => self.classify(pid, true),
                None => self.classify(pid, false),
            };
            self.seen.insert(pid, seen);
            if let Seen::Player(kind) = seen {
                let videos: Vec<(i32, String)> = self
                    .procs
                    .files(pid)
                    .into_iter()
                    .filter(|(_, p)| is_video(p))
                    .collect();
                let file = pick(&videos, self.picked.get(&pid));
                match &file {
                    Some(f) => self.picked.insert(pid, f.clone()),
                    None => self.picked.remove(&pid),
                };
                out.push(Found { pid, kind, file });
            }
        }
        out.sort_by_key(|f| f.pid);
        out
    }

    /// Forgets everything (the source was switched off).
    pub fn reset(&mut self) {
        self.seen.clear();
        self.picked.clear();
    }

    fn classify(&self, pid: u32, second: bool) -> Seen {
        match self.procs.name(pid).as_deref().and_then(kind_of) {
            Some(k) if self.procs.mine(pid) => Seen::Player(k),
            _ => Seen::Other { rechecked: second },
        }
    }
}

/// The file shown: the one picked before while still open, else the lowest fd.
fn pick(videos: &[(i32, String)], before: Option<&String>) -> Option<String> {
    if let Some(b) = before
        && videos.iter().any(|(_, p)| p == b)
    {
        return Some(b.clone());
    }
    videos
        .iter()
        .min_by_key(|(fd, _)| *fd)
        .map(|(_, p)| p.clone())
}

/// Turns scans into hub inputs, sending changes only.
#[derive(Default)]
pub struct Reporter {
    /// pid → (player, file) as last sent.
    sent: HashMap<u32, (Kind, Option<String>)>,
    status: HashMap<Switch, (String, Led)>,
}

pub fn key(pid: u32) -> String {
    format!("proc:{pid}")
}

impl Reporter {
    pub fn apply(&mut self, found: &[Found], now: f64) -> Vec<Input> {
        let mut out = Vec::new();
        let pids: HashSet<u32> = found.iter().map(|f| f.pid).collect();
        let gone: Vec<u32> = self
            .sent
            .keys()
            .copied()
            .filter(|p| !pids.contains(p))
            .collect();
        for pid in gone {
            if let Some((_, Some(_))) = self.sent.remove(&pid) {
                out.push(Input::Gone(key(pid)));
            }
        }
        for f in found {
            let before = self.sent.get(&f.pid).and_then(|(_, file)| file.clone());
            if before != f.file {
                match &f.file {
                    Some(path) => out.push(Input::Update(snapshot(f.pid, f.kind, path, now))),
                    None => out.push(Input::Gone(key(f.pid))),
                }
            }
            self.sent.insert(f.pid, (f.kind, f.file.clone()));
        }
        // One state per switch: playing if any of its processes has a video open.
        let mut now_status: HashMap<Switch, (String, Led)> = HashMap::new();
        for f in found {
            let (line, led) = if f.file.is_some() {
                ("video file open", Led::Playing)
            } else {
                ("running, no video open", Led::Idle)
            };
            let e = now_status
                .entry(f.kind.switch)
                .or_insert_with(|| (line.to_string(), led));
            if led == Led::Playing {
                *e = (line.to_string(), led);
            }
        }
        let mut switches: Vec<Switch> = self
            .status
            .keys()
            .chain(now_status.keys())
            .copied()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        switches.sort_by_key(|s| Switch::ALL.iter().position(|a| a == s));
        for sw in switches {
            let st = now_status
                .remove(&sw)
                .unwrap_or_else(|| ("not running".to_string(), Led::Off));
            if self.status.get(&sw) != Some(&st) {
                out.push(Input::Status(SourceStatus {
                    switch: sw,
                    via: Via::Process,
                    line: st.0.clone(),
                    led: st.1,
                }));
                self.status.insert(sw, st);
            }
        }
        out
    }
}

/// What a process source knows: the player and the file; no position, duration or pause.
pub fn snapshot(pid: u32, kind: Kind, path: &str, now: f64) -> Playing {
    Playing {
        key: key(pid),
        player: kind.player.to_string(),
        switch: kind.switch,
        via: Via::Process,
        path: Some(path.to_string()),
        title: None,
        position: None,
        duration: None,
        state: PlayState::Playing,
        rate: 1.0,
        at: now,
    }
}

/// The thread: scan, report, sleep [`INTERVAL`] (or until the settings change). While the
/// source is off it blocks until the settings change.
#[cfg(unix)]
pub fn run(shared: Arc<Shared>, wake: Receiver<()>, tx: Sender<Input>) {
    #[cfg(target_os = "macos")]
    let procs = crate::libproc::LibProc::new();
    #[cfg(not(target_os = "macos"))]
    let procs = ProcFs::system();
    let mut scanner = Scanner::new(procs);
    let mut reporter = Reporter::default();
    loop {
        let on = shared.processes();
        let found = if on {
            scanner.scan()
        } else {
            scanner.reset();
            Vec::new()
        };
        for input in reporter.apply(&found, crate::clock()) {
            if tx.send(input).is_err() {
                return;
            }
        }
        let got = if on {
            wake.recv_timeout(INTERVAL)
        } else {
            wake.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        if got == Err(RecvTimeoutError::Disconnected) {
            return;
        }
    }
}

/// Windows reads the players' command lines and window titles instead (`winplayers.rs`): this
/// thread is never started there.
#[cfg(not(unix))]
pub fn run(_shared: Arc<Shared>, _wake: Receiver<()>, _tx: Sender<Input>) {}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Tree {
        dir: PathBuf,
    }

    impl Tree {
        fn new(name: &str) -> Tree {
            let dir =
                std::env::temp_dir().join(format!("kotora-procfs-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Tree { dir }
        }

        fn proc(&self, pid: u32, comm: &str, fds: &[(i32, &str)]) {
            let p = self.dir.join(pid.to_string());
            std::fs::create_dir_all(p.join("fd")).unwrap();
            std::fs::write(p.join("comm"), format!("{comm}\n")).unwrap();
            for (fd, target) in fds {
                symlink(target, p.join("fd").join(fd.to_string())).unwrap();
            }
        }

        fn close(&self, pid: u32, fd: i32) {
            std::fs::remove_file(
                self.dir
                    .join(pid.to_string())
                    .join("fd")
                    .join(fd.to_string()),
            )
            .unwrap();
        }

        fn kill(&self, pid: u32) {
            std::fs::remove_dir_all(self.dir.join(pid.to_string())).unwrap();
        }

        fn scanner(&self) -> Scanner<ProcFs> {
            // SAFETY: getuid can't fail.
            Scanner::new(ProcFs::new(&self.dir, unsafe { libc::getuid() }))
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn files(found: &[Found]) -> Vec<(u32, Option<&str>)> {
        found.iter().map(|f| (f.pid, f.file.as_deref())).collect()
    }

    #[test]
    fn video_files_only() {
        for p in [
            "/v/A - 01.mkv",
            "/v/b.MP4",
            "/v/c.webm",
            "/v/d.m2ts",
            "/v/e.ts",
            "/v/f.ogm",
        ] {
            assert!(is_video(p), "{p}");
        }
        for p in [
            "/v/a.ass",
            "/v/a.srt",
            "/usr/share/fonts/x.ttf",
            "/usr/lib/libmpv.so.2",
            "socket:[1234]",
            "pipe:[9]",
            "anon_inode:[eventfd]",
            "/dev/dri/renderD128",
            "/v/a.mka",
            "/v/.mkv",
            "/v/x.mkv (deleted)",
            "rel/x.mkv",
        ] {
            assert!(!is_video(p), "{p}");
        }
    }

    #[test]
    fn finds_the_video_a_player_has_open() {
        let t = Tree::new("find");
        t.proc(100, "bash", &[(0, "/dev/pts/1")]);
        t.proc(
            200,
            "mpv",
            &[
                (0, "/dev/pts/1"),
                (3, "socket:[77]"),
                (5, "/usr/lib/libass.so.9"),
                (9, "/v/Show - 03.ass"),
                (12, "/v/Show - 03.mkv"),
                (14, "/usr/share/fonts/a.otf"),
            ],
        );
        t.proc(300, "vlc", &[(4, "/usr/lib/vlc/plugins/x.so")]);
        t.proc(400, "firefox", &[(30, "/v/Other - 01.mkv")]);
        let mut sc = t.scanner();
        let found = sc.scan();
        assert_eq!(
            files(&found),
            [(200, Some("/v/Show - 03.mkv")), (300, None)]
        );
        assert_eq!(
            found[0].kind,
            Kind {
                player: "mpv",
                switch: Switch::Mpv
            }
        );
        assert_eq!(found[1].kind.switch, Switch::Vlc);
    }

    #[test]
    fn follows_a_playlist_and_keeps_the_file_while_the_next_is_prefetched() {
        let t = Tree::new("playlist");
        t.proc(200, "mpv", &[(20, "/v/Show - 01.mkv")]);
        let mut sc = t.scanner();
        assert_eq!(files(&sc.scan()), [(200, Some("/v/Show - 01.mkv"))]);
        // The next file opens with a lower fd near the end: the current one stays.
        symlink("/v/Show - 02.mkv", t.dir.join("200/fd/7")).unwrap();
        assert_eq!(files(&sc.scan()), [(200, Some("/v/Show - 01.mkv"))]);
        // The current one closes: the next is shown.
        t.close(200, 20);
        assert_eq!(files(&sc.scan()), [(200, Some("/v/Show - 02.mkv"))]);
        t.close(200, 7);
        assert_eq!(files(&sc.scan()), [(200, None)]);
        t.kill(200);
        assert!(sc.scan().is_empty());
    }

    #[test]
    fn a_launcher_that_becomes_mpv_is_found_on_the_next_scan() {
        let t = Tree::new("exec");
        t.proc(500, "sh", &[]);
        let mut sc = t.scanner();
        assert!(sc.scan().is_empty());
        // `exec mpv …`: the name changes, the pid stays.
        std::fs::write(t.dir.join("500/comm"), "mpv\n").unwrap();
        symlink("/v/Show - 04.mkv", t.dir.join("500/fd/9")).unwrap();
        assert_eq!(files(&sc.scan()), [(500, Some("/v/Show - 04.mkv"))]);
        // A process read twice as "not a player" is never read again.
        t.proc(600, "bash", &[]);
        sc.scan();
        sc.scan();
        std::fs::write(t.dir.join("600/comm"), "mpv\n").unwrap();
        assert_eq!(files(&sc.scan()), [(500, Some("/v/Show - 04.mkv"))]);
    }

    #[test]
    fn other_users_processes_are_skipped() {
        let t = Tree::new("uid");
        t.proc(200, "mpv", &[(3, "/v/a.mkv")]);
        let mut sc = Scanner::new(ProcFs::new(&t.dir, u32::MAX - 7));
        assert!(sc.scan().is_empty());
    }

    #[test]
    fn names_map_to_switches() {
        assert_eq!(kind_of("mpv").unwrap().switch, Switch::Mpv);
        assert_eq!(kind_of("VLC").unwrap().switch, Switch::Vlc);
        assert_eq!(kind_of("mpc-qt").unwrap().switch, Switch::MpcQt);
        assert_eq!(kind_of("IINA").unwrap().switch, Switch::Iina);
        assert_eq!(kind_of("celluloid").unwrap().player, "Celluloid");
        assert!(kind_of("mpvpaper").is_none() && kind_of("vlc-cache-gen").is_none());
    }

    fn kinds(inputs: &[Input]) -> Vec<String> {
        inputs
            .iter()
            .map(|i| match i {
                Input::Update(p) => format!("update {} {}", p.key, p.path.as_deref().unwrap_or("")),
                Input::Gone(k) => format!("gone {k}"),
                Input::Status(s) => format!("status {:?} {}", s.switch, s.line),
                Input::Settings => "settings".into(),
            })
            .collect()
    }

    #[test]
    fn the_reporter_sends_changes_only() {
        let mpv = kind_of("mpv").unwrap();
        let f = |file: Option<&str>| {
            vec![Found {
                pid: 7,
                kind: mpv,
                file: file.map(String::from),
            }]
        };
        let mut r = Reporter::default();
        assert_eq!(
            kinds(&r.apply(&f(Some("/v/a.mkv")), 1.0)),
            ["update proc:7 /v/a.mkv", "status Mpv video file open"]
        );
        assert!(r.apply(&f(Some("/v/a.mkv")), 6.0).is_empty());
        assert_eq!(
            kinds(&r.apply(&f(Some("/v/b.mkv")), 11.0)),
            ["update proc:7 /v/b.mkv"]
        );
        assert_eq!(
            kinds(&r.apply(&f(None), 16.0)),
            ["gone proc:7", "status Mpv running, no video open"]
        );
        assert_eq!(
            kinds(&r.apply(&f(Some("/v/c.mkv")), 21.0)),
            ["update proc:7 /v/c.mkv", "status Mpv video file open"]
        );
        assert_eq!(
            kinds(&r.apply(&[], 26.0)),
            ["gone proc:7", "status Mpv not running"]
        );
        assert!(r.apply(&[], 31.0).is_empty());
    }

    #[test]
    fn snapshots_have_no_position() {
        let p = snapshot(7, kind_of("vlc").unwrap(), "/v/a.mkv", 3.0);
        assert_eq!(
            (p.via, p.position, p.duration, p.state, p.at),
            (Via::Process, None, None, PlayState::Playing, 3.0)
        );
        assert_eq!(p.file(), Some("/v/a.mkv"));
    }
}
