//! The MyAnimeList client: OAuth 2 with PKCE (`plain`, the only method MAL
//! supports), token refresh, the user's anime list, one entry (read and `PATCH my_list_status`),
//! a rate budget and 429 handling.
//!
//! HTTP goes through the [`Transport`] trait, so tests run against recorded JSON and never call MAL.
//! Nothing here logs a token, a code or the Client ID, and error texts never carry a response body
//! beyond MAL's own short `error` / `message` words.

use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::airing::{self, Broadcast, SeasonAnime};
use crate::{Change, ListItem, Media, Remote, Status, Values};
use crate::ratelimit::Limiter;
use crate::service::{AnimeHit, AnimeStats, Details, ListService, MyEntry, Profile, SearchHit, ServiceError, User};
use crate::time;
use crate::tokens::{Token, TokenStore};

/// The registered redirect: the port is fixed.
pub const PORT: u16 = 47_321;
pub const REDIRECT_URI: &str = "http://127.0.0.1:47321/callback";
const AUTHORIZE_URL: &str = "https://myanimelist.net/v1/oauth2/authorize";
const TOKEN_URL: &str = "https://myanimelist.net/v1/oauth2/token";
const API: &str = "https://api.myanimelist.net/v2";
/// The bearer token is only ever sent to this host.
const API_PREFIX: &str = "https://api.myanimelist.net/";
const SERVICE: &str = "mal";

mod manga;
pub use manga::{manga_type_label, parse_manga_details, parse_manga_hits, parse_manga_list_page};

// ------------------------------------------------------------------------------------ transport

pub use crate::http::{Request, Response, Transport, UreqTransport};

// ----------------------------------------------------------------------------------- PKCE etc.

const VERIFIER_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

/// `len` random characters from the PKCE alphabet (RFC 7636), from the OS random source.
fn random_string(len: usize) -> std::io::Result<String> {
    let mut bytes = vec![0u8; len];
    fill_random(&mut bytes)?;
    Ok(bytes.iter().map(|b| VERIFIER_CHARS[*b as usize % VERIFIER_CHARS.len()] as char).collect())
}

/// Random bytes from the system: `/dev/urandom`.
#[cfg(not(windows))]
fn fill_random(bytes: &mut [u8]) -> std::io::Result<()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")?.read_exact(bytes)
}

/// Random bytes from the system: Windows' preferred generator.
#[cfg(windows)]
fn fill_random(bytes: &mut [u8]) -> std::io::Result<()> {
    use windows_sys::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};
    // SAFETY: the buffer is writable for its length; no algorithm handle with this flag.
    let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), bytes.as_mut_ptr(), bytes.len() as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status == 0 { Ok(()) } else { Err(std::io::Error::other(format!("the system's random generator failed ({status:#x})"))) }
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
    authorize_url_to(client_id, pkce, REDIRECT_URI)
}

/// [`authorize_url`] with another registered redirect (Android: the app's own URI scheme).
pub fn authorize_url_to(client_id: &str, pkce: &Pkce, redirect_uri: &str) -> String {
    format!(
        "{AUTHORIZE_URL}?{}",
        form(&[
            ("response_type", "code"),
            ("client_id", client_id),
            ("code_challenge", &pkce.verifier),
            ("code_challenge_method", "plain"),
            ("state", &pkce.state),
            ("redirect_uri", redirect_uri),
        ])
    )
}

// ---------------------------------------------------------------------------------------- client

/// One request budget for the whole app: the list sync and the queue worker run on different
/// threads with their own clients, and together stay within MAL's 20 a minute.
static BUDGET: LazyLock<Arc<Mutex<Limiter>>> = LazyLock::new(|| Arc::new(Mutex::new(Limiter::mal())));

/// MAL calls with the rate budget and the 429 rule. Holds the Client ID, which `Debug` hides.
pub struct Client<T: Transport> {
    client_id: String,
    transport: T,
    limiter: Arc<Mutex<Limiter>>,
    sleep: Box<dyn FnMut(Duration) + Send>,
}

#[derive(Debug, PartialEq, Eq)]
enum CallError {
    Offline(String),
    /// 401 on an API call; on the token endpoint the refresh or code was refused.
    Unauthorized,
    RateLimited,
    /// 404: what was asked for isn't there.
    NotFound,
    Other(String),
}

impl From<CallError> for ServiceError {
    fn from(e: CallError) -> Self {
        match e {
            CallError::Offline(m) => ServiceError::Offline(m),
            CallError::Unauthorized => ServiceError::SignInAgain,
            CallError::RateLimited => ServiceError::RateLimited,
            CallError::NotFound => ServiceError::Other("MyAnimeList answered HTTP 404".into()),
            CallError::Other(m) => ServiceError::Other(m),
        }
    }
}

/// A 429 gets at most this many retries.
const MAX_RETRIES: u32 = 3;
const MAX_WAIT: u64 = 60;

impl<T: Transport> Client<T> {
    pub fn new(client_id: String, transport: T) -> Self {
        Client { client_id, transport, limiter: BUDGET.clone(), sleep: Box::new(std::thread::sleep) }
    }

    /// A budget of its own instead of the app's (tests, which run side by side).
    pub fn with_own_budget(mut self) -> Self {
        self.limiter = Arc::new(Mutex::new(Limiter::mal()));
        self
    }

    /// Replaces the sleep (tests record the waits instead of waiting).
    pub fn with_sleep(mut self, sleep: impl FnMut(Duration) + Send + 'static) -> Self {
        self.sleep = Box::new(sleep);
        self
    }

