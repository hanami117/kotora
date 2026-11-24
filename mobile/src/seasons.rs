//! The season picker (Android): the season grid of any season, Now and Next and back to 1963,
//! as the desktop's Airing page has it. The current season stays `pages.rs`'s `airing_page`
//! (cards with countdowns, merged with the list); this module only adds the other seasons: one
//! saved list per season asked from the active service, its cards in the desktop's words.
//! `pages.rs` is kept as the iOS branch has it, so the card words it can't share live here.

use std::collections::HashMap;

use kotora_core::airing::{self, Season, SeasonAnime, Timetable};
use kotora_core::service::ServiceError;
use kotora_core::time::{self, Clock};
use kotora_core::{ListItem, Status};

use crate::pages::{AiringCard, AiringMine, AiringOrder, AiringType};
use crate::{Kotora, KotoraError, ListStatus};

const PAGES: u32 = 4;
const DAY_SHORT: [&str; 7] = airing::DAY_SHORT;
const DAY_LONG: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// One season of the picker.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct SeasonPick {
    /// Seasons from the current one: 0 now, 1 next, -1 the one before, …
    pub offset: i32,
    /// "Fall 2026".
    pub label: String,
    pub year: u16,
}

/// A picked season's grid.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SeasonGrid {
    pub season: String,
    /// When its saved list was fetched; `None`: never (call [`Kotora::season_fetch`]).
    pub fetched_at: Option<i64>,
    pub stale: bool,
    pub has_shows: bool,
    pub cards: Vec<AiringCard>,
}

fn keeps_type(t: AiringType, format: &str) -> bool {
    match t {
        AiringType::All => true,
        AiringType::Tv => format == "tv",
        AiringType::Ona => format == "ona",
        AiringType::Movie => format == "movie",
    }
}

fn keeps_mine(m: AiringMine, on: bool) -> bool {
    match m {
        AiringMine::All => true,
        AiringMine::On => on,
        AiringMine::Off => !on,
    }
}

fn format_label(format: &str) -> &'static str {
    match format {
        "tv" => "TV",
        "movie" => "Movie",
        "ova" => "OVA",
        "ona" => "ONA",
        "special" => "Special",
        "tv_special" => "TV special",
        "music" => "Music",
        "cm" => "CM",
        "pv" => "PV",
        _ => "",
    }
}

fn date_text(d: &str) -> String {
    let p: Vec<&str> = d.split('-').collect();
    let month = |m: &str| m.parse::<usize>().ok().filter(|m| (1..=12).contains(m)).map(|m| MONTHS[m - 1]);
    match p.as_slice() {
        [y, m, day] => match (month(m), day.parse::<u32>()) {
            (Some(m), Ok(day)) => format!("{m} {day}, {y}"),
            _ => d.to_string(),
        },
        [y, m] => month(m).map_or_else(|| d.to_string(), |m| format!("{m} {y}")),
        _ => d.to_string(),
    }
}

fn card(show: &SeasonAnime, item: Option<&ListItem>, now: i64) -> (AiringCard, i64) {
    let status = item.map(|it| ListStatus::from(it.status));
    let behind = item.filter(|it| matches!(it.status, Status::Watching | Status::OnHold) || it.rewatching).and_then(|it| {
        let est = Timetable::of_media(&it.media).or_else(|| Timetable::of(show));
        airing::to_watch(&show.air_status, show.episodes, show.next_airing, est, it.progress, now)
    });
    let next = Timetable::of(show).map(|t| item.map_or(t, |it| t.with_progress(it.progress, now))).and_then(|t| t.next(now));
    let mut meta = Vec::new();
    let k = format_label(&show.format);
    if !k.is_empty() {
        meta.push(k.to_string());
    }
    meta.push(match show.episodes {
        Some(1) => "1 ep".into(),
        Some(n) => format!("{n} eps"),
        None => "? eps".into(),
    });
    if let Some(m) = show.mean {
        meta.push(format!("\u{2605} {m:.2}"));
    }
    let when = match (next, show.air_status.as_str()) {
        (Some((ep, at)), _) => {
            let c = Clock::local(at);
            let slot = format!("{} {}", DAY_SHORT[usize::from(c.weekday)], c.hhmm());
            if item.is_some() { format!("{slot} \u{b7} ep {ep} in {}", airing::countdown(at - now)) } else { format!("{slot} \u{b7} ep {ep}") }
        }
        (None, "not_yet_aired") => show.start_date.as_deref().map_or_else(|| "Not yet aired".into(), |d| format!("Starts {}", date_text(d))),
        (None, "finished_airing") => "Finished airing".into(),
        (None, _) => show.broadcast.map_or_else(String::new, |b| format!("{}s", DAY_LONG[usize::from(b.day % 7)])),
    };
    let lit = item.is_some() && next.is_some_and(|(_, at)| at - now < 2 * 86_400);
    let key = behind.map_or(-1, |w| i64::from(w.behind));
    (
        AiringCard { id: show.id, title: show.title.clone(), title_en: show.title_en.clone(), cover_url: show.cover_url.clone(), meta: meta.join(" \u{b7} "), when, status, lit, to_watch: behind.map(|w| w.label()) },
        key,
    )
}

