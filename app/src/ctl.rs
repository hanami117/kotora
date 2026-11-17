//! The control socket for `kot` (protocol in `kotora_ctl`), and single instance.
//!
//! Each request runs on its connection's thread and hops to the UI thread
//! (`AppHandle::update_if`) for the answer. Adapted from Uzumaki's `app/src/ctl.rs`.

use std::sync::mpsc;
use std::time::Duration;

use kotora_core::queue::OpState;
use kotora_core::score::ScoreFormat;
use kotora_core::service::ServiceId;
use kotora_core::{Change, Kind, Status};
use kotora_ctl::{Reply, Request, Server};

use crate::model::{Handle, State, Want};

/// How long a request waits for the UI thread (it answers within a frame unless it's stuck).
const UI_TIMEOUT: Duration = Duration::from_secs(5);
/// How long `Quit` waits after answering, so the client reads the reply first.
const QUIT_DELAY: Duration = Duration::from_millis(200);

/// Starts listening. The socket file goes away when the returned server is dropped.
pub fn start(handle: Handle) -> Option<Server> {
    match kotora_ctl::serve(&kotora_ctl::socket_path(), move |req| answer(&handle, req)) {
        Ok(server) => Some(server),
        Err(e) => {
            eprintln!("kotora: no control socket, so kot won't work: {e}");
            None
        }
    }
}

/// Whether another copy answers on the control socket; `show` asks it to show its window.
pub fn other_copy_running(show: bool) -> bool {
    let Ok(mut client) = kotora_ctl::Client::connect() else { return false };
    if show {
        // Windows: let the running copy bring its window to the front.
        #[cfg(windows)]
        crate::win::allow_focus();
        let _ = client.request(&Request::Show);
    }
    true
}

