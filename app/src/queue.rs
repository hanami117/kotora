//! The sync queue's worker (core's `queue.rs` does the work; this is its thread).
//!
//! The UI thread never touches the database: edits and queue actions go to the worker as
//! [`Cmd`]s. The worker thread starts with the first command and ends when nothing is left to
//! send, so an idle app has no thread and no timer. While a change waits for a retry (no network,
//! rate limit), the thread sleeps until that time, or until the next command wakes it.
//!
//! Every change reaches the UI as [`Msg::Queue`] through `AppHandle::update_if`. Nothing here runs
//! headless: without an app handle, [`send`] does nothing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::Duration;

use kotora_core::queue::{self, Outcome, Step};
use kotora_core::store::Store;
use kotora_core::time::now_unix;
use kotora_core::{Change, Media, Status, paths};

use crate::model::{Fail, Handle, Msg, State};


#[derive(Clone, Debug)]
pub enum Cmd {
    Edit { media_id: u32, change: Change },
    /// Put an anime on the list (search and add), at `progress` when given.
    Add { media: Box<Media>, status: Status, progress: Option<u32> },
    /// Send what waits now (no backoff wait); `refresh`: read the whole list afterwards.
    Kick { refresh: bool },
    KeepMine(i64),
    TakeTheirs(i64),
    Discard(i64),
    /// An import's adds and updates (`transfer.rs`), queued in one transaction.
    Import(Vec<kotora_core::listxml::Action>),
    /// Takes back an import's changes that still wait.
    UndoImport(Vec<i64>),
}

/// The worker's mailbox. `slot` holds the sender while a worker thread runs.
#[derive(Default)]
pub struct Worker {
    slot: Mutex<Option<Sender<Cmd>>>,
    /// The sign-in is gone: keep the queue, send nothing until the user signs in again.
    pub hold: AtomicBool,
    /// Settings → Updates: "Keep the higher progress in conflicts" is OFF (it is on by default).
    pub ask_on_higher: AtomicBool,
}

