//! Settings → General → Discord ("Show on Discord"): what Kotora shows as Discord's Rich Presence.
//!
//! Off by default, and then nothing exists: no thread, no socket, no work in `poll` beyond one
//! atomic read. Turned on, one worker thread (`kotora_core::discord`) talks to the Discord app on
//! this computer; Kotora makes no connection of its own. [`sync`] runs at the end of every
//! `poll`, builds the wanted status from Now Playing ([`Input`] → [`activity`], both pure) and
//! hands it to the worker only when it changed. Quitting clears it ([`shutdown`]).
//!
//! The status: Watching for an episode ("Kagurabachi", "Episode 7 of 24", the player's start and end
//! times while it plays), Playing for a chapter ("Blue Lock", "Chapter 362": Discord has no reading
//! type). Large image: the cover (a plain https URL the service serves; Discord's servers fetch
//! it), else Kotora's icon. One button to the entry on the user's service.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use kotora_core::config::Discord as Opts;
use kotora_core::discord::{self as dc, Activity, Worker};
use kotora_core::recognize::Found;
use kotora_core::service::ServiceId;

use crate::model::State;

/// What is playing or being read, before the options decide what to show.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Watching { title: String, episode: Option<u32>, total: Option<u32>, media_id: Option<u32>, cover: Option<String>, playing: bool, position: Option<f64>, duration: Option<f64>, rate: f64 },
    /// A comic being read (reading detection: YACReader or an open file), see [`current_reading`].
    Reading { title: String, chapter: Option<u32>, volume: Option<u32>, media_id: Option<u32>, cover: Option<String> },
}

/// The link to the Discord app's page for an entry on the user's service.
pub fn entry_url(service: ServiceId, manga: bool, id: u32) -> String {
    let kind = if manga { "manga" } else { "anime" };
    match service {
        ServiceId::Mal => format!("https://myanimelist.net/{kind}/{id}"),
        ServiceId::AniList => format!("https://anilist.co/{kind}/{id}"),
    }
}

fn button_label(service: ServiceId) -> &'static str {
    match service {
        ServiceId::Mal => "View on MyAnimeList",
        ServiceId::AniList => "View on AniList",
    }
}

/// The status for `input` under the options; `None` shows nothing. `now`: unix seconds.
pub fn activity(input: &Input, o: &Opts, service: ServiceId, now: i64) -> Option<Activity> {
    let icon = || (dc::ICON_ASSET.to_string(), "Kotora".to_string());
    let (kind, title, state, media_id, cover, manga, times) = match input {
        Input::Watching { title, episode, total, media_id, cover, playing, position, duration, rate } => {
            if !playing && o.hide_paused {
                return None;
            }
            let mut state = match (episode, total) {
                (Some(e), Some(t)) if *t > 0 => Some(format!("Episode {e} of {t}")),
                (Some(e), _) => Some(format!("Episode {e}")),
                _ => None,
            };
            if !playing {
                state = Some(match state {
                    Some(s) => format!("{s} \u{b7} Paused"),
                    None => "Paused".into(),
                });
            }
            // Times only while it plays: elapsed and remaining, as the player has them.
            let times = (*playing).then_some(()).and_then(|_| {
                let (pos, rate) = (position.filter(|p| *p >= 0.0)?, if *rate > 0.0 { *rate } else { 1.0 });
                let start = now - (pos / rate) as i64;
                let end = duration.filter(|d| *d > pos).map(|d| now + ((d - pos) / rate) as i64);
                Some((start, end))
            });
            (3, title, state, *media_id, cover, false, times)
        }
        Input::Reading { title, chapter, volume, media_id, cover } => {
            let state = match (chapter, volume) {
                (Some(c), _) => Some(format!("Chapter {c}")),
                (None, Some(v)) => Some(format!("Volume {v}")),
                _ => None,
            };
            (0, title, state, *media_id, cover, true, None)
        }
    };
    let private = o.hide_title;
    let details = if private { if manga { "Reading manga".to_string() } else { "Watching anime".to_string() } } else { title.clone() };
    // Private: neither the title (hover text) nor the cover nor the link says what it is. The episode count stays.
    let cover = cover.as_deref().filter(|u| o.cover && !private && u.starts_with("https://") && u.len() <= 256);
    let (large, small) = match cover {
        Some(u) => (Some((u.to_string(), title.clone())), Some(icon())),
        None => (Some(icon()), None),
    };
    let button = media_id.filter(|_| o.buttons && !private).map(|id| (button_label(service).to_string(), entry_url(service, manga, id)));
    let (start, end) = match times {
        Some((s, e)) => (Some(s), e),
        None => (None, None),
    };
    Some(Activity { kind, details, state, start, end, large, small, button })
}

