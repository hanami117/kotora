//! The update notice and Settings → About Kotora → Check for updates. The check itself (what is
//! sent, once a day, the record, the install kinds) is `kotora_core::update`; this file starts its
//! worker threads, keeps what it answered, and draws the banner and the Settings lines.
//!
//! Nothing here fetches in headless runs or tests (no handle), with `KOTORA_OFFLINE`, or in a
//! test instance unless `KOTORA_UPDATE_URL` points it somewhere. A failed automatic check is only
//! logged (`kotora: update check failed: …`) and tried again an hour later.

use std::f32::consts::PI;

use kotora_core::update::{self, CheckStatus, Install, Latest, Notice, Record};
use rune::{El, col, el, icon, row, text};

use crate::icons;
use crate::model::State;
use crate::theme::{self, PAL};
use crate::ui::{action, mix, small_button};

/// This copy's version and build, as the check compares them.
fn current() -> (&'static str, u32) {
    (update::VERSION, kotora_ctl::BUILD)
}

/// What the update check knows while the app runs (`State::update`).
#[derive(Debug, Default)]
pub struct Updates {
    /// The banner: a newer version that isn't skipped or closed.
    pub notice: Option<Notice>,
    /// Settings → About Kotora: the last check and a manual check's answer.
    pub check: CheckStatus,
    /// The hourly timer was started (once).
    timer: bool,
    /// An automatic check's worker is out.
    running: bool,
    /// The (version, build) closed with ✕: gone until the next start, for the timer too.
    dismissed: Option<(String, u32)>,
    /// The last failed attempt since the last answer: the timer retries an hour after it.
    last_attempt: Option<u64>,
}

/// A test instance never fetches unless `KOTORA_UPDATE_URL` points it somewhere;
/// `KOTORA_OFFLINE` never does.
fn may_fetch() -> bool {
    let var = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    !kotora_core::paths::offline_forced() && !(var("KOTORA_TEST_INSTANCE") && !var("KOTORA_UPDATE_URL"))
}

impl State {
    /// At start (after the settings are read): the day's check when it is on, and the hourly
    /// timer while the app runs (the tray keeps it open for days).
    pub fn start_updates(&mut self) {
        self.start_update_check();
        self.start_update_timer();
    }

    /// The day's check on a worker thread, when it is due (`update::run_auto` decides). Switched
    /// off, the worker only reads when the last check ran (for Settings) and sends nothing.
    pub fn start_update_check(&mut self) {
        let Some(handle) = self.handle.clone() else { return };
        if rune::is_headless() || !may_fetch() || self.update.running {
            return;
        }
        self.update.running = true;
        let enabled = self.prefs.check_updates;
        let _ = std::thread::Builder::new().name("kotora-update".into()).spawn(move || {
            let rec = update::run_auto(enabled);
            let install = update::this_install();
            handle.update_if(move |s: &mut State, _| s.update_known(&rec, install));
        });
    }

    fn start_update_timer(&mut self) {
        let Some(handle) = self.handle.clone() else { return };
        if rune::is_headless() || self.update.timer || !may_fetch() {
            return;
        }
        self.update.timer = true;
        let _ = std::thread::Builder::new().name("kotora-update-timer".into()).spawn(move || {
            loop {
                std::thread::sleep(update::RECHECK);
                // A tick that finds nothing due changes nothing on screen: no frame.
                handle.update_if(|s: &mut State, _| {
                    s.tick_update_check(update::now());
                    false
                });
            }
        });
    }

    /// One tick of the timer at `now`: the day's check when it is due.
    pub fn tick_update_check(&mut self, now: u64) {
        let u = &self.update;
        if update::auto_wanted(self.prefs.check_updates, u.running || u.check.running, u.check.last_check, u.last_attempt, now) {
            self.start_update_check();
        }
    }

