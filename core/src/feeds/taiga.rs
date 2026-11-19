// Portions ported from Taiga (https://github.com/erengy/taiga), Copyright (C) Eren Okka and
// contributors, GPL-3.0: src/track/feed.cpp, feed_filter.cpp, feed_filter_manager.cpp,
// feed_filter_util.cpp and feed_aggregator.cpp (item states, torrent categories, filter
// conditions, actions and options, the presets, preference filters, the archive, new episodes,
// the download order, the filters' XML).

//! Taiga's torrent engine: every feed item gets an anime (recognition, the user's list), a
//! category (Anime, Batch, Other) and a state (blank, selected, discarded); the user's filters
//! (conditions → discard / select / prefer) then decide, and the archive of what was downloaded
//! discards those again. Kept as close to Taiga as Rust allows: the same elements, operators,
//! presets, shortcodes and XML, so a Taiga filter list imports as it is.

use std::collections::HashMap;

use crate::recognize::{self, Found, Release, parse_release, relations::Relations};
use crate::{ListItem, Status};

use super::Item;

// ------------------------------------------------------------------------------- items

/// `FeedItemState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ItemState {
    #[default]
    Blank,
    DiscardedNormal,
    DiscardedInactive,
    DiscardedHidden,
    Selected,
}

impl ItemState {
    pub fn is_discarded(self) -> bool {
        matches!(
            self,
            ItemState::DiscardedNormal | ItemState::DiscardedInactive | ItemState::DiscardedHidden
        )
    }

    /// Taiga's sort: selected first, then blank, then discarded.
    fn priority(self) -> u8 {
        match self {
            ItemState::Selected => 0,
            ItemState::Blank => 1,
            ItemState::DiscardedNormal => 2,
            ItemState::DiscardedInactive => 3,
            ItemState::DiscardedHidden => 4,
        }
    }
}

/// `TorrentCategory`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Category {
    #[default]
    Anime,
    Batch,
    Other,
}

impl Category {
    pub const ALL: [Category; 3] = [Category::Anime, Category::Batch, Category::Other];

    pub fn label(self) -> &'static str {
        match self {
            Category::Anime => "Anime",
            Category::Batch => "Batch",
            Category::Other => "Other",
        }
    }
}

/// One feed item with what Taiga knows about it (`FeedItem` with its `EpisodeData`).
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub item: Item,
    /// The feed it came from (index into the sources; -1: a search).
    pub feed: i64,
    pub release: Release,
    /// The anime on the user's list, when recognition is sure.
    pub anime_id: Option<u32>,
    /// The anime's title as parsed (the list's title when known: the caller's job to show).
    pub anime_title: String,
    pub category: Category,
    pub state: ItemState,
    pub new_episode: bool,
}

impl Entry {
    /// `FeedItem::Discard(option)`.
    pub fn discard(&mut self, option: FilterOption) {
        self.state = match option {
            FilterOption::Default => ItemState::DiscardedNormal,
            FilterOption::Deactivate => ItemState::DiscardedInactive,
            FilterOption::Hide => ItemState::DiscardedHidden,
        };
    }

    /// `FeedItem::operator==`: guid, else link, else title.
    pub fn same(&self, other: &Entry) -> bool {
        let (a, b) = (&self.item, &other.item);
        if !a.guid.is_empty() && a.guid == b.guid {
            return true;
        }
        let link = |i: &Item| {
            i.torrent
                .clone()
                .or_else(|| i.magnet.clone())
                .or_else(|| i.page.clone())
        };
        if link(a).is_some() && link(a) == link(b) {
            return true;
        }
        !a.title.is_empty() && a.title == b.title
    }

    /// `anime::GetEpisodeHigh`: the range's end; `None` without a number.
    pub fn episode_high(&self) -> Option<u32> {
        self.release.episodes.map(|(_, b)| b)
    }

    /// The Episode column: "8", "1-12", "8v2", "Vol. 2".
    pub fn episode_text(&self) -> String {
        let mut s = match self.release.episodes {
            Some((a, b)) if a == b => a.to_string(),
            Some((a, b)) => format!("{a}-{b}"),
            None => String::new(),
        };
        if let Some(v) = self.release.version {
            s.push_str(&format!("v{v}"));
        }
        s
    }

    /// The Video column: the resolution and the video terms.
    pub fn video_text(&self) -> String {
        let mut v: Vec<String> = Vec::new();
        if let Some(r) = &self.release.parsed.resolution {
            v.push(normalize_resolution(r));
        }
        v.extend(self.release.video_terms.iter().cloned());
        v.join(" ")
    }
}

/// `anime::NormalizeVideoResolution`: "1920x1080" → "1080p".
pub fn normalize_resolution(r: &str) -> String {
    match resolution_height(r) {
        Some(h) if !r.to_ascii_lowercase().ends_with('p') => format!("{h}p"),
        _ => r.to_string(),
    }
}

