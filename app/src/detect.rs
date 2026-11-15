//! Detection glue: starts `kotora_detect` and one recognition thread, and hands their results to
//! the UI thread with `AppHandle::update_if`. Nothing here runs headless (no handle, no threads,
//! no config writes, no notifications).
//!
//! The recognition thread blocks on its channel. For each new file it parses the name, reads the
//! saved list and the cache from the database, and (only when nothing on the list matches, the
//! user is signed in and not offline) asks MyAnimeList once, within the shared rate budget.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use kotora_core::config::{self, Config};
use kotora_core::mal::{Client, Mal, UreqTransport};
use kotora_core::paths;
use kotora_core::recognize::{self, Found, Parsed};
use kotora_core::service::ListService;
use kotora_core::store::Store;
use kotora_core::time::now_unix;
use kotora_core::tokens::TokenStore;
use kotora_detect::{Detector, Report, Settings, Switch};

use crate::model::{Handle, Msg, State};
use crate::now::{Prefs, Seen};

const SERVICE: &str = "mal";

enum In {
    Report(Report),
    /// The user's pick for a title key (`None`: not an anime).
    Correct { key: String, media_id: Option<u32> },
}

/// The running detection, kept in the state.
pub struct Glue {
    tx: Sender<In>,
    detector: Arc<Detector>,
}

/// The detection settings from `config.toml`.
pub fn settings(c: &Config) -> Settings {
    let d = Settings::default();
    Settings {
        threshold: f64::from(c.updates.threshold) / 100.0,
        off: Switch::ALL.into_iter().filter(|sw| !c.players.get(sw.config_key())).collect(),
        mpv_sockets: if c.players.mpv_sockets.is_empty() { d.mpv_sockets } else { c.players.mpv_sockets.iter().map(PathBuf::from).collect() },
        vlc_port: c.vlc.port,
        vlc_password: c.vlc.password.clone(),
        processes: c.players.process_scan,
        process_minutes: c.players.process_minutes.clamp(1, 180),
    }
}

/// At start (never headless): reads the settings and starts the sources.
pub fn start(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
    let c = Config::load(&paths::config_file()).unwrap_or_default();
    s.prefs = Prefs::from_config(&c);
    s.worker.ask_on_higher.store(!c.updates.keep_higher_progress, std::sync::atomic::Ordering::Relaxed);
    crate::precise::check_conf(h.clone());
    let (tx, rx) = mpsc::channel::<In>();
    let rtx = tx.clone();
    let detector = kotora_detect::start(settings(&c), move |r| {
        let _ = rtx.send(In::Report(r));
    });
    let _ = std::thread::Builder::new().name("kotora-recognize".into()).spawn(move || run(&h, &rx));
    s.glue = Some(Glue { tx, detector: Arc::new(detector) });
}

/// A desktop notification (only in the real app).
pub fn notify(summary: &str, body: &str, live: bool) {
    if live && !rune::is_headless() {
        kotora_detect::notify::send(summary, body);
    }
}

/// Remembers the user's pick (or "not an anime") for a title.
pub fn correct(s: &State, key: String, media_id: Option<u32>) {
    if let Some(g) = &s.glue {
        let _ = g.tx.send(In::Correct { key, media_id });
    }
}

/// Writes one setting to `config.toml` and applies it to the sources, off the UI thread.
pub fn save_setting(s: &mut State, section: &'static str, key: &'static str, value: String) {
    let (Some(h), Some(g)) = (s.handle.clone(), s.glue.as_ref()) else { return };
    let detector = g.detector.clone();
    let _ = std::thread::Builder::new().name("kotora-settings".into()).spawn(move || {
        let path = paths::config_file();
        if let Err(e) = config::write_setting(&path, section, key, &value) {
            h.update_if(move |s, _| s.apply(Msg::QueueError(format!("Can't save the setting: {e}"))));
            return;
        }
        if let Ok(c) = Config::load(&path) {
            detector.set(settings(&c));
        }
    });
}

fn send(h: &Handle, msg: Msg) {
    h.update_if(move |s, _| s.apply(msg));
}

/// What recognition made of the current file.
struct Current {
    file: String,
    parsed: Option<Parsed>,
    found: Found,
    candidates: Vec<(u32, String)>,
}

