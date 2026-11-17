//! The AniList client: GraphQL over `https://graphql.anilist.co`, signed in with the implicit
//! grant (no client secret, no refresh: a token lasts a year), the user's anime list, one entry
//! (read, `SaveMediaListEntry`, `DeleteMediaListEntry`), search, details, seasons, a rate budget
//! and 429 handling. The manga list, through the same client and budget, is in `manga.rs`.
//!
//! AniList's terms (clause 5) allow a client of the user's own list, nothing more: nothing here
//! keeps a list of its own, exports, or crawls. Media data is asked for only to show the user's
//! list, a search, a details page or the Airing page, through the same small rate budget.
//!
//! Kotora's model is MyAnimeList-shaped; this module maps AniList onto it:
//! - statuses: `CURRENT` Watching, `PLANNING` Plan to watch, `COMPLETED` Completed, `PAUSED`
//!   On hold, `DROPPED` Dropped, and `REPEATING` is Completed with the rewatch flag (MAL's
//!   `is_rewatching`); `repeat` is the rewatch count.
//! - scores: read on the 100-point scale and kept as 1-10 (rounded); written as `scoreRaw` (the
//!   100-point value), which AniList shows in the user's own score format.
//! - formats, airing status and source as MAL's lower-case words (`tv`, `currently_airing`, …).
//! - `idMal` is read; an import of a MyAnimeList XML maps its ids to AniList media with
//!   [`Client::media_by_mal`] (`idMal_in`, 50 a request).
//!
//! HTTP goes through [`Transport`], so tests run against recorded made-up JSON. Nothing here logs
//! a token or the Client ID, and error texts carry only AniList's short error message.

use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::airing::{self, NextAiring, SeasonAnime};
use crate::http::{Request, Response, Transport};
use crate::mal::pct_encode;
use crate::ratelimit::Limiter;
use crate::service::{AnimeHit, AnimeStats, Details, ListService, MyEntry, Profile, SearchHit, ServiceError, User};
use crate::time;
use crate::tokens::{Token, TokenStore};
use crate::{Change, Kind, ListItem, Media, Remote, Score, ScoreFormat, Status, Values};

const SERVICE: &str = "anilist";
const AUTHORIZE_URL: &str = "https://anilist.co/api/v2/oauth/authorize";
/// AniList's own page that shows the token for pasting (a client registered with this redirect).
pub const PIN_URL: &str = "https://anilist.co/api/v2/oauth/pin";
/// Where an AniList API client is registered.
pub const DEVELOPER_PAGE: &str = "https://anilist.co/settings/developer";
/// The redirect to register with the AniList client: Kotora's loopback page (the same port as
/// MyAnimeList's; only one sign-in runs at a time).
pub const REDIRECT_URI: &str = crate::mal::REDIRECT_URI;
/// The only URL the bearer token is ever sent to.
const API: &str = "https://graphql.anilist.co";
/// An implicit-grant token lasts a year; this is the fallback when neither the redirect nor the
/// token says when it ends.
const YEAR: i64 = 365 * 86_400;

/// The page the browser opens: AniList's sign-in, then its redirect (the registered one) with the
/// token in the URL fragment.
pub fn authorize_url(client_id: &str) -> String {
    format!("{AUTHORIZE_URL}?client_id={}&response_type=token", pct_encode(client_id))
}

// ------------------------------------------------------------------------------------- tokens

/// What the user pastes, or what the loopback page passes on: a bare token (a JWT, from the PIN
/// page), or the address or fragment it came in (`…#access_token=…&expires_in=…`). Returns the
/// token and its end (unix seconds) at `now`.
pub fn parse_token_input(raw: &str, now: i64) -> Result<Token, String> {
    let s = raw.trim().trim_matches(|c| c == '"' || c == '\'' || c == '`').trim();
    if s.is_empty() {
        return Err("Paste the token first.".into());
    }
    let (access, expires_in) = if let Some(i) = s.find("access_token=") {
        let mut access = None;
        let mut expires_in = None;
        for pair in s[i..].split(['&', '#', '?']) {
            match pair.split_once('=') {
                Some(("access_token", v)) => access = Some(crate::loopback::pct_decode(v)),
                Some(("expires_in", v)) => expires_in = v.trim().parse::<i64>().ok(),
                _ => {}
            }
        }
        (access.unwrap_or_default(), expires_in)
    } else if let Some(i) = s.find("error=") {
        let e = s[i + 6..].split('&').next().unwrap_or("");
        return Err(format!("AniList refused the sign-in ({})", crate::loopback::pct_decode(e)));
    } else {
        (s.to_string(), None)
    };
    if access.is_empty() || access.chars().any(|c| c.is_whitespace()) || access.split('.').count() != 3 || access.len() < 40 {
        return Err("That isn't an AniList token. Copy the whole token from AniList's page (a long line of letters and dots).".into());
    }
    let ends = expires_in.map(|n| now + n).or_else(|| jwt_expiry(&access)).unwrap_or(now + YEAR);
    if ends <= now {
        return Err("That token has expired. Sign in again to get a new one.".into());
    }
    Ok(Token { access_token: access, refresh_token: String::new(), expires_at: ends })
}

/// The `exp` claim of a JWT (unix seconds), when it can be read. Nothing is checked: AniList
/// checks the token, this only tells when to ask for a new sign-in.
fn jwt_expiry(token: &str) -> Option<i64> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64url(payload)?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    v.get("exp")?.as_f64().map(|e| e as i64)
}

