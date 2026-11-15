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
    /// The matched entry as the offer last saw it: when it changes (`+1`, the editor, `kot`, a
    /// sync), "Not now" and the one notification no longer hold, and the offer is asked again.
    pub mark: Option<Mark>,
    /// The playing episode was counted by a change made while it played (by hand, `kot`, a
    /// sync): it is not "watching it again" when that change completed the entry.
    pub counted: bool,
}

/// What of the matched entry decides the offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub id: u32,
    pub progress: u32,
    pub status: Status,
    pub rewatching: bool,
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
    /// The rewatch state before, for Undo (a rewatch started or finished from here).
    pub old_rewatching: bool,
    pub old_times: u32,
    /// This record started a rewatch.
    pub rewatch: bool,
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
    /// Settings → Titles (`[display] titles`).
    pub titles: kotora_core::config::TitleLang,
}

impl Default for Prefs {
    fn default() -> Self {
        let u = kotora_core::config::Updates::default();
        Prefs { auto: u.auto, threshold: u.threshold, keep_higher: u.keep_higher_progress, players: Default::default(), vlc_set_up: false, precise_answered: false, titles: Default::default() }
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
            titles: c.display.titles,
        }
    }
}

/// The thresholds the Settings control cycles through.
pub const THRESHOLDS: [u8; 4] = [70, 80, 90, 100];

