//! MyAnimeList manga: the user's manga list (paged), one entry (read, `PATCH`), search and the
//! information page. The same client, token, rate budget and error mapping as anime; only the
//! URLs and the field names differ (`num_chapters_read`, `num_volumes_read`, `is_rereading`,
//! `num_times_reread`, `reading`, `plan_to_read`).
//!
//! Kotora keeps one shape for both kinds: chapters go in `progress` / `episodes`, the reread in
//! `rewatching` / `times_rewatched`, the statuses under their anime names (`Status`).

use serde::Deserialize;

use super::{API, Alt, CallError, ClassifyName, Client, Mal, Picture, Request, Transport, form, non_empty, pct_encode};
use crate::service::{AnimeHit, Author, Details, MyEntry, ServiceError};
use crate::{Change, Kind, ListItem, Media, Remote, Status, Values, time};

/// What the list asks for each manga. The braces are percent-encoded (`list_status{…}`).
const LIST_FIELDS: &str = "list_status%7Bnum_times_reread%7D,num_chapters,num_volumes,media_type,status,alternative_titles,\
authors%7Bfirst_name,last_name%7D";
/// What a search result row shows.
const BROWSE_FIELDS: &str = "alternative_titles,media_type,num_chapters,num_volumes,status,start_date,mean,authors%7Bfirst_name,last_name%7D";
/// The information page.
const DETAIL_FIELDS: &str = "alternative_titles,media_type,num_chapters,num_volumes,status,start_date,end_date,mean,rank,popularity,\
num_list_users,synopsis,genres,authors%7Bfirst_name,last_name%7D,serialization%7Bname%7D,my_list_status%7Bnum_times_reread%7D";

impl<T: Transport> Client<T> {
    /// The whole manga list, 100 entries a page, following `paging.next`
    /// (`GET /users/@me/mangalist`).
    pub fn fetch_manga_list(&mut self, access: &str) -> Result<Vec<ListItem>, ServiceError> {
        let mut url = format!("{API}/users/@me/mangalist?fields={LIST_FIELDS}&limit=100&nsfw=true");
        let mut items = Vec::new();
        for _ in 0..200 {
            let r = self.get(url, access)?;
            let (mut page, next) = parse_manga_list_page(&r.body).map_err(ServiceError::Other)?;
            items.append(&mut page);
            match next {
                Some(n) => url = n,
                None => return Ok(items),
            }
        }
        Err(ServiceError::Other("the manga list has too many pages".into()))
    }

    /// The user's entry for one manga (`GET /manga/{id}?fields=my_list_status{num_times_reread}`);
    /// `None` when it is not on the list.
    pub fn fetch_manga_entry(&mut self, access: &str, id: u32) -> Result<Option<Remote>, ServiceError> {
        let r = self.get(format!("{API}/manga/{id}?fields=my_list_status%7Bnum_times_reread%7D"), access)?;
        #[derive(Deserialize)]
        struct One {
            my_list_status: Option<MangaStatus>,
        }
        let one: One = serde_json::from_str(&r.body).map_err(|e| ServiceError::Other(format!("can't read the entry answer ({})", e.classify_name())))?;
        Ok(one.my_list_status.and_then(MangaStatus::remote))
    }

