//! The details page's service information, as the desktop's (`app/src/details.rs`): the saved
//! copy at once (fresh for [`DETAILS_FRESH_SECS`]), then the service's answer, saved for the next
//! time; the words of the tags, the stats row and the Information card come from here so both
//! apps read alike. Also the add of a title that is not on the list (from the details page, the
//! Airing page, a notification) and a pasted MyAnimeList/AniList link or id.

use kotora_core::service::{Details, ServiceId};
use kotora_core::{Change, Kind, Media, Scope, time};

use super::*;

/// The desktop's `details::FRESH_SECS`: a saved copy younger than this isn't asked again.
pub const DETAILS_FRESH_SECS: i64 = 3 * 86_400;

/// One line of the Information card: "Aired" · "Oct 3, 2025 to Dec 19, 2025".
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct InfoFact {
    pub label: String,
    pub value: String,
}

/// One cell of the stats row: "8.31" over "MAL" (with a star), "#1,204" over "ranked".
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct InfoStat {
    pub value: String,
    pub label: String,
    pub star: bool,
}

/// The service's information about an anime or a manga, in the words the page shows.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ServiceDetails {
    pub kind: MediaKind,
    pub id: u32,
    pub service: Service,
    pub title: String,
    pub title_en: Option<String>,
    pub title_ja: Option<String>,
    /// The English and Japanese titles that differ from `title`.
    pub other_titles: Vec<String>,
    /// "Also: Kirigaoka, Lanterns" or `None`.
    pub also: Option<String>,
    pub format: String,
    /// "TV", "24 episodes", "Airing", "Fall 2026" (manga: "Manga", "120 chapters", "12 volumes",
    /// "Publishing").
    pub tags: Vec<String>,
    /// Airing or publishing now: the run tag is lit.
    pub running: bool,
    pub episodes: Option<u32>,
    pub volumes: Option<u32>,
    pub air_status: String,
    pub mean: Option<f32>,
    pub stats: Vec<InfoStat>,
    /// The synopsis, or "No synopsis yet.".
    pub synopsis: String,
    /// Aired, Studios, Source, Length (manga: Published, Authors, Serialization, Chapters,
    /// Volumes). Genres are apart, as chips.
    pub facts: Vec<InfoFact>,
    pub genres: Vec<String>,
    pub cover_url: Option<String>,
    pub cover_small: Option<String>,
    /// When the copy was fetched (Unix seconds).
    pub fetched_at: i64,
    /// Younger than [`DETAILS_FRESH_SECS`]: no need to ask the service again.
    pub fresh: bool,
    /// On the user's list (of the active service).
    pub on_list: bool,
}

/// What the app knows of a title before its details come: enough for an add.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct TitleStub {
    pub kind: MediaKind,
    pub id: u32,
    pub title: String,
    pub cover_url: Option<String>,
    /// The service's raw format (`tv`, `manga`, …), or empty.
    pub format: String,
    pub episodes: Option<u32>,
}

/// An id in the search box: a pasted link of the active service (`link`), or a bare number.
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PastedId {
    pub id: u32,
    pub link: bool,
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `2025-10-03` → "Oct 3, 2025"; `2025-10` → "Oct 2025"; `2025` stays.
pub fn date_text(d: &str) -> String {
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

/// 98765 → "98,765".
pub fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn anime_type(format: &str) -> &'static str {
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

fn air_label(air: &str) -> &'static str {
    match air {
        "currently_airing" => "Airing",
        "finished_airing" => "Finished airing",
        "not_yet_aired" => "Not yet aired",
        _ => "",
    }
}

fn run_label(air: &str) -> &'static str {
    match air {
        "currently_publishing" => "Publishing",
        "finished" => "Finished",
        "on_hiatus" => "On hiatus",
        "discontinued" => "Discontinued",
        "not_yet_published" => "Not yet published",
        _ => "",
    }
}

/// `light_novel` → "Light novel".
fn source_label(s: &str) -> String {
    let t = s.replace('_', " ");
    let mut c = t.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().collect::<String>() + c.as_str())
}