/// `anime::GetVideoResolutionHeight`.
pub fn resolution_height(r: &str) -> Option<u32> {
    let r = r.trim().to_ascii_lowercase();
    if r == "4k" {
        return Some(2160);
    }
    r.rsplit('x')
        .next()?
        .trim_end_matches(['p', 'i'])
        .parse()
        .ok()
}

/// Types anitomy finds that are no episode (`Meow.IsValidAnimeType`).
const INVALID_TYPES: [&str; 11] = [
    "ed", "ending", "ncop", "nced", "op", "opening", "preview", "pv", "cm", "trailer", "teaser",
];
const VIDEO_EXTENSIONS: [&str; 12] = [
    "mkv", "mp4", "avi", "m4v", "webm", "ts", "m2ts", "ogm", "wmv", "flv", "mov", "rmvb",
];

/// `GetTorrentCategory`.
pub fn category_of(item: &Item, r: &Release) -> Category {
    if item.category.contains("Batch") || r.batch {
        return Category::Batch;
    }
    if r.types
        .iter()
        .any(|t| INVALID_TYPES.contains(&t.to_ascii_lowercase().as_str()))
    {
        return Category::Other;
    }
    if r.episodes.is_some_and(|(a, b)| a != b) {
        return Category::Batch;
    }
    if let Some(ext) = &r.extension
        && !VIDEO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
    {
        return Category::Other;
    }
    Category::Anime
}

/// The aggregator's `ExamineData` up to the filters: parse, identify on the list (the user's
/// picks, anime-relations), categorise. Items with the same key (a release in two feeds) once.
pub fn examine(
    items: &[(i64, Item)],
    list: &[ListItem],
    picks: &HashMap<String, (u32, String, String)>,
    rel: &Relations,
) -> Vec<Entry> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (feed, item) in items {
        if !seen.insert(item.key()) {
            continue;
        }
        let mut release = parse_release(&item.title);
        let cached = picks.get(&release.parsed.key()).cloned();
        let mut anime_id = None;
        if let Some(found) = recognize::on_list(&release.parsed, list, &cached, rel) {
            let found = match release.episodes {
                Some((a, b)) if a == b => {
                    let f = recognize::redirect(found, &mut release.parsed, list, rel);
                    let e = release.parsed.episode.unwrap_or(a);
                    release.episodes = Some((e, e));
                    f
                }
                _ => found,
            };
            if let Found::List {
                media_id,
                sure: true,
                ..
            } = found
            {
                anime_id = Some(media_id);
            }
        }
        let category = category_of(item, &release);
        out.push(Entry {
            item: item.clone(),
            feed: *feed,
            anime_title: release.parsed.title.clone(),
            release,
            anime_id,
            category,
            state: ItemState::Blank,
            new_episode: false,
        });
    }
    out
}

// ------------------------------------------------------------------------------ filters

/// `FeedFilterElement`, in Taiga's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Element {
    MetaId,
    MetaStatus,
    MetaType,
    MetaEpisodes,
    MetaDateStart,
    MetaDateEnd,
    UserNotes,
    UserStatus,
    LocalEpisodeAvailable,
    EpisodeTitle,
    EpisodeNumber,
    EpisodeVersion,
    EpisodeGroup,
    EpisodeVideoResolution,
    EpisodeVideoType,
    FileTitle,
    FileCategory,
    FileDescription,
    FileLink,
    FileSize,
}

impl Element {
    pub const ALL: [Element; 20] = [
        Element::FileTitle,
        Element::FileCategory,
        Element::FileDescription,
        Element::FileLink,
        Element::FileSize,
        Element::MetaId,
        Element::EpisodeTitle,
        Element::MetaDateStart,
        Element::MetaDateEnd,
        Element::MetaEpisodes,
        Element::MetaStatus,
        Element::MetaType,
        Element::UserNotes,
        Element::UserStatus,
        Element::EpisodeNumber,
        Element::EpisodeVersion,
        Element::LocalEpisodeAvailable,
        Element::EpisodeGroup,
        Element::EpisodeVideoResolution,
        Element::EpisodeVideoType,
    ];

