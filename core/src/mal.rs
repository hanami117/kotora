//! The MyAnimeList client, read only (DESIGN.md §8.1): OAuth 2 with PKCE (`plain`, the only method
//! MAL supports), token refresh, the user's anime list, a rate budget and 429 handling.
//!
//! HTTP goes through the [`Transport`] trait, so tests run against recorded JSON and never call MAL.
//! Nothing here logs a token, a code or the Client ID, and error texts never carry a response body
//! beyond MAL's own short `error` / `message` words.

use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::ListItem;
use crate::Media;
use crate::Status;
use crate::ratelimit::Limiter;
use crate::service::{ListService, ServiceError};
use crate::time;
use crate::tokens::{Token, TokenStore};

/// The registered redirect: the port is fixed (STATUS.md).
pub const PORT: u16 = 47_321;
pub const REDIRECT_URI: &str = "http://127.0.0.1:47321/callback";
const AUTHORIZE_URL: &str = "https://myanimelist.net/v1/oauth2/authorize";
const TOKEN_URL: &str = "https://myanimelist.net/v1/oauth2/token";
const API: &str = "https://api.myanimelist.net/v2";
/// The bearer token is only ever sent to this host.
const API_PREFIX: &str = "https://api.myanimelist.net/";
const SERVICE: &str = "mal";

// ------------------------------------------------------------------------------------ transport

pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub bearer: Option<String>,
    /// A form-encoded body (POST).
    pub form: Option<String>,
}

pub struct Response {
    pub status: u16,
    pub retry_after: Option<u64>,
    pub body: String,
}

/// One HTTP exchange. `Err` means no answer at all (DNS, connect, timeout).
pub trait Transport {
    fn send(&self, req: &Request) -> Result<Response, String>;
}

/// The real transport: blocking `ureq` on the caller's (worker) thread.
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .user_agent(concat!("Kotora/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        UreqTransport { agent }
    }
}

impl Transport for UreqTransport {
    fn send(&self, req: &Request) -> Result<Response, String> {
        let mut resp = if req.method == "POST" {
            let mut r = self.agent.post(&req.url).header("Content-Type", "application/x-www-form-urlencoded");
            if let Some(b) = &req.bearer {
                r = r.header("Authorization", &format!("Bearer {b}"));
            }
            r.send(req.form.clone().unwrap_or_default())
        } else {
            let mut r = self.agent.get(&req.url);
            if let Some(b) = &req.bearer {
                r = r.header("Authorization", &format!("Bearer {b}"));
            }
            r.call()
        }
        .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let retry_after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse().ok());
        let body = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        Ok(Response { status, retry_after, body })
    }
}

// ----------------------------------------------------------------------------------- PKCE etc.

const VERIFIER_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

/// `len` random characters from the PKCE alphabet (RFC 7636), from the OS random source.
fn random_string(len: usize) -> std::io::Result<String> {
    use std::io::Read;
    let mut bytes = vec![0u8; len];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| VERIFIER_CHARS[*b as usize % VERIFIER_CHARS.len()] as char).collect())
}

/// One sign-in attempt: the verifier (also the `plain` challenge) and the `state`.
pub struct Pkce {
    pub verifier: String,
    pub state: String,
}

impl Pkce {
    pub fn new() -> std::io::Result<Pkce> {
        Ok(Pkce { verifier: random_string(64)?, state: random_string(32)? })
    }
}

/// Percent-encodes everything except the unreserved characters.
pub fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", pct_encode(k), pct_encode(v))).collect::<Vec<_>>().join("&")
}

/// The page the browser opens. `code_challenge_method=plain`: the challenge is the verifier.
pub fn authorize_url(client_id: &str, pkce: &Pkce) -> String {
    format!(
        "{AUTHORIZE_URL}?{}",
        form(&[
            ("response_type", "code"),
            ("client_id", client_id),
            ("code_challenge", &pkce.verifier),
            ("code_challenge_method", "plain"),
            ("state", &pkce.state),
            ("redirect_uri", REDIRECT_URI),
        ])
    )
}

// ---------------------------------------------------------------------------------------- client

