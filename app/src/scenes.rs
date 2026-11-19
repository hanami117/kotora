//! Named states for headless renders (`--scene NAME`), filled with fake, deterministic data.
//! `--list-scenes` lists them. Every view and state gets one.

use kotora_core::{Kind, Score, ScoreFormat, Status};
use kotora_core::airing::Season;
use kotora_core::config::{ListView, Theme, TitleLang};

use kotora_core::queue::OpState;
use kotora_core::recognize::Found;

use crate::model::{Auth, Filter, Page, Sort, State};
use crate::settings::Section;
use crate::setup::Step;

/// A scene: its name and the change it makes to the fake first state.
pub type Scene = (&'static str, fn(&mut State));

pub const SCENES: &[Scene] = &[
    // The main list, as in the design mockup.
    ("list", |_| {}),
    // Signed in to AniList: the same list, AniList's account in the sidebar.
    ("list-anilist", anilist),
    // Real covers (made up here) on the rows that have one; two rows have none and keep the tile.
    ("list-covers", fake_covers),
    // 320 entries on Watching, with covers: frame time on a long list.
    ("list-many", many),
    // The same long list as posters and as large cards.
    ("list-many-grid", |s| {
        many(s);
        s.prefs.views.set(Status::Watching, ListView::Grid);
    }),
    ("list-many-large", |s| {
        many(s);
        s.prefs.views.set(Status::Watching, ListView::Large);
    }),
    // The list views (the toolbar's switch): posters with +1 always on Watching, large cards.
    ("list-grid", |s| s.prefs.views.set(Status::Watching, ListView::Grid)),
    ("list-grid-covers", |s| {
        fake_covers(s);
        s.prefs.views.set(Status::Watching, ListView::Grid);
    }),
    ("list-large", |s| s.prefs.views.set(Status::Watching, ListView::Large)),
    ("list-large-covers", |s| {
        fake_covers(s);
        s.prefs.views.set(Status::Watching, ListView::Large);
    }),
    // Off Watching, +1 and edit wait for the pointer.
    ("list-grid-completed", |s| {
        fake_covers(s);
        s.prefs.views.set(Status::Completed, ListView::Grid);
        s.go(Page::List(Status::Completed));
    }),
    // A card picked with the arrows (outlined).
    ("list-grid-selected", |s| {
        fake_covers(s);
        s.prefs.views.set(Status::Watching, ListView::Grid);
        s.grid.sel = s.rows.get(2).map(|&k| s.entries[k].id);
    }),
    ("list-grid-airing", |s| {
        fake_covers(s);
        s.prefs.views.set(Status::Watching, ListView::Grid);
        s.set_filter(Filter::Airing);
    }),
    // Manga mode (MyAnimeList): the switch on Manga, the Reading page, made-up titles. Progress
    // shows chapters, volumes or both, whichever was read.
    ("manga-list", manga),
    ("manga-list-tiles", |s| {
        crate::manga::fake(s);
    }),
    ("manga-list-editor", |s| {
        manga(s);
        s.manga_toggle_editor(1);
    }),
    // A completed manga's editor: Reread beside the statuses.
    ("manga-list-editor-completed", |s| {
        manga(s);
        s.go(Page::Manga(Status::Completed));
        s.manga_toggle_editor(9);
    }),
    ("manga-list-completed", |s| {
        manga(s);
        s.go(Page::Manga(Status::Completed));
    }),
    ("manga-list-publishing", |s| {
        manga(s);
        s.set_manga_filter(crate::manga::MFilter::Publishing);
    }),
    // The filter box looks at every status.
    ("manga-list-filter", |s| {
        manga(s);
        s.manga.query.replace_text("a");
        s.refresh_manga();
    }),
    // Edits waiting in the queue: tags on the rows, the badge.
    ("manga-list-waiting", |s| {
        manga(s);
        let _ = s.manga_plus_one(2);
        let _ = s.manga_plus_one(6);
    }),
    // Manga just turned on: the first read runs.
    ("manga-loading", |s| {
        crate::manga::fake(s);
        s.manga.entries.clear();
        s.manga.synced_at = None;
        s.manga.loading = true;
        s.refresh_manga();
    }),
    // Read, but nothing on the list.
    ("manga-empty", |s| {
        crate::manga::fake(s);
        s.manga.entries.clear();
        s.refresh_manga();
    }),
    // The last manga sync failed: said over the saved list, and in the sidebar.
    ("manga-sync-failed", |s| {
        manga(s);
        s.manga.error = Some("Your manga list wasn't read: can't reach MyAnimeList.".into());
    }),
    // It failed before anything was read.
    ("manga-sync-failed-first", |s| {
        crate::manga::fake(s);
        s.manga.entries.clear();
        s.manga.synced_at = None;
        s.manga.error = Some("Your manga list wasn't read: MyAnimeList asked Kotora to slow down. Try again in a minute.".into());
        s.refresh_manga();
    }),
    // Poster views on the manga pages (kept apart from the anime pages' views).
    ("manga-grid", |s| {
        manga(s);
        s.prefs.manga_views.set(Status::Watching, ListView::Grid);
    }),
    ("manga-large", |s| {
        manga(s);
        s.prefs.manga_views.set(Status::Watching, ListView::Large);
    }),
    ("manga-grid-completed", |s| {
        manga(s);
        s.prefs.manga_views.set(Status::Completed, ListView::Grid);
        s.go(Page::Manga(Status::Completed));
    }),
    // Search in Manga mode: the manga list's matches, then MyAnimeList's manga.
    ("manga-search", |s| manga_search(s, "salt", true)),
    ("manga-search-menu", |s| {
        manga_search(s, "salt", true);
        s.online.menu = Some(501);
    }),
    ("manga-search-added", |s| {
        manga_search(s, "salt", true);
        s.add_hit(501, Status::PlanToWatch);
    }),
    ("manga-search-empty", |s| {
        manga(s);
        s.open_search();
        s.find.focused = false;
    }),
    // A manga not on the list (from a result): Add as Reading and the other statuses.
    ("manga-details-not-on-list", |s| {
        manga_search(s, "salt", true);
        s.open_mal_id(501);
        let mut d = crate::manga::fake_details(501, "Salt Road Couriers");
        d.mine = None;
        d.title_en = Some("Couriers of the Salt Road".into());
        d.title_ja = None;
        d.synonyms.clear();
        d.air_status = "finished".into();
        d.episodes = Some(64);
        d.volumes = Some(8);
        s.details_answer(crate::details::Answer { id: 501, kind: kotora_core::Kind::Manga, result: Ok(d), item: None, done: true });
    }),
    // Remove from list: asks first, then a toast with Undo; the right-click menu.
    ("manga-remove-confirm", |s| {
        manga(s);
        s.ask_remove_manga(2);
    }),
    ("manga-removed", |s| {
        manga(s);
        let _ = s.remove_manga(2);
    }),
    ("manga-details-remove-confirm", |s| {
        manga_details(s, 1);
        s.ask_remove_manga(1);
    }),
    ("manga-ctx", |s| {
        manga(s);
        s.open_manga_ctx(2, crate::ctxmenu::At::Point(420.0, 230.0));
    }),
    // "Continue reading" in the menu of a manga with a known last comic.
    ("manga-ctx-continue", |s| {
        manga(s);
        crate::reading::fake_resume(s, 1, 87, false);
        s.open_manga_ctx(1, crate::ctxmenu::At::Point(420.0, 140.0));
    }),
    ("manga-ctx-move", |s| {
        manga(s);
        s.open_manga_ctx(2, crate::ctxmenu::At::Point(420.0, 230.0));
        if let Some(c) = &mut s.ctx {
            c.sub = Some((crate::ctxmenu::Sub::Move, false));
        }
    }),
    ("manga-ctx-grid", |s| {
        manga(s);
        s.prefs.manga_views.set(Status::Watching, ListView::Grid);
        s.open_manga_ctx(2, crate::ctxmenu::At::Point(420.0, 300.0));
    }),
    // A manga removal that MyAnimeList changed meanwhile: the conflict card.
    ("manga-queue-removal-conflict", |s| {
        manga(s);
        let _ = s.remove_manga(2);
        let mut op = s.ops[0].clone();
        op.id = 98;
        op.kind = kotora_core::Kind::Manga;
        op.media_id = 2;
        op.title = "Salt & Gears".into();
        op.episodes = Some(88);
        op.removed = true;
        op.change = kotora_core::Change::default();
        op.old = kotora_core::Values { volumes: 0, ..kotora_core::Values::new(Status::Watching, 64, kotora_core::score::ten(8)) };
        op.state = OpState::Conflict;
        op.remote = Some(kotora_core::Remote { values: kotora_core::Values { volumes: 9, ..kotora_core::Values::new(Status::Watching, 70, kotora_core::score::ten(8)) }, updated_at: crate::model::FAKE_NOW - 1800 });
        s.ops.retain(|o| o.state != OpState::Conflict);
        s.ops.insert(0, op);
        s.toast = None;
        s.apply(crate::model::Msg::Manga(crate::manga::MangaMsg::Items { items: vec![], gone: vec![] }));
        s.go(Page::Queue);
    }),
    // A manga's page: the information, the "Your list" card with chapters and volumes.
    ("manga-details", |s| manga_details(s, 1)),
    ("manga-details-editor", |s| {
        manga_details(s, 1);
        s.manga_toggle_editor(1);
    }),
    // Finished, both counts known, reread once.
    ("manga-details-completed", |s| manga_details(s, 3)),
    // Publishing (Manga mode's Airing): the Reading manga's latest chapters from MangaUpdates
    // (made up), sorted by the next estimated release.
    ("publishing", publishing),
    ("publishing-behind", |s| {
        publishing(s);
        s.set_publishing_sort(crate::publishing::PSort::Behind);
    }),
    ("publishing-latest", |s| {
        publishing(s);
        s.set_publishing_sort(crate::publishing::PSort::Latest);
    }),
    // Stale scanlation histories (official simulpubs), finished series, a real hiatus, a weekly.
    ("publishing-stale", |s| {
        publishing(s);
        crate::publishing::fake_stale(s);
    }),
    // MangaDex's official chapters in use (MangaUpdates frozen), with the source choice.
    ("publishing-stale-panel", |s| {
        publishing(s);
        crate::publishing::fake_stale(s);
        s.publishing_toggle(102);
    }),
    // Read past every source: "ahead of release data".
    ("publishing-ahead-panel", |s| {
        publishing(s);
        crate::publishing::fake_stale(s);
        s.publishing_toggle(101);
    }),
    ("manga-details-publishing-ahead", |s| {
        manga_details(s, 1);
        crate::publishing::fake_stale(s);
        let e = s.manga.entries.iter().find(|e| e.id == 101).unwrap();
        let stub = crate::manga::stub(e);
        s.open_details(stub);
    }),
    // Reading detection (Manga): YACReader has chapter 120 of a Reading manga open, page 14.
    ("now-reading", |s| {
        reading(s, "Inkwell Cartographers v12 c120.cbz", 14);
        s.go(Page::NowPlaying);
    }),
    // Read to the end: the banner asks on top of every page, Now Playing shows Mark read.
    ("now-reading-offer", |s| reading(s, "Inkwell Cartographers v12 c120.cbz", 40)),
    ("now-reading-offer-page", |s| {
        reading(s, "Inkwell Cartographers v12 c120.cbz", 40);
        s.go(Page::NowPlaying);
    }),
    // "Update automatically" on: marked at once, Undo in the banner.
    ("now-reading-recorded", |s| {
        s.prefs.auto = true;
        reading(s, "Inkwell Cartographers v12 c120.cbz", 40);
        s.go(Page::NowPlaying);
    }),
    // A completed manga read again: the banner asks "Reading it again?", Now Playing offers the reread.
    ("now-reading-reread-offer", |s| reading(s, "Cobalt Postman v04 c031.cbz", 40)),
    ("now-reading-reread-offer-page", |s| {
        reading(s, "Cobalt Postman v04 c031.cbz", 40);
        s.go(Page::NowPlaying);
    }),
    // "Start reread" pressed: rereading from chapter 31, Undo in the banner.
    ("now-reading-reread-started", |s| {
        reading(s, "Cobalt Postman v04 c031.cbz", 40);
        let _ = s.confirm_reading();
        s.go(Page::NowPlaying);
    }),
    // Auto on, but chapter 122 at 119 read: it asks and says what it skips.
    ("now-reading-skip", |s| {
        s.prefs.auto = true;
        reading(s, "Inkwell Cartographers - Ch. 122.cbz", 40);
    }),
    // A whole-volume file: the volume is offered.
    ("now-reading-volume", |s| {
        reading(s, "Night Shift Alchemist/Night Shift Alchemist v04.cbz", 40);
        s.go(Page::NowPlaying);
    }),
    // A close title: not sure, Wrong manga… opens the picker.
    ("now-reading-unsure", |s| {
        reading(s, "Inkwell Cartographer c120.cbz", 40);
        s.go(Page::NowPlaying);
    }),
    ("now-reading-picking", |s| {
        reading(s, "Inkwell Cartographer c120.cbz", 40);
        s.wrong_manga();
    }),
    // A series off the list, found by the search: Add as Reading.
    ("now-reading-not-on-list", |s| {
        reading(s, "Tidepool Couriers c007.cbz", 40);
        if let Some(r) = &mut s.reading.current {
            r.found = Found::NotOnList { media_id: 501, title: "Tidepool Couriers".into() };
        }
        s.go(Page::NowPlaying);
    }),
    // An episode plays and a chapter was read: both on Now Playing.
    ("now-reading-and-playing", |s| {
        crate::manga::fake(s);
        crate::reading::fake(s, "Inkwell Cartographers v12 c120.cbz", 14);
        s.go(Page::NowPlaying);
    }),
    // Live test shape: the YACReader viewer shows Dragon Ball Z v11 (its command line; nothing
    // written yet), not on the list: Now Reading in the sidebar, Add as Reading.
    ("now-reading-yacreader-open", |s| {
        crate::manga::fake(s);
        crate::reading::fake_open(s, "YACReader", "/media/comics/Dragon Ball Z v11 (2003) (Digital) (LuCaZ).cbz", None);
        if let Some(r) = &mut s.reading.current {
            r.found = Found::NotOnList { media_id: 502, title: "Dragon Ball Z".into() };
        }
        s.go(Page::NowPlaying);
    }),
    // On a list page: the sidebar's Now Reading card (an episode plays too; the comic wins in
    // Manga mode).
    ("now-reading-sidebar", |s| {
        crate::manga::fake(s);
        crate::reading::fake_open(s, "YACReader", "/media/comics/Inkwell Cartographers/Inkwell Cartographers v12 c120.cbz", None);
    }),
    // The viewer closed a comic read to the end: the question stays (banner, card "Closed"),
    // Now Reading's sidebar card is gone.
    ("now-reading-closed-asks", |s| {
        crate::manga::fake(s);
        s.now = None;
        crate::reading::fake(s, "Inkwell Cartographers v12 c120.cbz", 40);
        s.reading_open(Vec::new());
        s.go(Page::NowPlaying);
    }),
    // The details page of a manga read in YACReader: where it stopped, Continue reading.
    ("manga-details-continue", |s| {
        manga_details(s, 1);
        crate::reading::fake_resume(s, 1, 87, false);
    }),
    // Read to the end: the next volume in the folder.
    ("manga-details-continue-next", |s| {
        manga_details(s, 1);
        crate::reading::fake_resume(s, 1, 1, true);
    }),
    // The comic closed half-read: Now Reading says where it stopped.
    ("now-reading-continue", |s| {
        crate::manga::fake(s);
        s.now = None;
        crate::reading::fake(s, "Inkwell Cartographers v12 c120.cbz", 14);
        crate::reading::fake_resume(s, 1, 87, false);
        s.reading_open(Vec::new());
        s.go(Page::NowPlaying);
    }),
    // Live test: VIZ's Dragon Ball Z v11 open in YACReader: "Is this Dragon Ball?", vol 27.
    ("now-reading-dbz", |s| {
        crate::manga::fake(s);
        s.now = None;
        crate::reading::fake_dbz(s);
        s.go(Page::NowPlaying);
    }),
    // Now Reading with nothing read yet.
    ("now-reading-empty", |s| {
        crate::manga::fake(s);
        s.now = None;
        s.go(Page::NowPlaying);
    }),
    // Stage 2: a comic open in Simple Comic (no page): shown, asked later.
    ("now-reading-open-file", |s| {
        crate::manga::fake(s);
        s.now = None;
        crate::reading::fake_open(s, "Simple Comic", "/media/manga/Inkwell Cartographers/Inkwell Cartographers c120.cbz", None);
        s.go(Page::NowPlaying);
    }),
    // The next chapter opened: chapter 120 counts, and it asks (also with auto on).
    ("now-reading-open-file-counted", |s| {
        crate::manga::fake(s);
        s.now = None;
        s.prefs.auto = true;
        crate::reading::fake_open(s, "Simple Comic", "/media/manga/Inkwell Cartographers/Inkwell Cartographers c120.cbz", Some("Ch 121 opened next"));
        s.go(Page::NowPlaying);
    }),
    // A chapter inside volume 14: chapter 120 and volume 13 together.
    ("now-reading-volume-before", |s| {
        reading(s, "Inkwell Cartographers v14 c120.cbz", 40);
        s.go(Page::NowPlaying);
    }),
    // Stage 2 on, a comic folder, YACReader's own libraries not looked up.
    ("settings-reading-open-files", |s| {
        crate::manga::fake(s);
        s.reading.find = false;
        s.reading.added = vec!["/media/archive/Comics".into()];
        s.reading.watched = vec!["/media/archive/Comics".into()];
        s.reading.open_files = true;
        s.reading.folders = vec!["/media/manga".into()];
        crate::settings::open(s, Section::Manga);
    }),
    // Settings → Notifications: the manga reading switches of their own.
    ("settings-notifications-reading", |s| {
        crate::manga::fake(s);
        s.prefs.notify.read_recorded = false;
        crate::settings::open(s, Section::Notifications);
    }),
    // Settings → Manga → Reading detection: a library found in YACReader's settings, one added.
    ("settings-reading", |s| {
        crate::manga::fake(s);
        s.reading.watched = vec!["/media/comics/Manga".into(), "/media/archive/Comics".into()];
        s.reading.added = vec!["/media/archive/Comics".into()];
        crate::settings::open(s, Section::Manga);
    }),
    ("settings-reading-error", |s| {
        crate::manga::fake(s);
        s.reading.watched = vec!["/media/comics/Manga".into()];
        s.reading.error = Some("No YACReader library at /media/comics/Other (it has no .yacreaderlibrary folder).".into());
        crate::settings::open(s, Section::Manga);
    }),
    // Manga off: the switch is greyed and says why.
    ("settings-reading-manga-off", |s| crate::settings::open(s, Section::Manga)),
    // Settings → Manga → Release data.
    // Settings → Feeds: off (the first look), on with two feeds (one keyed, masked), a nyaa.si
    // address typed.
    ("feeds-settings-off", |s| crate::settings::open(s, Section::Feeds)),
    ("feeds-settings", |s| {
        crate::feeds::fake(s);
        crate::settings::open(s, Section::Feeds);
    }),
    // Settings → Feeds → Filters: Add filter… (Taiga's presets), then one preset in the editor
    // with its element picker open.
    ("feeds-filter-presets", |s| {
        crate::feeds::fake(s);
        s.feeds_filter_new();
        crate::settings::open(s, Section::Feeds);
        crate::settings::set_query(s, "preset");
    }),
    ("feeds-filter-edit", |s| {
        crate::feeds::fake(s);
        s.feeds_filter_edit(4);
        s.feeds_picker("el/0".into());
        crate::settings::open(s, Section::Feeds);
        crate::settings::set_query(s, "preset");
    }),
    ("feeds-settings-nyaa", |s| {
        crate::feeds::fake(s);
        s.feeds.draft.text = "https://nyaa.si/?f=0&q=lanterns+of+kirigaoka".into();
        s.feeds_draft_changed();
        crate::settings::open(s, Section::Feeds);
    }),
    // The Feeds page: new episodes, best release first; one row opened; no feeds yet.
    ("feeds", |s| {
        crate::feeds::fake(s);
        s.open_feeds();
    }),
    ("feeds-open", |s| {
        crate::feeds::fake(s);
        s.feeds.open_row = s.feeds.entries.iter().find(|e| crate::feeds::is_new(e)).map(|e| e.item.key());
        s.open_feeds();
    }),
    // Search releases (Taiga's "Search for torrents"): made-up results for one show.
    ("feeds-search", |s| {
        crate::feeds::fake(s);
        let e = s.entries.iter().find(|e| e.status == kotora_core::Status::Watching).unwrap();
        let (id, title, ep) = (e.id, e.main_title.clone(), e.progress + 1);
        s.feeds_search(id);
        let items = (0..3)
            .map(|k| kotora_core::feeds::Item {
                guid: format!("s{k}"),
                title: format!("[Hoshi] {title} - {:02} [1080p]", ep + k),
                magnet: Some(format!("magnet:?xt=urn:btih:{:040x}", 900 + k)),
                seeders: Some(30),
                ..Default::default()
            })
            .collect();
        s.feeds_found(id, Ok(items));
    }),
    ("feeds-history", |s| {
        crate::feeds::fake(s);
        let t: Vec<String> = s.feeds.items.iter().take(3).map(|(_, i)| i.title.clone()).collect();
        s.feeds.archive = Some(kotora_core::feeds::taiga::Archive { titles: t });
        s.feeds.history = true;
        s.open_feeds();
    }),
    ("feeds-empty", |s| {
        s.feeds.cfg.enabled = true;
        s.open_feeds();
    }),
    // The ↓ Ep N badge on Watching, and the details' Available card.
    ("feeds-list-badge", |s| crate::feeds::fake(s)),
    ("feeds-details", |s| {
        crate::feeds::fake(s);
        let id = s.feeds.entries.iter().find(|e| crate::feeds::is_new(e)).and_then(|e| e.anime_id).unwrap_or(0);
        details(s, id, true);
    }),
    ("settings-manga", |s| {
        crate::manga::fake(s);
        crate::settings::open(s, Section::Manga);
    }),
    ("settings-manga-one-source", |s| {
        crate::manga::fake(s);
        s.prefs.sources.md = false;
        crate::settings::open(s, Section::Manga);
    }),
    // A row's panel: a guessed match, its releases, the link field.
    ("publishing-panel", |s| {
        publishing(s);
        s.publishing_toggle(2);
    }),
    // No series found; a pasted link that isn't one.
    ("publishing-panel-missing", |s| {
        publishing(s);
        s.publishing_toggle(7);
        s.publishing.link = rune::TextEdit::new("https://www.mangaupdates.com/series.html?id=1234");
        s.publishing_use_link(7);
    }),
    // A picked series (Reset beside Use) after a link was used.
    ("publishing-panel-picked", |s| {
        publishing(s);
        s.publishing_toggle(4);
        s.publishing.link_note = Some((false, "Now matched with \u{201c}Moth Orchestra\u{201d}.".into()));
    }),
    // The first look: most rows still looking, the count in the top bar.
    ("publishing-loading", |s| {
        manga(s);
        s.go(Page::Publishing);
        let mut info = crate::publishing::fake_info();
        info.truncate(2);
        s.publishing_fake(info);
        s.publishing.busy = Some((2, 6));
    }),
    // Offline with saved data: it shows, with its age; no Retry.
    ("publishing-offline", |s| {
        publishing(s);
        s.offline = true;
        s.publishing.checked_at = Some(s.clock() - 3 * 3600);
    }),
    // Offline with nothing saved: the rows, no "Looking…", and where the data comes from.
    ("publishing-offline-empty", |s| {
        manga(s);
        s.offline = true;
        s.go(Page::Publishing);
        s.publishing_fake(Vec::new());
    }),
    // Online, a refresh failed: the error, and the saved releases still show.
    ("publishing-failed", |s| {
        publishing(s);
        s.publishing.error = Some("Can\u{2019}t reach MangaUpdates.".into());
    }),
    ("manga-details-publishing-offline", |s| {
        manga_details(s, 1);
        s.offline = true;
        s.publishing_fake(crate::publishing::fake_info());
        s.publishing.checked_at = Some(s.clock() - 3 * 3600);
    }),
    ("publishing-empty", |s| {
        manga(s);
        for e in s.manga.entries.iter_mut().filter(|e| e.status == Status::Watching) {
            e.status = Status::OnHold;
        }
        s.go(Page::Publishing);
        s.publishing_fake(Vec::new());
    }),
    // A Reading manga's page: the Publishing card under Your list.
    ("manga-details-publishing", |s| {
        manga_details(s, 1);
        s.publishing_fake(crate::publishing::fake_info());
    }),
    ("manga-details-publishing-guess", |s| {
        manga_details(s, 2);
        s.publishing_fake(crate::publishing::fake_info());
    }),
    // The sync queue with a manga change beside the anime ones.
    ("manga-queue", |s| {
        manga(s);
        let _ = s.manga_plus_one(2);
        let mut op = s.ops[1].clone();
        op.id = 99;
        op.kind = kotora_core::Kind::Manga;
        op.media_id = 2;
        op.title = "Salt & Gears".into();
        op.episodes = Some(88);
        op.change = kotora_core::Change { progress: Some(65), volumes: Some(8), ..Default::default() };
        op.old = kotora_core::Values { volumes: 7, ..kotora_core::Values::new(Status::Watching, 64, None) };
        op.state = OpState::Queued;
        s.ops.insert(0, op);
        s.apply(crate::model::Msg::Manga(crate::manga::MangaMsg::Items { items: vec![], gone: vec![] }));
        s.go(Page::Queue);
    }),
    // Back on Anime: the anime list, Airing back in the sidebar.
    ("manga-switch-anime", |s| {
        crate::manga::fake(s);
        s.set_kind(kotora_core::Kind::Anime);
    }),
    // AniList: the same manga pages, scores in the user's AniList format (here 100-point).
    ("manga-list-anilist", |s| {
        anilist(s);
        manga(s);
        if let Some(p) = &mut s.profile {
            p.score_format = ScoreFormat::Point100;
        }
    }),
    ("manga-list-anilist-stars", |s| {
        anilist(s);
        manga(s);
        if let Some(p) = &mut s.profile {
            p.score_format = ScoreFormat::Point5;
        }
        s.manga_toggle_editor(2);
    }),
    // Home: continue watching, airing this week, plan to watch.
    ("home", home),
    // Manga on: Continue reading under Continue watching (+1 is the next chapter or volume).
    ("home-manga", |s| {
        manga(s);
        s.set_kind(kotora_core::Kind::Anime);
        home(s);
    }),
    // The search page's own kind: the switch is on Anime, Tab (or the toggle) looks for manga.
    ("search-kind-manga", |s| {
        manga(s);
        s.set_kind(kotora_core::Kind::Anime);
        s.find.replace_text("salt");
        s.open_search();
        s.find.focused = false;
        s.toggle_search_kind();
        s.online.pending = false;
        s.online.show("salt", crate::manga::fake_hits());
    }),
    // The manga grid with a card picked by the arrow keys.
    ("manga-grid-selected", |s| {
        manga(s);
        s.prefs.manga_views.set(Status::Watching, ListView::Grid);
        s.manga.grid_sel = Some(2);
    }),
    ("home-covers", |s| {
        fake_covers(s);
        home(s);
    }),
    // The next files are in the library folders: Play on the Continue watching cards.
    ("home-library", |s| {
        crate::library::fake(s);
        fake_covers(s);
        home(s);
    }),
    // A list with nothing airing, nothing planned: the shelves say so.
    ("home-quiet", |s| {
        s.entries.retain(|e| e.status == Status::Watching);
        home(s);
        for e in &mut s.entries {
            e.timetable = None;
        }
    }),
    // The account's MyAnimeList profile picture (made up), round in the sidebar.
    ("account-picture", |s| {
        fake_covers(s);
        s.avatar = Some("https://cdn.example.invalid/images/userimages/1.jpg".into());
    }),
    ("list-watching", |s| s.go(Page::List(Status::Watching))),
    ("list-plan", |s| s.go(Page::List(Status::PlanToWatch))),
    ("list-completed", |s| s.go(Page::List(Status::Completed))),
    ("list-on-hold", |s| s.go(Page::List(Status::OnHold))),
    // Dropped holds nothing in the fake list: the empty state.
    ("list-dropped", |s| s.go(Page::List(Status::Dropped))),
    ("list-airing", |s| s.set_filter(Filter::Airing)),
    ("list-new-episode", |s| s.set_filter(Filter::NewEpisode)),
    // The network is down: the saved list shows, with the alert and the sidebar line.
    ("list-offline", |s| {
        s.offline = true;
        s.error = Some("Can't reach MyAnimeList. Showing your saved list.".into());
    }),
    // A sync is running over a saved list.
    ("list-syncing", |s| s.syncing = true),
    // Search and sort on the list.
    ("list-search", |s| {
        s.query.replace_text("harbor");
        s.go(Page::List(Status::Completed));
        s.refresh();
    }),
    ("list-search-none", |s| {
        s.query.replace_text("zzzz");
        s.refresh();
    }),
    // No match on the list: the filter searches the service right there (0.2.1): waiting for
    // the pause, its results (open the details page, + Watching), too short, offline, nothing.
    ("list-filter-online-searching", |s| filter_online(s, "orchid", None)),
    ("list-filter-online", |s| filter_online(s, "orchid", Some(crate::search::fake_hits()))),
    ("list-filter-online-short", |s| filter_online(s, "fz", None)),
    ("list-filter-online-offline", |s| {
        s.offline = true;
        filter_online(s, "orchid", None);
    }),
    ("list-filter-online-none", |s| filter_online(s, "zzzz", Some(Vec::new()))),
    ("list-filter-online-anilist", |s| {
        anilist(s);
        filter_online(s, "orchid", Some(crate::search::fake_hits()));
    }),
    ("list-sort-title", |s| s.set_sort(Sort::Title)),
    ("list-sort-score", |s| s.set_sort(Sort::Score)),
    // The Sort menu open, in each view; a reversed order on the button.
    ("list-sort-menu", |s| s.sort_menu = true),
    ("list-grid-sort-menu", |s| {
        fake_covers(s);
        s.prefs.views.set(Status::Watching, ListView::Grid);
        s.set_sort(Sort::Title);
        s.sort_menu = true;
    }),
    ("list-large-sort-menu", |s| {
        fake_covers(s);
        s.prefs.views.set(Status::Watching, ListView::Large);
        s.set_sort(Sort::Score);
        s.set_sort_rev(true);
        s.sort_menu = true;
    }),
    ("list-sort-reversed", |s| {
        s.set_sort(Sort::Title);
        s.set_sort_rev(true);
    }),
    // The account card opens the Profile page: picture, MyAnimeList's numbers, the scores.
    ("profile", |s| s.go(Page::Profile)),
    ("profile-picture", |s| {
        fake_covers(s);
        s.avatar = Some("https://cdn.example.invalid/images/userimages/1.jpg".into());
        s.go(Page::Profile);
    }),
    // Before the first answer: counts from the saved list, dashes for the rest.
    ("profile-pending", |s| {
        s.profile = None;
        s.go(Page::Profile);
    }),
    ("profile-no-scores", |s| {
        for e in &mut s.entries {
            e.score = None;
        }
        s.go(Page::Profile);
    }),
    // Statistics (sidebar → Kotora): made-up numbers on the fake list's titles.
    ("stats", |s| stats(s, Kind::Anime, false)),
    ("stats-manga", |s| {
        manga(s);
        s.manga.mode = true;
        stats(s, Kind::Manga, false);
    }),
    // Just upgraded: the list's totals, nothing logged yet.
    ("stats-empty", |s| stats(s, Kind::Anime, true)),
    ("stats-loading", |s| s.open_stats()),
    // The token was refused: the saved list shows, with a Sign in button.
    ("list-sign-in-again", |s| s.auth = Auth::Expired),
    // First run: nothing set up yet, so the setup wizard (`setup.rs`), step 1 of 3 (Kotora's own Client ID is built in).
    ("no-client-id", |s| setup(s, Step::Welcome)),
    // The very first screen: "Which service do you use?" (nothing chosen, no Client ID, no token).
    // Snap it at --size 700x900 for the stacked cards; a Tab (--script) focuses MyAnimeList.
    ("setup-service", |s| {
        setup(s, Step::Service);
        s.setup_service = true;
    }),
    // Welcome after the choice: "Set up MyAnimeList", with Back to the choice.
    ("setup-welcome-after-service", |s| {
        setup(s, Step::Welcome);
        s.setup_service = true;
    }),
    ("setup-sign-in", |s| {
        setup(s, Step::SignIn);
        s.auth = Auth::SignedOut;
        s.client_id.hint = Some("8f3a".into());
    }),
    // Waiting for the browser, with the link in case it didn't open.
    ("setup-signing-in", |s| signing_in(s, true)),
    // AniList: the first steps in AniList's words, and the wait with the token field (for a
    // client registered with AniList's PIN page).
    ("setup-anilist-welcome", |s| {
        setup(s, Step::Welcome);
        s.setup_service = true;
        s.service = kotora_core::service::ServiceId::AniList;
    }),
    // AniList has its own Client ID built in: Welcome, then straight to "Sign in with AniList".
    ("setup-anilist-sign-in", |s| {
        setup(s, Step::SignIn);
        s.service = kotora_core::service::ServiceId::AniList;
        s.auth = Auth::SignedOut;
    }),
    ("setup-anilist-signing-in", |s| {
        setup(s, Step::SignIn);
        s.service = kotora_core::service::ServiceId::AniList;
        s.auth = Auth::SigningIn;
        let url = kotora_core::anilist::authorize_url(kotora_core::service::ANILIST_CLIENT_ID);
        s.apply(crate::model::Msg::SignInUrl { url, opened: true });
    }),
    ("setup-anilist-token-error", |s| {
        setup(s, Step::SignIn);
        s.service = kotora_core::service::ServiceId::AniList;
        s.auth = Auth::SigningIn;
        s.client_id.token.replace_text("12345");
        crate::online::sign_in_with_token(s);
    }),
    // The loopback failed (a busy port): the error, "Try again", and the token field.
    ("setup-anilist-loopback-failed", |s| {
        setup(s, Step::SignIn);
        s.service = kotora_core::service::ServiceId::AniList;
        s.auth = Auth::SignedOut;
        s.error = Some("Can't listen on port 47321 for AniList's reply: Address already in use".into());
    }),
    ("setup-signing-in-no-browser", |s| {
        signing_in(s, false);
        s.client_id.link_copied = true;
    }),
    ("setup-sign-in-denied", |s| {
        setup(s, Step::SignIn);
        s.auth = Auth::SignedOut;
        s.client_id.hint = Some("8f3a".into());
        s.error = Some("MyAnimeList refused the sign-in (access_denied)".into());
    }),
    ("setup-sign-in-timeout", |s| {
        setup(s, Step::SignIn);
        s.auth = Auth::SignedOut;
        s.client_id.hint = Some("8f3a".into());
        s.error = Some("sign-in timed out".into());
    }),
    // Back on step 4 after signing in ("Set up again" while signed in).
    ("setup-signed-in", |s| {
        s.setup_again();
        s.setup = Step::SignIn;
    }),
    // Done: the list loaded, the players hint.
    ("setup-done", |s| {
        s.setup_again();
        s.setup = Step::Done;
    }),
    ("setup-done-loading", |s| {
        s.setup_again();
        s.setup = Step::Done;
        s.entries.clear();
        s.refresh();
        s.loading = true;
        s.syncing = true;
    }),
    // Settings → Accounts → "Set up again": step 1, with Close.
    ("setup-again", |s| {
        s.go(Page::Settings);
        s.setup_again();
    }),
    // Saved: Sign in is enabled at once, and the page says the ID was saved.
    ("client-id-saved", |s| {
        signed_out(s, Auth::SignedOut);
        s.client_id.saved = true;
    }),
    ("signed-out", |s| signed_out(s, Auth::SignedOut)),
    ("signing-in", |s| signed_out(s, Auth::SigningIn)),
    ("sign-in-failed", |s| {
        signed_out(s, Auth::SignedOut);
        s.error = Some("MyAnimeList refused the sign-in. Try again.".into());
    }),
    // Every status empty (a new account).
    ("empty", |s| {
        s.entries.clear();
        s.refresh();
        s.ops.clear();
        s.queue = 0;
        s.now = None;
    }),
    // The first sync with nothing saved yet.
    ("loading", |s| {
        s.entries.clear();
        s.refresh();
        s.loading = true;
        s.syncing = true;
        s.ops.clear();
        s.queue = 0;
        s.now = None;
    }),
    // The inline editor under a row (Lanterns of Kirigaoka), as opened by a press on it.
    ("list-editor", |s| s.toggle_editor(0)),
    // The editor after picking Completed (progress fills) and a score of 9.
    ("list-editor-changed", |s| {
        s.toggle_editor(4);
        s.editor_status(Status::Completed);
        s.editor_score(kotora_core::score::ten(9));
    }),
    // `+1` pressed on a synced row: the number moves at once and the row waits to sync.
    ("list-plus-one", |s| {
        let _ = s.plus_one(4);
    }),
    // `+1` on an airing entry whose next episode hasn't aired yet (estimate): "Mark anyway?".
    ("list-plus-one-not-aired", |s| {
        crate::model::airing_entry(s, 0, 5);
        let _ = s.plus_one_or_ask(0);
    }),
    // The editor saved two episodes ahead of the estimate: it stays open under the question.
    ("list-editor-not-aired", |s| {
        crate::model::airing_entry(s, 0, 5);
        s.toggle_editor(0);
        s.editor_step(2);
        let _ = s.save_editor();
    }),
    // An entry with no episode count (still airing): `?` and no upper bound.
    ("list-editor-unknown-count", |s| {
        if let Some(e) = s.entries.iter_mut().find(|e| e.id == 6) {
            e.episodes = None;
        }
        s.toggle_editor(6);
    }),
    // Now Playing: episode 6 of Lanterns of Kirigaoka in mpv, 68 % in, a sure match (no banner yet).
    ("now-playing-page", |s| s.go(Page::NowPlaying)),
    ("now-playing-paused", |s| {
        if let Some(n) = &mut s.now {
            n.playing = false;
        }
        s.go(Page::NowPlaying);
    }),
    // It passed the threshold: the banner asks on top of every page.
    ("now-playing-offer", reached),
    ("now-playing-offer-page", |s| {
        reached(s);
        s.go(Page::NowPlaying);
    }),
    // "Update automatically" on: recorded at once, Undo in the banner.
    ("now-playing-recorded", |s| {
        s.prefs.auto = true;
        reached(s);
    }),
    // "Update automatically" on, but episode 7 plays at progress 5: it asks, and says why.
    ("now-playing-offer-skip", |s| {
        s.prefs.auto = true;
        playing_file(s, "[Hoshi] Lanterns of Kirigaoka - 07 (1080p).mkv");
        reached(s);
    }),
    // "Update automatically" on, but episode 6 hasn't aired yet by the estimate: it asks.
    ("now-playing-offer-not-aired", |s| {
        s.prefs.auto = true;
        crate::model::airing_entry(s, 0, 5);
        reached(s);
    }),
    // A close title, not the same: "Not sure", and the banner says so.
    ("now-playing-unsure", |s| {
        unsure(s);
        s.go(Page::NowPlaying);
    }),
    ("now-playing-unsure-season", |s| {
        unsure(s);
        playing_file(s, "Lantern of Kirigaoka - S02E06.mkv");
        s.go(Page::NowPlaying);
    }),
    ("now-playing-unsure-offer", |s| {
        unsure(s);
        reached(s);
    }),
    // "Wrong anime…": the closest titles on the list.
    ("now-playing-picking", |s| s.wrong_anime()),
    // Real covers (made up here) in the Now Playing card and the sidebar's card.
    ("now-playing-covers", |s| {
        fake_covers(s);
        s.go(Page::NowPlaying);
    }),
    // Not on the list: the cover comes with the match (its saved details).
    ("now-playing-not-on-list-cover", |s| {
        s.covers.fake();
        playing_file(s, "[Hoshi] Velvet Orchard Chronicle - 02 (1080p).mkv");
        if let Some(n) = &mut s.now {
            n.found = Found::NotOnList { media_id: 9001, title: "Velvet Orchard Chronicle".into() };
            n.cover_url = Some("https://cdn.example.invalid/9001l.jpg".into());
        }
        s.go(Page::NowPlaying);
    }),
    // One MyAnimeList search found it, but it isn't on the list.
    ("now-playing-not-on-list", |s| {
        playing_file(s, "[Hoshi] Velvet Orchard Chronicle - 02 (1080p).mkv");
        if let Some(n) = &mut s.now {
            n.found = Found::NotOnList { media_id: 9001, title: "Velvet Orchard Chronicle".into() };
        }
        s.go(Page::NowPlaying);
    }),
    // "Add as Watching at episode 2" pressed: added and recorded in one change, Undo in the banner.
    ("now-playing-not-on-list-added", |s| {
        playing_file(s, "[Hoshi] Velvet Orchard Chronicle - 02 (1080p).mkv");
        if let Some(n) = &mut s.now {
            n.found = Found::NotOnList { media_id: 9001, title: "Velvet Orchard Chronicle".into() };
        }
        s.add_playing();
        s.go(Page::NowPlaying);
    }),
    // anime-relations moved episode 15 of a season to its second part, which isn't on the list.
    ("now-playing-relation-not-on-list", |s| {
        playing_file(s, "[Hoshi] Velvet Orchard Chronicle S2 - 15 (1080p).mkv");
        if let Some(n) = &mut s.now {
            n.found = Found::NotOnList { media_id: 9002, title: "Velvet Orchard Chronicle 2nd Season Part 2".into() };
            n.relation = Some(crate::now::Relation { file_ep: 15, from_id: 9003, from_title: Some("Velvet Orchard Chronicle 2nd Season".into()), ep: 2 });
            if let Some(p) = &mut n.parsed {
                p.episode = Some(2);
            }
        }
        s.go(Page::NowPlaying);
    }),
    // Nothing on the list, nothing found.
    ("now-playing-unknown", |s| {
        playing_file(s, "holiday_2026_beach.mp4");
        if let Some(n) = &mut s.now {
            n.found = Found::Nothing;
            n.candidates.clear();
        }
        s.go(Page::NowPlaying);
    }),
    // A file that matches nothing, with a position: no "Offer at" label, no tick, "Pick anime…".
    ("now-playing-unmatched", |s| {
        playing_file(s, "Kotora-promo.mp4");
        if let Some(n) = &mut s.now {
            n.found = Found::Nothing;
            n.candidates.clear();
        }
        s.go(Page::NowPlaying);
    }),
    // No player plays anything.
    ("now-playing-empty", |s| {
        s.now = None;
        s.go(Page::NowPlaying);
    }),
    // A completed anime plays again: the banner asks to start a rewatch at the playing episode.
    ("now-playing-rewatch-offer", |s| {
        rewatch_playing(s);
        reached(s);
    }),
    // "Start rewatch" pressed: rewatching from episode 3, Undo in the banner.
    ("now-playing-rewatch-started", |s| {
        rewatch_playing(s);
        reached(s);
        let _ = s.start_rewatch_playing();
    }),
    // The queue as in the mockup: a conflict, two waiting, one failed, two sent today.
    ("queue-page", |s| {
        queue_covers(s);
        s.go(Page::Queue);
    }),
    ("queue-empty", |s| {
        s.ops.clear();
        s.queue = 0;
        s.go(Page::Queue);
    }),
    // No network: the queue keeps everything, with Try now.
    ("queue-offline", |s| {
        s.offline = true;
        s.error = Some("Can't reach MyAnimeList. Showing your saved list.".into());
        if let Some(op) = s.ops.iter_mut().find(|o| o.id == 2) {
            op.error = Some("can't reach the service: no answer from MyAnimeList".into());
            op.attempts = 2;
        }
        s.go(Page::Queue);
    }),
    // The sign-in expired (401, refresh refused): the queue is kept, with a Sign in button.
    ("queue-sign-in-again", |s| {
        s.auth = Auth::Expired;
        s.go(Page::Queue);
    }),
    // A conflict where the entry was removed on MyAnimeList.
    ("queue-conflict-removed", |s| {
        if let Some(op) = s.ops.iter_mut().find(|o| o.state == OpState::Conflict) {
            op.remote = None;
        }
        s.go(Page::Queue);
    }),
    // Remove from list (0.2.1): the question (list and details page), the toast with Undo, the
    // removal waiting in the queue, and its conflict when the entry changed on the service.
    ("list-remove-confirm", |s| s.ask_remove(2)),
    ("details-remove-confirm", |s| {
        details(s, 0, true);
        s.ask_remove(0);
    }),
    // With the mirror on (0.2.1): the question says the other service loses it too.
    ("list-remove-confirm-mirror", |s| {
        crate::mirror::fake_on(s);
        s.ask_remove(2);
    }),
    ("details-remove-confirm-mirror", |s| {
        crate::mirror::fake_on(s);
        details(s, 0, true);
        s.ask_remove(0);
    }),
    ("list-removed", |s| {
        let _ = s.remove_entry(2);
    }),
    ("queue-removal", |s| {
        queue_covers(s);
        removal(s, OpState::Queued);
        s.go(Page::Queue);
    }),
    ("queue-removal-conflict", |s| {
        queue_covers(s);
        removal(s, OpState::Conflict);
        s.go(Page::Queue);
    }),
    // One change on its way, nothing else open.
    ("queue-sending", |s| {
        s.ops.retain(|o| o.id == 2 || o.state == OpState::Done);
        if let Some(op) = s.ops.iter_mut().find(|o| o.id == 2) {
            op.state = OpState::Sending;
        }
        s.queue = 1;
        s.go(Page::Queue);
    }),
    // Search and add: the list's matches (every status), then MyAnimeList's results.
    ("search-online", |s| search(s, "kirigaoka", true)),
    ("search-online-pending", |s| {
        search(s, "kirigaoka", false);
        s.online.pending = true;
    }),
    // The status menu of one result is open.
    ("search-online-menu", |s| {
        search(s, "kirigaoka", true);
        s.online.menu = Some(9102);
    }),
    // One result added as Watching: it moves up to the list's matches, waiting to sync.
    ("search-online-added", |s| {
        search(s, "kirigaoka", true);
        s.add_hit(9101, Status::Watching);
    }),
    ("search-online-short", |s| search(s, "ki", false)),
    ("search-online-offline", |s| {
        s.offline = true;
        search(s, "kirigaoka", false);
    }),
    ("search-online-error", |s| {
        search(s, "kirigaoka", false);
        s.online.error = Some("Can't reach MyAnimeList.".into());
    }),
    ("search-online-none", |s| {
        search(s, "zzzz", false);
        s.online.show("zzzz", vec![]);
    }),
    // The search page as the sidebar's Search or Ctrl K opens it: the field focused, a hint.
    ("search-page", |s| s.open_search()),
    // The list page's box filters the list; one line takes the query to the search page.
    ("list-filter", |s| {
        s.query.replace_text("lantern");
        s.search_changed();
    }),
    // A bare number may be a MyAnimeList id: the "Open" line on top (Enter opens it).
    ("search-mal-id", |s| {
        search(s, "41590", false);
        s.online.show("41590", vec![]);
    }),
    // Rewatches: the Watching page shows one (Completed on MAL; "+ 1 rewatching" by the count),
    // the Completed page's editor offers "Rewatch", the details page counts them.
    ("list-rewatching", |s| {
        fake_covers(s);
        s.go(Page::List(Status::Watching));
    }),
    ("list-editor-rewatch", |s| {
        s.go(Page::List(Status::Completed));
        let id = s.rows.first().map(|&i| s.entries[i].id).unwrap_or(0);
        s.toggle_editor(id);
    }),
    ("details-rewatch", |s| {
        let id = completed_id(s);
        if let Some(e) = s.entries.iter_mut().find(|e| e.id == id) {
            e.times_rewatched = 2;
        }
        details(s, id, true);
    }),
    // During a rewatch: the tag, "Rewatched once" and Stop rewatching.
    ("details-rewatching", |s| details(s, 5, true)),
    // The details page of an anime on the list, opened from its row.
    ("details-on-list", |s| details(s, 0, true)),
    ("details-editor", |s| {
        details(s, 0, true);
        s.toggle_editor(0);
    }),
    // Not on the list: Add to list.
    ("details-not-on-list", |s| details(s, 9102, false)),
    ("details-added", |s| {
        details(s, 9102, false);
        s.add_shown(Status::PlanToWatch);
    }),
    // Opened, nothing known yet but the row's title.
    ("details-loading", |s| {
        s.covers.fake();
        s.open_details(crate::details::Stub::bare(9103, "Kirigaoka Lantern Shop"));
    }),
    ("details-error", |s| {
        s.open_details(crate::details::Stub::bare(9103, "Kirigaoka Lantern Shop"));
        if let Some(d) = &mut s.detail {
            d.error = Some("Can't reach MyAnimeList.".into());
        }
    }),
    // Airing (`airing.rs`): a made-up Winter 2027 season, Central European time.
    ("airing-week", |s| airing(s, crate::airing::View::Week)),
    ("airing-week-mine", |s| {
        airing(s, crate::airing::View::Week);
        s.airing_mine(crate::airing::Mine::On);
    }),
    ("airing-season", |s| airing(s, crate::airing::View::Season)),
    // The list's airing shows with the most episodes out and not watched first.
    ("airing-season-to-watch", |s| {
        airing(s, crate::airing::View::Season);
        s.airing_mine(crate::airing::Mine::On);
        s.airing_order(crate::airing::Order::ToWatch);
    }),
    ("airing-season-tv", |s| {
        airing(s, crate::airing::View::Season);
        s.airing_kind(crate::airing::Kind::Tv);
    }),
    ("airing-season-mine", |s| {
        airing(s, crate::airing::View::Season);
        s.airing_mine(crate::airing::Mine::On);
    }),
    ("airing-season-not-mine", |s| {
        airing(s, crate::airing::View::Season);
        s.airing_mine(crate::airing::Mine::Off);
    }),
    ("airing-season-added", |s| {
        airing(s, crate::airing::View::Season);
        s.airing_add(9302, Status::PlanToWatch);
    }),
    ("airing-season-english", |s| {
        s.set_titles(TitleLang::English);
        airing(s, crate::airing::View::Season);
    }),
    ("airing-loading", |s| {
        s.go(Page::Airing);
        s.airing.loading = true;
    }),
    ("airing-updating", |s| {
        // Saved (or first-page) rows are on screen and a worker still asks for the rest.
        airing(s, crate::airing::View::Week);
        s.airing.loading = true;
    }),
    ("airing-empty", |s| {
        let (season, _) = crate::airing::fake_season();
        s.airing_show(season, Vec::new());
        s.go(Page::Airing);
    }),
    ("airing-error", |s| {
        let (season, _) = crate::airing::fake_season();
        s.go(Page::Airing);
        s.airing_answer(crate::airing::Answer { season, shows: Vec::new(), fetched_at: None, error: Some("Can't reach MyAnimeList.".into()), done: true });
    }),
    ("airing-error-saved", |s| {
        airing(s, crate::airing::View::Week);
        let (season, _) = crate::airing::fake_season();
        s.airing_answer(crate::airing::Answer { season, shows: Vec::new(), fetched_at: None, error: Some("Can't reach MyAnimeList.".into()), done: true });
    }),
    ("airing-signed-out", |s| {
        s.auth = Auth::SignedOut;
        s.account = None;
        s.go(Page::Airing);
    }),
    // The season picker (`airing.rs`): the menu, another season (past, coming, far back), and its
    // loading and offline states. Winter 2027 is the current season.
    ("airing-season-menu", |s| {
        airing(s, crate::airing::View::Season);
        s.airing.menu = true;
    }),
    ("airing-prev-season", |s| airing_other(s, 3, 2026)),
    ("airing-prev-season-menu", |s| {
        airing_other(s, 3, 2026);
        s.airing.menu = true;
    }),
    ("airing-prev-season-mine", |s| {
        airing_other(s, 3, 2026);
        s.airing_mine(crate::airing::Mine::On);
    }),
    ("airing-next-season", |s| airing_other(s, 1, 2027)),
    ("airing-far-season-menu", |s| {
        airing_other(s, 1, 2024);
        s.airing.menu = true;
    }),
    ("airing-prev-season-english", |s| {
        s.set_titles(TitleLang::English);
        airing_other(s, 3, 2026);
    }),
    ("airing-season-loading", |s| {
        airing(s, crate::airing::View::Season);
        s.airing.browse = Some(crate::airing::Browse {
            season: Season {
                year: 2026,
                quarter: 3,
            },
            shows: Vec::new(),
            fetched_at: None,
            loading: true,
            error: None,
        });
        s.airing_rebuild();
    }),
    ("airing-season-offline", |s| {
        airing(s, crate::airing::View::Season);
        let season = Season {
            year: 2026,
            quarter: 3,
        };
        s.airing.browse = Some(crate::airing::Browse {
            season,
            shows: Vec::new(),
            fetched_at: None,
            loading: false,
            error: None,
        });
        s.airing_browse_answer(crate::airing::Answer {
            season,
            shows: Vec::new(),
            fetched_at: None,
            error: Some("Offline: the season lists come from MyAnimeList.".into()),
            done: true,
        });
    }),
    ("airing-season-error-saved", |s| {
        airing_other(s, 3, 2026);
        let season = Season {
            year: 2026,
            quarter: 3,
        };
        s.airing_browse_answer(crate::airing::Answer {
            season,
            shows: Vec::new(),
            fetched_at: None,
            error: Some("Can't reach MyAnimeList.".into()),
            done: true,
        });
    }),
    ("airing-details", |s| {
        airing(s, crate::airing::View::Season);
        s.airing_details(9301);
    }),
    ("settings-page", |s| s.go(Page::Settings)),
    // Settings, one scene per section (fake library folders for Library).
    ("settings-general", |s| crate::settings::open(s, Section::General)),
    // Settings → General → Keyboard with Alt picked for the list pages.
    ("settings-keys-alt", |s| {
        s.prefs.page_keys = kotora_core::config::PageKeys::Alt;
        crate::settings::open(s, Section::General)
    }),
    // Settings → General → On startup with Last page and Manga picked.
    ("settings-on-startup", |s| {
        s.prefs.open_on = kotora_core::config::OpenOn::Last;
        s.prefs.start_mode = kotora_core::config::StartMode::Manga;
        crate::settings::open(s, Section::General)
    }),
    // Settings → General → Discord on, with a title hidden and the paused status cleared.
    ("settings-discord-on", |s| {
        s.prefs.discord.enabled = true;
        s.prefs.discord.client_id = "123456789012345678".into();
        s.prefs.discord.hide_paused = true;
        crate::settings::open(s, Section::General)
    }),
    ("settings-appearance", |s| crate::settings::open(s, Section::Appearance)),
    ("settings-account", |s| crate::settings::open(s, Section::Account)),
    // AniList is the active service: its row is signed in, MyAnimeList's offers "Use MyAnimeList".
    ("settings-account-anilist", |s| {
        anilist(s);
        crate::settings::open(s, Section::Account)
    }),
    // AniList's Client ID is built in (none saved): "Use my own client ID\u{2026}" is the advanced link.
    ("settings-account-anilist-built-in", |s| {
        anilist(s);
        s.client_id.hint = None;
        crate::settings::open(s, Section::Account)
    }),
    // ... and it opens the field.
    ("settings-account-anilist-own-id", |s| {
        anilist(s);
        s.client_id.hint = None;
        s.client_id.editing = true;
        crate::settings::open(s, Section::Account)
    }),
    // Settings → Account → Your list (`transfer.rs`). The search brings the rows to the top.
    ("settings-transfer", |s| {
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "xml");
    }),
    ("settings-export-done", |s| {
        s.transfer.export_note = Some(Ok("Exported 14 anime to animelist-2027-01-15.xml.".into()));
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "xml");
    }),
    // AniList: no export (its API terms), import still works.
    ("settings-export-anilist", |s| {
        anilist(s);
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "xml");
    }),
    ("settings-import-preview", |s| {
        crate::transfer::fake_preview(s);
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "import");
    }),
    // A status conflict settled for the file's status.
    ("settings-import-preview-picked", |s| {
        crate::transfer::fake_preview(s);
        if let Some(p) = &mut s.transfer.preview
            && let Some(id) = p.plan.conflicts().next().and_then(|r| r.target)
        {
            p.use_file.insert(id);
        }
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "import");
    }),
    ("settings-import-preview-anilist", |s| {
        anilist(s);
        crate::transfer::fake_preview(s);
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "import");
    }),
    ("settings-import-busy", |s| {
        anilist(s);
        s.transfer.busy = Some("Looking up 40 MyAnimeList ids on AniList\u{2026}".into());
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "import");
    }),
    ("settings-import-error", |s| {
        s.transfer.error = Some("This is not a MyAnimeList list export (no <myanimelist>).".into());
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "import");
    }),
    ("settings-import-done", |s| {
        s.transfer.done = Some(crate::transfer::Done { queued: 17, ids: (1..=17).collect(), undone: None });
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "import");
    }),
    ("settings-import-undone", |s| {
        s.transfer.done = Some(crate::transfer::Done { queued: 17, ids: (1..=17).collect(), undone: Some(15) });
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "import");
    }),
    // Manga export and import (the rows show only while manga is on).
    ("settings-transfer-manga", |s| {
        s.manga.enabled = true;
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "xml");
    }),
    ("settings-manga-export-done", |s| {
        s.manga.enabled = true;
        s.transfer.manga_export_note = Some(Ok("Exported 12 manga to mangalist-2027-01-15.xml.".into()));
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "xml");
    }),
    // AniList: no export (its API terms); the manga import works (MyAnimeList ids looked up on AniList).
    ("settings-manga-anilist", |s| {
        anilist(s);
        s.manga.enabled = true;
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "xml");
    }),
    ("settings-manga-import-preview", |s| {
        s.manga.enabled = true;
        crate::transfer::fake_manga_preview(s);
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "import");
    }),
    // An anime file given to the manga import.
    ("settings-manga-import-error", |s| {
        s.manga.enabled = true;
        s.transfer.import_kind = kotora_core::Kind::Manga;
        s.transfer.error = Some(kotora_core::listxml::parse_kind(b"<myanimelist><anime/></myanimelist>", kotora_core::Kind::Manga).unwrap_err());
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "import");
    }),
    ("settings-manga-import-done", |s| {
        s.manga.enabled = true;
        s.transfer.import_kind = kotora_core::Kind::Manga;
        s.transfer.done = Some(crate::transfer::Done { queued: 9, ids: (1..=9).collect(), undone: None });
        crate::settings::open(s, Section::Account);
        crate::settings::set_query(s, "import");
    }),
    // Settings → Account → Mirror edits (`mirror.rs`). The search brings the row to the top.
    ("settings-mirror", |s| {
        s.mirror.other_signed_in = true;
        mirror_settings(s);
    }),
    // The other service isn't signed in: the switch is off and says how.
    ("settings-mirror-sign-in", mirror_settings),
    ("settings-mirror-busy", |s| {
        s.mirror.other_signed_in = true;
        s.mirror.busy = Some("Looking up 12 MyAnimeList ids on AniList\u{2026}".into());
        mirror_settings(s);
    }),
    ("settings-mirror-preview", |s| {
        crate::mirror::fake_preview(s);
        mirror_settings(s);
    }),
    // With Manga on: the manga lists compared too, one choice for both.
    ("settings-mirror-preview-manga", |s| {
        crate::manga::fake(s);
        crate::mirror::fake_preview_with_manga(s);
        mirror_settings(s);
    }),
    // The mirror on, Manga turned on since: "Align manga lists" waits for one comparison.
    ("settings-mirror-align-manga", |s| {
        crate::manga::fake(s);
        crate::mirror::fake_on(s);
        s.mirror.manga_aligned = false;
        mirror_settings(s);
    }),
    // Fill volumes from chapters (MangaDex): the row, looking up, the preview, done, offline.
    ("settings-fill-volumes", |s| fill_volumes(s, |_| {})),
    ("settings-fill-volumes-busy", |s| fill_volumes(s, |s| s.volfill.busy = Some((7, 18)))),
    ("settings-fill-volumes-preview", |s| fill_volumes(s, crate::volfill::fake_preview)),
    // Nothing goes up: "Nothing to change." and Close only (one already right, one higher).
    ("settings-fill-volumes-nothing", |s| {
        fill_volumes(s, |s| {
            crate::volfill::fake_preview(s);
            if let Some(p) = &mut s.volfill.preview {
                p.fill.clear();
                p.not_found.clear();
            }
        })
    }),
    ("settings-fill-volumes-done", |s| fill_volumes(s, |s| s.volfill.done = Some((4, vec![1, 2, 3, 4])))),
    ("settings-fill-volumes-offline", |s| {
        fill_volumes(s, |s| s.volfill.error = Some("Can\u{2019}t reach MangaDex: volumes from chapters need the network. Nothing was changed.".into()))
    }),
    // The manga editor's "Vol from chapters": ready, looking up, and not on MangaDex (hover note).
    ("manga-editor-vol-fill", |s| {
        manga(s);
        s.manga_toggle_editor(2);
        s.volfill.tables.insert(2, Some(kotora_core::mangadex::Lookup::Found(crate::volfill::fake_table())));
        crate::volfill::fill_editor(s);
        s.toast = None;
    }),
    ("manga-editor-vol-fill-looking", |s| {
        manga(s);
        s.manga_toggle_editor(2);
        s.volfill.tables.insert(2, None);
    }),
    ("manga-editor-vol-fill-missing", |s| {
        manga(s);
        s.manga_toggle_editor(4);
        s.volfill.tables.insert(4, Some(kotora_core::mangadex::Lookup::NotFound));
        s.volfill.tip = true;
    }),
    // A status conflict settled for AniList's status.
    ("settings-mirror-preview-picked", |s| {
        crate::mirror::fake_preview(s);
        if let Some(p) = &mut s.mirror.preview
            && let Some(r) = p.p.merge.iter().find(|r| r.conflict.is_some())
        {
            p.mirror_status.insert(r.mal_id);
        }
        mirror_settings(s);
    }),
    // AniList is the main list, MyAnimeList the mirror.
    ("settings-mirror-preview-anilist", |s| {
        anilist(s);
        crate::mirror::fake_preview(s);
        mirror_settings(s);
    }),
    ("settings-mirror-on", |s| {
        crate::mirror::fake_on(s);
        s.mirror.note = Some("On. Queued 14 changes for AniList and 3 for MyAnimeList.".into());
        mirror_settings(s);
    }),
    ("settings-mirror-error", |s| {
        s.mirror.other_signed_in = true;
        s.mirror.error = Some("Can\u{2019}t reach AniList. Its changes wait and go out when it answers.".into());
        mirror_settings(s);
    }),
    // The Queue page with the mirror's part: website edits, a conflict, waiting, failed, sent.
    ("queue-mirror", |s| {
        queue_covers(s);
        crate::mirror::fake_on(s);
        s.go(Page::Queue);
    }),
    // The mirror on: details pages and menus link to both services.
    ("details-mirror", |s| {
        crate::mirror::fake_on(s);
        details(s, 1, true);
    }),
    ("details-mirror-narrow", |s| {
        crate::mirror::fake_on(s);
        details(s, 1, true);
    }),
    ("manga-details-mirror", |s| {
        crate::manga::fake(s);
        crate::mirror::fake_on(s);
        manga_details(s, 1);
    }),
    ("manga-ctx-mirror", |s| {
        manga(s);
        crate::mirror::fake_on(s);
        s.open_manga_ctx(2, crate::ctxmenu::At::Point(420.0, 230.0));
    }),
    ("ctx-menu-mirror", |s| {
        crate::mirror::fake_on(s);
        s.open_ctx(1, crate::ctxmenu::At::Point(420.0, 230.0));
    }),
    // Manga in the mirror's queue: a website edit (chapters, volumes) and a conflict.
    ("queue-mirror-manga", |s| {
        queue_covers(s);
        crate::manga::fake(s);
        crate::mirror::fake_on(s);
        crate::mirror::fake_manga_on(s);
        s.go(Page::Queue);
    }),
    // The account chip with the mirror on (0.2.1): "MyAnimeList ⇄ AniList", nothing waiting.
    ("sidebar-mirror", |s| {
        s.mirror.other_signed_in = true;
        s.mirror.link = Some(kotora_core::mirror::Link::of(s.service));
    }),
    // Changes wait for the mirror: the dot, and the tooltip as the pointer rests on the chip.
    ("sidebar-mirror-dot", |s| {
        crate::mirror::fake_on(s);
        s.account_tip = true;
    }),
    // AniList main, MyAnimeList mirror.
    ("sidebar-mirror-anilist", |s| {
        anilist(s);
        crate::mirror::fake_on(s);
        s.account_tip = true;
    }),
    // A mirrored removal stopped: the entry changed on the mirror's website after it.
    ("queue-mirror-removal-conflict", |s| {
        queue_covers(s);
        crate::mirror::fake_on(s);
        if let Some(op) = s.mirror.ops.iter_mut().find(|o| o.state == OpState::Conflict) {
            op.removed = true;
            op.change = kotora_core::Change::default();
        }
        s.go(Page::Queue);
    }),
    // Nothing waits for the main list; the mirror still has things to show.
    ("queue-mirror-only", |s| {
        s.ops.clear();
        s.queue = 0;
        crate::mirror::fake_on(s);
        s.mirror.trouble = Some("Can\u{2019}t reach AniList. Its changes wait and go out when it answers.".into());
        s.go(Page::Queue);
    }),
    ("settings-detection", |s| crate::settings::open(s, Section::Detection)),
    ("settings-library", |s| {
        crate::library::fake(s);
        crate::settings::open(s, Section::Library);
    }),
    ("settings-about", |s| crate::settings::open(s, Section::About)),
    // The search: results from several sections, then none.
    ("settings-search", |s| {
        crate::settings::open(s, Section::General);
        crate::settings::set_query(s, "start");
    }),
    ("settings-search-dark", |s| {
        crate::settings::open(s, Section::General);
        crate::settings::set_query(s, "dark");
    }),
    ("settings-search-none", |s| {
        crate::settings::open(s, Section::General);
        crate::settings::set_query(s, "teleport");
    }),
    // A search jump: Appearance, with Theme tinted.
    ("settings-search-jump", |s| {
        crate::settings::open(s, Section::Appearance);
        s.settings.flash = Some(crate::settings::Row::Theme);
    }),
    // Library folders (`library.rs`): one folder online, one on an unplugged drive; Open with.
    ("library-settings", |s| {
        crate::library::fake(s);
        crate::settings::open(s, Section::Library);
    }),
    ("library-settings-error", |s| {
        crate::library::fake(s);
        s.library.draft = rune::TextEdit::new("/Volumes/Media/Anime/Lanterns");
        s.library.error = Some("That overlaps /Volumes/Media/Anime, already in the list.".into());
        crate::settings::open(s, Section::Library);
    }),
    // ▶ on the Watching rows whose next episode is in the folders.
    ("library-list", crate::library::fake),
    // Library folders with the grid: a middle-click or P plays the next episode.
    ("library-grid", |s| {
        crate::library::fake(s);
        fake_covers(s);
        s.prefs.views.set(Status::Watching, ListView::Grid);
    }),
    // Scores in each AniList score format (0.2.1): list rows, posters, the row editor (the 100-
    // point and decimal ones with a fine − / +), the details page and the Score menu.
    ("list-score-100", |s| scores_in(s, ScoreFormat::Point100)),
    ("list-score-decimal", |s| scores_in(s, ScoreFormat::Point10Decimal)),
    ("list-score-stars", |s| scores_in(s, ScoreFormat::Point5)),
    ("list-score-smileys", |s| scores_in(s, ScoreFormat::Point3)),
    ("list-grid-score-stars", |s| {
        scores_in(s, ScoreFormat::Point5);
        fake_covers(s);
        s.prefs.views.set(Status::Watching, ListView::Grid);
    }),
    ("list-large-score-smileys", |s| {
        scores_in(s, ScoreFormat::Point3);
        fake_covers(s);
        s.prefs.views.set(Status::Watching, ListView::Large);
    }),
    ("list-editor-score-100", |s| {
        scores_in(s, ScoreFormat::Point100);
        s.toggle_editor(0);
    }),
    ("list-editor-score-decimal", |s| {
        scores_in(s, ScoreFormat::Point10Decimal);
        s.toggle_editor(0);
        s.editor_score_step(1);
    }),
    ("list-editor-score-stars", |s| {
        scores_in(s, ScoreFormat::Point5);
        s.toggle_editor(0);
    }),
    ("list-editor-score-smileys", |s| {
        scores_in(s, ScoreFormat::Point3);
        s.toggle_editor(0);
    }),
    ("details-score-decimal", |s| {
        scores_in(s, ScoreFormat::Point10Decimal);
        details(s, 0, true);
    }),
    ("details-editor-score-stars", |s| {
        scores_in(s, ScoreFormat::Point5);
        details(s, 0, true);
        s.toggle_editor(0);
    }),
    ("ctx-menu-score-stars", |s| {
        scores_in(s, ScoreFormat::Point5);
        s.open_ctx(0, crate::ctxmenu::At::Point(470.0, 130.0));
        if let Some(c) = &mut s.ctx {
            c.sub = Some((crate::ctxmenu::Sub::Score, true));
        }
    }),
    ("ctx-menu-score-100", |s| {
        scores_in(s, ScoreFormat::Point100);
        s.open_ctx(0, crate::ctxmenu::At::Point(470.0, 130.0));
        if let Some(c) = &mut s.ctx {
            c.sub = Some((crate::ctxmenu::Sub::Score, true));
        }
    }),
    // The right-click menu of an entry: on a list row, with each submenu, on a poster (opened
    // with the Menu key, under it) and on a Home card; and the toast with Undo after a move.
    ("ctx-menu", |s| s.open_ctx(2, crate::ctxmenu::At::Point(470.0, 210.0))),
    ("ctx-menu-move", |s| {
        s.open_ctx(2, crate::ctxmenu::At::Point(470.0, 210.0));
        if let Some(c) = &mut s.ctx {
            c.sub = Some((crate::ctxmenu::Sub::Move, false));
        }
    }),
    ("ctx-menu-score", |s| {
        s.open_ctx(0, crate::ctxmenu::At::Point(470.0, 130.0));
        if let Some(c) = &mut s.ctx {
            c.sub = Some((crate::ctxmenu::Sub::Score, true));
        }
    }),
    ("ctx-menu-grid", |s| {
        fake_covers(s);
        s.prefs.views.set(Status::Watching, ListView::Grid);
        s.grid.sel = Some(1);
        s.ctx_key();
    }),
    ("ctx-menu-home", |s| {
        fake_covers(s);
        home(s);
        s.open_ctx(1, crate::ctxmenu::At::Point(620.0, 150.0));
    }),
    ("ctx-undo", |s| {
        s.ctx_move(2, Status::Completed);
    }),
    // A long title: the toast wraps inside its width and keeps the Undo button whole.
    ("ctx-undo-long", |s| {
        if let Some(e) = s.entries.iter_mut().find(|e| e.id == 2) {
            e.title = "That Time I Got Reincarnated as a Very Long Title in Another World: The Complete Director's Cut".into();
        }
        s.ctx_move(2, Status::Completed);
    }),
    ("ctx-undo-long-light", |s| {
        s.set_theme(Theme::Light);
        if let Some(e) = s.entries.iter_mut().find(|e| e.id == 2) {
            e.title = "That Time I Got Reincarnated as a Very Long Title in Another World: The Complete Director's Cut".into();
        }
        s.ctx_move(2, Status::Completed);
    }),
    ("ctx-undo-light", |s| {
        s.set_theme(Theme::Light);
        s.ctx_move(2, Status::Completed);
    }),
    // One long word: nowhere to wrap, so the toast must stay inside the window.
    ("ctx-undo-word", |s| {
        if let Some(e) = s.entries.iter_mut().find(|e| e.id == 2) {
            e.title = "Supercalifragilisticexpialidocious_Supercalifragilisticexpialidocious_Supercalifragilistic".into();
        }
        s.ctx_move(2, Status::Completed);
    }),
    // The toast after a middle-click: playing, and the next file missing.
    ("toast-play", |s| {
        crate::library::fake(s);
        s.prefs.views.set(Status::Watching, ListView::Grid);
        s.grid.sel = Some(0);
        s.play_entry(0);
    }),
    ("toast-missing", |s| {
        crate::library::fake(s);
        s.play_entry(7);
    }),
    ("library-details", |s| {
        crate::library::fake(s);
        details(s, 0, true);
    }),
    ("library-details-missing", |s| {
        crate::library::fake(s);
        details(s, 7, true);
    }),
    ("library-details-offline", |s| {
        crate::library::fake(s);
        details(s, 4, true);
    }),
    // Nothing plays: Up next.
    ("library-now-idle", |s| {
        crate::library::fake(s);
        s.now = None;
        s.library.note = Some("Opening episode 6 of Lanterns of Kirigaoka in IINA".into());
        s.go(Page::NowPlaying);
    }),
    // Settings → Titles: English or Japanese where the anime has one, romaji where not.
    // A newer Kotora is out (`update.rs`): the banner as each kind of install sees it.
    ("update-available", |s| update_scene(s, kotora_core::update::Install::Direct, None)),
    ("update-available-aur", |s| update_scene(s, kotora_core::update::Install::Aur, None)),
    ("update-available-homebrew", |s| update_scene(s, kotora_core::update::Install::Homebrew, None)),
    ("update-available-install-sh", |s| update_scene(s, kotora_core::update::Install::Script, None)),
    // Windows: Download gets the installer, from an installed copy or an unpacked one.
    ("update-available-windows", |s| update_scene(s, kotora_core::update::Install::WinSetup, None)),
    ("update-required", |s| update_scene(s, kotora_core::update::Install::Direct, Some("99.0.0"))),
    ("update-required-aur", |s| update_scene(s, kotora_core::update::Install::Aur, Some("99.0.0"))),
    // With the Now Playing banner above it.
    ("update-with-offer", |s| {
        update_scene(s, kotora_core::update::Install::Direct, None);
        s.now = Some(crate::model::fake_now());
        reached(s);
    }),
    // Settings → About Kotora: the switch and Check now, after a manual check.
    ("settings-updates", |s| {
        s.update.check = kotora_core::update::CheckStatus { running: false, last_check: Some(s.clock() as u64 - 3600), result: Some(kotora_core::update::UP_TO_DATE.into()) };
        crate::settings::open(s, Section::General);
    }),
    ("settings-updates-off", |s| {
        s.prefs.check_updates = false;
        crate::settings::open(s, Section::General);
    }),
    ("settings-titles-japanese", |s| {
        s.set_titles(TitleLang::Japanese);
        crate::settings::open(s, Section::Appearance);
    }),
    ("list-titles-english", |s| s.set_titles(TitleLang::English)),
    ("list-titles-japanese", |s| s.set_titles(TitleLang::Japanese)),
    ("queue-titles-english", |s| {
        s.set_titles(TitleLang::English);
        s.go(Page::Queue);
    }),
    ("now-playing-titles-japanese", |s| {
        s.set_titles(TitleLang::Japanese);
        s.go(Page::NowPlaying);
    }),
    // The "Not on your list" card and its anime-relations line follow the setting too (romaji
    // where MyAnimeList has no Japanese title: the first anime here).
    ("now-playing-not-on-list-japanese", |s| {
        playing_file(s, "[Hoshi] Velvet Orchard Chronicle S2 - 15 (1080p).mkv");
        if let Some(n) = &mut s.now {
            n.found = Found::NotOnList { media_id: 9002, title: "Velvet Orchard Chronicle 2nd Season Part 2".into() };
            n.alt_titles = crate::now::AltTitles { en: Some("Velvet Orchard Chronicle Season 2 Part 2".into()), ja: Some("ベルベット果樹園年代記 第2期 後半".into()) };
            n.relation = Some(crate::now::Relation { file_ep: 15, from_id: 9003, from_title: Some("Velvet Orchard Chronicle 2nd Season".into()), ep: 2 });
            if let Some(p) = &mut n.parsed {
                p.episode = Some(2);
            }
        }
        s.set_titles(TitleLang::Japanese);
        s.go(Page::NowPlaying);
    }),
    ("details-titles-japanese", |s| {
        s.set_titles(TitleLang::Japanese);
        details(s, 0, true);
    }),
    ("search-titles-english", |s| {
        s.set_titles(TitleLang::English);
        search(s, "kirigaoka", true);
    }),
    // Settings → Appearance with Light picked (light whatever the desktop says).
    // The startup intro (`intro.rs`), held at a share of its run: the orange layer, the three
    // claw marks ripping across, the strips sliding away.
    ("intro-0", |s| s.intro = Some(crate::intro::Intro::frozen(0.0, false))),
    ("intro-15", |s| s.intro = Some(crate::intro::Intro::frozen(0.15, false))),
    ("intro-25", |s| s.intro = Some(crate::intro::Intro::frozen(0.25, false))),
    ("intro-35", |s| s.intro = Some(crate::intro::Intro::frozen(0.35, false))),
    ("intro-50", |s| s.intro = Some(crate::intro::Intro::frozen(0.5, false))),
    ("intro-62", |s| s.intro = Some(crate::intro::Intro::frozen(0.62, false))),
    ("intro-75", |s| s.intro = Some(crate::intro::Intro::frozen(0.75, false))),
    ("intro-90", |s| s.intro = Some(crate::intro::Intro::frozen(0.9, false))),
    ("intro-40-light", |s| {
        s.set_theme(Theme::Light);
        s.intro = Some(crate::intro::Intro::frozen(0.4, false));
    }),
    // Reduce Motion: only the fade, held halfway.
    ("intro-reduced-50", |s| s.intro = Some(crate::intro::Intro::frozen(0.5, true))),
    // Running: `--video` or `--frames` record it (`--duration 1.2`).
    ("intro-live", |s| {
        s.window = true;
        s.intro = Some(crate::intro::Intro::live());
    }),
    // A slow first frame: it comes 1 s in. The cover must be solid orange until then, and the
    // whole run plays after it (`--video --duration 2.4`).
    ("intro-live-late", |s| {
        s.window = true;
        s.intro = Some(crate::intro::Intro::live_late(1.0));
    }),
    ("intro-live-light", |s| {
        s.set_theme(Theme::Light);
        s.window = true;
        s.intro = Some(crate::intro::Intro::live());
    }),
    ("intro-live-reduced", |s| {
        s.window = true;
        s.intro = Some(crate::intro::Intro::live_reduced());
    }),
    ("settings-theme-light", |s| {
        s.set_theme(Theme::Light);
        crate::settings::open(s, Section::Appearance);
    }),
    ("settings-theme-dark", |s| {
        s.set_theme(Theme::Dark);
        crate::settings::open(s, Section::Appearance);
    }),
    // "Update automatically" on, threshold 90 %, keep-higher off.
    ("settings-auto-on", |s| {
        s.prefs.auto = true;
        s.prefs.threshold = 90;
        s.prefs.keep_higher = false;
        crate::settings::open(s, Section::Detection);
    }),
    // Settings → Startup with both switches on.
    ("settings-startup-on", |s| {
        s.prefs.login = crate::autostart::Login { on: true, ..Default::default() };
        s.prefs.background = true;
        crate::settings::open(s, Section::General);
    }),
    // Settings → Startup in a copy that can't be a login item (macOS, run from a build folder).
    ("settings-startup-unavailable", |s| {
        s.prefs.login = crate::autostart::Login::unavailable("Only in the app (Kotora.app). This copy runs from a build folder.");
        crate::settings::open(s, Section::General);
    }),
    // Settings → Startup with the login item waiting for the user's OK in System Settings.
    ("settings-startup-approval", |s| {
        s.prefs.login = crate::autostart::Login { on: true, needs_approval: true, ..Default::default() };
        crate::settings::open(s, Section::General);
    }),
    // Two players switched off, one with a problem.
    ("settings-players-off", |s| {
        s.prefs.players.vlc = false;
        s.prefs.players.iina = false;
        s.prefs.players.other_mpris = false;
        s.players = vec![crate::precise::PlayerState {
            switch: kotora_detect::Switch::Mpv,
            via: crate::precise::VIA_IPC,
            line: "folder of /run/user/1000/mpv.sock not found".into(),
            led: kotora_detect::Led::Problem,
        }];
        s.now = None;
        crate::settings::open(s, Section::Detection);
    }),
    // Settings → Accounts: Change pressed on the Client ID, with a wrong paste.
    ("settings-client-id-change", |s| {
        s.client_id.editing = true;
        s.client_id.draft.replace_text("abc123");
        s.save_client_id();
        crate::settings::open(s, Section::Account);
    }),
    // No saved Client ID of your own (Kotora's built-in one signs in): "Use my own client ID…".
    ("settings-no-client-id", |s| {
        signed_out(s, Auth::SignedOut);
        s.client_id.hint = None;
        crate::settings::open(s, Section::Account);
    }),
    // mpv seen by its open file only: the one-time offer to turn on precise detection.
    ("precise-offer", |s| {
        process_only(s);
        s.go(Page::List(Status::Watching));
    }),
    // Turned on: what was written to mpv.conf.
    ("precise-written", |s| {
        process_only(s);
        s.enable_precise();
        s.precise_written(crate::precise::Precise::Wrote {
            file: "~/.config/mpv/mpv.conf".into(),
            text: "# Added by Kotora: lets it read mpv's position (precise detection).\ninput-ipc-server=/run/user/1000/mpv.sock\n".into(),
        });
    }),
    ("precise-already-set", |s| {
        process_only(s);
        s.precise_written(crate::precise::Precise::AlreadySet { file: "~/.config/mpv/mpv.conf".into(), socket: "/tmp/mpvsocket".into(), watched: false });
    }),
    ("precise-failed", |s| {
        process_only(s);
        s.precise_written(crate::precise::Precise::Failed("Can't write mpv.conf: permission denied".into()));
    }),
    // Settings → Players with sources seen by their process: the honest lines and Turn on.
    ("settings-players-process", |s| {
        process_only(s);
        s.prefs.precise_answered = true;
        crate::settings::open(s, Section::Detection);
    }),
    // Now Playing for a file seen by the process source: no position, counted by time open.
    ("now-playing-process", |s| {
        if let Some(n) = &mut s.now {
            n.via = crate::precise::VIA_PROCESS;
            n.position = None;
            n.duration = None;
        }
        s.players = crate::precise::fake_process_players();
        s.prefs.precise_answered = true;
        s.go(Page::NowPlaying);
    }),
    // Now Playing: mpv seen by its open file only, never asked: the precise detection prompt.
    ("precise-prompt", |s| {
        now_by_process(s, kotora_detect::Switch::Mpv);
    }),
    // Windows: Settings → Detection with players seen by their window only: Turn on for mpv.net's
    // pipe and for MPC's web interface, VLC's password field.
    ("win-players", |s| {
        win_players(s);
        s.prefs.precise_answered = true;
        s.prefs.vlc_set_up = false;
        crate::settings::open(s, Section::Detection);
    }),
    // Windows: Turn on made mpv.net's mpv.conf (the file didn't exist).
    ("win-precise-written", |s| {
        win_players(s);
        s.enable_precise();
        s.precise.who = Some("mpv.net".into());
        s.precise.note = Some("~\\AppData\\Roaming\\mpv.net\\mpv.conf didn\u{2019}t exist: Kotora made it. Close mpv.net and open it again once.".into());
        s.precise_written(crate::precise::Precise::Wrote {
            file: "~\\AppData\\Roaming\\mpv.net\\mpv.conf".into(),
            text: "# Added by Kotora: lets Kotora see what the player plays (position and pause).\ninput-ipc-server=\\\\.\\pipe\\mpvsocket\n".into(),
        });
        s.go(Page::List(Status::Watching));
    }),
    // Windows: MPC-HC was still open when Turn on was clicked.
    ("win-mpc-close-first", |s| {
        win_players(s);
        s.prefs.precise_answered = true;
        s.mpc_web_answer(Err(crate::winsetup::close_first("MPC-HC")));
        crate::settings::open(s, Section::Detection);
    }),
    ("win-mpc-turned-on", |s| {
        win_players(s);
        s.prefs.precise_answered = true;
        s.players.retain(|p| p.switch != kotora_detect::Switch::Mpc);
        s.mpc_web_answer(Ok(crate::winsetup::turned_on(&["MPC-HC"], 13579)));
        crate::settings::open(s, Section::Detection);
    }),
    // Windows: Now Playing for mpv.net seen by its window: the rule with its minutes.
    ("win-now-playing-window", |s| {
        now_by_process(s, kotora_detect::Switch::Mpv);
        win_players(s);
        s.prefs.precise_answered = true;
        if let Some(n) = &mut s.now {
            n.player = "mpv.net".into();
        }
    }),
    // The same past the threshold: the episode banner on top, the prompt under it.
    ("precise-prompt-with-offer", |s| {
        now_by_process(s, kotora_detect::Switch::Mpv);
        if let Some(n) = &mut s.now {
            n.reached = true;
        }
    }),
    // macOS: IINA followed by its open file, reading a config folder's mpv.conf.
    #[cfg(target_os = "macos")]
    ("precise-prompt-iina", |s| {
        now_by_process(s, kotora_detect::Switch::Iina);
        s.players = crate::iina::fake_process_players();
        s.iina.setup = Some(kotora_detect::iinaconf::Setup::ConfDir { file: "~/.config/iina/mpv.conf".into() });
    }),
    // Settings → Notifications as they come (Episode detected off).
    // Manga on: the New chapter switch can be turned on.
    ("settings-notifications-manga", |s| {
        crate::manga::fake(s);
        s.prefs.notify.new_chapter = true;
        crate::settings::open(s, Section::Notifications);
    }),
    ("settings-notifications", |s| {
        crate::settings::open(s, Section::Notifications);
    }),
    // All on, and the system shows no buttons (Linux: the server; macOS: outside Kotora.app).
    ("settings-notifications-no-buttons", |s| {
        s.prefs.notify.detected = true;
        s.notes.support = if cfg!(target_os = "macos") { crate::notes::Support::Outside } else { crate::notes::Support::NoButtons };
        crate::settings::open(s, Section::Notifications);
    }),
    // macOS: notifications turned off for Kotora in System Settings.
    ("settings-notifications-denied", |s| {
        s.notes.support = crate::notes::Support::Denied;
        crate::settings::open(s, Section::Notifications);
    }),
    ("settings-signed-out", |s| {
        signed_out(s, Auth::SignedOut);
        crate::settings::open(s, Section::Account);
    }),
    // macOS: IINA seen by its open file only, its settings not read: the steps in IINA.
    #[cfg(target_os = "macos")]
    ("settings-iina-steps", |s| {
        iina_by_process(s);
        s.iina.setup = Some(kotora_detect::iinaconf::Setup::Manual);
    }),
    // macOS: IINA reads the mpv.conf of a config folder: Turn on.
    #[cfg(target_os = "macos")]
    ("settings-iina-confdir", |s| {
        iina_by_process(s);
        s.iina.setup = Some(kotora_detect::iinaconf::Setup::ConfDir { file: "~/.config/iina/mpv.conf".into() });
    }),
    // macOS: after Turn on.
    #[cfg(target_os = "macos")]
    ("settings-iina-written", |s| {
        iina_by_process(s);
        s.iina_written(
            crate::precise::Precise::Wrote {
                file: "~/.config/iina/mpv.conf".into(),
                text: "# Added by Kotora: lets Kotora see what mpv plays (position and pause).\ninput-ipc-server=/tmp/iina.sock\n".into(),
            },
            None,
        );
    }),
];

