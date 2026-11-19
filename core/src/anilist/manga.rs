//! AniList manga: the user's manga list (`MediaListCollection` of type `MANGA`, in chunks), one
//! entry (read, `SaveMediaListEntry`, `DeleteMediaListEntry`), search and the information page.
//! The same client, token, rate budget (25 a minute, shared with anime) and error mapping as
//! anime; only the type and a few fields differ.
//!
//! Kotora keeps one shape for both kinds (see `mal/manga.rs`): chapters go in `progress` /
//! `episodes`, volumes in `volumes` (`progressVolumes`), the reread in `rewatching` /
//! `times_rewatched` (`REPEATING` and `repeat`), the statuses under their anime names. Formats and
//! publishing statuses become MyAnimeList's words, so the labels are the same on both services:
//! - `MANGA` is `manga`, or `manhwa` / `manhua` by country of origin (KR; CN, TW); `NOVEL` is
//!   `light_novel` (AniList's "Light Novel"); `ONE_SHOT` is `one_shot`.
//! - `RELEASING` `currently_publishing`, `FINISHED` `finished`, `HIATUS` `on_hiatus`,
//!   `CANCELLED` `discontinued`, `NOT_YET_RELEASED` `not_yet_published`.
//!
//! Authors come from the staff with a story or art role ("Story & Art", "Story", "Art",
//! "Original Story", "Original Creator", "Illustration"); translators, letterers and the like are
//! left out. AniList has no magazine, so `serialization` stays empty. `idMal` is kept on every
//! list row (`Media.id_mal`) so the mirror can match MyAnimeList manga later.

use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    AniList, CallError, Client, Cover, FuzzyDate, Title, non_empty, plain, read, save_variables,
    score100, status_of,
};
use crate::http::Transport;
use crate::service::{AnimeHit, Author, Details, MyEntry, ServiceError};
use crate::{Change, Kind, ListItem, Media, Remote, Values};

/// What a list row, a search row and the details page need of a manga.
const MEDIA: &str = "id idMal title { romaji english native } synonyms format status chapters volumes countryOfOrigin \
startDate { year month day } endDate { year month day } averageScore popularity coverImage { medium large }";
/// The staff asked for authors: the first few by relevance (story and art come first there).
const STAFF: &str =
    "staff(sort: [RELEVANCE, ID], perPage: 8) { edges { role node { name { full } } } }";
/// One manga list entry; the score on the 100-point scale whatever the user's format.
const ENTRY: &str =
    "id mediaId status progress progressVolumes repeat score(format: POINT_100) updatedAt";

fn list_query() -> String {
    // No staff here: a chunk of 500 with staff would cost AniList far more, and the list shows none.
    format!(
        "query ($user: Int, $chunk: Int) {{ MediaListCollection(userId: $user, type: MANGA, chunk: $chunk, perChunk: 500) \
{{ hasNextChunk lists {{ entries {{ {ENTRY} media {{ {MEDIA} }} }} }} }} }}"
    )
}

fn entry_query() -> String {
    format!(
        "query ($id: Int) {{ Media(id: $id, type: MANGA) {{ id mediaListEntry {{ {ENTRY} }} }} }}"
    )
}

fn save_query() -> String {
    format!(
        "mutation ($mediaId: Int, $status: MediaListStatus, $progress: Int, $progressVolumes: Int, $scoreRaw: Int, $repeat: Int) \
{{ SaveMediaListEntry(mediaId: $mediaId, status: $status, progress: $progress, progressVolumes: $progressVolumes, scoreRaw: $scoreRaw, repeat: $repeat) {{ {ENTRY} }} }}"
    )
}

fn search_query() -> String {
    format!(
        "query ($q: String, $n: Int) {{ Page(perPage: $n) {{ media(search: $q, type: MANGA, isAdult: false) {{ {MEDIA} {STAFF} }} }} }}"
    )
}

/// AniList's manga for MyAnimeList manga ids (an import or the mirror): 50 ids a page.
fn by_mal_query() -> String {
    format!(
        "query ($ids: [Int], $page: Int) {{ Page(page: $page, perPage: 50) {{ pageInfo {{ hasNextPage }} media(idMal_in: $ids, type: MANGA) {{ {MEDIA} }} }} }}"
    )
}