    /// What the check knows: the banner when a newer, unskipped version is out (or this one is
    /// below `minimum_supported`, which no skip or ✕ hides). True when something on screen changed.
    pub fn update_known(&mut self, rec: &Record, install: Install) -> bool {
        let before = (self.update.notice.clone(), self.update.check.last_check);
        self.update.running = false;
        self.update.check.last_check = rec.last_check;
        self.update.last_attempt = rec.last_attempt;
        if !self.prefs.check_updates {
            return before != (self.update.notice.clone(), self.update.check.last_check);
        }
        let dismissed = self.update.dismissed.as_ref();
        self.update.notice = rec
            .latest
            .as_ref()
            .and_then(|l| update::notice(l, current(), rec.skipped(), install, cfg!(target_os = "macos")))
            .filter(|n| n.required || dismissed.is_none_or(|(v, b)| *v != n.version || *b != n.build));
        before != (self.update.notice.clone(), self.update.check.last_check)
    }

    /// Settings → About Kotora → Check for updates. On: check now (still at most once a day).
    /// Off: the banner goes, and nothing is sent from now on.
    pub fn set_check_updates(&mut self, on: bool) {
        if self.prefs.check_updates == on {
            return;
        }
        self.prefs.check_updates = on;
        crate::detect::save_setting(self, "app", "check_updates", on.to_string());
        if on {
            self.start_update_check();
        } else {
            self.update.notice = None;
        }
    }

    /// Settings → Check now: asks at once, whatever the day's limit or a skipped version, and
    /// says what it found in Settings.
    pub fn check_updates_now(&mut self) {
        if self.update.check.running {
            return;
        }
        let Some(handle) = self.handle.clone() else { return };
        if rune::is_headless() || !may_fetch() {
            let last = self.update.check.last_check;
            self.manual_answer(Err("off in offline and test copies".into()), last, Install::Direct);
            return;
        }
        self.update.check.running = true;
        self.update.check.result = None;
        let _ = std::thread::Builder::new().name("kotora-update".into()).spawn(move || {
            let (answer, rec) = update::run_manual();
            let install = update::this_install();
            handle.update_if(move |s: &mut State, _| {
                s.update.last_attempt = rec.last_attempt;
                s.manual_answer(answer, rec.last_check, install);
                true
            });
        });
    }

    /// A manual check's answer: a newer version shows the banner (even a skipped one); "up to
    /// date" and failures show in Settings.
    pub fn manual_answer(&mut self, answer: Result<Latest, String>, last_check: Option<u64>, install: Install) {
        self.update.check.running = false;
        self.update.check.last_check = last_check.or(self.update.check.last_check);
        let notice = answer.as_ref().ok().and_then(|l| update::notice(l, current(), None, install, cfg!(target_os = "macos")));
        let line = match (&answer, &notice) {
            (_, Some(n)) => n.message(),
            (Ok(_), None) => update::UP_TO_DATE.to_string(),
            (Err(_), None) => update::CHECK_FAILED.to_string(),
        };
        if notice.is_some() {
            self.update.notice = notice;
            self.update.dismissed = None;
        }
        self.update.check.result = Some(line);
    }

    /// ✕: gone until the next start. Not for the below-minimum warning.
    pub fn dismiss_update(&mut self) {
        if self.update.notice.as_ref().is_some_and(|n| n.required) {
            return;
        }
        self.update.dismissed = self.update.notice.take().map(|n| (n.version, n.build));
    }

    /// "Skip this version": gone, and this (version, build) never shows again; a newer one will.
    /// Not for the below-minimum warning.
    pub fn skip_update(&mut self) {
        if self.update.notice.as_ref().is_some_and(|n| n.required) {
            return;
        }
        if let Some(n) = self.update.notice.take()
            && self.handle.is_some()
            && !rune::is_headless()
        {
            let _ = std::thread::Builder::new().name("kotora-update".into()).spawn(move || update::save_skip(n.version, n.build));
        }
    }

    /// Scenes and tests: the banner as a check that found `latest` would show it.
    pub fn fake_update(&mut self, latest: Latest, install: Install) {
        let rec = Record { last_check: Some(self.clock() as u64 - 3600), latest: Some(latest), ..Default::default() };
        self.update_known(&rec, install);
    }
}