fn plural(n: u32, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

/// The page's words for `d`, as the desktop's `tags`, `stats`, `other_titles` and `body_cards`.
pub fn service_details(d: &Details, service: Service, fetched_at: i64, now: i64, on_list: bool) -> ServiceDetails {
    let manga = d.kind == Kind::Manga;
    let mut tags = Vec::new();
    let running = if manga {
        let t = kotora_core::mal::manga_type_label(&d.format);
        if !d.format.is_empty() && t != "Unknown" {
            tags.push(t);
        }
        if let Some(n) = d.episodes.filter(|n| *n > 0) {
            tags.push(plural(n, "chapter"));
        }
        if let Some(n) = d.volumes.filter(|n| *n > 0) {
            tags.push(plural(n, "volume"));
        }
        if !run_label(&d.air_status).is_empty() {
            tags.push(run_label(&d.air_status).into());
        }
        d.air_status == "currently_publishing"
    } else {
        if !anime_type(&d.format).is_empty() {
            tags.push(anime_type(&d.format).into());
        }
        tags.push(d.episodes.map_or("? episodes".into(), |n| plural(n, "episode")));
        if !air_label(&d.air_status).is_empty() {
            tags.push(air_label(&d.air_status).into());
        }
        if let Some(se) = d.season_text() {
            tags.push(se);
        }
        d.air_status == "currently_airing"
    };

    let mut stats = vec![InfoStat {
        value: d.mean.map_or("\u{2013}".into(), |m| format!("{m:.2}")),
        label: match service {
            Service::Mal => "MAL".into(),
            Service::AniList => "AniList".into(),
        },
        star: true,
    }];
    if let Some(r) = d.rank {
        stats.push(InfoStat { value: format!("#{}", thousands(r)), label: "ranked".into(), star: false });
    }
    if let Some(p) = d.popularity {
        stats.push(InfoStat { value: format!("#{}", thousands(p)), label: "popular".into(), star: false });
    }
    if let Some(m) = d.members {
        stats.push(InfoStat { value: thousands(m), label: "members".into(), star: false });
    }

    let fact = |label: &str, value: String| InfoFact { label: label.into(), value };
    let mut facts = Vec::new();
    let since = |live: bool| match (d.start_date.as_deref(), d.end_date.as_deref()) {
        (Some(a), Some(b)) => format!("{} to {}", date_text(a), date_text(b)),
        (Some(a), None) if live => format!("Since {}", date_text(a)),
        (Some(a), None) => date_text(a),
        _ => "Not yet known".into(),
    };
    if manga {
        let live = matches!(d.air_status.as_str(), "currently_publishing" | "on_hiatus");
        facts.push(fact("Published", since(live)));
        if !d.authors.is_empty() {
            let names: Vec<String> = d.authors.iter().map(|a| if a.role.is_empty() { a.name.clone() } else { format!("{} ({})", a.name, a.role) }).collect();
            facts.push(fact("Authors", names.join(", ")));
        }
        if !d.serialization.is_empty() {
            facts.push(fact("Serialization", d.serialization.join(", ")));
        }
        let count = |n: Option<u32>| match n.filter(|n| *n > 0) {
            Some(n) => n.to_string(),
            None if live => "Unknown while publishing".into(),
            None => "Unknown".into(),
        };
        facts.push(fact("Chapters", count(d.episodes)));
        facts.push(fact("Volumes", count(d.volumes)));
    } else {
        facts.push(fact("Aired", since(d.air_status == "currently_airing")));
        if !d.studios.is_empty() {
            facts.push(fact("Studios", d.studios.join(", ")));
        }
        if !d.source.is_empty() {
            facts.push(fact("Source", source_label(&d.source)));
        }
        if let Some(m) = d.minutes {
            facts.push(fact("Length", format!("{m} min per episode")));
        }
    }

    let other_titles = [d.title_en.as_ref(), d.title_ja.as_ref()].into_iter().flatten().filter(|t| **t != d.title && !t.is_empty()).cloned().collect();
    ServiceDetails {
        kind: d.kind.into(),
        id: d.id,
        service,
        title: d.title.clone(),
        title_en: d.title_en.clone(),
        title_ja: d.title_ja.clone(),
        other_titles,
        also: (!d.synonyms.is_empty()).then(|| format!("Also: {}", d.synonyms.join(", "))),
        format: d.format.clone(),
        tags,
        running,
        episodes: d.episodes,
        volumes: d.volumes,
        air_status: d.air_status.clone(),
        mean: d.mean,
        stats,
        synopsis: if d.synopsis.trim().is_empty() { "No synopsis yet.".into() } else { d.synopsis.clone() },
        facts,
        genres: d.genres.clone(),
        cover_url: d.cover_url.clone().or_else(|| d.cover_small.clone()),
        cover_small: d.cover_small.clone(),
        fetched_at,
        fresh: now - fetched_at < DETAILS_FRESH_SECS,
        on_list,
    }
}

/// A MyAnimeList id in the box (the desktop's `search::mal_id_in`): a pasted
/// `myanimelist.net/anime/41590[/slug]` or `anime.php?id=41590` link, or a bare number.
pub fn mal_id_in(q: &str, kind: Kind) -> Option<(u32, bool)> {
    let q = q.trim();
    let num = |t: &str| t.parse::<u32>().ok().filter(|&n| n > 0 && !t.starts_with('+'));
    if !q.is_empty() && q.len() <= 9 && q.bytes().all(|b| b.is_ascii_digit()) {
        return num(q).map(|id| (id, false));
    }
    let rest = q.strip_prefix("https://").or_else(|| q.strip_prefix("http://")).unwrap_or(q);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let path = rest.strip_prefix("myanimelist.net/")?;
    let digits = |t: &str| -> Option<u32> {
        let end = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
        num(&t[..end])
    };
    let name = kind.name();
    if let Some(t) = path.strip_prefix(&format!("{name}/")) {
        let end = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
        return (end > 0 && matches!(t[end..].chars().next(), None | Some('/' | '?' | '#'))).then(|| digits(t)).flatten().map(|id| (id, true));
    }
    let query = path.strip_prefix(&format!("{name}.php?"))?;
    query.split('&').find_map(|kv| kv.strip_prefix("id=")).and_then(digits).map(|id| (id, true))
}

/// An AniList id in the box (the desktop's `search::anilist_id_in`): `anilist.co/anime/170001[/slug]`
/// or a bare number.
pub fn anilist_id_in(q: &str, kind: Kind) -> Option<(u32, bool)> {
    let q = q.trim();
    if let Some(bare) = mal_id_in(q, kind).filter(|(_, link)| !link) {
        return Some(bare);
    }
    let rest = q.strip_prefix("https://").or_else(|| q.strip_prefix("http://")).unwrap_or(q);
    let t = rest.strip_prefix("www.").unwrap_or(rest).strip_prefix(&format!("anilist.co/{}/", kind.name()))?;
    let end = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let ok = end > 0 && matches!(t[end..].chars().next(), None | Some('/' | '?' | '#'));
    t[..end].parse::<u32>().ok().filter(|n| ok && *n > 0).map(|id| (id, true))
}

/// The desktop's `online::error_text`: what the page says when the service didn't answer.
fn details_error(e: ServiceError, service: Service) -> KotoraError {
    let title = service.id().title();
    match e {
        ServiceError::Offline(_) => KotoraError::Offline { message: format!("Can't reach {title}.") },
        ServiceError::SignInAgain => KotoraError::Other { message: format!("{title} needs you to sign in again.") },
        ServiceError::RateLimited => KotoraError::RateLimited,
        ServiceError::Other(m) => KotoraError::Other { message: m },
    }
}

impl Kotora {
    fn on_list(&self, store: &Store, service: Service, kind: Kind, id: u32) -> bool {
        store.load_item(Scope::new(service.name(), kind), id).ok().flatten().is_some()
    }
}

#[uniffi::export]
impl Kotora {
    /// The saved copy of a title's information (the active service's), if any: shown at once,
    /// before [`Kotora::fetch_details`] answers. Local.
    pub fn saved_details(&self, kind: MediaKind, id: u32) -> Option<ServiceDetails> {
        let service = self.active()?;
        let store = self.store();
        let (d, at) = store.details(Scope::new(service.name(), kind.into()), id).ok().flatten()?;
        let on = self.on_list(&store, service, kind.into(), id);
        Some(service_details(&d, service, at, time::now_unix(), on))
    }

    /// The service's information about a title, saved for the next time; the saved list entry's
    /// title, cover and airing status are refreshed with it. Network: call it off the main thread.
    pub fn fetch_details(&self, kind: MediaKind, id: u32) -> Result<ServiceDetails, KotoraError> {
        let Some(service) = self.active() else {
            let svc = self.store().kv_get(ACTIVE_KEY).ok().flatten().and_then(|s| Service::from_name(&s)).unwrap_or(Service::Mal);
            return Err(KotoraError::Offline { message: format!("Offline or signed out: the information comes from {}.", svc.id().title()) });
        };
        let mut svc = self.service(service)?;
        let d = svc.details_of(kind.into(), id).map_err(|e| details_error(e, service))?;
        let now = time::now_unix();
        let store = self.own_store()?;
        let _ = store.save_details(service.name(), &d, now);
        let _ = store.refresh_media(service.name(), &d.media(), now);
        let on = self.on_list(&store, service, kind.into(), id);
        Ok(service_details(&d, service, now, now, on))
    }

    /// "Add as Watching" / "+ Plan" for a title not on the list (details page, Airing, a
    /// notification): saved at once, queued for the service. The saved details fill the entry
    /// when there are some; else the stub, and the queue fills in the rest after the add. An
    /// entry already on the list just gets the status, as on the desktop.
    pub fn add_title(&self, stub: TitleStub, status: ListStatus) -> Result<(), KotoraError> {
        let service = self.active_or_err()?;
        let kind: Kind = stub.kind.into();
        let mut store = self.store();
        let scope = Scope::new(service.name(), kind);
        let now = time::now_unix();
        if store.load_item(scope, stub.id)?.is_some() {
            store.enqueue(scope, stub.id, Change { status: Some(status.into()), ..Default::default() }, now)?;
            return Ok(());
        }
        let media = match store.details(scope, stub.id).ok().flatten() {
            Some((d, _)) => d.media(),
            None => Media { kind, id: stub.id, title: stub.title, cover_url: stub.cover_url, format: stub.format, episodes: stub.episodes, ..Default::default() },
        };
        store.add_to_list(service.name(), &media, status.into(), None, now)?;
        Ok(())
    }

    /// A MyAnimeList or AniList link (of the active service) or a bare id in the search box, for
    /// "Open MyAnimeList anime 41590".
    pub fn pasted_id(&self, kind: MediaKind, text: String) -> Option<PastedId> {
        let service = self.active()?;
        let found = match service.id() {
            ServiceId::Mal => mal_id_in(&text, kind.into()),
            ServiceId::AniList => anilist_id_in(&text, kind.into()),
        };
        found.map(|(id, link)| PastedId { id, link })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(kind: Kind) -> Details {
        Details {
            kind,
            id: 7,
            title: "Kirigaoka no Chouchin".into(),
            title_en: Some("Lanterns of Kirigaoka".into()),
            title_ja: Some("Kirigaoka no Chouchin".into()),
            synonyms: vec!["Lanterns".into(), "KnC".into()],
            format: if kind == Kind::Manga { "manga".into() } else { "tv".into() },
            episodes: Some(24),
            air_status: if kind == Kind::Manga { "currently_publishing".into() } else { "currently_airing".into() },
            start_date: Some("2026-10-03".into()),
            end_date: None,
            season: Some((2026, "fall".into())),
            studios: vec!["Studio Hoshi".into()],
            genres: vec!["Drama".into()],
            source: "light_novel".into(),
            minutes: Some(24),
            mean: Some(8.314),
            rank: Some(1204),
            popularity: None,
            members: Some(98765),
            synopsis: String::new(),
            cover_url: None,
            cover_small: Some("s.jpg".into()),
            mine: None,
            volumes: None,
            authors: vec![kotora_core::service::Author { name: "Aoi Mori".into(), role: "Story".into() }],
            serialization: vec![],
            next_airing: None,
        }
    }

    #[test]
    fn anime_words_read_as_the_desktop() {
        let s = service_details(&sample(Kind::Anime), Service::Mal, 0, 10, false);
        assert_eq!(s.tags, ["TV", "24 episodes", "Airing", "Fall 2026"]);
        assert!(s.running && s.fresh);
        assert_eq!(s.other_titles, ["Lanterns of Kirigaoka"]);
        assert_eq!(s.also.as_deref(), Some("Also: Lanterns, KnC"));
        assert_eq!(s.synopsis, "No synopsis yet.");
        assert_eq!(s.stats[0], InfoStat { value: "8.31".into(), label: "MAL".into(), star: true });
        assert_eq!(s.stats[1].value, "#1,204");
        assert_eq!(s.stats[2].value, "98,765");
        let f: Vec<(&str, &str)> = s.facts.iter().map(|f| (f.label.as_str(), f.value.as_str())).collect();
        assert_eq!(f, [("Aired", "Since Oct 3, 2026"), ("Studios", "Studio Hoshi"), ("Source", "Light novel"), ("Length", "24 min per episode")]);
        assert_eq!(s.cover_url.as_deref(), Some("s.jpg"));
    }

    #[test]
    fn manga_words_read_as_the_desktop() {
        let s = service_details(&sample(Kind::Manga), Service::AniList, 0, DETAILS_FRESH_SECS, true);
        assert_eq!(s.tags, ["Manga", "24 chapters", "Publishing"]);
        assert!(!s.fresh && s.on_list);
        let f: Vec<(&str, &str)> = s.facts.iter().map(|f| (f.label.as_str(), f.value.as_str())).collect();
        assert_eq!(f, [("Published", "Since Oct 3, 2026"), ("Authors", "Aoi Mori (Story)"), ("Chapters", "24"), ("Volumes", "Unknown while publishing")]);
    }

    #[test]
    fn pasted_links_and_ids() {
        assert_eq!(mal_id_in("https://myanimelist.net/anime/41590/slug", Kind::Anime), Some((41590, true)));
        assert_eq!(mal_id_in("myanimelist.net/manga.php?id=2001", Kind::Manga), Some((2001, true)));
        assert_eq!(mal_id_in("https://myanimelist.net/anime/41590", Kind::Manga), None);
        assert_eq!(mal_id_in("41590", Kind::Anime), Some((41590, false)));
        assert_eq!(mal_id_in("myanimelist.net/anime/season", Kind::Anime), None);
        assert_eq!(anilist_id_in("https://anilist.co/anime/170001/x", Kind::Anime), Some((170001, true)));
        assert_eq!(anilist_id_in("12", Kind::Manga), Some((12, false)));
        assert_eq!(date_text("2025-10"), "Oct 2025");
    }

    #[derive(Default)]
    struct Vault(std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>);
    impl Platform for Vault {
        fn utc_offset(&self, _at: i64) -> i32 {
            0
        }
        fn log(&self, _l: LogLevel, _m: String) {}
        fn token_load(&self, s: String) -> Option<Vec<u8>> {
            self.0.lock().unwrap().get(&s).cloned()
        }
        fn token_save(&self, s: String, b: Vec<u8>) -> bool {
            self.0.lock().unwrap().insert(s, b);
            true
        }
        fn token_delete(&self, s: String) {
            self.0.lock().unwrap().remove(&s);
        }
    }

    #[test]
    fn add_title_uses_the_saved_details_and_pasted_ids_follow_the_service() {
        let dir = std::env::temp_dir().join(format!("kotora-mobile-details-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = Paths { config_dir: dir.join("c").display().to_string(), data_dir: dir.join("d").display().to_string(), cache_dir: dir.join("x").display().to_string() };
        let k = Kotora::new(paths, ClientIds { mal: "m".into(), anilist: "a".into() }, Arc::new(Vault::default())).unwrap();
        let stub = TitleStub { kind: MediaKind::Anime, id: 7, title: "Stub".into(), cover_url: None, format: String::new(), episodes: None };
        assert_eq!(k.add_title(stub.clone(), ListStatus::Watching), Err(KotoraError::NotSignedIn));
        assert!(k.pasted_id(MediaKind::Anime, "7".into()).is_none());
        assert!(matches!(k.fetch_details(MediaKind::Anime, 7), Err(KotoraError::Offline { .. })));

        let t = kotora_core::tokens::Token { access_token: "f".into(), refresh_token: "f".into(), expires_at: i64::MAX / 2 };
        k.tokens.save("mal", &t).unwrap();
        k.store().set_account("mal", "me").unwrap();
        assert!(k.saved_details(MediaKind::Anime, 7).is_none());
        k.store().save_details("mal", &sample(Kind::Anime), time::now_unix()).unwrap();
        let saved = k.saved_details(MediaKind::Anime, 7).unwrap();
        assert!(saved.fresh && !saved.on_list);

        k.add_title(stub, ListStatus::PlanToWatch).unwrap();
        let row = k.entry(MediaKind::Anime, 7).unwrap().unwrap();
        assert_eq!(row.title, "Kirigaoka no Chouchin", "the saved details fill the entry");
        assert_eq!(row.status, ListStatus::PlanToWatch);
        assert!(row.waiting);
        assert!(k.saved_details(MediaKind::Anime, 7).unwrap().on_list);

        assert_eq!(k.pasted_id(MediaKind::Anime, "https://myanimelist.net/anime/41590".into()), Some(PastedId { id: 41590, link: true }));
        assert_eq!(k.pasted_id(MediaKind::Anime, "https://anilist.co/anime/41590".into()), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
