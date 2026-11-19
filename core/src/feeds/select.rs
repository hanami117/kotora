//! From feed items to "new episodes for your list": matching, the "new" rule, filters (with the
//! reason an item is hidden), one row per (series, episode) with the best release first, and
//! the user's marks. Pure: the caller hands in the list, the picks, the marks and the filters.
//!
//! - **Lists:** Watching, Plan to watch and On hold (decision 3), and a rewatch.
//! - **Matching** is recognition's ([`recognize::on_list`], [`recognize::redirect`]) on those
//!   entries only, with the same picks as files (`Parsed::key`, decision 8). No search: a
//!   title off the list stays off it. Unsure matches never count as new; they go to
//!   "Needs a look".
//! - **New:** an episode past the progress (the rewatch's too), at most the aired count when
//!   known (else the episode count); Plan to watch: episode 1 or a batch only.
//! - **Filters** are plain words and lists (decision 7: no regex).

use std::collections::{HashMap, HashSet};

use crate::recognize::{self, Codec, Found, Release, parse_release, relations::Relations};
use crate::{ListItem, Status};

use super::Item;

/// What a list entry must be for its releases to count.
pub fn counts(it: &ListItem) -> bool {
    it.rewatching || matches!(it.status, Status::Watching | Status::PlanToWatch | Status::OnHold)
}

/// What to do with batches (ranges, season packs, "Complete").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Batches {
    #[default]
    Show,
    Hide,
    /// Only once the series finished airing.
    AfterFinished,
}

/// The filter: the global one, or a series' own after [`Filter::with`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    /// Best first; a group not here comes after them.
    pub groups_preferred: Vec<String>,
    /// Not empty: only these groups.
    pub groups_only: Vec<String>,
    pub groups_never: Vec<String>,
    /// Heights (1080, 720, …); empty: any.
    pub resolutions: Vec<u32>,
    /// The higher resolution first (else the order of `resolutions`, else any).
    pub prefer_highest: bool,
    /// This height first (then by `prefer_highest`); shows the others too.
    pub prefer_resolution: Option<u32>,
    /// `None`: any codec.
    pub codec: Option<Codec>,
    pub ten_bit: bool,
    /// Every one must appear (whole words, any case; "dual audio" is two words in a row).
    pub include: Vec<String>,
    /// None may appear.
    pub exclude: Vec<String>,
    pub batches: Batches,
    pub min_seeders: Option<u32>,
    /// Bytes per episode.
    pub max_size: Option<u64>,
    /// The feeds used; `None`: all.
    pub feeds: Option<Vec<i64>>,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            groups_preferred: Vec::new(),
            groups_only: Vec::new(),
            groups_never: Vec::new(),
            resolutions: Vec::new(),
            prefer_highest: true,
            prefer_resolution: None,
            codec: None,
            ten_bit: true,
            include: Vec::new(),
            exclude: Vec::new(),
            batches: Batches::Show,
            min_seeders: None,
            max_size: None,
            feeds: None,
        }
    }
}

/// A series' own filter: each field `None` inherits the global one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeriesFilter {
    pub groups_preferred: Option<Vec<String>>,
    pub groups_only: Option<Vec<String>>,
    pub groups_never: Option<Vec<String>>,
    pub resolutions: Option<Vec<u32>>,
    pub prefer_highest: Option<bool>,
    pub codec: Option<Option<Codec>>,
    pub ten_bit: Option<bool>,
    pub include: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
    pub batches: Option<Batches>,
    pub min_seeders: Option<Option<u32>>,
    pub max_size: Option<Option<u64>>,
    pub feeds: Option<Option<Vec<i64>>>,
}

impl Filter {
    /// This filter with a series' own fields over it.
    pub fn with(&self, s: &SeriesFilter) -> Filter {
        let f = self.clone();
        Filter {
            groups_preferred: s.groups_preferred.clone().unwrap_or(f.groups_preferred),
            groups_only: s.groups_only.clone().unwrap_or(f.groups_only),
            groups_never: s.groups_never.clone().unwrap_or(f.groups_never),
            resolutions: s.resolutions.clone().unwrap_or(f.resolutions),
            prefer_highest: s.prefer_highest.unwrap_or(f.prefer_highest),
            prefer_resolution: f.prefer_resolution,
            codec: s.codec.unwrap_or(f.codec),
            ten_bit: s.ten_bit.unwrap_or(f.ten_bit),
            include: s.include.clone().unwrap_or(f.include),
            exclude: s.exclude.clone().unwrap_or(f.exclude),
            batches: s.batches.unwrap_or(f.batches),
            min_seeders: s.min_seeders.unwrap_or(f.min_seeders),
            max_size: s.max_size.unwrap_or(f.max_size),
            feeds: s.feeds.clone().unwrap_or(f.feeds),
        }
    }
}

