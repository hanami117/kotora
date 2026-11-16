//! The watch hub: which player to follow, and when its episode counts as watched.
//!
//! Pure logic with the time passed in, so tests drive it. A file counts as watched ("reached")
//! when both hold:
//! - the position passed the threshold (default 80 % of the duration), and
//! - it really played for a while: a quarter of the duration, at most 2 minutes (a jump to the
//!   end doesn't count).
//!
//! Or when the player stops or changes file after at least half of it, with the same play time.
//!
//! A process source (`Via::Process`, see `procfs.rs`) knows neither position nor pause, only that
//! the file is open. For it the time the file stays open counts as time played, and the file
//! counts as watched once it was open for:
//! - the threshold share of the duration, when a duration is known, else
//! - [`Watch::open_secs`]: `[players] process_minutes`, default [`OPEN_MINUTES`] (15). That is
//!   about 60 % of a 24-minute episode: the OP and ED are often skipped, and since a pause can't
//!   be seen, a lower bar would count an episode left paused. Shorter episodes (5-minute shorts)
//!   need the IPC socket. The stop rule uses the same time.
//!
//! When an IPC or MPRIS source shows the same file (same file name) as a process source, the
//! process slot is hidden for that file: never followed, never reported as reached. The
//! sources that know the position win, and nothing is counted twice.
//!
//! Positions between two reports are worked out from the clock and the speed. Sources report
//! every pause, seek and speed change, so this stays right without asking the player again.
//! [`Watch::next_wake`] says when the next crossing is due; the runner sleeps until then.

use std::collections::{HashMap, HashSet};

use crate::{PlayState, Playing, SourceStatus, Switch, Via};

/// The followed player, as the app shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub playing: Playing,
    /// Seconds really played of this file so far.
    pub played: f64,
    /// The file counts as watched.
    pub reached: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Report {
    /// The followed player and what it plays, or nothing.
    Now(Option<Track>),
    /// A file passed the threshold (or stopped after half). Sent once per file and player.
    Reached { key: String, file: String },
    /// Every source's state, for Settings and the Now Playing page.
    Sources(Vec<SourceStatus>),
}

/// Share of the duration after which a stop still counts.
pub const STOP_SHARE: f64 = 0.5;
/// The longest play time asked for (seconds).
const MIN_PLAYED_CAP: f64 = 120.0;
/// Process sources: minutes a file must stay open when its duration is unknown (default).
pub const OPEN_MINUTES: u32 = 15;

fn min_played(duration: f64) -> f64 {
    (duration * 0.25).min(MIN_PLAYED_CAP)
}

struct Slot {
    p: Playing,
    played: f64,
    /// Until when `played` is counted.
    counted_to: f64,
    reached: bool,
    /// When this player last started playing (the most recent one is followed).
    started: f64,
    /// A process slot whose file an IPC or MPRIS source also shows: ignored for this file.
    hidden: bool,
}

impl Slot {
    fn position_at(&self, t: f64) -> Option<f64> {
        let pos = self.p.position?;
        Some(if self.p.state == PlayState::Playing { pos + (t - self.p.at).max(0.0) * self.p.rate } else { pos })
    }

    fn played_at(&self, t: f64) -> f64 {
        if self.p.state == PlayState::Playing { self.played + (t - self.counted_to).max(0.0) } else { self.played }
    }

    /// No position: the time the file stays open stands for the time played.
    fn by_open_time(&self) -> bool {
        self.p.via == Via::Process && self.p.position.is_none()
    }

    /// For [`Slot::by_open_time`]: how long the file must stay open.
    fn open_needed(&self, share: f64, open_secs: f64) -> f64 {
        match self.p.duration {
            Some(d) if d > 0.0 => share * d,
            _ => open_secs,
        }
    }

    fn passes(&self, t: f64, share: f64, open_secs: f64) -> bool {
        if self.by_open_time() {
            return self.played_at(t) >= self.open_needed(share, open_secs);
        }
        match (self.position_at(t), self.p.duration) {
            (Some(pos), Some(d)) if d > 0.0 => pos >= share * d && self.played_at(t) >= min_played(d),
            _ => false,
        }
    }

    fn track(&self, t: f64) -> Track {
        Track { playing: self.p.clone(), played: self.played_at(t), reached: self.reached }
    }
}

pub struct Watch {
    /// 0–1: the share of the episode after which it counts.
    pub threshold: f64,
    /// Process sources: seconds a file must stay open when its duration is unknown.
    pub open_secs: f64,
    /// Players switched off in Settings: their reports are ignored.
    pub off: HashSet<Switch>,
    slots: HashMap<String, Slot>,
    current: Option<String>,
    sources: Vec<SourceStatus>,
}