fn answer(handle: &Handle, req: Request) -> Reply {
    let status = |r: Result<kotora_ctl::Status, String>| match r {
        Ok(st) => Reply::Status(st),
        Err(message) => Reply::Error { message, anyway: false },
    };
    let reply = |r: Result<Result<Reply, String>, String>| match r.and_then(|r| r) {
        Ok(reply) => reply,
        Err(message) => Reply::Error { message, anyway: false },
    };
    match req {
        Request::Status => status(on_ui(handle, false, |s| s.ctl_status())),
        Request::Show => {
            handle.open();
            status(on_ui(handle, true, |s| {
                s.window = true;
                s.ctl_status()
            }))
        }
        Request::Quit => {
            let r = on_ui(handle, false, |s| s.ctl_status());
            let handle = handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(QUIT_DELAY);
                handle.quit();
            });
            status(r)
        }
        Request::List { status: st } => reply(on_ui(handle, false, move |s| list(s, st.as_deref()))),
        Request::PlusOne { title, anyway } => reply(on_ui(handle, true, move |s| plus_one(s, &title, anyway))),
        Request::Set { title, progress, status: st, score, score_text, anyway } => reply(on_ui(handle, true, move |s| {
            let status = match st.as_deref() {
                Some(name) => Some(parse_status(name)?),
                None => None,
            };
            let change = Change { status, progress, score: score_of(s, score, score_text.as_deref())?, ..Default::default() };
            set(s, &title, change, anyway)
        })),
        Request::Remove { title, yes } => reply(on_ui(handle, true, move |s| remove(s, &title, yes))),
        Request::Rewatch { title } => reply(on_ui(handle, true, move |s| {
            // A MyAnimeList id, or a part of the title.
            let i = match title.trim().parse::<u32>().ok().and_then(|id| s.entries.iter().position(|e| e.id == id)) {
                Some(i) => i,
                None => s.find(&title)?,
            };
            let i = s.start_rewatch(s.entries[i].id)?;
            Ok(Reply::Edited { entry: s.entries[i].ctl(s.score_format()) })
        })),
        Request::Queue => reply(on_ui(handle, false, |s| Ok(Reply::Queue { ops: s.ctl_queue() }))),
        Request::Now => reply(on_ui(handle, false, |s| Ok(Reply::Now { now: s.ctl_now() }))),
        Request::Airing => reply(on_ui(handle, false, |s| {
            if !s.has_list() {
                return Err("Not signed in.".to_string());
            }
            // The season is read when the Airing page opens; kot asks for it too (saved copy
            // first, MyAnimeList only when it is old).
            let note = if s.airing.shows.is_empty() {
                s.airing_fetch(false);
                Some("Kotora is reading the season; try again in a few seconds.".to_string())
            } else {
                None
            };
            Ok(Reply::Airing { items: s.ctl_airing(), note })
        })),
        Request::Dash => reply(on_ui(handle, false, |s| {
            if !s.has_list() {
                return Err("Not signed in.".to_string());
            }
            if s.airing.shows.is_empty() {
                s.airing_fetch(false);
            }
            Ok(Reply::Dash(s.ctl_dash()))
        })),
        Request::Confirm => reply(on_ui(handle, true, |s| {
            let (title, _) = s.confirm_offer()?;
            let i = s.entries.iter().position(|e| e.title == title).ok_or("That anime is not on your list.")?;
            Ok(Reply::Edited { entry: s.entries[i].ctl(s.score_format()) })
        })),
        Request::Dismiss => reply(on_ui(handle, true, |s| {
            if s.now.is_none() {
                return Err("Nothing is playing.".to_string());
            }
            s.dismiss_now();
            Ok(Reply::Now { now: s.ctl_now() })
        })),
        Request::SetClientId { id } => match kotora_core::config::save_client_id_for(&kotora_core::paths::config_file(), crate::account::active(), &id) {
            Err(message) => Reply::Error { message, anyway: false },
            Ok(id) => {
                let hint = kotora_core::config::client_id_hint(&id);
                let auth = crate::online::initial_auth();
                let signed_in = auth == crate::model::Auth::SignedIn;
                let _ = on_ui(handle, true, move |s| s.client_id_saved(Ok((hint, auth))));
                Reply::Done {
                    message: if signed_in {
                        "Saved the Client ID. A new ID needs a new sign-in when the current one runs out.".into()
                    } else {
                        "Saved the Client ID. Sign in from Kotora's window now.".into()
                    },
                }
            }
        },
        Request::Add { query, status: st } => {
            let status = match st.as_deref().map(parse_status).transpose() {
                Ok(s) => s.unwrap_or(Status::Watching),
                Err(message) => return Reply::Error { message, anyway: false },
            };
            match find_online(&query) {
                Ok(media) => reply(on_ui(handle, true, move |s| {
                    let i = s.add_media(media, status)?;
                    Ok(Reply::Edited { entry: s.entries[i].ctl(s.score_format()) })
                })),
                Err(message) => Reply::Error { message, anyway: false },
            }
        }
        // A manga: MyAnimeList only, with Manga on (its list is what the window shows).
        Request::MangaAdd { query, status: st } => {
            let status = match st.as_deref().map(parse_status).transpose() {
                Ok(s) => s.unwrap_or(Status::Watching),
                Err(message) => return Reply::Error { message, anyway: false },
            };
            match on_ui(handle, false, manga_ready) {
                Ok(Ok(())) => {}
                Ok(Err(message)) | Err(message) => return Reply::Error { message, anyway: false },
            }
            match find_online_of(&query, Kind::Manga) {
                Ok(media) => reply(on_ui(handle, true, move |s| {
                    let i = s.manga_add(media, status)?;
                    Ok(Reply::Edited { entry: s.manga_ctl(i) })
                })),
                Err(message) => Reply::Error { message, anyway: false },
            }
        }
        Request::MangaRemove { title, yes } => reply(on_ui(handle, true, move |s| {
            manga_ready(s)?;
            let i = s.find_manga(&title)?;
            let id = s.manga.entries[i].id;
            if !yes {
                let message = s.manga_remove_question(id).unwrap_or_default();
                return Ok(Reply::Error { message, anyway: true });
            }
            let name = s.manga.entries[i].title.clone();
            s.remove_manga(id)?;
            Ok(Reply::Done { message: format!("Removed {name} from your manga list. It waits in the sync queue (kot queue); Undo it there in Kotora to keep the manga.") })
        })),
        // `kot fill-volumes`: the window's manga list, looked up on this thread (MangaDex, the
        // saved answers first), then the preview held, or with --yes the volumes queued.
        Request::FillVolumes { yes } => {
            let ready = on_ui(handle, false, |s| -> Result<(Vec<kotora_core::ListItem>, ServiceId), String> {
                manga_ready(s)?;
                if kotora_core::paths::offline_forced() || s.offline {
                    return Err("Offline: volumes from chapters need MangaDex.".into());
                }
                Ok((crate::volfill::candidates(s), s.service))
            });
            let (items, service) = match ready {
                Ok(Ok(x)) => x,
                Ok(Err(message)) | Err(message) => return Reply::Error { message, anyway: false },
            };
            if items.is_empty() {
                return Reply::Done { message: "Nothing to look up: no manga has chapters read with volumes short of its total.".into() };
            }
            let rows = match crate::volfill::look_up(items, service, false, &std::sync::atomic::AtomicBool::new(false), |_, _| {}) {
                Ok(r) => r,
                Err(message) => return Reply::Error { message, anyway: false },
            };
            let plan = kotora_core::mangadex::plan_fill(&rows);
            let text = crate::volfill::plan_text(&plan);
            if plan.fill.is_empty() {
                return Reply::Done { message: text };
            }
            if !yes {
                return Reply::Error { message: text, anyway: true };
            }
            reply(on_ui(handle, true, move |s| {
                let actions = kotora_core::mangadex::fill_actions(&plan);
                let n = actions.len();
                s.volfill.applying = true;
                crate::queue::send(s, crate::queue::Cmd::FillVolumes(actions));
                Ok(Reply::Done { message: s.queued_text(n, "volume change", "volume changes") })
            }))
        }
        // Off the UI thread: reads the database and writes the file.
        Request::Export { path } => match crate::transfer::export_to(std::path::Path::new(&path), kotora_core::Kind::Anime) {
            Ok(message) => Reply::Done { message },
            Err(message) => Reply::Error { message, anyway: false },
        },
        Request::ExportManga { path } => match crate::transfer::export_to(std::path::Path::new(&path), kotora_core::Kind::Manga) {
            Ok(message) => Reply::Done { message },
            Err(message) => Reply::Error { message, anyway: false },
        },
        // Off the UI thread: the window holds only the anime list; this reads the database.
        // The window tells whether the last manga sync failed.
        Request::MangaList { status: st } => match on_ui(handle, false, |s| (s.has_list(), s.manga.error.clone())) {
            Ok((true, failed)) => match open_store().and_then(|store| manga_list(&store, crate::account::active(), st.as_deref(), failed)) {
                Ok(r) => r,
                Err(message) => Reply::Error { message, anyway: false },
            },
            Ok((false, _)) => Reply::Error { message: "Not signed in.".into(), anyway: false },
            Err(message) => Reply::Error { message, anyway: false },
        },
        Request::Manga { on } => match open_store().and_then(|store| manga_switch(&store, crate::account::active(), on)) {
            Err(message) => Reply::Error { message, anyway: false },
            Ok(message) if on != Some(true) => {
                // Off: the window's switch goes back to Anime.
                if on == Some(false) {
                    let _ = on_ui(handle, true, |s| {
                        s.manga.enabled = false;
                        if s.manga.mode {
                            s.set_kind(Kind::Anime);
                        }
                    });
                }
                Reply::Done { message }
            }
            // Turned on: one sync now reads the manga list (after the anime list).
            Ok(message) => match on_ui(handle, true, |s| {
                s.manga.enabled = true;
                let signed_in = s.has_list();
                if signed_in {
                    s.sync_now();
                }
                signed_in
            }) {
                Ok(true) => Reply::Done { message: format!("{message} Kotora is reading your manga list now: see it in a few seconds with kot list --manga.") },
                _ => Reply::Done { message: format!("{message} Sign in, then kot sync reads it.") },
            },
        },
        Request::Next { title } => reply(on_ui(handle, true, move |s| {
            let i = s.find_for(&title, Want::Next)?;
            let id = s.entries[i].id;
            crate::library::play_next(s, id).map(|message| Reply::Done { message: format!("{message}.") })
        })),
        Request::Sync => reply(on_ui(handle, true, |s| {
            if !s.has_list() {
                return Err("Not signed in.".to_string());
            }
            s.sync_now();
            Ok(Reply::Status(s.ctl_status()))
        })),
    }
}