/// Opens a link from `latest.json`: web links only (never `file:` or a custom scheme).
fn open(key: &'static str, label: &'static str, url: String, primary: bool) -> El {
    action(key.into(), label, primary, move |s: &mut State, _| {
        if update::is_web(&url) {
            crate::online::open_url(s, url.clone());
        }
    })
}

/// The banner on top of every page: a newer version is out. What's new and Download open the
/// browser; nothing is downloaded by the app. Package and install.sh copies get no Download
/// button and say how to update. Below `minimum_supported` it is a warning without Skip and ✕.
pub fn banner(s: &State) -> Option<El> {
    let n = s.update.notice.as_ref()?;
    let tone = if n.required { PAL.status(kotora_core::Status::Dropped) } else { PAL.st_completed };
    let name = match n.build {
        0 | 1 => format!("Kotora {}", n.version),
        b => format!("Kotora {} (build {b})", n.version),
    };
    let head = if n.required { format!("This version of Kotora no longer works. Please update to {name}.") } else { format!("{name} is available") };
    let small = n.how_to_update().unwrap_or(if n.required { "Kotora keeps running, but it may fail to sync." } else { "Nothing is downloaded until you click Download." });
    let mut b = row()
        .key("update-notice")
        .mx(theme::PAGE_PAD)
        .mt(14.0)
        .px(14.0)
        .py(10.0)
        .gap(12.0)
        .center_y()
        .radius(theme::R_LG)
        .linear_gradient(PI / 2.0, [(0.0, mix(PAL.bg, tone, 0.12)), (1.0, mix(PAL.bg, tone, 0.04))])
        .border(1.0, tone.alpha(0.3))
        .child(icon(if n.required { &icons::ALERT } else { &icons::DOWNLOAD }, 16.0).color(tone))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text(head).key("update-notice/text").font(theme::BODY_MED).color(PAL.text).wrap())
                .child(text(small.to_string()).key("update-notice/how").font(theme::LABEL).color(PAL.dim).wrap()),
        );
    if update::is_web(&n.notes_url) {
        b = b.child(open("update-notes", "What\u{2019}s new", n.notes_url.clone(), false));
    }
    if let Some(url) = &n.download {
        b = b.child(open("update-download", "Download", url.clone(), false));
    }
    if n.required {
        return Some(b);
    }
    Some(
        b.child(crate::ui::ghost("update-skip", "Skip this version", State::skip_update).h(26.0)).child(
            el().key("update-dismiss")
                .role("button")
                .label("Close")
                .focusable(true)
                .size(26.0, 26.0)
                .radius(13.0)
                .center()
                .hover(PAL.hover())
                .cursor(rune::Cursor::Pointer)
                .on_press(|s: &mut State| s.dismiss_update())
                .child(icon(&icons::CLOSE, 13.0).color(PAL.dim)),
        ),
    )
}

/// Settings → General → Check now: when the last check ran and what it found.
pub fn status_note(s: &State) -> String {
    let u = &s.update.check;
    let mut note = update::last_checked(u.last_check, s.clock().max(0) as u64);
    if let Some(n) = s.update.notice.as_ref().filter(|n| n.required) {
        note = format!("{} {note}.", n.message());
    } else if let Some(result) = &u.result {
        note = format!("{result} {note}.");
    } else {
        note.push('.');
    }
    note
}

/// Settings → General → Check now: the button (disabled while a check runs).
pub fn check_button(s: &State) -> El {
    if s.update.check.running {
        small_button("set/check-now", "Checking\u{2026}", false, None)
    } else {
        small_button("set/check-now", "Check now", false, Some(State::check_updates_now))
    }
}

/// Settings → General → Check for updates (labelled as kotora.moe says).
pub const CHECK_NOTE: &str = "Once a day, asks kotora.moe whether a new version is out, and says so. It sends only the Kotora version, the system, the processor type and how Kotora was installed. Nothing is downloaded.";

#[cfg(test)]
mod tests {
    use super::*;
    use kotora_core::update::Downloads;

