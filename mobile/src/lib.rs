//! Kotora's mobile API: one [`Kotora`] object over `kotora-core` (built without its `desktop`
//! feature), exported to Kotlin (and later Swift) with UniFFI.
//!
//! The core stays blocking, as on the desktop: Kotlin calls the network methods ([`Kotora::sync`],
//! [`Kotora::flush_queue`], [`Kotora::sign_in_finish`]) off the main thread. Local reads (the
//! list, counts) are a few milliseconds of SQLite.
//!
//! The UI's reads share one store connection behind a mutex; every network call opens its own
//! connection (WAL), as the desktop's worker threads do, so a slow request never holds the list.
//!
//! Platform types never appear here: paths, the UTC offset and the token vault come in through
//! [`Paths`] and the [`Platform`] callback.

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use kotora_core::anilist::{self, AniList};
use kotora_core::http::UreqTransport;
use kotora_core::mal::{self, Mal, Pkce};
use kotora_core::queue::{self, Outcome, Step};
use kotora_core::service::{ListService, ServiceError, ServiceId, User};
use kotora_core::store::Store;
use kotora_core::tokens::{TokenStore, TokenVault};
use kotora_core::{Change, Kind, ListItem, Scope, ScoreFormat, Status, paths, time};

uniffi::setup_scaffolding!();

mod edit;
pub use edit::*;
mod remove;
pub use remove::*;
mod insights;
pub use insights::*;

/// The redirect registered with Kotora's MyAnimeList client for Android.
pub const MAL_REDIRECT_URI: &str = "io.github.hanami117.kotora://oauth/mal";
/// The redirect registered with Kotora's AniList client for Android (an AniList client has one).
pub const ANILIST_REDIRECT_URI: &str = "io.github.hanami117.kotora://oauth/anilist";

/// How many queued changes one [`Kotora::flush_queue`] sends at most per service (each is one or
/// two requests within the service's rate budget).
const FLUSH_MAX: u32 = 40;

// ------------------------------------------------------------------------------------- types

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Service {
    Mal,
    AniList,
}

impl Service {
    const ALL: [Service; 2] = [Service::Mal, Service::AniList];

    fn id(self) -> ServiceId {
        match self {
            Service::Mal => ServiceId::Mal,
            Service::AniList => ServiceId::AniList,
        }
    }

    fn name(self) -> &'static str {
        self.id().name()
    }

    fn from_name(s: &str) -> Option<Service> {
        Service::ALL.into_iter().find(|v| v.name() == s)
    }
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Anime,
    Manga,
}

impl From<MediaKind> for Kind {
    fn from(k: MediaKind) -> Kind {
        match k {
            MediaKind::Anime => Kind::Anime,
            MediaKind::Manga => Kind::Manga,
        }
    }
}