/// `kot +1`: an episode that hasn't aired yet (by the estimate) is held: the reply says so and
/// `kot` asks "Mark anyway?" (or says to run it again with `--anyway`).
fn plus_one(s: &mut State, title: &str, anyway: bool) -> Result<Reply, String> {
    let i = s.find_for(title, Want::Watching)?;
    let (id, next) = (s.entries[i].id, s.entries[i].progress + 1);
    if !anyway && let Some(message) = s.not_aired_text(id, &Change { progress: Some(next), ..Default::default() }) {
        return Ok(Reply::Error { message, anyway: true });
    }
    let i = s.plus_one(id)?;
    Ok(Reply::Edited { entry: s.entries[i].ctl(s.score_format()) })
}

/// `kot set`: as `kot +1`, an episode not aired yet is held. Past the count is refused.
/// `kot remove`: held with the question unless `yes`; then off the list, the removal queued
/// (the app's toast offers Undo too).
fn remove(s: &mut State, title: &str, yes: bool) -> Result<Reply, String> {
    if !s.has_list() {
        return Err("Not signed in.".into());
    }
    let i = match title.trim().parse::<u32>().ok().and_then(|id| s.entries.iter().position(|e| e.id == id)) {
        Some(i) => i,
        None => s.find(title)?,
    };
    let id = s.entries[i].id;
    if !yes {
        let message = s.remove_question(id).unwrap_or_default();
        return Ok(Reply::Error { message, anyway: true });
    }
    let name = s.entries[i].title.clone();
    let also = s.mirror_target().map(|o| format!(" The mirror removes it from {o} too.")).unwrap_or_default();
    s.remove_entry(id)?;
    Ok(Reply::Done { message: format!("Removed {name} from your list. It waits in the sync queue (kot queue); Undo it there in Kotora to keep the anime.{also}") })
}