impl Watch {
    pub fn new(threshold: f64) -> Watch {
        Watch { threshold, open_secs: f64::from(OPEN_MINUTES) * 60.0, off: HashSet::new(), slots: HashMap::new(), current: None, sources: Vec::new() }
    }

    /// A source's new snapshot of one player.
    pub fn update(&mut self, p: Playing, now: f64) -> Vec<Report> {
        if self.off.contains(&p.switch) {
            return self.gone(&p.key, now);
        }
        let mut out = Vec::new();
        let key = p.key.clone();
        let (threshold, open_secs) = (self.threshold, self.open_secs);
        match self.slots.get_mut(&key) {
            Some(slot) if slot.p.file() == p.file() => {
                // The same file: count the play time up to now, then take the new snapshot.
                slot.played = slot.played_at(now);
                slot.counted_to = now;
                if p.state == PlayState::Stopped {
                    stop_rule(slot, now, open_secs, &mut out);
                }
                if p.state == PlayState::Playing && slot.p.state != PlayState::Playing {
                    slot.started = now;
                }
                slot.p = p;
            }
            old => {
                if let Some(slot) = old {
                    stop_rule(slot, now, open_secs, &mut out);
                }
                let started = if p.state == PlayState::Playing { now } else { f64::MIN };
                self.slots.insert(key.clone(), Slot { p, played: 0.0, counted_to: now, reached: false, started, hidden: false });
            }
        }
        self.hide_shadowed();
        if let Some(slot) = self.slots.get_mut(&key)
            && !slot.reached
            && slot.p.state != PlayState::Stopped
            && slot.passes(now, threshold, open_secs)
        {
            slot.reached = true;
            out.push(reached(slot));
        }
        self.follow(now, &mut out, Some(&key));
        out
    }

    /// The player went away (quit, socket closed, name left the bus, switched off).
    pub fn gone(&mut self, key: &str, now: f64) -> Vec<Report> {
        let mut out = Vec::new();
        if let Some(mut slot) = self.slots.remove(key) {
            stop_rule(&mut slot, now, self.open_secs, &mut out);
        }
        self.follow(now, &mut out, None);
        out
    }

    /// Forgets the players whose switch went off (Settings).
    pub fn drop_off(&mut self, now: f64) -> Vec<Report> {
        let keys: Vec<String> = self.slots.values().filter(|s| self.off.contains(&s.p.switch)).map(|s| s.p.key.clone()).collect();
        let mut out = Vec::new();
        for k in keys {
            self.slots.remove(&k);
        }
        self.follow(now, &mut out, None);
        out
    }

    /// Called at [`Watch::next_wake`]: the crossings due by now.
    pub fn tick(&mut self, now: f64) -> Vec<Report> {
        let mut out = Vec::new();
        let (threshold, open_secs) = (self.threshold, self.open_secs);
        let mut touched = None;
        for slot in self.slots.values_mut() {
            if !slot.reached && !slot.hidden && slot.p.state == PlayState::Playing && slot.passes(now, threshold, open_secs) {
                slot.reached = true;
                out.push(reached(slot));
                touched = Some(slot.p.key.clone());
            }
        }
        if touched.is_some() {
            self.follow(now, &mut out, touched.as_deref());
        }
        out
    }

    /// Seconds from `now` until the next crossing, if one can come without a new report.
    pub fn next_wake(&self, now: f64) -> Option<f64> {
        self.slots
            .values()
            .filter(|s| !s.reached && !s.hidden && s.p.state == PlayState::Playing && s.p.rate > 0.0)
            .filter_map(|s| {
                if s.by_open_time() {
                    return Some((s.open_needed(self.threshold, self.open_secs) - s.played_at(now)).max(0.05));
                }
                let d = s.p.duration.filter(|d| *d > 0.0)?;
                let pos = s.position_at(now)?;
                let by_pos = (self.threshold * d - pos) / s.p.rate;
                let by_time = min_played(d) - s.played_at(now);
                Some(by_pos.max(by_time).max(0.05))
            })
            .min_by(f64::total_cmp)
    }

    /// A source's state changed. Returns the whole list when it differs.
    pub fn status(&mut self, st: SourceStatus) -> Option<Report> {
        match self.sources.iter_mut().find(|s| s.switch == st.switch && s.via == st.via) {
            Some(old) if *old == st => return None,
            Some(old) => *old = st,
            None => self.sources.push(st),
        }
        Some(Report::Sources(self.sources.clone()))
    }