fn base64url(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.trim_end_matches('=').bytes() {
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------- client

/// One request budget for the whole app (sync, queue, search and details run on different
/// threads). AniList allows 90 a minute (30 when it is degraded); Kotora stays at 25.
static BUDGET: LazyLock<Arc<Mutex<Limiter>>> = LazyLock::new(|| Arc::new(Mutex::new(Limiter::anilist())));

/// A 429 gets at most this many retries.
const MAX_RETRIES: u32 = 3;
const MAX_WAIT: u64 = 60;

/// AniList calls with the rate budget and the 429 rule.
pub struct Client<T: Transport> {
    transport: T,
    limiter: Arc<Mutex<Limiter>>,
    sleep: Box<dyn FnMut(Duration) + Send>,
}

#[derive(Debug, PartialEq, Eq)]
enum CallError {
    Offline(String),
    /// A token AniList doesn't take (expired, revoked).
    Unauthorized,
    RateLimited,
    /// AniList has no such media (or entry).
    NotFound,
    Other(String),
}

impl From<CallError> for ServiceError {
    fn from(e: CallError) -> Self {
        match e {
            CallError::Offline(m) => ServiceError::Offline(m),
            CallError::Unauthorized => ServiceError::SignInAgain,
            CallError::RateLimited => ServiceError::RateLimited,
            CallError::NotFound => ServiceError::Other("AniList has no such anime".into()),
            CallError::Other(m) => ServiceError::Other(m),
        }
    }
}

impl<T: Transport> Client<T> {
    pub fn new(transport: T) -> Self {
        Client { transport, limiter: BUDGET.clone(), sleep: Box::new(std::thread::sleep) }
    }

    /// A budget of its own instead of the app's (tests, which run side by side).
    pub fn with_own_budget(mut self) -> Self {
        self.limiter = Arc::new(Mutex::new(Limiter::anilist()));
        self
    }

    /// Replaces the sleep (tests record the waits instead of waiting).
    pub fn with_sleep(mut self, sleep: impl FnMut(Duration) + Send + 'static) -> Self {
        self.sleep = Box::new(sleep);
        self
    }

    /// One GraphQL request; returns `data`.
    fn query(&mut self, access: &str, query: &str, variables: Value) -> Result<Value, CallError> {
        let body = json!({ "query": query, "variables": variables }).to_string();
        let req = Request { method: "POST", url: API.into(), bearer: Some(access.to_string()), form: None, json: Some(body) };
        let mut attempt = 0;
        loop {
            let wait = self.limiter.lock().unwrap_or_else(|e| e.into_inner()).acquire(Instant::now());
            if !wait.is_zero() {
                (self.sleep)(wait);
            }
            let resp = self.transport.send(&req).map_err(|_| CallError::Offline("no answer from AniList".into()))?;
            if resp.status == 429 {
                attempt += 1;
                if attempt > MAX_RETRIES {
                    return Err(CallError::RateLimited);
                }
                let wait = Duration::from_secs(resp.retry_after.unwrap_or(5u64 << attempt).clamp(1, MAX_WAIT));
                self.limiter.lock().unwrap_or_else(|e| e.into_inner()).hold(Instant::now(), wait);
                continue;
            }
            return answer(resp);
        }
    }
}

/// Sorts an answer: `data` on success, else what went wrong. AniList puts errors in an `errors`
/// list, with the HTTP status there too (`Invalid token` comes as 400).
fn answer(resp: Response) -> Result<Value, CallError> {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Body {
        data: Option<Value>,
        errors: Vec<GqlError>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct GqlError {
        message: String,
        status: Option<u16>,
    }
    if (500..=599).contains(&resp.status) {
        return Err(CallError::Offline(format!("AniList is having trouble (HTTP {})", resp.status)));
    }
    let body: Body = serde_json::from_str(&resp.body).unwrap_or_default();
    let first = body.errors.first();
    let message: String = first.map(|e| e.message.chars().take(80).collect()).unwrap_or_default();
    let status = first.and_then(|e| e.status).filter(|s| *s >= 400).unwrap_or(resp.status);
    let lower = message.to_ascii_lowercase();
    if resp.status == 401 || status == 401 || lower.contains("invalid token") || lower.contains("unauthorized") {
        return Err(CallError::Unauthorized);
    }
    if status == 403 {
        // AniList answers 403 when its API is switched off for maintenance: try again later.
        return Err(CallError::Offline("AniList isn't taking requests right now (HTTP 403)".into()));
    }
    if status == 404 {
        return Err(CallError::NotFound);
    }
    if (200..=299).contains(&resp.status) && body.errors.is_empty() {
        return body.data.ok_or_else(|| CallError::Other("AniList sent an answer we can't read".into()));
    }
    let words = if message.is_empty() { String::new() } else { format!(" ({message})") };
    Err(CallError::Other(format!("AniList answered HTTP {status}{words}")))
}

impl<T: Transport> std::fmt::Debug for Client<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("anilist::Client")
    }
}

// --------------------------------------------------------------------------------------- queries

/// What a list row, a search row and a season card need of a media.
const MEDIA: &str = "id idMal title { romaji english native } synonyms format episodes status season seasonYear \
startDate { year month day } endDate { year month day } averageScore popularity coverImage { medium large } \
nextAiringEpisode { episode airingAt }";
/// One list entry; the score on the 100-point scale whatever the user's format.
const ENTRY: &str = "id mediaId status progress repeat score(format: POINT_100) updatedAt";

const VIEWER: &str = "query { Viewer { id name avatar { large medium } createdAt mediaListOptions { scoreFormat } \
statistics { anime { count episodesWatched minutesWatched meanScore statuses { status count } } } } }";

fn list_query() -> String {
    format!(
        "query ($user: Int, $chunk: Int) {{ MediaListCollection(userId: $user, type: ANIME, chunk: $chunk, perChunk: 500) \
{{ hasNextChunk lists {{ entries {{ {ENTRY} media {{ {MEDIA} }} }} }} }} }}"
    )
}

fn entry_query() -> String {
    format!("query ($id: Int) {{ Media(id: $id, type: ANIME) {{ id mediaListEntry {{ {ENTRY} }} }} }}")
}

fn save_query() -> String {
    format!(
        "mutation ($mediaId: Int, $status: MediaListStatus, $progress: Int, $scoreRaw: Int, $repeat: Int) \
{{ SaveMediaListEntry(mediaId: $mediaId, status: $status, progress: $progress, scoreRaw: $scoreRaw, repeat: $repeat) {{ {ENTRY} }} }}"
    )
}

/// AniList media by MyAnimeList ids (an import), 50 to a page.
fn by_mal_query() -> String {
    format!("query ($ids: [Int], $page: Int) {{ Page(page: $page, perPage: 50) {{ pageInfo {{ hasNextPage }} media(idMal_in: $ids, type: ANIME) {{ {MEDIA} }} }} }}")
}

/// The most ids asked in one request.
pub const MAL_IDS_PER_REQUEST: usize = 50;

const DELETE: &str = "mutation ($id: Int) { DeleteMediaListEntry(id: $id) { deleted } }";

fn search_query() -> String {
    format!("query ($q: String, $n: Int) {{ Page(perPage: $n) {{ media(search: $q, type: ANIME, isAdult: false) {{ {MEDIA} }} }} }}")
}

fn details_query() -> String {
    format!(
        "query ($id: Int) {{ Media(id: $id, type: ANIME) {{ {MEDIA} description(asHtml: false) genres source duration \
studios(isMain: true) {{ nodes {{ name }} }} rankings {{ rank type allTime }} \
mediaListEntry {{ {ENTRY} startedAt {{ year month day }} completedAt {{ year month day }} }} }} }}"
    )
}

/// A season page: 50 is AniList's largest page.
pub const SEASON_PAGE: u32 = 50;

fn season_query() -> String {
    format!(
        "query ($season: MediaSeason, $year: Int, $page: Int) {{ Page(page: $page, perPage: {SEASON_PAGE}) {{ pageInfo {{ hasNextPage }} \
media(season: $season, seasonYear: $year, type: ANIME, sort: POPULARITY_DESC, isAdult: false) {{ {MEDIA} mediaListEntry {{ status progress }} }} }} }}"
    )
}

// ----------------------------------------------------------------------------------- JSON shapes

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(default)]
struct FuzzyDate {
    year: Option<i32>,
    month: Option<u32>,
    day: Option<u32>,
}