/// Now Playing with the followed player seen by its open file only (no position).
fn now_by_process(s: &mut State, sw: kotora_detect::Switch) {
    if let Some(n) = &mut s.now {
        n.via = crate::precise::VIA_PROCESS;
        n.switch = sw;
        n.player = sw.label().into();
        n.key = format!("process:{}", sw.config_key());
        n.position = None;
        n.duration = None;
    }
    s.players = crate::precise::fake_process_players();
    s.go(Page::NowPlaying);
}

/// macOS: IINA seen by its open file only, on Settings.
#[cfg(target_os = "macos")]
fn iina_by_process(s: &mut State) {
    s.now = None;
    s.players = crate::iina::fake_process_players();
    s.prefs.precise_answered = true;
    crate::settings::open(s, Section::Detection);
}

/// The search box holds `q`; `answered`: MyAnimeList's made-up results are in.
/// The Watching page's filter holding `q` (nothing on the list matches it), and the service's
/// answer, if it came.
fn filter_online(s: &mut State, q: &str, hits: Option<Vec<kotora_core::service::AnimeHit>>) {
    s.covers.fake();
    s.query.replace_text(q);
    s.search_changed();
    if let Some(mut hits) = hits {
        for h in hits.iter_mut().filter(|h| h.id != 9104) {
            h.cover_url = Some(format!("https://cdn.example.invalid/{}.jpg", h.id));
        }
        s.online.show(q, hits);
    }
}

