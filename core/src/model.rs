//! The data model the store, the services and the UI share.

use crate::Status;

/// An anime, as far as Kotora keeps it: only media on the user's list (no hoarding).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Media {
    /// The service's id (MyAnimeList id in v0.1).
    pub id: u32,
    pub title: String,
    pub title_en: Option<String>,
    pub title_ja: Option<String>,
    pub synonyms: Vec<String>,
    /// `None`: unknown (still airing with no count).
    pub episodes: Option<u32>,
    /// The service's raw format: `tv`, `movie`, `ova`, …
    pub format: String,
    /// The service's raw airing status: `currently_airing`, `finished_airing`, `not_yet_aired`
    /// (empty when unknown).
    pub air_status: String,
    pub cover_url: Option<String>,
}

impl Media {
    pub fn is_airing(&self) -> bool {
        self.air_status == "currently_airing"
    }

    /// Every title, for the search filter.
    pub fn all_titles(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.title.as_str())
            .chain(self.title_en.as_deref())
            .chain(self.title_ja.as_deref())
            .chain(self.synonyms.iter().map(String::as_str))
            .filter(|t| !t.is_empty())
    }
}

/// One entry of the user's list: the media and the list status. This is the cached remote entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    pub media: Media,
    pub status: Status,
    pub progress: u32,
    /// 1-10; `None`: not scored.
    pub score: Option<u8>,
    /// Watching a completed anime again (MAL `is_rewatching`; the status stays Completed).
    pub rewatching: bool,
    /// Finished rewatches (MAL `num_times_rewatched`).
    pub times_rewatched: u32,
    /// The remote `updated_at`, unix seconds (the base for conflict checks in stage 3).
    pub updated_at: i64,
}

/// The list fields an edit can change: status, progress, score and the rewatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Values {
    pub status: Status,
    pub progress: u32,
    /// 1-10; `None`: not scored.
    pub score: Option<u8>,
    pub rewatching: bool,
    pub times_rewatched: u32,
}

impl Values {
    /// Status, progress and score; not rewatching, never rewatched.
    pub const fn new(status: Status, progress: u32, score: Option<u8>) -> Values {
        Values { status, progress, score, rewatching: false, times_rewatched: 0 }
    }
}

/// One edit, or several merged: only the fields that change (`None`: unchanged).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Change {
    pub status: Option<Status>,
    pub progress: Option<u32>,
    /// `Some(None)` removes the score.
    pub score: Option<Option<u8>>,
    pub rewatching: Option<bool>,
    pub times_rewatched: Option<u32>,
}

impl Change {
    pub fn is_empty(&self) -> bool {
        self.status.is_none() && self.progress.is_none() && self.score.is_none() && self.rewatching.is_none() && self.times_rewatched.is_none()
    }

    /// `later` on top of `self`: the newer value of each field wins.
    pub fn merge(self, later: Change) -> Change {
        Change {
            status: later.status.or(self.status),
            progress: later.progress.or(self.progress),
            score: later.score.or(self.score),
            rewatching: later.rewatching.or(self.rewatching),
            times_rewatched: later.times_rewatched.or(self.times_rewatched),
        }
    }

    /// Drops the fields that already hold their value in `v` (an edit back to where it was).
    pub fn without_same(self, v: &Values) -> Change {
        Change {
            status: self.status.filter(|s| *s != v.status),
            progress: self.progress.filter(|p| *p != v.progress),
            score: self.score.filter(|s| *s != v.score),
            rewatching: self.rewatching.filter(|r| *r != v.rewatching),
            times_rewatched: self.times_rewatched.filter(|n| *n != v.times_rewatched),
        }
    }

    pub fn apply(&self, v: Values) -> Values {
        Values {
            status: self.status.unwrap_or(v.status),
            progress: self.progress.unwrap_or(v.progress),
            score: self.score.unwrap_or(v.score),
            rewatching: self.rewatching.unwrap_or(v.rewatching),
            times_rewatched: self.times_rewatched.unwrap_or(v.times_rewatched),
        }
    }