impl From<Kind> for MediaKind {
    fn from(k: Kind) -> MediaKind {
        match k {
            Kind::Anime => MediaKind::Anime,
            Kind::Manga => MediaKind::Manga,
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListStatus {
    Watching,
    PlanToWatch,
    Completed,
    OnHold,
    Dropped,
}

impl ListStatus {
    const ALL: [ListStatus; 5] = [ListStatus::Watching, ListStatus::PlanToWatch, ListStatus::Completed, ListStatus::OnHold, ListStatus::Dropped];
}

/// Whether an entry shows under `page`: its own status, and a rewatch (status Completed) under
/// Watching too, as on the desktop.
fn shows_in(status: ListStatus, rewatching: bool, page: ListStatus) -> bool {
    status == page || (page == ListStatus::Watching && rewatching)
}

impl From<ListStatus> for Status {
    fn from(s: ListStatus) -> Status {
        match s {
            ListStatus::Watching => Status::Watching,
            ListStatus::PlanToWatch => Status::PlanToWatch,
            ListStatus::Completed => Status::Completed,
            ListStatus::OnHold => Status::OnHold,
            ListStatus::Dropped => Status::Dropped,
        }
    }
}

impl From<Status> for ListStatus {
    fn from(s: Status) -> ListStatus {
        match s {
            Status::Watching => ListStatus::Watching,
            Status::PlanToWatch => ListStatus::PlanToWatch,
            Status::Completed => ListStatus::Completed,
            Status::OnHold => ListStatus::OnHold,
            Status::Dropped => ListStatus::Dropped,
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    /// Last changed first (the service's `updated_at`, or our own edit).
    Updated,
    Title,
    /// Highest score first, unscored last.
    Score,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// The app's folders. Each is used as it is (no `kotora` is added). The database and tokens go in
/// `data_dir` (the app passes `noBackupFilesDir`), `config.toml` in `config_dir`, caches in
/// `cache_dir`.
#[derive(uniffi::Record, Clone, Debug)]
pub struct Paths {
    pub config_dir: String,
    pub data_dir: String,
    pub cache_dir: String,
}

/// The Client IDs this build signs in with; empty: none in this build (sign-in says so).
#[derive(uniffi::Record, Clone, Debug, Default)]
pub struct ClientIds {
    pub mal: String,
    pub anilist: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Account {
    pub service: Service,
    pub user_name: String,
    pub picture_url: Option<String>,
    /// The service whose list the app shows.
    pub active: bool,
}

/// One sign-in in progress. The app keeps it (also across process death: the browser may take a
/// while) and passes it back with the redirect to [`Kotora::sign_in_finish`].
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PendingSignIn {
    pub service: Service,
    /// The page to open in a Custom Tab.
    pub url: String,
    /// MyAnimeList: must come back in the redirect. Empty for AniList.
    pub state: String,
    /// MyAnimeList's PKCE verifier. Empty for AniList.
    pub verifier: String,
}

/// One list entry as the list page and the details page show it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Row {
    pub kind: MediaKind,
    pub id: u32,
    pub title: String,
    pub title_en: Option<String>,
    pub status: ListStatus,
    /// Episodes watched, or chapters read.
    pub progress: u32,
    /// Episodes, or chapters; `None`: unknown.
    pub total: Option<u32>,
    /// Manga: volumes read and the volume count.
    pub volumes: u32,
    pub total_volumes: Option<u32>,
    /// In the user's score format ("8", "7.5", "85"); `None`: not scored.
    pub score: Option<String>,
    /// The service's raw format (`tv`, `movie`, `manga`, …) and airing status
    /// (`currently_airing`, …).
    pub format: String,
    pub air_status: String,
    pub start_date: Option<String>,
    pub cover_url: Option<String>,
    pub rewatching: bool,
    pub times_rewatched: u32,
    /// A change of this entry waits in the queue ("Waiting to sync").
    pub waiting: bool,
    pub updated_at: i64,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct StatusCount {
    pub status: ListStatus,
    pub count: u32,
}

/// An edit just made, with what Undo puts back.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Edit {
    pub kind: MediaKind,
    pub id: u32,
    /// "Hoshikuzu Relay: episode 7 saved".
    pub message: String,
    pub before_status: ListStatus,
    pub before_progress: u32,
    pub before_rewatching: bool,
    pub before_times_rewatched: u32,
    pub before_volumes: u32,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct SyncReport {
    pub service: Service,
    pub anime: u32,
    /// `None`: Manga mode is off (the manga list wasn't asked for).
    pub manga: Option<u32>,
    pub synced_at: i64,
}

#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct FlushReport {
    pub sent: u32,
    pub conflicts: u32,
    pub failed: u32,
    /// Still waiting (offline, rate limited, or more than one run sends).
    pub waiting: u32,
    /// A service wants a new sign-in; its changes wait.
    pub sign_in_again: bool,
    /// Seconds until the next change may go (a retry after an error), when one waits.
    pub retry_in: Option<i64>,
}

/// Kotlin gets one exception class per variant, with this `Display` text as its message (the
/// words the app shows).
#[derive(uniffi::Error, Debug, Clone, PartialEq)]
#[uniffi(flat_error)]
pub enum KotoraError {
    /// No network, a timeout or the service's trouble. The saved list still works.
    Offline { message: String },
    SignInAgain,
    RateLimited,
    NotSignedIn,
    /// This build has no Client ID for the service.
    NoClientId,
    /// The browser came back with something we can't use (refused, wrong state, no code).
    SignInFailed { message: String },
    NotFound,
    Other { message: String },
}

impl fmt::Display for KotoraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KotoraError::Offline { message } => write!(f, "{message}"),
            KotoraError::SignInAgain => write!(f, "Sign in again."),
            KotoraError::RateLimited => write!(f, "The service asked Kotora to slow down. Try again in a minute."),
            KotoraError::NotSignedIn => write!(f, "Not signed in."),
            KotoraError::NoClientId => write!(f, "This build of Kotora has no Client ID for that service."),
            KotoraError::SignInFailed { message } => write!(f, "{message}"),
            KotoraError::NotFound => write!(f, "That isn't on your list."),
            KotoraError::Other { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for KotoraError {}

impl From<ServiceError> for KotoraError {
    fn from(e: ServiceError) -> Self {
        match e {
            ServiceError::Offline(message) => KotoraError::Offline { message },
            ServiceError::SignInAgain => KotoraError::SignInAgain,
            ServiceError::RateLimited => KotoraError::RateLimited,
            ServiceError::Other(message) => KotoraError::Other { message },
        }
    }
}

impl From<kotora_core::store::StoreError> for KotoraError {
    fn from(e: kotora_core::store::StoreError) -> Self {
        KotoraError::Other { message: format!("The saved list can't be read or written: {e}") }
    }
}

fn other(e: impl fmt::Display) -> KotoraError {
    KotoraError::Other { message: e.to_string() }
}

// ---------------------------------------------------------------------------------- platform

/// What the app supplies: the user's time zone, a log, and the token vault (Keystore-wrapped on
/// Android).
#[uniffi::export(with_foreign)]
pub trait Platform: Send + Sync {
    /// Seconds east of UTC at `at_unix`, in the zone the user picked (DST included).
    fn utc_offset(&self, at_unix: i64) -> i32;
    fn log(&self, level: LogLevel, message: String);
    fn token_load(&self, service: String) -> Option<Vec<u8>>;
    /// False when the token couldn't be kept.
    fn token_save(&self, service: String, bytes: Vec<u8>) -> bool;
    fn token_delete(&self, service: String);
}

struct Vault(Arc<dyn Platform>);

impl TokenVault for Vault {
    fn load(&self, service: &str) -> Option<Vec<u8>> {
        self.0.token_load(service.to_string())
    }

    fn save(&self, service: &str, bytes: &[u8]) -> std::io::Result<()> {
        if self.0.token_save(service.to_string(), bytes.to_vec()) { Ok(()) } else { Err(std::io::Error::other("the token vault refused it")) }
    }

    fn delete(&self, service: &str) -> std::io::Result<()> {
        self.0.token_delete(service.to_string());
        Ok(())
    }
}

// ------------------------------------------------------------------------------------ Kotora

const ACTIVE_KEY: &str = "mobile.active_service";

#[derive(uniffi::Object)]
pub struct Kotora {
    db: PathBuf,
    store: Mutex<Store>,
    tokens: TokenStore,
    ids: ClientIds,
    platform: Arc<dyn Platform>,
}

impl Kotora {
    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A connection of its own, for a call that also waits on the network.
    fn own_store(&self) -> Result<Store, KotoraError> {
        Ok(Store::open(&self.db)?)
    }

    fn client_id(&self, service: Service) -> Option<String> {
        let id = match service {
            Service::Mal => &self.ids.mal,
            Service::AniList => &self.ids.anilist,
        };
        Some(id.trim().to_string()).filter(|s| !s.is_empty())
    }

    fn signed_in(&self, service: Service) -> bool {
        self.tokens.load(service.name()).is_some()
    }

    fn service(&self, service: Service) -> Result<Box<dyn ListService>, KotoraError> {
        if !self.signed_in(service) {
            return Err(KotoraError::NotSignedIn);
        }
        Ok(match service {
            Service::Mal => {
                let id = self.client_id(Service::Mal).ok_or(KotoraError::NoClientId)?;
                Box::new(Mal::new(mal::Client::new(id, UreqTransport::default()), self.tokens.clone()))
            }
            Service::AniList => Box::new(AniList::new(anilist::Client::new(UreqTransport::default()), self.tokens.clone())),
        })
    }

    fn active(&self) -> Option<Service> {
        let saved = self.store().kv_get(ACTIVE_KEY).ok().flatten().and_then(|s| Service::from_name(&s));
        saved.filter(|s| self.signed_in(*s)).or_else(|| Service::ALL.into_iter().find(|s| self.signed_in(*s)))
    }

    fn active_or_err(&self) -> Result<Service, KotoraError> {
        self.active().ok_or(KotoraError::NotSignedIn)
    }

    fn score_format(&self, store: &Store, service: Service) -> ScoreFormat {
        match service {
            Service::Mal => ScoreFormat::default(),
            Service::AniList => store.profile(service.name()).ok().flatten().map(|p| p.score_format).unwrap_or_default(),
        }
    }

    fn save_account(&self, service: Service, user: &User) -> Result<Account, KotoraError> {
        let store = self.store();
        store.set_account(service.name(), &user.name)?;
        let _ = store.set_picture(service.name(), user.picture.as_deref(), time::now_unix());
        let _ = store.set_profile(service.name(), &user.profile);
        // The first account (or the one just signed in to, when the other isn't) is the active one.
        let active = match store.kv_get(ACTIVE_KEY)?.and_then(|s| Service::from_name(&s)) {
            Some(s) if s != service && self.signed_in(s) => false,
            _ => {
                store.kv_set(ACTIVE_KEY, service.name())?;
                true
            }
        };
        Ok(Account { service, user_name: user.name.clone(), picture_url: user.picture.clone(), active })
    }

    fn rows(&self, store: &Store, service: Service, kind: Kind) -> Result<Vec<Row>, KotoraError> {
        let fmt = self.score_format(store, service);
        let waiting = waiting_ids(store, service, kind);
        Ok(store.load_list(Scope::new(service.name(), kind))?.iter().map(|it| row(it, fmt, waiting.contains(&it.media.id))).collect())
    }

    fn log(&self, level: LogLevel, message: String) {
        self.platform.log(level, message);
    }
}

fn waiting_ids(store: &Store, service: Service, kind: Kind) -> Vec<u32> {
    store.queue(service.name(), time::now_unix()).unwrap_or_default().into_iter().filter(|o| o.kind == kind && o.state.is_open()).map(|o| o.media_id).collect()
}

fn row(it: &ListItem, fmt: ScoreFormat, waiting: bool) -> Row {
    let m = &it.media;
    Row {
        kind: m.kind.into(),
        id: m.id,
        title: m.title.clone(),
        title_en: m.title_en.clone(),
        status: it.status.into(),
        progress: it.progress,
        total: m.episodes.filter(|n| *n > 0),
        volumes: it.volumes,
        total_volumes: m.volumes.filter(|n| *n > 0),
        score: it.score.map(|s| fmt.show(s)),
        format: m.format.clone(),
        air_status: m.air_status.clone(),
        start_date: m.start_date.clone(),
        cover_url: m.cover_url.clone(),
        rewatching: it.rewatching,
        times_rewatched: it.times_rewatched,
        waiting,
        updated_at: it.updated_at,
    }
}

/// The `code` of MyAnimeList's redirect (`…?code=…&state=…`), with the state checked.
fn mal_code(redirect: &str, state: &str) -> Result<String, KotoraError> {
    let fail = |m: &str| KotoraError::SignInFailed { message: m.to_string() };
    let query = redirect.split_once('?').map(|(_, q)| q).unwrap_or("");
    let query = query.split('#').next().unwrap_or("");
    let (mut code, mut got_state, mut error) = (None, None, None);
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("code", v)) => code = Some(kotora_core::http::pct_decode(v)),
            Some(("state", v)) => got_state = Some(kotora_core::http::pct_decode(v)),
            Some(("error", v)) => error = Some(kotora_core::http::pct_decode(v)),
            _ => {}
        }
    }
    if let Some(e) = error {
        return Err(fail(&if e == "access_denied" { "You cancelled the sign-in on MyAnimeList.".to_string() } else { format!("MyAnimeList refused the sign-in ({e}).") }));
    }
    if got_state.as_deref() != Some(state) || state.is_empty() {
        return Err(fail("That sign-in answer belongs to another attempt. Sign in again."));
    }
    code.filter(|c| !c.is_empty()).ok_or_else(|| fail("MyAnimeList sent no sign-in code. Sign in again."))
}

#[uniffi::export]
impl Kotora {
    /// Opens (or makes) the database. One per process: the folders and the offset source are
    /// set for the whole process on the first call.
    #[uniffi::constructor]
    pub fn new(paths: Paths, ids: ClientIds, platform: Arc<dyn Platform>) -> Result<Arc<Self>, KotoraError> {
        let data = PathBuf::from(&paths.data_dir);
        let roots = paths::Roots { config: PathBuf::from(&paths.config_dir), data: data.clone(), cache: PathBuf::from(&paths.cache_dir), state: data.join("state") };
        paths::set_roots(roots);
        let p = platform.clone();
        time::set_offset_source(Box::new(move |t| i64::from(p.utc_offset(t))));
        paths::create_private_dir(&data).map_err(|e| other(format!("Can't make the data folder: {e}")))?;
        let db = data.join("kotora.db");
        let store = Store::open(&db)?;
        let tokens = TokenStore::with_vault(Arc::new(Vault(platform.clone())));
        Ok(Arc::new(Kotora { db, store: Mutex::new(store), tokens, ids, platform }))
    }

    pub fn has_client_id(&self, service: Service) -> bool {
        self.client_id(service).is_some()
    }

    /// The signed-in accounts (a token and a saved user name).
    pub fn accounts(&self) -> Vec<Account> {
        let active = self.active();
        let store = self.store();
        Service::ALL
            .into_iter()
            .filter(|s| self.signed_in(*s))
            .map(|s| Account {
                service: s,
                user_name: store.account(s.name()).ok().flatten().unwrap_or_default(),
                picture_url: store.picture(s.name()).ok().and_then(|p| p.0),
                active: active == Some(s),
            })
            .collect()
    }

    pub fn active_service(&self) -> Option<Service> {
        self.active()
    }

    pub fn set_active_service(&self, service: Service) -> Result<(), KotoraError> {
        if !self.signed_in(service) {
            return Err(KotoraError::NotSignedIn);
        }
        Ok(self.store().kv_set(ACTIVE_KEY, service.name())?)
    }

    /// The page to open for a sign-in, and what [`Kotora::sign_in_finish`] needs back.
    pub fn sign_in_start(&self, service: Service) -> Result<PendingSignIn, KotoraError> {
        let id = self.client_id(service).ok_or(KotoraError::NoClientId)?;
        Ok(match service {
            Service::Mal => {
                let pkce = Pkce::new().map_err(|e| other(format!("Can't make a sign-in code: {e}")))?;
                let url = mal::authorize_url_to(&id, &pkce, MAL_REDIRECT_URI);
                PendingSignIn { service, url, state: pkce.state, verifier: pkce.verifier }
            }
            Service::AniList => PendingSignIn { service, url: anilist::authorize_url(&id), state: String::new(), verifier: String::new() },
        })
    }

    /// Completes a sign-in with the address the browser came back to. Network: call it off the
    /// main thread.
    pub fn sign_in_finish(&self, pending: PendingSignIn, redirect: String) -> Result<Account, KotoraError> {
        match pending.service {
            Service::Mal => {
                let code = mal_code(&redirect, &pending.state)?;
                let id = self.client_id(Service::Mal).ok_or(KotoraError::NoClientId)?;
                let mut svc = Mal::new(mal::Client::new(id, UreqTransport::default()), self.tokens.clone());
                let refused = |e: ServiceError| match e {
                    ServiceError::SignInAgain => KotoraError::SignInFailed { message: "MyAnimeList refused the sign-in. Try again.".into() },
                    e => e.into(),
                };
                svc.complete_sign_in_to(&code, &pending.verifier, MAL_REDIRECT_URI).map_err(refused)?;
                let user = svc.user().map_err(refused)?;
                self.log(LogLevel::Info, "signed in to MyAnimeList".into());
                self.save_account(Service::Mal, &user)
            }
            Service::AniList => self.sign_in_with_token(redirect),
        }
    }

    /// AniList: the redirect address, or a token pasted from AniList's PIN page.
    pub fn sign_in_with_token(&self, raw: String) -> Result<Account, KotoraError> {
        let token = anilist::parse_token_input(&raw, time::now_unix()).map_err(|message| KotoraError::SignInFailed { message })?;
        let mut svc = AniList::new(anilist::Client::new(UreqTransport::default()), self.tokens.clone());
        let user = svc.complete_sign_in(&token).map_err(|e| match e {
            ServiceError::SignInAgain => KotoraError::SignInFailed { message: "AniList refused the token. Sign in again.".into() },
            e => e.into(),
        })?;
        self.log(LogLevel::Info, "signed in to AniList".into());
        self.save_account(Service::AniList, &user)
    }

    /// Sign out: the token, the saved lists, details and waiting changes of `service` go (the MAL
    /// licence; AniList's terms). The other service's stay. The app clears its cover cache.
    pub fn sign_out(&self, service: Service) -> Result<(), KotoraError> {
        self.tokens.forget(service.name()).map_err(other)?;
        let mut store = self.store();
        store.clear_service(service.name())?;
        if store.kv_get(ACTIVE_KEY)?.as_deref() == Some(service.name()) {
            store.kv_del(ACTIVE_KEY)?;
        }
        Ok(())
    }

    pub fn manga_enabled(&self) -> bool {
        self.store().manga_enabled().unwrap_or(false)
    }

    /// Manga mode: the next sync also fetches the manga list.
    pub fn set_manga_enabled(&self, on: bool) -> Result<(), KotoraError> {
        Ok(self.store().set_manga_enabled(on)?)
    }

    /// The active list of `kind`, one status or all, sorted. Local: no network.
    pub fn list(&self, kind: MediaKind, status: Option<ListStatus>, sort: Sort) -> Result<Vec<Row>, KotoraError> {
        let service = self.active_or_err()?;
        let store = self.store();
        let mut rows = self.rows(&store, service, kind.into())?;
        if let Some(st) = status {
            rows.retain(|r| shows_in(r.status, r.rewatching, st));
        }
        match sort {
            Sort::Updated => rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then_with(|| a.title.cmp(&b.title))),
            Sort::Title => rows.sort_by_key(|r| r.title.to_lowercase()),
            Sort::Score => {
                let points = |r: &Row| r.score.as_deref().and_then(|s| s.parse::<f32>().ok()).unwrap_or(-1.0);
                rows.sort_by(|a, b| points(b).total_cmp(&points(a)).then_with(|| a.title.cmp(&b.title)));
            }
        }
        Ok(rows)
    }

    /// How many entries each status has (all five, in sidebar order).
    pub fn counts(&self, kind: MediaKind) -> Result<Vec<StatusCount>, KotoraError> {
        let service = self.active_or_err()?;
        let store = self.store();
        let list = store.load_list(Scope::new(service.name(), kind.into()))?;
        Ok(ListStatus::ALL.into_iter().map(|st| StatusCount { status: st, count: list.iter().filter(|i| shows_in(i.status.into(), i.rewatching, st)).count() as u32 }).collect())
    }

    pub fn entry(&self, kind: MediaKind, id: u32) -> Result<Option<Row>, KotoraError> {
        let service = self.active_or_err()?;
        let store = self.store();
        let fmt = self.score_format(&store, service);
        let waiting = waiting_ids(&store, service, kind.into()).contains(&id);
        Ok(store.load_item(Scope::new(service.name(), kind.into()), id)?.map(|it| row(&it, fmt, waiting)))
    }

    /// When the list of `kind` was last fetched (unix seconds).
    pub fn synced_at(&self, kind: MediaKind) -> Option<i64> {
        let service = self.active()?;
        self.store().synced_at(Scope::new(service.name(), kind.into())).ok().flatten()
    }

    /// Changes waiting to be sent, both kinds, all services.
    pub fn waiting_count(&self) -> u32 {
        let store = self.store();
        Service::ALL.into_iter().map(|s| store.open_count(s.name()).unwrap_or(0)).sum()
    }

    /// +1 episode (chapter): saved at once, queued for the service. The last one completes the
    /// entry, or ends a rewatch, as on MyAnimeList's website.
    pub fn plus_one(&self, kind: MediaKind, id: u32) -> Result<Edit, KotoraError> {
        let service = self.active_or_err()?;
        let k: Kind = kind.into();
        let mut store = self.store();
        let scope = Scope::new(service.name(), k);
        let item = store.load_item(scope, id)?.ok_or(KotoraError::NotFound)?;
        let total = item.media.episodes.filter(|n| *n > 0);
        if total.is_some_and(|n| item.progress >= n) {
            return Err(other(format!("{} is at its last {}.", item.media.title, k.unit())));
        }
        let before = item.values();
        let change = Change { progress: Some(item.progress + 1), ..Default::default() }.completes(&before, total);
        store.enqueue(scope, id, change, time::now_unix())?;
        let mut message = format!("{}: {} {} saved", item.media.title, k.unit(), item.progress + 1);
        if change.status == Some(Status::Completed) && before.status != Status::Completed {
            message.push_str(", completed");
        }
        Ok(Edit {
            kind,
            id,
            message,
            before_status: before.status.into(),
            before_progress: before.progress,
            before_rewatching: before.rewatching,
            before_times_rewatched: before.times_rewatched,
            before_volumes: before.volumes,
        })
    }

    /// Puts the values from before `edit` back (queued too; an edit not sent yet simply goes).
    pub fn undo(&self, edit: Edit) -> Result<(), KotoraError> {
        let service = self.active_or_err()?;
        let mut store = self.store();
        let scope = Scope::new(service.name(), edit.kind.into());
        let item = store.load_item(scope, edit.id)?.ok_or(KotoraError::NotFound)?;
        let back = Change {
            status: Some(edit.before_status.into()),
            progress: Some(edit.before_progress),
            rewatching: Some(edit.before_rewatching),
            times_rewatched: Some(edit.before_times_rewatched),
            volumes: (edit.kind == MediaKind::Manga).then_some(edit.before_volumes),
            ..Default::default()
        }
        .without_same(&item.values());
        if !back.is_empty() {
            store.enqueue(scope, edit.id, back, time::now_unix())?;
        }
        Ok(())
    }

    /// Fetches the active service's list (and the manga list in Manga mode) into the store.
    /// Network: call it off the main thread.
    pub fn sync(&self) -> Result<SyncReport, KotoraError> {
        let service = self.active_or_err()?;
        let mut svc = self.service(service)?;
        let mut store = self.own_store()?;
        let now = time::now_unix();
        let items = svc.fetch_list()?;
        let name = service.name();
        let old = store.load_list(name).unwrap_or_default();
        store.replace_list(name, &items, now)?;
        // Website edits go to the mirror, if it is on (off by default on phones).
        let _ = kotora_core::mirror::after_main_sync(&mut store, name, &old, now);
        kotora_core::sync::complete_media(svc.as_mut(), &store, None, now);
        if let Ok(user) = svc.user() {
            let _ = store.set_account(name, &user.name);
            let _ = store.set_picture(name, user.picture.as_deref(), now);
            let _ = store.set_profile(name, &user.profile);
        }
        let manga = match kotora_core::sync::sync_manga(svc.as_mut(), &mut store, now) {
            kotora_core::sync::MangaSync::Off => None,
            kotora_core::sync::MangaSync::Synced { items, .. } => Some(items.len() as u32),
            kotora_core::sync::MangaSync::Failed(e) => return Err(e.into()),
            kotora_core::sync::MangaSync::NotSaved(m) => return Err(other(m)),
        };
        Ok(SyncReport { service, anime: store.load_list(name)?.len() as u32, manga, synced_at: now })
    }

    /// Sends the waiting changes of every signed-in service (at most a few dozen per run).
    /// Network: call it off the main thread (a worker).
    pub fn flush_queue(&self) -> Result<FlushReport, KotoraError> {
        let mut report = FlushReport::default();
        let mut store = self.own_store()?;
        for service in Service::ALL.into_iter().filter(|s| self.signed_in(*s)) {
            if store.open_count(service.name())? == 0 {
                continue;
            }
            let mut svc = self.service(service)?;
            for _ in 0..FLUSH_MAX {
                match queue::step(&mut store, svc.as_mut(), time::now_unix())? {
                    Step::Idle => break,
                    Step::Wait(s) => {
                        report.retry_in = Some(report.retry_in.map_or(s, |r| r.min(s)));
                        break;
                    }
                    Step::SignInAgain => {
                        report.sign_in_again = true;
                        break;
                    }
                    Step::Handled { outcome, .. } => match outcome {
                        Outcome::Sent | Outcome::AlreadyThere => report.sent += 1,
                        Outcome::Conflict => report.conflicts += 1,
                        Outcome::Failed(m) => {
                            report.failed += 1;
                            self.log(LogLevel::Warn, format!("a change failed: {m}"));
                        }
                        Outcome::Retry(_) => {}
                    },
                }
            }
        }
        report.waiting = Service::ALL.into_iter().map(|s| store.open_count(s.name()).unwrap_or(0)).sum();
        Ok(report)
    }
}

/// The redirect URIs, for the manifest check in tests and the About page.
#[uniffi::export]
pub fn mal_redirect_uri() -> String {
    MAL_REDIRECT_URI.into()
}

#[uniffi::export]
pub fn anilist_redirect_uri() -> String {
    ANILIST_REDIRECT_URI.into()
}

/// The core's version (the one version of desktop and Android).
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

mod search;
pub use search::Hit;
/// Airing, Publishing and their notifications.
mod pages;
/// Title language and release sources (Android; the desktop's keys).
mod prefs;
/// The season picker's other seasons (Android).
mod seasons;
pub use seasons::{SeasonGrid, SeasonPick};
pub use prefs::{ReleaseSources, ShownTitle};

#[cfg(test)]
mod tests;

/// What the unit tests share. The folders and the offset source are set once per process (the
/// first `Kotora::new` wins), so every fake platform must report the same zone, and a test that
/// reads or writes `config.toml` takes [`testenv::fresh_config`] to have it to itself and empty.
#[cfg(test)]
pub(crate) mod testenv {
    /// The zone every fake platform reports: JST, as the desktop's airing tests.
    pub const OFFSET: i32 = 9 * 3600;

    static CONFIG: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Call after opening a `Kotora` (that sets the folders). Holds the lock until dropped.
    pub fn fresh_config() -> std::sync::MutexGuard<'static, ()> {
        let guard = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
        let _ = std::fs::remove_file(kotora_core::paths::config_file());
        guard
    }
}

mod links;
mod profile;
mod listopts;

/// The background refresh (queue + list), and the activity-log check.
mod background;
pub use background::*;
/// The details page's service information and the add of a title not on the list.
mod details;
pub use details::{InfoFact, InfoStat, PastedId, ServiceDetails, TitleStub};
