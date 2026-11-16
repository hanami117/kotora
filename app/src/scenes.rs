//! Named states for headless renders (`--scene NAME`), filled with fake, deterministic data.
//! `--list-scenes` lists them. Every view and state gets one.

use kotora_core::Status;
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
    // Home: continue watching, airing this week, plan to watch.
    ("home", home),
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
    // The token was refused: the saved list shows, with a Sign in button.
    ("list-sign-in-again", |s| s.auth = Auth::Expired),
    // First run: no Client ID in config.toml, so the setup wizard (`setup.rs`), step 1.
    ("no-client-id", |s| setup(s, Step::Welcome)),
    // Step 3 with a pasted value that isn't an ID (the inline error).
    ("no-client-id-error", |s| {
        setup(s, Step::Paste);
        s.client_id.draft.replace_text("kotora-app-secret-from-mal");
        s.save_client_id();
    }),
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
    ("setup-create", |s| setup(s, Step::Create)),
    // The redirect URL copied: the button says so.
    ("setup-create-copied", |s| {
        setup(s, Step::Create);
        s.client_id.copied = true;
    }),
    ("setup-paste", |s| setup(s, Step::Paste)),
    // 64 hex characters: the Client Secret's shape, refused.
    ("setup-paste-secret", |s| {
        setup(s, Step::Paste);
        s.client_id.draft.replace_text("3f9c0a7e5b1d2c48".repeat(4));
        s.save_client_id();
    }),
    // Saved (after Back from the sign-in step): Next is on.
    ("setup-paste-saved", |s| {
        setup(s, Step::Paste);
        s.auth = Auth::SignedOut;
        s.client_id.hint = Some("8f3a".into());
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
        s.editor_score(Some(9));
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
    // No Client ID yet: Settings offers to set one.
    ("settings-no-client-id", |s| {
        signed_out(s, Auth::NoClientId);
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
    s.details_answer(crate::details::Answer { id, result: Ok(d), item: None, done: true });
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
fn anilist(s: &mut State) {
    s.service = kotora_core::service::ServiceId::AniList;
    s.account = Some(("demo_user".into(), "AniList"));
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
        let rt = render("home-library", (1280.0, 800.0));
        assert!(rt.find("play").is_some(), "Play when the next file is there");
        let rt = render("home-quiet", (1280.0, 800.0));
        assert!(rt.find("home/airing/scroll").is_none() && rt.find("home/plan/scroll").is_none());
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
        assert!(rt.find("setup/back").is_none() && rt.find("setup/close").is_none(), "step 1, and no way around it without an ID");
        let rt = render("setup-create", (960.0, 600.0));
        for key in ["setup/open-api", "setup/copy-redirect", "setup/form/app-redirect-url", "setup/form/purpose-of-use", "setup/no-secret", "setup/back"] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        let rt = render("setup-paste", (1280.0, 800.0));
        assert!(rt.find("setup/cid/input").is_some() && rt.find("setup/cid/save").is_some() && rt.find("setup/cid/error").is_none());
        assert!(render("no-client-id-error", (1280.0, 800.0)).find("setup/cid/error").is_some());
        assert!(render("setup-paste-secret", (1280.0, 800.0)).find("setup/cid/error").is_some());
        assert!(render("setup-paste-saved", (1280.0, 800.0)).find("setup/cid-saved").is_some());
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
        assert!(render("settings-no-client-id", (1280.0, 800.0)).find("set/cid-change").is_some());
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
        // A query searches the whole list (and MyAnimeList): nothing on the list says so in a line.
        let rt = render("list-search-none", (1280.0, 800.0));
        assert!(rt.find("sr/list-none").is_some() && rt.find("search").is_some());
        assert!(render("list-search", (1280.0, 800.0)).find("rows").is_some());
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
            for key in ["about-card", "set/licences", "link/source", "link/site"] {
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