    /// Sends one change (`PATCH /manga/{id}/my_list_status`, form-encoded) and returns the entry
    /// MAL saved. Only the changed fields go; a score of 0 removes the score.
    pub fn patch_manga_entry(&mut self, access: &str, id: u32, change: &Change) -> Result<Remote, ServiceError> {
        let mut pairs: Vec<(&str, String)> = Vec::new();
        if let Some(st) = change.status {
            pairs.push(("status", st.mal_name_in(Kind::Manga).into()));
        }
        if let Some(p) = change.progress {
            pairs.push(("num_chapters_read", p.to_string()));
        }
        if let Some(v) = change.volumes {
            pairs.push(("num_volumes_read", v.to_string()));
        }
        if let Some(sc) = change.score {
            pairs.push(("score", sc.map_or(0, crate::Score::to_ten).to_string()));
        }
        if let Some(r) = change.rewatching {
            pairs.push(("is_rereading", r.to_string()));
        }
        if let Some(n) = change.times_rewatched {
            pairs.push(("num_times_reread", n.to_string()));
        }
        if pairs.is_empty() {
            return Err(ServiceError::Other("nothing to send".into()));
        }
        let body = form(&pairs.iter().map(|(k, v)| (*k, v.as_str())).collect::<Vec<_>>());
        let req = Request { method: "PATCH", url: format!("{API}/manga/{id}/my_list_status"), bearer: Some(access.to_string()), form: Some(body), json: None };
        let r = self.call(&req)?;
        let ms: MangaStatus = serde_json::from_str(&r.body).map_err(|e| ServiceError::Other(format!("can't read MyAnimeList's answer ({})", e.classify_name())))?;
        ms.remote().ok_or_else(|| ServiceError::Other("MyAnimeList's answer has no list status".into()))
    }