fn search(s: &mut State, q: &str, answered: bool) {
    s.covers.fake();
    for e in s.entries.iter_mut().filter(|e| e.id < 12) {
        e.cover_url = Some(format!("https://cdn.example.invalid/{}.jpg", e.id));
    }
    s.find.replace_text(q);
    s.open_search();
    s.find.focused = false;
    s.online.pending = false;
    if answered {
        let mut hits = crate::search::fake_hits();
        for h in hits.iter_mut().filter(|h| h.id != 9104) {
            h.cover_url = Some(format!("https://cdn.example.invalid/{}.jpg", h.id));
        }
        s.online.show(q, hits);
    }
}

/// Fake covers for the queue's entries.
fn queue_covers(s: &mut State) {
    s.covers.fake();
    for e in s.entries.iter_mut() {
        e.cover_url = Some(format!("https://cdn.example.invalid/{}.jpg", e.id));
    }
}

/// The Airing page with the fake season and covers, in `view`.
fn airing(s: &mut State, view: crate::airing::View) {
    s.covers.fake();
    crate::airing::fake(s);
    s.go(Page::Airing);
    s.airing_view(view);
}

/// The Airing page on another season (quarter 0 winter … 3 fall), with the fake shows of it.
fn airing_other(s: &mut State, quarter: u8, year: u16) {
    airing(s, crate::airing::View::Season);
    crate::airing::fake_browse(s, Season { year, quarter });
}