impl FuzzyDate {
    /// `2026-10-03`, `2026-10` or `2026`, as MAL writes them.
    fn text(self) -> Option<String> {
        match (self.year?, self.month, self.day) {
            (y, Some(m), Some(d)) => Some(format!("{y:04}-{m:02}-{d:02}")),
            (y, Some(m), None) => Some(format!("{y:04}-{m:02}")),
            (y, _, _) => Some(format!("{y:04}")),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Title {
    romaji: Option<String>,
    english: Option<String>,
    native: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Cover {
    medium: Option<String>,
    large: Option<String>,
}

#[derive(Deserialize, Clone, Copy)]
struct RawNext {
    episode: u32,
    #[serde(rename = "airingAt")]
    airing_at: i64,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawMedia {
    id: u32,
    id_mal: Option<u32>,
    title: Title,
    synonyms: Vec<String>,
    format: Option<String>,
    episodes: Option<u32>,
    status: Option<String>,
    season: Option<String>,
    season_year: Option<u16>,
    start_date: FuzzyDate,
    end_date: FuzzyDate,
    average_score: Option<f32>,
    popularity: Option<u32>,
    cover_image: Cover,
    next_airing_episode: Option<RawNext>,
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// AniList's format as MAL's word (`TV_SHORT` is still TV there).
fn format_word(f: Option<&str>) -> String {
    match f.unwrap_or("") {
        "TV" | "TV_SHORT" => "tv".into(),
        other => other.to_ascii_lowercase(),
    }
}

/// AniList's media status as MAL's airing words.
fn air_word(s: Option<&str>) -> String {
    match s.unwrap_or("") {
        "RELEASING" | "HIATUS" => "currently_airing",
        "FINISHED" | "CANCELLED" => "finished_airing",
        "NOT_YET_RELEASED" => "not_yet_aired",
        _ => "",
    }
    .into()
}

impl RawMedia {
    fn titles(&self) -> (String, Option<String>, Option<String>) {
        let main = non_empty(self.title.romaji.clone()).or_else(|| non_empty(self.title.english.clone())).or_else(|| non_empty(self.title.native.clone())).unwrap_or_default();
        (main, non_empty(self.title.english.clone()), non_empty(self.title.native.clone()))
    }

    fn synonyms(&self) -> Vec<String> {
        self.synonyms.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    }

    fn season(&self) -> Option<(u16, String)> {
        Some((self.season_year?, self.season.as_deref()?.to_ascii_lowercase()))
    }

    fn next(&self) -> Option<NextAiring> {
        self.next_airing_episode.filter(|n| n.episode > 0).map(|n| NextAiring { episode: n.episode, at: n.airing_at })
    }

    fn mean(&self) -> Option<f32> {
        self.average_score.filter(|s| *s > 0.0).map(|s| s / 10.0)
    }

    fn media(&self) -> Media {
        let (title, title_en, title_ja) = self.titles();
        let next = self.next();
        Media {
            kind: Kind::Anime,
            id: self.id,
            title,
            title_en,
            title_ja,
            synonyms: self.synonyms(),
            episodes: self.episodes.filter(|&e| e > 0),
            format: format_word(self.format.as_deref()),
            air_status: air_word(self.status.as_deref()),
            cover_url: self.cover_image.medium.clone().or_else(|| self.cover_image.large.clone()),
            start_date: self.start_date.text(),
            // The day and time of the exact next airing, so the timetable's slot is right.
            broadcast: next.map(NextAiring::broadcast),
            volumes: None,
            authors: Vec::new(),
            next_airing: next,
            id_mal: self.id_mal.filter(|id| *id > 0),
        }
    }

    fn hit(&self) -> AnimeHit {
        let m = self.media();
        AnimeHit {
            kind: Kind::Anime,
            id: m.id,
            title: m.title,
            title_en: m.title_en,
            title_ja: m.title_ja,
            synonyms: m.synonyms,
            format: m.format,
            episodes: m.episodes,
            air_status: m.air_status,
            season: self.season(),
            mean: self.mean(),
            cover_url: m.cover_url,
            ..Default::default()
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawEntry {
    id: u64,
    media_id: u32,
    status: Option<String>,
    progress: u32,
    repeat: u32,
    score: f32,
    updated_at: i64,
    started_at: FuzzyDate,
    completed_at: FuzzyDate,
}

/// A `POINT_100` score as Kotora keeps it (0 is no score).
fn score100(raw: f32) -> Option<Score> {
    if raw.is_finite() { Score::from_points(raw.round().max(0.0) as u32) } else { None }
}

/// AniList's list status as Kotora's status and the rewatch flag.
fn status_of(s: &str) -> Option<(Status, bool)> {
    Some(match s {
        "CURRENT" => (Status::Watching, false),
        "PLANNING" => (Status::PlanToWatch, false),
        "COMPLETED" => (Status::Completed, false),
        "REPEATING" => (Status::Completed, true),
        "PAUSED" => (Status::OnHold, false),
        "DROPPED" => (Status::Dropped, false),
        _ => return None,
    })
}

/// Kotora's status as AniList's (the rewatch is decided by the caller).
pub fn status_name(st: Status) -> &'static str {
    match st {
        Status::Watching => "CURRENT",
        Status::PlanToWatch => "PLANNING",
        Status::Completed => "COMPLETED",
        Status::OnHold => "PAUSED",
        Status::Dropped => "DROPPED",
    }
}

impl RawEntry {
    fn values(&self) -> Option<Values> {
        let (status, rewatching) = status_of(self.status.as_deref()?)?;
        Some(Values { status, progress: self.progress, score: score100(self.score), rewatching, times_rewatched: self.repeat, volumes: 0 })
    }

    fn remote(&self) -> Option<Remote> {
        Some(Remote { values: self.values()?, updated_at: self.updated_at })
    }
}

fn read<D: serde::de::DeserializeOwned>(v: Value, what: &str) -> Result<D, ServiceError> {
    serde_json::from_value(v).map_err(|_| ServiceError::Other(format!("can't read AniList's {what}")))
}

/// The `MediaListCollection` answer: the entries (each once: an entry in a custom list is also in
/// its status list) and whether another chunk follows.
pub fn parse_list(data: Value) -> Result<(Vec<ListItem>, bool), ServiceError> {
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "MediaListCollection")]
        collection: Option<Collection>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct Collection {
        has_next_chunk: bool,
        lists: Vec<Lst>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Lst {
        entries: Vec<Row>,
    }
    #[derive(Deserialize)]
    struct Row {
        #[serde(flatten)]
        entry: RawEntry,
        media: Option<RawMedia>,
    }
    let d: Data = read(data, "list")?;
    let c = d.collection.unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for row in c.lists.into_iter().flat_map(|l| l.entries) {
        let Some(media) = row.media.filter(|m| m.id != 0) else { continue };
        let Some(v) = row.entry.values() else { continue };
        if !seen.insert(media.id) {
            continue;
        }
        out.push(ListItem {
            media: media.media(),
            status: v.status,
            progress: v.progress,
            score: v.score,
            rewatching: v.rewatching,
            times_rewatched: v.times_rewatched,
            volumes: 0,
            updated_at: row.entry.updated_at,
        });
    }
    Ok((out, c.has_next_chunk))
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawViewer {
    id: u32,
    name: String,
    avatar: Cover,
    created_at: Option<i64>,
    media_list_options: Option<RawListOptions>,
    statistics: Option<RawStatistics>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawListOptions {
    score_format: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawStatistics {
    anime: Option<RawAnimeStats>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawAnimeStats {
    episodes_watched: u32,
    minutes_watched: f64,
    mean_score: f64,
    statuses: Vec<RawCount>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawCount {
    status: String,
    count: u32,
}

/// The `Viewer` answer: the user id and the user.
pub fn parse_viewer(data: Value) -> Result<(u32, User), ServiceError> {
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "Viewer")]
        viewer: Option<RawViewer>,
    }
    let d: Data = read(data, "user answer")?;
    let v = d.viewer.filter(|v| v.id != 0 && !v.name.is_empty()).ok_or_else(|| ServiceError::Other("AniList sent no user".into()))?;
    let score_format = v.media_list_options.as_ref().and_then(|o| o.score_format.as_deref()).and_then(ScoreFormat::from_anilist).unwrap_or_default();
    let stats = v.statistics.and_then(|s| s.anime).map(|a| {
        let count = |names: &[&str]| a.statuses.iter().filter(|c| names.contains(&c.status.as_str())).map(|c| c.count).sum();
        // The Profile page shows the mean out of 10. Above 10 it is on the 100-point scale;
        // else it is in the user's score format.
        let mean = match score_format {
            _ if a.mean_score > 10.0 => a.mean_score / 10.0,
            ScoreFormat::Point100 => a.mean_score / 10.0,
            ScoreFormat::Point10Decimal | ScoreFormat::Point10 => a.mean_score,
            ScoreFormat::Point5 => a.mean_score * 2.0,
            ScoreFormat::Point3 => a.mean_score * 10.0 / 3.0,
        };
        AnimeStats {
            days: if a.minutes_watched.is_finite() && a.minutes_watched > 0.0 { a.minutes_watched / 1440.0 } else { 0.0 },
            episodes: a.episodes_watched,
            mean_score: if mean.is_finite() && mean > 0.0 { mean.min(10.0) } else { 0.0 },
            rewatched: 0,
            watching: count(&["CURRENT"]),
            completed: count(&["COMPLETED", "REPEATING"]),
            on_hold: count(&["PAUSED"]),
            dropped: count(&["DROPPED"]),
            plan_to_watch: count(&["PLANNING"]),
        }
    });
    let joined = v.created_at.filter(|t| *t > 0).map(|t| {
        let (y, m, d) = time::civil_from_days(t.div_euclid(86_400));
        format!("{y:04}-{m:02}-{d:02}")
    });
    let picture = v.avatar.large.or(v.avatar.medium).filter(|p| p.starts_with("https://"));
    Ok((v.id, User { name: v.name, picture, profile: Profile { joined, stats, score_format } }))
}

/// The `Media { mediaListEntry }` answer: the user's entry, if on the list.
fn parse_entry(data: Value) -> Result<Option<RawEntry>, ServiceError> {
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "Media")]
        media: Option<One>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct One {
        media_list_entry: Option<RawEntry>,
    }
    let d: Data = read(data, "entry answer")?;
    Ok(d.media.and_then(|m| m.media_list_entry))
}

/// A search answer (`Page { media }`) as search-bar rows.
pub fn parse_hits(data: Value) -> Result<Vec<AnimeHit>, ServiceError> {
    Ok(page_media(data)?.0.iter().filter(|m| m.id != 0).map(RawMedia::hit).collect())
}

fn page_media(data: Value) -> Result<(Vec<RawMedia>, bool), ServiceError> {
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "Page")]
        page: Option<Pg>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct Pg {
        page_info: Option<Info>,
        media: Vec<RawMedia>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct Info {
        has_next_page: bool,
    }
    let d: Data = read(data, "search answer")?;
    let p = d.page.unwrap_or_default();
    Ok((p.media, p.page_info.is_some_and(|i| i.has_next_page)))
}

/// A season page: the shows, most popular first, and whether more follow.
pub fn parse_season(data: Value) -> Result<(Vec<SeasonAnime>, bool), ServiceError> {
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "Page")]
        page: Option<Pg>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct Pg {
        page_info: Option<Info>,
        media: Vec<SeasonRow>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct Info {
        has_next_page: bool,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SeasonRow {
        #[serde(flatten)]
        media: RawMedia,
        media_list_entry: Option<Mine>,
    }
    #[derive(Deserialize)]
    struct Mine {
        status: Option<String>,
        #[serde(default)]
        progress: u32,
    }
    let d: Data = read(data, "season list")?;
    let p = d.page.unwrap_or_default();
    let more = p.page_info.is_some_and(|i| i.has_next_page);
    let shows = p
        .media
        .into_iter()
        .filter(|r| r.media.id != 0)
        .map(|r| {
            let m = r.media.media();
            let mine = r.media_list_entry.and_then(|e| Some((status_of(e.status.as_deref()?)?.0.mal_name().to_string(), e.progress)));
            SeasonAnime {
                id: m.id,
                title: m.title,
                title_en: m.title_en,
                title_ja: m.title_ja,
                synonyms: m.synonyms,
                format: m.format,
                episodes: m.episodes,
                air_status: m.air_status,
                start_date: m.start_date,
                end_date: r.media.end_date.text(),
                broadcast: m.broadcast,
                mean: r.media.mean(),
                members: r.media.popularity,
                cover_url: m.cover_url,
                mine,
                next_airing: m.next_airing,
            }
        })
        .collect();
    Ok((shows, more))
}

/// AniList's description is HTML-ish even when asked for none (`<br>`, `<i>`): plain text.
fn plain(s: &str) -> String {
    let s = s.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n");
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#039;", "'").replace("&apos;", "'");
    let mut lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    lines.dedup_by(|a, b| a.is_empty() && b.is_empty());
    lines.join("\n").trim().to_string()
}

/// The details answer (`Media` with its description, studios, rankings and the user's entry).
pub fn parse_details(data: Value) -> Result<Details, ServiceError> {
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "Media")]
        media: Option<Full>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Full {
        #[serde(flatten)]
        media: RawMedia,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        genres: Vec<String>,
        #[serde(default)]
        source: Option<String>,
        #[serde(default)]
        duration: Option<u32>,
        #[serde(default)]
        studios: Option<Studios>,
        #[serde(default)]
        rankings: Vec<Ranking>,
        #[serde(default)]
        media_list_entry: Option<RawEntry>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Studios {
        nodes: Vec<Named>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Named {
        name: String,
    }
    #[derive(Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct Ranking {
        rank: u32,
        #[serde(rename = "type")]
        kind: String,
        all_time: bool,
    }
    let d: Data = read(data, "anime's information")?;
    let f = d.media.filter(|f| f.media.id != 0).ok_or_else(|| ServiceError::Other("AniList sent no anime".into()))?;
    let m = f.media.media();
    let rank = |kind: &str| f.rankings.iter().find(|r| r.all_time && r.kind == kind && r.rank > 0).map(|r| r.rank);
    let mine = f.media_list_entry.as_ref().and_then(|e| {
        let v = e.values()?;
        Some(MyEntry {
            status: v.status.mal_name().into(),
            progress: v.progress,
            score: v.score,
            rewatching: v.rewatching,
            times_rewatched: v.times_rewatched,
            volumes: 0,
            start_date: e.started_at.text(),
            finish_date: e.completed_at.text(),
        })
    });
    Ok(Details {
        kind: Kind::Anime,
        id: m.id,
        title: m.title,
        title_en: m.title_en,
        title_ja: m.title_ja,
        synonyms: m.synonyms,
        format: m.format,
        episodes: m.episodes,
        air_status: m.air_status,
        start_date: m.start_date,
        end_date: f.media.end_date.text(),
        season: f.media.season(),
        studios: f.studios.map(|s| s.nodes.into_iter().map(|n| n.name).filter(|n| !n.is_empty()).collect()).unwrap_or_default(),
        genres: f.genres.into_iter().filter(|g| !g.is_empty()).collect(),
        source: f.source.map(|s| s.to_ascii_lowercase()).unwrap_or_default(),
        minutes: f.duration.filter(|d| *d > 0),
        mean: f.media.mean(),
        rank: rank("RATED"),
        popularity: rank("POPULAR"),
        members: f.media.popularity,
        synopsis: f.description.as_deref().map(plain).unwrap_or_default(),
        cover_url: f.media.cover_image.large.clone().or_else(|| f.media.cover_image.medium.clone()),
        cover_small: f.media.cover_image.medium.clone().or_else(|| f.media.cover_image.large.clone()),
        mine,
        volumes: None,
        authors: Vec::new(),
        serialization: Vec::new(),
        next_airing: m.next_airing,
    })
}

/// The variables of `SaveMediaListEntry` for one change; only what changes goes (an argument
/// left out stays as it is on AniList). `current`: the entry on AniList now, needed only to end
/// a rewatch without a new status (`REPEATING` must become `COMPLETED`).
pub fn save_variables(media_id: u32, change: &Change, current: Option<&Values>) -> Value {
    let mut v = serde_json::Map::new();
    v.insert("mediaId".into(), json!(media_id));
    let status = match (change.rewatching, change.status) {
        // A rewatch is AniList's own status, whatever Kotora's status says (Completed).
        (Some(true), None | Some(Status::Completed | Status::Watching)) => Some("REPEATING"),
        (Some(true), Some(st)) => Some(status_name(st)),
        (Some(false), Some(st)) => Some(status_name(st)),
        (Some(false), None) => current.filter(|c| c.rewatching).map(|_| "COMPLETED"),
        (None, Some(st)) => Some(status_name(st)),
        (None, None) => None,
    };
    if let Some(st) = status {
        v.insert("status".into(), json!(st));
    }
    if let Some(p) = change.progress {
        v.insert("progress".into(), json!(p));
    }
    if let Some(sc) = change.score {
        // The 100-point value, as kept (0 removes the score); AniList shows it in the user's format.
        v.insert("scoreRaw".into(), json!(Score::points_or_zero(sc)));
    }
    if let Some(n) = change.times_rewatched {
        v.insert("repeat".into(), json!(n));
    }
    Value::Object(v)
}

impl<T: Transport> Client<T> {
    /// The signed-in user (id, name, avatar, join date, anime statistics).
    pub fn viewer(&mut self, access: &str) -> Result<(u32, User), ServiceError> {
        parse_viewer(self.query(access, VIEWER, json!({}))?)
    }

    /// The whole anime list of user `user` (500 entries a chunk).
    pub fn fetch_list(&mut self, access: &str, user: u32) -> Result<Vec<ListItem>, ServiceError> {
        let q = list_query();
        let mut all: Vec<ListItem> = Vec::new();
        // A real list is a few chunks; the cap only stops a loop.
        for chunk in 1..=40 {
            let (items, more) = parse_list(self.query(access, &q, json!({ "user": user, "chunk": chunk }))?)?;
            for it in items {
                if !all.iter().any(|a| a.media.id == it.media.id) {
                    all.push(it);
                }
            }
            if !more {
                return Ok(all);
            }
        }
        Err(ServiceError::Other("the list has too many chunks".into()))
    }

    fn raw_entry(&mut self, access: &str, media_id: u32) -> Result<Option<RawEntry>, ServiceError> {
        match self.query(access, &entry_query(), json!({ "id": media_id })) {
            Err(CallError::NotFound) => Ok(None),
            r => parse_entry(r?),
        }
    }

    /// The user's entry for one anime; `None` when it is not on the list.
    pub fn fetch_entry(&mut self, access: &str, media_id: u32) -> Result<Option<Remote>, ServiceError> {
        Ok(self.raw_entry(access, media_id)?.and_then(|e| e.remote()))
    }

    /// Sends one change (`SaveMediaListEntry`, which also adds an anime not on the list) and
    /// returns the entry AniList saved.
    pub fn save_entry(&mut self, access: &str, media_id: u32, change: &Change) -> Result<Remote, ServiceError> {
        if change.is_empty() {
            return Err(ServiceError::Other("nothing to send".into()));
        }
        // Ending a rewatch with no status picked: only AniList's status says what it is now.
        let current = if change.rewatching == Some(false) && change.status.is_none() { self.raw_entry(access, media_id)?.and_then(|e| e.values()) } else { None };
        let vars = save_variables(media_id, change, current.as_ref());
        if vars.as_object().is_some_and(|o| o.len() == 1) {
            // Only the rewatch flag, already off there: nothing to send, the entry as it is.
            return self.fetch_entry(access, media_id)?.ok_or_else(|| ServiceError::Other("that anime is not on your AniList list".into()));
        }
        let data = self.query(access, &save_query(), vars)?;
        #[derive(Deserialize)]
        struct Data {
            #[serde(rename = "SaveMediaListEntry")]
            saved: Option<RawEntry>,
        }
        let d: Data = read(data, "answer")?;
        d.saved.and_then(|e| e.remote()).ok_or_else(|| ServiceError::Other("AniList's answer has no list entry".into()))
    }

    /// Removes one anime from the list (`DeleteMediaListEntry` takes the entry's own id, so the
    /// entry is read first). Not on the list: nothing to do.
    pub fn delete_entry(&mut self, access: &str, media_id: u32) -> Result<(), ServiceError> {
        let Some(e) = self.raw_entry(access, media_id)? else { return Ok(()) };
        let data = self.query(access, DELETE, json!({ "id": e.id }))?;
        let deleted = data.pointer("/DeleteMediaListEntry/deleted").and_then(Value::as_bool).unwrap_or(false);
        if deleted { Ok(()) } else { Err(ServiceError::Other("AniList didn't remove the entry".into())) }
    }

    /// The AniList anime of these MyAnimeList ids, 50 ids a request (an id can match more than
    /// one AniList media: the first is kept). Ids AniList doesn't know are left out.
    pub fn media_by_mal(&mut self, access: &str, ids: &[u32]) -> Result<Vec<Media>, ServiceError> {
        let mut out: Vec<Media> = Vec::new();
        for chunk in ids.chunks(MAL_IDS_PER_REQUEST) {
            // A chunk is at most 50 ids, so one page holds it unless ids repeat on AniList.
            for page in 1..=3 {
                let (media, more) = page_media(self.query(access, &by_mal_query(), json!({ "ids": chunk, "page": page }))?)?;
                for m in media.iter().filter(|m| m.id != 0) {
                    let m = m.media();
                    if m.id_mal.is_some_and(|id| chunk.contains(&id)) && !out.iter().any(|o| o.id_mal == m.id_mal) {
                        out.push(m);
                    }
                }
                if !more {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// A title search for the search bar (`limit` up to 25). Under 3 characters nothing is asked.
    pub fn browse(&mut self, access: &str, query: &str, limit: u32) -> Result<Vec<AnimeHit>, ServiceError> {
        let q: String = query.trim().chars().take(64).collect();
        if q.chars().count() < 3 {
            return Ok(Vec::new());
        }
        parse_hits(self.query(access, &search_query(), json!({ "q": q, "n": limit.clamp(1, 25) }))?)
    }

    /// Recognition's search: 5 titles with their other names.
    pub fn search(&mut self, access: &str, query: &str) -> Result<Vec<SearchHit>, ServiceError> {
        Ok(self
            .browse(access, query, 5)?
            .into_iter()
            .map(|h| {
                let other_titles = h.title_en.into_iter().chain(h.title_ja).chain(h.synonyms).collect();
                SearchHit { id: h.id, title: h.title, other_titles }
            })
            .collect())
    }

    /// One anime's information page, with the user's entry.
    pub fn details(&mut self, access: &str, media_id: u32) -> Result<Details, ServiceError> {
        parse_details(self.query(access, &details_query(), json!({ "id": media_id }))?)
    }

    /// One page of a season (`page` from 0), most popular first.
    pub fn season(&mut self, access: &str, season: airing::Season, page: u32) -> Result<(Vec<SeasonAnime>, bool), ServiceError> {
        let vars = json!({ "season": season.name().to_ascii_uppercase(), "year": season.year, "page": page + 1 });
        parse_season(self.query(access, &season_query(), vars)?)
    }
}

// ---------------------------------------------------------------------------------------- service

/// How long the user read with the list is reused (the Profile refresh right after a sync).
const VIEWER_FRESH: Duration = Duration::from_secs(60);

/// The signed-in AniList account as a [`ListService`]. The token is checked for its end before
/// use; AniList has no refresh, so an ended or refused token means "sign in again".
pub struct AniList<T: Transport> {
    client: Client<T>,
    tokens: TokenStore,
    /// The user id and user from the last `Viewer` request (the list needs the id).
    viewer: Option<(u32, User, Instant)>,
}

impl<T: Transport> AniList<T> {
    pub fn new(client: Client<T>, tokens: TokenStore) -> Self {
        AniList { client, tokens, viewer: None }
    }

    fn access(&self) -> Result<String, ServiceError> {
        let t = self.tokens.load_anilist().ok_or(ServiceError::SignInAgain)?;
        if t.expires_at <= time::now_unix() {
            return Err(ServiceError::SignInAgain);
        }
        Ok(t.access_token)
    }

    fn viewer(&mut self) -> Result<(u32, User), ServiceError> {
        if let Some((id, u, at)) = &self.viewer
            && at.elapsed() < VIEWER_FRESH
        {
            return Ok((*id, u.clone()));
        }
        let access = self.access()?;
        let (id, u) = self.client.viewer(&access)?;
        self.viewer = Some((id, u.clone(), Instant::now()));
        Ok((id, u))
    }

    /// Completes a sign-in with a token (from the loopback page or pasted): checks it with
    /// AniList, then saves it. Returns the user.
    pub fn complete_sign_in(&mut self, token: &Token) -> Result<User, ServiceError> {
        let (id, user) = self.client.viewer(&token.access_token)?;
        self.tokens.save_anilist(token).map_err(|e| ServiceError::Other(format!("can't save the token file: {e}")))?;
        self.viewer = Some((id, user.clone(), Instant::now()));
        Ok(user)
    }
}

impl<T: Transport> ListService for AniList<T> {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn title(&self) -> &'static str {
        "AniList"
    }

    fn user_name(&mut self) -> Result<String, ServiceError> {
        self.viewer().map(|(_, u)| u.name)
    }

    fn user(&mut self) -> Result<User, ServiceError> {
        self.viewer().map(|(_, u)| u)
    }

    fn fetch_list(&mut self) -> Result<Vec<ListItem>, ServiceError> {
        // A fresh Viewer each sync: the Profile refresh after it reuses this one.
        self.viewer = None;
        let (id, _) = self.viewer()?;
        let access = self.access()?;
        self.client.fetch_list(&access, id)
    }

    fn get_entry(&mut self, media_id: u32) -> Result<Option<Remote>, ServiceError> {
        let access = self.access()?;
        self.client.fetch_entry(&access, media_id)
    }

    fn update_entry(&mut self, media_id: u32, change: &Change) -> Result<Remote, ServiceError> {
        let access = self.access()?;
        self.client.save_entry(&access, media_id, change)
    }

    fn delete_entry(&mut self, media_id: u32) -> Result<(), ServiceError> {
        let access = self.access()?;
        self.client.delete_entry(&access, media_id)
    }

    fn search(&mut self, query: &str) -> Result<Vec<SearchHit>, ServiceError> {
        let access = self.access()?;
        self.client.search(&access, query)
    }

    fn media_by_mal_ids(&mut self, ids: &[u32]) -> Result<Vec<Media>, ServiceError> {
        let access = self.access()?;
        self.client.media_by_mal(&access, ids)
    }

    fn browse(&mut self, query: &str, limit: u32) -> Result<Vec<AnimeHit>, ServiceError> {
        let access = self.access()?;
        self.client.browse(&access, query, limit)
    }

    fn details(&mut self, media_id: u32) -> Result<Details, ServiceError> {
        let access = self.access()?;
        self.client.details(&access, media_id)
    }

    fn season(&mut self, season: airing::Season, page: u32) -> Result<(Vec<SeasonAnime>, bool), ServiceError> {
        let access = self.access()?;
        self.client.season(&access, season, page)
    }

    fn fetch_manga_list(&mut self) -> Result<Vec<ListItem>, ServiceError> {
        self.manga_list()
    }

    fn get_manga_entry(&mut self, media_id: u32) -> Result<Option<Remote>, ServiceError> {
        let access = self.access()?;
        self.client.fetch_manga_entry(&access, media_id)
    }

    fn update_manga_entry(&mut self, media_id: u32, change: &Change) -> Result<Remote, ServiceError> {
        let access = self.access()?;
        self.client.save_manga_entry(&access, media_id, change)
    }

    fn delete_manga_entry(&mut self, media_id: u32) -> Result<(), ServiceError> {
        let access = self.access()?;
        self.client.delete_manga_entry(&access, media_id)
    }

    fn browse_manga(&mut self, query: &str, limit: u32) -> Result<Vec<AnimeHit>, ServiceError> {
        let access = self.access()?;
        self.client.browse_manga(&access, query, limit)
    }

    fn manga_details(&mut self, media_id: u32) -> Result<Details, ServiceError> {
        let access = self.access()?;
        self.client.manga_details(&access, media_id)
    }
}

mod manga;

#[cfg(test)]
mod tests;