fn run(h: &Handle, rx: &Receiver<In>) {
    let mut store: Option<Store> = None;
    let mut current: Option<Current> = None;
    for msg in rx {
        match msg {
            In::Report(Report::Now(None)) => {
                current = None;
                send(h, Msg::Now(None));
            }
            In::Report(Report::Now(Some(track))) => {
                let file = track.playing.file().unwrap_or_default().to_string();
                if current.as_ref().is_none_or(|c| c.file != file) {
                    current = Some(recognise(&mut store, &file));
                }
                let Some(c) = &current else { continue };
                send(h, Msg::Now(Some(Box::new(Seen { track, parsed: c.parsed.clone(), found: c.found.clone(), candidates: c.candidates.clone() }))));
            }
            In::Report(Report::Reached { key, file }) => send(h, Msg::Reached { key, file }),
            In::Report(Report::Sources(v)) => send(h, Msg::Players(v)),
            In::Correct { key, media_id } => {
                if let Some(st) = open(&mut store) {
                    let (id, source) = match media_id {
                        Some(id) => (id, "user"),
                        None => (0, "ignore"),
                    };
                    if let Err(e) = st.set_recognition(SERVICE, &key, id, source, "", now_unix()) {
                        send(h, Msg::QueueError(format!("Can't save your pick: {e}")));
                    }
                }
                if let Some(c) = &mut current
                    && c.parsed.as_ref().is_some_and(|p| p.key() == key)
                {
                    c.found = match media_id {
                        Some(media_id) => Found::List { media_id, sure: true, why: "you picked it".into() },
                        None => Found::Ignored,
                    };
                }
            }
        }
    }
}

fn open(store: &mut Option<Store>) -> Option<&mut Store> {
    if store.is_none() {
        *store = Store::open(&paths::db_file()).ok();
    }
    store.as_mut()
}

fn recognise(store: &mut Option<Store>, file: &str) -> Current {
    let nothing = |parsed| Current { file: file.to_string(), parsed, found: Found::Nothing, candidates: Vec::new() };
    let Some(parsed) = recognize::parse(file) else { return nothing(None) };
    let Some(st) = open(store) else { return nothing(Some(parsed)) };
    let items = st.load_list(SERVICE).unwrap_or_default();
    let key = parsed.key();
    let cached = st.recognition(SERVICE, &key).ok().flatten();
    let candidates = recognize::candidates(&parsed, &items).into_iter().take(8).map(|c| (c.media_id, c.title)).collect();
    let found = match recognize::on_list(&parsed, &items, &cached) {
        Some(f) => f,
        None => match cached.filter(|(_, src, _)| src == "search") {
            // Searched before (30 days): no second call.
            Some((0, _, _)) => Found::Nothing,
            Some((id, _, _)) if items.iter().any(|it| it.media.id == id) => Found::List { media_id: id, sure: false, why: "found by a MyAnimeList search".into() },
            Some((id, _, title)) => Found::NotOnList { media_id: id, title },
            None => search(st, &parsed, &items, &key),
        },
    };
    Current { file: file.to_string(), parsed: Some(parsed), found, candidates }
}

/// The one MAL search (signed in and online only). The answer is cached either way.
fn search(st: &Store, parsed: &Parsed, items: &[kotora_core::ListItem], key: &str) -> Found {
    if paths::offline_forced() || items.is_empty() {
        return Found::Nothing;
    }
    let Some(id) = Config::load(&paths::config_file()).ok().and_then(|c| c.mal_client_id) else { return Found::Nothing };
    let tokens = TokenStore::new(paths::token_file());
    if tokens.load_mal().is_none() {
        return Found::Nothing;
    }
    let mut mal = Mal::new(Client::new(id, UreqTransport::default()), tokens);
    let Ok(hits) = mal.search(&parsed.title) else { return Found::Nothing };
    let found = recognize::from_search(&hits, items);
    let (mid, title) = match &found {
        Found::List { media_id, .. } => (*media_id, String::new()),
        Found::NotOnList { media_id, title } => (*media_id, title.clone()),
        _ => (0, String::new()),
    };
    let _ = st.set_recognition(SERVICE, key, mid, "search", &title, now_unix());
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_follow_the_config() {
        let c = Config::parse("[updates]\nthreshold = 90\n[players]\nvlc = false\nmpv_sockets = [\"/tmp/x.sock\"]\n[vlc]\nport = 9090\npassword = \"p\"\n").unwrap();
        let s = settings(&c);
        assert!((s.threshold - 0.9).abs() < 1e-9);
        assert_eq!(s.off, vec![Switch::Vlc]);
        assert_eq!(s.mpv_sockets, vec![PathBuf::from("/tmp/x.sock")]);
        assert_eq!((s.vlc_port, s.vlc_password.as_str()), (9090, "p"));
        assert_eq!(settings(&Config::default()).mpv_sockets, Settings::default().mpv_sockets);
        assert!(s.processes && s.process_minutes == 15);
        let c = Config::parse("[players]\nprocess_scan = false\nprocess_minutes = 0\n").unwrap();
        assert_eq!((settings(&c).processes, settings(&c).process_minutes), (false, 1));
    }
}