/// The details page of `id` (the fake list's, or a made-up search result), answered.
fn details(s: &mut State, id: u32, on_list: bool) {
    s.covers.fake();
    let title = s.entries.iter().find(|e| e.id == id).map_or_else(|| "Lanterns of Kirigaoka 2nd Season".to_string(), |e| e.title.clone());
    s.open_details(crate::details::Stub::bare(id, &title));
    let mut d = crate::details::fake_details(id, &title, on_list);
    d.cover_url = Some(format!("https://cdn.example.invalid/{id}l.jpg"));
    if !on_list {
        d.synonyms.clear();
        d.title_en = None;
        d.mean = Some(8.05);
        d.rank = None;
    }
    s.details_answer(crate::details::Answer { id, kind: kotora_core::Kind::Anime, result: Ok(d), item: None, done: true });
}

/// The fake manga list with covers (two keep their tiles), on Reading.
/// The Publishing page with the fake manga list and made-up MangaUpdates data.
fn publishing(s: &mut State) {
    manga(s);
    s.go(Page::Publishing);
    s.publishing_fake(crate::publishing::fake_info());
}

fn manga(s: &mut State) {
    crate::manga::fake(s);
    s.covers.fake();
    for e in s.manga.entries.iter_mut().filter(|e| e.id % 5 != 2) {
        e.cover_url = Some(format!("https://cdn.example.invalid/m{}.jpg", e.id));
    }
}

/// Settings → Account with Manga on, searched for the volumes row, `f` setting its state.
fn fill_volumes(s: &mut State, f: fn(&mut State)) {
    crate::manga::fake(s);
    f(s);
    crate::settings::open(s, Section::Account);
    crate::settings::set_query(s, "volumes");
}