/// Why the filter hides an item (shown in the preview and under "Hidden").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hidden {
    ExcludeWord(String),
    MissingWord(String),
    GroupNever(String),
    GroupNotOnly,
    Resolution,
    Codec,
    TenBit,
    Batch,
    BatchBeforeEnd,
    FewSeeders(u32),
    TooLarge,
    FeedNotUsed,
}

impl std::fmt::Display for Hidden {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Hidden::ExcludeWord(w) => write!(f, "exclude word \u{201c}{w}\u{201d}"),
            Hidden::MissingWord(w) => write!(f, "no \u{201c}{w}\u{201d} in the name"),
            Hidden::GroupNever(g) => write!(f, "group {g} is never shown"),
            Hidden::GroupNotOnly => write!(f, "group not in \u{201c}only these\u{201d}"),
            Hidden::Resolution => write!(f, "resolution not chosen"),
            Hidden::Codec => write!(f, "codec not chosen"),
            Hidden::TenBit => write!(f, "10-bit not allowed"),
            Hidden::Batch => write!(f, "batches hidden"),
            Hidden::BatchBeforeEnd => write!(f, "batch before the series finished airing"),
            Hidden::FewSeeders(n) => write!(f, "only {n} seeders"),
            Hidden::TooLarge => write!(f, "too large per episode"),
            Hidden::FeedNotUsed => write!(f, "feed not used for this series"),
        }
    }
}

/// The name's words, lower case, letters and digits only.
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// `phrase`'s words appear in a row in `ws` (whole words, any case).
fn has_words(ws: &[String], phrase: &str) -> bool {
    let p = words(phrase);
    !p.is_empty() && ws.windows(p.len()).any(|w| w == p.as_slice())
}

fn same_group(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// The first reason the filter hides `item` (a release of `it`, `feed` its feed), if any.
pub fn hidden(f: &Filter, item: &Item, rel: &Release, it: &ListItem, feed: i64) -> Option<Hidden> {
    if f.feeds.as_ref().is_some_and(|fs| !fs.contains(&feed)) {
        return Some(Hidden::FeedNotUsed);
    }
    let ws = words(&item.title);
    if let Some(w) = f.exclude.iter().find(|w| has_words(&ws, w)) {
        return Some(Hidden::ExcludeWord(w.trim().to_string()));
    }
    if let Some(w) = f.include.iter().find(|w| !words(w).is_empty() && !has_words(&ws, w)) {
        return Some(Hidden::MissingWord(w.trim().to_string()));
    }
    let group = rel.parsed.group.as_deref().unwrap_or("");
    if let Some(g) = f.groups_never.iter().find(|g| same_group(g, group)) {
        return Some(Hidden::GroupNever(g.trim().to_string()));
    }
    if !f.groups_only.is_empty() && !f.groups_only.iter().any(|g| same_group(g, group)) {
        return Some(Hidden::GroupNotOnly);
    }
    if !f.resolutions.is_empty() && !rel.height().is_some_and(|h| f.resolutions.contains(&h)) {
        return Some(Hidden::Resolution);
    }
    if f.codec.is_some_and(|c| rel.codec != Some(c)) {
        return Some(Hidden::Codec);
    }
    if !f.ten_bit && rel.ten_bit {
        return Some(Hidden::TenBit);
    }
    if rel.batch {
        match f.batches {
            Batches::Show => {}
            Batches::Hide => return Some(Hidden::Batch),
            Batches::AfterFinished if it.media.air_status != "finished_airing" => return Some(Hidden::BatchBeforeEnd),
            Batches::AfterFinished => {}
        }
    }
    if let (Some(min), Some(s)) = (f.min_seeders, item.seeders)
        && s < min
    {
        return Some(Hidden::FewSeeders(s));
    }
    if let (Some(max), Some(size)) = (f.max_size, item.size) {
        let n = match rel.episodes {
            Some((a, b)) => b - a + 1,
            None => it.media.episodes.unwrap_or(1).max(1),
        };
        if size / u64::from(n) > max {
            return Some(Hidden::TooLarge);
        }
    }
    None
}

/// The episodes of `rel` that are new for `it`: `(first, last)`, or `None`. `aired`: the
/// episodes out so far, when known.
pub fn new_part(it: &ListItem, rel: &Release, aired: Option<u32>) -> Option<(u32, u32)> {
    let count = it.media.episodes.filter(|n| *n > 0);
    let cap = aired.or(count);
    let span = match rel.episodes {
        Some(s) => s,
        // A movie or a one-episode show: episode 1.
        None if count == Some(1) => (1, 1),
        // A season pack: all of it.
        None if rel.batch => (1, cap.unwrap_or(u32::MAX).max(1)),
        None => return None,
    };
    if it.status == Status::PlanToWatch && !it.rewatching {
        return (span.0 <= 1 && span.1 >= 1 && it.progress == 0).then(|| (1, span.1.min(cap.unwrap_or(span.1))));
    }
    let first = span.0.max(it.progress + 1);
    let last = cap.map_or(span.1, |c| span.1.min(c));
    (first <= last && span.0 != 0).then_some((first, last))
}

/// Series id, first and last episode.
pub type Span = (u32, u32, u32);

/// What the user marked: got (opened, with its hash and version), hidden, not this series.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    /// (media, first, last) → (info hash, version) of the release opened.
    pub got: HashMap<Span, (Option<String>, Option<u32>)>,
    pub hidden: HashSet<Span>,
    /// (media, `Parsed::key`): "this release is not this series".
    pub not_this: HashSet<(u32, String)>,
}