    fn latest(v: &str) -> Latest {
        Latest {
            version: v.into(),
            notes_url: "https://github.com/hanami117/kotora/releases/tag/v9".into(),
            downloads: Downloads { macos_dmg: "https://example.com/K.dmg".into(), linux_tarball: "https://example.com/k.tar.gz".into(), ..Default::default() },
            ..Default::default()
        }
    }

    fn record(v: &str, last: u64) -> Record {
        Record { last_check: Some(last), latest: Some(latest(v)), ..Default::default() }
    }

    #[test]
    fn a_newer_version_shows_and_skip_or_close_hide_it() {
        let mut s = State::fake();
        assert!(s.update_known(&record("99.0.0", 10), Install::Direct));
        assert_eq!(s.update.notice.as_ref().map(|n| n.version.as_str()), Some("99.0.0"));
        assert!(!s.update_known(&record("99.0.0", 10), Install::Direct), "the same answer: no visible change");
        s.dismiss_update();
        assert!(s.update.notice.is_none());
        s.update_known(&record("99.0.0", 20), Install::Direct);
        assert!(s.update.notice.is_none(), "closed until the next start, also for the timer");
        s.update_known(&record("99.0.1", 30), Install::Direct);
        assert!(s.update.notice.is_some(), "a newer one shows");
        s.skip_update();
        assert!(s.update.notice.is_none());
        // This version or older: nothing.
        s.update_known(&record(update::VERSION, 40), Install::Direct);
        assert!(s.update.notice.is_none());
    }

    #[test]
    fn switched_off_nothing_shows() {
        let mut s = State::fake();
        s.prefs.check_updates = false;
        s.update_known(&record("99.0.0", 10), Install::Direct);
        assert!(s.update.notice.is_none());
        assert_eq!(s.update.check.last_check, Some(10), "Settings still says when");
        s.prefs.check_updates = true;
        s.update_known(&record("99.0.0", 10), Install::Direct);
        assert!(s.update.notice.is_some());
        s.set_check_updates(false);
        assert!(s.update.notice.is_none(), "turning it off removes the banner");
    }

    #[test]
    fn below_the_minimum_the_warning_stays() {
        let mut s = State::fake();
        let rec = Record { last_check: Some(1), latest: Some(Latest { minimum_supported: Some("99.0.0".into()), ..latest("99.0.0") }), skipped: Some("99.0.0".into()), ..Default::default() };
        s.update_known(&rec, Install::Aur);
        let n = s.update.notice.clone().expect("shows even when skipped");
        assert!(n.required && n.download.is_none());
        s.dismiss_update();
        s.skip_update();
        assert!(s.update.notice.is_some(), "no ✕ or Skip for the warning");
    }

    #[test]
    fn nothing_is_fetched_without_a_live_app() {
        // State::fake has no handle (headless and tests): the checks are no-ops.
        let mut s = State::fake();
        s.start_updates();
        assert!(!s.update.running && !s.update.timer);
        s.check_updates_now();
        assert!(!s.update.check.running);
        s.tick_update_check(update::now());
        assert!(!s.update.running);
    }

    #[test]
    fn a_manual_answer_says_what_it_found() {
        let mut s = State::fake();
        s.manual_answer(Ok(latest(update::VERSION)), Some(5), Install::Direct);
        assert_eq!(s.update.check.result.as_deref(), Some(update::UP_TO_DATE));
        assert!(s.update.notice.is_none());
        s.manual_answer(Err("offline".into()), None, Install::Direct);
        assert_eq!(s.update.check.result.as_deref(), Some(update::CHECK_FAILED));
        assert_eq!(s.update.check.last_check, Some(5), "a failure keeps the last check");
        s.manual_answer(Ok(latest("99.0.0")), Some(6), Install::Homebrew);
        let n = s.update.notice.as_ref().unwrap();
        assert!(n.download.is_none(), "Homebrew: no Download");
        assert!(s.update.check.result.as_deref().unwrap().contains("brew upgrade"));
    }
}