/// `kot set --score`: the text in the user's own score format (a new `kot`), else 1-10 (an older
/// one, 0 removes).
fn score_of(s: &State, ten: Option<u8>, text: Option<&str>) -> Result<Option<Option<kotora_core::Score>>, String> {
    Ok(match text {
        Some(t) => Some(s.score_format().parse(t)?),
        None => ten.map(|n| kotora_core::Score::from_ten(n.into())),
    })
}

fn set(s: &mut State, title: &str, mut change: Change, anyway: bool) -> Result<Reply, String> {
    let i = s.find_for(title, Want::Watching)?;
    let id = s.entries[i].id;
    // A score that already shows as the entry's keeps the precise one.
    if let Some(sc) = change.score {
        change.score = Some(s.score_format().pick(s.entries[i].score, sc));
    }
    if !anyway && let Some(message) = s.not_aired_text(id, &change) {
        return Ok(Reply::Error { message, anyway: true });
    }
    let i = s.edit(id, change)?;
    Ok(Reply::Edited { entry: s.entries[i].ctl(s.score_format()) })
}

/// `kot add`: a MyAnimeList id (its information page), or a search that must find one anime:
/// a title that matches one result exactly, or a single result. Runs on the connection's thread.
/// `kot add --manga` and `kot remove --manga` need a signed-in list service with Manga on.
fn manga_ready(s: &mut State) -> Result<(), String> {
    if !s.has_list() {
        return Err("Not signed in.".into());
    }
    if !s.manga.enabled {
        return Err("Manga is off: turn it on with kot manga on (or the Anime | Manga switch).".into());
    }
    Ok(())
}

fn find_online(query: &str) -> Result<kotora_core::Media, String> {
    find_online_of(query, Kind::Anime)
}

/// One anime (or manga) the query names on the service: its id, an exact title, or the only
/// result.
fn find_online_of(query: &str, kind: Kind) -> Result<kotora_core::Media, String> {
    let title = crate::account::active().title();
    let mut mal = crate::account::open().ok_or_else(|| format!("kot add needs {title}: sign in first (and be online)."))?;
    let q = query.trim();
    if let Ok(id) = q.parse::<u32>() {
        return mal.details_of(kind, id).map(|d| d.media()).map_err(crate::online::error_text);
    }
    let hits = mal.browse_of(kind, q, 10).map_err(crate::online::error_text)?;
    let lower = q.to_lowercase();
    let exact: Vec<_> = hits
        .iter()
        .filter(|h| std::iter::once(&h.title).chain(h.title_en.iter()).chain(h.title_ja.iter()).chain(h.synonyms.iter()).any(|t| t.to_lowercase() == lower))
        .collect();
    match (exact.as_slice(), hits.as_slice()) {
        ([one], _) => Ok(one.media()),
        ([], [one]) => Ok(one.media()),
        (_, []) => Err(format!("{title} found nothing for \u{201c}{q}\u{201d}.")),
        _ => {
            let names: Vec<String> = hits.iter().take(6).map(|h| format!("  {:>6}  {}", h.id, h.title)).collect();
            Err(format!("Several {} match \u{201c}{q}\u{201d}; give the id:\n{}", if kind == Kind::Manga { "manga" } else { "anime" }, names.join("\n")))
        }
    }
}

/// `watching`, `plan`, `completed`, `on-hold`, `dropped` (and MAL's names, any case).
fn parse_status(name: &str) -> Result<Status, String> {
    let n = name.trim().to_lowercase().replace([' ', '_'], "-");
    let st = match n.as_str() {
        "plan-to-watch" | "ptw" | "planned" | "plan-to-read" | "ptr" => Some(Status::PlanToWatch),
        // Manga's Reading is the anime Watching.
        "reading" => Some(Status::Watching),
        "onhold" | "hold" | "paused" => Some(Status::OnHold),
        "complete" | "done" => Some(Status::Completed),
        _ => Status::ALL.into_iter().find(|s| s.slug() == n),
    };
    st.ok_or_else(|| format!("Unknown status \u{201c}{name}\u{201d}: use watching, plan, completed, on-hold or dropped."))
}

/// `kot list`: one status (sidebar order within it: last updated first) or all of them.
fn list(s: &State, status: Option<&str>) -> Result<Reply, String> {
    if !s.has_list() {
        return Err("Not signed in.".into());
    }
    let only = status.map(parse_status).transpose()?;
    let mut entries = Vec::new();
    for st in Status::ALL.into_iter().filter(|st| only.is_none_or(|o| o == *st)) {
        let mut rows: Vec<&crate::model::Entry> = s.entries.iter().filter(|e| e.status == st).collect();
        rows.sort_by_key(|e| std::cmp::Reverse(e.updated_at));
        entries.extend(rows.into_iter().map(|e| e.ctl(s.score_format())));
    }
    Ok(Reply::List { entries, note: None })
}

