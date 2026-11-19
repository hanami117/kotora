//! Volumes from chapters, with MangaDex's volume table (public API, no account, no key).
//!
//! MyAnimeList and AniList count volumes read, but readers who log chapters leave it at 0. MangaDex
//! knows which chapters each volume holds: for a manga of the user's list Kotora finds the
//! MangaDex series that links to it (a title search, kept only when its `links.mal` is the
//! MyAnimeList id, or its `links.al` the AniList id), reads its volume table
//! (`/manga/{id}/aggregate`), and [`volumes_for`] turns chapters read into volumes read.
//!
//! - **Only on request** (Settings → Account → Fill volumes from chapters…, the editor's button):
//!   no background calls. One search (two at most: the main title, then the English one) and one
//!   aggregate per series, through [`Limiter`] at under 3 requests a second (MangaDex allows 5).
//! - **Cached on disk** per series ([`Cache`]: one small JSON file each in the cache folder,
//!   safe to delete): found for [`FOUND_TTL`], not found for [`MISSING_TTL`]; a lookup asked again
//!   on purpose skips the cache.
//! - MangaDex's acceptable use: a `User-Agent` naming Kotora (as every request Kotora makes), the
//!   rate limit, and the credit "Volume data: MangaDex" wherever the numbers show. Nothing of
//!   the user's goes to MangaDex but the titles searched.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::http::{Request, Transport};
use crate::ratelimit::Limiter;
use crate::service::ServiceError;

pub const API: &str = "https://api.mangadex.org";

/// A found series is looked up again after 30 days (volumes get their chapters as they come out).
pub const FOUND_TTL: i64 = 30 * 86_400;
/// A series not found is asked again after a week.
pub const MISSING_TTL: i64 = 7 * 86_400;

/// One volume: its number and the highest chapter number in it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vol {
    pub volume: f64,
    pub last_chapter: f64,
}

/// A series' volume table, as MangaDex has it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Table {
    /// The MangaDex series id (a UUID).
    pub md_id: String,
    pub title: String,
    /// Volumes in order, each with its last chapter; volumes with no numbered chapter left out.
    pub volumes: Vec<Vol>,
}

/// What a lookup found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Lookup {
    Found(Table),
    /// No MangaDex series links to this manga (or it has no volume numbers).
    NotFound,
}

/// The manga looked up: its MyAnimeList id, or its AniList id when it has no MyAnimeList id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Mal(u32),
    AniList(u32),
}

impl Key {
    fn file_name(self) -> String {
        match self {
            Key::Mal(id) => format!("mal-{id}.json"),
            Key::AniList(id) => format!("al-{id}.json"),
        }
    }

    /// Whether the MangaDex series' links point at this manga.
    fn matches(self, links: &Links) -> bool {
        let num = |s: &Option<String>| s.as_deref().and_then(|v| v.trim().parse::<u32>().ok());
        match self {
            Key::Mal(id) => num(&links.mal) == Some(id),
            Key::AniList(id) => num(&links.al) == Some(id),
        }
    }
}

/// Volumes read for `chapters` chapters read: the highest volume whose last chapter is read
/// (volume 10.5 counts as 10). 0 when no volume is finished.
pub fn volumes_for(table: &Table, chapters: u32) -> u32 {
    let read = f64::from(chapters);
    table
        .volumes
        .iter()
        .filter(|v| v.last_chapter <= read)
        .map(|v| v.volume.floor().max(0.0) as u32)
        .max()
        .unwrap_or(0)
}

// ------------------------------------------------------------------------------- JSON shapes

#[derive(Deserialize, Default)]
#[serde(default)]
struct Links {
    mal: Option<String>,
    al: Option<String>,
}

/// A search hit: the series, its links and a title.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: String,
    pub title: String,
    pub mal: Option<u32>,
    pub al: Option<u32>,
}