/// MAL calls with the rate budget and the 429 rule. Holds the Client ID, which `Debug` hides.
pub struct Client<T: Transport> {
    client_id: String,
    transport: T,
    limiter: Limiter,
    sleep: Box<dyn FnMut(Duration) + Send>,
}

#[derive(Debug, PartialEq, Eq)]
enum CallError {
    Offline(String),
    /// 401 on an API call; on the token endpoint the refresh or code was refused.
    Unauthorized,
    RateLimited,
    Other(String),
}

impl From<CallError> for ServiceError {
    fn from(e: CallError) -> Self {
        match e {
            CallError::Offline(m) => ServiceError::Offline(m),
            CallError::Unauthorized => ServiceError::SignInAgain,
            CallError::RateLimited => ServiceError::RateLimited,
            CallError::Other(m) => ServiceError::Other(m),
        }
    }
}

/// A 429 gets at most this many retries.
const MAX_RETRIES: u32 = 3;
const MAX_WAIT: u64 = 60;

impl<T: Transport> Client<T> {
    pub fn new(client_id: String, transport: T) -> Self {
        Client { client_id, transport, limiter: Limiter::mal(), sleep: Box::new(std::thread::sleep) }
    }

    /// Replaces the sleep (tests record the waits instead of waiting).
    pub fn with_sleep(mut self, sleep: impl FnMut(Duration) + Send + 'static) -> Self {
        self.sleep = Box::new(sleep);
        self
    }

    fn call(&mut self, req: &Request) -> Result<Response, CallError> {
        let mut attempt = 0;
        loop {
            let wait = self.limiter.acquire(Instant::now());
            if !wait.is_zero() {
                (self.sleep)(wait);
            }
            let resp = self.transport.send(req).map_err(|_| CallError::Offline("no answer from MyAnimeList".into()))?;
            match resp.status {
                200..=299 => return Ok(resp),
                429 => {
                    attempt += 1;
                    if attempt > MAX_RETRIES {
                        return Err(CallError::RateLimited);
                    }
                    let wait = Duration::from_secs(resp.retry_after.unwrap_or(5u64 << attempt).clamp(1, MAX_WAIT));
                    self.limiter.hold(Instant::now(), wait);
                }
                401 => return Err(CallError::Unauthorized),
                s @ 500..=599 => return Err(CallError::Offline(format!("MyAnimeList is having trouble (HTTP {s})"))),
                403 => return Err(CallError::Other("MyAnimeList refused the request (HTTP 403); it may have blocked us for a while".into())),
                s => return Err(CallError::Other(format!("MyAnimeList answered HTTP {s}{}", api_message(&resp.body)))),
            }
        }
    }

    fn token_request(&mut self, pairs: &[(&str, &str)]) -> Result<Token, CallError> {
        let req = Request { method: "POST", url: TOKEN_URL.into(), bearer: None, form: Some(form(pairs)) };
        let resp = match self.call(&req) {
            // The token endpoint answers 400 / 401 for a bad code or a dead refresh token.
            Err(CallError::Other(m)) if m.contains("HTTP 400") => return Err(CallError::Unauthorized),
            r => r?,
        };
        let t: TokenReply = serde_json::from_str(&resp.body).map_err(|_| CallError::Other("MyAnimeList sent a token answer we can't read".into()))?;
        Ok(Token { access_token: t.access_token, refresh_token: t.refresh_token, expires_at: time::now_unix() + t.expires_in })
    }

    /// Step 3 of the sign-in: the code from the redirect, with the verifier.
    pub fn exchange_code(&mut self, code: &str, verifier: &str) -> Result<Token, ServiceError> {
        let id = self.client_id.clone();
        Ok(self.token_request(&[
            ("client_id", &id),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", REDIRECT_URI),
        ])?)
    }

    pub fn refresh(&mut self, refresh_token: &str) -> Result<Token, ServiceError> {
        let id = self.client_id.clone();
        Ok(self.token_request(&[("client_id", &id), ("grant_type", "refresh_token"), ("refresh_token", refresh_token)])?)
    }

