//! Recognition (DESIGN.md §5.5): file name → anime on the user's list, and the episode.
//!
//! 1. Parse the file name with anitomy (the `anitomy-ng` port, MPL-2.0). Without a title in the
//!    name, the folder's name gives it.
//! 2. The cache: the user's own pick ("Wrong anime…") or "not an anime" wins.
//! 3. The user's list: every title (main, English, Japanese, synonyms), normalised, with the
//!    season split off ("2nd Season", "Season 2", "S2", "II"). The same title and season is a
//!    sure match; a close one (letter pairs, Sørensen–Dice) is unsure and asks.
//! 4. Nothing on the list: one service search, cached for 30 days. A hit on the list is unsure;
//!    a hit elsewhere is shown as "not on your list" (adding comes later, v0.4).
//!
//! [`offer`] then says which episode to offer, if any (never at or below the progress, unless
//! rewatching).

use anitomy_ng::{ElementKind, Options};

use crate::service::SearchHit;
use crate::ListItem;

/// What the file name says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    /// The file's name (no folder).
    pub file_name: String,
    pub title: String,
    pub episode: Option<u32>,
    pub season: Option<u32>,
    pub group: Option<String>,
    pub resolution: Option<String>,
}

impl Parsed {
    /// The cache key: normalised title and season (`lanterns of kirigaoka#1`).
    pub fn key(&self) -> String {
        let (base, season) = split_season(&normalize(&self.title));
        format!("{base}#{}", self.season.or(season).unwrap_or(1))
    }
}

fn last_number(s: &str) -> Option<u32> {
    // "06", "06v2" (the version is its own element), "01-02" (the last), "12.5" (the whole part).
    s.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).next_back()
}

fn parse_name(name: &str) -> Parsed {
    let els = anitomy_ng::parse(name, Options::default());
    let first = |k: ElementKind| els.iter().find(|e| e.kind == k).map(|e| e.value.trim().to_string()).filter(|v| !v.is_empty());
    let episode = els.iter().filter(|e| e.kind == ElementKind::Episode).filter_map(|e| last_number(&e.value)).next_back();
    Parsed {
        file_name: name.to_string(),
        title: first(ElementKind::Title).unwrap_or_default(),
        episode,
        season: first(ElementKind::Season).and_then(|s| last_number(&s)),
        group: first(ElementKind::ReleaseGroup),
        resolution: first(ElementKind::VideoResolution),
    }
}

/// Parses a path (or a bare file name, or a player's title).
pub fn parse(path: &str) -> Option<Parsed> {
    let p = std::path::Path::new(path);
    let name = p.file_name()?.to_string_lossy().into_owned();
    let mut parsed = parse_name(&name);
    if parsed.title.is_empty() || parsed.title.chars().all(|c| c.is_ascii_digit()) {
        // "06.mkv" in "Lanterns of Kirigaoka/": the folder names the show.
        let folder = p.parent().and_then(|d| d.file_name()).map(|d| parse_name(&d.to_string_lossy()));
        match folder {
            Some(f) if !f.title.is_empty() => {
                parsed.title = f.title;
                parsed.season = parsed.season.or(f.season);
                parsed.group = parsed.group.or(f.group);
            }
            _ => return None,
        }
    }
    Some(parsed)
}

/// Lower case, letters and digits only, single spaces.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with(' ') && !out.is_empty() {
            // "Re:Zero" and "Re Zero" read the same; an apostrophe joins ("Kirigaoka's").
            if c != '\'' && c != '\u{2019}' {
                out.push(' ');
            }
        }
    }
    out.trim_end().to_string()
}

fn ordinal(w: &str) -> Option<u32> {
    let digits = w.strip_suffix("st").or_else(|| w.strip_suffix("nd")).or_else(|| w.strip_suffix("rd")).or_else(|| w.strip_suffix("th"))?;
    digits.parse().ok()
}

fn roman(w: &str) -> Option<u32> {
    match w {
        "ii" => Some(2),
        "iii" => Some(3),
        "iv" => Some(4),
        "v" => Some(5),
        "vi" => Some(6),
        _ => None,
    }
}

/// Splits a season marker off the end of a normalised title.
pub fn split_season(norm: &str) -> (String, Option<u32>) {
    let words: Vec<&str> = norm.split(' ').collect();
    let n = words.len();
    let cut = |k: usize, season: u32| (words[..n - k].join(" "), Some(season));
    if n >= 3 {
        // "season 2", "2nd season"
        if words[n - 2] == "season"
            && let Ok(s) = words[n - 1].parse()
        {
            return cut(2, s);
        }
        if words[n - 1] == "season"
            && let Some(s) = ordinal(words[n - 2])
        {
            return cut(2, s);
        }
    }
    if n >= 2 {
        let last = words[n - 1];
        if let Some(s) = last.strip_prefix('s').and_then(|d| d.parse().ok()).filter(|_| last.len() <= 3) {
            return cut(1, s);
        }
        if let Some(s) = roman(last) {
            return cut(1, s);
        }
    }
    (norm.to_string(), None)
}

