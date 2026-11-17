//! Reading detection, stage 2: a comic file some process has open (any reader), with no page.
//!
//! The process scan (`kotora_detect::comics`) says which comic files are open now; [`Tracker`]
//! turns that weak signal into [`Read`]s:
//!
//! - The comic open now is shown, unfinished, as soon as it is seen.
//! - It counts as read when the **next file of the same series** opens (a higher chapter or
//!   volume), or when it has been open for `minutes` (Settings, default 10). Closed sooner: not
//!   counted.
//! - A counted comic stays the one reported until another one is counted: a file opened after it
//!   doesn't push its question away.
//!
//! These are always weak ([`Read::weak`]): Kotora asks, even with "Update automatically" on.

use std::collections::HashMap;
use std::path::PathBuf;

use super::name;
use super::reading::Read;

/// One open comic, as the scan reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    /// The process: "Simple Comic", "okular".
    pub app: String,
    /// The comic file, or the folder of the image open.
    pub path: String,
}

struct Current {
    open: Open,
    read: Read,
    since: i64,
    counted: bool,
}

/// Follows the open comics between scans.
pub struct Tracker {
    minutes: u32,
    /// When each open path was first seen (gone ones are dropped).
    first: HashMap<String, i64>,
    cur: Option<Current>,
    /// The last report was a counted comic: an unfinished one doesn't replace it.
    holding: bool,
    last: Option<Read>,
}

/// What the file says, as a [`Read`] of a weak source.
pub fn read_of(open: &Open, at: i64) -> Read {
    let p = name::parse_path(&open.path);
    let chapter = p.last_chapter().filter(|n| n.is_finite() && *n >= 1.0).map(|n| n.floor() as u32);
    let volume = p.volume.map(|v| v.last).filter(|n| n.is_finite() && *n >= 1.0).map(|n| n.floor() as u32);
    let path = PathBuf::from(&open.path);
    Read {
        file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| open.path.clone()),
        path,
        series: p.series.clone(),
        chapter,
        volume,
        volume_file: p.chapter.is_none() && volume.is_some(),
        page: 0,
        pages: 0,
        finished: false,
        at,
        source: open.app.clone(),
        weak: true,
        why: None,
    }
}

/// The position of a comic in its series: (volume, chapter), for "the next file".
fn place(r: &Read) -> (u32, u32) {
    (r.volume.unwrap_or(0), r.chapter.unwrap_or(0))
}

impl Tracker {
    pub fn new(minutes: u32) -> Tracker {
        Tracker { minutes: minutes.clamp(1, 240), first: HashMap::new(), cur: None, holding: false, last: None }
    }

    pub fn set_minutes(&mut self, minutes: u32) {
        self.minutes = minutes.clamp(1, 240);
    }

