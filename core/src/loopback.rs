//! The loopback listener of the OAuth redirect: `http://127.0.0.1:47321/callback?code=…&state=…`.
//! It only runs while the user is signing in (a few minutes at most), then closes.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::mal::PORT;

/// Binds the fixed MAL port on the loopback address only.
pub fn bind() -> std::io::Result<TcpListener> {
    bind_on(PORT)
}

pub fn bind_on(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(("127.0.0.1", port))
}

/// Waits for the redirect and returns the `code`. Other requests (a favicon) are answered and
/// ignored. Fails on a wrong `state`, an `error` from MAL, the timeout, or `cancel`.
pub fn wait_for_code(listener: &TcpListener, state: &str, timeout: Duration, cancel: &AtomicBool) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("sign-in cancelled".into());
        }
        if Instant::now() >= deadline {
            return Err("sign-in timed out".into());
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(r) = handle(stream, state) {
                    return r;
                }
            }
            // Nothing yet. The wait is bounded and only runs during a sign-in the user started.
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// `Some(result)` for the callback request, `None` for anything else.
fn handle(mut stream: TcpStream, state: &str) -> Option<Result<String, String>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while buf.len() < 8192 && !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let line = head.lines().next().unwrap_or("");
    let target = line.strip_prefix("GET ").and_then(|r| r.split(' ').next()).unwrap_or("");
    let Some(query) = target.strip_prefix("/callback?") else {
        respond(&mut stream, 404, "Not found");
        return None;
    };
    let mut code = None;
    let mut got_state = None;
    let mut error = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k {
            "code" => code = Some(pct_decode(v)),
            "state" => got_state = Some(pct_decode(v)),
            "error" => error = Some(pct_decode(v)),
            _ => {}
        }
    }
    let result = if let Some(e) = error {
        Err(format!("MyAnimeList refused the sign-in ({e})"))
    } else if got_state.as_deref() != Some(state) {
        Err("sign-in state didn't match; try again".to_string())
    } else {
        code.filter(|c| !c.is_empty()).ok_or_else(|| "no code in the redirect".to_string())
    };
    match &result {
        Ok(_) => respond(&mut stream, 200, "Kotora is signed in to MyAnimeList. You can close this tab."),
        Err(e) => respond(&mut stream, 400, &format!("Kotora could not sign in: {e}")),
    }
    Some(result)
}

fn respond(stream: &mut TcpStream, status: u16, message: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let message = message.replace('&', "&amp;").replace('<', "&lt;");
    let body = format!("<!doctype html><meta charset=utf-8><title>Kotora</title><body style=\"font:16px system-ui;margin:3em\"><p>{message}</p>");
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

pub fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if b.len() >= i + 3 => {
                if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Sends one raw request to the listener and returns the response text.
    fn get(port: u16, target: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    fn run(targets: Vec<&'static str>, state: &'static str) -> (Result<String, String>, Vec<String>) {
        let l = bind_on(0).unwrap();
        let port = l.local_addr().unwrap().port();
        let cancel = Arc::new(AtomicBool::new(false));
        let t = std::thread::spawn(move || wait_for_code(&l, state, Duration::from_secs(10), &cancel));
        let replies: Vec<String> = targets.into_iter().map(|t| get(port, t)).collect();
        (t.join().unwrap(), replies)
    }

    #[test]
    fn returns_the_code_and_ignores_other_requests() {
        let (r, replies) = run(vec!["/favicon.ico", "/callback?code=a%2Bb&state=xyz"], "xyz");
        assert_eq!(r, Ok("a+b".to_string()));
        assert!(replies[0].starts_with("HTTP/1.1 404"));
        assert!(replies[1].starts_with("HTTP/1.1 200") && replies[1].contains("signed in"));
    }

    #[test]
    fn wrong_state_and_errors_fail() {
        let (r, replies) = run(vec!["/callback?code=c&state=other"], "xyz");
        assert!(r.unwrap_err().contains("state"));
        assert!(replies[0].starts_with("HTTP/1.1 400"));
        let (r, _) = run(vec!["/callback?error=access_denied&state=xyz"], "xyz");
        assert!(r.unwrap_err().contains("access_denied"));
    }

    #[test]
    fn cancel_and_timeout_end_the_wait() {
        let l = bind_on(0).unwrap();
        let cancel = AtomicBool::new(true);
        assert!(wait_for_code(&l, "s", Duration::from_secs(5), &cancel).unwrap_err().contains("cancel"));
        let cancel = AtomicBool::new(false);
        assert!(wait_for_code(&l, "s", Duration::from_millis(150), &cancel).unwrap_err().contains("timed out"));
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(pct_decode("a%20b+c%zz%4"), "a b c%zz%4");
    }
}