    fn call(&mut self, req: &Request) -> Result<Response, CallError> {
        let mut attempt = 0;
        loop {
            let wait = self.limiter.lock().unwrap_or_else(|e| e.into_inner()).acquire(Instant::now());
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
                    self.limiter.lock().unwrap_or_else(|e| e.into_inner()).hold(Instant::now(), wait);
                }
                401 => return Err(CallError::Unauthorized),
                s @ 500..=599 => return Err(CallError::Offline(format!("MyAnimeList is having trouble (HTTP {s})"))),
                403 => return Err(CallError::Other("MyAnimeList refused the request (HTTP 403); it may have blocked us for a while".into())),
                404 => return Err(CallError::NotFound),
                s => return Err(CallError::Other(format!("MyAnimeList answered HTTP {s}{}", api_message(&resp.body)))),
            }
        }
    }

    fn token_request(&mut self, pairs: &[(&str, &str)]) -> Result<Token, CallError> {
        let req = Request { method: "POST", url: TOKEN_URL.into(), bearer: None, form: Some(form(pairs)), json: None };
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
        self.exchange_code_to(code, verifier, REDIRECT_URI)
    }

    /// [`Client::exchange_code`] for a sign-in that used another redirect ([`authorize_url_to`]).
    pub fn exchange_code_to(&mut self, code: &str, verifier: &str, redirect_uri: &str) -> Result<Token, ServiceError> {
        let id = self.client_id.clone();
        Ok(self.token_request(&[
            ("client_id", &id),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
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
        self.call(&Request { method: "GET", url, bearer: Some(access.to_string()), form: None, json: None })
    }

    /// The user's list entry for one anime (`GET /anime/{id}?fields=my_list_status{num_times_rewatched}`,
    /// the braces percent-encoded: MAL leaves the rewatch count out unless asked); `None` when
    /// the anime is not on the list.
    pub fn fetch_entry(&mut self, access: &str, id: u32) -> Result<Option<Remote>, ServiceError> {
        let r = self.get(format!("{API}/anime/{id}?fields=my_list_status%7Bnum_times_rewatched%7D"), access)?;
        #[derive(Deserialize)]
        struct One {
            my_list_status: Option<ListStatus>,
        }
        let one: One = serde_json::from_str(&r.body).map_err(|e| ServiceError::Other(format!("can't read the entry answer ({})", e.classify_name())))?;
        Ok(one.my_list_status.and_then(ListStatus::remote))
    }

    /// Sends one change (`PATCH /anime/{id}/my_list_status`, form-encoded) and returns the entry
    /// MAL saved. Only the changed fields go; a score of 0 removes the score.
    pub fn patch_entry(&mut self, access: &str, id: u32, change: &Change) -> Result<Remote, ServiceError> {
        let mut pairs: Vec<(&str, String)> = Vec::new();
        if let Some(st) = change.status {
            pairs.push(("status", st.mal_name().into()));
        }
        if let Some(p) = change.progress {
            // MAL's request name differs from its answer's (`num_episodes_watched`).
            pairs.push(("num_watched_episodes", p.to_string()));
        }
        if let Some(sc) = change.score {
            pairs.push(("score", sc.map_or(0, crate::Score::to_ten).to_string()));
        }
        if let Some(r) = change.rewatching {
            pairs.push(("is_rewatching", r.to_string()));
        }
        if let Some(n) = change.times_rewatched {
            pairs.push(("num_times_rewatched", n.to_string()));
        }
        if pairs.is_empty() {
            return Err(ServiceError::Other("nothing to send".into()));
        }
        let body = form(&pairs.iter().map(|(k, v)| (*k, v.as_str())).collect::<Vec<_>>());
        let req = Request { method: "PATCH", url: format!("{API}/anime/{id}/my_list_status"), bearer: Some(access.to_string()), form: Some(body), json: None };
        let r = self.call(&req)?;
        let ls: ListStatus = serde_json::from_str(&r.body).map_err(|e| ServiceError::Other(format!("can't read MyAnimeList's answer ({})", e.classify_name())))?;
        ls.remote().ok_or_else(|| ServiceError::Other("MyAnimeList's answer has no list status".into()))
    }

    /// Removes one anime from the list (`DELETE /anime/{id}/my_list_status`). MAL answers 404
    /// when it isn't on the list: that is done too.
    pub fn delete_entry(&mut self, access: &str, id: u32) -> Result<(), ServiceError> {
        let req = Request { method: "DELETE", url: format!("{API}/anime/{id}/my_list_status"), bearer: Some(access.to_string()), form: None, json: None };
        match self.call(&req) {
            Ok(_) | Err(CallError::NotFound) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// A title search, `GET /anime?q=` (5 results). MAL wants at least 3 characters; the query is
    /// cut to 64.
    pub fn search_anime(&mut self, access: &str, query: &str) -> Result<Vec<SearchHit>, ServiceError> {
        let q: String = query.trim().chars().take(64).collect();
        if q.chars().count() < 3 {
            return Ok(Vec::new());
        }
        let r = self.get(format!("{API}/anime?q={}&limit=5&fields=alternative_titles", pct_encode(&q)), access)?;
        parse_search(&r.body).map_err(ServiceError::Other)
    }

    /// The search bar's search, `GET /anime?q=` with what a result row shows (`limit` up to 20).
    /// Under 3 characters nothing is asked.
    pub fn browse_anime(&mut self, access: &str, query: &str, limit: u32) -> Result<Vec<AnimeHit>, ServiceError> {
        let q: String = query.trim().chars().take(64).collect();
        if q.chars().count() < 3 {
            return Ok(Vec::new());
        }
        let r = self.get(format!("{API}/anime?q={}&limit={}&fields={BROWSE_FIELDS}", pct_encode(&q), limit.clamp(1, 20)), access)?;
        parse_hits(&r.body).map_err(ServiceError::Other)
    }

    /// One anime's information page (`GET /anime/{id}?fields=…`), with the user's entry.
    pub fn fetch_details(&mut self, access: &str, id: u32) -> Result<Details, ServiceError> {
        let r = self.get(format!("{API}/anime/{id}?fields={DETAIL_FIELDS}"), access)?;
        parse_details(&r.body).map_err(ServiceError::Other)
    }

    /// The user's name, profile picture, join date and anime statistics, in one request
    /// (`GET /users/@me?fields=picture,anime_statistics`; MAL leaves the picture out when none
    /// is set).
    pub fn fetch_user(&mut self, access: &str) -> Result<User, ServiceError> {
        let r = self.get(format!("{API}/users/@me?fields=picture,anime_statistics"), access)?;
        parse_user(&r.body).ok_or_else(|| ServiceError::Other("can't read the user answer".into()))
    }

    /// One page of a season's anime, most members first (`GET /anime/season/{year}/{season}`,
    /// [`SEASON_PAGE`] a page), and whether another page follows. NSFW titles are left out, as
    /// in search.
    pub fn fetch_season(&mut self, access: &str, season: airing::Season, page: u32) -> Result<(Vec<SeasonAnime>, bool), ServiceError> {
        let r = self.get(
            format!(
                "{API}/anime/season/{}/{}?sort=anime_num_list_users&limit={SEASON_PAGE}&offset={}&fields={SEASON_FIELDS}",
                season.year,
                season.name(),
                page * SEASON_PAGE
            ),
            access,
        )?;
        parse_season(&r.body).map_err(ServiceError::Other)
    }

    /// The whole list, 100 entries a page, following `paging.next`.
    pub fn fetch_list(&mut self, access: &str) -> Result<Vec<ListItem>, ServiceError> {
        let mut url = format!(
            "{API}/users/@me/animelist?fields=list_status%7Bnum_times_rewatched%7D,num_episodes,media_type,status,alternative_titles,start_date,broadcast&limit=100&nsfw=true"
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

/// What a search result row shows (the cover comes by default).
/// The Airing page: what a card and the timetable need.
const SEASON_FIELDS: &str = "alternative_titles,media_type,num_episodes,status,start_date,end_date,broadcast,mean,num_list_users,my_list_status";
/// Anime a season page asks for.
pub const SEASON_PAGE: u32 = 100;

const BROWSE_FIELDS: &str = "alternative_titles,media_type,num_episodes,status,start_season,mean";
/// The details page.
const DETAIL_FIELDS: &str = "alternative_titles,media_type,num_episodes,status,start_season,mean,rank,popularity,num_list_users,\
start_date,end_date,synopsis,source,average_episode_duration,studios,genres,my_list_status%7Bnum_times_rewatched%7D";

#[derive(Deserialize, Default)]
#[serde(default)]
struct FullNode {
    id: u32,
    title: String,
    main_picture: Option<Picture>,
    alternative_titles: Option<Alt>,
    num_episodes: Option<u32>,
    media_type: Option<String>,
    status: Option<String>,
    start_season: Option<Season>,
    mean: Option<f32>,
    rank: Option<u32>,
    popularity: Option<u32>,
    num_list_users: Option<u32>,
    start_date: Option<String>,
    end_date: Option<String>,
    synopsis: Option<String>,
    source: Option<String>,
    /// Seconds.
    average_episode_duration: Option<u32>,
    studios: Vec<Named>,
    genres: Vec<Named>,
    my_list_status: Option<FullListStatus>,
    broadcast: Option<RawBroadcast>,
}

#[derive(Deserialize)]
struct RawBroadcast {
    day_of_the_week: Option<String>,
    start_time: Option<String>,
}

impl RawBroadcast {
    fn parse(self) -> Option<Broadcast> {
        Broadcast::parse(self.day_of_the_week.as_deref()?, self.start_time.as_deref())
    }
}

#[derive(Deserialize)]
struct Season {
    year: u16,
    season: String,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
struct FullListStatus {
    status: Option<String>,
    #[serde(default)]
    score: u8,
    #[serde(default)]
    num_episodes_watched: u32,
    #[serde(default)]
    is_rewatching: bool,
    #[serde(default)]
    num_times_rewatched: u32,
    start_date: Option<String>,
    finish_date: Option<String>,
}

impl FullNode {
    fn alt(&self) -> (Option<String>, Option<String>, Vec<String>) {
        match &self.alternative_titles {
            Some(a) => (
                non_empty(a.en.clone()),
                non_empty(a.ja.clone()),
                a.synonyms.iter().filter(|s| !s.trim().is_empty()).cloned().collect(),
            ),
            None => (None, None, Vec::new()),
        }
    }

    fn season(&self) -> Option<(u16, String)> {
        self.start_season.as_ref().map(|s| (s.year, s.season.clone()))
    }

    fn hit(self) -> AnimeHit {
        let (title_en, title_ja, synonyms) = self.alt();
        AnimeHit {
            id: self.id,
            season: self.season(),
            title: self.title,
            title_en,
            title_ja,
            synonyms,
            format: self.media_type.unwrap_or_default(),
            episodes: self.num_episodes.filter(|&e| e > 0),
            air_status: self.status.unwrap_or_default(),
            mean: self.mean.filter(|m| *m > 0.0),
            cover_url: self.main_picture.and_then(|p| p.medium.or(p.large)),
            ..AnimeHit::default()
        }
    }
}

/// The answer of the search bar's `GET /anime?q=…&fields=…`.
pub fn parse_hits(body: &str) -> Result<Vec<AnimeHit>, String> {
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        data: Vec<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        node: FullNode,
    }
    let page: Page = serde_json::from_str(body).map_err(|e| format!("can't read the search answer ({})", e.classify_name()))?;
    Ok(page.data.into_iter().map(|i| i.node.hit()).collect())
}

/// The answer of `GET /anime/season/{year}/{season}?fields=…`, and whether more pages follow.
pub fn parse_season(body: &str) -> Result<(Vec<SeasonAnime>, bool), String> {
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        data: Vec<Item>,
        paging: Option<Paging>,
    }
    #[derive(Deserialize)]
    struct Item {
        node: FullNode,
    }
    let page: Page = serde_json::from_str(body).map_err(|e| format!("can't read the season list ({})", e.classify_name()))?;
    let more = page.paging.and_then(|p| p.next).is_some_and(|n| !n.is_empty());
    let list = page
        .data
        .into_iter()
        .filter(|i| i.node.id != 0)
        .map(|i| {
            let n = i.node;
            let (title_en, title_ja, synonyms) = n.alt();
            let mine = n.my_list_status.and_then(|m| Some((m.status.filter(|s| Status::from_mal(s).is_some())?, m.num_episodes_watched)));
            SeasonAnime {
                id: n.id,
                title: n.title,
                title_en,
                title_ja,
                synonyms,
                format: n.media_type.unwrap_or_default(),
                episodes: n.num_episodes.filter(|&e| e > 0),
                air_status: n.status.unwrap_or_default(),
                start_date: non_empty(n.start_date),
                end_date: non_empty(n.end_date),
                broadcast: n.broadcast.and_then(RawBroadcast::parse),
                mean: n.mean.filter(|m| *m > 0.0),
                members: n.num_list_users,
                // The medium size (225 px wide) is plenty for a card.
                cover_url: n.main_picture.and_then(|p| p.medium.or(p.large)),
                mine,
                next_airing: None,
            }
        })
        .collect();
    Ok((list, more))
}

/// The answer of `GET /anime/{id}?fields=…`.
pub fn parse_details(body: &str) -> Result<Details, String> {
    let n: FullNode = serde_json::from_str(body).map_err(|e| format!("can't read the anime's information ({})", e.classify_name()))?;
    if n.id == 0 || n.title.is_empty() {
        return Err("MyAnimeList sent no anime".into());
    }
    let (title_en, title_ja, synonyms) = n.alt();
    let season = n.season();
    let mine = n.my_list_status.and_then(|m| {
        let status = m.status.filter(|s| Status::from_mal(s).is_some())?;
        Some(MyEntry {
            status,
            progress: m.num_episodes_watched,
            score: crate::Score::from_ten(m.score.into()),
            rewatching: m.is_rewatching,
            times_rewatched: m.num_times_rewatched,
            start_date: non_empty(m.start_date),
            finish_date: non_empty(m.finish_date),
            volumes: 0,
        })
    });
    let (cover_url, cover_small) = match n.main_picture {
        Some(p) => (p.large.clone().or(p.medium.clone()), p.medium.or(p.large)),
        None => (None, None),
    };
    Ok(Details {
        id: n.id,
        title: n.title,
        title_en,
        title_ja,
        synonyms,
        format: n.media_type.unwrap_or_default(),
        episodes: n.num_episodes.filter(|&e| e > 0),
        air_status: n.status.unwrap_or_default(),
        start_date: non_empty(n.start_date),
        end_date: non_empty(n.end_date),
        season,
        studios: n.studios.into_iter().map(|s| s.name).filter(|s| !s.is_empty()).collect(),
        genres: n.genres.into_iter().map(|g| g.name).filter(|g| !g.is_empty()).collect(),
        source: n.source.unwrap_or_default(),
        minutes: n.average_episode_duration.filter(|&d| d > 0).map(|d| d.div_ceil(60)),
        mean: n.mean.filter(|m| *m > 0.0),
        rank: n.rank.filter(|&r| r > 0),
        popularity: n.popularity.filter(|&r| r > 0),
        members: n.num_list_users,
        synopsis: n.synopsis.unwrap_or_default().trim().to_string(),
        cover_url,
        cover_small,
        mine,
        ..Details::default()
    })
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
}

#[derive(Deserialize)]
struct Me {
    name: String,
    #[serde(default)]
    picture: Option<String>,
    #[serde(default)]
    joined_at: Option<String>,
    #[serde(default)]
    anime_statistics: Option<RawStats>,
}

/// MAL's `anime_statistics` (the fields Kotora shows; the rest are left out).
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawStats {
    num_items_watching: f64,
    num_items_completed: f64,
    num_items_on_hold: f64,
    num_items_dropped: f64,
    num_items_plan_to_watch: f64,
    num_days: f64,
    num_episodes: f64,
    num_times_rewatched: f64,
    mean_score: f64,
}

/// The `/users/@me` answer. MAL sends counts as numbers that may carry a fraction; they are
/// rounded and kept non-negative.
fn parse_user(body: &str) -> Option<User> {
    let me: Me = serde_json::from_str(body).ok()?;
    let n = |v: f64| if v.is_finite() && v > 0.0 { v.round().min(f64::from(u32::MAX)) as u32 } else { 0 };
    let f = |v: f64| if v.is_finite() && v > 0.0 { v } else { 0.0 };
    let stats = me.anime_statistics.map(|r| AnimeStats {
        days: f(r.num_days),
        episodes: n(r.num_episodes),
        mean_score: f(r.mean_score).min(10.0),
        rewatched: n(r.num_times_rewatched),
        watching: n(r.num_items_watching),
        completed: n(r.num_items_completed),
        on_hold: n(r.num_items_on_hold),
        dropped: n(r.num_items_dropped),
        plan_to_watch: n(r.num_items_plan_to_watch),
    });
    Some(User {
        name: me.name,
        picture: me.picture.filter(|p| p.starts_with("https://")),
        profile: Profile { joined: me.joined_at.filter(|j| !j.trim().is_empty()), stats, ..Profile::default() },
    })
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
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    broadcast: Option<RawBroadcast>,
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
    #[serde(default)]
    num_times_rewatched: u32,
    updated_at: Option<String>,
}

impl ListStatus {
    fn remote(self) -> Option<Remote> {
        let status = self.status.as_deref().and_then(Status::from_mal)?;
        Some(Remote {
            values: Values {
                status,
                progress: self.num_episodes_watched,
                score: crate::Score::from_ten(self.score.into()),
                rewatching: self.is_rewatching,
                times_rewatched: self.num_times_rewatched,
                volumes: 0,
            },
            updated_at: self.updated_at.as_deref().and_then(time::parse_rfc3339).unwrap_or(0),
        })
    }
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// One page of `animelist`: the entries and the `paging.next` link. Entries without a list status
/// are skipped (they can't be on the list).
/// The answer of `GET /anime?q=`.
pub fn parse_search(body: &str) -> Result<Vec<SearchHit>, String> {
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        data: Vec<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        node: Node,
    }
    #[derive(Deserialize)]
    struct Node {
        id: u32,
        title: String,
        #[serde(default)]
        alternative_titles: Option<Alt>,
    }
    #[derive(Deserialize, Default)]
    struct Alt {
        #[serde(default)]
        synonyms: Vec<String>,
        #[serde(default)]
        en: Option<String>,
        #[serde(default)]
        ja: Option<String>,
    }
    let page: Page = serde_json::from_str(body).map_err(|e| format!("can't read the search answer ({})", e.classify_name()))?;
    Ok(page
        .data
        .into_iter()
        .map(|i| {
            let alt = i.node.alternative_titles.unwrap_or_default();
            let other_titles = alt.en.into_iter().chain(alt.ja).chain(alt.synonyms).filter(|t| !t.is_empty()).collect();
            SearchHit { id: i.node.id, title: i.node.title, other_titles }
        })
        .collect())
}

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
                start_date: non_empty(n.start_date),
                broadcast: n.broadcast.and_then(RawBroadcast::parse),
                authors: Vec::new(),
                kind: Default::default(),
                volumes: None,
                next_airing: None,
                id_mal: None,
            },
            status,
            progress: ls.num_episodes_watched,
            score: crate::Score::from_ten(ls.score.into()),
            rewatching: ls.is_rewatching,
            times_rewatched: ls.num_times_rewatched,
            updated_at: ls.updated_at.as_deref().and_then(time::parse_rfc3339).unwrap_or(0),
            volumes: 0,
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
        self.complete_sign_in_to(code, verifier, REDIRECT_URI)
    }

    /// [`Mal::complete_sign_in`] for a sign-in that used another redirect.
    pub fn complete_sign_in_to(&mut self, code: &str, verifier: &str, redirect_uri: &str) -> Result<(), ServiceError> {
        let t = self.client.exchange_code_to(code, verifier, redirect_uri)?;
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
        self.authed(|c, a| c.fetch_user(a)).map(|u| u.name)
    }

    fn user(&mut self) -> Result<User, ServiceError> {
        self.authed(|c, a| c.fetch_user(a))
    }

    fn fetch_list(&mut self) -> Result<Vec<ListItem>, ServiceError> {
        self.authed(|c, a| c.fetch_list(a))
    }

    fn get_entry(&mut self, media_id: u32) -> Result<Option<Remote>, ServiceError> {
        self.authed(|c, a| c.fetch_entry(a, media_id))
    }

    fn update_entry(&mut self, media_id: u32, change: &Change) -> Result<Remote, ServiceError> {
        self.authed(|c, a| c.patch_entry(a, media_id, change))
    }

    fn delete_entry(&mut self, media_id: u32) -> Result<(), ServiceError> {
        self.authed(|c, a| c.delete_entry(a, media_id))
    }

    fn search(&mut self, query: &str) -> Result<Vec<SearchHit>, ServiceError> {
        self.authed(|c, a| c.search_anime(a, query))
    }

    fn browse(&mut self, query: &str, limit: u32) -> Result<Vec<AnimeHit>, ServiceError> {
        self.authed(|c, a| c.browse_anime(a, query, limit))
    }

    fn season(&mut self, season: airing::Season, page: u32) -> Result<(Vec<SeasonAnime>, bool), ServiceError> {
        self.authed(|c, a| c.fetch_season(a, season, page))
    }

    fn details(&mut self, media_id: u32) -> Result<Details, ServiceError> {
        self.authed(|c, a| c.fetch_details(a, media_id))
    }

    fn fetch_manga_list(&mut self) -> Result<Vec<ListItem>, ServiceError> {
        self.manga_list()
    }

    fn get_manga_entry(&mut self, media_id: u32) -> Result<Option<Remote>, ServiceError> {
        self.manga_entry(media_id)
    }

    fn update_manga_entry(&mut self, media_id: u32, change: &Change) -> Result<Remote, ServiceError> {
        self.manga_patch(media_id, change)
    }

    fn delete_manga_entry(&mut self, media_id: u32) -> Result<(), ServiceError> {
        self.manga_delete(media_id)
    }

    fn browse_manga(&mut self, query: &str, limit: u32) -> Result<Vec<AnimeHit>, ServiceError> {
        self.manga_browse(query, limit)
    }

    fn manga_details(&mut self, media_id: u32) -> Result<Details, ServiceError> {
        self.manga_page(media_id)
    }
}

// ------------------------------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use crate::score::ten;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// `/users/@me?fields=picture,anime_statistics`, made up in MAL's shape.
    #[test]
    fn user_with_statistics() {
        let body = r#"{"id":1,"name":"demo_user","picture":"https://cdn.example.invalid/u.jpg","joined_at":"2019-03-02T10:20:30+00:00",
            "anime_statistics":{"num_items_watching":11,"num_items_completed":236,"num_items_on_hold":7,"num_items_dropped":2,
            "num_items_plan_to_watch":48,"num_items":304,"num_days_watched":61.3,"num_days":63.07,"num_episodes":3790.0,
            "num_times_rewatched":4,"mean_score":7.62}}"#;
        let u = parse_user(body).unwrap();
        assert_eq!(u.name, "demo_user");
        assert_eq!(u.picture.as_deref(), Some("https://cdn.example.invalid/u.jpg"));
        assert_eq!(u.profile.joined.as_deref(), Some("2019-03-02T10:20:30+00:00"));
        let st = u.profile.stats.unwrap();
        assert_eq!((st.watching, st.completed, st.on_hold, st.dropped, st.plan_to_watch), (11, 236, 7, 2, 48));
        assert_eq!(st.total(), 304);
        assert_eq!(st.count(Status::Completed), 236);
        assert_eq!((st.episodes, st.rewatched), (3790, 4));
        assert!((st.days - 63.07).abs() < 1e-9 && (st.mean_score - 7.62).abs() < 1e-9);
        // No statistics asked for, no picture, odd numbers: nothing breaks.
        let u = parse_user(r#"{"name":"x","picture":"http://insecure.invalid/p.jpg","anime_statistics":{"num_items_watching":-3,"num_episodes":2.6,"mean_score":12}}"#).unwrap();
        assert!(u.picture.is_none(), "https only");
        let st = u.profile.stats.unwrap();
        assert_eq!((st.watching, st.episodes, st.mean_score), (0, 3, 10.0));
        assert!(parse_user(r#"{"name":"x"}"#).unwrap().profile.stats.is_none());
        assert!(parse_user("not json").is_none());
    }

    /// Answers from a script and records every request it saw.
    /// (method, url, bearer, form body)
    type Sent = (String, String, Option<String>, Option<String>);

    pub(super) struct Fake {
        answers: RefCell<VecDeque<Result<Response, String>>>,
        seen: Seen,
    }

    impl Transport for Fake {
        fn send(&self, req: &Request) -> Result<Response, String> {
            self.seen.lock().unwrap().push((req.method.into(), req.url.clone(), req.bearer.clone(), req.form.clone()));
            self.answers.borrow_mut().pop_front().expect("unexpected request")
        }
    }

    pub(super) type Seen = Arc<Mutex<Vec<Sent>>>;

    pub(super) fn fake(answers: Vec<Result<Response, String>>) -> (Fake, Seen) {
        let seen: Seen = Arc::default();
        (Fake { answers: RefCell::new(answers.into()), seen: seen.clone() }, seen)
    }

    pub(super) fn ok(body: &str) -> Result<Response, String> {
        Ok(Response { status: 200, retry_after: None, body: body.into() })
    }

    pub(super) fn status(code: u16, retry_after: Option<u64>) -> Result<Response, String> {
        Ok(Response { status: code, retry_after, body: "{}".into() })
    }

    const FIXTURE: &str = include_str!("../tests/mal_animelist_page1.json");
    const FIXTURE2: &str = include_str!("../tests/mal_animelist_page2.json");

    pub(super) fn tok_json(access: &str) -> String {
        format!(r#"{{"token_type":"Bearer","expires_in":3600,"access_token":"{access}","refresh_token":"fake-refresh-{access}"}}"#)
    }

    pub(super) fn client(f: Fake) -> (Client<Fake>, Arc<Mutex<Vec<Duration>>>) {
        let waits: Arc<Mutex<Vec<Duration>>> = Arc::default();
        let w = waits.clone();
        (Client::new("fake-client-id".into(), f).with_own_budget().with_sleep(move |d| w.lock().unwrap().push(d)), waits)
    }

    pub(super) fn tmp_tokens(name: &str, t: Option<Token>) -> TokenStore {
        let dir = std::env::temp_dir().join(format!("kotora-mal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = TokenStore::new(dir.join("tokens.json"));
        if let Some(t) = t {
            s.save_mal(&t).unwrap();
        }
        s
    }

    pub(super) fn token(access: &str, expires_in: i64) -> Token {
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
        assert_eq!((a.media.id, a.media.title.as_str(), a.status, a.progress, a.score), (1001, "Lanterns of Kirigaoka", Status::Watching, 5, ten(8)));
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
        assert!(seen[0].1.starts_with("https://api.myanimelist.net/v2/users/@me/animelist?fields=list_status%7Bnum_times_rewatched%7D,"));
        assert_eq!(items.iter().map(|i| i.times_rewatched).collect::<Vec<_>>(), [0, 0, 0, 2], "the count when MAL sends it, else 0");
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

    // -- one entry: read and PATCH (recorded shapes, made-up data; MAL is never called)

    const ENTRY: &str = include_str!("../tests/mal_anime_entry.json");
    const NOT_ON_LIST: &str = include_str!("../tests/mal_anime_not_on_list.json");
    const PATCHED: &str = include_str!("../tests/mal_patch_answer.json");

    #[test]
    fn reads_one_entry_or_none() {
        let (f, seen) = fake(vec![ok(ENTRY), ok(NOT_ON_LIST)]);
        let (mut c, _) = client(f);
        let r = c.fetch_entry("acc", 1001).unwrap().unwrap();
        assert_eq!(r.values, Values::new(Status::Watching, 5, ten(8)));
        assert_eq!(r.updated_at, 1_519_450_460);
        assert_eq!(c.fetch_entry("acc", 1003).unwrap(), None);
        let seen = seen.lock().unwrap();
        assert_eq!((seen[0].0.as_str(), seen[0].1.as_str()), ("GET", "https://api.myanimelist.net/v2/anime/1001?fields=my_list_status%7Bnum_times_rewatched%7D"));
        assert_eq!(seen[0].2.as_deref(), Some("acc"));
    }

    #[test]
    fn patch_sends_only_the_changed_fields_with_mals_names() {
        let (f, seen) = fake(vec![ok(PATCHED), ok(PATCHED)]);
        let (mut c, _) = client(f);
        let r = c.patch_entry("acc", 1001, &Change { progress: Some(6), ..Default::default() }).unwrap();
        assert_eq!(r.values.progress, 6);
        assert_eq!(r.updated_at, 1_519_552_800);
        c.patch_entry("acc", 1001, &Change { status: Some(Status::Completed), score: Some(None), ..Default::default() }).unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!((seen[0].0.as_str(), seen[0].1.as_str()), ("PATCH", "https://api.myanimelist.net/v2/anime/1001/my_list_status"));
        assert_eq!(seen[0].3.as_deref(), Some("num_watched_episodes=6"));
        assert_eq!(seen[1].3.as_deref(), Some("status=completed&score=0"));
        assert_eq!(seen[1].2.as_deref(), Some("acc"));
        assert!(c.patch_entry("acc", 1, &Change::default()).is_err(), "an empty change is never sent");
    }

    /// Remove from list: `DELETE my_list_status`; not on the list (404) is done too, a server
    /// error waits for a retry.
    #[test]
    fn delete_removes_the_entry_and_a_404_is_done() {
        let status = |n: u16| Ok(Response { status: n, retry_after: None, body: String::new() });
        let (f, seen) = fake(vec![ok("[]"), status(404), status(503), status(400)]);
        let (mut c, _) = client(f);
        assert_eq!(c.delete_entry("acc", 1001), Ok(()));
        assert_eq!(c.delete_entry("acc", 1001), Ok(()), "already gone");
        assert!(matches!(c.delete_entry("acc", 1001), Err(ServiceError::Offline(_))));
        assert!(matches!(c.delete_entry("acc", 1001), Err(ServiceError::Other(_))));
        let seen = seen.lock().unwrap();
        assert_eq!((seen[0].0.as_str(), seen[0].1.as_str(), seen[0].2.as_deref(), seen[0].3.as_deref()), ("DELETE", "https://api.myanimelist.net/v2/anime/1001/my_list_status", Some("acc"), None));
    }

    /// A manga's removal: its own address; 404 is done too.
    #[test]
    fn delete_removes_a_manga_entry() {
        let status = |n: u16| Ok(Response { status: n, retry_after: None, body: String::new() });
        let (f, seen) = fake(vec![ok("[]"), status(404), status(503)]);
        let (mut c, _) = client(f);
        assert_eq!(c.delete_manga_entry("acc", 2001), Ok(()));
        assert_eq!(c.delete_manga_entry("acc", 2001), Ok(()), "already gone");
        assert!(matches!(c.delete_manga_entry("acc", 2001), Err(ServiceError::Offline(_))));
        let seen = seen.lock().unwrap();
        assert_eq!((seen[0].0.as_str(), seen[0].1.as_str()), ("DELETE", "https://api.myanimelist.net/v2/manga/2001/my_list_status"));
    }

    /// A rewatch goes as MAL's form fields, and its answer is read back.
    #[test]
    fn patch_sends_the_rewatch() {
        let started = r#"{"status":"completed","score":8,"num_episodes_watched":0,"is_rewatching":true,"num_times_rewatched":1,"updated_at":"2026-11-15T10:00:00+00:00"}"#;
        let ended = r#"{"status":"completed","score":8,"num_episodes_watched":12,"is_rewatching":false,"num_times_rewatched":2,"updated_at":"2026-11-15T11:00:00+00:00"}"#;
        let (f, seen) = fake(vec![ok(started), ok(ended)]);
        let (mut c, _) = client(f);
        let r = c.patch_entry("acc", 1001, &Change { progress: Some(0), rewatching: Some(true), ..Default::default() }).unwrap();
        assert_eq!(r.values, Values { rewatching: true, times_rewatched: 1, ..Values::new(Status::Completed, 0, ten(8)) });
        let end = Change { status: Some(Status::Completed), progress: Some(12), rewatching: Some(false), times_rewatched: Some(2), score: None, volumes: None };
        let r = c.patch_entry("acc", 1001, &end).unwrap();
        assert_eq!((r.values.rewatching, r.values.times_rewatched, r.values.progress), (false, 2, 12));
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].3.as_deref(), Some("num_watched_episodes=0&is_rewatching=true"));
        assert_eq!(seen[1].3.as_deref(), Some("status=completed&num_watched_episodes=12&is_rewatching=false&num_times_rewatched=2"));
    }

    #[test]
    fn a_refused_patch_carries_mals_words() {
        let body = r#"{"error":"invalid_content","message":"num_watched_episodes is out of range"}"#;
        let (f, _) = fake(vec![Ok(Response { status: 400, retry_after: None, body: body.into() })]);
        let (mut c, _) = client(f);
        let e = c.patch_entry("acc", 1, &Change { progress: Some(99), ..Default::default() }).unwrap_err();
        assert!(matches!(&e, ServiceError::Other(m) if m.contains("out of range")), "{e:?}");
    }

    #[test]
    fn update_entry_refreshes_on_401_and_sends_again() {
        let (f, seen) = fake(vec![status(401, None), ok(&tok_json("new")), ok(PATCHED)]);
        let (c, _) = client(f);
        let mut mal = Mal::new(c, tmp_tokens("patch401", Some(token("old", 3000))));
        let r = mal.update_entry(1001, &Change { progress: Some(6), ..Default::default() }).unwrap();
        assert_eq!(r.values.progress, 6);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!((seen[2].0.as_str(), seen[2].2.as_deref()), ("PATCH", Some("new")));
    }

    const HITS: &str = include_str!("../tests/mal_search_browse.json");
    const DETAILS: &str = include_str!("../tests/mal_anime_details.json");

    #[test]
    fn browse_reads_rows_and_asks_for_their_fields() {
        let (f, seen) = fake(vec![ok(HITS)]);
        let (mut c, _) = client(f);
        let hits = c.browse_anime("fake-access", "  velvet orchard ", 10).unwrap();
        let url = seen.lock().unwrap()[0].1.clone();
        assert!(url.starts_with("https://api.myanimelist.net/v2/anime?q=velvet%20orchard&limit=10&fields="), "{url}");
        assert!(url.contains("start_season") && url.contains("mean") && url.contains("num_episodes"));
        assert_eq!(hits.len(), 2);
        let h = &hits[0];
        assert_eq!((h.id, h.title.as_str(), h.format.as_str(), h.episodes), (9001, "Velvet Orchard Chronicle", "tv", Some(12)));
        assert_eq!(h.season_text().as_deref(), Some("Fall 2025"));
        assert_eq!(h.mean, Some(8.12));
        assert_eq!(h.title_en.as_deref(), Some("Velvet Orchard"));
        assert_eq!(h.cover_url.as_deref(), Some("https://cdn.example.invalid/images/anime/1/9001.jpg"));
        assert_eq!(h.media().title, "Velvet Orchard Chronicle");
        // Unknown count and no score: none, not zero.
        assert_eq!((hits[1].episodes, hits[1].mean, hits[1].season.as_ref()), (None, None, None));
        assert!(c.browse_anime("fake-access", "ab", 10).unwrap().is_empty(), "under 3 characters nothing is asked");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn details_read_every_field_and_the_users_entry() {
        let (f, seen) = fake(vec![ok(DETAILS)]);
        let (mut c, _) = client(f);
        let d = c.fetch_details("fake-access", 9001).unwrap();
        let url = seen.lock().unwrap()[0].1.clone();
        assert!(url.starts_with("https://api.myanimelist.net/v2/anime/9001?fields=") && url.contains("my_list_status") && url.contains("synopsis"), "{url}");
        assert_eq!(d.title, "Velvet Orchard Chronicle");
        assert_eq!(d.title_ja.as_deref(), Some("ベルベット果樹園"));
        assert_eq!(d.synonyms, ["VOC"]);
        assert_eq!((d.episodes, d.format.as_str(), d.air_status.as_str()), (Some(12), "tv", "finished_airing"));
        assert_eq!((d.start_date.as_deref(), d.end_date.as_deref()), (Some("2025-10-03"), Some("2025-12-19")));
        assert_eq!(d.studios, ["Studio Example"]);
        assert_eq!(d.genres, ["Drama", "Slice of Life"]);
        assert_eq!((d.mean, d.rank, d.popularity, d.members), (Some(8.12), Some(312), Some(1450), Some(98765)));
        assert_eq!(d.minutes, Some(24));
        assert_eq!(d.source, "light_novel");
        assert!(d.synopsis.starts_with("A made-up story"));
        assert_eq!(d.cover_url.as_deref(), Some("https://cdn.example.invalid/images/anime/1/9001l.jpg"));
        assert_eq!(d.media().cover_url.as_deref(), Some("https://cdn.example.invalid/images/anime/1/9001.jpg"), "rows use the small cover");
        let m = d.mine.unwrap();
        assert_eq!((m.status(), m.progress, m.score, m.rewatching), (Some(Status::Watching), 5, ten(8), false));
        assert_eq!((m.start_date.as_deref(), m.finish_date), (Some("2025-10-10"), None));
        // Not on the list: no entry.
        let mut v: serde_json::Value = serde_json::from_str(DETAILS).unwrap();
        v.as_object_mut().unwrap().remove("my_list_status");
        assert!(parse_details(&v.to_string()).unwrap().mine.is_none());
        assert!(parse_details("{}").is_err());
        assert!(parse_details("<html>").is_err());
    }

    #[test]
    fn search_reads_hits_and_skips_short_queries() {
        let body = r#"{"data":[{"node":{"id":2001,"title":"Hoshizora Yuubin","main_picture":{"medium":"https://cdn.example.invalid/a.jpg"},
            "alternative_titles":{"synonyms":["Starfall Post"],"en":"Starfall Delivery Service","ja":""}}},
            {"node":{"id":2002,"title":"Hoshizora Yuubin 2nd Season"}}],"paging":{}}"#;
        let (f, seen) = fake(vec![ok(body)]);
        let (mut c, _) = client(f);
        let hits = c.search_anime("acc", "Starfall Delivery Service").unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!((hits[0].id, hits[0].title.as_str()), (2001, "Hoshizora Yuubin"));
        assert_eq!(hits[0].other_titles, ["Starfall Delivery Service", "Starfall Post"]);
        assert!(hits[1].other_titles.is_empty());
        assert_eq!(c.search_anime("acc", "ab").unwrap(), vec![], "too short: no request");
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].1, "https://api.myanimelist.net/v2/anime?q=Starfall%20Delivery%20Service&limit=5&fields=alternative_titles");
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

    #[test]
    fn the_user_reads_with_the_picture() {
        let (f, seen) = fake(vec![
            ok(r#"{"id":5,"name":"demo_user","picture":"https://cdn.example.invalid/images/userimages/5.jpg?t=1","location":""}"#),
            ok(r#"{"id":5,"name":"demo_user"}"#),
        ]);
        let (c, _) = client(f);
        let mut mal = Mal::new(c, tmp_tokens("user", Some(token("fresh", 3000))));
        let u = mal.user().unwrap();
        assert_eq!((u.name.as_str(), u.picture.as_deref()), ("demo_user", Some("https://cdn.example.invalid/images/userimages/5.jpg?t=1")));
        assert_eq!(mal.user().unwrap().picture, None, "no picture set");
        assert_eq!(seen.lock().unwrap()[0].1, "https://api.myanimelist.net/v2/users/@me?fields=picture,anime_statistics");
    }

    /// Made-up season answer in MyAnimeList's shape (no real titles).
    const SEASON: &str = r#"{"data":[
        {"node":{"id":9201,"title":"Kirigaoka no Lantern","main_picture":{"medium":"https://cdn.example.invalid/9201.jpg","large":"https://cdn.example.invalid/9201l.jpg"},
          "alternative_titles":{"synonyms":[],"en":"Lanterns of Kirigaoka","ja":"霧ヶ丘のランタン"},"media_type":"tv","num_episodes":12,"status":"currently_airing",
          "start_date":"2026-10-03","end_date":"2026-12-19","broadcast":{"day_of_the_week":"saturday","start_time":"23:30"},"mean":8.12,"num_list_users":154321,
          "my_list_status":{"status":"watching","score":0,"num_episodes_watched":6,"is_rewatching":false,"updated_at":"2026-11-10T10:00:00+00:00"}}},
        {"node":{"id":9202,"title":"Hoshizora Web Shorts","media_type":"ona","num_episodes":0,"status":"currently_airing","start_date":"2026-10",
          "broadcast":{"day_of_the_week":"friday"},"num_list_users":1200}},
        {"node":{"id":9203,"title":"Late Night Ronin","media_type":"tv","status":"not_yet_aired","start_date":"2026-11-29",
          "broadcast":{"day_of_the_week":"saturday","start_time":"25:30"},"num_list_users":900}}
      ],"paging":{"next":"https://api.myanimelist.net/v2/anime/season/2026/fall?offset=100"},"season":{"year":2026,"season":"fall"}}"#;

    #[test]
    fn reads_a_season_page() {
        let (list, more) = parse_season(SEASON).unwrap();
        assert!(more);
        assert_eq!(list.len(), 3);
        let a = &list[0];
        assert_eq!((a.id, a.title_en.as_deref(), a.episodes, a.members), (9201, Some("Lanterns of Kirigaoka"), Some(12), Some(154_321)));
        assert_eq!(a.broadcast, Some(Broadcast { day: 5, minute: Some(23 * 60 + 30) }));
        assert_eq!((a.start_date.as_deref(), a.end_date.as_deref()), (Some("2026-10-03"), Some("2026-12-19")));
        assert_eq!(a.mine, Some(("watching".into(), 6)));
        assert_eq!(a.cover_url.as_deref(), Some("https://cdn.example.invalid/9201.jpg"));
        let b = &list[1];
        assert_eq!((b.episodes, b.broadcast, b.mine.as_ref()), (None, Some(Broadcast { day: 4, minute: None }), None));
        assert_eq!(list[2].broadcast, Some(Broadcast { day: 6, minute: Some(90) }), "25:30 Saturday is 01:30 Sunday");
        let (_, more) = parse_season(r#"{"data":[],"paging":{}}"#).unwrap();
        assert!(!more);
        assert!(parse_season("nope").is_err());
    }

    #[test]
    fn the_season_request_asks_by_members_and_pages() {
        let (f, seen) = fake(vec![ok(SEASON)]);
        let (mut c, _) = client(f);
        let (list, _) = c.fetch_season("a", airing::Season { year: 2026, quarter: 3 }, 1).unwrap();
        assert_eq!(list.len(), 3);
        let url = seen.lock().unwrap()[0].1.clone();
        assert!(url.starts_with("https://api.myanimelist.net/v2/anime/season/2026/fall?sort=anime_num_list_users&limit=100&offset=100&fields="), "{url}");
        assert!(url.contains("broadcast") && url.contains("start_date") && url.contains("num_list_users") && url.contains("my_list_status"));
    }
}