    /// `TranslateElement`.
    pub fn label(self) -> &'static str {
        match self {
            Element::FileTitle => "File name",
            Element::FileCategory => "File category",
            Element::FileDescription => "File description",
            Element::FileLink => "File link",
            Element::FileSize => "File size",
            Element::MetaId => "Anime ID",
            Element::EpisodeTitle => "Episode title",
            Element::MetaDateStart => "Anime date started",
            Element::MetaDateEnd => "Anime date ended",
            Element::MetaEpisodes => "Anime episode count",
            Element::MetaStatus => "Anime airing status",
            Element::MetaType => "Anime type",
            Element::UserNotes => "Anime notes",
            Element::UserStatus => "Anime watching status",
            Element::EpisodeNumber => "Episode number",
            Element::EpisodeVersion => "Episode version",
            Element::LocalEpisodeAvailable => "Episode availability",
            Element::EpisodeGroup => "Episode fansub group",
            Element::EpisodeVideoResolution => "Episode video resolution",
            Element::EpisodeVideoType => "Episode video type",
        }
    }

    /// The XML shortcode.
    pub fn code(self) -> &'static str {
        match self {
            Element::MetaId => "meta_id",
            Element::MetaStatus => "meta_status",
            Element::MetaType => "meta_type",
            Element::MetaEpisodes => "meta_episodes",
            Element::MetaDateStart => "meta_date_start",
            Element::MetaDateEnd => "meta_date_end",
            Element::UserNotes => "user_tags",
            Element::UserStatus => "user_status",
            Element::LocalEpisodeAvailable => "local_episode_available",
            Element::EpisodeTitle => "episode_title",
            Element::EpisodeNumber => "episode_number",
            Element::EpisodeVersion => "episode_version",
            Element::EpisodeGroup => "episode_group",
            Element::EpisodeVideoResolution => "episode_video_resolution",
            Element::EpisodeVideoType => "episode_video_type",
            Element::FileTitle => "file_title",
            Element::FileCategory => "file_category",
            Element::FileDescription => "file_description",
            Element::FileLink => "file_link",
            Element::FileSize => "file_size",
        }
    }

    pub fn from_code(s: &str) -> Option<Element> {
        Element::ALL
            .into_iter()
            .find(|e| e.code().eq_ignore_ascii_case(s))
    }

    fn is_numeric(self) -> bool {
        matches!(
            self,
            Element::FileSize
                | Element::MetaId
                | Element::MetaEpisodes
                | Element::MetaStatus
                | Element::MetaType
                | Element::UserStatus
                | Element::EpisodeNumber
                | Element::EpisodeVersion
                | Element::LocalEpisodeAvailable
        )
    }
}

/// `FeedFilterOperator`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Operator {
    Equals,
    NotEquals,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    BeginsWith,
    EndsWith,
    Contains,
    NotContains,
}

impl Operator {
    pub const ALL: [Operator; 10] = [
        Operator::Equals,
        Operator::NotEquals,
        Operator::GreaterThan,
        Operator::GreaterThanOrEqual,
        Operator::LessThan,
        Operator::LessThanOrEqual,
        Operator::BeginsWith,
        Operator::EndsWith,
        Operator::Contains,
        Operator::NotContains,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Operator::Equals => "is",
            Operator::NotEquals => "is not",
            Operator::GreaterThan => "is greater than",
            Operator::GreaterThanOrEqual => "is greater than or equal to",
            Operator::LessThan => "is less than",
            Operator::LessThanOrEqual => "is less than or equal to",
            Operator::BeginsWith => "begins with",
            Operator::EndsWith => "ends with",
            Operator::Contains => "contains",
            Operator::NotContains => "does not contain",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Operator::Equals => "equals",
            Operator::NotEquals => "notequals",
            Operator::GreaterThan => "gt",
            Operator::GreaterThanOrEqual => "ge",
            Operator::LessThan => "lt",
            Operator::LessThanOrEqual => "le",
            Operator::BeginsWith => "beginswith",
            Operator::EndsWith => "endswith",
            Operator::Contains => "contains",
            Operator::NotContains => "notcontains",
        }
    }

    pub fn from_code(s: &str) -> Option<Operator> {
        Operator::ALL
            .into_iter()
            .find(|o| o.code().eq_ignore_ascii_case(s))
    }

    fn compare<T: PartialOrd>(self, a: T, b: T) -> bool {
        match self {
            Operator::NotEquals => a != b,
            Operator::GreaterThan => a > b,
            Operator::GreaterThanOrEqual => a >= b,
            Operator::LessThan => a < b,
            Operator::LessThanOrEqual => a <= b,
            _ => a == b,
        }
    }
}

/// `FeedFilterMatch`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Match {
    #[default]
    All,
    Any,
}

impl Match {
    pub const ALL: [Match; 2] = [Match::All, Match::Any];
    pub fn label(self) -> &'static str {
        match self {
            Match::All => "All conditions",
            Match::Any => "Any condition",
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Match::All => "all",
            Match::Any => "any",
        }
    }
}

/// `FeedFilterAction`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Action {
    #[default]
    Discard,
    Select,
    Prefer,
}

impl Action {
    pub const ALL: [Action; 3] = [Action::Discard, Action::Select, Action::Prefer];
    pub fn label(self) -> &'static str {
        match self {
            Action::Discard => "Discard matched items",
            Action::Select => "Select matched items",
            Action::Prefer => "Prefer matched items to similar ones",
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Action::Discard => "discard",
            Action::Select => "select",
            Action::Prefer => "prefer",
        }
    }
}

/// `FeedFilterOption`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FilterOption {
    #[default]
    Default,
    Deactivate,
    Hide,
}