impl Marks {
    /// A got mark covering all of `span`, if any: its version.
    fn got_covering(&self, media: u32, span: (u32, u32)) -> Option<Option<u32>> {
        self.got.iter().find(|((m, a, b), _)| *m == media && *a <= span.0 && *b >= span.1).map(|(_, (_, v))| *v)
    }

    fn hidden_covering(&self, media: u32, span: (u32, u32)) -> bool {
        self.hidden.iter().any(|(m, a, b)| *m == media && *a <= span.0 && *b >= span.1)
    }
}

/// What [`select`] reads besides the items.
pub struct Ctx<'a> {
    pub list: &'a [ListItem],
    /// The recognition cache by `Parsed::key`, the same one files use.
    pub picks: &'a HashMap<String, (u32, String, String)>,
    pub relations: &'a Relations,
    /// Episodes aired so far, by media id, when known.
    pub aired: &'a HashMap<u32, u32>,
    pub filter: &'a Filter,
    pub series: &'a HashMap<u32, SeriesFilter>,
    pub marks: &'a Marks,
}

/// One new (series, episodes): the releases of it, the best first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub media_id: u32,
    /// `(8, 8)`, or a batch's new part.
    pub episodes: (u32, u32),
    /// Indexes into the items, best first.
    pub releases: Vec<usize>,
    /// Already got, but a newer version came out ("v2 out").
    pub version_out: bool,
}

/// An item that might be a series on the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsure {
    pub item: usize,
    pub media_id: u32,
    pub why: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub rows: Vec<Row>,
    pub unsure: Vec<Unsure>,
    /// Items of the list's series the filter hides: (item, series, why).
    pub hidden: Vec<(usize, u32, Hidden)>,
}

impl Selection {
    /// New episodes (the sidebar badge): rows that aren't a newer version of one got.
    pub fn new_count(&self) -> usize {
        self.rows.iter().filter(|r| !r.version_out).count()
    }
}

/// The best release first: preferred group order, resolution, version, seeders.
fn rank(f: &Filter, rel: &Release, item: &Item) -> (usize, bool, i64, u32, u32) {
    let group = rel.parsed.group.as_deref().unwrap_or("");
    let g = f.groups_preferred.iter().position(|p| same_group(p, group)).unwrap_or(f.groups_preferred.len());
    let h = i64::from(rel.height().unwrap_or(0));
    let res = if f.prefer_highest {
        -h
    } else {
        f.resolutions.iter().position(|r| i64::from(*r) == h).unwrap_or(f.resolutions.len()) as i64
    };
    let not_preferred = f.prefer_resolution.is_some_and(|p| rel.height() != Some(p));
    (g, not_preferred, res, u32::MAX - rel.version.unwrap_or(1), u32::MAX - item.seeders.unwrap_or(0))
}