/// The search answer (`GET /manga?title=…`).
pub fn parse_search(body: &str) -> Result<Vec<Hit>, String> {
    #[derive(Deserialize)]
    struct Answer {
        #[serde(default)]
        data: Vec<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        id: String,
        #[serde(default)]
        attributes: Attrs,
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Attrs {
        title: std::collections::BTreeMap<String, String>,
        links: Option<Links>,
    }
    let a: Answer = serde_json::from_str(body)
        .map_err(|e| format!("can\u{2019}t read MangaDex\u{2019}s search answer: {e}"))?;
    Ok(a.data
        .into_iter()
        .map(|it| {
            let links = it.attributes.links.unwrap_or_default();
            let num = |s: &Option<String>| s.as_deref().and_then(|v| v.trim().parse::<u32>().ok());
            let title = it
                .attributes
                .title
                .get("en")
                .or_else(|| it.attributes.title.values().next())
                .cloned()
                .unwrap_or_default();
            Hit {
                mal: num(&links.mal),
                al: num(&links.al),
                id: it.id,
                title,
            }
        })
        .collect())
}

/// The aggregate answer (`GET /manga/{id}/aggregate`): `volumes` is an object keyed by volume
/// ("none" for chapters with no volume), or an empty array; each volume's `chapters` likewise.
/// Volumes and chapters that aren't numbers ("none", "Extra", "1-2") are left out.
pub fn parse_aggregate(body: &str) -> Result<Vec<Vol>, String> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| format!("can\u{2019}t read MangaDex\u{2019}s volume table: {e}"))?;
    let entries = |x: &Value| -> Vec<Value> {
        match x {
            Value::Object(m) => m.values().cloned().collect(),
            Value::Array(a) => a.clone(),
            _ => Vec::new(),
        }
    };
    let num = |x: Option<&Value>| {
        x.and_then(Value::as_str)
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|n| n.is_finite() && *n >= 0.0)
    };
    let mut out: Vec<Vol> = Vec::new();
    for vol in entries(v.get("volumes").unwrap_or(&Value::Null)) {
        let Some(volume) = num(vol.get("volume")) else {
            continue;
        };
        let last = entries(vol.get("chapters").unwrap_or(&Value::Null))
            .iter()
            .filter_map(|c| num(c.get("chapter")))
            .fold(None, |m: Option<f64>, c| Some(m.map_or(c, |m| m.max(c))));
        if let Some(last_chapter) = last {
            out.push(Vol {
                volume,
                last_chapter,
            });
        }
    }
    out.sort_by(|a, b| a.volume.total_cmp(&b.volume));
    Ok(out)
}

// ------------------------------------------------------------------------------------ client

/// The MangaDex client: its own rate budget, and a 429 waited out once.
pub struct Client<T: Transport> {
    transport: T,
    limiter: Limiter,
    /// Sleeps (the real clock; tests pass one that doesn't).
    sleep: fn(Duration),
}

impl<T: Transport> Client<T> {
    pub fn new(transport: T) -> Self {
        Client {
            transport,
            limiter: Limiter::new(120, Duration::from_millis(350)),
            sleep: std::thread::sleep,
        }
    }

    /// Tests: no real waiting.
    pub fn without_sleep(transport: T) -> Self {
        Client {
            sleep: |_| {},
            ..Client::new(transport)
        }
    }

    fn get(&mut self, url: String) -> Result<String, ServiceError> {
        for attempt in 0..2 {
            let wait = self.limiter.acquire(Instant::now());
            (self.sleep)(wait);
            let r = self
                .transport
                .send(&Request {
                    method: "GET",
                    url: url.clone(),
                    bearer: None,
                    form: None,
                    json: None,
                })
                .map_err(|e| ServiceError::Offline(format!("can\u{2019}t reach MangaDex: {e}")))?;
            match r.status {
                200..=299 => return Ok(r.body),
                429 if attempt == 0 => {
                    let wait = Duration::from_secs(r.retry_after.unwrap_or(5).min(60));
                    self.limiter.hold(Instant::now(), wait);
                }
                429 => return Err(ServiceError::RateLimited),
                404 => return Err(ServiceError::Other("MangaDex has no such series".into())),
                s => return Err(ServiceError::Other(format!("MangaDex answered HTTP {s}"))),
            }
        }
        Err(ServiceError::RateLimited)
    }

    /// Series whose title matches `q` (every content rating: the user's list decides).
    pub fn search(&mut self, q: &str) -> Result<Vec<Hit>, ServiceError> {
        let q: String = q.trim().chars().take(100).collect();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let ratings = ["safe", "suggestive", "erotica", "pornographic"]
            .iter()
            .map(|r| format!("&contentRating%5B%5D={r}"))
            .collect::<String>();
        let url = format!(
            "{API}/manga?title={}&limit=10{ratings}&order%5Brelevance%5D=desc",
            crate::mal::pct_encode(&q)
        );
        parse_search(&self.get(url)?).map_err(ServiceError::Other)
    }