impl FilterOption {
    pub const ALL: [FilterOption; 3] = [
        FilterOption::Default,
        FilterOption::Deactivate,
        FilterOption::Hide,
    ];
    pub fn label(self) -> &'static str {
        match self {
            FilterOption::Default => "Default",
            FilterOption::Deactivate => "Deactivate discarded items",
            FilterOption::Hide => "Hide discarded items",
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            FilterOption::Default => "default",
            FilterOption::Deactivate => "deactivate",
            FilterOption::Hide => "hide",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Condition {
    pub element: Element,
    pub op: Operator,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    pub name: String,
    pub enabled: bool,
    pub action: Action,
    pub matching: Match,
    pub option: FilterOption,
    /// Empty: every anime.
    pub anime_ids: Vec<u32>,
    pub conditions: Vec<Condition>,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            name: "New Filter".into(),
            enabled: true,
            action: Action::Discard,
            matching: Match::All,
            option: FilterOption::Default,
            anime_ids: Vec::new(),
            conditions: Vec::new(),
        }
    }
}

/// `FeedFilterPreset`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preset {
    pub description: &'static str,
    pub filter: Filter,
    pub is_default: bool,
}

/// Taiga's `MyStatus` numbers (filters store them).
pub fn my_status_code(s: Option<Status>) -> i64 {
    match s {
        None => 0,
        Some(Status::Watching) => 1,
        Some(Status::Completed) => 2,
        Some(Status::OnHold) => 3,
        Some(Status::Dropped) => 4,
        Some(Status::PlanToWatch) => 5,
    }
}

/// Taiga's `SeriesStatus` numbers from the service's airing status.
pub fn series_status_code(air: &str) -> i64 {
    match air {
        "finished_airing" => 1,
        "currently_airing" => 2,
        "not_yet_aired" => 3,
        _ => 0,
    }
}

/// Taiga's `SeriesType` numbers from the service's format.
pub fn series_type_code(format: &str) -> i64 {
    match format.to_ascii_lowercase().as_str() {
        "tv" | "tv_short" => 1,
        "ova" => 2,
        "movie" => 3,
        "special" | "tv_special" => 4,
        "ona" => 5,
        "music" => 6,
        _ => 0,
    }
}

/// The value words a condition shows (`TranslateValue`) for status and type elements.
pub fn value_label(
    element: Element,
    value: &str,
    title_of: &dyn Fn(u32) -> Option<String>,
) -> String {
    let n: i64 = value.trim().parse().unwrap_or(-1);
    match element {
        Element::MetaId => match value.trim().parse::<u32>().ok().and_then(title_of) {
            Some(t) => format!("{value} ({t})"),
            None if value.is_empty() => "(?)".into(),
            None => format!("{value} (?)"),
        },
        Element::UserStatus => [
            "Not in list",
            "Watching",
            "Completed",
            "On hold",
            "Dropped",
            "Plan to watch",
        ]
        .get(n as usize)
        .map_or_else(|| value.to_string(), |s| s.to_string()),
        Element::MetaStatus => [
            "Unknown",
            "Finished airing",
            "Currently airing",
            "Not yet aired",
        ]
        .get(n as usize)
        .map_or_else(|| value.to_string(), |s| s.to_string()),
        Element::MetaType => ["Unknown", "TV", "OVA", "Movie", "Special", "ONA", "Music"]
            .get(n as usize)
            .map_or_else(|| value.to_string(), |s| s.to_string()),
        _ if value.is_empty() => "(empty)".into(),
        _ => value.to_string(),
    }
}

/// `TranslateCondition`: `Episode number is less than or equal to "%watched%"`.
pub fn condition_text(c: &Condition, title_of: &dyn Fn(u32) -> Option<String>) -> String {
    format!(
        "{} {} \u{201c}{}\u{201d}",
        c.element.label(),
        c.op.label(),
        value_label(c.element, &c.value, title_of)
    )
}

