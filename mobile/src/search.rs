//! Search and add, as the desktop's search page: the active service's results for a query (anime
//! or manga), and an add that goes through the queue (so it works offline too). Which results are
//! already on the list, the pause in typing and the short cache are the app's (as on the desktop,
//! where they live in the UI's state).

use kotora_core::service::AnimeHit;
use kotora_core::{Change, Kind, Scope, time};

use super::*;

/// Results asked for (the desktop's `search::LIMIT`).
pub const SEARCH_LIMIT: u32 = 10;

/// One search result: the service's answer, and the words its line shows.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Hit {
    pub kind: MediaKind,
    pub id: u32,
    pub title: String,
    pub title_en: Option<String>,
    pub title_ja: Option<String>,
    pub synonyms: Vec<String>,
    /// The service's raw format (`tv`, `movie`, `manga`, …).
    pub format: String,
    /// Episodes, or chapters; `None`: unknown.
    pub episodes: Option<u32>,
    pub volumes: Option<u32>,
    pub air_status: String,
    pub season_year: Option<u16>,
    /// `winter` / `spring` / `summer` / `fall`.
    pub season_name: Option<String>,
    /// The service's mean score, 1-10.
    pub mean: Option<f32>,
    pub cover_url: Option<String>,
    pub authors: Vec<String>,
    pub start_date: Option<String>,
    /// The tag before the line: "TV", "Movie", "Light novel"; empty: none.
    pub type_label: String,
    /// "12 eps · Airing · Fall 2026 · ★ 8.31", as the desktop's result line.
    pub meta: String,
}

