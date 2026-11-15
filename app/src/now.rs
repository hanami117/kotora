//! Now Playing: what the followed player shows, what recognition made of it, and the offer
//! (DESIGN.md §5.5, step 5). The detection threads are in `detect.rs`; this is the UI-thread side.
//!
//! The flow: a file passes the threshold ([`Msg::Reached`](crate::model::Msg)) → the banner asks
//! "Update to N?" and a desktop notification says the same. Update (or `kot confirm`) queues the
//! progress through the sync queue, like `+1`. With "Update automatically" on (off by default),
//! a sure match is queued at once and the banner offers Undo; an unsure one still asks.

use kotora_core::recognize::{Found, Parsed};
use kotora_core::{Change, Status};
use kotora_detect::{PlayState, SourceStatus, Switch, Track};

use crate::model::{Entry, Page, State};

/// The followed player and its file.
#[derive(Clone, Debug, PartialEq)]
pub struct Now {
    pub key: String,
    pub player: String,
    /// "IPC socket", "MPRIS", "web interface".
    pub via: &'static str,
    /// The file name, or the player's title for a stream.
    pub file: String,
    pub position: Option<f64>,
    pub duration: Option<f64>,
    /// When `position` was true (`kotora_detect::clock`), and the speed: the position is worked
    /// out from the clock whenever it is drawn or asked for. While it plays and shows, `tick` redraws it
    /// once a second. `None` in scenes.
    pub at: Option<f64>,
    pub rate: f64,
    pub playing: bool,
    /// Passed the threshold (or stopped after half).
    pub reached: bool,
    pub parsed: Option<Parsed>,
    pub found: Found,
    /// Recognition still runs (a search can take a second).
    pub recognizing: bool,
    /// The closest titles on the list, for "Wrong anime…".
    pub candidates: Vec<(u32, String)>,
    /// The cover of a match that isn't on the list (a list entry has its own).
    pub cover_url: Option<String>,
    /// Why a match that isn't on the list is another anime than the file's (anime-relations).
    pub relation: Option<String>,
    /// "Not now".
    pub dismissed: bool,
    /// The banner acted already (notified, or recorded automatically).
    pub acted: bool,
    pub recorded: Option<Recorded>,
    /// "Wrong anime…" is open.
    pub picking: bool,
}

/// An episode recorded from here, for the banner's Undo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recorded {
    pub id: u32,
    pub episode: u32,
    pub old_progress: u32,
    pub old_status: Status,
    /// The episode was the last one: the entry is now Completed.
    pub completed: bool,
    pub auto: bool,
}

/// What the detection glue sends with each report of the followed player.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub track: Track,
    pub parsed: Option<Parsed>,
    pub found: Found,
    pub candidates: Vec<(u32, String)>,
    /// The cover of an anime found on MyAnimeList but not on the list (saved details).
    pub cover_url: Option<String>,
    /// anime-relations moved the file to an anime that isn't on the list: why, in words.
    pub relation: Option<String>,
}

/// Settings → Updates and Players, as `config.toml` has them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub auto: bool,
    pub threshold: u8,
    pub keep_higher: bool,
    pub players: kotora_core::config::Players,
    /// macOS: VLC's web password is set.
    pub vlc_set_up: bool,
    /// The "precise detection" offer for mpv was answered (`[notices] mpv_precise`).
    pub precise_answered: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        let u = kotora_core::config::Updates::default();
        Prefs { auto: u.auto, threshold: u.threshold, keep_higher: u.keep_higher_progress, players: Default::default(), vlc_set_up: false, precise_answered: false }
    }
}

impl Prefs {
    pub fn from_config(c: &kotora_core::config::Config) -> Prefs {
        Prefs {
            auto: c.updates.auto,
            threshold: c.updates.threshold,
            keep_higher: c.updates.keep_higher_progress,
            players: c.players.clone(),
            vlc_set_up: !c.vlc.password.is_empty(),
            precise_answered: c.notices.mpv_precise,
        }
    }
}

/// The thresholds the Settings control cycles through.
pub const THRESHOLDS: [u8; 4] = [70, 80, 90, 100];

/// The episode to offer for an entry on the list (as `recognize::offer`).
pub fn offer_for(e: &Entry, episode: Option<u32>) -> Option<u32> {
    let ep = episode.or((e.episodes == Some(1)).then_some(1))?;
    if ep == 0 || e.episodes.is_some_and(|n| ep > n) || (ep <= e.progress && !e.rewatching) {
        return None;
    }
    Some(ep)
}