    /// The volume table of MangaDex series `md_id` (every language: volume numbers are the
    /// publisher's whichever translation listed the chapter).
    pub fn aggregate(&mut self, md_id: &str) -> Result<Vec<Vol>, ServiceError> {
        let id: String = md_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        parse_aggregate(&self.get(format!("{API}/manga/{id}/aggregate"))?)
            .map_err(ServiceError::Other)
    }

    /// The series linking to `key`, searched by each of `titles` in turn (until one links), and
    /// its volume table.
    pub fn lookup(&mut self, key: Key, titles: &[String]) -> Result<Lookup, ServiceError> {
        let mut seen = std::collections::HashSet::new();
        for t in titles.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
            if !seen.insert(t.to_lowercase()) {
                continue;
            }
            let hits = self.search(t)?;
            let Some(hit) = hits.into_iter().find(|h| {
                key.matches(&Links {
                    mal: h.mal.map(|n| n.to_string()),
                    al: h.al.map(|n| n.to_string()),
                })
            }) else {
                continue;
            };
            let volumes = self.aggregate(&hit.id)?;
            if volumes.is_empty() {
                return Ok(Lookup::NotFound);
            }
            return Ok(Lookup::Found(Table {
                md_id: hit.id,
                title: hit.title,
                volumes,
            }));
        }
        Ok(Lookup::NotFound)
    }
}

impl<T: Transport> Client<T> {
    /// The series linking to `key` (its `links.mal` / `links.al`), searched by each of `titles`;
    /// no title guessing: a series without the link is never taken.
    pub fn find_linked(
        &mut self,
        key: Key,
        titles: &[String],
    ) -> Result<Option<Hit>, ServiceError> {
        let mut seen = std::collections::HashSet::new();
        for t in titles
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .take(3)
        {
            if !seen.insert(t.to_lowercase()) {
                continue;
            }
            if let Some(h) = self.search(t)?.into_iter().find(|h| {
                key.matches(&Links {
                    mal: h.mal.map(|n| n.to_string()),
                    al: h.al.map(|n| n.to_string()),
                })
            }) {
                return Ok(Some(h));
            }
        }
        Ok(None)
    }

    /// The newest chapters of series `md_id`, every language, external ones too (official
    /// releases MangaDex links to, e.g. MANGA Plus), by the time they became readable.
    pub fn feed(&mut self, md_id: &str) -> Result<Vec<FeedChapter>, ServiceError> {
        let id: String = md_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        let ratings = ["safe", "suggestive", "erotica", "pornographic"]
            .iter()
            .map(|r| format!("&contentRating%5B%5D={r}"))
            .collect::<String>();
        let url = format!(
            "{API}/manga/{id}/feed?limit=100&order%5BreadableAt%5D=desc&includeExternalUrl=1&includeEmptyPages=1&includeFuturePublishAt=0{ratings}"
        );
        parse_feed(&self.get(url)?).map_err(ServiceError::Other)
    }

    /// Series `md_id`'s title (checks a pasted link).
    pub fn title_of(&mut self, md_id: &str) -> Result<(String, String), ServiceError> {
        let id: String = md_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        let body = self.get(format!("{API}/manga/{id}"))?;
        let v: Value = serde_json::from_str(&body).map_err(|e| {
            ServiceError::Other(format!("can\u{2019}t read MangaDex\u{2019}s series: {e}"))
        })?;
        let data = v
            .get("data")
            .ok_or_else(|| ServiceError::Other("MangaDex has no such series".into()))?;
        let titles = data
            .get("attributes")
            .and_then(|a| a.get("title"))
            .and_then(Value::as_object);
        let title = titles
            .and_then(|m| m.get("en").or_else(|| m.values().next()))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        Ok((
            data.get("id")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string(),
            title,
        ))
    }
}