    fn get(&mut self, url: String, access: &str) -> Result<Response, CallError> {
        // Never send the bearer token anywhere but MAL's API (a `paging.next` could say anything).
        if !url.starts_with(API_PREFIX) {
            return Err(CallError::Other("MyAnimeList sent a link to another site; ignored".into()));
        }
        self.call(&Request { method: "GET", url, bearer: Some(access.to_string()), form: None })
    }

    pub fn fetch_user_name(&mut self, access: &str) -> Result<String, ServiceError> {
        let r = self.get(format!("{API}/users/@me"), access)?;
        let me: Me = serde_json::from_str(&r.body).map_err(|_| ServiceError::Other("can't read the user answer".into()))?;
        Ok(me.name)
    }

    /// The whole list, 100 entries a page, following `paging.next`.
    pub fn fetch_list(&mut self, access: &str) -> Result<Vec<ListItem>, ServiceError> {
        let mut url = format!(
            "{API}/users/@me/animelist?fields=list_status,num_episodes,media_type,status,alternative_titles&limit=100&nsfw=true"
        );
        let mut items = Vec::new();
        // A real list is a few pages; the cap only stops a loop of links.
        for _ in 0..200 {
            let r = self.get(url, access)?;
            let (mut page, next) = parse_list_page(&r.body).map_err(ServiceError::Other)?;
            items.append(&mut page);
            match next {
                Some(n) => url = n,
                None => return Ok(items),
            }
        }
        Err(ServiceError::Other("the list has too many pages".into()))
    }
}

impl<T: Transport> std::fmt::Debug for Client<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mal::Client(<client id hidden>)")
    }
}

/// MAL's short error words (`error`, `message`), if the body has them; never the whole body.
fn api_message(body: &str) -> String {
    #[derive(Deserialize)]
    struct E {
        error: Option<String>,
        message: Option<String>,
    }
    match serde_json::from_str::<E>(body) {
        Ok(E { error, message }) => {
            let words: Vec<String> = [error, message].into_iter().flatten().map(|s| s.chars().take(80).collect()).collect();
            if words.is_empty() { String::new() } else { format!(" ({})", words.join(": ")) }
        }
        Err(_) => String::new(),
    }
}

// ----------------------------------------------------------------------------------- JSON shapes

#[derive(Deserialize)]
struct TokenReply {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
}

#[derive(Deserialize)]
struct Me {
    name: String,
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    data: Vec<Raw>,
    paging: Option<Paging>,
}

#[derive(Deserialize)]
struct Paging {
    next: Option<String>,
}

#[derive(Deserialize)]
struct Raw {
    node: Node,
    list_status: Option<ListStatus>,
}

#[derive(Deserialize)]
struct Node {
    id: u32,
    title: String,
    main_picture: Option<Picture>,
    alternative_titles: Option<Alt>,
    num_episodes: Option<u32>,
    media_type: Option<String>,
    status: Option<String>,
}

#[derive(Deserialize)]
struct Picture {
    medium: Option<String>,
    large: Option<String>,
}

#[derive(Deserialize)]
struct Alt {
    #[serde(default)]
    synonyms: Vec<String>,
    en: Option<String>,
    ja: Option<String>,
}

