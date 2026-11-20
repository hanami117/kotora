//! Home's "Airing this week" shelf and the manga details page's Publishing card, as the
//! desktop's (`app/src/home.rs` `airing_week`, `app/src/publishing.rs` `details_card`). Local
//! reads only: the saved season lists, the list and the saved release files.

use std::collections::HashSet;

use kotora_core::airing::{DAY_SHORT, WEEK};
use kotora_core::time::{self, Clock};
use kotora_core::{Scope, Status};

use crate::pages::{AiringMine, AiringOrder, AiringType, PublishingLook, PublishingSort};
use crate::{Kotora, KotoraError, ListStatus};

/// One card of Home's "Airing this week": the next episode of an airing show on the list.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct HomeAiring {
    pub id: u32,
    pub title: String,
    pub cover_url: Option<String>,
    /// "Ep 5" (the chip on the poster).
    pub episode: String,
    /// The episode airs today (the chip is filled).
    pub today: bool,
    /// "Today 16:30", "Tomorrow 01:05", "Sat 23:00".
    pub when: String,
    pub at: i64,
    pub status: ListStatus,
}

/// The Publishing card under "Your list" on a Reading manga's details page.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PublishingCard {
    /// "Ch 121 · yesterday", "Looking…" or "–".
    pub latest: String,
    /// "Caught up", "2 to read" or "–".
    pub to_read: String,
    /// "Weekly, next around Fri Jan 22" or "–".
    pub next: String,
    /// The match's error, when the last check failed.
    pub error: Option<String>,
    /// "Release data from MangaUpdates. Next dates are estimates from past releases."
    pub credit: String,
}

/// "Today 16:30", "Tomorrow 00:01", "Sat 23:00": the desktop's `Airing::when`.
fn when(at: i64, now: i64) -> String {
    let c = Clock::local(at);
    let day = match c.day - Clock::local(now).day {
        0 => "Today",
        1 => "Tomorrow",
        _ => DAY_SHORT[usize::from(c.weekday)],
    };
    format!("{day} {}", c.hhmm())
}

impl Kotora {
    pub(crate) fn home_airing_at(&self, now: i64) -> Result<Vec<HomeAiring>, KotoraError> {
        let service = self.active_or_err()?;
        let rewatching: HashSet<u32> =
            self.store().load_list(Scope::new(service.name(), kotora_core::Kind::Anime))?.iter().filter(|it| it.rewatching && it.status == Status::Completed).map(|it| it.media.id).collect();
        let page = self.airing_page_at(AiringType::All, AiringMine::On, AiringOrder::Popular, now)?;
        let today = Clock::local(now).day;
        let mut seen = HashSet::new();
        let mut v: Vec<HomeAiring> = page
            .days
            .into_iter()
            .flat_map(|d| d.slots)
            .filter(|s| s.at > now && s.at - now < WEEK)
            .filter_map(|s| {
                let status = s.status?;
                let keeps = matches!(status, ListStatus::Watching | ListStatus::PlanToWatch | ListStatus::OnHold) || rewatching.contains(&s.id);
                (keeps && seen.insert(s.id)).then(|| HomeAiring {
                    id: s.id,
                    title: s.title,
                    cover_url: s.cover_url,
                    episode: format!("Ep {}", s.episode),
                    today: Clock::local(s.at).day == today,
                    when: when(s.at, now),
                    at: s.at,
                    status,
                })
            })
            .collect();
        v.sort_by_key(|a| a.at);
        Ok(v)
    }

    pub(crate) fn publishing_card_at(&self, media_id: u32, now: i64) -> Result<Option<PublishingCard>, KotoraError> {
        let page = self.publishing_page_at(PublishingSort::Next, now)?;
        let Some(r) = page.rows.into_iter().find(|r| r.id == media_id) else { return Ok(None) };
        let latest = if !r.latest.is_empty() {
            if r.when.is_empty() { r.latest } else { format!("{} \u{b7} {}", r.latest, r.when) }
        } else if r.look == PublishingLook::Pending {
            "Looking\u{2026}".into()
        } else {
            "\u{2013}".into()
        };
        let to_read = match r.behind {
            Some(0) => "Caught up".into(),
            Some(n) => format!("{n} to read"),
            None => "\u{2013}".into(),
        };
        let next = if r.pace.is_empty() { "\u{2013}".into() } else if r.next.is_empty() { r.pace } else { format!("{}, {}", r.pace, r.next) };
        Ok(Some(PublishingCard { latest, to_read, next, error: r.error, credit: format!("{}. Next dates are estimates from past releases.", page.credit) }))
    }
}