    /// Removes one manga from the list (`DELETE /manga/{id}/my_list_status`). MAL answers 404
    /// when it isn't on the list: that is done too.
    pub fn delete_manga_entry(&mut self, access: &str, id: u32) -> Result<(), ServiceError> {
        let req = Request { method: "DELETE", url: format!("{API}/manga/{id}/my_list_status"), bearer: Some(access.to_string()), form: None, json: None };
        match self.call(&req) {
            Ok(_) | Err(CallError::NotFound) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// The search bar's manga search, `GET /manga?q=` (`limit` up to 20). Under 3 characters
    /// nothing is asked.
    pub fn browse_manga(&mut self, access: &str, query: &str, limit: u32) -> Result<Vec<AnimeHit>, ServiceError> {
        let q: String = query.trim().chars().take(64).collect();
        if q.chars().count() < 3 {
            return Ok(Vec::new());
        }
        let r = self.get(format!("{API}/manga?q={}&limit={}&fields={BROWSE_FIELDS}", pct_encode(&q), limit.clamp(1, 20)), access)?;
        parse_manga_hits(&r.body).map_err(ServiceError::Other)
    }

    /// One manga's information page (`GET /manga/{id}?fields=…`), with the user's entry.
    pub fn fetch_manga_details(&mut self, access: &str, id: u32) -> Result<Details, ServiceError> {
        let r = self.get(format!("{API}/manga/{id}?fields={DETAIL_FIELDS}"), access)?;
        parse_manga_details(&r.body).map_err(ServiceError::Other)
    }
}

impl<T: Transport> Mal<T> {
    pub(super) fn manga_list(&mut self) -> Result<Vec<ListItem>, ServiceError> {
        self.authed(|c, a| c.fetch_manga_list(a))
    }
    pub(super) fn manga_entry(&mut self, id: u32) -> Result<Option<Remote>, ServiceError> {
        self.authed(|c, a| c.fetch_manga_entry(a, id))
    }
    pub(super) fn manga_patch(&mut self, id: u32, change: &Change) -> Result<Remote, ServiceError> {
        self.authed(|c, a| c.patch_manga_entry(a, id, change))
    }
    pub(super) fn manga_delete(&mut self, id: u32) -> Result<(), ServiceError> {
        self.authed(|c, a| c.delete_manga_entry(a, id))
    }
    pub(super) fn manga_browse(&mut self, query: &str, limit: u32) -> Result<Vec<AnimeHit>, ServiceError> {
        self.authed(|c, a| c.browse_manga(a, query, limit))
    }
    pub(super) fn manga_page(&mut self, id: u32) -> Result<Details, ServiceError> {
        self.authed(|c, a| c.fetch_manga_details(a, id))
    }
}

// ----------------------------------------------------------------------------------- JSON shapes

#[derive(Deserialize, Default)]
#[serde(default)]
struct MangaNode {
    id: u32,
    title: String,
    main_picture: Option<Picture>,
    alternative_titles: Option<Alt>,
    num_chapters: Option<u32>,
    num_volumes: Option<u32>,
    media_type: Option<String>,
    status: Option<String>,
    authors: Vec<RawAuthor>,
    start_date: Option<String>,
    end_date: Option<String>,
    mean: Option<f32>,
    rank: Option<u32>,
    popularity: Option<u32>,
    num_list_users: Option<u32>,
    synopsis: Option<String>,
    genres: Vec<Named>,
    serialization: Vec<RawSerial>,
    my_list_status: Option<MangaStatus>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawAuthor {
    node: Person,
    role: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Person {
    first_name: String,
    last_name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSerial {
    node: Named,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Named {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MangaStatus {
    status: Option<String>,
    score: u8,
    num_chapters_read: u32,
    num_volumes_read: u32,
    is_rereading: bool,
    num_times_reread: u32,
    start_date: Option<String>,
    finish_date: Option<String>,
    updated_at: Option<String>,
}

impl MangaStatus {
    fn status(&self) -> Option<Status> {
        self.status.as_deref().and_then(|s| Status::from_mal_in(Kind::Manga, s))
    }

    fn values(&self) -> Option<Values> {
        Some(Values {
            status: self.status()?,
            progress: self.num_chapters_read,
            score: crate::Score::from_ten(self.score.into()),
            rewatching: self.is_rereading,
            times_rewatched: self.num_times_reread,
            volumes: self.num_volumes_read,
        })
    }

    fn updated_at(&self) -> i64 {
        self.updated_at.as_deref().and_then(time::parse_rfc3339).unwrap_or(0)
    }

    fn remote(self) -> Option<Remote> {
        Some(Remote { values: self.values()?, updated_at: self.updated_at() })
    }
}

/// "First Last"; MAL may leave either empty.
fn author_name(p: &Person) -> String {
    [p.first_name.trim(), p.last_name.trim()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ")
}

impl MangaNode {
    fn alt(&self) -> (Option<String>, Option<String>, Vec<String>) {
        match &self.alternative_titles {
            Some(a) => (non_empty(a.en.clone()), non_empty(a.ja.clone()), a.synonyms.iter().filter(|s| !s.trim().is_empty()).cloned().collect()),
            None => (None, None, Vec::new()),
        }
    }

    fn authors(&self) -> Vec<Author> {
        self.authors.iter().map(|a| Author { name: author_name(&a.node), role: a.role.trim().to_string() }).filter(|a| !a.name.is_empty()).collect()
    }

    /// MAL says 0 when a count is unknown (still publishing).
    fn chapters(&self) -> Option<u32> {
        self.num_chapters.filter(|&n| n > 0)
    }

    fn volumes(&self) -> Option<u32> {
        self.num_volumes.filter(|&n| n > 0)
    }

    fn media(self) -> Media {
        let (title_en, title_ja, synonyms) = self.alt();
        Media {
            kind: Kind::Manga,
            id: self.id,
            episodes: self.chapters(),
            volumes: self.volumes(),
            authors: self.authors().into_iter().map(|a| a.name).collect(),
            title: self.title,
            title_en,
            title_ja,
            synonyms,
            format: self.media_type.unwrap_or_default(),
            air_status: self.status.unwrap_or_default(),
            cover_url: self.main_picture.and_then(|p| p.medium.or(p.large)),
            start_date: non_empty(self.start_date),
            broadcast: None,
            next_airing: None,
            id_mal: None,
        }
    }
}

/// One page of `mangalist`: the entries and the `paging.next` link. Entries without a list
/// status (or with one Kotora doesn't know) are skipped.
pub fn parse_manga_list_page(body: &str) -> Result<(Vec<ListItem>, Option<String>), String> {
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        data: Vec<Raw>,
        paging: Option<super::Paging>,
    }
    #[derive(Deserialize)]
    struct Raw {
        node: MangaNode,
        list_status: Option<MangaStatus>,
    }
    let page: Page = serde_json::from_str(body).map_err(|e| format!("can't read the manga list answer ({})", e.classify_name()))?;
    let mut out = Vec::new();
    for raw in page.data {
        let Some(ls) = raw.list_status else { continue };
        let Some(v) = ls.values() else { continue };
        out.push(ListItem {
            media: raw.node.media(),
            status: v.status,
            progress: v.progress,
            score: v.score,
            rewatching: v.rewatching,
            times_rewatched: v.times_rewatched,
            volumes: v.volumes,
            updated_at: ls.updated_at(),
        });
    }
    Ok((out, page.paging.and_then(|p| p.next).filter(|n| !n.is_empty())))
}

/// The answer of `GET /manga?q=…&fields=…`.
pub fn parse_manga_hits(body: &str) -> Result<Vec<AnimeHit>, String> {
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        data: Vec<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        node: MangaNode,
    }
    let page: Page = serde_json::from_str(body).map_err(|e| format!("can't read the search answer ({})", e.classify_name()))?;
    Ok(page
        .data
        .into_iter()
        .map(|i| {
            let mean = i.node.mean.filter(|m| *m > 0.0);
            let m = i.node.media();
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
                mean,
                cover_url: m.cover_url,
                volumes: m.volumes,
                authors: m.authors,
                start_date: m.start_date,
            }
        })
        .collect())
}

/// The answer of `GET /manga/{id}?fields=…`.
pub fn parse_manga_details(body: &str) -> Result<Details, String> {
    let n: MangaNode = serde_json::from_str(body).map_err(|e| format!("can't read the manga's information ({})", e.classify_name()))?;
    if n.id == 0 || n.title.is_empty() {
        return Err("MyAnimeList sent no manga".into());
    }
    let (title_en, title_ja, synonyms) = n.alt();
    let authors = n.authors();
    let (episodes, volumes) = (n.chapters(), n.volumes());
    let mine = n.my_list_status.as_ref().and_then(|m| {
        let v = m.values()?;
        Some(MyEntry {
            status: v.status.mal_name().into(),
            progress: v.progress,
            score: v.score,
            rewatching: v.rewatching,
            times_rewatched: v.times_rewatched,
            volumes: v.volumes,
            start_date: non_empty(m.start_date.clone()),
            finish_date: non_empty(m.finish_date.clone()),
        })
    });
    let (cover_url, cover_small) = match n.main_picture {
        Some(p) => (p.large.clone().or(p.medium.clone()), p.medium.or(p.large)),
        None => (None, None),
    };
    Ok(Details {
        kind: Kind::Manga,
        id: n.id,
        title: n.title,
        title_en,
        title_ja,
        synonyms,
        format: n.media_type.unwrap_or_default(),
        episodes,
        volumes,
        air_status: n.status.unwrap_or_default(),
        start_date: non_empty(n.start_date),
        end_date: non_empty(n.end_date),
        genres: n.genres.into_iter().map(|g| g.name).filter(|g| !g.is_empty()).collect(),
        mean: n.mean.filter(|m| *m > 0.0),
        rank: n.rank.filter(|&r| r > 0),
        popularity: n.popularity.filter(|&r| r > 0),
        members: n.num_list_users,
        synopsis: n.synopsis.unwrap_or_default().trim().to_string(),
        cover_url,
        cover_small,
        mine,
        authors,
        serialization: n.serialization.into_iter().map(|s| s.node.name).filter(|s| !s.is_empty()).collect(),
        ..Details::default()
    })
}

/// A manga's `media_type` as the UI shows it: "Manga", "Light novel", "Manhwa", "One-shot", …
pub fn manga_type_label(media_type: &str) -> String {
    match media_type {
        "manga" => "Manga".into(),
        "light_novel" => "Light novel".into(),
        "novel" => "Novel".into(),
        "one_shot" => "One-shot".into(),
        "doujinshi" => "Doujinshi".into(),
        "manhwa" => "Manhwa".into(),
        "manhua" => "Manhua".into(),
        "oel" => "OEL".into(),
        "" | "unknown" => "Unknown".into(),
        other => {
            let t = other.replace('_', " ");
            match t.get(..1) {
                Some(f) => f.to_uppercase() + &t[1..],
                None => t,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::score::ten;
    use super::super::tests::{client, fake, ok, status, tmp_tokens, token, tok_json};
    use super::*;
    use crate::service::ListService;

    const PAGE1: &str = include_str!("../../tests/mal_mangalist_page1.json");
    const PAGE2: &str = include_str!("../../tests/mal_mangalist_page2.json");
    const ENTRY: &str = include_str!("../../tests/mal_manga_entry.json");
    const PATCHED: &str = include_str!("../../tests/mal_manga_patch_answer.json");
    const HITS: &str = include_str!("../../tests/mal_manga_search.json");
    const DETAILS: &str = include_str!("../../tests/mal_manga_details.json");
    const NOT_ON_LIST: &str = r#"{"id":2005,"title":"Not on the list"}"#;

    #[test]
    fn the_list_pages_and_reads_chapters_volumes_and_authors() {
        let (f, seen) = fake(vec![ok(PAGE1), ok(PAGE2)]);
        let (mut c, _) = client(f);
        let items = c.fetch_manga_list("acc").unwrap();
        assert_eq!(items.iter().map(|i| i.media.id).collect::<Vec<_>>(), [2001, 2002, 2003], "no list status: skipped");
        let a = &items[0];
        assert_eq!((a.media.kind, a.status, a.progress, a.volumes, a.score), (Kind::Manga, Status::Watching, 21, 3, ten(8)));
        assert_eq!((a.media.episodes, a.media.volumes), (None, None), "0 from MAL: unknown, still publishing");
        assert_eq!(a.media.authors, ["Aoi Hoshimiya"]);
        assert_eq!((a.media.format.as_str(), a.media.air_status.as_str()), ("manga", "currently_publishing"));
        assert!(a.media.is_publishing() && !a.media.is_novel());
        assert_eq!(a.media.title_en.as_deref(), Some("Paper Lanterns"));
        assert_eq!(a.media.title_ja, None, "empty: none");
        assert_eq!(a.updated_at, 1_793_523_600);
        let b = &items[1];
        assert_eq!((b.status, b.rewatching, b.times_rewatched, b.score), (Status::Completed, true, 1, None));
        assert_eq!((b.media.episodes, b.media.volumes), (Some(48), Some(6)));
        assert!(b.media.is_novel());
        assert_eq!(b.media.authors, ["Ren Tsukishiro", "Mokuren"]);
        assert_eq!(items[2].status, Status::PlanToWatch, "plan_to_read");
        let seen = seen.lock().unwrap();
        assert_eq!(
            seen[0].1,
            "https://api.myanimelist.net/v2/users/@me/mangalist?fields=list_status%7Bnum_times_reread%7D,num_chapters,num_volumes,media_type,status,\
alternative_titles,authors%7Bfirst_name,last_name%7D&limit=100&nsfw=true"
        );
        assert_eq!(seen[1].1, "https://api.myanimelist.net/v2/users/@me/mangalist?offset=100&limit=100");
        assert!(seen.iter().all(|s| s.2.as_deref() == Some("acc")));
    }

    #[test]
    fn a_foreign_next_link_is_refused() {
        let body = r#"{"data":[],"paging":{"next":"https://evil.example.invalid/steal"}}"#;
        let (f, seen) = fake(vec![ok(body)]);
        let (mut c, _) = client(f);
        assert!(matches!(c.fetch_manga_list("acc"), Err(ServiceError::Other(_))));
        assert_eq!(seen.lock().unwrap().len(), 1, "the bearer never leaves MAL");
    }

    #[test]
    fn errors_map_like_anime() {
        let (f, _) = fake(vec![status(429, Some(1)), status(429, Some(1)), status(429, Some(1)), status(429, Some(1))]);
        assert_eq!(client(f).0.fetch_manga_list("a"), Err(ServiceError::RateLimited));
        let (f, _) = fake(vec![Err("no route".into())]);
        assert!(matches!(client(f).0.fetch_manga_entry("a", 1), Err(ServiceError::Offline(_))));
        let (f, _) = fake(vec![status(403, None)]);
        assert!(matches!(client(f).0.browse_manga("a", "lanterns", 5), Err(ServiceError::Other(m)) if m.contains("403")));
    }

    #[test]
    fn reads_one_entry_or_none() {
        let (f, seen) = fake(vec![ok(ENTRY), ok(NOT_ON_LIST)]);
        let (mut c, _) = client(f);
        let r = c.fetch_manga_entry("acc", 2001).unwrap().unwrap();
        assert_eq!(r.values, Values { volumes: 3, ..Values::new(Status::Watching, 21, ten(8)) });
        assert_eq!(c.fetch_manga_entry("acc", 2005).unwrap(), None);
        let seen = seen.lock().unwrap();
        assert_eq!((seen[0].0.as_str(), seen[0].1.as_str()), ("GET", "https://api.myanimelist.net/v2/manga/2001?fields=my_list_status%7Bnum_times_reread%7D"));
    }

    #[test]
    fn patch_sends_mangas_field_names() {
        let (f, seen) = fake(vec![ok(PATCHED), ok(PATCHED), ok(PATCHED)]);
        let (mut c, _) = client(f);
        let r = c.patch_manga_entry("acc", 2001, &Change { progress: Some(22), volumes: Some(4), ..Default::default() }).unwrap();
        assert_eq!((r.values.progress, r.values.volumes, r.values.status), (22, 4, Status::Watching));
        assert_eq!(r.updated_at, 1_794_823_200);
        c.patch_manga_entry("acc", 2001, &Change { status: Some(Status::PlanToWatch), score: Some(None), ..Default::default() }).unwrap();
        c.patch_manga_entry("acc", 2001, &Change { rewatching: Some(true), times_rewatched: Some(2), status: Some(Status::Watching), ..Default::default() })
            .unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!((seen[0].0.as_str(), seen[0].1.as_str()), ("PATCH", "https://api.myanimelist.net/v2/manga/2001/my_list_status"));
        assert_eq!(seen[0].3.as_deref(), Some("num_chapters_read=22&num_volumes_read=4"));
        assert_eq!(seen[1].3.as_deref(), Some("status=plan_to_read&score=0"));
        assert_eq!(seen[2].3.as_deref(), Some("status=reading&is_rereading=true&num_times_reread=2"));
        assert!(c.patch_manga_entry("acc", 1, &Change::default()).is_err(), "an empty change is never sent");
    }

    #[test]
    fn search_reads_rows_and_skips_short_queries() {
        let (f, seen) = fake(vec![ok(HITS)]);
        let (mut c, _) = client(f);
        assert!(c.browse_manga("acc", " pa ", 10).unwrap().is_empty());
        let hits = c.browse_manga("acc", "paper lanterns", 50).unwrap();
        assert_eq!(hits.len(), 2);
        let h = &hits[0];
        assert_eq!((h.kind, h.id, h.mean, h.episodes, h.volumes), (Kind::Manga, 2001, Some(7.91), None, None));
        assert_eq!((h.title_ja.as_deref(), h.start_date.as_deref()), (Some("霧ヶ丘の紙灯籠"), Some("2021-04-08")));
        assert_eq!(h.authors, ["Aoi Hoshimiya"]);
        let m = h.media();
        assert_eq!((m.kind, m.authors.len(), m.start_date.as_deref()), (Kind::Manga, 1, Some("2021-04-08")));
        assert_eq!((hits[1].format.as_str(), hits[1].mean, hits[1].episodes), ("one_shot", None, Some(1)));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "the short query asked nothing");
        assert_eq!(
            seen[0].1,
            format!("https://api.myanimelist.net/v2/manga?q=paper%20lanterns&limit=20&fields={BROWSE_FIELDS}"),
            "limit clamped to 20"
        );
    }

    #[test]
    fn details_read_authors_serialization_and_the_users_entry() {
        let (f, seen) = fake(vec![ok(DETAILS)]);
        let (mut c, _) = client(f);
        let d = c.fetch_manga_details("acc", 2002).unwrap();
        assert_eq!((d.kind, d.id, d.format.as_str(), d.episodes, d.volumes), (Kind::Manga, 2002, "light_novel", Some(48), Some(6)));
        assert_eq!(d.authors, [Author { name: "Ren Tsukishiro".into(), role: "Story".into() }, Author { name: "Mokuren".into(), role: "Art".into() }]);
        assert_eq!(d.serialization, ["Made-up Bunko"]);
        assert_eq!(d.genres, ["Drama", "Mystery"]);
        assert_eq!((d.start_date.as_deref(), d.end_date.as_deref()), (Some("2018-03"), Some("2022-12-20")));
        assert_eq!((d.mean, d.rank, d.popularity, d.members), (Some(8.12), Some(310), Some(2200), Some(41000)));
        assert!(d.synopsis.starts_with("A made-up story") && !d.synopsis.ends_with(' '));
        assert_eq!(d.cover_url.as_deref(), Some("https://cdn.example.invalid/m2002l.jpg"));
        let mine = d.mine.clone().unwrap();
        assert_eq!((mine.status(), mine.progress, mine.volumes, mine.score, mine.rewatching, mine.times_rewatched), (Some(Status::Completed), 15, 2, ten(9), true, 1));
        assert_eq!(mine.finish_date.as_deref(), Some("2023-02"));
        let m = d.media();
        assert_eq!((m.kind, m.volumes, m.authors.clone()), (Kind::Manga, Some(6), vec!["Ren Tsukishiro".to_string(), "Mokuren".to_string()]));
        assert_eq!(seen.lock().unwrap()[0].1, format!("https://api.myanimelist.net/v2/manga/2002?fields={DETAIL_FIELDS}"));
        // Saved details round-trip; older saved details (no kind) are anime.
        let back: Details = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back, d);
        let old: Details = serde_json::from_str(r#"{"id":1,"title":"Old","mine":{"status":"watching","progress":3,"score":null,"rewatching":false,"start_date":null,"finish_date":null}}"#).unwrap();
        assert_eq!((old.kind, old.mine.unwrap().volumes), (Kind::Anime, 0));
        assert!(parse_manga_details("{}").is_err());
    }

    #[test]
    fn type_labels() {
        assert_eq!(manga_type_label("light_novel"), "Light novel");
        assert_eq!(manga_type_label("one_shot"), "One-shot");
        assert_eq!(manga_type_label("manhwa"), "Manhwa");
        assert_eq!(manga_type_label("web_comic"), "Web comic");
        assert_eq!(manga_type_label(""), "Unknown");
    }

    /// Through the signed-in service: the token is used, a 401 refreshes once, and the kind
    /// picks the manga calls.
    #[test]
    fn the_service_routes_by_kind_and_refreshes_on_401() {
        let (f, seen) = fake(vec![status(401, None), ok(&tok_json("new")), ok(PATCHED), ok(ENTRY), ok(PAGE2)]);
        let (c, _) = client(f);
        let mut mal = Mal::new(c, tmp_tokens("manga401", Some(token("old", 3600))));
        let r = mal.update_entry_of(Kind::Manga, 2001, &Change { progress: Some(22), ..Default::default() }).unwrap();
        assert_eq!(r.values.progress, 22);
        assert!(mal.get_entry_of(Kind::Manga, 2001).unwrap().is_some());
        assert_eq!(mal.fetch_list_of(Kind::Manga).unwrap().len(), 1);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 5);
        assert_eq!((seen[0].2.as_deref(), seen[2].2.as_deref()), (Some("old"), Some("new")));
        assert!(seen[2].1.ends_with("/manga/2001/my_list_status") && seen[3].1.contains("/manga/2001?"));
    }
}