/// One chapter of a series' feed.
#[derive(Clone, Debug, PartialEq)]
pub struct FeedChapter {
    /// As written ("74", "79.1"); empty for a volume-only entry.
    pub chapter: String,
    pub volume: Option<String>,
    pub lang: String,
    /// When it became readable (unix seconds).
    pub at: i64,
    /// An official release on another site (MANGA Plus, …): MangaDex only links to it.
    pub external: bool,
}

/// `/manga/{id}/feed`'s answer. Chapters not readable yet or unavailable are left out.
pub fn parse_feed(body: &str) -> Result<Vec<FeedChapter>, String> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| format!("can\u{2019}t read MangaDex\u{2019}s chapters: {e}"))?;
    let Some(data) = v.get("data").and_then(Value::as_array) else {
        return Err("MangaDex\u{2019}s chapters answer has no data".into());
    };
    let s = |a: &Value, k: &str| {
        a.get(k)
            .and_then(Value::as_str)
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
    };
    Ok(data
        .iter()
        .filter_map(|c| {
            let a = c.get("attributes")?;
            if a.get("isUnavailable").and_then(Value::as_bool) == Some(true) {
                return None;
            }
            let at = s(a, "readableAt")
                .or_else(|| s(a, "publishAt"))
                .and_then(|t| crate::time::parse_rfc3339(&t))?;
            Some(FeedChapter {
                chapter: s(a, "chapter").unwrap_or_default(),
                volume: s(a, "volume"),
                lang: s(a, "translatedLanguage").unwrap_or_default(),
                at,
                external: s(a, "externalUrl").is_some(),
            })
        })
        .collect())
}

/// The feed as releases for the estimate: English chapters when the feed has a few (other
/// languages are often numbered or timed apart), every language otherwise.
pub fn feed_releases(feed: &[FeedChapter]) -> Vec<crate::mangaupdates::Release> {
    let en = feed.iter().filter(|c| c.lang == "en").count() >= 3;
    feed.iter()
        .filter(|c| !en || c.lang == "en")
        .map(|c| crate::mangaupdates::Release {
            chapter: c.chapter.clone(),
            volume: c.volume.clone(),
            day: c.at.div_euclid(86_400) * 86_400,
            added: c.at,
        })
        .collect()
}

/// The series id (a UUID) in a pasted `mangadex.org/title/<uuid>/…` link, or a bare UUID.
pub fn md_id_from_link(s: &str) -> Option<String> {
    let s = s.trim().to_ascii_lowercase();
    let rest = match s.split_once("mangadex.org/") {
        Some((_, r)) => r
            .strip_prefix("title/")
            .or_else(|| r.strip_prefix("manga/"))?,
        None => s.as_str(),
    };
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_hexdigit() || *c == '-')
        .collect();
    let parts: Vec<usize> = id.split('-').map(str::len).collect();
    (parts == [8, 4, 4, 4, 12]).then_some(id)
}

/// A series' page on mangadex.org.
pub fn md_url(id: &str) -> String {
    format!("https://mangadex.org/title/{id}")
}

impl Client<crate::http::UreqTransport> {
    /// The real client (`User-Agent: Kotora/<version>`).
    pub fn online() -> Self {
        Client::new(crate::http::UreqTransport::default())
    }
}

// ------------------------------------------------------------------------------------- cache

#[derive(Serialize, Deserialize)]
struct Cached {
    at: i64,
    lookup: Lookup,
}

/// One JSON file per series in `<cache>/mangadex/` (safe to delete).
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Cache { dir }
    }

    /// Kotora's own cache folder.
    pub fn default_dir() -> Self {
        Cache::new(crate::paths::cache_dir().join("mangadex"))
    }

    /// A lookup still fresh at `now`.
    pub fn get(&self, key: Key, now: i64) -> Option<Lookup> {
        let text = std::fs::read_to_string(self.dir.join(key.file_name())).ok()?;
        let c: Cached = serde_json::from_str(&text).ok()?;
        let ttl = if matches!(c.lookup, Lookup::Found(_)) {
            FOUND_TTL
        } else {
            MISSING_TTL
        };
        (now - c.at < ttl).then_some(c.lookup)
    }

    pub fn put(&self, key: Key, lookup: &Lookup, now: i64) {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        if let Ok(text) = serde_json::to_string(&Cached {
            at: now,
            lookup: lookup.clone(),
        }) {
            let tmp = self.dir.join(format!("{}.tmp", key.file_name()));
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, self.dir.join(key.file_name()));
            }
        }
    }
}