    /// One scan's open comics at `now` (Unix seconds). Returns a [`Read`] to report when it
    /// differs from the last one reported.
    pub fn update(&mut self, open: &[Open], now: i64) -> Option<Read> {
        self.first.retain(|p, _| open.iter().any(|o| o.path == *p));
        for o in open {
            self.first.entry(o.path.clone()).or_insert(now);
        }
        // The comic followed: the current one while open, else the one opened last.
        let still = self.cur.as_ref().is_some_and(|c| open.iter().any(|o| o.path == c.open.path));
        let newest = open.iter().max_by_key(|o| (self.first.get(&o.path).copied().unwrap_or(now), o.path.clone()));
        let mut out = None;
        if !still {
            match newest {
                Some(o) => {
                    let since = self.first.get(&o.path).copied().unwrap_or(now);
                    let next = read_of(o, since);
                    // The next file of the same series: the one before counts as read.
                    if let Some(c) = self.cur.take()
                        && !c.counted
                        && !c.read.series.is_empty()
                        && crate::manga::reading::key(&c.read.series) == crate::manga::reading::key(&next.series)
                        && place(&next) > place(&c.read)
                    {
                        out = Some(Read { finished: true, why: Some(format!("{} opened next", next.label())), ..c.read });
                    }
                    self.cur = Some(Current { open: o.clone(), read: next, since, counted: false });
                }
                None => self.cur = None,
            }
        }
        if out.is_none()
            && let Some(c) = &mut self.cur
            && !c.counted
            && now - c.since >= i64::from(self.minutes) * 60
        {
            c.counted = true;
            out = Some(Read { finished: true, why: Some(format!("open for {} minutes", self.minutes)), ..c.read.clone() });
        }
        if out.is_none()
            && let Some(c) = self.cur.as_ref().filter(|c| !c.counted && !self.holding)
        {
            out = Some(c.read.clone());
        }
        let out = out?;
        self.holding = out.finished;
        if self.last.as_ref() == Some(&out) {
            return None;
        }
        self.last = Some(out.clone());
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(path: &str) -> Open {
        Open { app: "Simple Comic".into(), path: path.into() }
    }

    const M: i64 = 60;

    #[test]
    fn an_open_comic_shows_unfinished_once() {
        let mut t = Tracker::new(10);
        let r = t.update(&[open("/c/Glass Lantern Club/c012.cbz")], 0).unwrap();
        assert_eq!((r.chapter, r.finished, r.weak, r.source.as_str()), (Some(12), false, true, "Simple Comic"));
        assert_eq!(t.update(&[open("/c/Glass Lantern Club/c012.cbz")], M), None, "no change, no report");
    }

    #[test]
    fn the_next_file_counts_the_one_before() {
        let mut t = Tracker::new(10);
        t.update(&[open("/c/Glass Lantern Club/c012.cbz")], 0);
        let r = t.update(&[open("/c/Glass Lantern Club/c013.cbz")], 3 * M).unwrap();
        assert_eq!((r.chapter, r.finished), (Some(12), true));
        assert_eq!(r.why.as_deref(), Some("Ch 13 opened next"));
        // Chapter 13 open now doesn't push the question away …
        assert_eq!(t.update(&[open("/c/Glass Lantern Club/c013.cbz")], 4 * M), None);
        // … until it counts itself.
        let r = t.update(&[open("/c/Glass Lantern Club/c014.cbz")], 6 * M).unwrap();
        assert_eq!((r.chapter, r.finished), (Some(13), true));
    }

    #[test]
    fn minutes_open_count() {
        let mut t = Tracker::new(10);
        t.update(&[open("/c/Glass Lantern Club/c012.cbz")], 0);
        assert_eq!(t.update(&[open("/c/Glass Lantern Club/c012.cbz")], 9 * M), None);
        let r = t.update(&[open("/c/Glass Lantern Club/c012.cbz")], 10 * M).unwrap();
        assert!(r.finished);
        assert_eq!(r.why.as_deref(), Some("open for 10 minutes"));
        assert_eq!(t.update(&[open("/c/Glass Lantern Club/c012.cbz")], 20 * M), None, "counted once");
    }

    #[test]
    fn closed_early_or_another_series_counts_nothing() {
        let mut t = Tracker::new(10);
        t.update(&[open("/c/Glass Lantern Club/c012.cbz")], 0);
        assert_eq!(t.update(&[], M), None);
        let r = t.update(&[open("/c/Tsuki no Niwa/c003.cbz")], 2 * M).unwrap();
        assert_eq!((r.series.as_str(), r.finished), ("Tsuki no Niwa", false));
        // A lower chapter of the same series is no "next".
        let r = t.update(&[open("/c/Tsuki no Niwa/c001.cbz")], 3 * M).unwrap();
        assert_eq!((r.chapter, r.finished), (Some(1), false));
    }

    #[test]
    fn a_volume_folder_of_images() {
        let mut t = Tracker::new(5);
        let r = t.update(&[open("/c/Tsuki no Niwa/Volume 02")], 0).unwrap();
        assert_eq!((r.series.as_str(), r.volume, r.volume_file), ("Tsuki no Niwa", Some(2), true));
        let r = t.update(&[open("/c/Tsuki no Niwa/Volume 03")], M).unwrap();
        assert_eq!((r.volume, r.finished), (Some(2), true));
    }
}
