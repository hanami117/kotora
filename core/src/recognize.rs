//! Recognition: file name → anime on the user's list, and the episode.
//!
//! 1. Parse the file name with anitomy (the `anitomy-ng` port, MPL-2.0). Without a title in the
//!    name, the folder's name gives it.
//! 2. The cache, per title and season: the user's own pick ("Wrong anime…") or "not an anime"
//!    wins. A pick of another season than the one the file names gives way: to that season on
//!    the list ([`on_list`]), else to a search hit naming it ([`from_search_picked`]).
//! 3. The user's list: every title (main, English, Japanese, synonyms), normalised, with the
//!    season split off ("2nd Season", "Second Season", "Season 2", "第2期", "S2", "II"). The same
//!    title and season is a sure match, also when the file's title is the start of the list's
//!    ("Yakusou S03" for "Yakusou no Sasayaki 3rd Season"). Another season never matches; a
//!    close title (letter pairs, Sørensen–Dice) is unsure and asks.
//! 4. Nothing on the list: one service search, cached for 30 days. A hit on the list is unsure;
//!    a hit elsewhere is shown as "not on your list" (adding comes later, v0.4).
//!
//! 5. anime-relations ([`redirect`], [`relations`]): an episode numbered on past a season's end
//!    ("Season 2 - 15") moves to the anime it belongs to ("2nd Season Part 2", episode 2), as in
//!    Taiga.
//!
//! [`offer`] then says which episode to offer, if any (never at or below the progress, unless
//! rewatching).

pub mod relations;
mod release;

pub use release::{Codec, Release, parse_release};

use anitomy_ng::{ElementKind, Options};
use relations::Relations;

use crate::ListItem;
use crate::service::SearchHit;

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
    s.split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .next_back()
}