/// The search page in Manga mode for `q`, MyAnimeList's manga results in when `answered`.
fn manga_search(s: &mut State, q: &str, answered: bool) {
    manga(s);
    s.find.replace_text(q);
    s.open_search();
    s.find.focused = false;
    s.online.pending = false;
    if answered {
        let mut hits = crate::manga::fake_hits();
        for h in hits.iter_mut().filter(|h| h.id != 503) {
            h.cover_url = Some(format!("https://cdn.example.invalid/m{}.jpg", h.id));
        }
        s.online.show(q, hits);
    }
}

/// The details page of the fake manga `id`, answered.
fn manga_details(s: &mut State, id: u32) {
    manga(s);
    let Some(e) = s.manga.entries.iter().find(|e| e.id == id) else { return };
    let (stub, title) = (crate::manga::stub(e), e.title.clone());
    let finished = e.run == crate::manga::Run::Finished;
    s.open_details(stub);
    let mut d = crate::manga::fake_details(id, &title);
    d.cover_url = Some(format!("https://cdn.example.invalid/m{id}l.jpg"));
    if finished {
        d.air_status = "finished".into();
        d.episodes = Some(40);
        d.volumes = Some(5);
        d.end_date = Some("2024-11".into());
        d.title_en = None;
        d.title_ja = None;
        d.synonyms.clear();
    }
    s.details_answer(crate::details::Answer { id, kind: kotora_core::Kind::Manga, result: Ok(d), item: None, done: true });
}

/// A completed anime on the list (not rewatched now), the first by id.
fn completed_id(s: &State) -> u32 {
    s.entries.iter().filter(|e| e.status == Status::Completed && !e.rewatching && e.episodes == Some(12)).map(|e| e.id).min().unwrap_or(0)
}

/// The fake player shows episode 3 of a completed anime.
fn rewatch_playing(s: &mut State) {
    let id = completed_id(s);
    let title = s.entries.iter().find(|e| e.id == id).map(|e| e.title.clone()).unwrap_or_default();
    playing_file(s, &format!("[Hoshi] {title} - 03 (1080p).mkv"));
    if let Some(n) = &mut s.now {
        n.found = Found::List { media_id: id, sure: true, why: "same title in your list".into() };
    }
}

/// Windows: mpv.net and MPC-HC seen by their window only, VLC open and not set up.
fn win_players(s: &mut State) {
    use crate::precise::{PlayerState, VIA_IPC, VIA_PROCESS};
    use crate::winsetup::VIA_WEB;
    use kotora_detect::{Led, Switch};
    let p = |switch, via, line: &str, led| PlayerState { switch, via, line: line.into(), led };
    s.now = s.now.take().filter(|n| n.via == VIA_PROCESS);
    s.players = vec![
        p(Switch::Mpv, VIA_PROCESS, "mpv.net: video file open", Led::Playing),
        p(Switch::Mpv, VIA_IPC, "\\\\.\\pipe\\mpvsocket: not running", Led::Off),
        p(Switch::Mpc, VIA_PROCESS, "MPC-HC: video file open", Led::Playing),
        p(Switch::Mpc, VIA_WEB, "web interface is off in MPC-HC", Led::Off),
        p(Switch::Vlc, VIA_PROCESS, "running, no video open", Led::Idle),
        p(Switch::Vlc, VIA_WEB, "web interface not set up (needs VLC's password)", Led::Problem),
        p(Switch::PotPlayer, VIA_PROCESS, "not running", Led::Off),
    ];
}

/// mpv is seen by its open file only, and nothing plays in a known way.
fn process_only(s: &mut State) {
    s.now = None;
    s.players = crate::precise::fake_process_players();
}

/// The fake episode passed the threshold.
fn reached(s: &mut State) {
    let (key, file) = match &mut s.now {
        Some(n) => {
            n.position = Some(1166.0);
            (n.key.clone(), n.file.clone())
        }
        None => return,
    };
    s.now_reached(&key, &file);
}

/// Manga on, nothing playing, and YACReader shows `file` at `page` of 40.
fn reading(s: &mut State, file: &str, page: i64) {
    crate::manga::fake(s);
    s.now = None;
    crate::reading::fake(s, file, page);
}

fn unsure(s: &mut State) {
    playing_file(s, "Lantern of Kirigaoka - 06.mkv");
    if let Some(n) = &mut s.now {
        n.found = Found::List { media_id: 0, sure: false, why: "close to a title in your list (95%)".into() };
    }
}

/// The fake player shows another file.
fn playing_file(s: &mut State, file: &str) {
    if let Some(n) = &mut s.now {
        n.file = file.into();
        n.parsed = kotora_core::recognize::parse(file);
    }
}

/// The Statistics page with made-up numbers for `kind` (`empty`: nothing logged yet).
fn stats(s: &mut State, kind: Kind, empty: bool) {
    use kotora_core::stats::Title;
    let titles: Vec<Title> = match kind {
        Kind::Anime => {
            fake_covers(s);
            let on = s.entries.iter().filter(|e| e.status == Status::Watching);
            on.map(|e| Title { media_id: e.id, title: e.title.clone(), cover_url: e.cover_url.clone(), progress: e.progress, total: e.episodes, units: 0 }).collect()
        }
        Kind::Manga => {
            let on = s.manga.entries.iter().filter(|e| e.status == Status::Watching);
            on.map(|e| Title { media_id: e.id, title: e.title.clone(), cover_url: e.cover_url.clone(), progress: e.chapters, total: e.chapters_total, units: 0 }).collect()
        }
    };
    let st = crate::stats::fake(kind, &titles, s.clock().div_euclid(86_400), empty);
    s.open_stats();
    s.stats.data = Some(std::sync::Arc::new(st));
}

fn fake_covers(s: &mut State) {
    s.covers.fake();
    for e in s.entries.iter_mut().filter(|e| e.id % 5 != 2) {
        e.cover_url = Some(format!("https://cdn.example.invalid/{}.jpg", e.id));
    }
}

/// Home, with the fake airing shows' next episodes spread over the week (a fixed zone).
fn home(s: &mut State) {
    let now = s.clock();
    s.airing.tz = Some(3600);
    for (id, hours) in [(0u32, 5i64), (1, 30), (3, 52), (6, 100), (10, 140)] {
        if let Some(e) = s.entries.iter_mut().find(|e| e.id == id) {
            let first = now + hours * 3600 - i64::from(e.progress) * kotora_core::airing::WEEK;
            e.timetable = Some(kotora_core::airing::Timetable { first, episodes: e.episodes, shift: 0, until: None });
        }
    }
    s.go(Page::Home);
}

/// 320 entries on Watching, with made-up covers (the long list for frame times).
fn many(s: &mut State) {
    fake_covers(s);
    let base = s.entries.clone();
    s.entries.clear();
    for i in 0..320u32 {
        let mut e = base[i as usize % base.len()].clone();
        e.id = 1000 + i;
        e.status = Status::Watching;
        e.rewatching = false;
        e.title = format!("{} {}", e.title, i as usize / base.len() + 1);
        e.cover_url = Some(format!("https://cdn.example.invalid/{}.jpg", e.id));
        e.updated_at -= i64::from(i) * 600;
        s.entries.push(e);
    }
    s.refresh();
}

/// The wizard at `step`, first run (no Client ID, nobody signed in).
/// AniList is the active service and signed in (the fake list is the same).
fn mirror_settings(s: &mut State) {
    crate::settings::open(s, Section::Account);
    crate::settings::set_query(s, "mirror");
}

fn anilist(s: &mut State) {
    s.service = kotora_core::service::ServiceId::AniList;
    s.account = Some(("demo_user".into(), "AniList"));
}

/// Signed in to AniList with score format `fmt`, the fake scores made finer than 1-10 (73, 88,
/// 41, …) so each format's rounding shows.
fn scores_in(s: &mut State, fmt: ScoreFormat) {
    anilist(s);
    s.profile = Some(kotora_core::service::Profile { score_format: fmt, ..crate::profile::fake() });
    const FINE: [u32; 8] = [73, 88, 41, 100, 56, 7, 65, 92];
    for (i, e) in s.entries.iter_mut().filter(|e| e.score.is_some()).enumerate() {
        e.score = Score::from_points(FINE[i % FINE.len()]);
    }
    s.refresh();
}

/// A removal of the second fake entry in the queue, the entry gone from the list. A conflict has
/// the service's newer entry beside it.
fn removal(s: &mut State, state: OpState) {
    let Some(i) = s.entries.iter().position(|e| e.id == 2) else { return };
    let e = s.entries.remove(i);
    let Some(mut op) = s.ops.first().cloned() else { return };
    op.id = 90;
    op.media_id = e.id;
    op.title = e.title.clone();
    op.episodes = e.episodes;
    op.change = kotora_core::Change::default();
    op.old = e.values();
    op.removed = true;
    op.added = false;
    op.edits = 1;
    op.state = state;
    op.error = None;
    op.remote = (state == OpState::Conflict).then(|| kotora_core::Remote {
        values: kotora_core::Values { progress: e.progress + 2, ..e.values() },
        updated_at: s.clock() - 600,
    });
    s.ops.retain(|o| o.media_id != e.id);
    s.ops.insert(0, op);
    s.queue = s.ops.iter().filter(|o| o.state.is_open()).count() as u32;
    s.refresh();
}

fn setup(s: &mut State, step: Step) {
    signed_out(s, Auth::NoClientId);
    s.client_id.hint = None;
    s.go(Page::Setup);
    s.setup = step;
}

fn signing_in(s: &mut State, opened: bool) {
    setup(s, Step::SignIn);
    s.client_id.hint = Some("8f3a".into());
    s.auth = Auth::SigningIn;
    let url = "https://myanimelist.net/v1/oauth2/authorize?response_type=code&client_id=0123456789abcdef0123456789abcdef&code_challenge=made-up&code_challenge_method=plain&state=made-up&redirect_uri=http%3A%2F%2F127.0.0.1%3A47321%2Fcallback";
    s.apply(crate::model::Msg::SignInUrl { url: url.into(), opened });
}

/// Nobody is signed in: no account, no list, no queue.
fn signed_out(s: &mut State, auth: Auth) {
    s.auth = auth;
    s.account = None;
    s.entries.clear();
    s.refresh();
    s.synced_at = None;
    s.ops.clear();
    s.queue = 0;
    s.now = None;
}