fn open_store() -> Result<kotora_core::store::Store, String> {
    kotora_core::store::Store::open(&kotora_core::paths::db_file()).map_err(|e| format!("Can't open the saved list: {e}"))
}

/// `kot list --manga`: the saved manga list, read only (what the last sync read; nothing is
/// asked of the service). One status (sidebar order within it: last updated first) or all.
/// `failed`: why the last manga sync failed (from the window), if it did: said instead of the
/// list when there is none yet, else after it.
fn manga_list(store: &kotora_core::store::Store, service: ServiceId, status: Option<&str>, failed: Option<String>) -> Result<Reply, String> {
    let only = status.map(parse_status).transpose()?;
    let scope = (service.name(), Kind::Manga);
    let err = |e: kotora_core::store::StoreError| format!("Can't read the saved manga list: {e}");
    if !store.manga_enabled().map_err(err)? {
        return Err("Manga is off: turn it on with kot manga on (syncs then read your manga list too).".into());
    }
    if store.synced_at(scope).map_err(err)?.is_none() {
        return Err(match failed {
            Some(m) => format!("{m} Try kot sync again later."),
            None => "Your manga list wasn't read yet: run kot sync, then try again.".into(),
        });
    }
    let items = store.load_list(scope).map_err(err)?;
    // Scores as the user reads them on the service (AniList's own format; MyAnimeList 1-10).
    let format = match service {
        ServiceId::Mal => ScoreFormat::Point10,
        ServiceId::AniList => store.profile(service.name()).ok().flatten().map(|p| p.score_format).unwrap_or_default(),
    };
    // Open changes (none can be made yet without the UI, but the queue holds both kinds).
    let ops = store.queue(service.name(), kotora_core::time::now_unix()).map_err(err)?;
    let waiting = |id: u32| ops.iter().find(|o| o.kind == Kind::Manga && o.media_id == id && o.state != OpState::Done).map(|o| crate::model::waiting_label(o.state).to_string());
    let mut entries = Vec::new();
    for st in Status::ALL.into_iter().filter(|st| only.is_none_or(|o| o == *st)) {
        // `load_list` is newest update first already.
        entries.extend(items.iter().filter(|it| it.status == st).map(|it| kotora_ctl::Entry {
            id: it.media.id,
            title: it.media.title.clone(),
            status: st.label_in(Kind::Manga).into(),
            progress: it.progress,
            episodes: it.media.episodes,
            score: it.score.map(kotora_core::Score::to_ten),
            score_text: it.score.map(|s| format.show(s)),
            waiting: waiting(it.media.id),
            rewatching: it.rewatching,
            times_rewatched: it.times_rewatched,
            volumes: Some(it.volumes),
            volumes_total: it.media.volumes,
        }));
    }
    let note = failed.map(|m| format!("{m} This is the list saved by the last good sync."));
    Ok(Reply::List { entries, note })
}


/// `kot manga [on|off]`: the `manga:enabled` switch ([`kotora_core::store::Store::manga_enabled`]).
/// Off keeps the saved manga list; nothing is ever written to the service.
fn manga_switch(store: &kotora_core::store::Store, service: ServiceId, on: Option<bool>) -> Result<String, String> {
    let err = |e: kotora_core::store::StoreError| format!("Can't save the setting: {e}");
    let svc = service.title();
    match on {
        None => {
            let state = if store.manga_enabled().map_err(err)? { format!("on: syncs read your {svc} manga list too") } else { "off (kot manga on turns it on)".into() };
            Ok(format!("Manga is {state}."))
        }
        Some(true) => {
            store.set_manga_enabled(true).map_err(err)?;
            Ok(format!("Manga is on: syncs read your {svc} manga list too (read only)."))
        }
        Some(false) => {
            store.set_manga_enabled(false).map_err(err)?;
            Ok("Manga is off: syncs no longer read your manga list (the saved copy stays).".into())
        }
    }
}

