//! Writes a made-up list into a fresh Kotora database, through the core's own store, for the
//! iOS spike's demo mode (and any screenshot run): `cargo run --example demo_db -- <out.db> [now]`.
//! The app ships the file and opens it with the real `Kotora` object; nothing here is real and
//! nothing is sent anywhere. The titles are the mockup's (as Android's SampleRepository).

use kotora_core::store::Store;
use kotora_core::{Kind, ListItem, Media, Score, Status};

#[allow(clippy::too_many_arguments)]
fn item(kind: Kind, id: u32, title: &str, status: Status, progress: u32, total: Option<u32>, score: Option<u32>, updated_at: i64, format: &str, air: &str) -> ListItem {
    ListItem {
        media: Media {
            kind,
            id,
            title: title.into(),
            title_en: None,
            title_ja: None,
            synonyms: Vec::new(),
            episodes: total,
            format: format.into(),
            air_status: air.into(),
            cover_url: None,
            start_date: Some(if air == "currently_airing" { "2026-10-03" } else { "2025-01-10" }.into()),
            broadcast: None,
            volumes: None,
            authors: Vec::new(),
            next_airing: None,
            id_mal: None,
        },
        status,
        progress,
        score: score.and_then(Score::from_ten),
        rewatching: false,
        times_rewatched: 0,
        volumes: 0,
        updated_at,
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("usage: demo_db <out.db> [now]");
    let now: i64 = args.next().map(|s| s.parse().expect("now: unix seconds")).unwrap_or(1_795_000_000);
    let _ = std::fs::remove_file(&out);
    let mut store = Store::open(std::path::Path::new(&out)).expect("open the database");

    let (a, w, air, done) = (Kind::Anime, Status::Watching, "currently_airing", "finished_airing");
    let mut anime = vec![
        item(a, 7, "Hoshikuzu Relay", w, 6, Some(12), Some(8), now - 60, "tv", air),
        item(a, 3, "Kagerou Bakery", w, 5, Some(13), Some(7), now - 300, "tv", air),
        item(a, 11, "Tsukimi Station 2nd Season", w, 3, Some(12), None, now - 3_600, "tv", air),
        ListItem { rewatching: true, ..item(a, 19, "Ashita no Lantern", Status::Completed, 20, Some(25), Some(9), now - 7_200, "tv", done) },
        item(a, 5, "Neko Tantei Rin", w, 1, Some(8), None, now - 86_400, "ona", air),
        item(a, 23, "Sakamichi Cycle Club", w, 14, Some(24), Some(8), now - 2 * 86_400, "tv", done),
        item(a, 29, "Kumo no Ue no Toshokan", w, 2, None, None, now - 3 * 86_400, "tv", air),
        item(a, 31, "Shiosai Diner", w, 9, Some(12), Some(7), now - 4 * 86_400, "tv", done),
    ];
    anime[0].media.title_en = Some("Stardust Relay".into());
    for (i, t) in ["Garasu no Umi", "Tenbin Academy", "Hanabi Courier", "Yoru no Kissaten", "Mizuiro Signal"].iter().enumerate() {
        anime.push(item(a, 40 + i as u32, t, Status::PlanToWatch, 0, Some(12), None, now - (10 + i as i64) * 86_400, "tv", "not_yet_aired"));
    }
    for (i, t) in ["Kitsune Postman", "Aozora Workshop", "Ginga Ramen", "Sora Tobu Shoten", "Kaze no Orchestra", "Momiji Line"].iter().enumerate() {
        anime.push(item(a, 60 + i as u32, t, Status::Completed, 12, Some(12), Some(9 - i as u32 % 3), now - (30 + i as i64) * 86_400, "tv", done));
    }
    anime.push(item(a, 80, "Fuyu no Robot", Status::OnHold, 4, Some(11), None, now - 60 * 86_400, "tv", done));
    anime.push(item(a, 81, "Hikari Tantei Office", Status::Dropped, 2, Some(12), Some(4), now - 90 * 86_400, "tv", done));

    let m = Kind::Manga;
    let manga = vec![
        ListItem { volumes: 3, ..item(m, 101, "Paper Lanterns", w, 21, None, Some(8), now - 600, "manga", "currently_publishing") },
        ListItem { volumes: 5, ..item(m, 102, "Umibe no Atelier", w, 44, Some(52), Some(9), now - 86_400, "manga", "finished") },
        item(m, 103, "Kaminari Bento", Status::PlanToWatch, 0, None, None, now - 5 * 86_400, "manga", "currently_publishing"),
        ListItem { volumes: 7, ..item(m, 104, "Hoshi no Tegami", Status::Completed, 60, Some(60), Some(8), now - 40 * 86_400, "manga", "finished") },
    ];

    store.replace_list("mal", &anime, now - 240).expect("save the anime list");
    store.replace_list(kotora_core::Scope::new("mal", Kind::Manga), &manga, now - 240).expect("save the manga list");
    store.set_account("mal", "sample-user").expect("save the account");
    store.set_manga_enabled(true).expect("turn Manga mode on");
    println!("{out}: {} anime, {} manga (made up)", anime.len(), manga.len());
}
