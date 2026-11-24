//! The loopback listener of the OAuth redirect: `http://127.0.0.1:47321/callback?code=…&state=…`
//! (MyAnimeList), or `/callback#access_token=…` (AniList's implicit grant: the token is in the
//! fragment, which the browser never sends, so the page there posts it back). It only runs while
//! the user is signing in (a few minutes at most), then closes.

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

/// AniList's implicit grant: waits for the token the callback page posts back and returns what
/// it posted (`access_token=…&token_type=Bearer&expires_in=…`; `anilist::parse_token_input`
/// reads it). Fails on an `error` from AniList, the timeout, or `cancel`.
pub fn wait_for_token(listener: &TcpListener, timeout: Duration, cancel: &AtomicBool) -> Result<String, String> {
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
                if let Some(r) = handle_token(stream) {
                    return r;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// The page AniList redirects to: it takes the token out of the address (and out of the
/// browser's history) and posts it to Kotora.
const TOKEN_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Kotora</title><body style=\"font:16px system-ui;margin:3em\">\
<p id=m>Passing the sign-in on to Kotora\u{2026}</p><noscript><p>Turn on JavaScript for this page, or copy the token from AniList by hand.</p></noscript>\
<script>var h=location.hash.slice(1);history.replaceState(null,'','/callback');\
fetch('/token',{method:'POST',body:h}).then(function(r){return r.text()}).then(function(t){document.getElementById('m').textContent=t})\
.catch(function(){document.getElementById('m').textContent='Kotora stopped waiting. Try the sign-in again.'});</script>";

/// `Some(result)` once the token (or an error) came, `None` for anything else.
fn handle_token(mut stream: TcpStream) -> Option<Result<String, String>> {
    let (head, body) = read_request(&mut stream);
    let line = head.lines().next().unwrap_or("");
    let mut parts = line.split(' ');
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    match (method, target.split('?').next().unwrap_or("")) {
        ("GET", "/callback") => {
            if let Some(e) = target.split_once('?').and_then(|(_, q)| q.split('&').find_map(|p| p.strip_prefix("error="))) {
                let e = pct_decode(e);
                respond(&mut stream, 400, &format!("Kotora could not sign in: AniList refused the sign-in ({e})"));
                return Some(Err(format!("AniList refused the sign-in ({e})")));
            }
            respond_html(&mut stream, 200, TOKEN_PAGE);
            None
        }
        ("POST", "/token") => {
            let posted = body.trim().to_string();
            if posted.contains("access_token=") {
                respond_text(&mut stream, 200, "Kotora got the sign-in from AniList. You can close this tab.");
                Some(Ok(posted))
            } else if let Some(e) = posted.split('&').find_map(|p| p.strip_prefix("error=")) {
                let e = pct_decode(e);
                respond_text(&mut stream, 400, &format!("Kotora could not sign in: AniList refused the sign-in ({e})"));
                Some(Err(format!("AniList refused the sign-in ({e})")))
            } else {
                respond_text(&mut stream, 400, "There was no sign-in in this address. Copy the token from AniList and paste it into Kotora.");
                Some(Err("no token in the redirect".into()))
            }
        }
        _ => {
            respond(&mut stream, 404, "Not found");
            None
        }
    }
}

/// The request head, and the body when there is a `Content-Length` (8 KB at most each).
fn read_request(stream: &mut TcpStream) -> (String, String) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let end = |b: &[u8]| b.windows(4).position(|w| w == b"\r\n\r\n");
    while buf.len() < 8192 && end(&buf).is_none() {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let Some(at) = end(&buf) else { return (String::from_utf8_lossy(&buf).into_owned(), String::new()) };
    let head = String::from_utf8_lossy(&buf[..at]).into_owned();
    let len = head
        .lines()
        .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.trim().parse::<usize>().ok()))
        .unwrap_or(0)
        .min(8192);
    let mut body = buf[at + 4..].to_vec();
    while body.len() < len {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    body.truncate(len);
    (head, String::from_utf8_lossy(&body).into_owned())
}

/// `Some(result)` for the callback request, `None` for anything else.
fn handle(mut stream: TcpStream, state: &str) -> Option<Result<String, String>> {
    let (head, _) = read_request(&mut stream);
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
    let message = message.replace('&', "&amp;").replace('<', "&lt;");
    respond_html(stream, status, &format!("<!doctype html><meta charset=utf-8><title>Kotora</title><body style=\"font:16px system-ui;margin:3em\"><p>{message}</p>"));
}

fn respond_html(stream: &mut TcpStream, status: u16, body: &str) {
    send(stream, status, "text/html", body);
}

fn respond_text(stream: &mut TcpStream, status: u16, body: &str) {
    send(stream, status, "text/plain", body);
}

fn send(stream: &mut TcpStream, status: u16, kind: &str, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

pub use crate::http::pct_decode;

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

    fn post(port: u16, target: &str, body: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "POST {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    fn run_token(steps: Vec<(&'static str, &'static str, &'static str)>) -> (Result<String, String>, Vec<String>) {
        let l = bind_on(0).unwrap();
        let port = l.local_addr().unwrap().port();
        let cancel = Arc::new(AtomicBool::new(false));
        let t = std::thread::spawn(move || wait_for_token(&l, Duration::from_secs(10), &cancel));
        let replies = steps.into_iter().map(|(m, target, body)| if m == "GET" { get(port, target) } else { post(port, target, body) }).collect();
        (t.join().unwrap(), replies)
    }

    #[test]
    fn the_anilist_page_posts_the_token_back() {
        let (r, replies) = run_token(vec![("GET", "/favicon.ico", ""), ("GET", "/callback", ""), ("POST", "/token", "access_token=a.b.c&token_type=Bearer&expires_in=31536000")]);
        assert_eq!(r, Ok("access_token=a.b.c&token_type=Bearer&expires_in=31536000".to_string()));
        assert!(replies[0].starts_with("HTTP/1.1 404"));
        assert!(replies[1].starts_with("HTTP/1.1 200") && replies[1].contains("location.hash") && replies[1].contains("replaceState"), "the page strips the token from the history");
        assert!(replies[2].starts_with("HTTP/1.1 200") && replies[2].contains("close this tab"));
    }

    #[test]
    fn an_anilist_refusal_or_no_token_fails() {
        let (r, replies) = run_token(vec![("GET", "/callback?error=access_denied&error_description=x", "")]);
        assert!(r.unwrap_err().contains("access_denied"));
        assert!(replies[0].starts_with("HTTP/1.1 400"));
        let (r, _) = run_token(vec![("POST", "/token", "")]);
        assert!(r.unwrap_err().contains("no token"));
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(pct_decode("a%20b+c%zz%4"), "a b c%zz%4");
    }
}