/// A lookup through the cache: a fresh saved answer, else MangaDex (then saved). `refresh`:
/// ask MangaDex even when a saved answer is fresh. Returns the lookup and whether MangaDex was
/// asked.
pub fn find<T: Transport>(
    client: &mut Client<T>,
    cache: &Cache,
    key: Key,
    titles: &[String],
    now: i64,
    refresh: bool,
) -> Result<(Lookup, bool), ServiceError> {
    if !refresh && let Some(l) = cache.get(key, now) {
        return Ok((l, false));
    }
    let l = client.lookup(key, titles)?;
    cache.put(key, &l, now);
    Ok((l, true))
}

// -------------------------------------------------------------------------------------- plan

/// The key a list entry is looked up by on `service`: its MyAnimeList id (AniList: `idMal`),
/// else its AniList id.
pub fn key_of(service: crate::service::ServiceId, m: &crate::Media) -> Key {
    match (service, m.id_mal.filter(|id| *id > 0)) {
        (crate::service::ServiceId::Mal, _) => Key::Mal(m.id),
        (crate::service::ServiceId::AniList, Some(mal)) => Key::Mal(mal),
        (crate::service::ServiceId::AniList, None) => Key::AniList(m.id),
    }
}

/// The titles searched for an entry: the main one, then the English one.
pub fn titles_of(m: &crate::Media) -> Vec<String> {
    std::iter::once(m.title.clone())
        .chain(m.title_en.clone())
        .filter(|t| !t.trim().is_empty())
        .collect()
}

/// Whether an entry is worth a lookup: chapters read, and volumes not at the known total.
pub fn wants_lookup(it: &crate::ListItem) -> bool {
    it.progress > 0 && it.media.volumes.is_none_or(|n| it.volumes < n)
}

/// One manga in the preview.
#[derive(Clone, Debug, PartialEq)]
pub struct FillRow {
    pub media_id: u32,
    pub title: String,
    pub chapters: u32,
    /// Volumes read now.
    pub from: u32,
    /// Volumes read by MangaDex's table (capped at the known total).
    pub to: u32,
}

/// What "Fill volumes from chapters" would do.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FillPlan {
    /// Volumes that go up.
    pub fill: Vec<FillRow>,
    /// Titles MangaDex has no linked series (or no volume numbers) for.
    pub not_found: Vec<String>,
    /// Already what MangaDex says.
    pub same: Vec<FillRow>,
    /// Higher than MangaDex says: left alone (volumes are never lowered).
    pub higher: Vec<FillRow>,
}

/// The preview from the looked-up entries (`None`: not looked up, left out). Never lowers.
pub fn plan_fill(rows: &[(crate::ListItem, Option<Lookup>)]) -> FillPlan {
    let mut p = FillPlan::default();
    for (it, l) in rows {
        let Some(l) = l else { continue };
        let Lookup::Found(t) = l else {
            p.not_found.push(it.media.title.clone());
            continue;
        };
        let mut to = volumes_for(t, it.progress);
        if let Some(n) = it.media.volumes.filter(|n| *n > 0) {
            to = to.min(n);
        }
        let row = FillRow {
            media_id: it.media.id,
            title: it.media.title.clone(),
            chapters: it.progress,
            from: it.volumes,
            to,
        };
        match to.cmp(&it.volumes) {
            std::cmp::Ordering::Greater => p.fill.push(row),
            std::cmp::Ordering::Equal => p.same.push(row),
            std::cmp::Ordering::Less => p.higher.push(row),
        }
    }
    for list in [&mut p.fill, &mut p.same, &mut p.higher] {
        list.sort_by_cached_key(|r| r.title.to_lowercase());
    }
    p.not_found.sort_by_cached_key(|t| t.to_lowercase());
    p
}

/// The queue's changes for a plan: the volumes of each manga that goes up, nothing else.
pub fn fill_actions(p: &FillPlan) -> Vec<crate::listxml::Action> {
    p.fill
        .iter()
        .map(|r| crate::listxml::Action::Update {
            kind: crate::Kind::Manga,
            media_id: r.media_id,
            change: crate::Change {
                volumes: Some(r.to),
                ..Default::default()
            },
        })
        .collect()
}

#[cfg(test)]
mod tests;