/// Sørensen–Dice over letter pairs (spaces dropped): 1.0 the same, 0.0 nothing shared.
pub fn similarity(a: &str, b: &str) -> f32 {
    let pairs = |s: &str| -> Vec<(char, char)> {
        let c: Vec<char> = s.chars().filter(|c| *c != ' ').collect();
        c.windows(2).map(|w| (w[0], w[1])).collect()
    };
    let (pa, mut pb) = (pairs(a), pairs(b));
    if pa.is_empty() || pb.is_empty() {
        return if a == b { 1.0 } else { 0.0 };
    }
    let total = pa.len() + pb.len();
    let mut shared = 0;
    for p in &pa {
        if let Some(i) = pb.iter().position(|q| q == p) {
            pb.swap_remove(i);
            shared += 1;
        }
    }
    2.0 * shared as f32 / total as f32
}

/// Close enough to ask.
pub const UNSURE_AT: f32 = 0.75;

/// One anime on the list and how well it matches.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub media_id: u32,
    pub title: String,
    pub score: f32,
}

/// Every list entry scored against the parsed title, best first (only those above 0.3).
pub fn candidates(p: &Parsed, items: &[ListItem]) -> Vec<Candidate> {
    let (base, season_in_title) = split_season(&normalize(&p.title));
    let season = p.season.or(season_in_title).unwrap_or(1);
    let mut out: Vec<Candidate> = items
        .iter()
        .map(|it| {
            let score = it
                .media
                .all_titles()
                .map(|t| {
                    let (b, s) = split_season(&normalize(t));
                    let same_season = s.unwrap_or(1) == season;
                    if b == base {
                        if same_season { 1.0 } else { 0.0 }
                    } else {
                        // "Lanterns of Kirigaoka Season 2" as one title still counts.
                        let whole = normalize(t);
                        if season > 1 && whole == format!("{base} {season}") {
                            return 0.9;
                        }
                        similarity(&b, &base) * if same_season { 1.0 } else { 0.6 }
                    }
                })
                .fold(0.0f32, f32::max);
            Candidate { media_id: it.media.id, title: it.media.title.clone(), score }
        })
        .filter(|c| c.score > 0.3)
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

/// What recognition found.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    /// On the user's list. `sure`: the same title and season, or the user's pick.
    List { media_id: u32, sure: bool, why: String },
    /// A search found it, but it is not on the list.
    NotOnList { media_id: u32, title: String },
    /// The user said this file is not an anime.
    Ignored,
    Nothing,
}

/// The cache's answer for a key: (media id, source, title).
pub type Cached = Option<(u32, String, String)>;

/// Steps 2 and 3. `None`: nothing on the list; the caller may search (step 4).
pub fn on_list(p: &Parsed, items: &[ListItem], cached: &Cached) -> Option<Found> {
    if let Some((id, source, _)) = cached {
        match source.as_str() {
            "ignore" => return Some(Found::Ignored),
            "user" if items.iter().any(|it| it.media.id == *id) => return Some(Found::List { media_id: *id, sure: true, why: "you picked it".into() }),
            _ => {}
        }
    }
    let c = candidates(p, items);
    let best = c.first().filter(|b| b.score >= UNSURE_AT)?;
    let exact = c.iter().filter(|x| x.score >= 1.0).count();
    let mut sure = best.score >= 1.0 && exact == 1;
    let mut why = if sure { "same title in your list".to_string() } else { format!("close to a title in your list ({:.0}%)", best.score * 100.0) };
    // An episode past the count: a different season or numbering, so ask.
    let item = items.iter().find(|it| it.media.id == best.media_id)?;
    if let (Some(ep), Some(n)) = (p.episode, item.media.episodes)
        && ep > n
    {
        sure = false;
        why = format!("episode {ep} is past its {n}");
    }
    Some(Found::List { media_id: best.media_id, sure, why })
}

/// Step 4: the search's hits against the list.
pub fn from_search(hits: &[SearchHit], items: &[ListItem]) -> Found {
    for h in hits {
        if items.iter().any(|it| it.media.id == h.id) {
            return Found::List { media_id: h.id, sure: false, why: "found by a MyAnimeList search".into() };
        }
    }
    match hits.first() {
        Some(h) => Found::NotOnList { media_id: h.id, title: h.title.clone() },
        None => Found::Nothing,
    }
}

/// The episode to offer for `item`, if any. A one-episode show (a movie) with no number in the
/// name is episode 1. Never an episode at or below the progress, unless rewatching; never past
/// the count.
pub fn offer(item: &ListItem, episode: Option<u32>) -> Option<u32> {
    let ep = episode.or((item.media.episodes == Some(1)).then_some(1))?;
    if ep == 0 || item.media.episodes.is_some_and(|n| ep > n) {
        return None;
    }
    if ep <= item.progress && !item.rewatching {
        return None;
    }
    Some(ep)
}

#[cfg(test)]
mod tests;
