//! Named states for headless renders (`--scene NAME`), filled with fake, deterministic data.
//! `--list-scenes` lists them. Every view and state gets one (CLAUDE.md).

use kotora_core::Status;
use kotora_core::config::{Theme, TitleLang};

use kotora_core::queue::OpState;
use kotora_core::recognize::Found;

use crate::model::{Auth, Filter, Page, Sort, State};
use crate::setup::Step;

/// A scene: its name and the change it makes to the fake first state.
pub type Scene = (&'static str, fn(&mut State));

pub const SCENES: &[Scene] = &[
    // The main list, as `mockup/shots/list.png`.
    ("list", |_| {}),
    // Real covers (made up here) on the rows that have one; two rows have none and keep the tile.
    ("list-covers", fake_covers),
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
            n.relation = Some("episode 15 of Velvet Orchard Chronicle 2nd Season is episode 2 of Velvet Orchard Chronicle 2nd Season Part 2 (anime-relations)".into());
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
    ("queue-page", |s| s.go(Page::Queue)),
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
    ("airing-details", |s| {
        airing(s, crate::airing::View::Season);
        s.airing_details(9301);
    }),
    ("settings-page", |s| s.go(Page::Settings)),
    // Settings → Titles: English or Japanese where the anime has one, romaji where not.
    ("settings-titles-japanese", |s| {
        s.set_titles(TitleLang::Japanese);
        s.go(Page::Settings);
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
    ("details-titles-japanese", |s| {
        s.set_titles(TitleLang::Japanese);
        details(s, 0, true);
    }),
    ("search-titles-english", |s| {
        s.set_titles(TitleLang::English);
        search(s, "kirigaoka", true);
    }),
    // Settings → Appearance with Light picked (light whatever the desktop says).
    ("settings-theme-light", |s| {
        s.set_theme(Theme::Light);
        s.go(Page::Settings);
    }),
    ("settings-theme-dark", |s| {
        s.set_theme(Theme::Dark);
        s.go(Page::Settings);
    }),
    // "Update automatically" on, threshold 90 %, keep-higher off.
    ("settings-auto-on", |s| {
        s.prefs.auto = true;
        s.prefs.threshold = 90;
        s.prefs.keep_higher = false;
        s.go(Page::Settings);
    }),
    // Settings → Startup with both switches on.
    ("settings-startup-on", |s| {
        s.prefs.login = true;
        s.prefs.background = true;
        s.go(Page::Settings);
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
        s.go(Page::Settings);
    }),
    // Settings → Accounts: Change pressed on the Client ID, with a wrong paste.
    ("settings-client-id-change", |s| {
        s.client_id.editing = true;
        s.client_id.draft.replace_text("abc123");
        s.save_client_id();
        s.go(Page::Settings);
    }),
    // No Client ID yet: Settings offers to set one.
    ("settings-no-client-id", |s| {
        signed_out(s, Auth::NoClientId);
        s.client_id.hint = None;
        s.go(Page::Settings);
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
        s.go(Page::Settings);
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
        s.go(Page::Settings);
    }),
    // All on, and the system shows no buttons (Linux: the server; macOS: outside Kotora.app).
    ("settings-notifications-no-buttons", |s| {
        s.prefs.notify.detected = true;
        s.notes.support = if cfg!(target_os = "macos") { crate::notes::Support::Outside } else { crate::notes::Support::NoButtons };
        s.go(Page::Settings);
    }),
    // macOS: notifications turned off for Kotora in System Settings.
    ("settings-notifications-denied", |s| {
        s.notes.support = crate::notes::Support::Denied;
        s.go(Page::Settings);
    }),
    ("settings-signed-out", |s| {
        signed_out(s, Auth::SignedOut);
        s.go(Page::Settings);
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
    s.go(Page::Settings);
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

/// The Airing page with the fake season and covers, in `view`.
fn airing(s: &mut State, view: crate::airing::View) {
    s.covers.fake();
    crate::airing::fake(s);
    s.go(Page::Airing);
    s.airing_view(view);
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

/// The wizard at `step`, first run (no Client ID, nobody signed in).
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

#[cfg(test)]
mod tests {
    use super::SCENES;
    use crate::model::Sort;
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
                // The sign-in screen and the wizard have no top bar.
                assert!(rt.find("top").is_some() || rt.find("welcome").is_some() || rt.find("page/setup").is_some(), "{name}: page title");
            }
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
        let rt = render("settings-page", (1280.0, 800.0));
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
        assert!(rt.find("display-card").is_some() && rt.find("set/titles/japanese").is_some() && rt.find("set/titles/romaji").is_some());
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

    /// Settings: the Accounts and About cards, side by side when wide, stacked when narrow.
    #[test]
    fn settings_has_accounts_and_about() {
        for size in [(1360.0, 860.0), (960.0, 600.0)] {
            let rt = render("settings-page", size);
            for key in ["account-card", "acct/mal", "acct/anilist", "token-note", "sign-out", "about-card", "link/source"] {
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
        let rt = render("now-playing-empty", (1280.0, 800.0));
        assert!(rt.find("np-empty").is_some() && rt.find("np-mini").is_none() && rt.find("np-players").is_some());
    }

    #[test]
    fn settings_has_updates_and_players() {
        for size in [(1360.0, 860.0), (960.0, 600.0)] {
            let rt = render("settings-page", size);
            for key in ["updates-card", "set/auto", "set/threshold", "set/keep-higher", "set/asks-on", "players-card", "set/switch/mpv", "set/process-scan", "set/process-minutes/plus"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
        }
        let rt = render("settings-auto-on", (1360.0, 860.0));
        assert!(rt.find("set/asks-off").is_some());
        assert!(render("settings-players-off", (1360.0, 860.0)).find("set/switch/vlc").is_some());
    }

    #[test]
    fn settings_has_the_startup_switches() {
        for scene in ["settings-page", "settings-startup-on"] {
            let rt = render(scene, (1360.0, 860.0));
            assert!(rt.find("startup-card").is_some(), "{scene}");
            assert!(rt.find("set/login").is_some() && rt.find("set/background").is_some(), "{scene}");
        }
        // Narrow: one column, still there.
        assert!(render("settings-startup-on", (960.0, 700.0)).find("set/login").is_some());
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

    #[test]
    fn settings_has_the_notification_switches() {
        let rt = render("settings-notifications", (1280.0, 800.0));
        for k in ["notifications-card", "set/notify-detected", "set/notify-offer", "set/notify-recorded"] {
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

    /// The type column hides below 1100 pt.
    #[test]
    fn narrow_window_drops_the_type_column() {
        let wide = render("list", (1280.0, 800.0));
        let narrow = render("list", (1000.0, 700.0));
        assert!(wide.find("thead/type").is_some());
        assert!(narrow.find("thead/type").is_none());
    }
}