fn on_ui<T: Send + 'static>(handle: &Handle, redraw: bool, f: impl FnOnce(&mut State) -> T + Send + 'static) -> Result<T, String> {
    let (tx, rx) = mpsc::sync_channel(1);
    handle.update_if(move |s, _| {
        let _ = tx.send(f(s));
        redraw
    });
    rx.recv_timeout(UI_TIMEOUT).map_err(|_| "Kotora didn't answer in time".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kotora_core::store::Store;
    use kotora_core::{ListItem, Media, Score};

    fn manga(id: u32, title: &str, status: Status, ch: u32, vol: u32, updated_at: i64) -> ListItem {
        ListItem {
            media: Media { kind: Kind::Manga, id, title: title.into(), episodes: Some(120), volumes: Some(12), ..Media::default() },
            status,
            progress: ch,
            score: Score::from_ten(8),
            rewatching: false,
            times_rewatched: 0,
            volumes: vol,
            updated_at,
        }
    }

    fn rows(r: Result<Reply, String>) -> Vec<(String, String, u32, Option<u32>)> {
        match r {
            Ok(Reply::List { entries, .. }) => entries.into_iter().map(|e| (e.title, e.status, e.progress, e.volumes)).collect(),
            other => panic!("not a list: {other:?}"),
        }
    }

    /// `kot manga` and `kot list --manga`: off by default, read from the saved
    /// list (Reading first, newest update first), with chapters and volumes; anime never shows.
    /// `kot add --manga` / `kot remove --manga`: Manga on (MyAnimeList or AniList); remove asks first, then
    /// takes it off the manga list (the anime with the same id stays).
    #[test]
    fn kot_adds_and_removes_manga_only_with_manga_on() {
        let mut s = State::fake();
        assert!(manga_ready(&mut s).unwrap_err().starts_with("Manga is off"));
        s.service = ServiceId::AniList;
        assert!(manga_ready(&mut s).unwrap_err().starts_with("Manga is off"), "AniList too: only Manga must be on");
        let mut s = State::fake();
        crate::manga::fake(&mut s);
        assert!(manga_ready(&mut s).is_ok());
        assert_eq!(s.find_manga("salt").map(|i| s.manga.entries[i].id), Ok(2));
        assert_eq!(s.find_manga("2").map(|i| s.manga.entries[i].id), Ok(2), "an id");
        assert!(s.find_manga("a").unwrap_err().starts_with("Several manga match"));
        let i = s.find_manga("salt").unwrap();
        assert_eq!(s.manga_ctl(i).volumes, Some(0));
        s.remove_manga(2).unwrap();
        assert!(s.find_manga("salt").is_err());
        assert!(s.entries.iter().any(|e| e.id == 2), "the anime 2 stays");
    }

    #[test]
    fn kot_lists_the_saved_manga() {
        let mut store = Store::open_memory().unwrap();
        let mut anime = manga(5, "An anime", Status::Watching, 3, 0, 1);
        anime.media.kind = Kind::Anime;
        store.replace_list("mal", &[anime], 1).unwrap();
        assert!(manga_switch(&store, ServiceId::Mal, None).unwrap().starts_with("Manga is off"));
        assert!(manga_list(&store, ServiceId::Mal, None, None).unwrap_err().starts_with("Manga is off"));
        manga_switch(&store, ServiceId::Mal, Some(true)).unwrap();
        assert!(store.manga_enabled().unwrap());
        assert!(manga_list(&store, ServiceId::Mal, None, None).unwrap_err().contains("kot sync"), "not read yet");
        // A failed manga sync is said instead (`kot list --manga` never says "not read yet" then).
        let failed = || Some("Your manga list wasn't read: can't reach MyAnimeList.".to_string());
        assert_eq!(manga_list(&store, ServiceId::Mal, None, failed()).unwrap_err(), "Your manga list wasn't read: can't reach MyAnimeList. Try kot sync again later.");
        let list = [manga(2, "Paper Lanterns", Status::Watching, 21, 3, 50), manga(3, "Iron Garden", Status::Completed, 120, 12, 60), manga(4, "Salt Road", Status::Watching, 7, 1, 70)];
        store.replace_list(("mal", Kind::Manga), &list, 100).unwrap();
        assert_eq!(
            rows(manga_list(&store, ServiceId::Mal, None, None)),
            [("Salt Road".into(), "Reading".into(), 7, Some(1)), ("Paper Lanterns".into(), "Reading".into(), 21, Some(3)), ("Iron Garden".into(), "Completed".into(), 120, Some(12))]
        );
        assert_eq!(rows(manga_list(&store, ServiceId::Mal, Some("reading"), None)).len(), 2);
        assert_eq!(rows(manga_list(&store, ServiceId::Mal, Some("completed"), None)).len(), 1);
        assert!(rows(manga_list(&store, ServiceId::Mal, Some("plan-to-read"), None)).is_empty());
        let Ok(Reply::List { entries, .. }) = manga_list(&store, ServiceId::Mal, Some("completed"), None) else { panic!() };
        // After a good sync, a failed one shows the saved list with a note.
        let Ok(Reply::List { note, entries: saved }) = manga_list(&store, ServiceId::Mal, None, failed()) else { panic!() };
        assert_eq!(saved.len(), 3);
        assert!(note.is_some_and(|n| n.ends_with("This is the list saved by the last good sync.")));
        assert_eq!((entries[0].episodes, entries[0].volumes_total, entries[0].score_text.as_deref()), (Some(120), Some(12), Some("8")));
        assert!(manga_list(&store, ServiceId::AniList, None, None).unwrap_err().contains("kot sync"), "AniList's manga list is its own: not read yet");
        // Off: syncs stop reading it, the saved copy stays.
        assert!(manga_switch(&store, ServiceId::Mal, Some(false)).unwrap().starts_with("Manga is off"));
        assert_eq!(store.load_list(("mal", Kind::Manga)).unwrap().len(), 3);
        assert_eq!(store.load_list("mal").unwrap().len(), 1, "the anime list untouched");
    }

    /// AniList: `kot manga on` works, and `kot list --manga` shows AniList's own saved manga
    /// (never MyAnimeList's), with scores in the user's AniList score format.
    #[test]
    fn kot_manga_works_on_anilist() {
        let mut store = Store::open_memory().unwrap();
        assert_eq!(manga_switch(&store, ServiceId::AniList, None).unwrap(), "Manga is off (kot manga on turns it on).");
        assert_eq!(manga_switch(&store, ServiceId::AniList, Some(true)).unwrap(), "Manga is on: syncs read your AniList manga list too (read only).");
        assert!(store.manga_enabled().unwrap());
        assert_eq!(manga_switch(&store, ServiceId::AniList, None).unwrap(), "Manga is on: syncs read your AniList manga list too.");
        store.replace_list(("mal", Kind::Manga), &[manga(2, "On MyAnimeList", Status::Watching, 1, 0, 1)], 1).unwrap();
        assert!(manga_list(&store, ServiceId::AniList, None, None).unwrap_err().contains("kot sync"), "not read yet");
        let mut reread = manga(190002, "Iron Garden", Status::Completed, 4, 0, 60);
        reread.rewatching = true;
        reread.times_rewatched = 1;
        reread.score = Score::from_points(73);
        store.replace_list(("anilist", Kind::Manga), &[manga(190001, "Paper Lanterns", Status::Watching, 21, 3, 50), reread], 100).unwrap();
        let entries = |store: &Store| match manga_list(store, ServiceId::AniList, None, None) {
            Ok(Reply::List { entries, .. }) => entries,
            other => panic!("not a list: {other:?}"),
        };
        let got = entries(&store);
        assert_eq!(got.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Paper Lanterns", "Iron Garden"], "AniList's own, not MyAnimeList's");
        assert_eq!((got[1].status.as_str(), got[1].rewatching, got[1].times_rewatched), ("Completed", true, 1), "rereading");
        // No profile saved yet: the window's default too (1-10).
        assert_eq!(got[1].score_text.as_deref(), Some("7"));
        for (format, text) in [(ScoreFormat::Point100, "73"), (ScoreFormat::Point10Decimal, "7.3"), (ScoreFormat::Point10, "7"), (ScoreFormat::Point5, "★★★★☆")] {
            store.set_profile("anilist", &kotora_core::service::Profile { score_format: format, ..Default::default() }).unwrap();
            assert_eq!(entries(&store)[1].score_text.as_deref(), Some(text), "{format:?}");
        }
        assert_eq!((got[0].volumes, got[0].volumes_total), (Some(3), Some(12)));
    }

    /// `kot +1` and `kot set` hold an episode that hasn't aired yet; `--anyway` marks it.
    #[test]
    fn kot_asks_before_an_episode_not_aired_yet() {
        let mut s = State::fake();
        crate::model::airing_entry(&mut s, 0, 5);
        let p = |s: &State| s.entries.iter().find(|e| e.id == 0).unwrap().progress;
        match plus_one(&mut s, "Kirigaoka", false) {
            Ok(Reply::Error { message, anyway: true }) => assert!(message.starts_with("Episode 6 of Lanterns of Kirigaoka hasn't aired yet"), "{message}"),
            other => panic!("not held: {other:?}"),
        }
        assert_eq!(p(&s), 5);
        assert!(matches!(plus_one(&mut s, "Kirigaoka", true), Ok(Reply::Edited { .. })));
        assert_eq!(p(&s), 6);
        let set9 = || Change { progress: Some(9), ..Default::default() };
        assert!(matches!(set(&mut s, "Kirigaoka", set9(), false), Ok(Reply::Error { anyway: true, .. })));
        assert_eq!(p(&s), 6);
        assert!(matches!(set(&mut s, "Kirigaoka", set9(), true), Ok(Reply::Edited { .. })));
        assert_eq!(p(&s), 9);
        // Past the count: a plain error, anyway or not.
        assert!(set(&mut s, "Kirigaoka", Change { progress: Some(13), ..Default::default() }, true).is_err());
        // An aired episode goes through at once.
        let mut s = State::fake();
        crate::model::airing_entry(&mut s, 0, 8);
        assert!(matches!(plus_one(&mut s, "Kirigaoka", false), Ok(Reply::Edited { .. })));
    }

    /// `kot remove` asks first (held with the question), then takes it off the list.
    #[test]
    fn kot_remove_asks_then_removes() {
        let mut s = State::fake();
        let id = s.entries[1].id;
        let title = s.entries[1].title.clone();
        match remove(&mut s, &title, false) {
            Ok(Reply::Error { message, anyway: true }) => assert!(message.starts_with(&format!("Remove {title} from your list?")), "{message}"),
            other => panic!("not held: {other:?}"),
        }
        assert!(s.entries.iter().any(|e| e.id == id));
        assert!(matches!(remove(&mut s, &id.to_string(), true), Ok(Reply::Done { .. })), "by id");
        assert!(!s.entries.iter().any(|e| e.id == id));
        assert!(remove(&mut s, &title, true).is_err(), "no longer on the list");
    }

    /// `kot set --score` reads the score in the user's format and keeps a precise score that
    /// already shows as the one typed; `kot list` gets the score written that way.
    #[test]
    fn kot_scores_in_the_users_format() {
        use kotora_core::{Score, ScoreFormat};
        let mut s = State::fake();
        s.service = kotora_core::service::ServiceId::AniList;
        let fmt = |s: &mut State, f: ScoreFormat| s.profile = Some(kotora_core::service::Profile { score_format: f, ..Default::default() });
        let score = |s: &State| s.entries.iter().find(|e| e.id == 0).unwrap().score;
        let run = |s: &mut State, text: &str| {
            let sc = score_of(s, None, Some(text))?;
            set(s, "Kirigaoka", Change { score: sc, ..Default::default() }, true).map(|r| match r {
                Reply::Edited { entry } => entry,
                other => panic!("{other:?}"),
            })
        };
        fmt(&mut s, ScoreFormat::Point10Decimal);
        let e = run(&mut s, "7.3").unwrap();
        assert_eq!((score(&s), e.score_text.as_deref(), e.score), (Score::from_points(73), Some("7.3"), Some(7)));
        fmt(&mut s, ScoreFormat::Point5);
        let e = run(&mut s, "4").unwrap();
        assert_eq!((score(&s), e.score_text.as_deref()), (Score::from_points(73), Some("\u{2605}\u{2605}\u{2605}\u{2605}\u{2606}")), "4 stars again: still 73");
        run(&mut s, "*****").unwrap();
        assert_eq!(score(&s), Score::from_points(100));
        fmt(&mut s, ScoreFormat::Point3);
        assert!(run(&mut s, "5").unwrap_err().contains("1 to 3"));
        run(&mut s, "none").unwrap();
        assert_eq!(score(&s), None);
        // MyAnimeList: 1-10 only; an older `kot`'s number is 1-10 too.
        s.service = kotora_core::service::ServiceId::Mal;
        assert!(run(&mut s, "7.5").is_err());
        assert_eq!(score_of(&s, Some(8), None), Ok(Some(Score::from_ten(8))));
        assert_eq!(score_of(&s, Some(0), None), Ok(Some(None)));
    }

    /// The dashboard gathers a 300-entry list (with a library and a season of airing shows) well
    /// inside the 20 ms budget, and the whole round trip over the socket does too.
    #[test]
    fn the_dashboard_is_gathered_fast() {
        let mut s = State::fake();
        crate::library::fake(&mut s);
        crate::model::airing_entry(&mut s, 0, 5);
        let template = s.entries.clone();
        let mut n = 10_000;
        while n < 10_300 {
            for e in &template {
                let mut e = e.clone();
                e.id = n;
                e.status = Status::Watching;
                n += 1;
                s.entries.push(e);
            }
        }
        let started = std::time::Instant::now();
        let dash = s.ctl_dash();
        let gather = started.elapsed();
        assert!(dash.rows.len() >= 300, "{} rows", dash.rows.len());
        assert!(dash.rows.iter().any(|r| r.on_disk || r.air_at.is_some()), "the fake list has something to show");
        let path = std::env::temp_dir().join(format!("kotora-dash-test-{}", std::process::id())).join("ctl.sock");
        let server = kotora_ctl::serve(&path, move |_| Reply::Dash(dash.clone())).unwrap();
        let mut client = kotora_ctl::Client::connect_to(server.path()).unwrap();
        client.request(&Request::Dash).unwrap();
        let started = std::time::Instant::now();
        assert!(matches!(client.request(&Request::Dash).unwrap(), Reply::Dash(_)));
        let trip = started.elapsed();
        eprintln!("dash: gather {gather:?}, socket round trip {trip:?}");
        assert!(gather + trip < Duration::from_millis(20), "gather {gather:?} + round trip {trip:?}");
    }
}