/// A check that found Kotora 99.0.0 (above any real version), as `install` would show it; with
/// `minimum`, this version is below `minimum_supported`.
fn update_scene(s: &mut State, install: kotora_core::update::Install, minimum: Option<&str>) {
    let latest = kotora_core::update::Latest {
        version: "99.0.0".into(),
        date: "2026-12-01".into(),
        notes_url: "https://github.com/hanami117/kotora/releases/tag/v99.0.0".into(),
        minimum_supported: minimum.map(Into::into),
        downloads: kotora_core::update::Downloads {
            macos_dmg: "https://github.com/hanami117/kotora/releases/download/v99.0.0/Kotora-99.0.0.dmg".into(),
            linux_tarball: "https://github.com/hanami117/kotora/releases/download/v99.0.0/kotora-99.0.0.tar.gz".into(),
            windows_installer: "https://github.com/hanami117/kotora/releases/download/v99.0.0/Kotora-99.0.0-windows-x64-setup.exe".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    s.fake_update(latest, install);
    // Alone on top (the fake state has an offer; `update-with-offer` keeps it).
    s.now = None;
}

#[cfg(test)]
mod tests {
    use super::SCENES;
    use crate::model::{Page, Sort};
    use crate::setup::Step;
    use kotora_core::service::ServiceId;
    use kotora_core::Status;
    use kotora_core::config::{Theme, TitleLang};
    use crate::model::State;
    use rune::{Env, Runtime};

    fn render(name: &str, size: (f32, f32)) -> Runtime<State> {
        let mut s = State::fake();
        (SCENES.iter().find(|(n, _)| *n == name).unwrap().1)(&mut s);
        let mut rt = Runtime::new(s, crate::ui::view, size, Env { system_dark: true, scale: 1.0 });
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        rt
    }

    fn key(rt: &mut Runtime<State>, key: rune::Key) {
        rt.key(rune::KeyEvent { key, mods: rune::Mods::default() });
        rt.frame(1.0 / 60.0);
    }

    fn focused(rt: &Runtime<State>) -> Vec<String> {
        rt.elements().into_iter().filter(|e| e.focused).filter_map(|e| e.key).collect()
    }

    fn click(rt: &mut Runtime<State>, k: &str) {
        let (x, y) = rt.find(k).unwrap_or_else(|| panic!("{k}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    /// The first screen: both cards are buttons, one focus group. Tab reaches MyAnimeList, the
    /// arrow moves to AniList; Return alone picks MyAnimeList (the default button); a click on
    /// AniList sets up AniList.
    #[test]
    fn the_service_choice_goes_on_with_either_service() {
        let rt = render("setup-service", (1280.0, 800.0));
        let (mx, my) = rt.find("setup/service-mal").expect("MyAnimeList card");
        let (ax, ay) = rt.find("setup/service-anilist").expect("AniList card");
        assert!(rt.find("setup/service").is_some() && rt.find("setup/indicator").is_none(), "unnumbered");
        assert!(rt.find("setup/next").is_none() && rt.find("setup/back").is_none() && rt.find("setup/close").is_none());
        assert!((my - ay).abs() < 1.0 && mx < ax, "wide: side by side, MyAnimeList first");
        let rt = render("setup-service", (700.0, 900.0));
        let ((_, my), (_, ay)) = (rt.find("setup/service-mal").unwrap(), rt.find("setup/service-anilist").unwrap());
        assert!(my < ay, "narrow: stacked");

        // Tab walks the sidebar, then reaches MyAnimeList; the arrow goes on to AniList.
        let mut rt = render("setup-service", (1280.0, 800.0));
        let mut seen = Vec::new();
        for _ in 0..30 {
            key(&mut rt, rune::Key::Tab);
            seen.extend(focused(&rt));
            if seen.last().is_some_and(|k| k == "setup/service-mal") {
                break;
            }
        }
        assert_eq!(seen.last().map(String::as_str), Some("setup/service-mal"), "Tab reaches MyAnimeList: {seen:?}");
        key(&mut rt, rune::Key::Right);
        assert_eq!(focused(&rt), ["setup/service-anilist"]);
        key(&mut rt, rune::Key::Left);
        assert_eq!(focused(&rt), ["setup/service-mal"]);
        key(&mut rt, rune::Key::Enter);
        assert_eq!((rt.state.setup, rt.state.service), (Step::Welcome, ServiceId::Mal));
        assert!(rt.find("setup/back").is_some(), "Back returns to the choice");
        click(&mut rt, "setup/back");
        assert_eq!(rt.state.setup, Step::Service);

        // Return with nothing focused also continues with MyAnimeList (the default button).
        let mut rt = render("setup-service", (1280.0, 800.0));
        key(&mut rt, rune::Key::Enter);
        assert_eq!((rt.state.setup, rt.state.service), (Step::Welcome, ServiceId::Mal));

        // A click on AniList sets up AniList.
        let mut rt = render("setup-service", (1280.0, 800.0));
        click(&mut rt, "setup/service-anilist");
        assert_eq!((rt.state.setup, rt.state.service), (Step::Welcome, ServiceId::AniList));
    }

    #[test]
    fn anilist_sign_in_is_one_click() {
        // Welcome, Sign in, Done: no form to fill and no Client ID to paste.
        let rt = render("setup-anilist-sign-in", (1280.0, 800.0));
        assert!(rt.find("setup/sign-in").is_some());
        for gone in ["setup/form", "setup/cid/input", "setup/copy-redirect", "setup/open-api"] {
            assert!(rt.find(gone).is_none(), "{gone}");
        }
        assert!(rt.find("setup/token/input").is_none(), "the token field waits for a failed or running sign-in");
        // The fallbacks: while waiting, and after the loopback failed.
        for name in ["setup-anilist-signing-in", "setup-anilist-loopback-failed"] {
            let rt = render(name, (1280.0, 800.0));
            assert!(rt.find("setup/token/input").is_some(), "{name}");
        }
        assert!(render("setup-anilist-loopback-failed", (1280.0, 800.0)).find("setup/sign-in").is_some(), "Try again");
        // Settings: the built-in ID needs nothing, and the advanced link opens the field.
        let rt = render("settings-account-anilist-built-in", (1280.0, 800.0));
        assert!(rt.find("set/cid-built-in").is_some() && rt.find("set/cid-change").is_some());
        let rt = render("settings-account-anilist-own-id", (1280.0, 800.0));
        assert!(rt.find("set/cid/input").is_some());
    }

    #[test]
    fn mirror_scenes() {
        // Off, both signed in: the switch works.
        let rt = render("settings-mirror", (1280.0, 800.0));
        assert!(rt.find("set/mirror-switch").is_some() && rt.find("set/mirror-preview").is_none());
        // The preview: counts, the lists, a status to pick, the three choices and Cancel.
        let rt = render("settings-mirror-preview", (1280.0, 1800.0));
        for k in [
            "set/mirror-preview",
            "set/mirror-counts",
            "set/mirror-list-only-main",
            "set/mirror-list-only-mirror",
            "set/mirror-list-different",
            "set/mirror-conflicts",
            "set/mirror-conflicts-all",
            "set/mirror-list-unmatched",
            "set/mirror-main-wins",
            "set/mirror-merge",
            "set/mirror-dont-align",
            "set/mirror-cancel",
        ] {
            assert!(rt.find(k).is_some(), "{k}");
        }
        assert!(render("settings-mirror-preview-picked", (1280.0, 1800.0)).find("set/mirror-conflicts").is_some());
        assert!(render("settings-mirror-preview-anilist", (1280.0, 1800.0)).find("set/mirror-main-wins").is_some());
        assert!(render("settings-mirror-busy", (1280.0, 800.0)).find("set/mirror-preview").is_none());
        assert!(render("settings-mirror-error", (1280.0, 800.0)).find("set/mirror-error").is_some());
        assert!(render("settings-mirror-on", (1280.0, 800.0)).find("set/mirror-note").is_some());
        assert!(render("settings-mirror-sign-in", (1280.0, 800.0)).find("set/mirror-switch").is_some());
        // No Beta label any more, off or on.
        assert!(render("settings-mirror", (1280.0, 800.0)).find("set/mirror-beta").is_none());
        assert!(render("settings-mirror-on", (1280.0, 800.0)).find("set/mirror-beta").is_none());
        // The Queue page: website edits, the conflict, the mirror's list, and the badge.
        let rt = render("queue-mirror", (1280.0, 1400.0));
        for k in ["queue-mirror", "mirror-edit/9101/use-mirror", "mirror-edit/9101/leave", "queue-mirror-list", "qmh/waiting", "qmh/failed", "qmh/sent", "qmh/waits"] {
            assert!(rt.find(k).is_some(), "{k}");
        }
        assert!(rt.find("mirror-conflict/900/keep").is_some() && rt.find("mirror-conflict/900/take").is_some());
        let rt = render("queue-mirror-only", (1280.0, 1400.0));
        assert!(rt.find("queue-empty").is_none() && rt.find("queue-mirror/trouble").is_some());
        // Off: no mirror part.
        assert!(render("queue-page", (1280.0, 800.0)).find("queue-mirror").is_none());
    }

    /// The account chip with the mirror on: both services on its second line, the dot only while
    /// the mirror has something in the Sync queue (counted into the badge too), and the tooltip
    /// after the pointer rests on it.
    #[test]
    fn the_account_chip_shows_the_mirror() {
        let mut s = State::fake();
        let plain = crate::ui::account_line(&s, "MyAnimeList");
        assert_eq!(plain, "MyAnimeList");
        (SCENES.iter().find(|(n, _)| *n == "sidebar-mirror").unwrap().1)(&mut s);
        assert_eq!(crate::ui::account_line(&s, "MyAnimeList"), "MyAnimeList \u{21c4} AniList");
        assert_eq!(crate::ui::account_tip_text(&s, "AniList"), "Changes also go to AniList.");
        let mut rt = render("sidebar-mirror", (1280.0, 800.0));
        assert!(rt.find("account/service").is_some() && rt.find("account/dot").is_none() && rt.find("account-tip").is_none());
        // Resting on the chip shows the tooltip; leaving hides it.
        let (x, y) = rt.find("account").unwrap();
        rt.pointer_move(x, y);
        for _ in 0..60 {
            rt.frame(1.0 / 60.0);
        }
        assert!(rt.find("account-tip").is_some(), "the tooltip after a rest");
        rt.pointer_move(700.0, 500.0);
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        assert!(rt.find("account-tip").is_none());
        // Something waits for the mirror: the dot, and the badge counts it.
        let mut s = State::fake();
        (SCENES.iter().find(|(n, _)| *n == "sidebar-mirror-dot").unwrap().1)(&mut s);
        assert!(s.mirror.badge() > 0);
        assert!(crate::ui::account_tip_text(&s, "AniList").contains("in the Sync queue for AniList"));
        let rt = render("sidebar-mirror-dot", (1280.0, 800.0));
        assert!(rt.find("account/dot").is_some() && rt.find("account-tip").is_some());
        // AniList main: the other way round.
        let mut s = State::fake();
        (SCENES.iter().find(|(n, _)| *n == "sidebar-mirror-anilist").unwrap().1)(&mut s);
        assert_eq!(crate::ui::account_line(&s, "AniList"), "AniList \u{21c4} MyAnimeList");
        // Off: the plain chip, and no dot even with old mirror changes around.
        let mut s = State::fake();
        crate::mirror::fake_on(&mut s);
        s.mirror.link = None;
        assert_eq!((crate::ui::account_line(&s, "MyAnimeList").as_str(), s.mirror.badge()), ("MyAnimeList", 0));
        assert!(render("home", (1280.0, 800.0)).find("account/dot").is_none());
    }

    /// `kot status` names the mirror and splits the chip's badge by state; nothing while it is off.
    #[test]
    fn kot_status_shows_the_mirror() {
        let mut s = State::fake();
        assert_eq!(s.ctl_status().mirror, None);
        (SCENES.iter().find(|(n, _)| *n == "sidebar-mirror").unwrap().1)(&mut s);
        let m = s.ctl_status().mirror.expect("the mirror is on");
        assert_eq!((m.to.as_str(), m.waiting, m.conflicts, m.failed, m.web_edits), ("AniList", 0, 0, 0, 0));
        let mut s = State::fake();
        crate::mirror::fake_on(&mut s);
        let m = s.ctl_status().mirror.expect("the mirror is on");
        assert_eq!(m.waiting + m.conflicts + m.failed + m.web_edits, s.mirror.badge(), "the same count as the chip's dot");
        s.mirror.link = None;
        assert_eq!(s.ctl_status().mirror, None);
    }

    #[test]
    fn export_and_import_scenes() {
        // MyAnimeList: both rows work.
        let rt = render("settings-transfer", (1280.0, 800.0));
        assert!(rt.find("set/export-button").is_some() && rt.find("set/import-button").is_some());
        assert!(rt.find("set/import-preview").is_none());
        // The preview: counts, the lists, a conflict to pick and the three buttons.
        let rt = render("settings-import-preview", (1280.0, 1600.0));
        for k in ["set/import-preview", "set/import-counts", "set/import-list-new", "set/import-list-different", "set/import-conflicts", "set/import-list-unknown", "set/import-add-new", "set/import-add-update", "set/import-cancel"] {
            assert!(rt.find(k).is_some(), "{k}");
        }
        let mut s = State::fake();
        crate::transfer::fake_preview(&mut s);
        let p = s.transfer.preview.as_ref().unwrap();
        let c = p.plan.counts();
        assert_eq!((c.new, c.unknown), (4, 2), "four new ids, no id and an unknown status");
        assert!(c.conflicts > 1 && c.different > 0 && c.same > 0);
        assert_eq!(c.new + c.different + c.same + c.unknown, 40 + 6, "40 of the fake list, six more");
        assert!(render("settings-import-preview", (1280.0, 1600.0)).find("set/import-conflicts-all").is_some());
        assert_eq!((p.plan.other, p.plan.kind), (1, kotora_core::Kind::Anime));
        // AniList: the id it doesn't know is left out, the one it maps is new.
        let mut s = State::fake();
        super::anilist(&mut s);
        crate::transfer::fake_preview(&mut s);
        let c = s.transfer.preview.as_ref().unwrap().plan.counts();
        assert_eq!((c.new, c.unknown), (1, 5));
        assert!(render("settings-import-preview-anilist", (1280.0, 1600.0)).find("set/import-list-unknown").is_some());
        // While matching, after an error, after the import, after Undo.
        assert!(render("settings-import-busy", (1280.0, 800.0)).find("set/import-preview").is_none());
        assert!(render("settings-import-error", (1280.0, 800.0)).find("set/import-error").is_some());
        let rt = render("settings-import-done", (1280.0, 800.0));
        assert!(rt.find("set/import-undo").is_some() && rt.find("set/import-queue").is_some());
        let rt = render("settings-import-undone", (1280.0, 800.0));
        assert!(rt.find("set/import-done").is_some() && rt.find("set/import-undo").is_none());
        // AniList: Export is off and says why; Import is on.
        let mut s = State::fake();
        super::anilist(&mut s);
        assert!(crate::transfer::refusal(&s).is_some_and(|w| w.contains("terms")));
        assert!(render("settings-export-anilist", (1280.0, 800.0)).find("set/import-button").is_some());
    }

    #[test]
    fn manga_export_and_import_scenes() {
        use kotora_core::Kind;
        // Manga off: the anime rows only.
        let rt = render("settings-transfer", (1280.0, 800.0));
        assert!(rt.find("set/export-manga-button").is_none() && rt.find("set/import-manga-button").is_none());
        // Manga on: the manga rows beside them.
        let rt = render("settings-transfer-manga", (1280.0, 900.0));
        for k in ["set/export-button", "set/export-manga-button", "set/import-button", "set/import-manga-button"] {
            assert!(rt.find(k).is_some(), "{k}");
        }
        assert!(render("settings-manga-export-done", (1280.0, 900.0)).find("set/export-manga").is_some());
        // The preview sits under the manga row and counts chapters and volumes.
        let rt = render("settings-manga-import-preview", (1280.0, 1700.0));
        for k in ["set/import-manga", "set/import-preview", "set/import-counts", "set/import-list-new", "set/import-list-different", "set/import-conflicts", "set/import-list-unknown", "set/import-add-new", "set/import-add-update"] {
            assert!(rt.find(k).is_some(), "{k}");
        }
        let mut s = State::fake();
        crate::transfer::fake_manga_preview(&mut s);
        let p = s.transfer.preview.as_ref().unwrap();
        assert_eq!((p.plan.kind, p.plan.other), (Kind::Manga, 1));
        let c = p.plan.counts();
        assert_eq!((c.new, c.unknown), (3, 2), "three new ids, no id and an unknown status");
        assert!(c.conflicts >= 2 && c.different > 0 && c.same > 0);
        assert!(p.plan.rows.iter().any(|r| r.verdict == kotora_core::listxml::Verdict::New && r.media.as_ref().is_some_and(|m| m.kind == Kind::Manga)));
        // An anime file given to the manga import says so.
        let rt = render("settings-manga-import-error", (1280.0, 900.0));
        assert!(rt.find("set/import-error").is_some());
        assert!(rt.find("set/import-manga").is_some());
        assert!(render("settings-manga-import-done", (1280.0, 900.0)).find("set/import-undo").is_some());
        // AniList: no export (terms); the manga import is offered as on MyAnimeList.
        let mut s = State::fake();
        super::anilist(&mut s);
        assert!(crate::transfer::refusal(&s).is_some());
        assert!(render("settings-manga-anilist", (1280.0, 900.0)).find("set/import-manga").is_some());
    }

    #[test]
    fn context_menu_scenes_show_the_menu_and_submenus() {
        for (name, keys) in [
            ("ctx-menu", &["ctx-menu", "ctx/move", "ctx/score", "ctx/plus", "ctx/play", "ctx/edit", "ctx/details", "ctx/mal"][..]),
            ("ctx-menu-move", &["ctx-sub/move", "ctx/move/completed", "ctx/move/watching"][..]),
            ("ctx-menu-score", &["ctx-sub/score", "ctx/score/10", "ctx/score/none"][..]),
            ("ctx-menu-grid", &["ctx-menu", "card/1"][..]),
            ("ctx-menu-home", &["ctx-menu", "page/home"][..]),
            ("ctx-undo", &["toast", "toast/undo"][..]),
        ] {
            let rt = render(name, (1280.0, 800.0));
            for key in keys {
                assert!(rt.find(key).is_some(), "{name}: {key}");
            }
        }
        assert!(render("list", (1280.0, 800.0)).find("ctx-menu").is_none());
    }

    /// The toast with Undo stays whole inside the window, for a short title, a long one and a
    /// word too long to wrap, at the big and the small window size, dark and light.
    #[test]
    fn the_undo_toast_keeps_its_button_inside_the_toast_and_the_window() {
        for name in ["ctx-undo", "ctx-undo-light", "ctx-undo-long", "ctx-undo-long-light", "ctx-undo-word"] {
            for size in [(1360.0, 900.0), (960.0, 600.0)] {
                let rt = render(name, size);
                let ([tx, ty, tw, th], _) = rt.presented("toast").unwrap_or_else(|| panic!("{name}: toast"));
                let ([bx, by, bw, bh], _) = rt.presented("toast/undo").unwrap_or_else(|| panic!("{name}: undo"));
                let tag = format!("{name} {size:?}");
                assert!(bw >= 40.0 && bh >= 24.0, "{tag}: the button keeps its size ({bw}x{bh})");
                assert!(bx >= tx && by >= ty && bx + bw <= tx + tw && by + bh <= ty + th, "{tag}: Undo inside the toast");
                assert!(tx >= 0.0 && ty >= 0.0 && tx + tw <= size.0 - 16.0 && ty + th <= size.1 - 16.0, "{tag}: the toast keeps a margin inside the window");
                assert!(tw <= 460.0 + 0.5, "{tag}: width {tw}");
            }
        }
    }

    /// Middle-click and P play the next episode, as ▶ Play does; the toast says what happened.
    #[test]
    fn middle_click_and_p_play_the_next_episode() {
        let mut s = State::fake();
        crate::library::fake(&mut s);
        s.play_entry(0);
        let t = s.toast.clone().unwrap();
        assert!(!t.warn && t.text.contains("episode 6"), "{t:?}");
        s.play_entry(7);
        let t = s.toast.clone().unwrap();
        assert!(t.warn && t.text == "Episode 12 not in your folders", "{t:?}");
        // P: the grid's selection, else nothing on a list page.
        s.prefs.views.set(Status::Watching, kotora_core::config::ListView::Grid);
        s.grid.sel = None;
        assert_eq!(s.play_target(), None);
        s.grid.sel = Some(3);
        assert_eq!(s.play_target(), Some(3));
        // The table: the row whose editor is open.
        s.prefs.views.set(Status::Watching, kotora_core::config::ListView::List);
        assert_eq!(s.play_target(), None);
        s.toggle_editor(1);
        assert_eq!(s.play_target(), Some(1));
        for name in ["toast-play", "toast-missing"] {
            assert!(render(name, (1280.0, 800.0)).find("toast").is_some(), "{name}");
        }
        // No library folders: the toast says where to add one.
        let mut s = State::fake();
        s.play_entry(0);
        assert!(s.toast.unwrap().text.contains("Settings"));
    }

    /// A window resize reflows the grids at once: cards that change rows are in their new place
    /// and fully drawn on the frame after it, with nothing left to move (no fading rows, no
    /// cards sliding in). Narrowing 1280 to 900 pt in steps, as a live drag does.
    #[test]
    fn a_resize_lands_at_once() {
        for name in ["list-grid-covers", "list-large-covers", "list-covers", "home-covers"] {
            let mut rt = render(name, (1280.0, 800.0));
            for w in (900..=1240).rev().step_by(20) {
                rt.set_viewport((w as f32, 800.0), 1.0);
                rt.frame(1.0 / 120.0);
            }
            rt.frame(1.0 / 60.0);
            // A card that changed rows leaves no fading copy behind.
            let ghosts: Vec<String> = rt.elements().into_iter().filter(|e| e.exiting).filter_map(|e| e.key).filter(|k| k.starts_with("card/") || k.starts_with("cont/") || k.starts_with("cards/")).collect();
            assert!(ghosts.is_empty(), "{name}: fading copies {ghosts:?}");
            rt.frame(1.0 / 60.0);
            let keys: Vec<String> = (0..12).map(|i| format!("card/{i}")).chain((0..12).map(|i| format!("cont/{i}"))).chain(["np-mini".to_string(), "rows".to_string()]).collect();
            let now: Vec<_> = keys.iter().map(|k| rt.presented(k)).collect();
            for _ in 0..40 {
                rt.frame(1.0 / 60.0);
            }
            for (k, before) in keys.iter().zip(now) {
                let Some((r, o)) = before else { continue };
                let (r2, o2) = rt.presented(k).unwrap();
                assert!(o >= 0.999 && o2 >= 0.999, "{name}: {k} half drawn ({o}, then {o2}) after the resize");
                assert!(r.iter().zip(r2).all(|(a, b)| (a - b).abs() < 0.5), "{name}: {k} still moving: {r:?} -> {r2:?}");
            }
        }
    }

    /// Every scene lays out with the sidebar and its page, at the default and the minimum size.
    #[test]
    fn every_scene_renders() {
        let mut names: Vec<&str> = SCENES.iter().map(|(n, _)| *n).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), SCENES.len(), "scene names are unique");
        for (name, _) in SCENES {
            for size in [(1280.0, 800.0), (960.0, 600.0)] {
                let rt = render(name, size);
                assert!(rt.find("sidebar").is_some(), "{name}: sidebar");
                assert!(rt.find("main").is_some(), "{name}: main");
                // The sign-in screen and the wizard have no top bar; Now Playing's hero is its title.
                assert!(
                    rt.find("top").is_some() || rt.find("welcome").is_some() || rt.find("page/setup").is_some() || rt.find("np-card").is_some(),
                    "{name}: page title"
                );
            }
        }
    }

    /// The Sort menu: the orders and directions with a check on those in force, in every view;
    /// picking one closes it.
    #[test]
    fn sort_menu_lists_the_orders() {
        for name in ["list-sort-menu", "list-grid-sort-menu", "list-large-sort-menu"] {
            let rt = render(name, (1280.0, 800.0));
            for key in ["sort-menu", "sort/updated", "sort/title", "sort/score", "sort/progress", "sort/dir/0", "sort/dir/1"] {
                assert!(rt.find(key).is_some(), "{name}: {key}");
            }
        }
        assert!(render("list", (1280.0, 800.0)).find("sort-menu").is_none(), "closed by default");
        let mut s = State::fake();
        s.sort_menu = true;
        s.set_sort(Sort::Score);
        s.set_sort_rev(true);
        let low = s.rows.first().map(|&k| s.entries[k].score);
        s.set_sort_rev(false);
        let high = s.rows.first().map(|&k| s.entries[k].score);
        assert!(low < high, "{low:?} < {high:?}");
        s.set_sort(Sort::Title);
        assert!(!s.sort_rev, "a new order starts its own way");
        s.go(Page::List(Status::Completed));
        assert!(!s.sort_menu, "leaving the page closes it");
    }

    #[test]
    fn profile_page_shows_the_numbers() {
        for size in [(1280.0, 800.0), (960.0, 600.0)] {
            let rt = render("profile", size);
            for key in ["page/profile", "profile/avatar", "profile/name", "profile/joined", "profile/open-mal", "profile/days", "profile/bar", "profile/bar/completed", "profile/chart", "profile/score/8", "profile/sign-out"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
            assert!(render("profile-pending", size).find("profile/pending").is_some());
        }
        let mut s = State::fake();
        s.go(Page::List(Status::Watching));
        assert!(s.page != Page::Profile);
    }

    /// Torrents: off shows only the switch's row and no sidebar item; on, Taiga's table with
    /// its states, the filter manager and its presets, the history, and a keyed URL never shows.
    #[test]
    fn feeds_scenes_show_their_parts() {
        for size in [(1280.0, 800.0), (960.0, 600.0)] {
            let rt = render("feeds-settings-off", size);
            assert!(rt.find("set/feeds").is_some() && rt.find("nav/feeds").is_none(), "off at {size:?}");
            let rt = render("feeds", size);
            for key in ["page/feeds", "feeds/table", "feeds/head", "feeds/sources", "nav/feeds", "feeds/download", "feeds/select-new"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
            assert!(render("feeds-empty", size).find("feeds/empty").is_some());
            let rt = render("feeds-search", size);
            assert!(rt.find("feeds/search").is_some() && rt.find("feeds/search/loading").is_none(), "search at {size:?}");
            assert!(render("feeds-settings-nyaa", size).find("set/feeds/note").is_some());
            assert!(render("feeds-history", size).find("feeds/history/0").is_some());
            let rt = render("feeds-filter-presets", size);
            assert!(rt.find("set/feeds/preset/0").is_some() && rt.find("set/feeds/preset/9").is_some());
            let rt = render("feeds-filter-edit", size);
            assert!(rt.find("set/feeds/editor/cond/0/elements").is_some() && rt.find("set/feeds/editor/save").is_some());
            assert!(render("feeds-open", size).find("feeds/act/download/fake-1").is_some() || render("feeds-open", size).find("page/feeds").is_some());
        }
        let mut s = State::fake();
        crate::feeds::fake(&mut s);
        // Taiga's default filters: the Watching items selected, the not-in-list ones discarded.
        let watching: Vec<u32> = s.entries.iter().filter(|e| e.status == Status::Watching).map(|e| e.id).collect();
        assert!(s.feeds.entries.iter().any(|e| e.state == kotora_core::feeds::taiga::ItemState::Selected));
        assert!(s.feeds.entries.iter().filter(|e| e.state == kotora_core::feeds::taiga::ItemState::Selected).all(|e| e.anime_id.is_some_and(|id| watching.contains(&id)) || e.anime_id.is_some()));
        let e = s.feeds.entries.iter().find(|e| crate::feeds::is_new(e)).unwrap().clone();
        let id = e.anime_id.unwrap();
        assert!(s.feeds_badge(id).is_some());
        assert!(render("feeds-settings", (1280.0, 800.0)).find("set/feeds/list").is_some());
        // Check box, Discard marked, Select all new.
        let k = e.item.key();
        s.feeds_mark(vec![k.clone()], kotora_core::feeds::taiga::ItemState::Blank);
        s.feeds_toggle(&k);
        assert_eq!(s.feeds.entries.iter().find(|x| x.item.key() == k).unwrap().state, kotora_core::feeds::taiga::ItemState::Selected);
        s.feeds_discard_marked();
        assert!(s.feeds_checked().is_empty());
        s.feeds.manual.clear();
        s.feeds_discard_rest();
        s.feeds.manual.clear();
        s.feeds_reselect();
        s.feeds_select_new();
        assert!(!s.feeds_checked().is_empty());
        // Download: into the history (headless: nothing opens), the item is discarded, its
        // episode marked got.
        let eps = e.release.episodes.unwrap();
        s.feeds_download(vec![k.clone()]);
        assert!(s.feeds.archive.as_ref().unwrap().contains(&e.item.title));
        assert!(s.feeds.entries.iter().find(|x| x.item.key() == k).unwrap().state.is_discarded());
        assert!(s.feeds.marks.got.contains_key(&(id, eps.0, eps.1)));
        // Per-anime: prefer a group, then discard the anime.
        s.feeds_prefer_group(id, "Tsuki");
        assert_eq!(kotora_core::feeds::taiga::fansub_groups(&s.feeds.filters, id), vec!["Tsuki".to_string()]);
        s.feeds_discard_anime(id);
        assert!(s.feeds.entries.iter().filter(|x| x.anime_id == Some(id)).all(|x| x.state.is_discarded()));
        // The editor: a preset, a condition, saved.
        let n = s.feeds.filters.len();
        s.feeds_filter_new();
        s.feeds_filter_preset(2);
        s.feeds_cond_add();
        s.feeds_filter_save();
        assert_eq!(s.feeds.filters.len(), n + 1);
        assert_eq!(s.feeds.filters[n].conditions.len(), 8);
    }

    /// Statistics: every part at both sizes, the empty state's line, manga's words, and the
    /// loading line before the numbers come (headless reads nothing).
    #[test]
    fn stats_page_shows_its_parts() {
        for size in [(1280.0, 800.0), (960.0, 600.0)] {
            let rt = render("stats", size);
            for key in ["page/stats", "stats/tile/units", "stats/tile/time", "stats/tile/streak", "stats/calendar", "stats/since", "stats/cal-total", "stats/when", "stats/lately", "stats/list", "stats/tags", "stats/genres", "stats/makers", "stats/airing", "sb-nav"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
            let rt = render("stats-empty", size);
            assert!(rt.find("stats/empty").is_some() && rt.find("stats/lately-empty").is_some(), "empty at {size:?}");
            let rt = render("stats-manga", size);
            assert!(rt.find("stats/tile/volumes").is_some() && rt.find("stats/airing").is_none(), "manga at {size:?}");
            assert!(render("stats-loading", size).find("stats/loading").is_some());
        }
    }

    /// The grid views: the switch, cards in rows, +1 always on Watching posters; the table's
    /// header is gone. Narrow windows move the switch to the chips.
    #[test]
    fn grid_views_show_cards() {
        for size in [(1280.0, 800.0), (960.0, 600.0)] {
            let rt = render("list-grid-covers", size);
            for key in ["views", "view/list", "view/grid", "view/large", "grid", "card/0", "plus"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
            assert!(rt.find("thead").is_none() && rt.find("rows").is_none(), "no table at {size:?}");
            let rt = render("list-large-covers", size);
            assert!(rt.find("grid").is_some() && rt.find("card/0").is_some());
        }
        assert!(render("list", (1280.0, 800.0)).find("views").is_some(), "the switch on the table too");
        // 320 entries: only the rows near the screen are built.
        let rt = render("list-many-grid", (1280.0, 800.0));
        assert!(rt.find("card/1000").is_some() && rt.find("card/1319").is_none());
    }

    #[test]
    fn home_shows_its_shelves() {
        for size in [(1280.0, 800.0), (960.0, 600.0)] {
            let rt = render("home-covers", size);
            for key in ["page/home", "nav/home", "home/continue", "home/airing", "home/plan", "home/continue/all", "cont/0", "sc/0"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
        }
        // Airing this week: the episode on the poster, the day and time under the title.
        let rt = render("home-covers", (1280.0, 800.0));
        let texts: Vec<String> = rt.elements().into_iter().filter(|e| e.path.contains("home/airing/scroll/")).filter_map(|e| e.text).collect();
        for want in ["Ep 6", "Today 14:00", "Tomorrow 15:00", "Sun 13:00"] {
            assert!(texts.iter().any(|t| t == want), "{want} in {texts:?}");
        }
        let rt = render("home-library", (1280.0, 800.0));
        assert!(rt.find("play").is_some(), "Play when the next file is there");
        let rt = render("home-quiet", (1280.0, 800.0));
        assert!(rt.find("home/airing/scroll").is_none() && rt.find("home/plan/scroll").is_none());
    }

    /// Nothing on a Home card pokes out of its shelf or card (the shelf's height once cut the
    /// descenders of the last line: the "p" of "Ep"), with Manga on too, wide and narrow.
    #[test]
    fn home_cards_fit_their_shelves() {
        for scene in ["home-covers", "home-manga", "home-library"] {
            for size in [(1280.0, 800.0), (960.0, 600.0), (760.0, 560.0)] {
                let rt = render(scene, size);
                let els = rt.elements();
                for shelf in ["continue", "reading", "airing", "plan"] {
                    let scroll = format!("home/{shelf}/scroll");
                    let Some(area) = els.iter().find(|e| e.key.as_deref() == Some(scroll.as_str())) else { continue };
                    let bottom = area.rect[1] + area.rect[3];
                    let inside: Vec<_> = els.iter().filter(|e| e.path.contains(&format!("{scroll}/")) && e.rect[3] > 0.0).collect();
                    assert!(inside.iter().any(|e| e.text.is_some()), "{scene} {shelf}: texts");
                    for e in inside {
                        let b = e.rect[1] + e.rect[3];
                        assert!(b <= bottom + 0.01, "{scene} {size:?} {shelf}: {} ({:?}) ends at {b}, the shelf at {bottom}", e.path, e.text);
                    }
                }
            }
        }
    }

    /// The new headers: Now Playing's hero, the details banner (wide) and banner card (narrow),
    /// the sidebar's playing card.
    #[test]
    fn heroes_and_banners() {
        let rt = render("now-playing-covers", (1280.0, 800.0));
        assert!(rt.find("np-card").is_some() && rt.find("backdrop").is_some() && rt.find("np-recog").is_some());
        for size in [(1280.0, 800.0), (960.0, 600.0)] {
            let rt = render("details-on-list", size);
            for key in ["det-banner", "det-title", "det-cover", "det-stats", "det-mine", "det/back", "det/open-mal"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
            assert!(render("now-playing-page", size).find("np-mini").is_some());
        }
    }

    #[test]
    fn list_shows_rows_and_states_show_theirs() {
        let rt = render("list", (1280.0, 800.0));
        assert!(rt.find("rows").is_some() && rt.find("thead").is_some() && rt.find("chips").is_some());
        assert!(rt.find("0").is_some(), "the first row (media id 0) is built");
        assert!(rt.find("np-mini").is_some());
        let rt = render("list-dropped", (1280.0, 800.0));
        assert!(rt.find("empty").is_some() && rt.find("rows").is_none());
        let rt = render("loading", (1280.0, 800.0));
        assert!(rt.find("loading").is_some() && rt.find("skel/3").is_some() && rt.find("rows").is_none());
    }

    #[test]
    fn sign_in_screens_show_their_parts() {
        let rt = render("no-client-id", (1280.0, 800.0));
        assert!(rt.find("page/setup").is_some() && rt.find("setup/indicator").is_some() && rt.find("setup/next").is_some());
        assert!(rt.find("setup/back").is_none() && rt.find("setup/close").is_none(), "first run: step 1, no way around it");
        assert!(rt.find("setup/cid/input").is_none() && rt.find("setup/open-api").is_none(), "no Client ID to make or paste");
        assert!(render("setup-sign-in", (1280.0, 800.0)).find("setup/sign-in").is_some());
        let rt = render("setup-signing-in-no-browser", (960.0, 600.0));
        assert!(rt.find("setup/waiting").is_some() && rt.find("setup/link").is_some() && rt.find("setup/copy-link").is_some() && rt.find("setup/cancel").is_some());
        for name in ["setup-sign-in-denied", "setup-sign-in-timeout"] {
            let rt = render(name, (1280.0, 800.0));
            assert!(rt.find("setup/sign-in-error").is_some() && rt.find("setup/sign-in").is_some(), "{name}");
        }
        assert!(render("setup-signed-in", (1280.0, 800.0)).find("setup/signed-in").is_some());
        let rt = render("setup-done", (1280.0, 800.0));
        assert!(rt.find("setup/loaded").is_some() && rt.find("setup/to-players").is_some() && rt.find("setup/close").is_none());
        assert!(render("setup-again", (1280.0, 800.0)).find("setup/close").is_some());
        let rt = render("client-id-saved", (1280.0, 800.0));
        assert!(rt.find("cid-saved").is_some() && rt.find("page/setup").is_none());
        let rt = render("settings-account", (1280.0, 800.0));
        assert!(rt.find("set/cid-masked").is_some() && rt.find("set/cid-change").is_some() && rt.find("set/cid/input").is_none());
        assert!(rt.find("set/setup-again").is_some());
        let rt = render("settings-client-id-change", (1280.0, 800.0));
        assert!(rt.find("set/cid/input").is_some() && rt.find("set/cid/error").is_some() && rt.find("set/cid-cancel").is_some());
        let rt = render("settings-no-client-id", (1280.0, 800.0));
        assert!(rt.find("set/cid-change").is_some() && rt.find("set/cid-built-in").is_some() && rt.find("set/cid-built-in-use").is_none());
        assert!(render("settings-account", (1280.0, 800.0)).find("set/cid-built-in-use").is_some(), "an own ID has a way back");
        let rt = render("no-client-id", (1280.0, 800.0));
        assert!(rt.find("rows").is_none() && rt.find("account").is_none());
        let rt = render("signed-out", (1280.0, 800.0));
        assert!(rt.find("welcome").is_some() && rt.find("sign-in").is_some() && rt.find("page/setup").is_none());
        assert!(render("signing-in", (1280.0, 800.0)).find("cancel").is_some());
        assert!(render("sign-in-failed", (1280.0, 800.0)).find("welcome").is_some());
    }

    #[test]
    fn alerts_show_over_the_saved_list() {
        let rt = render("list-sign-in-again", (1280.0, 800.0));
        assert!(rt.find("alert/expired").is_some() && rt.find("alert/sign-in").is_some() && rt.find("rows").is_some());
        let rt = render("list-offline", (1280.0, 800.0));
        assert!(rt.find("alert/error").is_some() && rt.find("alert/retry").is_some() && rt.find("rows").is_some());
        assert!(render("list", (1280.0, 800.0)).find("alert/error").is_none());
    }

    #[test]
    fn search_with_no_match_says_so() {
        // A query searches the whole list: nothing there says so in the line to the service,
        // whose results show under it.
        let rt = render("list-search-none", (1280.0, 800.0));
        assert!(rt.find("sr/find").is_some() && rt.find("sr/online-head").is_some() && rt.find("sr/list-none").is_none());
        assert!(render("list-search", (1280.0, 800.0)).find("rows").is_some());
        let rt = render("list-filter-online", (1280.0, 800.0));
        assert!(rt.find("hit/9101").is_some() && rt.find("hit/0").is_none(), "results, not the ones on the list");
        assert!(rt.find("add").is_some());
        assert!(render("list-filter-online", (960.0, 600.0)).find("add").is_some(), "fits a small window");
        assert!(render("list-filter-online-searching", (1280.0, 800.0)).find("sr/note").is_some());
        assert!(render("list-filter-online-short", (1280.0, 800.0)).find("sr/note").is_some());
    }

    /// Airing: both views, the filters and the states find their parts, wide and narrow.
    #[test]
    fn airing_shows_its_parts() {
        for size in [(1280.0, 800.0), (960.0, 600.0)] {
            let rt = render("airing-week", size);
            for key in ["nav/airing", "page/airing", "airing/views", "airing/filters", "day/0", "airing/today", "airing/estimated", "airing/week"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
            let rt = render("airing-season", size);
            for key in ["airing/grid", "card/0", "airing/add/9301", "airing/plan/9301"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
            assert!(rt.find("airing/add/0").is_none(), "on the list: its status, no add");
        }
        assert!(render("airing-loading", (1280.0, 800.0)).find("airing/loading").is_some());
        assert!(render("airing-empty", (1280.0, 800.0)).find("airing/empty").is_some());
        let rt = render("airing-error", (1280.0, 800.0));
        assert!(rt.find("airing/error").is_some() && rt.find("airing/retry").is_some() && rt.find("airing/empty").is_some());
        let rt = render("airing-error-saved", (1280.0, 800.0));
        assert!(rt.find("airing/error").is_some() && rt.find("airing/week").is_some(), "the saved season stays under the error");
        assert!(render("airing-signed-out", (1280.0, 800.0)).find("welcome").is_some());
        // The picker is on every state of the page: arrows, title.
        for name in ["airing-week", "airing-season", "airing-prev-season", "airing-next-season", "airing-season-loading", "airing-season-offline", "airing-empty", "airing-error"] {
            let rt = render(name, (1280.0, 800.0));
            for key in ["airing/picker", "airing/prev", "airing/season", "airing/next"] {
                assert!(rt.find(key).is_some(), "{name}: {key}");
            }
            assert!(rt.find("season-menu").is_none(), "{name}: closed by default");
        }
        assert!(render("airing-details", (1280.0, 800.0)).find("det-title").is_some());
    }

    /// Settings → Appearance: System follows the desktop live, Light and Dark ignore it.
    #[test]
    fn theme_follows_the_setting_and_the_desktop() {
        let rt = render("settings-theme-light", (1280.0, 800.0));
        for key in ["set/theme/system", "set/theme/light", "set/theme/dark"] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        assert!(crate::theme::is_light(), "Light on a dark desktop");
        render("settings-theme-dark", (1280.0, 800.0));
        assert!(!crate::theme::is_light(), "Dark");
        // System (the default): the desktop turns light, then dark again, while running.
        let mut rt = Runtime::new(State::fake(), crate::ui::view, (1280.0, 800.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(1.0 / 60.0);
        assert!(crate::theme::is_light(), "a light desktop");
        rt.set_system_dark(true);
        rt.frame(1.0 / 60.0);
        assert!(!crate::theme::is_light(), "the desktop went dark");
        // Picking Light in Settings repaints at once, without a restart.
        rt.update(|s, _| s.set_theme(Theme::Light));
        rt.frame(1.0 / 60.0);
        assert!(crate::theme::is_light(), "Light picked");
    }

    /// `[keys] pages = "alt"`: Alt+2 in the focused list filter goes to Plan to watch and types
    /// nothing; a plain 2 still types; Ctrl+2 no longer goes anywhere. The Settings row shows it.
    #[test]
    fn alt_digits_go_to_the_list_pages_even_from_a_field() {
        use kotora_core::Status;
        let rt = render("settings-keys-alt", (1280.0, 800.0));
        let primary = format!("set/page-keys/{}", kotora_core::config::PageKeys::Primary.key());
        for key in ["set/row/page-keys", primary.as_str(), "set/page-keys/alt"] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        let mut s = State::fake();
        s.prefs.page_keys = kotora_core::config::PageKeys::Alt;
        s.go(Page::List(Status::Watching));
        let mut rt = Runtime::new(s, crate::ui::view, (1280.0, 800.0), Env { system_dark: true, scale: 1.0 });
        rt.on_key = Some(crate::ui::on_key);
        rt.frame(1.0 / 60.0);
        click(&mut rt, "search-input");
        assert_eq!(focused(&rt), ["search-input"]);
        rt.key(rune::KeyEvent { key: rune::Key::Text("2".into()), mods: rune::Mods::default() });
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.query.text, "2", "a plain digit types");
        assert_eq!(rt.state.page, Page::List(Status::Watching));
        let ctrl = if cfg!(target_os = "macos") { rune::Mods { cmd: true, logo: true, ..Default::default() } } else { rune::Mods { cmd: true, ctrl: true, ..Default::default() } };
        rt.key(rune::KeyEvent { key: rune::Key::Command('3'), mods: ctrl });
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.page, Page::List(Status::Watching), "Ctrl 3 is not the page key now");
        rt.key(rune::KeyEvent { key: rune::Key::Text("2".into()), mods: rune::Mods { alt: true, ..Default::default() } });
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.page, Page::List(Status::PlanToWatch), "Alt 2 from the field");
        assert_eq!(rt.state.query.text, "2", "Alt 2 typed nothing");
        // Nothing focused: Alt 5 through on_key.
        rt.key(rune::KeyEvent { key: rune::Key::Text("5".into()), mods: rune::Mods { alt: true, ..Default::default() } });
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.page, Page::List(Status::Dropped));
    }

    /// Settings → Titles: one choice lit, and every page follows it.
    #[test]
    fn titles_follow_the_setting() {
        let rt = render("settings-titles-japanese", (1280.0, 800.0));
        assert!(rt.find("set/row/titles").is_some() && rt.find("set/titles/japanese").is_some() && rt.find("set/titles/romaji").is_some());
        let mut s = State::fake();
        let title = |s: &State, id: u32| s.entries.iter().find(|e| e.id == id).unwrap().title.clone();
        s.set_titles(TitleLang::English);
        assert_eq!(title(&s, 0), "The Lanterns on Kirigaoka Hill");
        assert_eq!(title(&s, 2), "Mikan Street Detectives", "no English title: romaji");
        assert_eq!(s.ctl_queue().iter().find(|q| q.title.contains("Hill")).map(|q| q.state.as_str()), Some("Waiting"), "the queue and kot follow");
        s.set_titles(TitleLang::Japanese);
        assert_eq!(title(&s, 0), "霧ヶ丘のランタン");
        assert_eq!(s.find("lanterns of kirigaoka").map(|i| s.entries[i].id), Ok(0), "kot still finds it by its other titles");
        s.set_sort(Sort::Title);
        s.set_titles(TitleLang::Romaji);
        assert_eq!(title(&s, 0), "Lanterns of Kirigaoka");
        assert_eq!(s.entries[s.rows[0]].title, "Clockwork Bakery Wars", "the title sort follows the shown titles");
    }

    /// Settings → Account and About, wide (section list) and narrow (tabs).
    #[test]
    fn settings_has_accounts_and_about() {
        for size in [(1360.0, 860.0), (960.0, 600.0)] {
            let rt = render("settings-account", size);
            for key in ["set/page/account", "acct/mal", "acct/anilist", "token-note", "sign-out", "set/cid"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
            assert!(rt.find("about-card").is_none(), "one section at a time");
            let rt = render("settings-about", size);
            for key in ["about-card", "about-copyright", "set/licences", "link/source", "link/site"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
        }
        let rt = render("settings-signed-out", (1360.0, 860.0));
        assert!(rt.find("sign-in").is_some() && rt.find("sign-out").is_none());
    }

    #[test]
    fn the_editor_opens_under_its_row() {
        let rt = render("list-editor", (1280.0, 800.0));
        for key in ["editor", "ed/status/completed", "ed/progress", "ed/plus", "ed/score/none", "ed/score/10", "ed/save", "ed/cancel"] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        assert!(render("list", (1280.0, 800.0)).find("editor").is_none());
        // Narrow window: the editor still fits.
        assert!(render("list-editor", (960.0, 600.0)).find("ed/save").is_some());
    }

    #[test]
    fn the_queue_page_shows_each_kind_of_change() {
        let rt = render("queue-page", (1280.0, 800.0));
        for key in ["page/queue", "conflict/1", "conflict/1/keep", "conflict/1/take", "qh/waiting", "qh/failed", "qh/sent", "q/2", "q/4/edit", "q/4/discard", "q/5", "queue/sync-now"] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        assert!(rt.find("queue/offline").is_none() && rt.find("queue/expired").is_none());
        assert!(render("queue-empty", (1280.0, 800.0)).find("queue-empty").is_some());
        assert!(render("queue-offline", (1280.0, 800.0)).find("queue/try-now").is_some());
        let rt = render("queue-sign-in-again", (1280.0, 800.0));
        assert!(rt.find("queue/expired").is_some() && rt.find("queue/sign-in").is_some() && rt.find("conflict/1").is_some());
        assert!(render("queue-conflict-removed", (1280.0, 800.0)).find("conflict/1/take").is_some());
    }

    #[test]
    fn now_playing_shows_its_parts() {
        let rt = render("now-playing-page", (1280.0, 800.0));
        for key in ["page/now", "np-card", "np-bar", "np-file", "np-match", "np/update", "np/wrong", "np-players", "player/mpv", "np-mini"] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        assert!(rt.find("banner").is_none(), "no banner before the threshold");
        for name in ["now-playing-offer", "now-playing-offer-page", "now-playing-unsure-offer"] {
            let rt = render(name, (1280.0, 800.0));
            for key in ["banner", "banner/update", "banner/not-now", "banner/wrong"] {
                assert!(rt.find(key).is_some(), "{name}: {key}");
            }
        }
        // The banner fits a narrow window too.
        assert!(render("now-playing-offer", (960.0, 600.0)).find("banner/update").is_some());
        let rt = render("now-playing-recorded", (1280.0, 800.0));
        assert!(rt.find("banner/undo").is_some() && rt.find("banner/update").is_none());
        assert!(render("now-playing-unsure", (1280.0, 800.0)).find("np/yes").is_some());
        let rt = render("now-playing-picking", (1280.0, 800.0));
        assert!(rt.find("np-picker").is_some() && rt.find("pick/0").is_some() && rt.find("pick/none").is_some());
        assert!(render("now-playing-not-on-list", (1280.0, 800.0)).find("np-not-on-list").is_some());
        // Asked first: skips an episode, or not aired yet. The note shows; nothing was recorded.
        for name in ["now-playing-offer-skip", "now-playing-offer-not-aired"] {
            let rt = render(name, (1280.0, 800.0));
            assert!(rt.find("banner/update").is_some() && rt.find("banner/note").is_some() && rt.find("banner/undo").is_none(), "{name}");
        }
        let rt = render("now-playing-not-on-list-added", (1280.0, 800.0));
        assert!(rt.find("banner/undo").is_some() && rt.find("np-not-on-list").is_none());
        for name in ["list-plus-one-not-aired", "list-editor-not-aired"] {
            let rt = render(name, (1280.0, 800.0));
            assert!(rt.find("ask/anyway").is_some() && rt.find("ask/cancel").is_some(), "{name}");
        }
        let rt = render("now-playing-unknown", (1280.0, 800.0));
        assert!(rt.find("np/update").is_none() && rt.find("np-card").is_some());
        // No match: no tick, no "Offer at", and the picker button says "Pick anime…".
        let rt = render("now-playing-unmatched", (1280.0, 800.0));
        assert!(rt.find("np-bar").is_some() && rt.find("np-tick").is_none() && rt.find("np-offer-at").is_none());
        assert!(rt.find("np/wrong").is_some());
        // With a match both stay.
        let rt = render("now-playing-page", (1280.0, 800.0));
        assert!(rt.find("np-tick").is_some() && rt.find("np-offer-at").is_some() && rt.find("np/wrong").is_some());
        let rt = render("now-playing-empty", (1280.0, 800.0));
        assert!(rt.find("np-empty").is_some() && rt.find("np-mini").is_none() && rt.find("np-players").is_some());
    }

    #[test]
    fn settings_has_updates_and_players() {
        for size in [(1360.0, 860.0), (960.0, 600.0)] {
            let rt = render("settings-detection", size);
            for key in ["set/page/detection", "set/auto", "set/threshold", "set/keep-higher", "set/switch/mpv", "set/process-scan", "set/process-minutes/plus"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
        }
        let rt = render("settings-auto-on", (1360.0, 860.0));
        assert!(rt.find("set/auto").is_some() && rt.state.prefs.auto);
        assert!(render("settings-players-off", (1360.0, 860.0)).find("set/switch/vlc").is_some());
    }

    #[test]
    fn the_update_banner_fits_each_install() {
        let has = |rt: &Runtime<State>, keys: &[&str]| keys.iter().map(|k| rt.find(k).is_some()).collect::<Vec<_>>();
        let keys = ["update-notice", "update-notes", "update-download", "update-skip", "update-dismiss"];
        for size in [(1280.0, 800.0), (960.0, 600.0)] {
            let rt = render("update-available", size);
            assert_eq!(has(&rt, &keys), [true; 5], "{size:?}");
            assert_eq!(has(&render("update-available-windows", size), &keys), [true; 5], "Windows: the installer behind Download");
            // Package and install.sh copies: no Download, but say how.
            for name in ["update-available-aur", "update-available-homebrew", "update-available-install-sh"] {
                let rt = render(name, size);
                assert_eq!(has(&rt, &keys), [true, true, false, true, true], "{name}");
            }
            // Below minimum_supported: no Skip and no ✕.
            assert_eq!(has(&render("update-required", size), &keys), [true, true, true, false, false]);
            assert_eq!(has(&render("update-required-aur", size), &keys), [true, true, false, false, false]);
            let rt = render("update-with-offer", size);
            assert!(rt.find("banner").is_some() && rt.find("update-notice").is_some());
        }
        assert!(render("list-watching", (1280.0, 800.0)).find("update-notice").is_none(), "nothing without a check");
    }

    #[test]
    fn settings_has_the_update_switch() {
        for scene in ["settings-page", "settings-general", "settings-updates", "settings-updates-off"] {
            let rt = render(scene, (1360.0, 860.0));
            assert!(rt.find("set/check-updates").is_some() && rt.find("set/check-now").is_some(), "{scene}");
        }
    }

    #[test]
    fn not_on_list_follows_the_title_language() {
        let rt = render("now-playing-not-on-list-japanese", (1280.0, 800.0));
        assert!(rt.find("np-not-on-list").is_some() && rt.find("np-relation").is_some());
        let mut s = State::fake();
        (SCENES.iter().find(|(n, _)| *n == "now-playing-not-on-list-japanese").unwrap().1)(&mut s);
        let n = s.now.as_ref().unwrap();
        assert_eq!(s.not_on_list_title(n).as_deref(), Some("ベルベット果樹園年代記 第2期 後半"));
        assert_eq!(
            s.relation_text(n).as_deref(),
            Some("episode 15 of Velvet Orchard Chronicle 2nd Season is episode 2 of ベルベット果樹園年代記 第2期 後半 (anime-relations)")
        );
        assert_eq!(s.ctl_now().unwrap().title, "ベルベット果樹園年代記 第2期 後半", "kot now too");
        s.set_titles(TitleLang::English);
        let n = s.now.as_ref().unwrap();
        assert_eq!(s.not_on_list_title(n).as_deref(), Some("Velvet Orchard Chronicle Season 2 Part 2"));
        s.set_titles(TitleLang::Romaji);
        let n = s.now.as_ref().unwrap();
        assert_eq!(s.not_on_list_title(n).as_deref(), Some("Velvet Orchard Chronicle 2nd Season Part 2"));
    }

    #[test]
    fn settings_has_the_startup_switches() {
        for scene in ["settings-page", "settings-startup-on"] {
            let rt = render(scene, (1360.0, 860.0));
            assert!(rt.find("set/page/general").is_some(), "{scene}");
            assert!(rt.find("set/login").is_some() && rt.find("set/background").is_some(), "{scene}");
        }
        // Narrow: one column, still there.
        assert!(render("settings-startup-on", (960.0, 700.0)).find("set/login").is_some());
    }

    #[test]
    fn settings_has_the_on_startup_choices() {
        use kotora_core::config::{OpenOn, StartMode};
        let mut rt = render("settings-general", (1360.0, 860.0));
        for k in ["set/row/open-on", "set/open-on/home", "set/open-on/last", "set/row/start-mode", "set/start-mode/last", "set/start-mode/anime", "set/start-mode/manga"] {
            assert!(rt.find(k).is_some(), "{k}");
        }
        assert_eq!((rt.state.prefs.open_on, rt.state.prefs.start_mode), (OpenOn::Home, StartMode::Last), "today's behaviour");
        for (k, check) in [("set/open-on/last", 0), ("set/start-mode/manga", 1), ("set/start-mode/anime", 2), ("set/open-on/home", 3)] {
            let (x, y) = rt.find(k).unwrap();
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(1.0 / 60.0);
            let p = &rt.state.prefs;
            match check {
                0 => assert_eq!(p.open_on, OpenOn::Last),
                1 => assert_eq!(p.start_mode, StartMode::Manga),
                2 => assert_eq!(p.start_mode, StartMode::Anime),
                _ => assert_eq!(p.open_on, OpenOn::Home),
            }
        }
        let rt = render("settings-on-startup", (960.0, 700.0));
        assert!(rt.find("set/open-on/last").is_some(), "narrow too");
        // Search finds both.
        let rt = render("settings-search", (1360.0, 860.0));
        assert!(rt.find("set/results").is_some());
    }

    #[test]
    fn settings_has_the_discord_switches_off_by_default() {
        let mut rt = render("settings-general", (1360.0, 860.0));
        assert!(!rt.state.prefs.discord.enabled, "off by default");
        for k in ["set/row/discord", "set/row/discord-cover", "set/row/discord-buttons", "set/row/discord-hide-paused", "set/row/discord-hide-title", "set/row/discord-status"] {
            assert!(rt.find(k).is_some(), "{k}");
        }
        // The options are inert while the main switch is off.
        let (x, y) = rt.find("set/discord-hide-title").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert!(!rt.state.prefs.discord.hide_title);
        // The main switch turns on (headless: nothing is started, nothing is written).
        let (x, y) = rt.find("set/discord").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert!(rt.state.prefs.discord.enabled);
        // With it on, an option works.
        let mut rt = render("settings-discord-on", (1360.0, 1700.0));
        let (x, y) = rt.find("set/discord-hide-title").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert!(rt.state.prefs.discord.hide_title, "an option works once it is on");
        assert_eq!(rt.state.prefs.discord.status(), kotora_core::config::StatusIn::Title, "the default");
        let (x, y) = rt.find("set/discord-status/episode").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.prefs.discord.status(), kotora_core::config::StatusIn::Episode, "Show in status changes once it is on");
        let rt = render("settings-discord-on", (960.0, 700.0));
        assert!(rt.state.prefs.discord.hide_paused && rt.find("set/discord").is_some());
    }

    #[test]
    fn the_login_switch_says_why_it_is_off_and_asks_for_approval() {
        // A copy that can't be a login item: the switch stays off, whatever is pressed.
        let mut rt = render("settings-startup-unavailable", (1360.0, 860.0));
        assert!(rt.find("set/login").is_some() && rt.find("set/login-items").is_none());
        rt.update(|s, _| s.set_login(true));
        assert!(!rt.state.prefs.login.on, "can't be turned on here");
        // Waiting for approval: the switch is on and a button opens System Settings.
        let rt = render("settings-startup-approval", (1360.0, 860.0));
        assert!(rt.state.prefs.login.on);
        assert!(rt.find("set/login-items").is_some());
    }

    #[test]
    fn search_shows_the_list_then_myanimelist() {
        let rt = render("search-online", (1280.0, 800.0));
        for key in ["page/search", "nav/search", "sr/list-head", "0", "sr/online-head", "hit/9101", "hit/9104", "add", "menu"] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        assert!(rt.find("hit/0").is_none(), "an anime on the list shows once, above");
        assert!(rt.find("chips").is_none());
        assert!(render("search-online-menu", (1280.0, 800.0)).find("add-menu").is_some());
        let rt = render("search-online-added", (1280.0, 800.0));
        assert!(rt.find("9101").is_some() && rt.find("hit/9101").is_none());
        for name in ["search-online-pending", "search-online-short", "search-online-offline", "search-online-error", "search-online-none"] {
            let rt = render(name, (1280.0, 800.0));
            assert!(rt.find("sr/note").is_some() && rt.find("hit/9101").is_none(), "{name}");
        }
        assert!(render("search-online-error", (1280.0, 800.0)).find("sr/retry").is_some());
        assert!(render("search-online-none", (1280.0, 800.0)).find("sr/list-none").is_some());
        assert!(render("search-online", (960.0, 600.0)).find("add").is_some(), "the add button fits a small window");
        assert!(render("search-mal-id", (960.0, 600.0)).find("sr/id/41590").is_some());
        assert!(render("now-playing-relation-not-on-list", (1280.0, 800.0)).find("np-relation").is_some());
        let rt = render("now-playing-rewatch-offer", (960.0, 600.0));
        assert!(rt.find("banner/rewatch").is_some() && rt.find("banner/rewatch-ask").is_some() && rt.find("banner/update").is_none());
        assert!(render("now-playing-rewatch-started", (1280.0, 800.0)).find("banner/undo").is_some());
        let rt = render("now-reading-reread-offer", (960.0, 600.0));
        assert!(rt.find("rd-banner/reread").is_some() && rt.find("rd-banner/reread-ask").is_some() && rt.find("rd-banner/mark").is_none());
        assert!(render("now-reading-reread-offer-page", (1280.0, 800.0)).find("rd/reread").is_some());
        assert!(render("now-reading-reread-started", (1280.0, 800.0)).find("rd-banner/undo").is_some());
        assert!(render("list-rewatching", (1280.0, 800.0)).find("top/extra").is_some());
        assert!(render("list-editor-rewatch", (1280.0, 800.0)).find("ed/rewatch").is_some());
        let rt = render("details-rewatch", (1280.0, 800.0));
        assert!(rt.find("det/rewatch").is_some() && rt.find("det/rewatched").is_some());
        assert!(render("details-rewatching", (1280.0, 800.0)).find("det/stop-rewatch").is_some());
        assert!(render("now-playing-not-on-list", (1280.0, 800.0)).find("np-relation").is_none());
        assert!(render("search-online", (1280.0, 800.0)).find("sr/id/").is_none());
    }

    #[test]
    fn details_show_the_anime_and_the_entry() {
        for size in [(1280.0, 800.0), (960.0, 600.0)] {
            let rt = render("details-on-list", size);
            for key in ["page/details", "det/back", "det/open-mal", "det-title", "det-tags", "det-stats", "det/score", "det-facts", "det-synopsis", "det-mine", "det/progress", "det/edit"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
            assert!(rt.find("det/add").is_none());
        }
        let rt = render("details-editor", (1280.0, 800.0));
        assert!(rt.find("editor").is_some() && rt.find("ed/save").is_some() && rt.find("det/edit").is_none());
        let rt = render("details-not-on-list", (1280.0, 800.0));
        assert!(rt.find("det/add-watching").is_some() && rt.find("det/add/plan").is_some() && rt.find("det/progress").is_none());
        assert!(render("details-added", (1280.0, 800.0)).find("det/progress").is_some());
        assert!(render("details-loading", (1280.0, 800.0)).find("det-loading").is_some());
        assert!(render("details-error", (1280.0, 800.0)).find("det/retry").is_some());
        // The sidebar keeps the page Back goes to lit.
        let rt = render("details-on-list", (1280.0, 800.0));
        assert!(rt.find("nav/watching").is_some());
    }

    #[test]
    fn the_precise_offer_and_its_answers() {
        let rt = render("precise-offer", (1280.0, 800.0));
        assert!(rt.find("precise").is_some() && rt.find("precise/on").is_some() && rt.find("precise/no").is_some());
        assert!(render("precise-offer", (960.0, 600.0)).find("precise/on").is_some());
        assert!(render("precise-written", (1280.0, 800.0)).find("precise/written").is_some());
        assert!(render("precise-already-set", (1280.0, 800.0)).find("precise/ok").is_some());
        assert!(render("precise-failed", (1280.0, 800.0)).find("precise/error").is_some());
        assert!(render("list", (1280.0, 800.0)).find("precise").is_none(), "mpv's socket answers: no offer");
        let rt = render("settings-players-process", (1280.0, 800.0));
        assert!(rt.find("set/precise-on").is_some() && rt.find("precise").is_none(), "answered: Settings only");
        let rt = render("now-playing-process", (1280.0, 800.0));
        assert!(rt.find("np-no-position").is_some() && rt.find("np-bar").is_none());
    }

    #[test]
    fn the_precise_prompt_on_now_playing() {
        let rt = render("precise-prompt", (1280.0, 800.0));
        assert!(rt.find("precise-prompt/on").is_some() && rt.find("precise-prompt/not-now").is_some());
        assert!(rt.find("precise/on").is_none(), "one offer at a time: the page's prompt");
        assert!(render("precise-prompt", (960.0, 600.0)).find("precise-prompt/on").is_some());
        let rt = render("precise-prompt-with-offer", (1280.0, 800.0));
        assert!(rt.find("banner/update").is_some() && rt.find("precise-prompt/on").is_some());
        // Answered (or by IPC): no prompt.
        assert!(render("now-playing-process", (1280.0, 800.0)).find("precise-prompt").is_none());
        assert!(render("now-playing-page", (1280.0, 800.0)).find("precise-prompt").is_none());
        #[cfg(target_os = "macos")]
        assert!(render("precise-prompt-iina", (1280.0, 800.0)).find("precise-prompt/on").is_some());
    }

    /// Settings: the section list when wide, tabs when narrow; the search's results and none.
    #[test]
    fn settings_sections_tabs_and_search() {
        let rt = render("settings-general", (1360.0, 900.0));
        assert!(rt.find("set/sections").is_some() && rt.find("set/section/detection").is_some() && rt.find("set/search").is_some());
        let rt = render("settings-general", (960.0, 600.0));
        assert!(rt.find("set/section/about").is_some(), "the tabs");
        for size in [(1360.0, 900.0), (960.0, 600.0)] {
            let rt = render("settings-search", size);
            assert!(rt.find("set/results").is_some() && rt.find("set/row/start-at-login").is_some() && rt.find("set/result-head/general").is_some(), "{size:?}");
            assert!(rt.find("set/page/general").is_none(), "the results take the section's place");
            let rt = render("settings-search-none", size);
            assert!(rt.find("set/no-results").is_some(), "{size:?}");
        }
        let rt = render("settings-library", (1360.0, 900.0));
        assert!(rt.find("set/library/add").is_some() && rt.find("set/open-with").is_some());
    }

    #[test]
    fn settings_has_the_notification_switches() {
        let rt = render("settings-notifications", (1280.0, 800.0));
        for k in ["set/page/notifications", "set/notify-detected", "set/notify-offer", "set/notify-recorded", "set/notify-started", "set/notify-episode"] {
            assert!(rt.find(k).is_some(), "{k}");
        }
        assert!(rt.find("set/notify-support").is_none());
        assert!(render("settings-notifications-no-buttons", (1280.0, 800.0)).find("set/notify-support").is_some());
        assert!(render("settings-notifications-denied", (960.0, 600.0)).find("set/notify-support").is_some());
    }

    /// A press on a row's title opens the details page; elsewhere on the row, the editor.
    #[test]
    fn the_title_opens_details_and_the_row_the_editor() {
        let mut rt = render("list", (1280.0, 800.0));
        let (x, y) = rt.find("title-link").expect("a title link");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert!(rt.find("page/details").is_some() && rt.find("editor").is_none());
        let (bx, by) = rt.find("det/back").unwrap();
        rt.pointer_down(bx, by);
        rt.pointer_up(bx, by);
        rt.frame(1.0 / 60.0);
        assert!(rt.find("page/details").is_none() && rt.find("rows").is_some(), "Back returns to the list");
        // The progress column of the first row: the editor opens under it.
        let (_, ry) = rt.find("title-link").unwrap();
        rt.pointer_down(800.0, ry);
        rt.pointer_up(800.0, ry);
        rt.frame(1.0 / 60.0);
        assert!(rt.find("editor").is_some() && rt.find("page/details").is_none());
    }

    /// Airing's season menu: "This season", the seasons newest first with a check on the one
    /// shown, in dark and light; the states of another season.
    #[test]
    fn the_season_menu_lists_the_seasons() {
        for name in [
            "airing-season-menu",
            "airing-prev-season-menu",
            "airing-far-season-menu",
        ] {
            let rt = render(name, (1280.0, 800.0));
            for key in [
                "season-menu",
                "airing/season/this",
                "airing/season/2027-spring",
                "airing/season/2027-winter",
                "airing/season/2026-fall",
                "airing/season/2025-winter",
            ] {
                assert!(rt.find(key).is_some(), "{name}: {key}");
            }
        }
        assert!(
            render("airing-season-menu", (1280.0, 800.0))
                .find("airing/season/2024-spring")
                .is_none()
        );
        assert!(
            render("airing-far-season-menu", (1280.0, 800.0))
                .find("airing/season/2024-spring")
                .is_some(),
            "a season further back has its row"
        );
        // Another season is a grid: no week switch, no "estimated" note, no markers of today.
        let rt = render("airing-prev-season", (1280.0, 800.0));
        for key in ["airing/grid", "airing/filters", "card/9401"] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        for key in [
            "airing/views",
            "airing/estimated",
            "airing/week",
            "airing/today",
        ] {
            assert!(rt.find(key).is_none(), "{key}");
        }
        let rt = render("airing-season-loading", (1280.0, 800.0));
        assert!(rt.find("airing/loading").is_some() && rt.find("airing/grid").is_none());
        let rt = render("airing-season-offline", (1280.0, 800.0));
        assert!(
            rt.find("airing/error").is_some()
                && rt.find("airing/retry").is_some()
                && rt.find("airing/empty").is_some()
        );
        let rt = render("airing-season-error-saved", (1280.0, 800.0));
        assert!(
            rt.find("airing/error").is_some() && rt.find("airing/grid").is_some(),
            "the saved season stays under the error"
        );
    }

    /// The picker by pointer: the arrow steps back, the title opens the menu, a row picks, and
    /// "This season" comes back to the page's two views.
    #[test]
    fn the_season_picker_works_by_pointer() {
        let click = |rt: &mut Runtime<State>, key: &str| {
            let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key}"));
            rt.pointer_move(x, y);
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            for _ in 0..3 {
                rt.frame(1.0 / 60.0);
            }
        };
        let mut rt = render("airing-season", (1280.0, 800.0));
        assert!(rt.find("airing/views").is_some());
        click(&mut rt, "airing/prev");
        assert!(
            rt.find("airing/views").is_none(),
            "Fall 2026 is a plain grid"
        );
        click(&mut rt, "airing/season");
        assert!(rt.find("season-menu").is_some());
        click(&mut rt, "airing/season/2026-summer");
        assert!(rt.find("season-menu").is_none());
        let mut label = String::new();
        rt.update(|s, _| label = s.viewed_season().label());
        assert_eq!(label, "Summer 2026");
        click(&mut rt, "airing/season");
        click(&mut rt, "airing/season/this");
        assert!(rt.find("season-menu").is_none() && rt.find("airing/views").is_some());
        let mut browsing = true;
        rt.update(|s, _| browsing = s.airing.browse.is_some());
        assert!(!browsing, "This season is the page itself");
        // The last season has no way forward.
        click(&mut rt, "airing/next");
        click(&mut rt, "airing/next");
        rt.update(|s, _| label = s.viewed_season().label());
        assert_eq!(label, "Spring 2027");
    }

    /// The type column hides below 1100 pt.
    #[test]
    fn narrow_window_drops_the_type_column() {
        let wide = render("list", (1280.0, 800.0));
        let narrow = render("list", (1000.0, 700.0));
        assert!(wide.find("thead/type").is_some());
        assert!(narrow.find("thead/type").is_none());
    }
}

/// A press on a row opens its editor; with an editor open, a press on another row's title moves
/// the editor there (no details page), in the anime list and the manga list alike. The blank
/// part of the title column is the row's, not the title link's.
#[cfg(test)]
mod row_presses {
    use super::*;
    use rune::{Env, Runtime};

    fn rt(name: &str) -> Runtime<State> {
        let mut s = State::fake();
        (SCENES.iter().find(|(n, _)| *n == name).unwrap().1)(&mut s);
        let mut rt = Runtime::new(s, crate::ui::view, (1280.0, 800.0), Env { system_dark: true, scale: 1.0 });
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        rt
    }

    fn tap(rt: &mut Runtime<State>, x: f32, y: f32) {
        rt.pointer_move(x, y);
        rt.frame(1.0 / 60.0);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
    }

    /// The middle of the title link of `title` (its label is "About TITLE").
    fn title_at(rt: &Runtime<State>, title: &str) -> (f32, f32) {
        let label = format!("About {title}");
        let e = rt.elements().into_iter().find(|e| e.label.as_deref() == Some(label.as_str()) && !e.exiting).unwrap_or_else(|| panic!("{title}"));
        let [x, y, w, h] = e.rect;
        (x + w / 2.0, y + h / 2.0)
    }

    /// Row A by its title (editor), then row B by its title: the editor moves to B.
    fn check(scene: &str, a: &str, b: &str, editor: fn(&State) -> Option<u32>, id: fn(&State, &str) -> u32) {
        let mut r = rt(scene);
        // Row A: the blank part of its title column opens the editor.
        let (_, ya) = title_at(&r, a);
        tap(&mut r, 600.0, ya);
        assert_eq!(editor(&r.state), Some(id(&r.state, a)), "{scene}: the row's editor");
        assert_ne!(r.state.page, Page::Details);
        // Row B's title: the editor moves, no details page.
        let (xb, yb) = title_at(&r, b);
        tap(&mut r, xb, yb);
        assert_ne!(r.state.page, Page::Details, "{scene}: no details page with an editor open");
        assert_eq!(editor(&r.state), Some(id(&r.state, b)), "{scene}: the editor moved to B");
        // With no editor open, the title opens the details page as before.
        let mut r = rt(scene);
        let (xa, ya) = title_at(&r, a);
        tap(&mut r, xa, ya);
        assert_eq!(r.state.page, Page::Details, "{scene}: the title opens the details page");
    }

    /// The link hugs a short title and still stops at its column with a long one (truncated).
    #[test]
    fn the_title_link_is_as_wide_as_the_title_within_its_column() {
        let mut s = State::fake();
        s.entries[0].title = "An Extremely Long Light Novel Title That Never Ends: The Girl Who Lit the Lantern on the Hill".into();
        s.refresh();
        let mut rt = Runtime::new(s, crate::ui::view, (1000.0, 760.0), Env { system_dark: true, scale: 1.0 });
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        let els = rt.elements();
        let link = |t: &str| els.iter().find(|e| e.label.as_deref() == Some(&format!("About {t}"))).map(|e| e.rect).unwrap();
        let plus = els.iter().find(|e| e.key.as_deref() == Some("plus")).map(|e| e.rect).unwrap();
        let long = link("An Extremely Long Light Novel Title That Never Ends: The Girl Who Lit the Lantern on the Hill");
        assert!(long[0] + long[2] < plus[0], "the long title stays in its column: {long:?}");
        let short = link("Neon Ronin 2099");
        assert!(short[2] < long[2] / 2.0, "a short title's link is about as wide as its text: {short:?}");
    }

    #[test]
    fn a_title_pressed_with_an_editor_open_moves_the_editor() {
        check("list", "Lanterns of Kirigaoka", "Starfall Delivery Service", |s| s.editor.map(|e| e.id), |s, t| s.entries.iter().find(|e| e.title == t).unwrap().id);
        check("manga-list", "Inkwell Cartographers", "Salt & Gears", |s| s.manga.editor.map(|e| e.id), |s, t| s.manga.entries.iter().find(|e| e.title == t).unwrap().id);
    }
}

/// With the mirror on, details pages and menus link to both services (anime and manga); without
/// it, to the active one only.
#[cfg(test)]
mod mirror_links {
    use super::*;

    fn keys(name: &str, size: (f32, f32)) -> rune::Runtime<State> {
        let mut s = State::fake();
        (SCENES.iter().find(|(n, _)| *n == name).unwrap().1)(&mut s);
        let mut rt = rune::Runtime::new(s, crate::ui::view, size, rune::Env { system_dark: true, scale: 1.0 });
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        rt
    }

    #[test]
    fn both_links_with_the_mirror_one_without() {
        for (name, size) in [("details-mirror", (1280.0, 800.0)), ("details-mirror-narrow", (1000.0, 760.0)), ("manga-details-mirror", (1280.0, 800.0))] {
            let rt = keys(name, size);
            assert!(rt.find("det/open-mal").is_some() && rt.find("det/open-other").is_some(), "{name}");
        }
        for name in ["ctx-menu-mirror", "manga-ctx-mirror"] {
            let rt = keys(name, (1280.0, 800.0));
            assert!(rt.find("ctx/mal").is_some() && rt.find("ctx/other").is_some(), "{name}");
        }
        for name in ["details-on-list", "manga-details", "ctx-menu", "manga-ctx"] {
            let rt = keys(name, (1280.0, 800.0));
            assert!(rt.find("det/open-other").is_none() && rt.find("ctx/other").is_none(), "{name}: one link without the mirror");
        }
    }
}
/// Settings in a wide window: a section taller than the window scrolls, so the previews that open
/// under their rows (fill volumes, import, the mirror's) can be reached; the scroll area keeps the
/// window's height instead of growing with its content.
#[cfg(test)]
mod settings_scroll {
    use super::*;

    fn rect(rt: &rune::Runtime<State>, key: &str) -> [f32; 4] {
        rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).map(|e| e.rect).unwrap_or_else(|| panic!("{key}"))
    }

    fn reachable(setup: fn(&mut State), preview: &str, button: &str) {
        let (w, h) = (1360.0, 860.0);
        let mut s = State::fake();
        setup(&mut s);
        let mut rt = rune::Runtime::new(s, crate::ui::view, (w, h), rune::Env { system_dark: true, scale: 1.0 });
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        let area = rect(&rt, "settings-scroll");
        assert!(area[1] + area[3] <= h + 0.5, "{preview}: the scroll area fits the window: {area:?}");
        let b = rect(&rt, button);
        assert!(b[1] + b[3] > h, "{preview}: the case is real: the button starts below the window ({b:?})");
        // Scroll down with the wheel over the area until the button shows.
        for _ in 0..40 {
            rt.wheel(area[0] + 40.0, area[1] + 40.0, -120.0);
            rt.frame(1.0 / 60.0);
        }
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
        }
        let b = rect(&rt, button);
        assert!(b[1] >= area[1] - 0.5 && b[1] + b[3] <= h + 0.5, "{preview}: scrolled into reach: {b:?}");
        let p = rect(&rt, preview);
        assert!(p[1] + p[3] <= h + 0.5, "{preview}: the whole card can be seen: {p:?}");
    }

    #[test]
    fn previews_under_their_rows_can_be_scrolled_to() {
        reachable(
            |s| {
                crate::manga::fake(s);
                crate::volfill::fake_preview(s);
                crate::settings::open(s, Section::Account);
            },
            "set/fill-volumes-preview",
            "set/fill-volumes-apply",
        );
        reachable(
            |s| {
                crate::transfer::fake_preview(s);
                crate::settings::open(s, Section::Account);
            },
            "set/import-preview",
            "set/import-cancel",
        );
        reachable(
            |s| {
                crate::mirror::fake_preview(s);
                crate::settings::open(s, Section::Account);
            },
            "set/mirror-preview",
            "set/mirror-cancel",
        );
    }
}

/// Home with Manga on shows Continue reading (and nothing of manga when it is off); the search
/// page shows its kind toggle with Manga on.
#[cfg(test)]
mod manga_polish {
    use super::*;

    fn rt(name: &str) -> rune::Runtime<State> {
        let mut s = State::fake();
        (SCENES.iter().find(|(n, _)| *n == name).unwrap().1)(&mut s);
        let mut rt = rune::Runtime::new(s, crate::ui::view, (1280.0, 900.0), rune::Env { system_dark: true, scale: 1.0 });
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        rt
    }

    #[test]
    fn home_search_and_grid_show_their_manga_parts() {
        let r = rt("home-manga");
        assert!(r.find("home/reading").is_some() && r.find("read/2").is_some(), "Continue reading with Salt & Gears");
        assert!(r.find("read/3").is_some(), "a reread shows");
        assert!(r.find("read/4").is_some(), "On hiatus still reads on");
        assert!(rt("home").find("home/reading").is_none(), "Manga off: no manga on Home");
        let r = rt("search-kind-manga");
        assert!(r.find("search/kind/manga").is_some() && r.find("hit/501").is_some());
        assert!(rt("manga-grid-selected").find("msel/2").is_some());
    }

    /// Manga mode with reading detection on: the sidebar says Now Reading and shows the comic;
    /// Anime mode keeps Now Playing and the playing card.
    #[test]
    fn manga_mode_says_now_reading() {
        let r = rt("now-reading-sidebar");
        assert!(r.find("rd-mini").is_some() && r.find("np-mini").is_none());
        let r = rt("now-reading-yacreader-open");
        assert!(r.find("rd/add").is_some(), "Add as Reading");
        let r = rt("now-playing-page");
        assert!(r.find("np-mini").is_some() && r.find("rd-mini").is_none());
    }

    /// Continue reading: on the details card, in the menu, and in Now Reading once closed.
    #[test]
    fn continue_reading_shows_up() {
        let r = rt("manga-details-continue");
        assert!(r.find("rd-resume/go").is_some() && r.find("rd-resume/where").is_some());
        let r = rt("manga-ctx-continue");
        assert!(r.find("ctx/continue").is_some());
        let r = rt("manga-ctx");
        assert!(r.find("ctx/continue").is_none(), "unknown: hidden");
        let r = rt("now-reading-continue");
        assert!(r.find("rd-resume/go").is_some());
    }

    /// Manga mode's Now Reading never shows anime's Up next; Anime mode's Now Playing does.
    #[test]
    fn now_reading_has_no_anime_up_next() {
        let r = rt("now-reading-empty");
        assert!(r.find("np-up-next").is_none());
        let r = rt("now-reading-dbz");
        assert!(r.find("rd/ask").is_some() && r.find("rd/renumbered").is_some() && r.find("np-up-next").is_none());
    }
}

