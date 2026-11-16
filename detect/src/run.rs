//! Starts the sources and the watch hub, each on its own thread. Every thread blocks (a socket
//! read, a folder watch, the bus, a channel); the hub sleeps until the next threshold crossing
//! or the next report. Nothing ticks.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::watch::{Report, Watch};
use crate::{Playing, SourceStatus, Switch};

/// What a source tells the hub.
#[derive(Debug)]
pub enum Input {
    Update(Playing),
    Gone(String),
    Status(SourceStatus),
    /// The settings changed.
    Settings,
}

/// The detection settings (Settings → Updates and Players, `config.toml`).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 0–1.
    pub threshold: f64,
    /// Players switched off.
    pub off: Vec<Switch>,
    /// mpv / IINA sockets; read once at start.
    pub mpv_sockets: Vec<PathBuf>,
    pub vlc_port: u16,
    /// VLC's web password (macOS); empty: not set up.
    pub vlc_password: String,
    /// The process source (`procfs`): players' open files, no setup needed.
    pub processes: bool,
    /// Process source: minutes a file must stay open to count when its duration is unknown.
    pub process_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { threshold: 0.8, off: Vec::new(), mpv_sockets: crate::mpv::default_sockets(), vlc_port: 8080, vlc_password: String::new(), processes: true, process_minutes: crate::watch::OPEN_MINUTES }
    }
}

/// The settings the threads share.
pub struct Shared {
    settings: Mutex<Settings>,
    /// The mpv / IINA sockets with a thread, so one is never watched twice.
    sockets: Mutex<HashSet<PathBuf>>,
}

impl Shared {
    fn get(&self) -> Settings {
        self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// VLC's web source: switched on, port, password.
    pub fn vlc(&self) -> (bool, u16, String) {
        let s = self.get();
        (!s.off.contains(&Switch::Vlc), s.vlc_port, s.vlc_password)
    }

    /// The process source is on.
    pub fn processes(&self) -> bool {
        self.get().processes
    }
}

/// The running detection. Dropping it leaves the threads blocked and idle until the app ends.
pub struct Detector {
    tx: Sender<Input>,
    shared: Arc<Shared>,
    vlc_wake: Option<Sender<()>>,
    proc_wake: Option<Sender<()>>,
}

impl Detector {
    /// New settings: the threshold and switches apply at once, and sockets that aren't watched yet
    /// start to be (a socket dropped from the list stays watched until the app ends).
    pub fn set(&self, s: Settings) {
        for sock in &s.mpv_sockets {
            self.watch_socket(sock);
        }
        *self.shared.settings.lock().unwrap_or_else(|e| e.into_inner()) = s;
        let _ = self.tx.send(Input::Settings);
        for w in [&self.vlc_wake, &self.proc_wake].into_iter().flatten() {
            let _ = w.send(());
        }
    }
}

impl Detector {
    /// Starts watching an mpv / IINA socket at once (no restart). `false`: it was watched already.
    pub fn watch_socket(&self, sock: &std::path::Path) -> bool {
        spawn_socket(&self.shared, &self.tx, sock)
    }

    /// The socket has a thread, from the settings or [`Detector::watch_socket`].
    pub fn watches(&self, sock: &std::path::Path) -> bool {
        self.shared.sockets.lock().unwrap_or_else(|e| e.into_inner()).contains(sock)
    }
}

fn spawn_socket(shared: &Shared, tx: &Sender<Input>, sock: &std::path::Path) -> bool {
    if !shared.sockets.lock().unwrap_or_else(|e| e.into_inner()).insert(sock.to_path_buf()) {
        return false;
    }
    let (sock, tx) = (sock.to_path_buf(), tx.clone());
    let _ = std::thread::Builder::new().name("kotora-mpv".into()).spawn(move || crate::mpv::run(sock, tx));
    true
}

/// Starts every source of this system and the hub. `sink` gets the hub's reports, on the hub's
/// thread.
pub fn start(settings: Settings, sink: impl Fn(Report) + Send + 'static) -> Detector {
    let (tx, rx) = mpsc::channel::<Input>();
    let shared = Arc::new(Shared { settings: Mutex::new(settings.clone()), sockets: Mutex::new(HashSet::new()) });
    for sock in &settings.mpv_sockets {
        spawn_socket(&shared, &tx, sock);
    }
    #[cfg(target_os = "linux")]
    {
        let tx = tx.clone();
        let _ = std::thread::Builder::new().name("kotora-mpris".into()).spawn(move || crate::mpris::run(tx));
    }
    let vlc_wake = if crate::VLC_WEB {
        let (wtx, wrx) = mpsc::channel();
        let (sh, tx) = (shared.clone(), tx.clone());
        let _ = std::thread::Builder::new().name("kotora-vlc".into()).spawn(move || crate::vlc::run(sh, wrx, tx));
        Some(wtx)
    } else {
        None
    };
    let proc_wake = if cfg!(any(target_os = "linux", target_os = "macos")) {
        let (wtx, wrx) = mpsc::channel();
        let (sh, tx) = (shared.clone(), tx.clone());
        let _ = std::thread::Builder::new().name("kotora-procs".into()).spawn(move || crate::procfs::run(sh, wrx, tx));
        Some(wtx)
    } else {
        None
    };
    let sh = shared.clone();
    let _ = std::thread::Builder::new().name("kotora-watch".into()).spawn(move || {
        let mut w = Watch::new(settings.threshold);
        w.open_secs = f64::from(settings.process_minutes) * 60.0;
        w.off = settings.off.iter().copied().collect();
        loop {
            let now = crate::clock();
            let got = match w.next_wake(now) {
                Some(secs) => rx.recv_timeout(Duration::from_secs_f64(secs)),
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            let now = crate::clock();
            let reports = match got {
                Ok(Input::Update(p)) => w.update(p, now),
                Ok(Input::Gone(k)) => w.gone(&k, now),
                Ok(Input::Status(st)) => w.status(st).into_iter().collect(),
                Ok(Input::Settings) => {
                    let s = sh.get();
                    w.threshold = s.threshold;
                    w.open_secs = f64::from(s.process_minutes) * 60.0;
                    w.off = s.off.iter().copied().collect();
                    let mut out = w.drop_off(now);
                    out.extend(w.tick(now));
                    out
                }
                Err(RecvTimeoutError::Timeout) => w.tick(now),
                Err(RecvTimeoutError::Disconnected) => return,
            };
            for r in reports {
                sink(r);
            }
        }
    });
    Detector { tx, shared, vlc_wake, proc_wake }
}