#[uniffi::export]
impl Kotora {
    /// Home's "Airing this week": the next episode of each airing show on the list (Watching,
    /// Plan to watch, On hold, or a rewatch) within seven days, soonest first. Local.
    pub fn home_airing(&self) -> Result<Vec<HomeAiring>, KotoraError> {
        self.home_airing_at(time::now_unix())
    }

    /// The Publishing card of a Reading manga (`None`: not Reading). Local; Publishing's
    /// refresh fills it.
    pub fn publishing_card(&self, media_id: u32) -> Result<Option<PublishingCard>, KotoraError> {
        self.publishing_card_at(media_id, time::now_unix())
    }
}

#[cfg(test)]
mod tests {
    use kotora_core::Status;
    use kotora_core::airing::WEEK;

    use crate::ListStatus;
    use crate::pages::tests::{EP1, show, signed_in_mal};

    #[test]
    fn home_airing_has_the_next_episode_of_the_list_s_shows_within_a_week() {
        let (k, dir) = signed_in_mal("home-airing");
        let mut done = show(3, "Paper Lanterns", Status::Completed, 12);
        done.media.title = "Paper Lanterns".into();
        let dropped = show(4, "Quiet Harbour", Status::Dropped, 1);
        k.store().replace_list("mal", &[show(1, "Hoshikuzu Relay", Status::Watching, 4), show(2, "Tide Line", Status::PlanToWatch, 0), done, dropped], EP1).unwrap();
        // Friday before episode 6 (JST, the fake platform's zone).
        let now = EP1 + 4 * WEEK + 6 * 86_400 - 3600 * 10;
        let v = k.home_airing_at(now).unwrap();
        let ids: Vec<u32> = v.iter().map(|a| a.id).collect();
        assert_eq!(ids, vec![1, 2], "not the completed or the dropped one");
        assert_eq!((v[0].episode.as_str(), v[0].when.as_str(), v[0].today, v[0].status), ("Ep 6", "Tomorrow 23:30", false, ListStatus::Watching));
        // Saturday morning: today.
        let v = k.home_airing_at(now + 12 * 3600).unwrap();
        assert!(v[0].today && v[0].when == "Today 23:30", "{:?}", v[0]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_publishing_card_is_for_reading_manga_only() {
        let (k, dir) = signed_in_mal("home-publishing");
        let mut reading = crate::pages::tests::fixture(7, "Inkwell Cartographers", Status::Watching, 3);
        reading.media.kind = kotora_core::Kind::Manga;
        let mut plan = reading.clone();
        plan.media.id = 8;
        plan.status = Status::PlanToWatch;
        k.store().replace_list(kotora_core::Scope::new("mal", kotora_core::Kind::Manga), &[reading, plan], EP1).unwrap();
        let c = k.publishing_card_at(7, EP1).unwrap().expect("a Reading manga has the card");
        assert_eq!((c.latest.as_str(), c.to_read.as_str(), c.next.as_str()), ("Looking\u{2026}", "\u{2013}", "\u{2013}"), "nothing checked yet");
        assert!(c.credit.ends_with("Next dates are estimates from past releases."), "{}", c.credit);
        assert_eq!(k.publishing_card_at(8, EP1).unwrap(), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn keep_higher_progress_is_on_by_default_and_saved() {
        let (k, dir) = signed_in_mal("keep-higher");
        let _config = crate::testenv::fresh_config();
        assert!(k.keep_higher_progress());
        k.set_keep_higher_progress(false).unwrap();
        assert!(!k.keep_higher_progress());
        k.set_keep_higher_progress(true).unwrap();
        assert!(k.keep_higher_progress());
        let _ = std::fs::remove_dir_all(dir);
    }
}