/// "18:52".
pub fn clock_text(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

impl Now {
    pub fn from_seen(seen: Seen) -> Now {
        let p = &seen.track.playing;
        Now {
            key: p.key.clone(),
            player: p.player.clone(),
            via: p.via.label(),
            file: p.file().map(file_name).unwrap_or_default(),
            position: p.position,
            duration: p.duration,
            at: Some(p.at),
            rate: p.rate,
            playing: p.state == PlayState::Playing,
            reached: seen.track.reached,
            parsed: seen.parsed,
            found: seen.found,
            recognizing: false,
            candidates: seen.candidates,
            cover_url: seen.cover_url,
            relation: seen.relation,
            dismissed: false,
            acted: false,
            recorded: None,
            picking: false,
        }
    }

    /// The position now: the last report plus the time since, while playing.
    pub fn position_now(&self) -> Option<f64> {
        let p = self.position?;
        let p = match self.at {
            Some(at) if self.playing => p + (kotora_detect::clock() - at).max(0.0) * self.rate,
            _ => p,
        };
        Some(self.duration.map_or(p, |d| p.min(d)))
    }

    /// What the screen shows of the position now: the whole second on the Now Playing page
    /// (`seconds`), else the sidebar's rounded percent. `None` with no position (or no length,
    /// for the percent).
    pub fn shown(&self, seconds: bool) -> Option<(bool, u64)> {
        let p = self.position_now()?;
        if seconds {
            return Some((true, p as u64));
        }
        let d = self.duration.filter(|d| *d > 0.0)?;
        Some((false, (p / d * 100.0).round() as u64))
    }

    /// Seconds from now until [`Now::shown`] next changes, only while the file plays and its
    /// position is known (not for paused, idle or process-only sources). The seconds are
    /// aligned to the player's own, and never come faster than once a second.
    pub fn next_change(&self, seconds: bool) -> Option<f64> {
        if !self.playing || self.at.is_none() || self.rate <= 0.0 {
            return None;
        }
        let p = self.position_now()?;
        let end = self.duration.filter(|d| *d > 0.0);
        if end.is_some_and(|d| p >= d) {
            return None;
        }
        let next = if seconds {
            p.floor() + 1.0
        } else {
            // The percent is rounded: it turns over half way between two.
            let d = end?;
            ((p / d * 100.0 + 0.5).floor() + 0.5) / 100.0 * d
        };
        if end.is_some_and(|d| next > d) {
            return None;
        }
        let wait = (next - p) / self.rate;
        // A hair past the turn, so the draw shows the new value.
        Some(if seconds && self.rate > 1.0 { wait.max(1.0) } else { wait } + 0.002)
    }

    /// 0–1 watched, by position.
    pub fn fraction(&self) -> Option<f32> {
        let (p, d) = (self.position_now()?, self.duration.filter(|d| *d > 0.0)?);
        Some((p / d).clamp(0.0, 1.0) as f32)
    }

    /// The matched entry's id.
    pub fn media_id(&self) -> Option<u32> {
        match self.found {
            Found::List { media_id, .. } => Some(media_id),
            _ => None,
        }
    }

    pub fn sure(&self) -> bool {
        matches!(self.found, Found::List { sure: true, .. })
    }

    pub fn episode(&self) -> Option<u32> {
        self.parsed.as_ref().and_then(|p| p.episode)
    }
}

/// Runs in `poll`: while a followed player is playing with a known position and a window shows
/// it, asks for the next poll when the shown value turns over (the whole second on the Now
/// Playing page, else the sidebar's percent), and says whether it changed. Paused, idle, hidden
/// or process-only: no wake at all. Showing the window, a page change or a player report all
/// run `poll` first, so the timer starts again by itself.
pub fn tick(s: &mut State, tx: &mut rune::Tx) -> bool {
    let seconds = s.page == Page::NowPlaying;
    let next = if s.on_screen() { s.now.as_ref().and_then(|n| Some((n.shown(seconds)?, n.next_change(seconds)?))) } else { None };
    let Some((sig, wait)) = next else {
        s.tick = None;
        return false;
    };
    tx.wake_after(std::time::Duration::from_secs_f64(wait));
    s.tick.replace(sig) != Some(sig)
}

fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

impl State {
    /// A window shows (not hidden). No handle (scenes, tests): `window` says.
    fn on_screen(&self) -> bool {
        self.handle.as_ref().map_or(self.window, |h| h.is_visible())
    }

    /// The entry and the episode the banner offers now, if any.
    pub fn offer(&self) -> Option<(usize, u32)> {
        let now = self.now.as_ref()?;
        let id = now.media_id()?;
        let i = self.entries.iter().position(|e| e.id == id)?;
        let ep = offer_for(&self.entries[i], now.episode())?;
        Some((i, ep))
    }

    /// The banner shows: an episode recorded from here (with Undo), or an offer once the file
    /// counts as watched.
    pub fn banner(&self) -> bool {
        let Some(now) = &self.now else { return false };
        if now.dismissed || !self.has_list() {
            return false;
        }
        now.recorded.is_some() || (now.reached && self.offer().is_some())
    }

    /// A report about the followed player. Keeps the banner's state while the file stays.
    pub fn now_seen(&mut self, seen: Option<Seen>) -> bool {
        let Some(seen) = seen else {
            let changed = self.now.is_some();
            // A recorded episode keeps its Undo banner until dismissed.
            if self.now.as_ref().is_some_and(|n| n.recorded.is_some() && !n.dismissed) {
                if let Some(n) = &mut self.now {
                    n.playing = false;
                }
                return true;
            }
            self.now = None;
            return changed;
        };
        let new = Now::from_seen(seen);
        match &mut self.now {
            Some(old) if old.key == new.key && old.file == new.file => {
                let (dismissed, acted, recorded, picking, reached) = (old.dismissed, old.acted, old.recorded, old.picking, old.reached);
                *old = Now { dismissed, acted, recorded, picking, reached: reached || new.reached, ..new };
            }
            _ => self.now = Some(new),
        }
        self.act();
        true
    }

    /// The file passed the threshold.
    pub fn now_reached(&mut self, key: &str, file: &str) -> bool {
        let Some(now) = &mut self.now else { return false };
        if now.key != key || now.file != file_name(file) {
            return false;
        }
        now.reached = true;
        self.act();
        true
    }

    /// Once reached and recognised: record (auto, sure) or ask with a notification. Once per file.
    fn act(&mut self) {
        let Some(now) = &self.now else { return };
        if !now.reached || now.acted || now.recognizing || now.dismissed || !self.has_list() {
            return;
        }
        let Some((i, ep)) = self.offer() else { return };
        let title = self.entries[i].title.clone();
        // A player seen by its open file only knows no position or pause: the time open is a
        // guess, so it always asks, even with "Update automatically" on.
        let auto = self.prefs.auto && now.sure() && now.via != crate::precise::VIA_PROCESS;
        if let Some(n) = &mut self.now {
            n.acted = true;
        }
        if auto {
            if self.record(true).is_ok() {
                crate::detect::notify(&format!("Recorded {title} episode {ep}"), "Kotora updated your list. Undo is in the banner.", self.handle.is_some());
            }
        } else {
            let q = if self.now.as_ref().is_some_and(Now::sure) { "Update" } else { "Not sure: update" };
            crate::detect::notify(&format!("{q} {title} to episode {ep}?"), "Open Kotora, or run kot confirm.", self.handle.is_some());
        }
    }

    /// Update (the banner, `kot confirm`): the offered episode goes to the queue.
    pub fn record(&mut self, auto: bool) -> Result<(String, u32), String> {
        let (i, ep) = self.offer().ok_or("Nothing to update: no recognised new episode is playing.")?;
        let e = &self.entries[i];
        let (id, title, old_progress, old_status) = (e.id, e.title.clone(), e.progress, e.status);
        // A show you planned to watch is now one you watch. The last episode completes it instead
        // (`edit` does that when the total is known).
        let last = e.episodes.is_some_and(|n| n > 0 && ep == n);
        let status = (old_status == Status::PlanToWatch && !last).then_some(Status::Watching);
        self.edit(id, Change { progress: Some(ep), status, ..Default::default() })?;
        let completed = old_status != Status::Completed && self.entries.iter().any(|e| e.id == id && e.status == Status::Completed);
        if let Some(n) = &mut self.now {
            n.recorded = Some(Recorded { id, episode: ep, old_progress, old_status, completed, auto });
            n.acted = true;
            n.picking = false;
        }
        Ok((title, ep))
    }

    /// Undo on the banner: the entry goes back (the queue drops the change if it hasn't left).
    pub fn undo_record(&mut self) {
        let Some(r) = self.now.as_ref().and_then(|n| n.recorded) else { return };
        let _ = self.edit(r.id, Change { progress: Some(r.old_progress), status: Some(r.old_status), ..Default::default() });
        if let Some(n) = &mut self.now {
            n.recorded = None;
            n.dismissed = true;
        }
    }

    /// Not now (the banner, `kot dismiss`).
    pub fn dismiss_now(&mut self) -> bool {
        match &mut self.now {
            Some(n) if !n.dismissed => {
                n.dismissed = true;
                n.picking = false;
                true
            }
            _ => false,
        }
    }

    /// "Wrong anime…": the picker on the Now Playing page.
    pub fn wrong_anime(&mut self) {
        if let Some(n) = &mut self.now {
            n.picking = true;
        }
        self.go(crate::model::Page::NowPlaying);
    }

    /// The user's pick: remembered for this title (and season), and the match is sure.
    pub fn pick_anime(&mut self, media_id: u32) {
        let Some(n) = &mut self.now else { return };
        n.found = Found::List { media_id, sure: true, why: "you picked it".into() };
        n.picking = false;
        n.dismissed = false;
        n.acted = false;
        let key = n.parsed.as_ref().map(Parsed::key);
        if let Some(key) = key {
            crate::detect::correct(self, key, Some(media_id));
        }
        self.act();
    }

    /// "Not an anime": this title is ignored from now on.
    pub fn ignore_file(&mut self) {
        let Some(n) = &mut self.now else { return };
        n.found = Found::Ignored;
        n.picking = false;
        n.dismissed = true;
        let key = n.parsed.as_ref().map(Parsed::key);
        if let Some(key) = key {
            crate::detect::correct(self, key, None);
        }
    }

    /// The sources' states (Settings, the Now Playing page).
    pub fn players_seen(&mut self, sources: Vec<SourceStatus>) -> bool {
        let sources: Vec<crate::precise::PlayerState> = sources.into_iter().map(Into::into).collect();
        if self.players == sources {
            return false;
        }
        self.players = sources;
        true
    }

    /// What plays, as `kot now` prints it.
    pub fn ctl_now(&self) -> Option<kotora_ctl::NowInfo> {
        let n = self.now.as_ref()?;
        let title = match (n.media_id().and_then(|id| self.entries.iter().find(|e| e.id == id)), &n.found) {
            (Some(e), _) => e.title.clone(),
            (None, Found::NotOnList { title, .. }) => title.clone(),
            _ => n.parsed.as_ref().map(|p| p.title.clone()).unwrap_or_else(|| n.file.clone()),
        };
        let matched = match &n.found {
            Found::List { sure: true, why, .. } => format!("In your list, sure: {why}"),
            Found::List { why, .. } => format!("In your list, not sure: {why}"),
            Found::NotOnList { .. } => match &n.relation {
                Some(why) => format!("Not on your list: {why}"),
                None => "Not on your list (found on MyAnimeList)".into(),
            },
            Found::Ignored => "Marked as not an anime".into(),
            Found::Nothing => "Not found on your list".into(),
        };
        Some(kotora_ctl::NowInfo {
            player: n.player.clone(),
            via: n.via.into(),
            file: n.file.clone(),
            playing: n.playing,
            percent: n.fraction().map(|f| (f * 100.0).round() as u8),
            title,
            episode: n.episode(),
            matched,
            offer: if n.recorded.is_some() { None } else { self.offer().map(|(_, ep)| ep) },
            reached: n.reached,
            recorded: n.recorded.map(|r| r.episode),
            dismissed: n.dismissed,
        })
    }

    // ------------------------------------------------------------------------------- settings

    pub fn set_auto(&mut self, on: bool) {
        self.prefs.auto = on;
        crate::detect::save_setting(self, "updates", "auto", on.to_string());
    }

    pub fn next_threshold(&mut self) {
        let i = THRESHOLDS.iter().position(|t| *t == self.prefs.threshold).map_or(1, |i| (i + 1) % THRESHOLDS.len());
        self.prefs.threshold = THRESHOLDS[i];
        crate::detect::save_setting(self, "updates", "threshold", THRESHOLDS[i].to_string());
    }

    pub fn set_keep_higher(&mut self, on: bool) {
        self.prefs.keep_higher = on;
        self.worker.ask_on_higher.store(!on, std::sync::atomic::Ordering::Relaxed);
        crate::detect::save_setting(self, "updates", "keep_higher_progress", on.to_string());
    }

    pub fn set_process_scan(&mut self, on: bool) {
        self.prefs.players.process_scan = on;
        crate::detect::save_setting(self, "players", "process_scan", on.to_string());
    }

    /// Settings' minutes stepper: 1, 5, 10, … 180.
    pub fn step_process_minutes(&mut self, up: bool) {
        let m = self.prefs.players.process_minutes.clamp(1, 180);
        let next = if up { if m < 5 { 5 } else { m + 5 } } else if m <= 5 { 1 } else { m - 5 };
        let next = next.clamp(1, 180);
        if next == m {
            return;
        }
        self.prefs.players.process_minutes = next;
        crate::detect::save_setting(self, "players", "process_minutes", next.to_string());
    }

    pub fn set_player(&mut self, sw: Switch, on: bool) {
        self.prefs.players.set(sw.config_key(), on);
        crate::detect::save_setting(self, "players", sw.config_key(), on.to_string());
    }

    /// The source line for a switch: "IPC socket · playing".
    pub fn player_line(&self, sw: Switch) -> (String, kotora_detect::Led) {
        let mine: Vec<&crate::precise::PlayerState> = self.players.iter().filter(|s| s.switch == sw).collect();
        // The liveliest source; on a tie (both not running, say) the one that knows the position
        // (IPC socket, MPRIS, web interface) over the process scan.
        let best = mine.iter().max_by_key(|s| {
            let led = match s.led {
                kotora_detect::Led::Playing => 3,
                kotora_detect::Led::Idle => 2,
                kotora_detect::Led::Problem => 1,
                kotora_detect::Led::Off => 0,
            };
            (led, s.via != crate::precise::VIA_PROCESS)
        });
        match best {
            Some(s) => (format!("{} \u{b7} {}", s.via, s.line), s.led),
            None => (
                match sw {
                    Switch::Vlc if cfg!(target_os = "macos") && !self.prefs.vlc_set_up => "web interface \u{b7} not set up (needs VLC's password)".into(),
                    Switch::Mpv | Switch::Iina => "IPC socket \u{b7} not running".into(),
                    _ => "MPRIS \u{b7} not running".into(),
                },
                kotora_detect::Led::Off,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FAKE_NOW, Page};

    fn fresh() -> State {
        let mut s = State::fake();
        s.now = None;
        s
    }

    fn seen(file: &str, media_id: Option<u32>, sure: bool, reached: bool) -> Seen {
        let playing = kotora_detect::Playing {
            key: "mpv:/tmp/mpv.sock".into(),
            player: "mpv".into(),
            switch: Switch::Mpv,
            via: kotora_detect::Via::MpvIpc,
            path: Some(format!("/v/{file}")),
            title: None,
            position: Some(1000.0),
            duration: Some(1420.0),
            state: PlayState::Playing,
            rate: 1.0,
            at: kotora_detect::clock(),
        };
        Seen {
            track: Track { playing, played: 900.0, reached },
            parsed: kotora_core::recognize::parse(&format!("/v/{file}")),
            found: match media_id {
                Some(id) => Found::List { media_id: id, sure, why: "test".into() },
                None => Found::Nothing,
            },
            candidates: vec![],
            cover_url: None,
            relation: None,
        }
    }

    #[test]
    fn the_banner_asks_once_the_episode_counts() {
        let mut s = fresh();
        // Lanterns of Kirigaoka (id 0) is at 5 of 12.
        s.now_seen(Some(seen("Lanterns of Kirigaoka - 06.mkv", Some(0), true, false)));
        assert_eq!(s.offer().map(|(_, ep)| ep), Some(6));
        assert!(!s.banner(), "not before the threshold");
        assert!(s.now_reached("mpv:/tmp/mpv.sock", "/v/Lanterns of Kirigaoka - 06.mkv"));
        assert!(s.banner());
        assert!(s.now.as_ref().unwrap().acted);
        assert_eq!(s.entries.iter().find(|e| e.id == 0).unwrap().progress, 5, "asking changes nothing");
        // Another report of the same file keeps the state.
        s.now_seen(Some(seen("Lanterns of Kirigaoka - 06.mkv", Some(0), true, false)));
        assert!(s.banner() && s.now.as_ref().unwrap().reached);
        // Update: queued like +1.
        let (title, ep) = s.record(false).unwrap();
        assert_eq!((title.as_str(), ep), ("Lanterns of Kirigaoka", 6));
        let e = s.entries.iter().find(|e| e.id == 0).unwrap();
        assert_eq!((e.progress, e.waiting), (6, Some("Waiting to sync")));
        assert!(s.banner(), "Undo shows");
        s.undo_record();
        assert_eq!(s.entries.iter().find(|e| e.id == 0).unwrap().progress, 5);
        assert!(!s.banner());
    }

    #[test]
    fn auto_records_sure_matches_only() {
        let mut s = fresh();
        s.prefs.auto = true;
        s.now_seen(Some(seen("Lanterns of Kirigaoka - 06.mkv", Some(0), true, true)));
        assert_eq!(s.entries.iter().find(|e| e.id == 0).unwrap().progress, 6);
        assert!(s.now.as_ref().unwrap().recorded.is_some_and(|r| r.auto));

        let mut s = fresh();
        s.prefs.auto = true;
        s.now_seen(Some(seen("Lantern of Kirigaoka - 06.mkv", Some(0), false, true)));
        assert_eq!(s.entries.iter().find(|e| e.id == 0).unwrap().progress, 5, "unsure: asks");
        assert!(s.banner());

        // Seen by its open file only (no position or pause): a sure match still asks.
        let mut s = fresh();
        s.prefs.auto = true;
        let mut v = seen("Lanterns of Kirigaoka - 06.mkv", Some(0), true, true);
        v.track.playing.via = kotora_detect::Via::Process;
        v.track.playing.position = None;
        v.track.playing.duration = None;
        s.now_seen(Some(v));
        assert_eq!(s.entries.iter().find(|e| e.id == 0).unwrap().progress, 5, "process: asks");
        assert!(s.banner() && s.now.as_ref().unwrap().recorded.is_none());
    }

    #[test]
    fn the_source_that_knows_the_position_wins_a_tie() {
        use crate::precise::{PlayerState, VIA_IPC, VIA_PROCESS};
        use kotora_detect::Led;
        let p = |via, led| PlayerState { switch: Switch::Mpv, via, line: "not running".into(), led };
        let mut s = fresh();
        s.players = vec![p(VIA_IPC, Led::Off), p(VIA_PROCESS, Led::Off)];
        assert!(s.player_line(Switch::Mpv).0.starts_with("IPC socket"));
        s.players = vec![p(VIA_PROCESS, Led::Off), p(VIA_IPC, Led::Off)];
        assert!(s.player_line(Switch::Mpv).0.starts_with("IPC socket"));
        s.players = vec![p(VIA_IPC, Led::Off), p(VIA_PROCESS, Led::Playing)];
        assert!(s.player_line(Switch::Mpv).0.starts_with("process"));
    }

    #[test]
    fn process_minutes_step_within_1_to_180() {
        let mut s = fresh();
        assert_eq!(s.prefs.players.process_minutes, 15);
        s.step_process_minutes(false);
        s.step_process_minutes(false);
        assert_eq!(s.prefs.players.process_minutes, 5);
        s.step_process_minutes(false);
        s.step_process_minutes(false);
        assert_eq!(s.prefs.players.process_minutes, 1);
        s.step_process_minutes(true);
        assert_eq!(s.prefs.players.process_minutes, 5);
        s.prefs.players.process_minutes = 178;
        s.step_process_minutes(true);
        assert_eq!(s.prefs.players.process_minutes, 180);
        s.step_process_minutes(true);
        assert_eq!(s.prefs.players.process_minutes, 180);
    }

    #[test]
    fn no_offer_for_old_episodes_unknown_files_or_signed_out() {
        let mut s = fresh();
        s.now_seen(Some(seen("Lanterns of Kirigaoka - 05.mkv", Some(0), true, true)));
        assert!(s.offer().is_none() && !s.banner());
        s.now_seen(Some(seen("Something - 01.mkv", None, false, true)));
        assert!(!s.banner());
        assert!(s.record(false).is_err());
        let mut s = fresh();
        s.auth = crate::model::Auth::SignedOut;
        s.now_seen(Some(seen("Lanterns of Kirigaoka - 06.mkv", Some(0), true, true)));
        assert!(!s.banner());
    }

    #[test]
    fn not_now_and_wrong_anime() {
        let mut s = fresh();
        s.now_seen(Some(seen("Lanterns of Kirigaoka - 06.mkv", Some(0), false, true)));
        assert!(s.dismiss_now() && !s.banner());
        assert!(!s.dismiss_now());
        s.wrong_anime();
        assert_eq!(s.page, Page::NowPlaying);
        assert!(s.now.as_ref().unwrap().picking);
        // Picking another entry (id 4, Kumo to Kaze no Ryokan) makes it sure.
        s.pick_anime(4);
        let n = s.now.as_ref().unwrap();
        assert!(n.sure() && !n.picking && n.media_id() == Some(4));
        s.ignore_file();
        assert_eq!(s.now.as_ref().unwrap().found, Found::Ignored);
        assert!(!s.banner());
        // The player stops: the page empties.
        assert!(s.now_seen(None));
        assert!(s.now.is_none());
        let _ = FAKE_NOW;
    }

    #[test]
    fn plan_to_watch_becomes_watching() {
        let mut s = fresh();
        let id = s.entries.iter().find(|e| e.status == Status::PlanToWatch && e.progress == 0).unwrap().id;
        let title = s.entries.iter().find(|e| e.id == id).unwrap().title.clone();
        s.now_seen(Some(seen(&format!("{title} - 01.mkv"), Some(id), true, true)));
        s.record(false).unwrap();
        let e = s.entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!((e.status, e.progress), (Status::Watching, 1));
    }

    #[test]
    fn the_last_episode_completes_and_undo_restores() {
        let mut s = fresh();
        let (id, title, total) = {
            let e = s.entries.iter().find(|e| e.status == Status::Watching && e.episodes.is_some_and(|n| n > 1)).unwrap();
            (e.id, e.title.clone(), e.episodes.unwrap())
        };
        s.entries.iter_mut().find(|e| e.id == id).unwrap().progress = total - 1;
        s.now_seen(Some(seen(&format!("{title} - {total:02}.mkv"), Some(id), true, true)));
        s.record(false).unwrap();
        let e = s.entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!((e.status, e.progress), (Status::Completed, total));
        assert!(s.now.as_ref().unwrap().recorded.unwrap().completed);
        s.undo_record();
        let e = s.entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!((e.status, e.progress), (Status::Watching, total - 1));
    }

    #[test]
    fn kot_now_says_what_plays() {
        let mut s = fresh();
        assert_eq!(s.ctl_now(), None);
        s.now_seen(Some(seen("Lanterns of Kirigaoka - 06.mkv", Some(0), true, true)));
        let n = s.ctl_now().unwrap();
        assert_eq!((n.title.as_str(), n.episode, n.offer, n.reached, n.percent), ("Lanterns of Kirigaoka", Some(6), Some(6), true, Some(70)));
        s.record(false).unwrap();
        let n = s.ctl_now().unwrap();
        assert_eq!((n.offer, n.recorded), (None, Some(6)));
    }

    #[test]
    fn offers_and_times() {
        assert_eq!(clock_text(1132.4), "18:52");
        assert_eq!(clock_text(3725.0), "1:02:05");
        let s = fresh();
        let e = s.entries.iter().find(|e| e.id == 0).unwrap();
        assert_eq!(offer_for(e, Some(6)), Some(6));
        assert_eq!(offer_for(e, Some(5)), None);
        assert_eq!(offer_for(e, Some(13)), None);
    }

    #[test]
    fn the_threshold_cycles() {
        let mut s = fresh();
        assert_eq!(s.prefs.threshold, 80);
        s.next_threshold();
        assert_eq!(s.prefs.threshold, 90);
        s.next_threshold();
        s.next_threshold();
        assert_eq!(s.prefs.threshold, 70);
    }

    // ---- the position tick: a wake only while a shown position moves ----

    /// A state with the fake mpv playing at 12:30 of 24:00, as reported just now.
    fn playing_state(page: Page) -> State {
        let mut s = State::fake();
        let n = s.now.as_mut().unwrap();
        n.position = Some(750.0);
        n.duration = Some(1440.0);
        n.at = Some(kotora_detect::clock());
        n.playing = true;
        s.page = page;
        s.window = true;
        s
    }

    /// The next wake a `poll` left, as a headless runtime sees it (seconds from the last frame).
    fn wake_after_poll(s: State) -> Option<f64> {
        let mut rt = rune::Runtime::new(s, crate::ui::view, (1280.0, 800.0), rune::Env { system_dark: true, scale: 1.0 });
        rt.poll = Some(crate::ui::poll);
        rt.frame(1.0 / 60.0);
        let at = rt.wake_deadline()?;
        Some(at - 1.0 / 60.0)
    }

    #[test]
    fn playing_and_shown_wakes_within_a_second() {
        // The Now Playing page: the next whole second.
        let w = wake_after_poll(playing_state(Page::NowPlaying)).expect("a wake is scheduled");
        assert!(w > 0.0 && w <= 1.1, "within a second, got {w}");
        // Another page: the sidebar's percent (52 % here: the next turn is about 14 s away).
        let w = wake_after_poll(playing_state(Page::Queue)).expect("the sidebar percent still moves");
        assert!(w > 1.0 && w < 15.0, "got {w}");
    }

    #[test]
    fn paused_hidden_or_without_position_never_wakes() {
        let mut s = playing_state(Page::NowPlaying);
        s.now.as_mut().unwrap().playing = false;
        assert_eq!(wake_after_poll(s), None, "paused");
        let mut s = playing_state(Page::NowPlaying);
        s.window = false;
        assert_eq!(wake_after_poll(s), None, "window hidden");
        let mut s = playing_state(Page::NowPlaying);
        s.now.as_mut().unwrap().position = None;
        assert_eq!(wake_after_poll(s), None, "no position (process source)");
        let mut s = playing_state(Page::NowPlaying);
        s.now = None;
        assert_eq!(wake_after_poll(s), None, "nothing playing");
        // At the very end there is nothing left to move.
        let mut s = playing_state(Page::NowPlaying);
        s.now.as_mut().unwrap().position = Some(1440.0);
        assert_eq!(wake_after_poll(s), None, "at the end");
    }

    #[test]
    fn the_tick_reports_a_change_once_per_turn() {
        let mut s = playing_state(Page::NowPlaying);
        let mut tx = rune::Tx::default();
        assert!(tick(&mut s, &mut tx), "first time: the second is new");
        assert!(!tick(&mut s, &mut tx), "same second: no redraw");
        // A second later the shown second differs.
        s.now.as_mut().unwrap().position = Some(751.0);
        assert!(tick(&mut s, &mut tx));
        // Pausing clears it, so playing again draws once.
        s.now.as_mut().unwrap().playing = false;
        assert!(!tick(&mut s, &mut tx) && s.tick.is_none());
    }

    #[test]
    fn seconds_align_to_the_players_and_never_come_faster_than_one_a_second() {
        let mut s = playing_state(Page::NowPlaying);
        let n = s.now.as_mut().unwrap();
        n.position = Some(750.25);
        n.at = Some(kotora_detect::clock());
        let w = n.next_change(true).unwrap();
        assert!((w - 0.752).abs() < 0.01, "to the next whole second, got {w}");
        n.rate = 2.0;
        assert!(n.next_change(true).unwrap() >= 1.0, "double speed: still at most once a second");
        // The percent turns over half way: 750 of 1440 s is 52.08 %, so 52.5 % is next.
        n.rate = 1.0;
        n.position = Some(750.0);
        let w = n.next_change(false).unwrap();
        assert!((w - (0.525 * 1440.0 - 750.0)).abs() < 0.01, "got {w}");
    }

    /// Not a check: `cargo test --release -p kotora tick_cost -- --ignored --nocapture` prints the
    /// CPU one tick costs (poll, view, layout, draw list; no GPU), for the budgets in ROADMAP.md.
    #[test]
    #[ignore]
    fn tick_cost() {
        for page in [Page::NowPlaying, Page::Queue] {
            let mut rt = rune::Runtime::new(playing_state(page), crate::ui::view, (1280.0, 800.0), rune::Env { system_dark: true, scale: 1.0 });
            rt.poll = Some(crate::ui::poll);
            for _ in 0..3 {
                rt.frame(1.0 / 60.0);
            }
            let n = 600;
            let start = std::time::Instant::now();
            for i in 0..n {
                rt.update(move |s, _| {
                    let now = s.now.as_mut().unwrap();
                    now.position = Some(750.0 + i as f64);
                });
                rt.frame(1.0);
            }
            let each = start.elapsed().as_secs_f64() / n as f64;
            println!("{page:?}: {:.2} ms a tick, {:.3} % of one core at one tick a second", each * 1e3, each * 100.0);
        }
    }
}