/// How long after a comic was last opened or its page saved it still counts as being read.
/// The readers say nothing when they close, so the status ends by itself after this.
pub const READING_FRESH_SECS: i64 = 30 * 60;

/// What is playing, or else what is being read. The episode wins when both would show.
pub fn current(s: &State) -> Option<Input> {
    current_at(s, kotora_core::time::now_unix())
}

fn current_at(s: &State, now: i64) -> Option<Input> {
    current_watching(s).or_else(|| current_reading(s, now))
}

/// The comic reading detection reports, while it is fresh and matched to a manga on the service.
fn current_reading(s: &State, now: i64) -> Option<Input> {
    let r = s.reading.current.as_ref()?;
    if !s.manga.enabled || now - r.read.at > READING_FRESH_SECS {
        return None;
    }
    let media_id = match &r.found {
        Found::List { media_id, .. } | Found::NotOnList { media_id, .. } => *media_id,
        _ => return None,
    };
    let title = s.reading_title(r);
    if title.is_empty() || (r.read.chapter.is_none() && r.read.volume.is_none()) {
        return None;
    }
    let cover = s.manga.entries.iter().find(|e| e.id == media_id).and_then(|e| e.cover_url.clone()).or_else(|| r.cover_url.clone());
    Some(Input::Reading { title, chapter: r.read.chapter, volume: r.read.volume, media_id: Some(media_id), cover })
}

/// What is playing, from the state: a recognised file (on the list or found by search).
fn current_watching(s: &State) -> Option<Input> {
    let n = s.now.as_ref().filter(|n| !n.gone)?;
    let media_id = match n.found {
        Found::List { media_id, .. } | Found::NotOnList { media_id, .. } => media_id,
        _ => return None,
    };
    let (_, title) = crate::ui::now_title(s, n);
    if title.is_empty() {
        return None;
    }
    let total = s.entries.iter().find(|e| e.id == media_id).and_then(|e| e.episodes);
    Some(Input::Watching {
        title,
        episode: n.episode(),
        total,
        media_id: Some(media_id),
        cover: crate::ui::now_cover_url(s, n).map(str::to_string),
        playing: n.playing,
        position: n.position_now(),
        duration: n.duration,
        rate: n.rate,
    })
}

// ----------------------------------------------------------------------------------- the worker

struct Link {
    worker: Worker,
    client_id: String,
    /// What the worker was last given (`None` for nothing; the outer `None`: not yet).
    last: Option<Option<Activity>>,
}

static LINK: Mutex<Option<Link>> = Mutex::new(None);
/// The one thing `poll` reads while the switch is off.
static ON: AtomicBool = AtomicBool::new(false);

/// How long a quit waits for Discord to take the status down.
const QUIT_WAIT: Duration = Duration::from_millis(400);

/// Whether the Application ID in use is still the placeholder: nothing is started then.
pub fn needs_client_id(o: &Opts) -> bool {
    dc::is_placeholder(&dc::client_id(&o.client_id))
}

