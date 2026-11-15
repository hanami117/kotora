//! The control socket for `kot` (protocol in `kotora_ctl`), and single instance.
//!
//! Each request runs on its connection's thread and hops to the UI thread
//! (`AppHandle::update_if`) for the answer. Adapted from Uzumaki's `app/src/ctl.rs`.

use std::sync::mpsc;
use std::time::Duration;

use kotora_core::{Change, Status};
use kotora_ctl::{Reply, Request, Server};

use crate::model::{Handle, State};

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
        let _ = client.request(&Request::Show);
    }
    true
}

fn answer(handle: &Handle, req: Request) -> Reply {
    let status = |r: Result<kotora_ctl::Status, String>| match r {
        Ok(st) => Reply::Status(st),
        Err(message) => Reply::Error { message },
    };
    let reply = |r: Result<Result<Reply, String>, String>| match r.and_then(|r| r) {
        Ok(reply) => reply,
        Err(message) => Reply::Error { message },
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
        Request::PlusOne { title } => reply(on_ui(handle, true, move |s| {
            let i = s.find(&title)?;
            let i = s.plus_one(s.entries[i].id)?;
            Ok(Reply::Edited { entry: s.entries[i].ctl() })
        })),
        Request::Set { title, progress, status: st, score } => reply(on_ui(handle, true, move |s| {
            let i = s.find(&title)?;
            let status = match st.as_deref() {
                Some(name) => Some(parse_status(name)?),
                None => None,
            };
            let change = Change { status, progress, score: score.map(|n| (n > 0).then_some(n)) };
            let i = s.edit(s.entries[i].id, change)?;
            Ok(Reply::Edited { entry: s.entries[i].ctl() })
        })),
        Request::Queue => reply(on_ui(handle, false, |s| Ok(Reply::Queue { ops: s.ctl_queue() }))),
        Request::Now => reply(on_ui(handle, false, |s| Ok(Reply::Now { now: s.ctl_now() }))),
        Request::Confirm => reply(on_ui(handle, true, |s| {
            let (title, _) = s.record(false)?;
            let i = s.entries.iter().position(|e| e.title == title).ok_or("That anime is not on your list.")?;
            Ok(Reply::Edited { entry: s.entries[i].ctl() })
        })),
        Request::Dismiss => reply(on_ui(handle, true, |s| {
            if s.now.is_none() {
                return Err("Nothing is playing.".to_string());
            }
            s.dismiss_now();
            Ok(Reply::Now { now: s.ctl_now() })
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
        entries.extend(rows.into_iter().map(|e| e.ctl()));
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
