//! The control socket for `kot` (protocol in `kotora_ctl`), and single instance.
//!
//! Each request runs on its connection's thread and hops to the UI thread
//! (`AppHandle::update_if`) for the answer. Adapted from Uzumaki's `app/src/ctl.rs`.

use std::sync::mpsc;
use std::time::Duration;

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
    let r = match req {
        Request::Status => on_ui(handle, false, |s| s.ctl_status()),
        Request::Show => {
            handle.open();
            on_ui(handle, true, |s| {
                s.window = true;
                s.ctl_status()
            })
        }
        Request::Quit => {
            let r = on_ui(handle, false, |s| s.ctl_status());
            let handle = handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(QUIT_DELAY);
                handle.quit();
            });
            r
        }
    };
    match r {
        Ok(st) => Reply::Status(st),
        Err(message) => Reply::Error { message },
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