#[derive(Deserialize)]
struct ListStatus {
    status: Option<String>,
    #[serde(default)]
    score: u8,
    #[serde(default)]
    num_episodes_watched: u32,
    #[serde(default)]
    is_rewatching: bool,
    updated_at: Option<String>,
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// One page of `animelist`: the entries and the `paging.next` link. Entries without a list status
/// are skipped (they can't be on the list).
pub fn parse_list_page(body: &str) -> Result<(Vec<ListItem>, Option<String>), String> {
    let page: Page = serde_json::from_str(body).map_err(|e| format!("can't read the list answer ({})", e.classify_name()))?;
    let mut out = Vec::new();
    for raw in page.data {
        let Some(ls) = raw.list_status else { continue };
        let Some(status) = ls.status.as_deref().and_then(Status::from_mal) else { continue };
        let n = raw.node;
        let alt = n.alternative_titles;
        out.push(ListItem {
            media: Media {
                id: n.id,
                title: n.title,
                title_en: non_empty(alt.as_ref().and_then(|a| a.en.clone())),
                title_ja: non_empty(alt.as_ref().and_then(|a| a.ja.clone())),
                synonyms: alt.map(|a| a.synonyms).unwrap_or_default().into_iter().filter(|s| !s.trim().is_empty()).collect(),
                // MAL says 0 when the count is unknown.
                episodes: n.num_episodes.filter(|&e| e > 0),
                format: n.media_type.unwrap_or_default(),
                air_status: n.status.unwrap_or_default(),
                // The medium size is plenty for a 34 x 48 row.
                cover_url: n.main_picture.and_then(|p| p.medium.or(p.large)),
            },
            status,
            progress: ls.num_episodes_watched,
            score: (ls.score > 0).then_some(ls.score.min(10)),
            rewatching: ls.is_rewatching,
            updated_at: ls.updated_at.as_deref().and_then(time::parse_rfc3339).unwrap_or(0),
        });
    }
    Ok((out, page.paging.and_then(|p| p.next).filter(|n| !n.is_empty())))
}

trait ClassifyName {
    fn classify_name(&self) -> &'static str;
}

impl ClassifyName for serde_json::Error {
    fn classify_name(&self) -> &'static str {
        match self.classify() {
            serde_json::error::Category::Syntax => "not JSON",
            serde_json::error::Category::Eof => "cut short",
            serde_json::error::Category::Data => "unexpected shape",
            serde_json::error::Category::Io => "read error",
        }
    }
}

// ---------------------------------------------------------------------------------------- service

/// The signed-in MAL account as a [`ListService`]: it keeps the token fresh (refresh before use,
/// and once on a 401) and saves the new token to the token file.
pub struct Mal<T: Transport> {
    client: Client<T>,
    tokens: TokenStore,
}

impl<T: Transport> Mal<T> {
    pub fn new(client: Client<T>, tokens: TokenStore) -> Self {
        Mal { client, tokens }
    }

    fn save(&self, t: &Token) -> Result<(), ServiceError> {
        self.tokens.save_mal(t).map_err(|e| ServiceError::Other(format!("can't save the token file: {e}")))
    }

    /// A token that is good for the next call.
    fn access(&mut self) -> Result<String, ServiceError> {
        let t = self.tokens.load_mal().ok_or(ServiceError::SignInAgain)?;
        if !t.is_stale(time::now_unix()) {
            return Ok(t.access_token);
        }
        self.refresh_now(&t)
    }

    fn refresh_now(&mut self, old: &Token) -> Result<String, ServiceError> {
        let fresh = self.client.refresh(&old.refresh_token)?;
        self.save(&fresh)?;
        Ok(fresh.access_token)
    }

    /// Runs `f` with a token; on a 401 refreshes once and tries again.
    fn authed<R>(&mut self, f: impl Fn(&mut Client<T>, &str) -> Result<R, ServiceError>) -> Result<R, ServiceError> {
        let access = self.access()?;
        match f(&mut self.client, &access) {
            Err(ServiceError::SignInAgain) => {
                let old = self.tokens.load_mal().ok_or(ServiceError::SignInAgain)?;
                let access = self.refresh_now(&old)?;
                f(&mut self.client, &access)
            }
            r => r,
        }
    }

    /// Completes a sign-in: exchanges the code and saves the token file.
    pub fn complete_sign_in(&mut self, code: &str, verifier: &str) -> Result<(), ServiceError> {
        let t = self.client.exchange_code(code, verifier)?;
        self.save(&t)
    }
}

impl<T: Transport> ListService for Mal<T> {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn title(&self) -> &'static str {
        "MyAnimeList"
    }

    fn user_name(&mut self) -> Result<String, ServiceError> {
        self.authed(|c, a| c.fetch_user_name(a))
    }

    fn fetch_list(&mut self) -> Result<Vec<ListItem>, ServiceError> {
        self.authed(|c, a| c.fetch_list(a))
    }
}