/// `FeedFilterManager::InitializePresets`, in Taiga's order and words.
pub fn presets() -> Vec<Preset> {
    let mut v = Vec::new();
    let mut add = |action,
                   matching,
                   is_default,
                   option,
                   name: &str,
                   description: &'static str,
                   conditions: Vec<(Element, Operator, String)>| {
        v.push(Preset {
            description,
            is_default,
            filter: Filter {
                name: name.into(),
                enabled: true,
                action,
                matching,
                option,
                anime_ids: Vec::new(),
                conditions: conditions
                    .into_iter()
                    .map(|(element, op, value)| Condition { element, op, value })
                    .collect(),
            },
        });
    };
    use Element as E;
    use Operator as O;
    add(
        Action::Discard,
        Match::All,
        false,
        FilterOption::Default,
        "(Custom)",
        "Lets you create a custom filter from scratch",
        vec![],
    );
    add(
        Action::Prefer,
        Match::All,
        false,
        FilterOption::Default,
        "[Fansub] Anime",
        "Lets you choose a fansub group for one or more anime",
        vec![(E::EpisodeGroup, O::Equals, "TaigaSubs (change this)".into())],
    );
    add(
        Action::Discard,
        Match::Any,
        false,
        FilterOption::Default,
        "Discard bad video keywords",
        "Discards everything that is AVI, DIVX, LQ, RMVB, SD, WMV or XVID",
        ["AVI", "DIVX", "LQ", "RMVB", "SD", "WMV", "XVID"]
            .iter()
            .map(|k| (E::EpisodeVideoType, O::Contains, k.to_string()))
            .collect(),
    );
    add(
        Action::Prefer,
        Match::Any,
        false,
        FilterOption::Default,
        "Prefer new versions",
        "Prefers v2 files and above when there are earlier releases of the same episode as well",
        vec![(E::EpisodeVersion, O::GreaterThan, "1".into())],
    );
    add(
        Action::Select,
        Match::Any,
        true,
        FilterOption::Default,
        "Select currently watching",
        "Selects files that belong to anime that you're currently watching",
        vec![(
            E::UserStatus,
            O::Equals,
            my_status_code(Some(Status::Watching)).to_string(),
        )],
    );
    add(
        Action::Select,
        Match::All,
        true,
        FilterOption::Default,
        "Select airing anime in plan to watch",
        "Selects files that belong to an airing anime that you're planning to watch",
        vec![
            (E::MetaStatus, O::Equals, "2".into()),
            (
                E::UserStatus,
                O::Equals,
                my_status_code(Some(Status::PlanToWatch)).to_string(),
            ),
        ],
    );
    add(
        Action::Discard,
        Match::All,
        true,
        FilterOption::Default,
        "Discard dropped",
        "Discards files that belong to anime that you've dropped watching",
        vec![(
            E::UserStatus,
            O::Equals,
            my_status_code(Some(Status::Dropped)).to_string(),
        )],
    );
    add(
        Action::Discard,
        Match::Any,
        true,
        FilterOption::Deactivate,
        "Discard and deactivate not-in-list anime",
        "Discards files that do not belong to any anime in your list",
        vec![(E::UserStatus, O::Equals, "0".into())],
    );
    add(
        Action::Discard,
        Match::Any,
        true,
        FilterOption::Default,
        "Discard watched and available episodes",
        "Discards episodes you've already watched or downloaded",
        vec![
            (E::EpisodeNumber, O::LessThanOrEqual, "%watched%".into()),
            (E::LocalEpisodeAvailable, O::Equals, "True".into()),
        ],
    );
    add(
        Action::Prefer,
        Match::Any,
        true,
        FilterOption::Default,
        "Prefer high-resolution files",
        "Prefers 1080p files when there are other files of the same episode as well",
        vec![(E::EpisodeVideoResolution, O::Equals, "1080p".into())],
    );
    v
}

/// The default presets as filters (`AddPresets`).
pub fn default_filters() -> Vec<Filter> {
    presets()
        .into_iter()
        .filter(|p| p.is_default)
        .map(|p| p.filter)
        .collect()
}

/// What the conditions read about the list and the library.
pub struct World<'a> {
    pub list: &'a [ListItem],
    /// The library has the file of (anime, episode).
    pub available: &'a dyn Fn(u32, u32) -> bool,
}

impl World<'_> {
    fn anime(&self, id: Option<u32>) -> Option<&ListItem> {
        let id = id?;
        self.list.iter().find(|it| it.media.id == id)
    }
}

/// `ReplaceVariables` for condition values.
fn replace_variables(value: &str, e: &Entry, w: &World) -> String {
    if !value.contains('%') {
        return value.to_string();
    }
    let a = w.anime(e.anime_id);
    let num = |n: Option<u32>| n.map_or(String::new(), |n| n.to_string());
    let fields: [(&str, String); 10] = [
        (
            "title",
            a.map_or_else(|| e.anime_title.clone(), |a| a.media.title.clone()),
        ),
        (
            "watched",
            a.map_or(String::new(), |a| {
                if a.progress > 0 {
                    a.progress.to_string()
                } else {
                    String::new()
                }
            }),
        ),
        ("total", num(a.and_then(|a| a.media.episodes))),
        (
            "status",
            a.map_or(String::new(), |a| {
                my_status_code(Some(a.status)).to_string()
            }),
        ),
        (
            "rewatching",
            a.map_or(String::new(), |a| u8::from(a.rewatching).to_string()),
        ),
        ("id", num(a.map(|a| a.media.id))),
        ("episode", num(e.episode_high())),
        ("group", e.release.parsed.group.clone().unwrap_or_default()),
        (
            "resolution",
            e.release.parsed.resolution.clone().unwrap_or_default(),
        ),
        ("version", e.release.version.unwrap_or(1).to_string()),
    ];
    let mut out = value.to_string();
    for (k, v) in fields {
        out = out.replace(&format!("%{k}%"), &v);
    }
    out
}

/// Taiga's `ParseSizeString` ("1 GB" is 1024³ there).
fn parse_size_value(v: &str) -> u64 {
    let s = v.trim().to_ascii_uppercase().replace("IB", "B");
    let cut = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    let n: f64 = s[..cut].parse().unwrap_or(0.0);
    let m = match s[cut..].trim() {
        "KB" => 1024.0,
        "MB" => 1024.0 * 1024.0,
        "GB" => 1024f64.powi(3),
        "TB" => 1024f64.powi(4),
        _ => 1.0,
    };
    (n * m) as u64
}