/// Hands `cmd` to the worker, starting it if it isn't running.
pub fn send(s: &mut State, cmd: Cmd) {
    let Some(h) = s.handle.clone() else { return };
    let w = s.worker.clone();
    let mut slot = w.slot.lock().unwrap_or_else(|e| e.into_inner());
    let cmd = match slot.as_ref() {
        Some(tx) => match tx.send(cmd) {
            Ok(()) => return,
            Err(mpsc::SendError(cmd)) => cmd,
        },
        None => cmd,
    };
    let (tx, rx) = mpsc::channel();
    let _ = tx.send(cmd);
    *slot = Some(tx);
    drop(slot);
    let w2 = w.clone();
    let spawned = std::thread::Builder::new().name("kotora-queue".into()).spawn(move || run(&h, &w2, &rx));
    if spawned.is_err() {
        *w.slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

fn report(h: &Handle, msg: Msg) {
    h.update_if(move |s, _| s.apply(msg));
}

/// The queue and the touched entries, as the UI shows them.
fn snapshot(store: &Store, touched: &[u32], offline: Option<bool>) -> Msg {
    let now = now_unix();
    let ops = store.queue(crate::account::name(), now).unwrap_or_default();
    let mut items = Vec::new();
    let mut gone = Vec::new();
    for &id in touched {
        match store.load_item(crate::account::name(), id) {
            Ok(Some(it)) => items.push(it),
            Ok(None) => gone.push(id),
            Err(_) => {}
        }
    }
    Msg::Queue { ops, items, gone, offline }
}

/// An import or its undo: applied, then the result goes to the import panel. Returns the entries
/// touched.
fn transfer(h: &Handle, store: &mut Store, cmd: Cmd) -> Vec<u32> {
    use kotora_core::listxml::Action;
    let service = crate::account::name();
    match cmd {
        Cmd::Import(actions) => {
            let touched: Vec<u32> = actions
                .iter()
                .map(|a| match a {
                    Action::Add { media, .. } => media.id,
                    Action::Update { media_id, .. } => *media_id,
                })
                .collect();
            let r = store.import(service, &actions, now_unix()).map_err(|e| format!("The import can't be saved: {e}"));
            let ok = r.is_ok();
            h.update_if(move |s, _| crate::transfer::imported(s, r));
            if ok { touched } else { Vec::new() }
        }
        Cmd::UndoImport(ids) => {
            let now = now_unix();
            let touched: Vec<u32> = store.queue(service, now).unwrap_or_default().into_iter().filter(|o| ids.contains(&o.id)).map(|o| o.media_id).collect();
            let r = store.undo_import(service, &ids, now).map_err(|e| format!("The import can't be taken back: {e}"));
            h.update_if(move |s, _| crate::transfer::undone(s, r));
            touched
        }
        _ => Vec::new(),
    }
}

/// Applies one command to the store; returns the entry it touched.
fn apply(store: &mut Store, cmd: Cmd, refresh: &mut bool) -> Result<Option<u32>, String> {
    let now = now_unix();
    let media = |store: &Store, id: i64| store.queue(crate::account::name(), now).ok().and_then(|q| q.into_iter().find(|o| o.id == id)).map(|o| o.media_id);
    let r = match cmd {
        Cmd::Edit { media_id, change } => store.enqueue(crate::account::name(), media_id, change, now).map(|_| Some(media_id)),
        Cmd::Add { media, status, progress } => store.add_to_list(crate::account::name(), &media, status, progress, now).map(|_| Some(media.id)),
        Cmd::Kick { refresh: r } => {
            *refresh |= r;
            store.retry_now(crate::account::name()).map(|_| None)
        }
        Cmd::KeepMine(id) => {
            let m = media(store, id);
            store.keep_mine(id).map(|_| m)
        }
        Cmd::TakeTheirs(id) => {
            let m = media(store, id);
            store.take_theirs(crate::account::name(), id, now).map(|_| m)
        }
        Cmd::Discard(id) => {
            let m = media(store, id);
            store.discard(crate::account::name(), id, now).map(|_| m)
        }
        // Handled by `transfer`.
        Cmd::Import(_) | Cmd::UndoImport(_) => Ok(None),
    };
    r.map_err(|e| e.to_string())
}

/// The service, if Kotora may send now: online, signed in, not held.
fn service(w: &Worker) -> Option<crate::account::Service> {
    if w.hold.load(Ordering::Relaxed) {
        return None;
    }
    crate::account::open()
}

fn run(h: &Handle, w: &Worker, rx: &Receiver<Cmd>) {
    let mut store = match Store::open(&paths::db_file()) {
        Ok(s) => s,
        Err(e) => {
            *w.slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
            return report(h, Msg::QueueError(format!("Can't open the saved list: {e}")));
        }
    };
    let _ = store.queue_start(crate::account::name(), now_unix());
    let mut svc: Option<crate::account::Service> = None;
    let mut refresh = false;
    let mut next: Option<Cmd> = None;
    loop {
        // Commands first: edits made while sending merge before the next send.
        let mut touched = Vec::new();
        let mut added = Vec::new();
        while let Some(cmd) = next.take().or_else(|| rx.try_recv().ok()) {
            if let Cmd::Add { media, .. } = &cmd {
                added.push(media.id);
            }
            if matches!(cmd, Cmd::Import(_) | Cmd::UndoImport(_)) {
                // Read the whole list once the queue is empty: the imported adds get their
                // covers and episodes from the service.
                refresh |= matches!(cmd, Cmd::Import(_));
                touched.extend(transfer(h, &mut store, cmd));
                continue;
            }
            match apply(&mut store, cmd, &mut refresh) {
                Ok(Some(id)) => touched.push(id),
                Ok(None) => {}
                Err(m) => report(h, Msg::QueueError(m)),
            }
        }
        if !touched.is_empty() {
            report(h, snapshot(&store, &touched, None));
            // The same changes may wait for the mirror now.
            crate::mirror::kick(h, false);
        }
        // A switch to the other service: its client from now on.
        if svc.as_ref().is_some_and(|c| c.name() != crate::account::name()) {
            svc = None;
        }
        if svc.is_none() {
            svc = service(w);
        }
        // An add that knew only the title (Now Playing): its episodes, type and cover now, not
        // at the next sync.
        if !added.is_empty()
            && let Some(service) = svc.as_deref_mut()
        {
            let done = kotora_core::sync::complete_media(service, &store, Some(&added), now_unix());
            if !done.is_empty() {
                report(h, snapshot(&store, &done, None));
            }
        }
        let wait = match svc.as_deref_mut() {
            Some(service) if !w.hold.load(Ordering::Relaxed) => match queue::step_with(&mut store, service, now_unix(), !w.ask_on_higher.load(Ordering::Relaxed)) {
                Ok(Step::Handled { kind, media_id, outcome }) => {
                    let offline = match outcome {
                        Outcome::Retry(_) => Some(true),
                        Outcome::Sent | Outcome::AlreadyThere => Some(false),
                        _ => None,
                    };
                    // The list page shows anime only until Manga mode has a UI: a manga
                    // change refreshes the queue and the badge, no anime row.
                    let ids: &[u32] = if kind == kotora_core::Kind::Anime { &[media_id] } else { &[] };
                    report(h, snapshot(&store, ids, offline));
                    continue;
                }
                Ok(Step::Wait(secs)) => Some(Duration::from_secs(secs.clamp(1, queue::MAX_BACKOFF) as u64)),
                Ok(Step::Idle) => None,
                Ok(Step::SignInAgain) => {
                    w.hold.store(true, Ordering::Relaxed);
                    report(h, snapshot(&store, &[], None));
                    report(h, Msg::SyncFailed(Fail::SignInAgain));
                    None
                }
                Err(e) => {
                    report(h, Msg::QueueError(format!("The sync queue can't be saved: {e}")));
                    None
                }
            },
            _ => None,
        };
        match wait {
            Some(d) => match rx.recv_timeout(d) {
                Ok(cmd) => next = Some(cmd),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => {
                // Nothing to send: end, unless a command slipped in. Senders hold the lock to
                // send, so after this check none can be lost.
                let mut slot = w.slot.lock().unwrap_or_else(|e| e.into_inner());
                match rx.try_recv() {
                    Ok(cmd) => next = Some(cmd),
                    Err(_) => {
                        *slot = None;
                        break;
                    }
                }
            }
        }
    }
    if refresh {
        h.update_if(|s, _| {
            crate::online::refresh(s);
            true
        });
    }
}