fn details_query() -> String {
    format!(
        "query ($id: Int) {{ Media(id: $id, type: MANGA) {{ {MEDIA} {STAFF} description(asHtml: false) genres rankings {{ rank type allTime }} \
mediaListEntry {{ {ENTRY} startedAt {{ year month day }} completedAt {{ year month day }} }} }} }}"
    )
}

// ----------------------------------------------------------------------------------- JSON shapes

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawManga {
    id: u32,
    id_mal: Option<u32>,
    title: Title,
    synonyms: Vec<String>,
    format: Option<String>,
    status: Option<String>,
    chapters: Option<u32>,
    volumes: Option<u32>,
    country_of_origin: Option<String>,
    start_date: FuzzyDate,
    end_date: FuzzyDate,
    average_score: Option<f32>,
    popularity: Option<u32>,
    cover_image: Cover,
    staff: Option<Staff>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Staff {
    edges: Vec<StaffEdge>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct StaffEdge {
    role: Option<String>,
    node: Option<StaffNode>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct StaffNode {
    name: StaffName,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct StaffName {
    full: Option<String>,
}

/// AniList's manga format as MyAnimeList's word (`manga_type_label` turns it into a label).
pub fn format_word(format: Option<&str>, country: Option<&str>) -> String {
    match (format.unwrap_or(""), country.unwrap_or("")) {
        ("MANGA", "KR") => "manhwa".into(),
        ("MANGA", "CN" | "TW") => "manhua".into(),
        ("NOVEL", _) => "light_novel".into(),
        ("", _) => String::new(),
        (other, _) => other.to_ascii_lowercase(),
    }
}

/// AniList's media status as MyAnimeList's publishing words.
pub fn publishing_word(status: Option<&str>) -> String {
    match status.unwrap_or("") {
        "RELEASING" => "currently_publishing",
        "FINISHED" => "finished",
        "HIATUS" => "on_hiatus",
        "CANCELLED" => "discontinued",
        "NOT_YET_RELEASED" => "not_yet_published",
        _ => "",
    }
    .into()
}

/// The staff roles that make an author, as AniList writes them (a note in brackets, such as
/// "Story (ch. 1-20)", is left on).
fn author_role(role: &str) -> bool {
    let base = role.split('(').next().unwrap_or("").trim();
    matches!(
        base,
        "Story & Art" | "Story" | "Art" | "Original Story" | "Original Creator" | "Illustration"
    )
}

impl RawManga {
    fn titles(&self) -> (String, Option<String>, Option<String>) {
        let main = non_empty(self.title.romaji.clone())
            .or_else(|| non_empty(self.title.english.clone()))
            .or_else(|| non_empty(self.title.native.clone()))
            .unwrap_or_default();
        (
            main,
            non_empty(self.title.english.clone()),
            non_empty(self.title.native.clone()),
        )
    }

    fn authors(&self) -> Vec<Author> {
        let mut out: Vec<Author> = Vec::new();
        for e in self.staff.iter().flat_map(|s| &s.edges) {
            let (Some(role), Some(name)) = (
                e.role.as_deref().map(str::trim),
                e.node.as_ref().and_then(|n| non_empty(n.name.full.clone())),
            ) else {
                continue;
            };
            if !author_role(role) {
                continue;
            }
            // One person with two roles ("Story", then "Art") is one author: "Story & Art".
            match out.iter_mut().find(|a| a.name == name) {
                Some(a) if !a.role.split(" & ").any(|r| r == role) => {
                    a.role = format!("{} & {role}", a.role)
                }
                Some(_) => {}
                None => out.push(Author {
                    name,
                    role: role.to_string(),
                }),
            }
        }
        out
    }

    fn mean(&self) -> Option<f32> {
        self.average_score.filter(|s| *s > 0.0).map(|s| s / 10.0)
    }

    fn media(&self) -> Media {
        let (title, title_en, title_ja) = self.titles();
        Media {
            kind: Kind::Manga,
            id: self.id,
            title,
            title_en,
            title_ja,
            synonyms: self
                .synonyms
                .iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            episodes: self.chapters.filter(|&c| c > 0),
            format: format_word(self.format.as_deref(), self.country_of_origin.as_deref()),
            air_status: publishing_word(self.status.as_deref()),
            cover_url: self
                .cover_image
                .medium
                .clone()
                .or_else(|| self.cover_image.large.clone()),
            start_date: self.start_date.text(),
            broadcast: None,
            volumes: self.volumes.filter(|&v| v > 0),
            authors: self.authors().into_iter().map(|a| a.name).collect(),
            next_airing: None,
            id_mal: self.id_mal.filter(|id| *id > 0),
        }
    }

    fn hit(&self) -> AnimeHit {
        let m = self.media();
        AnimeHit {
            kind: Kind::Manga,
            id: m.id,
            title: m.title,
            title_en: m.title_en,
            title_ja: m.title_ja,
            synonyms: m.synonyms,
            format: m.format,
            episodes: m.episodes,
            air_status: m.air_status,
            season: None,
            mean: self.mean(),
            cover_url: m.cover_url,
            volumes: m.volumes,
            authors: m.authors,
            start_date: m.start_date,
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
    progress_volumes: Option<u32>,
    repeat: u32,
    score: f32,
    updated_at: i64,
    started_at: FuzzyDate,
    completed_at: FuzzyDate,
}

impl RawEntry {
    fn values(&self) -> Option<Values> {
        let (status, rewatching) = status_of(self.status.as_deref()?)?;
        Some(Values {
            status,
            progress: self.progress,
            score: score100(self.score),
            rewatching,
            times_rewatched: self.repeat,
            volumes: self.progress_volumes.unwrap_or(0),
        })
    }

    fn remote(&self) -> Option<Remote> {
        Some(Remote {
            values: self.values()?,
            updated_at: self.updated_at,
        })
    }
}

/// The manga `MediaListCollection` answer: the entries (each once: an entry in a custom list is
/// also in its status list) and whether another chunk follows.
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
        media: Option<RawManga>,
    }
    let d: Data = read(data, "manga list")?;
    let c = d.collection.unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for row in c.lists.into_iter().flat_map(|l| l.entries) {
        let Some(media) = row.media.filter(|m| m.id != 0) else {
            continue;
        };
        let Some(v) = row.entry.values() else {
            continue;
        };
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
            volumes: v.volumes,
            updated_at: row.entry.updated_at,
        });
    }
    Ok((out, c.has_next_chunk))
}

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
    let d: Data = read(data, "manga entry answer")?;
    Ok(d.media.and_then(|m| m.media_list_entry))
}

/// A manga search answer (`Page { media }`) as search-bar rows.
pub fn parse_hits(data: Value) -> Result<Vec<AnimeHit>, ServiceError> {
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "Page")]
        page: Option<Pg>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Pg {
        media: Vec<RawManga>,
    }
    let d: Data = read(data, "manga search answer")?;
    Ok(d.page
        .unwrap_or_default()
        .media
        .iter()
        .filter(|m| m.id != 0)
        .map(RawManga::hit)
        .collect())
}