    pub fn sources(&self) -> &[SourceStatus] {
        &self.sources
    }

    /// The source state for one switch and way, if reported.
    pub fn source(&self, switch: Switch, via: Via) -> Option<&SourceStatus> {
        self.sources.iter().find(|s| s.switch == switch && s.via == via)
    }

    pub fn current(&self, now: f64) -> Option<Track> {
        self.slots.get(self.current.as_ref()?).map(|s| s.track(now))
    }

    /// Picks the player to follow: the one that most recently started playing; else the one
    /// followed so far, while it is still there; else any. Reports `Now` when it, or what it
    /// shows, changed.
    fn follow(&mut self, now: f64, out: &mut Vec<Report>, touched: Option<&str>) {
        let playing = self.slots.values().filter(|s| !s.hidden && s.p.state == PlayState::Playing && s.p.file().is_some()).max_by(|a, b| a.started.total_cmp(&b.started));
        let pick = match playing {
            Some(s) => Some(s.p.key.clone()),
            None => match &self.current {
                Some(k) if self.slots.get(k).is_some_and(|s| !s.hidden && s.p.file().is_some()) => Some(k.clone()),
                _ => self.slots.values().filter(|s| !s.hidden && s.p.file().is_some()).max_by(|a, b| a.started.total_cmp(&b.started)).map(|s| s.p.key.clone()),
            },
        };
        if pick != self.current || (pick.is_some() && pick.as_deref() == touched) {
            self.current = pick;
            out.push(Report::Now(self.current(now)));
        }
    }
}

impl Watch {
    /// Hides the process slots whose file another kind of source shows. Once hidden, a slot
    /// stays hidden for that file (so the IPC source going first can't make it count later).
    fn hide_shadowed(&mut self) {
        let names: HashSet<String> = self.slots.values().filter(|s| s.p.via != Via::Process).filter_map(|s| s.p.file().map(|f| file_name(f).to_string())).collect();
        if names.is_empty() {
            return;
        }
        for s in self.slots.values_mut() {
            if s.p.via == Via::Process && !s.hidden && s.p.file().is_some_and(|f| names.contains(file_name(f))) {
                s.hidden = true;
            }
        }
    }
}

fn file_name(f: &str) -> &str {
    crate::file_name(f)
}

fn reached(slot: &Slot) -> Report {
    Report::Reached { key: slot.p.key.clone(), file: slot.p.file().unwrap_or_default().to_string() }
}