/// `EvaluateCondition`.
pub fn evaluate(c: &Condition, e: &Entry, w: &World) -> bool {
    let a = w.anime(e.anime_id);
    let element: String = match c.element {
        Element::FileTitle => e.item.title.clone(),
        Element::FileCategory => e.category.label().to_string(),
        Element::FileDescription => e.item.description.clone(),
        Element::FileLink => e
            .item
            .torrent
            .clone()
            .or_else(|| e.item.magnet.clone())
            .or_else(|| e.item.page.clone())
            .unwrap_or_default(),
        Element::FileSize => e.item.size.unwrap_or(0).to_string(),
        Element::MetaId => a.map_or("0".into(), |a| a.media.id.to_string()),
        Element::EpisodeTitle => e.anime_title.clone(),
        Element::MetaDateStart => a
            .and_then(|a| a.media.start_date.clone())
            .unwrap_or_default(),
        Element::MetaDateEnd => String::new(),
        Element::MetaEpisodes => a
            .and_then(|a| a.media.episodes)
            .map_or(String::new(), |n| n.to_string()),
        Element::MetaStatus => a
            .map_or(0, |a| series_status_code(&a.media.air_status))
            .to_string(),
        Element::MetaType => a
            .map_or(0, |a| series_type_code(&a.media.format))
            .to_string(),
        Element::UserNotes => String::new(),
        Element::UserStatus => my_status_code(a.map(|a| a.status)).to_string(),
        Element::EpisodeNumber => match e.episode_high() {
            Some(n) => n.to_string(),
            None => a
                .and_then(|a| a.media.episodes)
                .map_or(String::new(), |n| n.to_string()),
        },
        Element::EpisodeVersion => e.release.version.unwrap_or(1).to_string(),
        Element::LocalEpisodeAvailable => match (a, e.episode_high()) {
            (Some(a), Some(n)) => u8::from((w.available)(a.media.id, n)).to_string(),
            (Some(_), None) => "0".into(),
            _ => String::new(),
        },
        Element::EpisodeGroup => e.release.parsed.group.clone().unwrap_or_default(),
        Element::EpisodeVideoResolution => e.release.parsed.resolution.clone().unwrap_or_default(),
        Element::EpisodeVideoType => e.release.video_terms.join(" "),
    };
    let value = replace_variables(&c.value, e, w);
    let numeric = !element.is_empty() && !value.is_empty() && c.element.is_numeric();
    let ieq = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    match c.op {
        Operator::Equals
        | Operator::NotEquals
        | Operator::GreaterThan
        | Operator::GreaterThanOrEqual
        | Operator::LessThan
        | Operator::LessThanOrEqual => {
            match c.element {
                Element::FileSize => {
                    return c.op.compare(
                        element.parse::<u64>().unwrap_or(0),
                        parse_size_value(&value),
                    );
                }
                Element::EpisodeVideoResolution => {
                    return c.op.compare(
                        resolution_height(&element).unwrap_or(0),
                        resolution_height(&value).unwrap_or(0),
                    );
                }
                _ => {}
            }
            if numeric {
                let el: i64 = element.trim().parse().unwrap_or(0);
                if matches!(c.op, Operator::Equals | Operator::NotEquals)
                    && value.eq_ignore_ascii_case("true")
                {
                    return c.op.compare(el, 1);
                }
                c.op.compare(el, value.trim().parse::<i64>().unwrap_or(0))
            } else if matches!(c.op, Operator::Equals | Operator::NotEquals) {
                c.op.compare(ieq(&element, &value), true)
            } else {
                c.op.compare(
                    element.to_lowercase().cmp(&value.to_lowercase()),
                    std::cmp::Ordering::Equal,
                )
            }
        }
        Operator::BeginsWith => element.starts_with(&value),
        Operator::EndsWith => element.ends_with(&value),
        Operator::Contains => element.to_lowercase().contains(&value.to_lowercase()),
        Operator::NotContains => !element.to_lowercase().contains(&value.to_lowercase()),
    }
}

fn matched(f: &Filter, e: &Entry, w: &World) -> bool {
    match f.matching {
        Match::All => f.conditions.iter().all(|c| evaluate(c, e, w)),
        Match::Any => f.conditions.iter().any(|c| evaluate(c, e, w)),
    }
}

/// `ApplyFilter`: whether the filter had an effect on entry `i`.
pub fn apply_filter(
    f: &Filter,
    entries: &mut [Entry],
    i: usize,
    recursive: bool,
    w: &World,
) -> bool {
    if !f.enabled || entries[i].state.is_discarded() {
        return false;
    }
    if !f.anime_ids.is_empty()
        && !entries[i]
            .anime_id
            .is_some_and(|id| f.anime_ids.contains(&id))
    {
        return false;
    }
    let m = matched(f, &entries[i], w);
    match f.action {
        Action::Discard if m => entries[i].discard(f.option),
        Action::Select if m => entries[i].state = ItemState::Selected,
        Action::Discard | Action::Select => return false,
        Action::Prefer => {
            if recursive {
                if !f.anime_ids.is_empty() {
                    // A strong preference (limited to some anime).
                    if m {
                        entries[i].state = ItemState::Selected;
                    } else {
                        entries[i].discard(f.option);
                    }
                } else if m {
                    if !apply_preference_filter(f, entries, i, w) {
                        return false;
                    }
                } else {
                    return false;
                }
            } else if !m {
                entries[i].discard(f.option);
            } else {
                return false;
            }
        }
    }
    true
}