/// A by-MAL-id page: the manga and whether more pages follow.
fn parse_page(data: Value) -> Result<(Vec<RawManga>, bool), ServiceError> {
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "Page")]
        page: Option<Pg>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct Pg {
        page_info: Option<Info>,
        media: Vec<RawManga>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct Info {
        has_next_page: bool,
    }
    let d: Data = read(data, "manga lookup answer")?;
    let p = d.page.unwrap_or_default();
    Ok((p.media, p.page_info.is_some_and(|i| i.has_next_page)))
}

/// The manga details answer (`Media` with its description, staff, rankings and the user's entry).
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
        media: RawManga,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        genres: Vec<String>,
        #[serde(default)]
        rankings: Vec<Ranking>,
        #[serde(default)]
        media_list_entry: Option<RawEntry>,
    }
    #[derive(Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct Ranking {
        rank: u32,
        #[serde(rename = "type")]
        kind: String,
        all_time: bool,
    }
    let d: Data = read(data, "manga's information")?;
    let f = d
        .media
        .filter(|f| f.media.id != 0)
        .ok_or_else(|| ServiceError::Other("AniList sent no manga".into()))?;
    let m = f.media.media();
    let rank = |kind: &str| {
        f.rankings
            .iter()
            .find(|r| r.all_time && r.kind == kind && r.rank > 0)
            .map(|r| r.rank)
    };
    let mine = f.media_list_entry.as_ref().and_then(|e| {
        let v = e.values()?;
        Some(MyEntry {
            status: v.status.mal_name().into(),
            progress: v.progress,
            score: v.score,
            rewatching: v.rewatching,
            times_rewatched: v.times_rewatched,
            volumes: v.volumes,
            start_date: e.started_at.text(),
            finish_date: e.completed_at.text(),
        })
    });
    Ok(Details {
        kind: Kind::Manga,
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
        season: None,
        studios: Vec::new(),
        genres: f.genres.into_iter().filter(|g| !g.is_empty()).collect(),
        source: String::new(),
        minutes: None,
        mean: f.media.mean(),
        rank: rank("RATED"),
        popularity: rank("POPULAR"),
        members: f.media.popularity,
        synopsis: f.description.as_deref().map(plain).unwrap_or_default(),
        cover_url: f
            .media
            .cover_image
            .large
            .clone()
            .or_else(|| f.media.cover_image.medium.clone()),
        cover_small: f
            .media
            .cover_image
            .medium
            .clone()
            .or_else(|| f.media.cover_image.large.clone()),
        mine,
        volumes: m.volumes,
        authors: f.media.authors(),
        serialization: Vec::new(),
        next_airing: None,
    })
}