/// The desktop's `kind_label` (anime) and MyAnimeList's manga type label; empty for unknown.
fn type_label(h: &AnimeHit) -> String {
    if h.kind == Kind::Manga {
        let t = kotora_core::mal::manga_type_label(&h.format);
        return if t == "Unknown" { String::new() } else { t };
    }
    match h.format.as_str() {
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
    .to_string()
}

/// The desktop's `search::hit_meta`.
pub fn hit_meta(h: &AnimeHit) -> String {
    let mut bits = Vec::new();
    if h.kind == Kind::Manga {
        if let Some(n) = h.episodes.filter(|n| *n > 0) {
            bits.push(format!("{n} ch"));
        }
        if let Some(n) = h.volumes.filter(|n| *n > 0) {
            bits.push(if n == 1 { "1 vol".to_string() } else { format!("{n} vols") });
        }
        let run = match h.air_status.as_str() {
            "currently_publishing" => "Publishing",
            "finished" => "Finished",
            "on_hiatus" => "On hiatus",
            "discontinued" => "Discontinued",
            "not_yet_published" => "Not yet published",
            _ => "",
        };
        if !run.is_empty() {
            bits.push(run.to_string());
        }
        if let Some(m) = h.mean {
            bits.push(format!("\u{2605} {m:.2}"));
        }
        return bits.join(" \u{b7} ");
    }
    if let Some(n) = h.episodes {
        bits.push(if n == 1 { "1 ep".to_string() } else { format!("{n} eps") });
    }
    match h.air_status.as_str() {
        "currently_airing" => bits.push("Airing".into()),
        "not_yet_aired" => bits.push("Not yet aired".into()),
        _ => {}
    }
    if let Some(se) = h.season_text() {
        bits.push(se);
    }
    if let Some(m) = h.mean {
        bits.push(format!("\u{2605} {m:.2}"));
    }
    bits.join(" \u{b7} ")
}

impl From<&AnimeHit> for Hit {
    fn from(h: &AnimeHit) -> Hit {
        Hit {
            kind: h.kind.into(),
            id: h.id,
            title: h.title.clone(),
            title_en: h.title_en.clone(),
            title_ja: h.title_ja.clone(),
            synonyms: h.synonyms.clone(),
            format: h.format.clone(),
            episodes: h.episodes,
            volumes: h.volumes,
            air_status: h.air_status.clone(),
            season_year: h.season.as_ref().map(|s| s.0),
            season_name: h.season.as_ref().map(|s| s.1.clone()),
            mean: h.mean,
            cover_url: h.cover_url.clone(),
            authors: h.authors.clone(),
            start_date: h.start_date.clone(),
            type_label: type_label(h),
            meta: hit_meta(h),
        }
    }
}

impl From<&Hit> for AnimeHit {
    fn from(h: &Hit) -> AnimeHit {
        AnimeHit {
            kind: h.kind.into(),
            id: h.id,
            title: h.title.clone(),
            title_en: h.title_en.clone(),
            title_ja: h.title_ja.clone(),
            synonyms: h.synonyms.clone(),
            format: h.format.clone(),
            episodes: h.episodes,
            air_status: h.air_status.clone(),
            season: h.season_year.zip(h.season_name.clone()),
            mean: h.mean,
            cover_url: h.cover_url.clone(),
            volumes: h.volumes,
            authors: h.authors.clone(),
            start_date: h.start_date.clone(),
        }
    }
}

#[uniffi::export]
impl Kotora {
    /// The active service's results for `query` (anime or manga), as the desktop's search page
    /// asks them. Network: call it off the main thread.
    pub fn search(&self, kind: MediaKind, query: String) -> Result<Vec<Hit>, KotoraError> {
        let service = self.active_or_err()?;
        let mut svc = self.service(service)?;
        let q = query.split_whitespace().collect::<Vec<_>>().join(" ");
        Ok(svc.browse_of(kind.into(), &q, SEARCH_LIMIT)?.iter().map(Hit::from).collect())
    }

    /// "+ Watching" or another status on a result: saved at once, queued for the service. An
    /// entry already on the list just gets the status, as on the desktop.
    pub fn add(&self, hit: Hit, status: ListStatus) -> Result<(), KotoraError> {
        let service = self.active_or_err()?;
        let media = AnimeHit::from(&hit).media();
        let mut store = self.store();
        let scope = Scope::new(service.name(), media.kind);
        let now = time::now_unix();
        if store.load_item(scope, media.id)?.is_some() {
            store.enqueue(scope, media.id, Change { status: Some(status.into()), ..Default::default() }, now)?;
        } else {
            store.add_to_list(service.name(), &media, status.into(), None, now)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[derive(Default)]
    struct Fake(Mutex<HashMap<String, Vec<u8>>>);

    impl Platform for Fake {
        fn utc_offset(&self, _at_unix: i64) -> i32 {
            crate::testenv::OFFSET
        }
        fn log(&self, _level: LogLevel, _message: String) {}
        fn token_load(&self, service: String) -> Option<Vec<u8>> {
            self.0.lock().unwrap().get(&service).cloned()
        }
        fn token_save(&self, service: String, bytes: Vec<u8>) -> bool {
            self.0.lock().unwrap().insert(service, bytes);
            true
        }
        fn token_delete(&self, service: String) {
            self.0.lock().unwrap().remove(&service);
        }
    }

    fn signed_in(name: &str) -> Arc<Kotora> {
        let dir = std::env::temp_dir().join(format!("kotora-mobile-search-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = Paths { config_dir: dir.join("c").display().to_string(), data_dir: dir.join("d").display().to_string(), cache_dir: dir.join("k").display().to_string() };
        let k = Kotora::new(paths, ClientIds { mal: "fake".into(), anilist: "fake".into() }, Arc::new(Fake::default())).unwrap();
        let t = kotora_core::tokens::Token { access_token: "fake".into(), refresh_token: "fake".into(), expires_at: i64::MAX / 2 };
        k.tokens.save("mal", &t).unwrap();
        k.store().set_account("mal", "made-up-user").unwrap();
        k
    }

    fn anime(id: u32, eps: Option<u32>, air: &str, season: Option<(u16, &str)>, mean: Option<f32>) -> AnimeHit {
        AnimeHit {
            kind: Kind::Anime,
            id,
            title: format!("Made-up {id}"),
            title_en: Some("Made Up".into()),
            title_ja: None,
            synonyms: vec!["MU".into()],
            format: "tv".into(),
            episodes: eps,
            air_status: air.into(),
            season: season.map(|(y, s)| (y, s.to_string())),
            mean,
            cover_url: Some("https://example.invalid/c.jpg".into()),
            volumes: None,
            authors: Vec::new(),
            start_date: None,
        }
    }

    #[test]
    fn the_result_line_says_what_the_desktop_says() {
        let h = Hit::from(&anime(1, Some(12), "currently_airing", Some((2026, "fall")), Some(8.31)));
        assert_eq!(h.type_label, "TV");
        assert_eq!(h.meta, "12 eps \u{b7} Airing \u{b7} Fall 2026 \u{b7} \u{2605} 8.31");
        assert_eq!(Hit::from(&anime(2, Some(1), "finished_airing", None, None)).meta, "1 ep");
        let mut m = anime(3, Some(40), "on_hiatus", None, Some(7.0));
        m.kind = Kind::Manga;
        m.format = "light_novel".into();
        m.volumes = Some(1);
        let h = Hit::from(&m);
        assert_eq!((h.type_label.as_str(), h.meta.as_str()), ("Light novel", "40 ch \u{b7} 1 vol \u{b7} On hiatus \u{b7} \u{2605} 7.00"));
        // Back to the core's hit without a loss.
        assert_eq!(AnimeHit::from(&h), m);
    }

    #[test]
    fn add_queues_the_entry_and_a_second_add_changes_its_status() {
        let k = signed_in("add");
        let hit = Hit::from(&anime(77, Some(12), "currently_airing", Some((2026, "fall")), None));
        k.add(hit.clone(), ListStatus::PlanToWatch).unwrap();
        let row = k.entry(MediaKind::Anime, 77).unwrap().unwrap();
        assert_eq!((row.status, row.progress, row.waiting), (ListStatus::PlanToWatch, 0, true));
        assert_eq!(row.cover_url.as_deref(), Some("https://example.invalid/c.jpg"));
        assert_eq!(k.waiting_count(), 1);
        k.add(hit, ListStatus::Watching).unwrap();
        assert_eq!(k.entry(MediaKind::Anime, 77).unwrap().unwrap().status, ListStatus::Watching);
        assert_eq!(k.list(MediaKind::Anime, Some(ListStatus::Watching), Sort::Updated).unwrap().len(), 1);
    }

    #[test]
    fn search_and_add_need_a_sign_in() {
        let dir = std::env::temp_dir().join(format!("kotora-mobile-search-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = Paths { config_dir: dir.join("c").display().to_string(), data_dir: dir.join("d").display().to_string(), cache_dir: dir.join("k").display().to_string() };
        let k = Kotora::new(paths, ClientIds::default(), Arc::new(Fake::default())).unwrap();
        assert_eq!(k.search(MediaKind::Anime, "made up".into()), Err(KotoraError::NotSignedIn));
        let hit = Hit::from(&anime(1, None, "", None, None));
        assert_eq!(k.add(hit, ListStatus::Watching), Err(KotoraError::NotSignedIn));
    }
}