/// `ApplyPreferenceFilter`: the other items of the same anime, episode and group that the
/// preference doesn't match are discarded.
pub fn apply_preference_filter(f: &Filter, entries: &mut [Entry], i: usize, w: &World) -> bool {
    let has = |el: Element| f.conditions.iter().any(|c| c.element == el);
    let (by_id, by_title, by_number, by_group) = (
        has(Element::MetaId),
        has(Element::EpisodeTitle),
        has(Element::EpisodeNumber),
        has(Element::EpisodeGroup),
    );
    let me = entries[i].clone();
    let mut applied = false;
    for j in 0..entries.len() {
        if j == i || entries[j].state.is_discarded() || entries[j].same(&me) {
            continue;
        }
        let o = &entries[j];
        if o.anime_id.is_none() && me.anime_id.is_none() {
            if !by_title && !o.anime_title.eq_ignore_ascii_case(&me.anime_title) {
                continue;
            }
        } else if !by_id && o.anime_id != me.anime_id {
            continue;
        }
        if !by_number && o.release.episodes != me.release.episodes {
            continue;
        }
        let g = |e: &Entry| {
            e.release
                .parsed
                .group
                .clone()
                .unwrap_or_default()
                .to_lowercase()
        };
        if !by_group && g(o) != g(&me) {
            continue;
        }
        applied |= apply_filter(f, entries, j, false, w);
    }
    applied
}

/// `MarkNewEpisodes`: past the progress of the anime on the list.
pub fn mark_new_episodes(entries: &mut [Entry], w: &World) {
    for e in entries.iter_mut() {
        if let Some(a) = w.anime(e.anime_id) {
            let n = e.episode_high().or(a.media.episodes).unwrap_or(0);
            e.new_episode = n > a.progress;
        }
    }
}

/// The archive of downloaded titles (`TorrentArchive`), oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Archive {
    pub titles: Vec<String>,
}

impl Archive {
    pub fn contains(&self, t: &str) -> bool {
        self.titles.iter().any(|x| x == t)
    }
    pub fn add(&mut self, t: &str) {
        if !self.contains(t) {
            self.titles.push(t.to_string());
        }
    }
    /// Keeps the newest `max` (0: none, as Taiga saves none).
    pub fn trimmed(&self, max: usize) -> Vec<String> {
        self.titles[self.titles.len().saturating_sub(max)..].to_vec()
    }
}

/// `ExamineData` after identification: new episodes, filters (preferences last), the archive,
/// then Taiga's sort by state.
pub fn run_filters(
    entries: &mut [Entry],
    filters: &[Filter],
    enabled: bool,
    archive: &Archive,
    w: &World,
) {
    mark_new_episodes(entries, w);
    if enabled {
        for preferences in [false, true] {
            for i in 0..entries.len() {
                for f in filters {
                    if preferences != (f.action == Action::Prefer) {
                        continue;
                    }
                    apply_filter(f, entries, i, true, w);
                }
            }
        }
    }
    for e in entries.iter_mut() {
        if !e.state.is_discarded() && archive.contains(&e.item.title) {
            e.state = ItemState::DiscardedNormal;
        }
    }
    entries.sort_by_key(|e| e.state.priority());
}

/// `SetFansubFilter`: prefer `group` (and the resolution) for one anime; an empty group takes it
/// back.
pub fn set_fansub_filter(
    filters: &mut Vec<Filter>,
    anime_id: u32,
    title: &str,
    group: &str,
    resolution: &str,
) -> bool {
    for k in 0..filters.len() {
        let f = &mut filters[k];
        let Some(pos) = f.anime_ids.iter().position(|id| *id == anime_id) else {
            continue;
        };
        let Some(gc) = f
            .conditions
            .iter()
            .position(|c| c.element == Element::EpisodeGroup)
        else {
            continue;
        };
        if f.anime_ids.len() > 1 {
            f.anime_ids.remove(pos);
            if group.is_empty() {
                return true;
            }
            break;
        }
        if group.is_empty() {
            filters.remove(k);
        } else {
            f.conditions[gc].value = group.to_string();
            if !resolution.is_empty() {
                match f
                    .conditions
                    .iter_mut()
                    .find(|c| c.element == Element::EpisodeVideoResolution)
                {
                    Some(c) => c.value = resolution.to_string(),
                    None => f.conditions.push(Condition {
                        element: Element::EpisodeVideoResolution,
                        op: Operator::Equals,
                        value: resolution.to_string(),
                    }),
                }
            }
        }
        return true;
    }
    if group.is_empty() {
        return false;
    }
    let mut conditions = vec![Condition {
        element: Element::EpisodeGroup,
        op: Operator::Equals,
        value: group.to_string(),
    }];
    if !resolution.is_empty() {
        conditions.push(Condition {
            element: Element::EpisodeVideoResolution,
            op: Operator::Equals,
            value: resolution.to_string(),
        });
    }
    filters.push(Filter {
        name: format!("[Fansub] {title}"),
        enabled: true,
        action: Action::Prefer,
        matching: Match::All,
        option: FilterOption::Default,
        anime_ids: vec![anime_id],
        conditions,
    });
    true
}

