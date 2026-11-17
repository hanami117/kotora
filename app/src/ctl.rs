//! The control socket for `kot` (protocol in `kotora_ctl`), and single instance.
//!
//! Each request runs on its connection's thread and hops to the UI thread
//! (`AppHandle::update_if`) for the answer. Adapted from Uzumaki's `app/src/ctl.rs`.

use std::sync::mpsc;
use std::time::Duration;

use kotora_core::{Change, Status};
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
        // Off the UI thread: reads the database and writes the file.
        Request::Export { path } => match crate::transfer::export_to(std::path::Path::new(&path)) {
            Ok(message) => Reply::Done { message },
            Err(message) => Reply::Error { message, anyway: false },
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
fn find_online(query: &str) -> Result<kotora_core::Media, String> {
    let title = crate::account::active().title();
    let mut mal = crate::account::open().ok_or_else(|| format!("kot add needs {title}: sign in first (and be online)."))?;
    let q = query.trim();
    if let Ok(id) = q.parse::<u32>() {
        return mal.details(id).map(|d| d.media()).map_err(crate::online::error_text);
    }
    let hits = mal.browse(q, 10).map_err(crate::online::error_text)?;
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
            Err(format!("Several anime match \u{201c}{q}\u{201d}; give the id:\n{}", names.join("\n")))
        }
    }
}

/// `watching`, `plan`, `completed`, `on-hold`, `dropped` (and MAL's names, any case).
fn parse_status(name: &str) -> Result<Status, String> {
    let n = name.trim().to_lowercase().replace([' ', '_'], "-");
    let st = match n.as_str() {
        "plan-to-watch" | "ptw" | "planned" => Some(Status::PlanToWatch),
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
    Ok(Reply::List { entries })
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
