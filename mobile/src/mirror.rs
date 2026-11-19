//! Mirror edits, as on the desktop (`app/src/mirror.rs`; the work is core's
//! `kotora_core::mirror`). One main service (the active one) and the other as its mirror,
//! matched by MyAnimeList id.
//!
//! - Settings → "Mirror edits to {other}": the switch. Turning it on reads the other service's
//!   list ([`Kotora::mirror_compare`], network) and returns a [`MirrorPreview`] with the three
//!   choices; nothing is saved or sent until one is picked ([`Kotora::mirror_choose`]). Turning
//!   it off drops the mirror's changes that still wait ([`Kotora::mirror_stop`]).
//! - Every edit the phone makes on the main list is queued for the mirror by the core's queue
//!   (inside the same transaction). [`Kotora::mirror_refresh`] does the mirror worker's part:
//!   look up AniList ids of new adds, send the mirror's queue, and (after a main sync) read the
//!   mirror's list to find edits made on its website. The phones call it from the work they
//!   already run (the foreground flush and sync, and [`Kotora::background_refresh`] from the one
//!   periodic task): no scheduler of its own.
//! - The Sync queue page shows the mirror's part ([`Kotora::mirror_queue`]): website edits to
//!   settle, conflicts, what waits, failed or went today, and adds still looking up their id.
//!
//! The words are the desktop's, built here so both phones say exactly the same.

use std::collections::HashSet;

use kotora_core::listxml::{Action, Verdict};
use kotora_core::mirror::{self, Align, Link, Preview, Settle, WebEdit};
use kotora_core::queue::{Op, OpState};
use kotora_core::{Media, Values};

use super::*;
use crate::links::{ServiceLink, media_url};

/// The mirror service's last problem (offline, sign in again), in plain words; `kv`, so a
/// background run's trouble shows when the app opens.
const TROUBLE_KEY: &str = "mobile.mirror.trouble";
/// Status conflicts listed one by one; the rest go by "All …" (desktop `CONFLICTS_SHOWN`).
const CONFLICTS_SHOWN: usize = 8;

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// "just now", "3 min ago", "2 h ago", "4 days ago" (desktop `ui::sync_age`).
fn sync_age(secs: i64) -> String {
    match secs.max(0) {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// An entry's values in words; a manga's: Reading, chapter and volume, rereading.
fn values_text(kind: Kind, v: &Values, fmt: ScoreFormat) -> String {
    let manga = kind == Kind::Manga;
    let mut t = v.status.label_in(kind).to_string();
    if v.rewatching {
        t.push_str(if manga { ", rereading" } else { ", rewatching" });
    }
    if v.progress > 0 {
        t.push_str(&format!(", {} {}", kind.unit(), v.progress));
    }
    if manga && v.volumes > 0 {
        t.push_str(&format!(", volume {}", v.volumes));
    }
    if let Some(sc) = v.score {
        t.push_str(&format!(", score {}", fmt.words(sc)));
    }
    t
}

fn saved_text(kind: Kind, v: Option<&mirror::Saved>, removed: &str, fmt: ScoreFormat) -> String {
    v.map_or_else(|| removed.to_string(), |v| values_text(kind, &v.values(), fmt))
}

/// The desktop's trouble line for the mirror service.
fn trouble_text(to: ServiceId, e: &ServiceError) -> String {
    match e {
        ServiceError::SignInAgain => format!("Sign in to {} again: the mirror waits until then.", to.title()),
        ServiceError::Offline(_) => format!("Can\u{2019}t reach {}. Its changes wait and go out when it answers.", to.title()),
        ServiceError::RateLimited => format!("{} asked Kotora to slow down. Its changes go out in a minute.", to.title()),
        ServiceError::Other(m) => format!("{}: {m}", to.title()),
    }
}

fn service_of(id: ServiceId) -> Service {
    if id == ServiceId::Mal { Service::Mal } else { Service::AniList }
}

// -------------------------------------------------------------------------------- records

/// How the preview's choice aligns the lists.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MirrorAlign {
    /// "{main} wins": the mirror becomes a copy of the main list.
    MainWins,
    Merge,
    DontAlign,
}

impl From<MirrorAlign> for Align {
    fn from(a: MirrorAlign) -> Align {
        match a {
            MirrorAlign::MainWins => Align::MainWins,
            MirrorAlign::Merge => Align::Merge,
            MirrorAlign::DontAlign => Align::DontAlign,
        }
    }
}

/// How a website edit is settled.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MirrorSettle {
    UseMain,
    UseMirror,
    Leave,
}

impl From<MirrorSettle> for Settle {
    fn from(s: MirrorSettle) -> Settle {
        match s {
            MirrorSettle::UseMain => Settle::UseMain,
            MirrorSettle::UseMirror => Settle::UseMirror,
            MirrorSettle::Leave => Settle::Leave,
        }
    }
}

/// "Align manga lists": Manga turned on after the mirror.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MirrorAlignManga {
    pub title: String,
    pub note: String,
    /// "Compare…".
    pub button: String,
}