/// `GetFansubFilter`: the groups preferred for an anime.
pub fn fansub_groups(filters: &[Filter], anime_id: u32) -> Vec<String> {
    filters
        .iter()
        .filter(|f| f.anime_ids.contains(&anime_id))
        .flat_map(|f| {
            f.conditions
                .iter()
                .filter(|c| c.element == Element::EpisodeGroup)
                .map(|c| c.value.clone())
        })
        .collect()
}

/// `AddDiscardFilter`: discard everything of one anime.
pub fn discard_filter(anime_id: u32, title: &str) -> Filter {
    Filter {
        name: format!("Discard \u{201c}{title}\u{201d}"),
        enabled: true,
        action: Action::Discard,
        matching: Match::All,
        option: FilterOption::Default,
        anime_ids: Vec::new(),
        conditions: vec![Condition {
            element: Element::MetaId,
            op: Operator::Equals,
            value: anime_id.to_string(),
        }],
    }
}

/// The download order (`Aggregator::Download` with no item): by anime, then episode or date.
pub fn download_order(entries: &[Entry], by_date: bool, descending: bool) -> Vec<usize> {
    let mut v: Vec<usize> = (0..entries.len())
        .filter(|i| entries[*i].state == ItemState::Selected)
        .collect();
    v.sort_by(|&a, &b| {
        let (x, y) = (&entries[a], &entries[b]);
        x.anime_id.cmp(&y.anime_id).then_with(|| {
            let o = if by_date {
                x.item.published.cmp(&y.item.published)
            } else {
                x.release
                    .episodes
                    .map(|e| e.0)
                    .cmp(&y.release.episodes.map(|e| e.0))
            };
            if descending { o.reverse() } else { o }
        })
    });
    v
}

// ----------------------------------------------------------------------------- XML

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `FeedFilterManager::Export`: Taiga's `<filter>` XML.
pub fn export_filters(filters: &[Filter]) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<filter>\n");
    for f in filters {
        out.push_str(&format!(
            "  <item action=\"{}\" match=\"{}\" option=\"{}\" enabled=\"{}\" name=\"{}\">\n",
            f.action.code(),
            f.matching.code(),
            f.option.code(),
            f.enabled,
            xml_escape(&f.name)
        ));
        for id in &f.anime_ids {
            out.push_str(&format!("    <anime id=\"{id}\" />\n"));
        }
        for c in &f.conditions {
            out.push_str(&format!(
                "    <condition element=\"{}\" operator=\"{}\" value=\"{}\" />\n",
                c.element.code(),
                c.op.code(),
                xml_escape(&c.value)
            ));
        }
        out.push_str("  </item>\n");
    }
    out.push_str("</filter>\n");
    out
}

/// `FeedFilterManager::Import`: Taiga's XML (`<filter>` root, or any root holding `<item>`s).
/// Unknown elements or operators drop that condition.
pub fn import_filters(xml: &str) -> Result<Vec<Filter>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    let holder = if root.has_tag_name("filter") {
        Some(root)
    } else {
        root.descendants().find(|n| n.has_tag_name("filter"))
    };
    let holder = holder.ok_or("no <filter> in this file")?;
    let mut out = Vec::new();
    for it in holder.children().filter(|n| n.has_tag_name("item")) {
        let attr = |k: &str| it.attribute(k).unwrap_or("");
        let action = Action::ALL
            .into_iter()
            .find(|a| a.code() == attr("action"))
            .unwrap_or_default();
        let matching = Match::ALL
            .into_iter()
            .find(|m| m.code() == attr("match"))
            .unwrap_or_default();
        let option = FilterOption::ALL
            .into_iter()
            .find(|o| o.code() == attr("option"))
            .unwrap_or_default();
        let anime_ids = it
            .children()
            .filter(|n| n.has_tag_name("anime"))
            .filter_map(|n| n.attribute("id")?.parse().ok())
            .collect();
        let conditions = it
            .children()
            .filter(|n| n.has_tag_name("condition"))
            .filter_map(|n| {
                Some(Condition {
                    element: Element::from_code(n.attribute("element")?)?,
                    op: Operator::from_code(n.attribute("operator")?)?,
                    value: n.attribute("value").unwrap_or("").to_string(),
                })
            })
            .collect();
        out.push(Filter {
            name: attr("name").to_string(),
            enabled: attr("enabled") != "false",
            action,
            matching,
            option,
            anime_ids,
            conditions,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