/// The match of one release name: `Some((media, sure, why))` on the list's counting entries.
fn find(rel: &mut Release, ctx: &Ctx, entries: &[ListItem]) -> Option<(u32, bool, String)> {
    let key = rel.parsed.key();
    let cached = ctx.picks.get(&key).cloned();
    let found = recognize::on_list(&rel.parsed, entries, &cached, ctx.relations)?;
    // Ranges move as a whole only when both ends move to the same anime.
    let found = match rel.episodes {
        Some((a, b)) if a == b => {
            let f = recognize::redirect(found, &mut rel.parsed, entries, ctx.relations);
            let e = rel.parsed.episode.unwrap_or(a);
            rel.episodes = Some((e, e));
            f
        }
        Some((a, b)) => {
            let (mut pa, mut pb) = (rel.parsed.clone(), rel.parsed.clone());
            pa.episode = Some(a);
            pb.episode = Some(b);
            let fa = recognize::redirect(found.clone(), &mut pa, entries, ctx.relations);
            let fb = recognize::redirect(found.clone(), &mut pb, entries, ctx.relations);
            match (&fa, &fb) {
                (Found::List { media_id: x, .. }, Found::List { media_id: y, .. }) if x == y && fa != found => {
                    rel.episodes = Some((pa.episode.unwrap_or(a), pb.episode.unwrap_or(b)));
                    rel.parsed.episode = pb.episode;
                    fb
                }
                _ => found,
            }
        }
        None => found,
    };
    match found {
        Found::List { media_id, sure, why } => Some((media_id, sure, why)),
        _ => None,
    }
}

/// Releases of one row: (item index, parsed name).
type Releases = Vec<(usize, Release)>;

/// Feed items `(feed id, item)` → what is new for the list.
pub fn select(items: &[(i64, Item)], ctx: &Ctx) -> Selection {
    let entries: Vec<ListItem> = ctx.list.iter().filter(|it| counts(it)).cloned().collect();
    let mut out = Selection::default();
    let mut groups: HashMap<(u32, (u32, u32)), Releases> = HashMap::new();
    let mut seen = HashSet::new();
    for (i, (feed, item)) in items.iter().enumerate() {
        // The same release in two feeds counts once.
        if !seen.insert(item.key()) {
            continue;
        }
        let mut rel = parse_release(&item.title);
        let key = rel.parsed.key();
        let Some((media, sure, why)) = find(&mut rel, ctx, &entries) else { continue };
        if ctx.marks.not_this.contains(&(media, key)) {
            continue;
        }
        let Some(it) = entries.iter().find(|e| e.media.id == media) else { continue };
        let Some(span) = new_part(it, &rel, ctx.aired.get(&media).copied()) else { continue };
        if !sure {
            out.unsure.push(Unsure { item: i, media_id: media, why });
            continue;
        }
        let f = ctx.series.get(&media).map_or_else(|| ctx.filter.clone(), |s| ctx.filter.with(s));
        if let Some(h) = hidden(&f, item, &rel, it, *feed) {
            out.hidden.push((i, media, h));
            continue;
        }
        if ctx.marks.hidden_covering(media, span) {
            continue;
        }
        groups.entry((media, span)).or_default().push((i, rel));
    }
    for ((media, span), mut rels) in groups {
        let f = ctx.series.get(&media).map_or_else(|| ctx.filter.clone(), |s| ctx.filter.with(s));
        rels.sort_by_key(|(i, r)| (rank(&f, r, &items[*i].1), *i));
        let version_out = match ctx.marks.got_covering(media, span) {
            None => false,
            Some(v) => {
                let got = v.unwrap_or(1);
                if !rels.iter().any(|(_, r)| r.version.unwrap_or(1) > got) {
                    continue;
                }
                rels.retain(|(_, r)| r.version.unwrap_or(1) > got);
                true
            }
        };
        out.rows.push(Row { media_id: media, episodes: span, releases: rels.into_iter().map(|(i, _)| i).collect(), version_out });
    }
    // List order: series by title, then episode.
    let title = |m: u32| ctx.list.iter().find(|it| it.media.id == m).map(|it| it.media.title.to_lowercase()).unwrap_or_default();
    out.rows.sort_by_key(|r| (title(r.media_id), r.episodes));
    out
}

#[cfg(test)]
mod tests;