#[uniffi::export]
impl Kotora {
    /// The picker's seasons: Next, Now, then back `back` seasons (not before 1963).
    pub fn season_picks(&self, back: u32) -> Vec<SeasonPick> {
        let cur = Season::of(time::now_unix());
        let first = (-(back as i32)).max(Season::OLDEST.index() - cur.index());
        (first..=1).rev().map(|offset| cur.shift(offset)).zip((first..=1).rev()).map(|(s, offset)| SeasonPick { offset, label: s.label(), year: s.year }).collect()
    }

    /// A picked season's grid from its saved list and the list. Local.
    pub fn season_grid(&self, offset: i32, kind: AiringType, mine: AiringMine, order: AiringOrder) -> Result<SeasonGrid, KotoraError> {
        let service = self.active_or_err()?;
        let now = time::now_unix();
        let cur = Season::of(now);
        let se = cur.shift(offset);
        let store = self.store();
        let saved = store.season(service.name(), se)?;
        let list = store.load_list(service.name())?;
        drop(store);
        let stale = !saved.as_ref().is_some_and(|(_, at)| se.is_fresh(cur, *at, now));
        let fetched_at = saved.as_ref().map(|(_, at)| *at);
        let shows = saved.map(|s| s.0).unwrap_or_default();
        let items: HashMap<u32, &ListItem> = list.iter().map(|it| (it.media.id, it)).filter(|(_, it)| it.status != Status::Dropped).collect();
        let mut cards: Vec<(AiringCard, i64)> =
            shows.iter().filter(|s| keeps_type(kind, &s.format) && keeps_mine(mine, items.contains_key(&s.id))).map(|s| card(s, items.get(&s.id).copied(), now)).collect();
        if order == AiringOrder::ToWatch {
            cards.sort_by_key(|(_, k)| std::cmp::Reverse(*k));
        }
        Ok(SeasonGrid { season: se.label(), fetched_at, stale, has_shows: !shows.is_empty(), cards: cards.into_iter().map(|c| c.0).collect() })
    }

    /// Asks the active service for a picked season when its saved list is old (or `force`).
    /// Network: call it off the main thread.
    pub fn season_fetch(&self, offset: i32, force: bool) -> Result<(), KotoraError> {
        let service = self.active_or_err()?;
        let now = time::now_unix();
        let cur = Season::of(now);
        let se = cur.shift(offset);
        let fresh = self.store().season(service.name(), se)?.is_some_and(|(_, at)| se.is_fresh(cur, at, now));
        if fresh && !force {
            return Ok(());
        }
        let mut svc = self.service(service)?;
        let mut all = Vec::new();
        for page in 0..PAGES {
            let (mut v, more) = svc.season(se, page).map_err(|e| {
                let title = service.id().title();
                match e {
                    ServiceError::Offline(_) => KotoraError::Offline { message: format!("Can\u{2019}t reach {title}.") },
                    ServiceError::SignInAgain => KotoraError::SignInAgain,
                    ServiceError::RateLimited => KotoraError::RateLimited,
                    ServiceError::Other(m) => KotoraError::Other { message: m },
                }
            })?;
            all.append(&mut v);
            if !more {
                break;
            }
        }
        // The current and previous seasons stay (the week and the notifications use them).
        self.store().save_season(service.name(), se, &all, now, &[cur, cur.previous()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use kotora_core::airing::Season;

    #[test]
    fn offsets_cross_the_year() {
        let fall = Season { year: 2026, quarter: 3 };
        assert_eq!(fall.shift(1).label(), "Winter 2027");
        assert_eq!(fall.shift(-4).label(), "Fall 2025");
    }
}