/// A completed entry (not being rewatched) plays again: the episode a rewatch would start at.
/// Asked, never recorded by itself.
pub fn rewatch_for(e: &Entry, episode: Option<u32>) -> Option<u32> {
    if e.status != Status::Completed || e.rewatching {
        return None;
    }
    let ep = episode.or((e.episodes == Some(1)).then_some(1))?;
    (ep > 0 && e.episodes.is_none_or(|n| ep <= n)).then_some(ep)
}

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
            mark: None,
            counted: false,
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
    pub(crate) fn on_screen(&self) -> bool {
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

    /// "Watching it again?": a completed entry plays, with the episode a rewatch starts at.
    pub fn rewatch_offer(&self) -> Option<(usize, u32)> {
        let now = self.now.as_ref()?;
        let id = now.media_id()?;
        let i = self.entries.iter().position(|e| e.id == id)?;
        if now.counted {
            return None;
        }
        let ep = rewatch_for(&self.entries[i], now.episode())?;
        Some((i, ep))
    }

    /// The banner shows: an episode recorded from here (with Undo), or an offer once the file
    /// counts as watched (an episode, or a rewatch).
    pub fn banner(&self) -> bool {
        let Some(now) = &self.now else { return false };
        if now.dismissed || !self.has_list() {
            return false;
        }
        now.recorded.is_some() || (now.reached && (self.offer().is_some() || self.rewatch_offer().is_some()))
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
                let (dismissed, acted, recorded, picking, reached, mark, counted) = (old.dismissed, old.acted, old.recorded, old.picking, old.reached, old.mark, old.counted);
                *old = Now { dismissed, acted, recorded, picking, reached: reached || new.reached, mark, counted, ..new };
            }
            _ => self.now = Some(new),
        }
        if !self.recheck_now() {
            self.act(true);
        }
        true
    }

    /// The matched entry's [`Mark`] now.
    fn entry_mark(&self) -> Option<Mark> {
        let id = self.now.as_ref()?.media_id()?;
        let e = self.entries.iter().find(|e| e.id == id)?;
        Some(Mark { id, progress: e.progress, status: e.status, rewatching: e.rewatching })
    }

    /// Call after the entries change (an edit, a sync, the queue). When the playing file's
    /// entry changed, the offer is decided again: "Not now" and the notification were about the
    /// old progress. Ahead by two, then `+1` by hand: the playing episode is the next one and is
    /// asked for (at once if the file passed the threshold). Counted by hand: the offer goes.
    /// It asks then, never records by itself. Says whether it acted.
    pub fn recheck_now(&mut self) -> bool {
        let mark = self.entry_mark();
        let Some(n) = &mut self.now else { return false };
        let old = std::mem::replace(&mut n.mark, mark);
        // The first sight of the match (a new file, a pick) and a recorded episode (its Undo
        // banner stays) keep their state.
        let (Some(old), Some(mark)) = (old, mark) else { return false };
        if old == mark || old.id != mark.id || n.recorded.is_some() {
            return false;
        }
        if !mark.rewatching && n.episode().is_some_and(|ep| mark.progress >= ep) {
            n.counted = true;
        }
        n.dismissed = false;
        n.acted = false;
        self.act(false);
        true
    }

    /// The file passed the threshold.
    pub fn now_reached(&mut self, key: &str, file: &str) -> bool {
        let Some(now) = &mut self.now else { return false };
        if now.key != key || now.file != file_name(file) {
            return false;
        }
        now.reached = true;
        self.act(true);
        true
    }

    /// Once reached and recognised: record (auto, sure, `may_record`) or ask with a notification.
    /// Once per file, and again when the entry changes ([`State::recheck_now`]).
    fn act(&mut self, may_record: bool) {
        let Some(now) = &self.now else { return };
        if !now.reached || now.acted || now.recognizing || now.dismissed || !self.has_list() {
            return;
        }
        let Some((i, ep)) = self.offer() else {
            // A completed anime plays again: always ask, even with "Update automatically" on.
            if let Some((i, ep)) = self.rewatch_offer() {
                let title = self.entries[i].title.clone();
                if let Some(n) = &mut self.now {
                    n.acted = true;
                }
                crate::detect::notify(&format!("Watching {title} again?"), &format!("Start a rewatch at episode {ep}: open Kotora, or run kot confirm."), self.handle.is_some());
            }
            return;
        };
        let title = self.entries[i].title.clone();
        // A player seen by its open file only knows no position or pause: the time open is a
        // guess, so it always asks, even with "Update automatically" on.
        let auto = may_record && self.prefs.auto && now.sure() && now.via != crate::precise::VIA_PROCESS;
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

    /// An edit from the banner itself (Update, Start rewatch, Undo): it answers the offer, so
    /// [`State::recheck_now`] doesn't ask again.
    fn edit_playing(&mut self, id: u32, change: Change) -> Result<usize, String> {
        if let Some(n) = &mut self.now {
            n.mark = None;
        }
        let r = self.edit(id, change);
        let mark = self.entry_mark();
        if let Some(n) = &mut self.now {
            n.mark = mark;
        }
        r
    }

    /// Update (the banner, `kot confirm`): the offered episode goes to the queue.
    pub fn record(&mut self, auto: bool) -> Result<(String, u32), String> {
        if self.offer().is_none() && !auto && self.rewatch_offer().is_some() {
            return self.start_rewatch_playing();
        }
        let (i, ep) = self.offer().ok_or("Nothing to update: no recognised new episode is playing.")?;
        let e = &self.entries[i];
        let (id, title, old_progress, old_status) = (e.id, e.title.clone(), e.progress, e.status);
        let (old_rewatching, old_times) = (e.rewatching, e.times_rewatched);
        // A show you planned to watch is now one you watch. The last episode completes it instead
        // (`edit` does that when the total is known).
        let last = e.episodes.is_some_and(|n| n > 0 && ep == n);
        let status = (old_status == Status::PlanToWatch && !last).then_some(Status::Watching);
        self.edit_playing(id, Change { progress: Some(ep), status, ..Default::default() })?;
        let completed = old_status != Status::Completed && self.entries.iter().any(|e| e.id == id && e.status == Status::Completed);
        if let Some(n) = &mut self.now {
            n.recorded = Some(Recorded { id, episode: ep, old_progress, old_status, completed, auto, old_rewatching, old_times, rewatch: false });
            n.acted = true;
            n.picking = false;
        }
        Ok((title, ep))
    }

    /// "Start rewatch" on the banner (or `kot confirm`): the completed entry is rewatched from
    /// here, at the playing episode, as one queued change. Undo puts it back.
    pub fn start_rewatch_playing(&mut self) -> Result<(String, u32), String> {
        let (i, ep) = self.rewatch_offer().ok_or("Nothing to rewatch: no completed anime is playing.")?;
        let e = &self.entries[i];
        let (id, title, old_progress, old_status, old_times) = (e.id, e.title.clone(), e.progress, e.status, e.times_rewatched);
        let start = Change::start_rewatch(&e.values()).ok_or("That anime is not completed.")?;
        // One edit: the rewatch and the episode (the last one ends it again, counted).
        let cur = start.apply(e.values());
        let change = start.merge(Change { progress: Some(ep), ..Default::default() }.completes(&cur, e.episodes));
        self.edit_playing(id, change)?;
        if let Some(n) = &mut self.now {
            n.recorded = Some(Recorded { id, episode: ep, old_progress, old_status, completed: false, auto: false, old_rewatching: false, old_times, rewatch: true });
            n.acted = true;
            n.picking = false;
        }
        Ok((title, ep))
    }

    /// Undo on the banner: the entry goes back (the queue drops the change if it hasn't left).
    pub fn undo_record(&mut self) {
        let Some(r) = self.now.as_ref().and_then(|n| n.recorded) else { return };
        let back = Change {
            progress: Some(r.old_progress),
            status: Some(r.old_status),
            rewatching: Some(r.old_rewatching),
            times_rewatched: Some(r.old_times),
            ..Default::default()
        };
        let _ = self.edit_playing(r.id, back);
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
        self.recheck_now();
        self.act(true);
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

    /// A completed anime plays again: the banner asks to start a rewatch (never by itself,
    /// even with "Update automatically" on); then episodes count as usual, and the last one ends
    /// the rewatch in the same queued change.
    #[test]
    fn a_completed_anime_playing_again_offers_a_rewatch() {
        let mut s = fresh();
        s.prefs.auto = true;
        let id = s.entries.iter().find(|e| e.status == Status::Completed && !e.rewatching && e.episodes == Some(12)).unwrap().id;
        let times = s.entries.iter().find(|e| e.id == id).unwrap().times_rewatched;
        s.now_seen(Some(seen("Some Show - 03.mkv", Some(id), true, true)));
        assert_eq!(s.offer(), None, "no plain episode offer: it is completed");
        assert_eq!(s.rewatch_offer().map(|(_, ep)| ep), Some(3));
        assert!(s.banner() && s.now.as_ref().unwrap().recorded.is_none(), "asked, not recorded");
        let e = s.entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!((e.progress, e.rewatching), (12, false), "asking changes nothing");
        s.start_rewatch_playing().unwrap();
        let e = s.entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!((e.status, e.progress, e.rewatching), (Status::Completed, 3, true));
        assert!(s.now.as_ref().unwrap().recorded.is_some_and(|r| r.rewatch));
        // Undo: back to completed, not rewatching.
        s.undo_record();
        let e = s.entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!((e.progress, e.rewatching, e.times_rewatched), (12, false, times));
        // `kot confirm` answers the question too; then the last episode ends the rewatch.
        let mut s = fresh();
        s.now_seen(Some(seen("Some Show - 11.mkv", Some(id), true, true)));
        s.record(false).unwrap();
        assert_eq!(s.entries.iter().find(|e| e.id == id).unwrap().progress, 11);
        s.now_seen(Some(seen("Some Show - 12.mkv", Some(id), true, false)));
        assert!(s.now_reached("mpv:/tmp/mpv.sock", "/v/Some Show - 12.mkv"));
        assert_eq!(s.offer().map(|(_, ep)| ep), Some(12), "during the rewatch: normal counting");
        s.record(false).unwrap();
        let e = s.entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!((e.status, e.progress, e.rewatching, e.times_rewatched), (Status::Completed, 12, false, times + 1));
    }

    /// The playing file's state for `seen`, by IPC (with a position) or by its process only.
    fn seen_via(file: &str, reached: bool, process: bool) -> Seen {
        let mut v = seen(file, Some(0), true, reached);
        if process {
            v.track.playing.via = kotora_detect::Via::Process;
            v.track.playing.position = None;
            v.track.playing.duration = None;
        }
        v
    }

    fn progress(s: &State) -> u32 {
        s.entries.iter().find(|e| e.id == 0).unwrap().progress
    }

    /// Lanterns of Kirigaoka (id 0) is at 5 of 12 and episode 7 plays, past the threshold: one
    /// is missing. "Not now", then `+1` by hand (6): episode 7 is the next one and is asked for
    /// at once, by IPC and by process. Asked, never recorded by itself.
    #[test]
    fn ahead_by_two_then_plus_one_asks_for_the_playing_episode() {
        for process in [false, true] {
            for not_now in [false, true] {
                let mut s = fresh();
                s.now_seen(Some(seen_via("Lanterns of Kirigaoka - 07.mkv", true, process)));
                assert_eq!(progress(&s), 5);
                // Even with "Update automatically" on, asking again never records.
                s.prefs.auto = true;
                if not_now {
                    assert!(s.dismiss_now() && !s.banner());
                }
                s.plus_one(0).unwrap();
                assert_eq!(progress(&s), 6);
                assert_eq!(s.offer().map(|(_, ep)| ep), Some(7));
                assert!(s.banner(), "process {process}, not now {not_now}");
                let n = s.now.as_ref().unwrap();
                assert!(n.acted && n.recorded.is_none(), "asked again, not recorded");
                assert_eq!(s.ctl_now().unwrap().offer, Some(7));
                assert_eq!(s.record(false).unwrap().1, 7);
                assert_eq!(progress(&s), 7);
            }
        }
    }

    /// The same before the threshold: no banner yet, then it asks once the file counts.
    #[test]
    fn plus_one_before_the_threshold_waits_for_it() {
        let mut s = fresh();
        s.now_seen(Some(seen("Lanterns of Kirigaoka - 07.mkv", Some(0), true, false)));
        s.plus_one(0).unwrap();
        assert!(!s.banner());
        assert!(s.now_reached("mpv:/tmp/mpv.sock", "/v/Lanterns of Kirigaoka - 07.mkv"));
        assert!(s.banner());
        assert_eq!(s.offer().map(|(_, ep)| ep), Some(7));
    }

    /// `+1` by hand to the playing episode: the offer goes, nothing is counted twice.
    #[test]
    fn plus_one_to_the_playing_episode_ends_the_offer() {
        for process in [false, true] {
            let mut s = fresh();
            s.now_seen(Some(seen_via("Lanterns of Kirigaoka - 06.mkv", true, process)));
            assert!(s.banner());
            s.plus_one(0).unwrap();
            assert_eq!(progress(&s), 6);
            assert!(s.offer().is_none() && !s.banner(), "process {process}");
            assert_eq!(s.ctl_now().unwrap().offer, None);
            assert!(s.record(false).is_err(), "kot confirm has nothing to add");
            assert_eq!(progress(&s), 6);
        }
        // The last episode, counted by hand, completes the entry: no "watching it again?".
        let mut s = fresh();
        s.edit(0, Change { progress: Some(11), ..Default::default() }).unwrap();
        s.now_seen(Some(seen("Lanterns of Kirigaoka - 12.mkv", Some(0), true, true)));
        assert!(s.banner());
        s.plus_one(0).unwrap();
        assert_eq!(s.entries.iter().find(|e| e.id == 0).unwrap().status, Status::Completed);
        assert!(s.offer().is_none() && s.rewatch_offer().is_none() && !s.banner());
    }

    /// A change from a sync or the queue (another device, `kot`) decides the offer again too;
    /// one that leaves the progress alone keeps "Not now".
    #[test]
    fn a_synced_change_decides_the_offer_again() {
        let mut s = fresh();
        s.now_seen(Some(seen("Lanterns of Kirigaoka - 07.mkv", Some(0), true, true)));
        assert!(s.dismiss_now());
        let e = s.entries.iter().find(|e| e.id == 0).unwrap();
        let media = kotora_core::Media {
            id: 0,
            title: e.title.clone(),
            title_en: None,
            title_ja: None,
            synonyms: vec![],
            episodes: Some(12),
            format: "tv".into(),
            air_status: "currently_airing".into(),
            cover_url: None,
            start_date: None,
            broadcast: None,
        };
        let item = |progress, score| kotora_core::ListItem { media: media.clone(), status: Status::Watching, progress, score, rewatching: false, times_rewatched: 0, updated_at: 7 };
        s.apply(crate::model::Msg::Queue { ops: vec![], items: vec![item(5, Some(8))], gone: vec![], offline: None });
        assert!(!s.banner(), "same progress: Not now holds");
        s.apply(crate::model::Msg::Queue { ops: vec![], items: vec![item(6, Some(8))], gone: vec![], offline: None });
        assert!(s.banner());
        assert_eq!(s.offer().map(|(_, ep)| ep), Some(7));
    }

    /// Undo on the banner puts the entry back and stays closed: it doesn't ask again at once.
    #[test]
    fn undo_does_not_ask_again() {
        let mut s = fresh();
        s.now_seen(Some(seen("Lanterns of Kirigaoka - 06.mkv", Some(0), true, true)));
        s.record(false).unwrap();
        s.undo_record();
        assert_eq!(progress(&s), 5);
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