/// Starts or stops the worker for the options. Never headless (no handle: the callers check).
pub fn apply(o: &Opts) {
    let id = dc::client_id(&o.client_id);
    let on = o.enabled && !dc::is_placeholder(&id);
    let mut link = LINK.lock().unwrap_or_else(|e| e.into_inner());
    if link.as_ref().is_some_and(|l| !on || l.client_id != id)
        && let Some(old) = link.take()
    {
        // Off the UI thread: the status comes down, then the thread ends.
        let _ = std::thread::Builder::new().name("kotora-discord-stop".into()).spawn(move || old.worker.stop(Duration::from_secs(3)));
    }
    if on && link.is_none() {
        *link = Some(Link { worker: Worker::start(id.clone(), dc::system_connector(), dc::Timing::default()), client_id: id, last: None });
    }
    // The next sync sends the status again (an option changed what it says).
    if let Some(l) = link.as_mut() {
        l.last = None;
    }
    ON.store(link.is_some(), Ordering::Relaxed);
}

/// Quit: clears the status (a short wait) and ends the worker.
pub fn shutdown() {
    ON.store(false, Ordering::Relaxed);
    let old = LINK.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(l) = old {
        l.worker.stop(QUIT_WAIT);
    }
}

/// At the end of `poll`: gives the worker the status when it changed. Off: one atomic read.
pub fn sync(s: &State) {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    let want = current(s).and_then(|i| activity(&i, &s.prefs.discord, s.service, kotora_core::time::now_unix()));
    let mut link = LINK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(l) = link.as_mut() else { return };
    let same = match (&l.last, &want) {
        (Some(None), None) => true,
        (Some(Some(a)), Some(b)) => a.same_as(b, 3),
        _ => false,
    };
    // Nothing to clear before anything was shown.
    if same || (l.last.is_none() && want.is_none()) {
        l.last.get_or_insert(None);
        return;
    }
    l.worker.set(want.clone());
    l.last = Some(want);
}

impl State {
    /// An option of Settings → General → Discord changed.
    fn discord_changed(&mut self) {
        let o = self.prefs.discord.clone();
        if self.handle.is_some() {
            apply(&o);
            sync(self);
        }
    }

    pub fn set_discord(&mut self, on: bool) {
        self.prefs.discord.enabled = on;
        crate::detect::save_setting(self, "discord", "enabled", on.to_string());
        self.discord_changed();
    }

    pub fn set_discord_cover(&mut self, on: bool) {
        self.prefs.discord.cover = on;
        crate::detect::save_setting(self, "discord", "cover", on.to_string());
        self.discord_changed();
    }

    pub fn set_discord_buttons(&mut self, on: bool) {
        self.prefs.discord.buttons = on;
        crate::detect::save_setting(self, "discord", "buttons", on.to_string());
        self.discord_changed();
    }

    pub fn set_discord_hide_paused(&mut self, on: bool) {
        self.prefs.discord.hide_paused = on;
        crate::detect::save_setting(self, "discord", "hide_paused", on.to_string());
        self.discord_changed();
    }