/// Settings' mirror row.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MirrorState {
    pub main: Service,
    pub to: Service,
    pub on: bool,
    /// "Mirror edits to AniList".
    pub title: String,
    /// The line under the title (what it does, why it can't be turned on, or its state).
    pub note: String,
    /// The switch can be turned on now (both services signed in).
    pub can_turn_on: bool,
    pub other_signed_in: bool,
    pub align_manga: Option<MirrorAlignManga>,
    /// Changes still waiting for the mirror.
    pub waiting: u32,
    /// Website edits, conflicts and failures (what needs the user in the Queue).
    pub attention: u32,
    /// The mirror's part of the Sync queue badge (0 while off).
    pub badge: u32,
    pub trouble: Option<String>,
    /// The account line: "MyAnimeList ⇄ AniList" with the mirror on, else the service.
    pub account_line: String,
    /// "Changes also go to AniList. 2 in the Sync queue for AniList." with the mirror on.
    pub account_tip: Option<String>,
    /// While comparing: "Reading your AniList list…" (and the manga one).
    pub reading: String,
    pub reading_manga: String,
    /// "Saving…" / "Turning the mirror off…".
    pub saving: String,
    pub turning_off: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MirrorCount {
    /// "only-main", "only-mirror", "different", "same", "unmatched" (the colour).
    pub key: String,
    pub count: u32,
    /// "only on MyAnimeList".
    pub label: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MirrorGroup {
    /// "only-main", "only-mirror", "different", "unmatched".
    pub key: String,
    pub title: String,
    pub lines: Vec<String>,
}

/// A status conflict to pick (with "Merge").
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MirrorPick {
    pub mal_id: u32,
    pub title: String,
    /// "MyAnimeList: Watching".
    pub main_label: String,
    /// "AniList: Completed".
    pub mirror_label: String,
    /// The mirror's status is picked.
    pub mirror_picked: bool,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MirrorPart {
    pub kind: MediaKind,
    /// "MANGA LISTS" when both kinds are shown.
    pub heading: Option<String>,
    /// "Your MyAnimeList list and your AniList list, matched by MyAnimeList id".
    pub title: String,
    pub counts: Vec<MirrorCount>,
    pub groups: Vec<MirrorGroup>,
    /// "STATUS TO PICK (WITH “MERGE”)", when there are conflicts.
    pub picks_heading: Option<String>,
    pub picks: Vec<MirrorPick>,
    /// More than one conflict: "and 3 more. All of them:" / "All of them:".
    pub all_line: Option<String>,
    pub all_main_label: String,
    pub all_mirror_label: String,
    pub all_main_on: bool,
    pub all_mirror_on: bool,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MirrorPreviewView {
    pub main: Service,
    pub to: Service,
    pub parts: Vec<MirrorPart>,
    /// What each choice does (the last line is the faint footnote).
    pub explain: Vec<String>,
    /// "MyAnimeList wins".
    pub main_wins: String,
    pub merge: String,
    pub dont_align: String,
    pub cancel: String,
}

/// A website edit to settle.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MirrorEditCard {
    pub kind: MediaKind,
    pub mal_id: u32,
    /// "AniList: Hoshikuzu Relay".
    pub title: String,
    pub why: String,
    pub main_head: String,
    pub main_text: String,
    pub use_main: Option<String>,
    pub mirror_head: String,
    pub mirror_text: String,
    pub use_mirror: Option<String>,
    /// "Leave both as they are".
    pub leave: String,
}

/// A mirrored change whose entry changed on the mirror's website.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MirrorConflictCard {
    pub id: i64,
    pub title: String,
    pub why: String,
    pub mine_head: String,
    pub mine_text: String,
    /// "Send MyAnimeList’s" / "Remove anyway" (keep mine).
    pub keep: String,
    pub theirs_head: String,
    pub theirs_text: String,
    /// "Keep AniList’s" / "Leave it removed" / "Keep it on AniList" (take theirs).
    pub take: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MirrorOpRow {
    pub id: i64,
    pub title: String,
    /// "Progress 3 → 5".
    pub what: String,
    /// The error (failed), or "…; Kotora tries again by itself" (a retry).
    pub note: Option<String>,
    pub note_is_error: bool,
    /// "Waiting", "Sending", "Failed", "Sent".
    pub state: String,
    /// Failed: a "Discard" button.
    pub can_discard: bool,
    pub sent: bool,
    /// "now", "2 h".
    pub age: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MirrorOpGroup {
    /// "waiting", "failed", "sent".
    pub key: String,
    /// "WAITING", "FAILED", "SENT TODAY".
    pub label: String,
    pub rows: Vec<MirrorOpRow>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MirrorWaitRow {
    pub kind: MediaKind,
    pub mal_id: u32,
    pub title: String,
    /// "Looking up its AniList id" / "Not on AniList".
    pub why: String,
}

/// The Sync queue page's mirror part.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct MirrorQueue {
    /// "AniList mirror".
    pub heading: String,
    /// "read 3 min ago".
    pub read: Option<String>,
    pub trouble: Option<String>,
    pub edits: Vec<MirrorEditCard>,
    pub conflicts: Vec<MirrorConflictCard>,
    pub groups: Vec<MirrorOpGroup>,
    /// "NOT ON ANILIST YET".
    pub waits_heading: String,
    pub waits: Vec<MirrorWaitRow>,
}

/// What one [`Kotora::mirror_refresh`] did.
#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct MirrorRun {
    pub sent: u32,
    /// The mirror's list was read (website edits looked for).
    pub read: bool,
    pub trouble: Option<String>,
}

// ------------------------------------------------------------------------------- preview

/// The two lists compared, waiting for the user's pick (desktop `Prev`).
struct Part {
    p: Preview,
    fetched: Vec<ListItem>,
    mirror_status: HashSet<u32>,
}

struct Prev {
    parts: Vec<Part>,
    /// The mirror is on already: the manga lists are aligned now, nothing else.
    extend: bool,
}

impl Prev {
    fn link(&self) -> Link {
        self.parts[0].p.link
    }

    fn actions(&self, align: Align) -> (Vec<Action>, Vec<Action>) {
        let (mut to, mut main) = (Vec::new(), Vec::new());
        for part in &self.parts {
            let (t, m) = mirror::align_actions(&part.p, align, &part.mirror_status);
            to.extend(t);
            main.extend(m);
        }
        (to, main)
    }
}

/// The lists compared. Read it with [`MirrorPreview::view`], pick statuses, then hand it to
/// [`Kotora::mirror_choose`].
#[derive(uniffi::Object)]
pub struct MirrorPreview {
    inner: Mutex<Prev>,
    fmts: (ScoreFormat, ScoreFormat),
}

impl MirrorPreview {
    fn lock(&self) -> MutexGuard<'_, Prev> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[uniffi::export]
impl MirrorPreview {
    pub fn view(&self) -> MirrorPreviewView {
        let prev = self.lock();
        let link = prev.link();
        let (main, to) = (link.main.title(), link.to.title());
        let both = prev.parts.len() > 1;
        let parts = prev.parts.iter().map(|p| part_view(p, both, self.fmts)).collect();
        let n = |a: Align| {
            let (t, m) = prev.actions(a);
            (t.len(), m.len())
        };
        let (wins_to, _) = n(Align::MainWins);
        let (merge_to, merge_main) = n(Align::Merge);
        let what = match (prev.parts.len(), prev.parts[0].p.kind) {
            (1, Kind::Manga) => "Manga",
            (1, _) => "Anime",
            _ => "Entries",
        };
        let explain = vec![
            format!("{main} wins: {to} becomes a copy of your {main} list ({}). {what} only on {to} stay there.", plural(wins_to, "change", "changes")),
            format!("Merge: what is missing on each side is added; where they differ, the one further along wins and an empty score is filled ({} for {to}, {} for {main}).", plural(merge_to, "change", "changes"), plural(merge_main, "change", "changes")),
            "Don\u{2019}t align: nothing changes now; only edits you make from now on are mirrored.".to_string(),
            "Nothing is removed from either list, and every change goes through the Queue at each service\u{2019}s pace.".to_string(),
        ];
        MirrorPreviewView {
            main: service_of(link.main),
            to: service_of(link.to),
            parts,
            explain,
            main_wins: format!("{main} wins"),
            merge: "Merge".into(),
            dont_align: "Don\u{2019}t align".into(),
            cancel: "Cancel".into(),
        }
    }

    /// One status conflict: the mirror's status (`true`) or the main list's.
    pub fn pick_status(&self, kind: MediaKind, mal_id: u32, mirror_status: bool) {
        let mut prev = self.lock();
        if let Some(p) = prev.parts.iter_mut().find(|p| p.p.kind == Kind::from(kind)) {
            if mirror_status {
                p.mirror_status.insert(mal_id);
            } else {
                p.mirror_status.remove(&mal_id);
            }
        }
    }

    /// "All MyAnimeList’s" / "All AniList’s".
    pub fn pick_all(&self, kind: MediaKind, mirror_status: bool) {
        let mut prev = self.lock();
        if let Some(p) = prev.parts.iter_mut().find(|p| p.p.kind == Kind::from(kind)) {
            p.mirror_status = if mirror_status { p.p.merge.iter().filter(|r| r.conflict.is_some()).map(|r| r.mal_id).collect() } else { HashSet::new() };
        }
    }
}

fn part_view(part: &Part, heading: bool, (fm, ft): (ScoreFormat, ScoreFormat)) -> MirrorPart {
    let p = &part.p;
    let kind = p.kind;
    let (main, to) = (p.link.main.title(), p.link.to.title());
    let c = p.counts();
    let vt = |v: &Values, f: ScoreFormat| values_text(kind, v, f);
    let what = if kind == Kind::Manga { "manga list" } else { "list" };
    let count = |key: &str, n: usize, label: String| MirrorCount { key: key.into(), count: n as u32, label };
    let counts = vec![
        count("only-main", c.only_main, format!("only on {main}")),
        count("only-mirror", c.only_mirror, format!("only on {to}")),
        count("different", c.different, "different".into()),
        count("same", c.same, "the same".into()),
        count("unmatched", c.unmatched, "not mirrored".into()),
    ];
    let mut groups = Vec::new();
    let mut group = |key: &str, title: String, lines: Vec<String>| {
        if !lines.is_empty() {
            groups.push(MirrorGroup { key: key.into(), title, lines });
        }
    };
    group("only-main", format!("Only on {main}"), p.plan.rows.iter().filter(|r| r.verdict == Verdict::New).map(|r| format!("{}: {}", r.title(), r.entry.values().map(|v| vt(&v, fm)).unwrap_or_default())).collect());
    group("only-mirror", format!("Only on {to}"), p.only_mirror.iter().map(|o| format!("{}: {}", o.item.media.title, vt(&o.item.values(), ft))).collect());
    group("different", "On both, different".into(), p.merge.iter().map(|r| format!("{}: {main} {} \u{b7} {to} {}", r.title, vt(&r.main, fm), vt(&r.mirror, ft))).collect());
    let mut unmatched: Vec<String> = p
        .plan
        .rows
        .iter()
        .filter_map(|r| match &r.verdict {
            Verdict::Unknown(why) => Some(format!("{}: {why}", r.entry.title)),
            _ => None,
        })
        .collect();
    unmatched.extend(p.unmatched_mirror.iter().map(|t| format!("{t}: on {to}, no MyAnimeList id")));
    unmatched.extend(p.only_mirror.iter().filter(|o| o.main_media.is_none()).map(|o| format!("{}: not on {main}, can\u{2019}t be added there", o.item.media.title)));
    group("unmatched", "Not mirrored".into(), unmatched);
    let conflicts: Vec<_> = p.merge.iter().filter(|r| r.conflict.is_some()).collect();
    let n = conflicts.len();
    let picks = conflicts
        .iter()
        .take(CONFLICTS_SHOWN)
        .filter_map(|r| {
            let c = r.conflict?;
            Some(MirrorPick {
                mal_id: r.mal_id,
                title: r.title.clone(),
                main_label: format!("{main}: {}", c.mine.label_in(kind)),
                mirror_label: format!("{to}: {}", c.file.label_in(kind)),
                mirror_picked: part.mirror_status.contains(&r.mal_id),
            })
        })
        .collect();
    let all_line = (n > 1).then(|| {
        let more = if n > CONFLICTS_SHOWN { format!("and {} more. ", n - CONFLICTS_SHOWN) } else { String::new() };
        format!("{more}All of them:")
    });
    MirrorPart {
        kind: kind.into(),
        heading: heading.then(|| format!("{} lists", kind.label()).to_uppercase()),
        title: format!("Your {main} {what} and your {to} {what}, matched by MyAnimeList id"),
        counts,
        groups,
        picks_heading: (n > 0).then(|| "STATUS TO PICK (WITH \u{201c}MERGE\u{201d})".to_string()),
        picks,
        all_line,
        all_main_label: format!("All {main}\u{2019}s"),
        all_mirror_label: format!("All {to}\u{2019}s"),
        all_main_on: part.mirror_status.is_empty(),
        all_mirror_on: n > 0 && conflicts.iter().all(|r| part.mirror_status.contains(&r.mal_id)),
    }
}

// ---------------------------------------------------------------------------------- Kotora

impl Kotora {
    /// The saved link, when it is this active list's. A link of the other direction (made
    /// before a switch of services) goes, as the desktop's switch drops it.
    fn mirror_link_now(&self, store: &Store) -> Option<Link> {
        let link = store.mirror_link().ok().flatten()?;
        // The active service read from `store` itself: `self.active()` would lock it again.
        let saved = store.kv_get(ACTIVE_KEY).ok().flatten().and_then(|s| Service::from_name(&s));
        let active = saved.filter(|s| self.signed_in(*s)).or_else(|| Service::ALL.into_iter().find(|s| self.signed_in(*s)));
        match active {
            Some(a) if a.id() == link.main => Some(link),
            _ => {
                let _ = store.unlink_mirror();
                let _ = store.kv_del(TROUBLE_KEY);
                None
            }
        }
    }

    fn fmt_of(&self, store: &Store, id: ServiceId) -> ScoreFormat {
        self.score_format(store, service_of(id))
    }

    fn set_trouble(store: &Store, t: Option<&str>) {
        let _ = match t {
            Some(t) => store.kv_set(TROUBLE_KEY, t),
            None => store.kv_del(TROUBLE_KEY),
        };
    }

    /// Reads the mirror's list of `kind` and compares it with the saved main list.
    fn compare_kind(&self, link: Link, kind: Kind) -> Result<Part, KotoraError> {
        let open = |id: ServiceId| self.service(service_of(id)).map_err(|_| other(format!("Sign in to {} first, and check you are online.", id.title())));
        let mut to = open(link.to)?;
        let fetched = to.fetch_list_of(kind).map_err(|e| other(trouble_text(link.to, &e)))?;
        let main = self.own_store()?.load_list((link.main.name(), kind)).map_err(|e| other(format!("Can\u{2019}t read the saved list: {e}")))?;
        let lookup = |svc: &mut dyn ListService, ids: Vec<u32>| -> Result<HashMap<u32, Media>, KotoraError> {
            if ids.is_empty() {
                return Ok(HashMap::new());
            }
            let found = svc.media_by_mal_ids_of(kind, &ids).map_err(|e| other(format!("Can\u{2019}t look the ids up on {}: {e}", svc.title())))?;
            Ok(found.into_iter().filter_map(|m| Some((m.id_mal?, m))).collect())
        };
        let mirror_media = if link.to == ServiceId::AniList { lookup(to.as_mut(), mirror::ids_only_on_main_of(link, kind, &main, &fetched))? } else { HashMap::new() };
        let main_media = if link.main == ServiceId::AniList { lookup(open(link.main)?.as_mut(), mirror::ids_only_on_mirror(link, &main, &fetched))? } else { HashMap::new() };
        Ok(Part { p: mirror::compare_of(link, kind, &main, &fetched, &mirror_media, &main_media), fetched, mirror_status: HashSet::new() })
    }

    fn preview_of(&self, parts: Vec<Part>, extend: bool) -> Arc<MirrorPreview> {
        let link = parts[0].p.link;
        let store = self.store();
        let fmts = (self.fmt_of(&store, link.main), self.fmt_of(&store, link.to));
        Arc::new(MirrorPreview { inner: Mutex::new(Prev { parts, extend }), fmts })
    }
}

use std::collections::HashMap;

#[uniffi::export]
impl Kotora {
    /// Settings' mirror row. Local.
    pub fn mirror_state(&self) -> Option<MirrorState> {
        let active = self.active()?;
        let store = self.store();
        let link_on = self.mirror_link_now(&store);
        let link = Link::of(active.id());
        let (main, to) = (link.main.title(), link.to.title());
        let other_signed_in = self.signed_in(service_of(link.to));
        let now = time::now_unix();
        let ops = link_on.map(|l| store.queue(l.to.name(), now).unwrap_or_default()).unwrap_or_default();
        let edits = link_on.map(|_| store.web_edits().unwrap_or_default()).unwrap_or_default();
        let waits = link_on.map(|_| store.mirror_waits().unwrap_or_default()).unwrap_or_default();
        let waiting = ops.iter().filter(|o| o.state.is_open()).count();
        let attention = edits.len() + ops.iter().filter(|o| matches!(o.state, OpState::Conflict | OpState::Failed)).count();
        let badge = if link_on.is_some() { edits.len() + waiting + waits.iter().filter(|w| w.error.is_none()).count() } else { 0 };
        let trouble = link_on.and_then(|_| store.kv_get(TROUBLE_KEY).ok().flatten());
        let note = match link_on {
            Some(_) => {
                let mut parts = vec![format!("On. Every change you make here also goes to {to}. Changes made on its website are never overwritten: they show in the Queue.")];
                if waiting > 0 {
                    parts.push(format!("{} waiting for {to}.", plural(waiting, "change", "changes")));
                }
                if attention > 0 {
                    parts.push(format!("{} to look at in the Queue.", plural(attention, "thing", "things")));
                }
                if let Some(t) = &trouble {
                    parts.push(t.clone());
                }
                parts.join(" ")
            }
            None if !other_signed_in => format!("Sign in to {to} too: choose Use {to} above, sign in, then come back to {main}. Your {main} list stays the main one."),
            None => format!("Every change you make in Kotora also goes to your {to} list, through its own queue. Turning it on first compares the two lists and shows you what would change."),
        };
        let manga_on = store.manga_enabled().unwrap_or(false);
        let align_manga = (link_on.is_some() && manga_on && !store.mirror_manga_aligned().unwrap_or(false)).then(|| MirrorAlignManga {
            title: "Align manga lists".into(),
            note: "Manga edits are mirrored too. Your two manga lists weren\u{2019}t compared yet: align them once, with the same choices.".into(),
            button: "Compare\u{2026}".into(),
        });
        let account_tip = link_on.map(|_| {
            let mut t = format!("Changes also go to {to}.");
            if badge > 0 {
                t.push_str(&format!(" {badge} in the Sync queue for {to}."));
            }
            t
        });
        Some(MirrorState {
            main: active,
            to: service_of(link.to),
            on: link_on.is_some(),
            title: format!("Mirror edits to {to}"),
            note,
            can_turn_on: other_signed_in,
            other_signed_in,
            align_manga,
            waiting: waiting as u32,
            attention: attention as u32,
            badge: badge as u32,
            trouble,
            account_line: if link_on.is_some() { format!("{main} \u{21c4} {to}") } else { main.to_string() },
            account_tip,
            reading: format!("Reading your {to} list\u{2026}"),
            reading_manga: format!("Reading your {to} manga list\u{2026}"),
            saving: "Saving\u{2026}".into(),
            turning_off: "Turning the mirror off\u{2026}".into(),
        })
    }

    /// With the mirror on: the other service's name ("Also removes it from AniList.").
    pub fn mirror_target(&self) -> Option<String> {
        let store = self.store();
        self.mirror_link_now(&store).map(|l| l.to.title().to_string())
    }

    /// Turning it on: reads the other service's anime list (and its manga list in Manga mode)
    /// and compares. Nothing is saved yet. Network: off the main thread.
    pub fn mirror_compare(&self) -> Result<Arc<MirrorPreview>, KotoraError> {
        let active = self.active_or_err()?;
        let link = Link::of(active.id());
        let mut parts = vec![self.compare_kind(link, Kind::Anime)?];
        if self.manga_enabled() {
            parts.push(self.compare_kind(link, Kind::Manga)?);
        }
        Ok(self.preview_of(parts, false))
    }

    /// "Align manga lists" → Compare…: the manga lists of a mirror that is on. Network.
    pub fn mirror_compare_manga(&self) -> Result<Arc<MirrorPreview>, KotoraError> {
        let link = self.mirror_link_now(&self.store()).ok_or_else(|| other("The mirror is off."))?;
        Ok(self.preview_of(vec![self.compare_kind(link, Kind::Manga)?], true))
    }

    /// A choice in the preview: the mirror's lists as read are saved, the alignment queued, the
    /// link saved. Returns the note ("On. Queued 3 changes for AniList."). Local; then flush.
    pub fn mirror_choose(&self, preview: Arc<MirrorPreview>, align: MirrorAlign) -> Result<String, KotoraError> {
        let prev = preview.lock();
        let link = prev.link();
        let (to, main) = prev.actions(align.into());
        let with_manga = prev.parts.iter().any(|p| p.p.kind == Kind::Manga);
        let r = (|| {
            let mut store = self.store();
            let now = time::now_unix();
            for part in &prev.parts {
                store.replace_list((link.to.name(), part.p.kind), &part.fetched, now)?;
            }
            if prev.extend {
                return store.align_mirror(&to, &main, now);
            }
            let r = store.start_mirror(link, &to, &main, now)?;
            if with_manga {
                store.set_mirror_manga_aligned()?;
            }
            Ok(r)
        })();
        match r {
            Ok((a, b)) => Ok(match (a.len(), b.len()) {
                (0, 0) => format!("On. Edits you make here from now on also go to {}.", link.to.title()),
                (a, 0) => format!("On. Queued {} for {}.", plural(a, "change", "changes"), link.to.title()),
                (a, b) => format!("On. Queued {} for {} and {} for {}.", plural(a, "change", "changes"), link.to.title(), plural(b, "change", "changes"), link.main.title()),
            }),
            Err(e) => Err(other(format!("The mirror can\u{2019}t be turned on: {e}"))),
        }
    }

    /// Off: the link goes and the mirror's waiting changes are dropped. Returns the note.
    pub fn mirror_stop(&self) -> Result<String, KotoraError> {
        let mut store = self.store();
        let Some(link) = store.mirror_link().ok().flatten() else { return Ok("Off. Nothing more goes to the mirror.".into()) };
        let n = store.stop_mirror().map_err(|e| other(format!("The mirror can\u{2019}t be turned off: {e}")))?;
        let _ = store.kv_del(TROUBLE_KEY);
        Ok(match n {
            0 => "Off. Nothing more goes to the mirror.".to_string(),
            n => format!("Off. {} for {} that hadn\u{2019}t been sent {} dropped.", plural(n, "change", "changes"), link.to.title(), if n == 1 { "was" } else { "were" }),
        })
    }

    /// The Sync queue page's mirror part; `None` while off or with nothing in it. Local.
    pub fn mirror_queue(&self) -> Option<MirrorQueue> {
        let store = self.store();
        let link = self.mirror_link_now(&store)?;
        let (main, to) = (link.main.title(), link.to.title());
        let (fm, ft) = (self.fmt_of(&store, link.main), self.fmt_of(&store, link.to));
        let now = time::now_unix();
        let ops = store.queue(link.to.name(), now).unwrap_or_default();
        let edits = store.web_edits().unwrap_or_default();
        let waits = store.mirror_waits().unwrap_or_default();
        if ops.is_empty() && edits.is_empty() && waits.is_empty() {
            return None;
        }
        let synced_at = Kind::ALL.into_iter().filter_map(|k| store.synced_at((link.to.name(), k)).ok().flatten()).max();
        let groups = [("waiting", "WAITING", (|st| matches!(st, OpState::Queued | OpState::Sending)) as fn(OpState) -> bool), ("failed", "FAILED", |st| st == OpState::Failed), ("sent", "SENT TODAY", |st| st == OpState::Done)]
            .into_iter()
            .map(|(key, label, keep)| MirrorOpGroup { key: key.into(), label: label.into(), rows: ops.iter().filter(|o| keep(o.state)).map(|o| op_row(o, now, ft)).collect() })
            .filter(|g| !g.rows.is_empty())
            .collect();
        Some(MirrorQueue {
            heading: format!("{to} mirror"),
            read: synced_at.map(|at| format!("read {}", sync_age(now - at))),
            trouble: store.kv_get(TROUBLE_KEY).ok().flatten(),
            edits: edits.iter().map(|e| edit_card(e, main, to, now, (fm, ft))).collect(),
            conflicts: ops.iter().filter(|o| o.state == OpState::Conflict).map(|o| conflict_card(o, main, to, (fm, ft))).collect(),
            groups,
            waits_heading: format!("NOT ON {} YET", to.to_uppercase()),
            waits: waits.iter().map(|w| MirrorWaitRow { kind: w.kind.into(), mal_id: w.mal_id, title: w.title.clone(), why: w.error.clone().unwrap_or_else(|| format!("Looking up its {to} id")) }).collect(),
        })
    }

    /// Settles a website edit. "Add to AniList" (an AniList main list) looks the media up
    /// first: network then, so call it off the main thread.
    pub fn mirror_settle(&self, kind: MediaKind, mal_id: u32, how: MirrorSettle) -> Result<(), KotoraError> {
        let link = self.mirror_link_now(&self.store()).ok_or_else(|| other("The mirror is off."))?;
        let k: Kind = kind.into();
        let mut media = None;
        let missing = self.store().web_edits().unwrap_or_default().iter().any(|e| e.kind == k && e.mal_id == mal_id && e.main_id.is_none());
        if how == MirrorSettle::UseMirror && missing && link.main == ServiceId::AniList {
            let mut svc = self.service(service_of(link.main)).map_err(|_| other(format!("{} is needed to add it: sign in, and check you are online.", link.main.title())))?;
            media = svc.media_by_mal_ids_of(k, &[mal_id]).map_err(|e| other(trouble_text(link.main, &e)))?.into_iter().next();
        }
        self.store().settle_web_edit(k, mal_id, how.into(), media.as_ref(), time::now_unix()).map_err(|e| other(e.to_string()))
    }

    /// A mirror conflict: send the main list's change anyway ("Send MyAnimeList’s", "Remove anyway").
    pub fn mirror_keep_mine(&self, id: i64) -> Result<(), KotoraError> {
        self.store().keep_mine(id).map_err(|e| other(e.to_string()))
    }

    /// A mirror conflict: keep the mirror's entry as it is.
    pub fn mirror_take_theirs(&self, id: i64) -> Result<(), KotoraError> {
        let link = self.mirror_link_now(&self.store()).ok_or_else(|| other("The mirror is off."))?;
        self.store().take_theirs(link.to.name(), id, time::now_unix()).map_err(|e| other(e.to_string()))
    }

    /// A failed mirror change: drop it.
    pub fn mirror_discard(&self, id: i64) -> Result<(), KotoraError> {
        let link = self.mirror_link_now(&self.store()).ok_or_else(|| other("The mirror is off."))?;
        self.store().discard(link.to.name(), id, time::now_unix()).map_err(|e| other(e.to_string()))
    }

    /// The mirror worker's run (desktop `kotora-mirror`), bounded like [`Kotora::flush_queue`]:
    /// look up the AniList ids of new adds, send the mirror's queue, and with `read_list` (after
    /// a main sync) read the mirror's list for website edits. A failure here never touches the
    /// main list's queue: it is kept as the mirror's trouble line. Network: off the main thread.
    /// Nothing while the mirror is off.
    pub fn mirror_refresh(&self, read_list: bool) -> MirrorRun {
        let mut run = MirrorRun::default();
        let Ok(mut store) = self.own_store() else { return run };
        let Some(link) = self.mirror_link_now(&store) else { return run };
        let to = link.to.name();
        let mut svc = match self.service(service_of(link.to)) {
            Ok(s) => s,
            Err(_) => {
                let t = format!("Not signed in to {}: its changes wait.", link.to.title());
                Self::set_trouble(&store, Some(&t));
                run.trouble = Some(t);
                return run;
            }
        };
        let mut trouble: Option<String> = store.kv_get(TROUBLE_KEY).ok().flatten();
        let mut stop = false;
        if store.mirror_waits().is_ok_and(|w| w.iter().any(|x| x.error.is_none())) {
            match mirror::map_waits(svc.as_mut(), &mut store, time::now_unix()) {
                Ok(_) => {}
                Err(e) => {
                    stop = e == ServiceError::SignInAgain;
                    trouble = Some(trouble_text(link.to, &e));
                }
            }
        }
        let _ = store.retry_now(to);
        for _ in 0..FLUSH_MAX {
            if stop {
                break;
            }
            match queue::step(&mut store, svc.as_mut(), time::now_unix()) {
                Ok(Step::Handled { outcome, .. }) => match outcome {
                    Outcome::Retry(m) => trouble = Some(trouble_text(link.to, &ServiceError::Offline(m))),
                    Outcome::Sent | Outcome::AlreadyThere => {
                        run.sent += 1;
                        trouble = None;
                    }
                    _ => {}
                },
                Ok(Step::Wait(_)) | Ok(Step::Idle) => break,
                Ok(Step::SignInAgain) => {
                    stop = true;
                    trouble = Some(trouble_text(link.to, &ServiceError::SignInAgain));
                }
                Err(e) => {
                    trouble = Some(format!("The mirror\u{2019}s queue can\u{2019}t be saved: {e}"));
                    stop = true;
                }
            }
        }
        if read_list && !stop {
            match mirror::sync_mirror(svc.as_mut(), &mut store, time::now_unix()) {
                Ok(_) => {
                    run.read = true;
                    trouble = None;
                }
                Err(e) => trouble = Some(trouble_text(link.to, &e)),
            }
        }
        Self::set_trouble(&store, trouble.as_deref());
        run.trouble = trouble;
        run
    }

    /// "Open on AniList" for an entry of the active list, with the mirror on and the other
    /// service's id known (MyAnimeList by the MyAnimeList id, AniList by its saved list's id).
    pub fn mirror_service_link(&self, kind: MediaKind, id: u32) -> Option<ServiceLink> {
        let store = self.store();
        let link = self.mirror_link_now(&store)?;
        let k: Kind = kind.into();
        let mal = match link.main {
            ServiceId::Mal => Some(id),
            ServiceId::AniList => store.load_item((link.main.name(), k), id).ok().flatten().and_then(|it| it.media.id_mal),
        };
        let other = match link.to {
            ServiceId::Mal => mal,
            ServiceId::AniList => mal.and_then(|m| store.mal_id_pairs((link.to.name(), k)).unwrap_or_default().into_iter().find(|(_, x)| *x == m).map(|(i, _)| i)),
        }
        .filter(|i| *i > 0)?;
        let to = service_of(link.to);
        Some(ServiceLink { service: to, label: format!("Open on {}", link.to.title()), url: media_url(to, kind, other) })
    }
}

fn op_row(op: &Op, now: i64, fmt: ScoreFormat) -> MirrorOpRow {
    let (note, note_is_error) = match (&op.error, op.state) {
        (Some(e), OpState::Failed) => (Some(e.clone()), true),
        (Some(e), OpState::Queued) => (Some(format!("{e}; Kotora tries again by itself")), false),
        _ => (None, false),
    };
    MirrorOpRow {
        id: op.id,
        title: op.title.clone(),
        what: queue::describe_op_with(op, fmt),
        note,
        note_is_error,
        state: op.state.label().into(),
        can_discard: op.state == OpState::Failed,
        sent: op.state == OpState::Done,
        age: time::age(op.sent_at.unwrap_or(op.edited_at), now),
    }
}

fn edit_card(e: &WebEdit, main: &str, to: &str, now: i64, (fm, ft): (ScoreFormat, ScoreFormat)) -> MirrorEditCard {
    let (why, use_main, use_mirror) = match (&e.main, &e.mirror) {
        (Some(_), Some(_)) => (format!("Changed on {to} {}. Kotora didn\u{2019}t overwrite it. Pick one, or leave both as they are.", sync_age(now - e.at)), Some(format!("Use {main}\u{2019}s")), Some(format!("Use {to}\u{2019}s"))),
        (None, Some(_)) => (format!("Added on {to}; it isn\u{2019}t on your {main} list."), None, Some(format!("Add to {main}"))),
        (Some(_), None) => (format!("Removed on {to}; it is still on your {main} list."), Some(format!("Add back to {to}")), None),
        (None, None) => (String::new(), None, None),
    };
    MirrorEditCard {
        kind: e.kind.into(),
        mal_id: e.mal_id,
        title: format!("{to}: {}", e.title),
        why,
        main_head: main.to_string(),
        main_text: saved_text(e.kind, e.main.as_ref(), "Not on the list", fm),
        use_main,
        mirror_head: to.to_string(),
        mirror_text: saved_text(e.kind, e.mirror.as_ref(), "Removed", ft),
        use_mirror,
        leave: "Leave both as they are".into(),
    }
}

fn conflict_card(op: &Op, main: &str, to: &str, (fm, ft): (ScoreFormat, ScoreFormat)) -> MirrorConflictCard {
    let vt = |v: &Values, f: ScoreFormat| values_text(op.kind, v, f);
    let title = format!("{to}: {}", op.title);
    if op.removed {
        return MirrorConflictCard {
            id: op.id,
            title,
            why: format!("{to} changed after you removed it. Kotora didn\u{2019}t remove it there. Pick one."),
            mine_head: format!("Removed from your {main} list"),
            mine_text: format!("Was {}", vt(&op.old, ft)),
            keep: "Remove anyway".into(),
            theirs_head: to.to_string(),
            theirs_text: op.remote.as_ref().map_or_else(|| "Removed".to_string(), |r| vt(&r.values, ft)),
            take: format!("Keep it on {to}"),
        };
    }
    let (theirs_text, take) = match &op.remote {
        Some(r) => (vt(&r.values, ft), format!("Keep {to}\u{2019}s")),
        None => ("Removed".to_string(), "Leave it removed".to_string()),
    };
    MirrorConflictCard {
        id: op.id,
        title,
        why: format!("{to} changed after this edit. Kotora didn\u{2019}t overwrite it. Pick one."),
        mine_head: format!("From your {main} list"),
        mine_text: vt(&op.mine(), fm),
        keep: format!("Send {main}\u{2019}s"),
        theirs_head: to.to_string(),
        theirs_text,
        take,
    }
}

#[cfg(test)]
mod tests {
    //! The mirror without a network: lists put straight into the store, a preview built from a
    //! made-up "fetched" mirror list, then the store's own mirror state.

    use std::sync::Arc;

    use kotora_core::{Media, Score, Status};

    use super::*;

    #[derive(Default)]
    struct Vault(Mutex<HashMap<String, Vec<u8>>>);

    impl Platform for Vault {
        fn utc_offset(&self, _at_unix: i64) -> i32 {
            crate::testenv::OFFSET
        }
        fn log(&self, _level: LogLevel, _message: String) {}
        fn token_load(&self, service: String) -> Option<Vec<u8>> {
            self.0.lock().unwrap().get(&service).cloned()
        }
        fn token_save(&self, service: String, bytes: Vec<u8>) -> bool {
            self.0.lock().unwrap().insert(service, bytes);
            true
        }
        fn token_delete(&self, service: String) {
            self.0.lock().unwrap().remove(&service);
        }
    }

    fn open(name: &str) -> (Arc<Kotora>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("kotora-mobile-mirror-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = Paths {
            config_dir: dir.join("files").display().to_string(),
            data_dir: dir.join("nobackup").display().to_string(),
            cache_dir: dir.join("cache").display().to_string(),
        };
        (Kotora::new(paths, ClientIds { mal: "fake".into(), anilist: "fake".into() }, Arc::new(Vault::default())).unwrap(), dir)
    }

    fn sign_in(k: &Kotora, s: Service) {
        let t = kotora_core::tokens::Token { access_token: "fake".into(), refresh_token: "fake".into(), expires_at: i64::MAX / 2 };
        k.tokens.save(s.name(), &t).unwrap();
        k.store().set_account(s.name(), "made-up-user").unwrap();
    }

    fn item(id: u32, id_mal: Option<u32>, title: &str, status: Status, progress: u32) -> ListItem {
        let media = Media { kind: Kind::Anime, id, id_mal, title: title.into(), episodes: Some(12), ..Default::default() };
        ListItem { media, status, progress, score: Score::from_ten(7), rewatching: false, times_rewatched: 0, volumes: 0, updated_at: 100 }
    }

    /// MyAnimeList main (1 Hoshikuzu Relay, 2 Paper Kite), AniList mirror (Hoshikuzu further on,
    /// 3 Lantern only there): the preview the switch would show.
    fn setup(name: &str) -> (Arc<Kotora>, PathBuf, Arc<MirrorPreview>) {
        let (k, dir) = open(name);
        sign_in(&k, Service::Mal);
        sign_in(&k, Service::AniList);
        k.set_active_service(Service::Mal).unwrap();
        let main = [item(1, None, "Hoshikuzu Relay", Status::Watching, 3), item(2, None, "Paper Kite Diaries", Status::PlanToWatch, 0)];
        k.store().replace_list("mal", &main, 1_000).unwrap();
        let fetched = vec![item(101, Some(1), "Hoshikuzu Relay", Status::Watching, 5), item(103, Some(3), "Lantern Post Office", Status::Completed, 12)];
        let link = Link::of(ServiceId::Mal);
        let mirror_media: HashMap<u32, Media> = [(2, Media { id: 102, id_mal: Some(2), title: "Paper Kite Diaries".into(), ..Default::default() })].into();
        let main_list = k.store().load_list("mal").unwrap();
        let p = mirror::compare_of(link, Kind::Anime, &main_list, &fetched, &mirror_media, &HashMap::new());
        let prev = k.preview_of(vec![Part { p, fetched, mirror_status: HashSet::new() }], false);
        (k, dir, prev)
    }

    #[test]
    fn the_switch_says_what_the_desktop_says() {
        let (k, dir) = open("words");
        assert!(k.mirror_state().is_none(), "signed out: no row");
        sign_in(&k, Service::Mal);
        let s = k.mirror_state().unwrap();
        assert_eq!((s.title.as_str(), s.on, s.can_turn_on), ("Mirror edits to AniList", false, false));
        assert_eq!(s.note, "Sign in to AniList too: choose Use AniList above, sign in, then come back to MyAnimeList. Your MyAnimeList list stays the main one.");
        sign_in(&k, Service::AniList);
        let s = k.mirror_state().unwrap();
        assert!(s.can_turn_on);
        assert!(s.note.starts_with("Every change you make in Kotora also goes to your AniList list"), "{}", s.note);
        assert_eq!(s.account_line, "MyAnimeList");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_preview_counts_and_merge_queue_both_sides() {
        let (k, dir, prev) = setup("merge");
        let v = prev.view();
        let counts: Vec<(String, u32)> = v.parts[0].counts.iter().map(|c| (c.label.clone(), c.count)).collect();
        assert_eq!(counts[..4], [("only on MyAnimeList".to_string(), 1), ("only on AniList".to_string(), 1), ("different".to_string(), 1), ("the same".to_string(), 0)]);
        assert_eq!(v.main_wins, "MyAnimeList wins");
        assert!(v.explain[1].starts_with("Merge: what is missing on each side is added"), "{}", v.explain[1]);
        let note = k.mirror_choose(prev, MirrorAlign::Merge).unwrap();
        assert_eq!(note, "On. Queued 1 change for AniList and 2 changes for MyAnimeList.");
        let s = k.mirror_state().unwrap();
        assert!(s.on && s.note.starts_with("On. Every change you make here also goes to AniList."), "{}", s.note);
        assert_eq!(s.account_line, "MyAnimeList \u{21c4} AniList");
        assert_eq!(s.waiting, 1);
        let q = k.mirror_queue().unwrap();
        assert_eq!((q.heading.as_str(), q.groups[0].label.as_str(), q.groups[0].rows.len()), ("AniList mirror", "WAITING", 1));
        assert_eq!(k.mirror_target().as_deref(), Some("AniList"));
        // An edit on the main list follows to the mirror (core's queue, same transaction).
        k.plus_one(MediaKind::Anime, 2).unwrap();
        assert_eq!(k.mirror_state().unwrap().waiting, 1, "Paper Kite's add on AniList takes the +1 in");
        let link = k.mirror_service_link(MediaKind::Anime, 1).unwrap();
        assert_eq!(link.url, "https://anilist.co/anime/101");
        let off = k.mirror_stop().unwrap();
        assert_eq!(off, "Off. 1 change for AniList that hadn\u{2019}t been sent was dropped.");
        assert!(!k.mirror_state().unwrap().on);
        assert!(k.mirror_queue().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dont_align_queues_nothing_and_a_switch_of_services_drops_the_link() {
        let (k, dir, prev) = setup("none");
        assert_eq!(k.mirror_choose(prev, MirrorAlign::DontAlign).unwrap(), "On. Edits you make here from now on also go to AniList.");
        assert!(k.mirror_state().unwrap().on);
        k.set_active_service(Service::AniList).unwrap();
        let s = k.mirror_state().unwrap();
        assert!(!s.on && s.title == "Mirror edits to MyAnimeList", "the old link isn't this list's");
        k.set_active_service(Service::Mal).unwrap();
        assert!(!k.mirror_state().unwrap().on, "and it went");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_mirror_without_its_sign_in_waits_and_says_so() {
        let (k, dir, prev) = setup("signin");
        k.mirror_choose(prev, MirrorAlign::MainWins).unwrap();
        k.tokens.forget("anilist").unwrap();
        let run = k.mirror_refresh(true);
        assert_eq!(run.trouble.as_deref(), Some("Not signed in to AniList: its changes wait."));
        assert!(k.mirror_state().unwrap().note.ends_with("Not signed in to AniList: its changes wait."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn values_and_cards_read_like_the_desktop() {
        let v = Values { status: Status::Watching, progress: 5, score: Score::from_ten(8), rewatching: false, times_rewatched: 0, volumes: 0 };
        assert_eq!(values_text(Kind::Anime, &v, ScoreFormat::Point10), "Watching, episode 5, score 8");
        let e = WebEdit { kind: Kind::Anime, mal_id: 9, title: "Moonlit Harbor".into(), main_id: None, main: None, mirror_id: Some(109), mirror: Some(v.into()), at: 0 };
        let c = edit_card(&e, "MyAnimeList", "AniList", 100, (ScoreFormat::Point10, ScoreFormat::Point10));
        assert_eq!((c.title.as_str(), c.why.as_str(), c.use_mirror.as_deref(), c.main_text.as_str()), ("AniList: Moonlit Harbor", "Added on AniList; it isn\u{2019}t on your MyAnimeList list.", Some("Add to MyAnimeList"), "Not on the list"));
        assert_eq!(sync_age(4000), "1 h ago");
    }
}