/// The variables of `SaveMediaListEntry` for one manga change: the anime ones
/// ([`save_variables`]: status with `REPEATING` for a reread, chapters as `progress`, `scoreRaw`,
/// `repeat`) and the volumes read as `progressVolumes`. Only what changes goes.
pub fn save_manga_variables(media_id: u32, change: &Change, current: Option<&Values>) -> Value {
    let mut v = save_variables(media_id, change, current);
    if let (Some(n), Some(o)) = (change.volumes, v.as_object_mut()) {
        o.insert("progressVolumes".into(), json!(n));
    }
    v
}

impl<T: Transport> Client<T> {
    /// The whole manga list of user `user` (500 entries a chunk).
    pub fn fetch_manga_list(
        &mut self,
        access: &str,
        user: u32,
    ) -> Result<Vec<ListItem>, ServiceError> {
        let q = list_query();
        let mut all: Vec<ListItem> = Vec::new();
        // A real list is a few chunks; the cap only stops a loop.
        for chunk in 1..=40 {
            let (items, more) =
                parse_list(self.query(access, &q, json!({ "user": user, "chunk": chunk }))?)?;
            for it in items {
                if !all.iter().any(|a| a.media.id == it.media.id) {
                    all.push(it);
                }
            }
            if !more {
                return Ok(all);
            }
        }
        Err(ServiceError::Other(
            "the manga list has too many chunks".into(),
        ))
    }

    fn raw_manga_entry(
        &mut self,
        access: &str,
        media_id: u32,
    ) -> Result<Option<RawEntry>, ServiceError> {
        match self.query(access, &entry_query(), json!({ "id": media_id })) {
            Err(CallError::NotFound) => Ok(None),
            r => parse_entry(r?),
        }
    }

    /// The user's entry for one manga; `None` when it is not on the list.
    pub fn fetch_manga_entry(
        &mut self,
        access: &str,
        media_id: u32,
    ) -> Result<Option<Remote>, ServiceError> {
        Ok(self
            .raw_manga_entry(access, media_id)?
            .and_then(|e| e.remote()))
    }