/// A stop or a new file: half watched (and really played) still counts.
fn stop_rule(slot: &mut Slot, now: f64, open_secs: f64, out: &mut Vec<Report>) {
    if !slot.reached && !slot.hidden && slot.p.file().is_some() && slot.passes(now, STOP_SHARE, open_secs) {
        slot.reached = true;
        out.push(reached(slot));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(key: &str, file: &str, pos: f64, dur: f64, state: PlayState, at: f64) -> Playing {
        Playing {
            key: key.into(),
            player: "mpv".into(),
            switch: Switch::Mpv,
            via: Via::MpvIpc,
            path: Some(file.into()),
            title: None,
            position: Some(pos),
            duration: Some(dur),
            state,
            rate: 1.0,
            at,
        }
    }

    fn reached_files(r: &[Report]) -> Vec<String> {
        r.iter()
            .filter_map(|r| match r {
                Report::Reached { file, .. } => Some(file.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_played_episode_crosses_at_the_threshold_by_the_clock() {
        let mut w = Watch::new(0.8);
        // 24 min episode, starts at 0.
        let r = w.update(snap("a", "/v/ep6.mkv", 0.0, 1440.0, PlayState::Playing, 0.0), 0.0);
        assert!(matches!(r.as_slice(), [Report::Now(Some(t))] if !t.reached));
        // The hub wakes when 80 % is due: 1152 s.
        let wake = w.next_wake(0.0).unwrap();
        assert!((wake - 1152.0).abs() < 0.01, "{wake}");
        assert!(w.tick(1151.0).is_empty());
        let r = w.tick(1152.0);
        assert_eq!(reached_files(&r), ["/v/ep6.mkv"]);
        assert!(matches!(r.last(), Some(Report::Now(Some(t))) if t.reached));
        // Once per file.
        assert!(w.tick(1300.0).is_empty());
        assert_eq!(w.next_wake(1300.0), None);
    }

    #[test]
    fn a_pause_holds_the_clock() {
        let mut w = Watch::new(0.8);
        w.update(snap("a", "f", 0.0, 1000.0, PlayState::Playing, 0.0), 0.0);
        w.update(snap("a", "f", 300.0, 1000.0, PlayState::Paused, 300.0), 300.0);
        assert_eq!(w.next_wake(300.0), None, "nothing is due while paused");
        assert!(w.tick(5000.0).is_empty());
        w.update(snap("a", "f", 300.0, 1000.0, PlayState::Playing, 5000.0), 5000.0);
        assert!((w.next_wake(5000.0).unwrap() - 500.0).abs() < 0.01);
    }

    #[test]
    fn a_jump_to_the_end_does_not_count_until_it_really_played() {
        let mut w = Watch::new(0.8);
        w.update(snap("a", "f", 0.0, 1440.0, PlayState::Playing, 0.0), 0.0);
        // Seek to 90 % after 10 s: the position passes, the play time doesn't (120 s needed).
        let r = w.update(snap("a", "f", 1300.0, 1440.0, PlayState::Playing, 10.0), 10.0);
        assert!(reached_files(&r).is_empty());
        assert!((w.next_wake(10.0).unwrap() - 110.0).abs() < 0.01);
        assert_eq!(reached_files(&w.tick(120.0)), ["f"]);
    }

    #[test]
    fn stopping_after_half_counts_and_before_does_not() {
        let mut w = Watch::new(0.8);
        w.update(snap("a", "half", 0.0, 1000.0, PlayState::Playing, 0.0), 0.0);
        assert_eq!(reached_files(&w.gone("a", 600.0)), ["half"]);
        assert!(matches!(w.gone("a", 600.0).as_slice(), []));

        w.update(snap("a", "early", 0.0, 1000.0, PlayState::Playing, 0.0), 0.0);
        let r = w.gone("a", 400.0);
        assert!(reached_files(&r).is_empty());
        assert!(matches!(r.as_slice(), [Report::Now(None)]));

        // A new file in the same player after half of the old one.
        w.update(snap("a", "one", 0.0, 1000.0, PlayState::Playing, 0.0), 0.0);
        let r = w.update(snap("a", "two", 0.0, 1000.0, PlayState::Playing, 700.0), 700.0);
        assert_eq!(reached_files(&r), ["one"]);
        assert!(matches!(r.last(), Some(Report::Now(Some(t))) if t.playing.path.as_deref() == Some("two")));
    }

    #[test]
    fn the_most_recent_player_is_followed() {
        let mut w = Watch::new(0.8);
        w.update(snap("a", "fa", 0.0, 1000.0, PlayState::Playing, 0.0), 0.0);
        w.update(snap("b", "fb", 0.0, 1000.0, PlayState::Playing, 10.0), 10.0);
        assert_eq!(w.current(10.0).unwrap().playing.key, "b");
        // b pauses; a still plays, so a is followed.
        let r = w.update(snap("b", "fb", 5.0, 1000.0, PlayState::Paused, 15.0), 15.0);
        assert!(matches!(r.as_slice(), [Report::Now(Some(t))] if t.playing.key == "a"));
        // Both paused: the followed one stays.
        w.update(snap("a", "fa", 20.0, 1000.0, PlayState::Paused, 20.0), 20.0);
        assert_eq!(w.current(20.0).unwrap().playing.key, "a");
    }

    #[test]
    fn a_switched_off_player_is_ignored() {
        let mut w = Watch::new(0.8);
        w.update(snap("a", "f", 0.0, 1000.0, PlayState::Playing, 0.0), 0.0);
        w.off.insert(Switch::Mpv);
        let r = w.update(snap("a", "f", 10.0, 1000.0, PlayState::Playing, 10.0), 10.0);
        assert!(matches!(r.as_slice(), [Report::Now(None)]));
        assert!(w.current(10.0).is_none());
    }

    #[test]
    fn unknown_duration_never_crosses_and_speed_counts() {
        let mut w = Watch::new(0.8);
        let mut p = snap("a", "f", 0.0, 1000.0, PlayState::Playing, 0.0);
        p.duration = None;
        w.update(p, 0.0);
        assert_eq!(w.next_wake(0.0), None);
        assert!(w.tick(1e6).is_empty());

        let mut w = Watch::new(0.8);
        let mut p = snap("a", "f", 0.0, 1000.0, PlayState::Playing, 0.0);
        p.rate = 2.0;
        w.update(p, 0.0);
        // 800 s of video at 2x: 400 s.
        assert!((w.next_wake(0.0).unwrap() - 400.0).abs() < 0.01);
    }

    fn proc_snap(pid: u32, file: &str, at: f64) -> Playing {
        crate::procfs::snapshot(pid, crate::procfs::kind_of("mpv").unwrap(), file, at)
    }

    #[test]
    fn a_process_source_counts_the_time_the_file_stays_open() {
        let mut w = Watch::new(0.8);
        let r = w.update(proc_snap(9, "/v/Show - 05.mkv", 0.0), 0.0);
        assert!(matches!(r.as_slice(), [Report::Now(Some(t))] if t.playing.via == Via::Process));
        // Unknown duration: 15 minutes open.
        assert!((w.next_wake(0.0).unwrap() - 900.0).abs() < 0.01);
        assert!(w.tick(899.0).is_empty());
        // The same file again (the source sends changes only, but it is harmless).
        assert!(reached_files(&w.update(proc_snap(9, "/v/Show - 05.mkv", 500.0), 500.0)).is_empty());
        assert!((w.next_wake(500.0).unwrap() - 400.0).abs() < 0.01);
        assert_eq!(reached_files(&w.tick(900.0)), ["/v/Show - 05.mkv"]);
        assert_eq!(w.next_wake(900.0), None);

        // The minutes come from the settings.
        let mut w = Watch::new(0.8);
        w.open_secs = 600.0;
        w.update(proc_snap(9, "f.mkv", 0.0), 0.0);
        assert!((w.next_wake(0.0).unwrap() - 600.0).abs() < 0.01);

        // A known duration: the threshold share of it.
        let mut w = Watch::new(0.8);
        let mut p = proc_snap(9, "f.mkv", 0.0);
        p.duration = Some(1440.0);
        w.update(p, 0.0);
        assert!((w.next_wake(0.0).unwrap() - 1152.0).abs() < 0.01);
    }

    #[test]
    fn a_file_closed_early_by_a_process_source_does_not_count() {
        let mut w = Watch::new(0.8);
        w.update(proc_snap(9, "a.mkv", 0.0), 0.0);
        // Next file of the playlist after 10 minutes: a.mkv doesn't count.
        let r = w.update(proc_snap(9, "b.mkv", 600.0), 600.0);
        assert!(reached_files(&r).is_empty());
        // b.mkv open 16 minutes, then the player quits before a tick: the stop rule counts it.
        assert_eq!(reached_files(&w.gone("proc:9", 1560.0)), ["b.mkv"]);
    }

    #[test]
    fn ipc_and_mpris_win_over_a_process_source_for_the_same_file() {
        let mut w = Watch::new(0.8);
        w.update(proc_snap(9, "/media/anime/Show - 06.mkv", 0.0), 0.0);
        assert_eq!(w.current(0.0).unwrap().playing.key, "proc:9");
        // mpv's IPC shows the same file through a symlinked folder.
        let r = w.update(snap("mpv:/run/mpv.sock", "/home/u/anime/Show - 06.mkv", 2.0, 1440.0, PlayState::Playing, 3.0), 3.0);
        assert!(matches!(r.as_slice(), [Report::Now(Some(t))] if t.playing.via == Via::MpvIpc));
        // Only the IPC crossing is due (1152 s of video, from 2 s), the process one is ignored.
        assert!((w.next_wake(3.0).unwrap() - 1150.0).abs() < 0.01);
        // A rescan of the process changes nothing.
        assert!(w.update(proc_snap(9, "/media/anime/Show - 06.mkv", 8.0), 8.0).is_empty());
        // Paused at 40 % for a long time, then mpv quits: no report from either.
        w.update(snap("mpv:/run/mpv.sock", "/home/u/anime/Show - 06.mkv", 576.0, 1440.0, PlayState::Paused, 600.0), 600.0);
        assert!(w.tick(5000.0).is_empty());
        assert!(reached_files(&w.gone("mpv:/run/mpv.sock", 5000.0)).is_empty());
        assert!(w.current(5000.0).is_none(), "the hidden process slot is not followed");
        assert!(reached_files(&w.gone("proc:9", 5005.0)).is_empty());
        // Another file in the same process is not hidden.
        w.update(proc_snap(9, "/media/anime/Show - 07.mkv", 5010.0), 5010.0);
        assert_eq!(w.current(5010.0).unwrap().playing.key, "proc:9");
    }

    #[test]
    fn source_states_report_only_changes() {
        let mut w = Watch::new(0.8);
        let st = SourceStatus { switch: Switch::Vlc, via: Via::Mpris, line: "not running".into(), led: crate::Led::Off };
        assert!(w.status(st.clone()).is_some());
        assert!(w.status(st.clone()).is_none());
        assert!(w.status(SourceStatus { line: "playing".into(), led: crate::Led::Playing, ..st }).is_some());
        assert_eq!(w.sources().len(), 1);
    }
}