    /// Whether `v` already holds every value of this change.
    pub fn is_in(&self, v: &Values) -> bool {
        self.without_same(v).is_empty()
    }

    /// Start a rewatch of a completed anime (Taiga, MAL): rewatching, progress back to 0, the
    /// status stays Completed. `None` unless the entry is Completed and not rewatching already.
    pub fn start_rewatch(cur: &Values) -> Option<Change> {
        (cur.status == Status::Completed && !cur.rewatching).then_some(Change { progress: Some(0), rewatching: Some(true), ..Change::default() })
    }

    /// The rules of the last episode, as the MAL website has them: a change that reaches the last
    /// episode (`episodes`, when known) completes the entry; during a rewatch it ends the rewatch
    /// instead, one more in the count, in the same change. A change that picks the status or the
    /// rewatch itself is left alone.
    pub fn completes(mut self, cur: &Values, episodes: Option<u32>) -> Change {
        let reaches = matches!((self.progress, episodes), (Some(p), Some(n)) if n > 0 && p == n && cur.progress < n);
        if !reaches || self.status.is_some_and(|s| s != Status::Completed) || self.rewatching.is_some() {
            return self;
        }
        if cur.rewatching {
            self.status = Some(Status::Completed);
            self.rewatching = Some(false);
            self.times_rewatched = Some(cur.times_rewatched + 1);
        } else if cur.status != Status::Completed {
            self.status = Some(Status::Completed);
        }
        self
    }
}

/// The service's copy of one entry (MAL `my_list_status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Remote {
    pub values: Values,
    /// Unix seconds.
    pub updated_at: i64,
}

impl ListItem {
    pub fn values(&self) -> Values {
        Values { status: self.status, progress: self.progress, score: self.score, rewatching: self.rewatching, times_rewatched: self.times_rewatched }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done(times: u32) -> Values {
        Values { times_rewatched: times, ..Values::new(Status::Completed, 12, Some(8)) }
    }

    #[test]
    fn a_rewatch_starts_only_on_a_completed_entry() {
        let c = Change::start_rewatch(&done(0)).unwrap();
        assert_eq!(c.apply(done(0)), Values { progress: 0, rewatching: true, ..done(0) }, "still Completed");
        assert_eq!(Change::start_rewatch(&Values::new(Status::Watching, 3, None)), None);
        assert_eq!(Change::start_rewatch(&c.apply(done(0))), None, "already rewatching");
    }

    #[test]
    fn the_last_episode_completes_or_ends_the_rewatch() {
        let ep = |n| Change { progress: Some(n), ..Change::default() };
        // A first watch: Completed.
        let w = Values::new(Status::Watching, 11, None);
        assert_eq!(ep(12).completes(&w, Some(12)).status, Some(Status::Completed));
        assert_eq!(ep(11).completes(&Values::new(Status::Watching, 10, None), Some(12)), ep(11));
        assert_eq!(ep(12).completes(&w, None), ep(12), "unknown count: nothing more");
        // A rewatch: one change ends it and counts it.
        let re = Values { progress: 11, rewatching: true, ..done(1) };
        let c = ep(12).completes(&re, Some(12));
        assert_eq!(c, Change { progress: Some(12), status: Some(Status::Completed), rewatching: Some(false), times_rewatched: Some(2), score: None });
        assert_eq!(c.apply(re), Values { progress: 12, rewatching: false, times_rewatched: 2, ..done(1) });
        assert_eq!(ep(5).completes(&re, Some(12)), ep(5), "mid-rewatch: normal counting");
        // A status picked by hand wins.
        let drop = Change { status: Some(Status::Dropped), ..ep(12) };
        assert_eq!(drop.completes(&re, Some(12)), drop);
    }
}