// ------------------------------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// Answers from a script and records every request it saw.
    /// (method, url, bearer, form body)
    type Sent = (String, String, Option<String>, Option<String>);

    struct Fake {
        answers: RefCell<VecDeque<Result<Response, String>>>,
        seen: Seen,
    }

    impl Transport for Fake {
        fn send(&self, req: &Request) -> Result<Response, String> {
            self.seen.lock().unwrap().push((req.method.into(), req.url.clone(), req.bearer.clone(), req.form.clone()));
            self.answers.borrow_mut().pop_front().expect("unexpected request")
        }
    }

    type Seen = Arc<Mutex<Vec<Sent>>>;

    fn fake(answers: Vec<Result<Response, String>>) -> (Fake, Seen) {
        let seen: Seen = Arc::default();
        (Fake { answers: RefCell::new(answers.into()), seen: seen.clone() }, seen)
    }

    fn ok(body: &str) -> Result<Response, String> {
        Ok(Response { status: 200, retry_after: None, body: body.into() })
    }

    fn status(code: u16, retry_after: Option<u64>) -> Result<Response, String> {
        Ok(Response { status: code, retry_after, body: "{}".into() })
    }

    const FIXTURE: &str = include_str!("../tests/mal_animelist_page1.json");
    const FIXTURE2: &str = include_str!("../tests/mal_animelist_page2.json");

    fn tok_json(access: &str) -> String {
        format!(r#"{{"token_type":"Bearer","expires_in":3600,"access_token":"{access}","refresh_token":"fake-refresh-{access}"}}"#)
    }

    fn client(f: Fake) -> (Client<Fake>, Arc<Mutex<Vec<Duration>>>) {
        let waits: Arc<Mutex<Vec<Duration>>> = Arc::default();
        let w = waits.clone();
        (Client::new("fake-client-id".into(), f).with_sleep(move |d| w.lock().unwrap().push(d)), waits)
    }

    fn tmp_tokens(name: &str, t: Option<Token>) -> TokenStore {
        let dir = std::env::temp_dir().join(format!("kotora-mal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = TokenStore::new(dir.join("tokens.json"));
        if let Some(t) = t {
            s.save_mal(&t).unwrap();
        }
        s
    }

    fn token(access: &str, expires_in: i64) -> Token {
        Token { access_token: access.into(), refresh_token: format!("fake-refresh-{access}"), expires_at: time::now_unix() + expires_in }
    }

    #[test]
    fn authorize_url_uses_plain_pkce_and_the_fixed_redirect() {
        let p = Pkce { verifier: "v".repeat(64), state: "st~1".into() };
        let u = authorize_url("fake-client-id", &p);
        assert!(u.starts_with("https://myanimelist.net/v1/oauth2/authorize?response_type=code&client_id=fake-client-id&"));
        assert!(u.contains(&format!("code_challenge={}", p.verifier)));
        assert!(u.contains("code_challenge_method=plain"));
        assert!(u.contains("state=st~1"));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A47321%2Fcallback"));
    }

    #[test]
    fn pkce_values_are_long_enough_and_different() {
        let a = Pkce::new().unwrap();
        let b = Pkce::new().unwrap();
        assert!((43..=128).contains(&a.verifier.len()));
        assert!(a.verifier.bytes().all(|c| VERIFIER_CHARS.contains(&c)));
        assert_ne!(a.verifier, b.verifier);
        assert_ne!(a.state, b.state);
    }

    #[test]
    fn exchange_sends_the_verifier_and_no_secret() {
        let (f, seen) = fake(vec![ok(&tok_json("acc1"))]);
        let (mut c, _) = client(f);
        let t = c.exchange_code("the code", "verif").unwrap();
        assert_eq!(t.access_token, "acc1");
        assert!(t.expires_at > time::now_unix() + 3000);
        let seen = seen.lock().unwrap();
        assert_eq!((seen[0].0.as_str(), seen[0].1.as_str()), ("POST", TOKEN_URL));
        let body = seen[0].3.as_deref().unwrap();
        for part in ["grant_type=authorization_code", "code=the%20code", "code_verifier=verif", "client_id=fake-client-id", "redirect_uri=http%3A%2F%2F127.0.0.1%3A47321%2Fcallback"] {
            assert!(body.contains(part), "{part} in {body}");
        }
        assert!(!body.contains("secret"));
        assert!(seen[0].2.is_none());
    }

    #[test]
    fn a_refused_refresh_means_sign_in_again() {
        let (f, _) = fake(vec![status(400, None)]);
        let (mut c, _) = client(f);
        assert_eq!(c.refresh("r"), Err(ServiceError::SignInAgain));
    }

    #[test]
    fn parses_pages_and_follows_paging() {
        let (items, next) = parse_list_page(FIXTURE).unwrap();
        assert_eq!(items.len(), 3, "the entry without a list status is skipped");
        assert_eq!(next.as_deref(), Some("https://api.myanimelist.net/v2/users/@me/animelist?offset=100&limit=100"));
        let a = &items[0];
        assert_eq!((a.media.id, a.media.title.as_str(), a.status, a.progress, a.score), (1001, "Lanterns of Kirigaoka", Status::Watching, 5, Some(8)));
        assert_eq!(a.media.title_en.as_deref(), Some("Lanterns of Kirigaoka (English)"));
        assert_eq!(a.media.title_ja, None, "a blank title counts as none");
        assert_eq!(a.media.synonyms, vec!["Kirigaoka no Tomoshibi"]);
        assert!(a.media.is_airing());
        assert_eq!(items[1].media.air_status, "not_yet_aired");
        assert_eq!(a.media.episodes, Some(12));
        assert_eq!(a.media.format, "tv");
        assert_eq!(a.media.cover_url.as_deref(), Some("https://cdn.example.invalid/1001m.jpg"));
        assert_eq!(a.updated_at, 1_519_450_460);
        assert!(!a.rewatching);
        let b = &items[1];
        assert_eq!((b.status, b.score, b.media.episodes, b.rewatching), (Status::PlanToWatch, None, None, true));
        assert_eq!(items[2].status, Status::OnHold);
        assert!(parse_list_page("not json").is_err());
        assert_eq!(parse_list_page("{\"data\":[]}").unwrap().0.len(), 0);
    }

    #[test]
    fn fetch_list_pages_and_sends_the_bearer_only_to_mal() {
        let (f, seen) = fake(vec![ok(FIXTURE), ok(FIXTURE2)]);
        let (mut c, waits) = client(f);
        let items = c.fetch_list("fake-access").unwrap();
        assert_eq!(items.len(), 4);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[0].1.starts_with("https://api.myanimelist.net/v2/users/@me/animelist?fields=list_status,"));
        assert!(seen[0].1.contains("limit=100"));
        assert!(seen[1].1.contains("offset=100"));
        assert!(seen.iter().all(|s| s.2.as_deref() == Some("fake-access")));
        // The second request waited for the one-second gap.
        assert!(waits.lock().unwrap().iter().any(|d| *d >= Duration::from_millis(900)));
    }

    #[test]
    fn a_foreign_next_link_is_refused() {
        let page = r#"{"data":[],"paging":{"next":"https://evil.example.invalid/steal"}}"#;
        let (f, seen) = fake(vec![ok(page)]);
        let (mut c, _) = client(f);
        assert!(matches!(c.fetch_list("fake-access"), Err(ServiceError::Other(_))));
        assert_eq!(seen.lock().unwrap().len(), 1, "no second request was made");
    }

    #[test]
    fn a_429_waits_for_retry_after_then_succeeds() {
        let (f, seen) = fake(vec![status(429, Some(7)), ok(r#"{"data":[]}"#)]);
        let (mut c, waits) = client(f);
        assert_eq!(c.fetch_list("a").unwrap().len(), 0);
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert!(waits.lock().unwrap().iter().any(|d| *d >= Duration::from_secs(6) && *d <= Duration::from_secs(8)), "{:?}", waits.lock().unwrap());
    }

    #[test]
    fn endless_429s_give_up() {
        let (f, seen) = fake((0..10).map(|_| status(429, Some(1))).collect());
        let (mut c, _) = client(f);
        assert_eq!(c.fetch_list("a"), Err(ServiceError::RateLimited));
        assert_eq!(seen.lock().unwrap().len(), 1 + MAX_RETRIES as usize);
    }

    #[test]
    fn errors_are_sorted_into_offline_and_other() {
        let (f, _) = fake(vec![Err("dns".into())]);
        assert!(matches!(client(f).0.fetch_list("a"), Err(ServiceError::Offline(_))));
        let (f, _) = fake(vec![status(503, None)]);
        assert!(matches!(client(f).0.fetch_list("a"), Err(ServiceError::Offline(_))));
        let (f, _) = fake(vec![status(403, None)]);
        assert!(matches!(client(f).0.fetch_list("a"), Err(ServiceError::Other(m)) if m.contains("403")));
        let (f, _) = fake(vec![Ok(Response { status: 400, retry_after: None, body: r#"{"error":"invalid_request","message":"bad fields"}"#.into() })]);
        assert!(matches!(client(f).0.fetch_list("a"), Err(ServiceError::Other(m)) if m.contains("invalid_request: bad fields")));
    }

    #[test]
    fn debug_hides_the_client_id() {
        let (f, _) = fake(vec![]);
        assert!(!format!("{:?}", client(f).0).contains("fake-client-id"));
    }

    // -- the service: token handling

    #[test]
    fn a_fresh_token_is_used_as_it_is() {
        let (f, seen) = fake(vec![ok(r#"{"data":[]}"#)]);
        let (c, _) = client(f);
        let mut mal = Mal::new(c, tmp_tokens("fresh", Some(token("good", 3000))));
        assert_eq!(mal.fetch_list().unwrap().len(), 0);
        assert_eq!(seen.lock().unwrap()[0].2.as_deref(), Some("good"));
        assert_eq!(mal.name(), "mal");
    }

    #[test]
    fn a_stale_token_is_refreshed_and_saved_first() {
        let (f, seen) = fake(vec![ok(&tok_json("new")), ok(r#"{"data":[]}"#)]);
        let (c, _) = client(f);
        let tokens = tmp_tokens("stale", Some(token("old", 10)));
        let mut mal = Mal::new(c, tokens);
        mal.fetch_list().unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].1, TOKEN_URL);
        assert!(seen[0].3.as_deref().unwrap().contains("grant_type=refresh_token"));
        assert!(seen[0].3.as_deref().unwrap().contains("refresh_token=fake-refresh-old"));
        assert_eq!(seen[1].2.as_deref(), Some("new"));
        assert_eq!(mal.tokens.load_mal().unwrap().access_token, "new");
    }

    #[test]
    fn a_401_refreshes_once_and_retries() {
        let (f, seen) = fake(vec![status(401, None), ok(&tok_json("new")), ok(r#"{"data":[]}"#)]);
        let (c, _) = client(f);
        let mut mal = Mal::new(c, tmp_tokens("401", Some(token("old", 3000))));
        assert!(mal.fetch_list().is_ok());
        assert_eq!(seen.lock().unwrap().len(), 3);
    }

    #[test]
    fn a_dead_refresh_token_means_sign_in_again_and_a_missing_file_too() {
        let (f, _) = fake(vec![status(401, None), status(400, None)]);
        let (c, _) = client(f);
        let mut mal = Mal::new(c, tmp_tokens("dead", Some(token("old", 3000))));
        assert_eq!(mal.fetch_list(), Err(ServiceError::SignInAgain));
        let (f, seen) = fake(vec![]);
        let (c, _) = client(f);
        let mut mal = Mal::new(c, tmp_tokens("none", None));
        assert_eq!(mal.user_name(), Err(ServiceError::SignInAgain));
        assert!(seen.lock().unwrap().is_empty(), "no request without a token");
    }

    #[test]
    fn sign_in_saves_the_token_and_the_user_name_reads() {
        let (f, _) = fake(vec![ok(&tok_json("first")), ok(r#"{"id":5,"name":"demo_user"}"#)]);
        let (c, _) = client(f);
        let mut mal = Mal::new(c, tmp_tokens("signin", None));
        mal.complete_sign_in("code", "verifier").unwrap();
        assert_eq!(mal.tokens.load_mal().unwrap().access_token, "first");
        assert_eq!(mal.user_name().unwrap(), "demo_user");
    }
}