    pub fn set_discord_hide_title(&mut self, on: bool) {
        self.prefs.discord.hide_title = on;
        crate::detect::save_setting(self, "discord", "hide_title", on.to_string());
        self.discord_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watching(playing: bool) -> Input {
        Input::Watching { title: "Kagurabachi".into(), episode: Some(7), total: Some(24), media_id: Some(61), cover: Some("https://cdn.myanimelist.net/images/anime/1/2.jpg".into()), playing, position: Some(300.0), duration: Some(1400.0), rate: 1.0 }
    }

    fn on() -> Opts {
        Opts { enabled: true, ..Opts::default() }
    }

    #[test]
    fn an_episode_playing() {
        let a = activity(&watching(true), &on(), ServiceId::AniList, 10_000).unwrap();
        assert_eq!((a.kind, a.details.as_str(), a.state.as_deref()), (3, "Kagurabachi", Some("Episode 7 of 24")));
        assert_eq!((a.start, a.end), (Some(9_700), Some(11_100)), "elapsed and remaining from the player");
        assert_eq!(a.large.as_ref().unwrap().0, "https://cdn.myanimelist.net/images/anime/1/2.jpg");
        assert_eq!(a.small.as_ref().unwrap().0, dc::ICON_ASSET);
        assert_eq!(a.button, Some(("View on AniList".into(), "https://anilist.co/anime/61".into())));
        let m = activity(&watching(true), &on(), ServiceId::Mal, 0).unwrap();
        assert_eq!(m.button.unwrap(), ("View on MyAnimeList".into(), "https://myanimelist.net/anime/61".into()));
    }

    #[test]
    fn speed_and_unknown_length() {
        let Input::Watching { .. } = watching(true) else { unreachable!() };
        let fast = match watching(true) {
            Input::Watching { title, episode, total, media_id, cover, playing, .. } => Input::Watching { title, episode, total, media_id, cover, playing, position: Some(300.0), duration: Some(1400.0), rate: 2.0 },
            r => r,
        };
        let a = activity(&fast, &on(), ServiceId::Mal, 10_000).unwrap();
        assert_eq!((a.start, a.end), (Some(9_850), Some(10_550)));
        let open = match watching(true) {
            Input::Watching { title, episode, media_id, cover, playing, .. } => Input::Watching { title, episode, total: None, media_id, cover, playing, position: Some(60.0), duration: None, rate: 1.0 },
            r => r,
        };
        let a = activity(&open, &on(), ServiceId::Mal, 1_000).unwrap();
        assert_eq!((a.start, a.end, a.state.as_deref()), (Some(940), None, Some("Episode 7")));
    }

    #[test]
    fn paused_shows_no_clock_or_hides() {
        let a = activity(&watching(false), &on(), ServiceId::Mal, 10_000).unwrap();
        assert_eq!((a.start, a.end, a.state.as_deref()), (None, None, Some("Episode 7 of 24 \u{b7} Paused")));
        let hide = Opts { hide_paused: true, ..on() };
        assert!(activity(&watching(false), &hide, ServiceId::Mal, 0).is_none());
        assert!(activity(&watching(true), &hide, ServiceId::Mal, 0).is_some());
    }

    #[test]
    fn privacy_and_options() {
        let p = activity(&watching(true), &Opts { hide_title: true, ..on() }, ServiceId::Mal, 0).unwrap();
        assert_eq!(p.details, "Watching anime");
        assert_eq!(p.large.as_ref().unwrap(), &(dc::ICON_ASSET.to_string(), "Kotora".to_string()), "no cover, no title in the hover text");
        assert!(p.small.is_none() && p.button.is_none());
        let json = p.to_json().to_string();
        assert!(!json.contains("Kagurabachi") && !json.contains("myanimelist") && !json.contains("61"), "{json}");
        let q = activity(&watching(true), &Opts { cover: false, buttons: false, ..on() }, ServiceId::Mal, 0).unwrap();
        assert_eq!(q.large.unwrap().0, dc::ICON_ASSET);
        assert!(q.button.is_none() && q.small.is_none());
        // A cover that isn't https, or is too long, is not sent.
        let odd = match watching(true) {
            Input::Watching { title, episode, total, media_id, playing, position, duration, rate, .. } => Input::Watching { title, episode, total, media_id, cover: Some("http://x/y.jpg".into()), playing, position, duration, rate },
            r => r,
        };
        assert_eq!(activity(&odd, &on(), ServiceId::Mal, 0).unwrap().large.unwrap().0, dc::ICON_ASSET);
    }

    #[test]
    fn a_chapter() {
        let r = Input::Reading { title: "Blue Lock".into(), chapter: Some(362), volume: None, media_id: Some(99), cover: None };
        let a = activity(&r, &on(), ServiceId::AniList, 5).unwrap();
        assert_eq!((a.kind, a.details.as_str(), a.state.as_deref()), (0, "Blue Lock", Some("Chapter 362")));
        assert_eq!(a.button.unwrap().1, "https://anilist.co/manga/99");
        assert!(a.start.is_none());
        let p = activity(&r, &Opts { hide_title: true, ..on() }, ServiceId::Mal, 5).unwrap();
        assert_eq!(p.details, "Reading manga");
    }

    #[test]
    fn what_plays_comes_from_now_playing() {
        let s = State::fake();
        match current(&s) {
            Some(Input::Watching { title, .. }) => assert!(!title.is_empty()),
            other => assert!(s.now.is_none() || other.is_none(), "{other:?}"),
        }
        let mut s = State::fake();
        s.now = None;
        assert!(current(&s).is_none(), "nothing plays");
        if let Some(n) = State::fake().now {
            let mut s = State::fake();
            s.now = Some(crate::now::Now { gone: true, ..n });
            assert!(current(&s).is_none(), "a player that is gone shows nothing");
        }
    }

    fn reading_state() -> State {
        let mut s = State::fake();
        crate::manga::fake(&mut s);
        s.now = None;
        crate::reading::fake(&mut s, "Inkwell Cartographers v12 c120.cbz", 14);
        s.reading.current.as_mut().unwrap().read.at = 1_000;
        s
    }

    #[test]
    fn a_comic_being_read_is_shown() {
        let s = reading_state();
        let Some(Input::Reading { title, chapter, volume, media_id, .. }) = current_at(&s, 1_100) else { panic!("{:?}", current_at(&s, 1_100)) };
        assert_eq!((title.as_str(), chapter, volume, media_id), ("Inkwell Cartographers", Some(120), Some(12), Some(1)));
        let a = activity(&current_at(&s, 1_100).unwrap(), &on(), ServiceId::Mal, 1_100).unwrap();
        assert_eq!((a.kind, a.details.as_str(), a.state.as_deref()), (0, "Inkwell Cartographers", Some("Chapter 120")));
        assert_eq!(a.button.unwrap().1, "https://myanimelist.net/manga/1");
    }

    #[test]
    fn reading_ends_when_it_goes_stale_or_is_cleared() {
        let mut s = reading_state();
        assert!(current_at(&s, 1_000 + READING_FRESH_SECS).is_some());
        assert!(current_at(&s, 1_001 + READING_FRESH_SECS).is_none(), "no signal for 30 minutes: reading stopped");
        s.manga.enabled = false;
        assert!(current_at(&s, 1_100).is_none(), "Manga mode off");
        s.manga.enabled = true;
        s.reading.current = None;
        assert!(current_at(&s, 1_100).is_none(), "the watch stopped or was reset");
        // A file that matches nothing shows nothing.
        crate::reading::fake(&mut s, "Nothing Known Anywhere c5.cbz", 14);
        s.reading.current.as_mut().unwrap().read.at = 1_000;
        assert!(current_at(&s, 1_100).is_none());
    }

    #[test]
    fn the_episode_wins_over_the_comic() {
        let mut s = reading_state();
        s.now = State::fake().now;
        match current_at(&s, 1_100) {
            Some(Input::Watching { .. }) => {}
            other => assert!(s.now.as_ref().is_none_or(|n| n.gone) || current_watching(&s).is_none(), "{other:?}"),
        }
        let w = current_watching(&s);
        if w.is_some() {
            assert!(matches!(current_at(&s, 1_100), Some(Input::Watching { .. })));
        }
    }

    #[test]
    fn off_means_nothing_runs() {
        // The default: no worker, and `sync` returns at the atomic read.
        assert!(!Opts::default().enabled);
        let s = State::fake();
        sync(&s);
        assert!(LINK.lock().unwrap().is_none());
        // The built-in id is a real Application ID, so a fresh install never asks for one.
        assert!(!needs_client_id(&on()));
        shutdown();
    }
}