    /// Sends one manga change (`SaveMediaListEntry`, which also adds a manga not on the list)
    /// and returns the entry AniList saved.
    pub fn save_manga_entry(
        &mut self,
        access: &str,
        media_id: u32,
        change: &Change,
    ) -> Result<Remote, ServiceError> {
        if change.is_empty() {
            return Err(ServiceError::Other("nothing to send".into()));
        }
        // Ending a reread with no status picked: only AniList's status says what it is now.
        let current = if change.rewatching == Some(false) && change.status.is_none() {
            self.raw_manga_entry(access, media_id)?
                .and_then(|e| e.values())
        } else {
            None
        };
        let vars = save_manga_variables(media_id, change, current.as_ref());
        if vars.as_object().is_some_and(|o| o.len() == 1) {
            // Only the reread flag, already off there: nothing to send, the entry as it is.
            return self.fetch_manga_entry(access, media_id)?.ok_or_else(|| {
                ServiceError::Other("that manga is not on your AniList list".into())
            });
        }
        let data = self.query(access, &save_query(), vars)?;
        #[derive(Deserialize)]
        struct Data {
            #[serde(rename = "SaveMediaListEntry")]
            saved: Option<RawEntry>,
        }
        let d: Data = read(data, "answer")?;
        d.saved
            .and_then(|e| e.remote())
            .ok_or_else(|| ServiceError::Other("AniList's answer has no list entry".into()))
    }

    /// Removes one manga from the list (the entry is read first for its own id). Not on the
    /// list: nothing to do.
    pub fn delete_manga_entry(&mut self, access: &str, media_id: u32) -> Result<(), ServiceError> {
        let Some(e) = self.raw_manga_entry(access, media_id)? else {
            return Ok(());
        };
        let data = self.query(access, super::DELETE, json!({ "id": e.id }))?;
        let deleted = data
            .pointer("/DeleteMediaListEntry/deleted")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if deleted {
            Ok(())
        } else {
            Err(ServiceError::Other(
                "AniList didn't remove the entry".into(),
            ))
        }
    }

    /// The search bar's manga search (`limit` up to 25). Under 3 characters nothing is asked.
    pub fn browse_manga(
        &mut self,
        access: &str,
        query: &str,
        limit: u32,
    ) -> Result<Vec<AnimeHit>, ServiceError> {
        let q: String = query.trim().chars().take(64).collect();
        if q.chars().count() < 3 {
            return Ok(Vec::new());
        }
        parse_hits(self.query(
            access,
            &search_query(),
            json!({ "q": q, "n": limit.clamp(1, 25) }),
        )?)
    }

    /// The AniList manga of these MyAnimeList manga ids, 50 ids a request (the first AniList
    /// manga of an id is kept). Ids AniList doesn't know are left out.
    pub fn manga_by_mal(&mut self, access: &str, ids: &[u32]) -> Result<Vec<Media>, ServiceError> {
        let mut out: Vec<Media> = Vec::new();
        for chunk in ids.chunks(super::MAL_IDS_PER_REQUEST) {
            for page in 1..=3 {
                let (media, more) = parse_page(self.query(
                    access,
                    &by_mal_query(),
                    json!({ "ids": chunk, "page": page }),
                )?)?;
                for m in media.iter().filter(|m| m.id != 0) {
                    let m = m.media();
                    if m.id_mal.is_some_and(|id| chunk.contains(&id))
                        && !out.iter().any(|o| o.id_mal == m.id_mal)
                    {
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

    /// One manga's information page, with the user's entry.
    pub fn manga_details(&mut self, access: &str, media_id: u32) -> Result<Details, ServiceError> {
        parse_details(self.query(access, &details_query(), json!({ "id": media_id }))?)
    }
}

impl<T: Transport> AniList<T> {
    pub(super) fn manga_by_mal_ids(&mut self, ids: &[u32]) -> Result<Vec<Media>, ServiceError> {
        let access = self.access()?;
        self.client.manga_by_mal(&access, ids)
    }

    pub(super) fn manga_list(&mut self) -> Result<Vec<ListItem>, ServiceError> {
        // A fresh Viewer when the last one is old (the manga list follows the anime sync, which
        // has just read it: no extra request then).
        let (id, _) = self.viewer()?;
        let access = self.access()?;
        self.client.fetch_manga_list(&access, id)
    }
}