fn parse_name(name: &str) -> Parsed {
    let els = anitomy_ng::parse(name, Options::default());
    let first = |k: ElementKind| {
        els.iter()
            .find(|e| e.kind == k)
            .map(|e| e.value.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let episode = els
        .iter()
        .filter(|e| e.kind == ElementKind::Episode)
        .filter_map(|e| last_number(&e.value))
        .next_back();
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
        let folder = p
            .parent()
            .and_then(|d| d.file_name())
            .map(|d| parse_name(&d.to_string_lossy()));
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
    let digits = w
        .strip_suffix("st")
        .or_else(|| w.strip_suffix("nd"))
        .or_else(|| w.strip_suffix("rd"))
        .or_else(|| w.strip_suffix("th"))?;
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

fn ordinal_word(w: &str) -> Option<u32> {
    ordinal(w).or(match w {
        "second" => Some(2),
        "third" => Some(3),
        "fourth" => Some(4),
        "fifth" => Some(5),
        "sixth" => Some(6),
        _ => None,
    })
}

/// "第3期" (Japanese "3rd period"), alone or at the end of a word: (what comes before it, 3).
fn ki(w: &str) -> Option<(&str, u32)> {
    let inner = w.strip_suffix('期')?;
    let at = inner.rfind('第')?;
    let n = inner[at + '第'.len_utf8()..].parse().ok()?;
    Some((&inner[..at], n))
}

/// Splits a season marker off a normalised title: "2nd season", "second season", "season 2",
/// "第2期" (also before a subtitle: "x 2nd season the sky" is "x the sky", season 2), or "s2"
/// and "ii" at the end.
pub fn split_season(norm: &str) -> (String, Option<u32>) {
    let words: Vec<&str> = norm.split(' ').collect();
    let n = words.len();
    let before = |k: usize, season: u32| (words[..k].join(" "), Some(season));
    // Only the marker goes: "x 2nd season part 2" is "x part 2", season 2 (not "x").
    let without = |from: usize, to: usize, season: u32| {
        (
            words[..from]
                .iter()
                .chain(&words[to..])
                .copied()
                .collect::<Vec<_>>()
                .join(" "),
            Some(season),
        )
    };
    // The last marker after the first word wins.
    for i in (1..n).rev() {
        if words[i] == "season" {
            if i + 1 < n
                && let Ok(s) = words[i + 1].parse()
            {
                return without(i, i + 2, s);
            }
            if i >= 2
                && let Some(s) = ordinal_word(words[i - 1])
            {
                return without(i - 1, i + 1, s);
            }
        }
        if let Some((head, s)) = ki(words[i]) {
            let mut rest: Vec<&str> = words[..i].to_vec();
            if !head.is_empty() {
                rest.push(head);
            }
            rest.extend(&words[i + 1..]);
            return (rest.join(" "), Some(s));
        }
    }
    if n == 1
        && let Some((head, s)) = ki(words[0]).filter(|(h, _)| !h.is_empty())
    {
        return (head.to_string(), Some(s));
    }
    if n >= 2 {
        let last = words[n - 1];
        if let Some(s) = last
            .strip_prefix('s')
            .and_then(|d| d.parse().ok())
            .filter(|_| last.len() <= 3)
        {
            return before(n - 1, s);
        }
        if let Some(s) = roman(last) {
            return before(n - 1, s);
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

/// The season the file names, in its name ("S03E07") or its title ("2nd Season"). `None`: it
/// names none (season 1 then, for matching).
pub fn file_season(p: &Parsed) -> Option<u32> {
    p.season.or(split_season(&normalize(&p.title)).1)
}

/// The seasons an anime's titles name: one title is enough ("Yakusou no Sasayaki: Kouhen" with
/// the English "… Season 3" is season 3). None named: season 1.
fn seasons_of<'a>(titles: impl Iterator<Item = &'a str>) -> Vec<u32> {
    let mut v: Vec<u32> = titles
        .filter_map(|t| split_season(&normalize(t)).1)
        .collect();
    if v.is_empty() {
        v.push(1);
    }
    v
}

/// The file names a season and the entry is another season: "S03E07" against "Yakusou no
/// Sasayaki" (season 1).
pub fn season_conflict(p: &Parsed, item: &ListItem) -> bool {
    file_season(p).is_some_and(|s| !seasons_of(item.media.all_titles()).contains(&s))
}

/// The shortest base title a prefix match accepts ("Yaku" is too short to mean a series).
const PREFIX_MIN: usize = 4;
/// A prefix match in the file's season: sure when alone, below a whole title's 1.0.
const PREFIX_SURE: f32 = 0.99;

/// Every list entry scored against the parsed title, best first (only those above 0.3). A
/// file's title that a list title starts with ("Yakusou" for "Yakusou no Sasayaki 3rd Season")
/// counts as the same series: the season then decides (sure when the file names the season).
/// Another season of the same title scores 0. Equal scores: the entry being watched first.
pub fn candidates(p: &Parsed, items: &[ListItem]) -> Vec<Candidate> {
    let (base, _) = split_season(&normalize(&p.title));
    let named = file_season(p);
    let season = named.unwrap_or(1);
    let prefix = format!("{base} ");
    let mut out: Vec<(Candidate, bool)> = items
        .iter()
        .map(|it| {
            let in_season = seasons_of(it.media.all_titles()).contains(&season);
            let score = it
                .media
                .all_titles()
                .map(|t| {
                    let (b, s) = split_season(&normalize(t));
                    let same_season = s.unwrap_or(1) == season;
                    if b == base {
                        if same_season { 1.0 } else { 0.0 }
                    } else if base.chars().count() >= PREFIX_MIN && b.starts_with(&prefix) {
                        // The series by its start; its season from any of its titles.
                        match (in_season, named) {
                            (false, _) => 0.0,
                            (true, Some(_)) => PREFIX_SURE,
                            (true, None) => 0.9,
                        }
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
            let watching = it.rewatching || it.status == crate::Status::Watching;
            (
                Candidate {
                    media_id: it.media.id,
                    title: it.media.title.clone(),
                    score,
                },
                watching,
            )
        })
        .filter(|(c, _)| c.score > 0.3)
        .collect();
    out.sort_by(|(a, aw), (b, bw)| b.score.total_cmp(&a.score).then(bw.cmp(aw)));
    out.into_iter().map(|(c, _)| c).collect()
}

/// What recognition found.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    /// On the user's list. `sure`: the same title and season, or the user's pick.
    List {
        media_id: u32,
        sure: bool,
        why: String,
    },
    /// A search found it, but it is not on the list.
    NotOnList {
        media_id: u32,
        title: String,
    },
    /// The user said this file is not an anime.
    Ignored,
    Nothing,
}

/// The cache's answer for a key: (media id, source, title).
pub type Cached = Option<(u32, String, String)>;

/// Steps 2 and 3. `None`: nothing on the list; the caller may search (step 4). `rel` only keeps
/// an episode past the count sure when a relation explains it ([`redirect`] moves it after).
///
/// The user's pick wins, except when the file names a season the picked entry isn't: a pick of
/// season 1 never takes "S03E07". The list's match for that season answers then, and with none
/// `None`: the caller searches, and [`from_search_picked`] keeps the pick only when the search
/// finds nothing naming the file's season. (Picks are kept per title and season, `Parsed::key`,
/// so this only undoes a wrong pick for this very season.)
pub fn on_list(p: &Parsed, items: &[ListItem], cached: &Cached, rel: &Relations) -> Option<Found> {
    let c = candidates(p, items);
    let mut passed_over = None;
    if let Some((id, source, _)) = cached {
        match source.as_str() {
            "ignore" => return Some(Found::Ignored),
            "user" => {
                if let Some(it) = items.iter().find(|it| it.media.id == *id) {
                    if !season_conflict(p, it) {
                        return Some(Found::List {
                            media_id: *id,
                            sure: true,
                            why: "you picked it".into(),
                        });
                    }
                    passed_over = Some((it.media.id, it.media.title.clone()));
                }
            }
            _ => {}
        }
    }
    // The passed-over pick itself is no answer.
    let best = c.first().filter(|b| {
        b.score >= UNSURE_AT && passed_over.as_ref().is_none_or(|(id, _)| *id != b.media_id)
    })?;
    let ties = c.iter().filter(|x| x.score >= best.score).count();
    let mut sure = best.score >= PREFIX_SURE && ties == 1;
    let named = file_season(p).filter(|s| *s > 1);
    let mut why = match (sure, named) {
        (true, Some(s)) if best.score < 1.0 => format!(
            "season {s} of a title in your list that starts with \u{201c}{}\u{201d}",
            p.title
        ),
        (true, Some(s)) => format!("the title and season {s} match your list"),
        (true, None) => "same title in your list".to_string(),
        (false, _) if ties > 1 && best.score >= PREFIX_SURE => {
            format!("{ties} titles in your list match")
        }
        (false, _) => format!("close to a title in your list ({:.0}%)", best.score * 100.0),
    };
    if let Some((_, t)) = passed_over {
        why = format!("{why}; your earlier pick, {t}, is another season");
    }
    // An episode past the count: a different season or numbering, so ask.
    let item = items.iter().find(|it| it.media.id == best.media_id)?;
    if let (Some(ep), Some(n)) = (p.episode, item.media.episodes)
        && ep > n
        && rel.redirect(best.media_id, ep).is_none()
    {
        sure = false;
        why = format!("episode {ep} is past its {n}");
    }
    Some(Found::List {
        media_id: best.media_id,
        sure,
        why,
    })
}

/// Step 4: the search's hits against the list. A hit on the list of another season than the
/// file's comes last: a hit off the list that names the file's season is the better answer.
pub fn from_search(p: &Parsed, hits: &[SearchHit], items: &[ListItem]) -> Found {
    from_search_picked(p, hits, items, None)
}

/// As [`from_search`], with the user's pick that [`on_list`] passed over (another season than
/// the file's): a hit naming the file's season beats it; with none, the pick stands.
pub fn from_search_picked(
    p: &Parsed,
    hits: &[SearchHit],
    items: &[ListItem],
    pick: Option<u32>,
) -> Found {
    let on_list = |h: &SearchHit| items.iter().find(|it| it.media.id == h.id);
    let mut other_season = None;
    for h in hits {
        if let Some(it) = on_list(h) {
            if !season_conflict(p, it) {
                return Found::List {
                    media_id: h.id,
                    sure: false,
                    why: "found by a MyAnimeList search".into(),
                };
            }
            other_season = other_season.or(Some(h.id));
        }
    }
    let off: Vec<&SearchHit> = hits.iter().filter(|h| on_list(h).is_none()).collect();
    let names_season = |h: &&&SearchHit| {
        file_season(p).is_some_and(|s| {
            seasons_of(
                std::iter::once(h.title.as_str()).chain(h.other_titles.iter().map(String::as_str)),
            )
            .contains(&s)
        })
    };
    if let Some(h) = off.iter().find(names_season) {
        return Found::NotOnList {
            media_id: h.id,
            title: h.title.clone(),
        };
    }
    if let Some(id) = pick.filter(|id| items.iter().any(|it| it.media.id == *id)) {
        return Found::List {
            media_id: id,
            sure: true,
            why: "you picked it".into(),
        };
    }
    if let Some(id) = other_season {
        let s = file_season(p).unwrap_or(1);
        return Found::List {
            media_id: id,
            sure: false,
            why: format!("found by a MyAnimeList search, but the file says season {s}"),
        };
    }
    match off.first() {
        Some(h) => Found::NotOnList {
            media_id: h.id,
            title: h.title.clone(),
        },
        None => Found::Nothing,
    }
}

/// The destination's title said shortly: what it adds to the source's title ("Part 2"), or all
/// of it.
fn short_title<'a>(from: &str, to: &'a str) -> &'a str {
    match to
        .strip_prefix(from)
        .and_then(|rest| rest.strip_prefix(' '))
    {
        Some(rest) if !rest.trim().is_empty() => rest.trim(),
        _ => to,
    }
}

/// Step 5: anime-relations. When a rule moves the parsed episode of the found anime, the
/// destination and its episode replace them (`parsed.episode` too, so the offer follows) and
/// `why` says so: "episode 15 of Re:Zero … 2nd Season is episode 2 of Part 2". A destination on
/// the list is a list match (as sure as the source's); one not on the list is
/// [`Found::NotOnList`]. Applied once (no chains), like Taiga.
pub fn redirect(found: Found, parsed: &mut Parsed, items: &[ListItem], rel: &Relations) -> Found {
    let Some(ep) = parsed.episode else {
        return found;
    };
    let (from, sure, why_before, title_before) = match &found {
        Found::List {
            media_id,
            sure,
            why,
        } => (*media_id, *sure, Some(why.as_str()), None),
        Found::NotOnList { media_id, title } => (*media_id, false, None, Some(title.as_str())),
        _ => return found,
    };
    let Some(r) = rel.redirect(from, ep) else {
        return found;
    };
    let title_of = |id: u32| {
        items
            .iter()
            .find(|it| it.media.id == id)
            .map(|it| it.media.title.clone())
    };
    let from_title = title_of(from)
        .or(title_before.map(str::to_string))
        .or(r.from_title.clone())
        .unwrap_or_else(|| "this anime".into());
    let to_item = items.iter().find(|it| it.media.id == r.to);
    let to_title = title_of(r.to)
        .or(r.to_title.clone())
        .unwrap_or_else(|| format!("MyAnimeList anime {}", r.to));
    let because = if r.to == from {
        format!(
            "episode {ep} of {from_title} is its episode {} (numbered on from the season before)",
            r.episode
        )
    } else {
        format!(
            "episode {ep} of {from_title} is episode {} of {}",
            r.episode,
            short_title(&from_title, &to_title)
        )
    };
    parsed.episode = Some(r.episode);
    match to_item {
        Some(_) => {
            // "same title in your list" was about the source; keep a user's pick.
            let why = match why_before {
                Some(w) if w == "you picked it" => format!("{w}; {because}"),
                _ => because,
            };
            Found::List {
                media_id: r.to,
                sure,
                why,
            }
        }
        None => Found::NotOnList {
            media_id: r.to,
            title: to_title,
        },
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

/// The episodes a record of `episode` would skip (the first and the last), when it is two or more
/// ahead of the progress: "Update automatically" asks then instead of recording.
pub fn skipped(progress: u32, episode: u32) -> Option<(u32, u32)> {
    (episode > progress.saturating_add(1)).then(|| (progress + 1, episode - 1))
}

#[cfg(test)]
mod tests;
