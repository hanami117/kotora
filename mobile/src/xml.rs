//! Export and import the list as a MyAnimeList XML file: the desktop's Settings → Account →
//! Your list (`app/src/transfer.rs`), over the same core calls (`kotora_core::listxml`).
//!
//! The phone owns the files: the app writes [`XmlExport::bytes`] where the share sheet or the
//! document picker (iOS) / `ACTION_CREATE_DOCUMENT` (Android) says, and reads the picked file's
//! bytes into [`XmlImport::read`]. No path goes through here.
//!
//! - **Export** (MyAnimeList lists only: AniList's terms don't allow it, [`listxml::export_refusal`]).
//! - **Import:** [`XmlImport::read`] reads the file and compares it with the saved list (with
//!   AniList active, it looks the file's MyAnimeList ids up first: network, a worker thread),
//!   [`XmlImport::apply`] queues the adds and updates as one transaction, [`XmlImport::undo`]
//!   takes back what hasn't been sent. The words are the desktop's.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use kotora_core::listxml::{self, Mode, Plan, PlanRow, Verdict};
use kotora_core::queue::describe_in;
use kotora_core::store::Store;
use kotora_core::{Kind, time};

use crate::{Kotora, KotoraError, MediaKind, Service, other};

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The other service's name when the mirror of `service` is on.
fn mirror_target(db: &std::path::Path, service: Service) -> Option<&'static str> {
    let store = Store::open(db).ok()?;
    store.mirror_link().ok().flatten().filter(|l| l.main == service.id()).map(|l| l.to.title())
}

// ---------------------------------------------------------------------------------------- export

/// A written export: the file's bytes and the desktop's note once it is saved.
#[derive(uniffi::Object)]
pub struct XmlExport {
    xml: String,
    kind: Kind,
    written: usize,
    skipped: Vec<String>,
    file_name: String,
}

#[uniffi::export]
impl XmlExport {
    pub fn bytes(&self) -> Vec<u8> {
        self.xml.as_bytes().to_vec()
    }

    /// `animelist-2026-11-16.xml` / `mangalist-…`: the name the save dialog proposes.
    pub fn file_name(&self) -> String {
        self.file_name.clone()
    }

    /// "Exported 12 anime to NAME." (and who was left out), once the file is saved as `name`.
    pub fn saved_note(&self, name: String) -> String {
        let what = if self.kind == Kind::Manga { plural(self.written, "manga", "manga") } else { plural(self.written, "anime", "anime") };
        let mut msg = format!("Exported {what} to {name}.");
        if !self.skipped.is_empty() {
            msg.push_str(&format!(" Left out (no MyAnimeList id): {}.", self.skipped.join(", ")));
        }
        msg
    }
}

/// Settings' export row, as the desktop shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ExportRow {
    /// "Export anime list…"
    pub title: String,
    /// The note under it: what the file holds, or why it can't be used.
    pub note: String,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------------------- import

/// A status the user picks (the desktop's "Status to pick").
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ImportConflict {
    /// The entry's id on the active service (what [`XmlImport::apply`] takes).
    pub id: u32,
    pub title: String,
    /// "Keep Watching"
    pub keep: String,
    /// "Use Dropped"
    pub use_file: String,
}

/// One group of the preview: a heading and its lines.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ImportGroup {
    pub heading: String,
    pub rows: Vec<String>,
}

/// A file compared with the list, waiting for the user's pick (the desktop's words).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ImportPreview {
    pub file: String,
    /// "40 anime"
    pub total: String,
    pub new: u32,
    pub different: u32,
    pub same: u32,
    pub unknown: u32,
    /// New on the list, Different (with "update"), Left out: only the non-empty ones.
    pub groups: Vec<ImportGroup>,
    pub conflicts: Vec<ImportConflict>,
    /// Under the groups: the queue, the count rule, what is left out.
    pub note: String,
    /// "Add new only (3)", enabled when there is something new.
    pub add_new: String,
    pub add_new_enabled: bool,
    /// "Add new and update (5)"
    pub add_and_update: String,
    pub add_and_update_enabled: bool,
}

/// An import handed to the queue (or nothing to do), with the desktop's message.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ImportDone {
    pub queued: u32,
    pub message: String,
    /// Open Queue and Undo import are shown.
    pub can_undo: bool,
}

/// One import of a file into the active account's anime or manga list. Made by
/// [`Kotora::xml_import`].
#[derive(uniffi::Object)]
pub struct XmlImport {
    db: PathBuf,
    service: Service,
    kind: Kind,
    open: Box<dyn Fn() -> Result<Box<dyn kotora_core::service::ListService>, KotoraError> + Send + Sync>,
    busy: Mutex<String>,
    plan: Mutex<Option<Plan>>,
    queued: Mutex<Vec<i64>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[uniffi::export]
impl XmlImport {
    /// Settings' import row: title and note (the desktop's).
    pub fn row(&self) -> ExportRow {
        let service = self.service.id().title();
        let manga = self.kind == Kind::Manga;
        ExportRow {
            title: if manga { "Import manga list\u{2026}" } else { "Import anime list\u{2026}" }.into(),
            note: if manga {
                format!("Read a MyAnimeList manga XML export (.xml or .xml.gz) and add it to your {service} manga list. You see what changes before anything is sent.")
            } else {
                format!("Read a MyAnimeList anime XML export (.xml or .xml.gz) and add it to your {service} anime list. You see what changes before anything is sent.")
            },
            enabled: true,
        }
    }

    /// What [`XmlImport::read`] is doing now ("Reading the file…", "Looking up 12 MyAnimeList
    /// ids on AniList…"); empty when idle.
    pub fn busy(&self) -> String {
        lock(&self.busy).clone()
    }

    /// Reads the picked file (`bytes`, plain or gzipped; `file_name` as the picker gives it) and
    /// compares it with the saved list. With AniList it looks ids up: network, a worker thread.
    pub fn read(&self, bytes: Vec<u8>, file_name: String) -> Result<ImportPreview, KotoraError> {
        *lock(&self.busy) = "Reading the file\u{2026}".into();
        let r = self.read_inner(&bytes);
        lock(&self.busy).clear();
        let plan = r?;
        let preview = self.preview(&plan, file_name);
        *lock(&self.plan) = Some(plan);
        Ok(preview)
    }

    /// Add new only (`new_only`) or Add new and update: the changes go to the sync queue as one
    /// transaction. `use_file`: conflict ids settled for the file's status (the rest keep yours).
    pub fn apply(&self, new_only: bool, use_file: Vec<u32>) -> Result<ImportDone, KotoraError> {
        let plan = lock(&self.plan).take().ok_or_else(|| other("Choose a file first."))?;
        let mode = if new_only { Mode::NewOnly } else { Mode::NewAndDifferent };
        let picks: HashSet<u32> = use_file.into_iter().collect();
        let actions = listxml::actions(&plan, mode, &picks);
        if actions.is_empty() {
            return Ok(ImportDone { queued: 0, message: "Nothing to change: your list already has everything in the file.".into(), can_undo: false });
        }
        let mut store = Store::open(&self.db)?;
        let ids = store.import(self.service.name(), &actions, time::now_unix()).map_err(|e| other(format!("The import can't be saved: {e}")))?;
        let n = ids.len();
        *lock(&self.queued) = ids;
        let what = plural(n, "change", "changes");
        let main = self.service.id().title();
        let message = match mirror_target(&self.db, self.service) {
            Some(to) => format!("Queued {what} for {main}, mirrored to {to}. They go out at each service\u{2019}s pace; the Queue page shows them."),
            None => format!("Queued {what} for {main}. They go out at its pace; the Queue page shows them."),
        };
        Ok(ImportDone { queued: n as u32, message, can_undo: true })
    }

    /// Undo import: what still waits is discarded. Returns the desktop's message.
    pub fn undo(&self) -> Result<String, KotoraError> {
        let ids = std::mem::take(&mut *lock(&self.queued));
        let mut store = Store::open(&self.db)?;
        let n = store.undo_import(self.service.name(), &ids, time::now_unix()).map_err(|e| other(format!("The import can't be taken back: {e}")))?;
        Ok(format!("Took back {} that hadn\u{2019}t been sent yet.", plural(n, "change", "changes")))
    }

    /// Cancel: forgets the file.
    pub fn cancel(&self) {
        lock(&self.plan).take();
    }
}

impl XmlImport {
    fn read_inner(&self, bytes: &[u8]) -> Result<Plan, KotoraError> {
        let file = listxml::parse_kind(bytes, self.kind).map_err(other)?;
        let store = Store::open(&self.db)?;
        let list = store.load_list((self.service.name(), self.kind)).map_err(|e| other(format!("Can\u{2019}t read the saved list: {e}")))?;
        let mut mapped = HashMap::new();
        if self.service != Service::Mal {
            let ids = Plan::ids_to_map(&file, &list, self.service.name());
            if !ids.is_empty() {
                let title = self.service.id().title();
                *lock(&self.busy) = format!("Looking up {} on {title}\u{2026}", plural(ids.len(), "MyAnimeList id", "MyAnimeList ids"));
                let mut svc = (self.open)().map_err(|_| other(format!("{title} is needed to match MyAnimeList ids: sign in, and check you are online.")))?;
                let media = svc.media_by_mal_ids_of(self.kind, &ids).map_err(|e| other(format!("Can\u{2019}t look the ids up on {title}: {e}")))?;
                mapped = media.into_iter().filter_map(|m| Some((m.id_mal?, m))).collect();
            }
        }
        Ok(listxml::plan(&file, self.service.name(), &list, &mapped))
    }

    fn preview(&self, plan: &Plan, file: String) -> ImportPreview {
        let c = plan.counts();
        let service = self.service.id().title();
        let kind = plan.kind;
        let manga = kind == Kind::Manga;
        let mut groups = Vec::new();
        let new: Vec<String> = plan.rows.iter().filter(|r| r.verdict == Verdict::New).map(|r| format!("{}: {}", r.title(), values_text(r))).collect();
        if !new.is_empty() {
            groups.push(ImportGroup { heading: format!("New on your {service} list"), rows: new });
        }
        let diff: Vec<String> = plan
            .rows
            .iter()
            .filter(|r| r.verdict == Verdict::Different && r.conflict.is_none())
            .map(|r| match r.current {
                _ if !r.entry.update_on_import => format!("{}: kept (the file asks not to update it)", r.title()),
                Some(cur) if !r.update.is_empty() => format!("{}: {}", r.title(), describe_in(kind, &r.update, &cur)),
                _ => format!("{}: yours is further along, kept", r.title()),
            })
            .collect();
        if !diff.is_empty() {
            groups.push(ImportGroup { heading: "Different (with \u{201c}update\u{201d})".into(), rows: diff });
        }
        let conflicts = plan
            .conflicts()
            .filter_map(|r| {
                let (id, k) = (r.target?, r.conflict?);
                Some(ImportConflict { id, title: r.title().to_string(), keep: format!("Keep {}", k.mine.label()), use_file: format!("Use {}", k.file.label()) })
            })
            .collect();
        let unknown: Vec<String> = plan
            .rows
            .iter()
            .filter_map(|r| match &r.verdict {
                Verdict::Unknown(why) => Some(format!("{}: {why}", if r.entry.title.is_empty() { format!("MyAnimeList {}", r.entry.mal_id) } else { r.entry.title.clone() })),
                _ => None,
            })
            .collect();
        if !unknown.is_empty() {
            groups.push(ImportGroup { heading: "Left out".into(), rows: unknown });
        }
        let count_rule = if manga { "A higher chapter or volume count wins" } else { "A higher episode count wins" };
        let to = match mirror_target(&self.db, self.service) {
            Some(m) => format!("{service}, and to {m} (the mirror), at each service\u{2019}s pace"),
            None => format!("{service} at its pace"),
        };
        let mut notes = vec![format!("Changes go through the sync queue to {to}, and wait there while you are offline. {count_rule}; a score fills an empty one.")];
        if plan.other > 0 {
            let verb = if plan.other == 1 { "is" } else { "are" };
            let (what, hint) = if manga { (plural(plan.other, "anime entry", "anime entries"), "Import anime list\u{2026}") } else { (plural(plan.other, "manga entry", "manga entries"), "Import manga list\u{2026}") };
            notes.push(format!("{what} in the file {verb} left out: use \u{201c}{hint}\u{201d} for those."));
        }
        notes.push("Dates and notes in the file are not imported.".into());
        ImportPreview {
            file,
            total: if manga { plural(plan.rows.len(), "manga", "manga") } else { plural(plan.rows.len(), "anime", "anime") },
            new: c.new as u32,
            different: c.different as u32,
            same: c.same as u32,
            unknown: c.unknown as u32,
            groups,
            conflicts,
            note: notes.join(" "),
            add_new: format!("Add new only ({})", c.new),
            add_new_enabled: c.new > 0,
            add_and_update: format!("Add new and update ({})", c.new + c.to_update),
            add_and_update_enabled: c.new + c.to_update > 0,
        }
    }
}

fn values_text(r: &PlanRow) -> String {
    let Some(v) = r.entry.values() else { return String::new() };
    let manga = r.entry.kind == Kind::Manga;
    let mut t = v.status.label_in(r.entry.kind).to_string();
    if v.rewatching {
        t.push_str(if manga { ", rereading" } else { ", rewatching" });
    }
    if v.progress > 0 {
        t.push_str(&format!(", {} {}", if manga { "chapter" } else { "episode" }, v.progress));
    }
    if v.volumes > 0 {
        t.push_str(&format!(", volume {}", v.volumes));
    }
    if let Some(sc) = v.score {
        t.push_str(&format!(", score {}", sc.to_ten()));
    }
    t
}

// ------------------------------------------------------------------------------------- api

#[uniffi::export]
impl Kotora {
    /// Settings' export row for the active account's `kind` list (the desktop's words).
    pub fn xml_export_row(&self, kind: MediaKind) -> ExportRow {
        let manga = matches!(kind, MediaKind::Manga);
        let title = if manga { "Export manga list\u{2026}" } else { "Export anime list\u{2026}" }.to_string();
        let Some(service) = self.active() else {
            return ExportRow { title, note: "Sign in first: the export is written from your saved list.".into(), enabled: false };
        };
        if let Some(why) = listxml::export_refusal(service.id()) {
            return ExportRow { title, note: why.into(), enabled: false };
        }
        let note = if manga {
            "Your manga list as a MyAnimeList XML file, which MyAnimeList, AniList, Kitsu and Taiga can import. Status, chapters, volumes, score, rereads and the dates Kotora has."
        } else {
            "Your anime list as a MyAnimeList XML file, which MyAnimeList, AniList, Kitsu and Taiga can import. Status, episodes, score, rewatches and the dates Kotora has."
        };
        ExportRow { title, note: note.into(), enabled: true }
    }

    /// Writes the saved `kind` list of the active account as a MyAnimeList XML file. Local.
    pub fn xml_export(&self, kind: MediaKind) -> Result<Arc<XmlExport>, KotoraError> {
        let service = self.active_or_err()?;
        if let Some(why) = listxml::export_refusal(service.id()) {
            return Err(other(why));
        }
        let kind: Kind = kind.into();
        let ex = listxml::export_kind(&self.store(), service.id(), kind).map_err(other)?;
        if ex.written == 0 {
            return Err(other(if kind == Kind::Manga { "Your saved manga list is empty: turn manga on, sync, and try again." } else { "Your saved list is empty: sign in and sync first." }));
        }
        Ok(Arc::new(XmlExport { xml: ex.xml, kind, written: ex.written, skipped: ex.skipped, file_name: listxml::default_file_name_of(kind, time::now_unix()) }))
    }

    /// An import of a MyAnimeList XML file into the active account's `kind` list.
    pub fn xml_import(self: Arc<Self>, kind: MediaKind) -> Result<Arc<XmlImport>, KotoraError> {
        let service = self.active_or_err()?;
        let me = self.clone();
        Ok(Arc::new(XmlImport {
            db: self.db.clone(),
            service,
            kind: kind.into(),
            open: Box::new(move || me.service(service)),
            busy: Mutex::new(String::new()),
            plan: Mutex::new(None),
            queued: Mutex::new(Vec::new()),
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use kotora_core::{ListItem, Media, Score, Status};

    use super::*;
    use crate::{ClientIds, LogLevel, Paths, Platform};

    #[derive(Default)]
    struct Fake(Mutex<HashMap<String, Vec<u8>>>);

    impl Platform for Fake {
        fn utc_offset(&self, _at: i64) -> i32 {
            0
        }
        fn log(&self, _level: LogLevel, _message: String) {}
        fn token_load(&self, s: String) -> Option<Vec<u8>> {
            self.0.lock().unwrap().get(&s).cloned()
        }
        fn token_save(&self, s: String, b: Vec<u8>) -> bool {
            self.0.lock().unwrap().insert(s, b);
            true
        }
        fn token_delete(&self, s: String) {
            self.0.lock().unwrap().remove(&s);
        }
    }

    fn open(name: &str, service: &str) -> Arc<Kotora> {
        let dir = std::env::temp_dir().join(format!("kotora-xml-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = |d: &str| dir.join(d).display().to_string();
        let k = Kotora::new(Paths { config_dir: p("files"), data_dir: p("nobackup"), cache_dir: p("cache") }, ClientIds { mal: "x".into(), anilist: "y".into() }, Arc::new(Fake::default())).unwrap();
        let t = kotora_core::tokens::Token { access_token: "fake".into(), refresh_token: "fake".into(), expires_at: i64::MAX / 2 };
        k.tokens.save(service, &t).unwrap();
        k.store().set_account(service, "made-up-user").unwrap();
        k
    }

    fn item(id: u32, title: &str, status: Status, progress: u32) -> ListItem {
        ListItem {
            media: Media { kind: Kind::Anime, id, title: title.into(), episodes: Some(12), format: "tv".into(), ..Default::default() },
            status,
            progress,
            score: Score::from_ten(8),
            rewatching: false,
            times_rewatched: 0,
            volumes: 0,
            updated_at: 1,
        }
    }

    fn file(rows: &[(u32, &str, &str, u32)]) -> Vec<u8> {
        let mut x = String::from("<myanimelist><myinfo><user_name>u</user_name></myinfo>");
        for (id, title, st, ep) in rows {
            x.push_str(&format!("<anime><series_animedb_id>{id}</series_animedb_id><series_title><![CDATA[{title}]]></series_title><series_type>TV</series_type><series_episodes>12</series_episodes><my_watched_episodes>{ep}</my_watched_episodes><my_score>8</my_score><my_status>{st}</my_status></anime>"));
        }
        x.push_str("</myanimelist>");
        x.into_bytes()
    }

    #[test]
    fn export_writes_the_mal_list_and_refuses_anilist() {
        let k = open("export", "mal");
        assert!(k.xml_export(MediaKind::Anime).is_err(), "empty list");
        k.store().replace_list("mal", &[item(1, "Hoshikuzu Relay", Status::Watching, 6)], 1_000).unwrap();
        let row = k.xml_export_row(MediaKind::Anime);
        assert!(row.enabled && row.title == "Export anime list\u{2026}");
        let ex = k.xml_export(MediaKind::Anime).unwrap();
        assert!(ex.file_name().starts_with("animelist-") && ex.file_name().ends_with(".xml"));
        let xml = String::from_utf8(ex.bytes()).unwrap();
        assert!(xml.contains("<series_animedb_id>1</series_animedb_id>") && xml.contains("Hoshikuzu Relay"));
        assert_eq!(ex.saved_note("a.xml".into()), "Exported 1 anime to a.xml.");
        // The file reads back as the same list: all the same.
        let im = k.clone().xml_import(MediaKind::Anime).unwrap();
        let p = im.read(ex.bytes(), "a.xml".into()).unwrap();
        assert_eq!((p.new, p.different, p.same, p.unknown), (0, 0, 1, 0));

        let a = open("export-anilist", "anilist");
        assert!(!a.xml_export_row(MediaKind::Anime).enabled);
        assert!(a.xml_export(MediaKind::Anime).is_err());
    }

    #[test]
    fn import_previews_queues_and_undoes() {
        let k = open("import", "mal");
        k.store().replace_list("mal", &[item(1, "Hoshikuzu Relay", Status::Watching, 6), item(2, "Kagerou Bakery", Status::Watching, 3)], 1_000).unwrap();
        let im = k.clone().xml_import(MediaKind::Anime).unwrap();
        assert!(im.row().note.contains("MyAnimeList anime XML export"));
        let bytes = file(&[(1, "Hoshikuzu Relay", "Watching", 8), (2, "Kagerou Bakery", "Dropped", 3), (9, "Moonlit Harbor", "Plan to Watch", 0), (0, "No Id", "Watching", 1)]);
        let p = im.read(bytes.clone(), "animelist.xml".into()).unwrap();
        assert_eq!((p.new, p.different, p.same, p.unknown), (1, 2, 0, 1));
        assert_eq!(p.total, "4 anime");
        assert_eq!(p.conflicts.len(), 1);
        assert_eq!(p.conflicts[0].keep, "Keep Watching");
        assert_eq!(p.conflicts[0].use_file, "Use Dropped");
        assert_eq!(p.add_new, "Add new only (1)");
        assert_eq!(p.add_and_update, "Add new and update (3)");
        assert!(p.groups.iter().any(|g| g.heading == "New on your MyAnimeList list" && g.rows.len() == 1 && g.rows[0].starts_with("Moonlit Harbor: Plan to")));
        assert!(p.groups.iter().any(|g| g.heading == "Left out"));
        assert!(p.note.starts_with("Changes go through the sync queue to MyAnimeList at its pace"));

        let done = im.apply(false, vec![2]).unwrap();
        assert_eq!(done.queued, 3);
        assert!(done.can_undo && done.message.starts_with("Queued 3 changes for MyAnimeList."));
        assert_eq!(im.undo().unwrap(), "Took back 3 changes that hadn\u{2019}t been sent yet.");
        assert!(im.apply(true, vec![]).is_err(), "the plan is used once");

        // Nothing new: nothing queued.
        let p = im.read(file(&[(1, "Hoshikuzu Relay", "Watching", 6)]), "same.xml".into()).unwrap();
        assert!(!p.add_new_enabled);
        let done = im.apply(true, vec![]).unwrap();
        assert_eq!((done.queued, done.can_undo), (0, false));
    }

    #[test]
    fn a_manga_file_is_refused_by_the_anime_import() {
        let k = open("kind", "mal");
        let im = k.clone().xml_import(MediaKind::Anime).unwrap();
        let x = b"<myanimelist><myinfo><user_export_type>2</user_export_type></myinfo><manga><manga_mangadb_id>2</manga_mangadb_id><my_status>Reading</my_status></manga></myanimelist>".to_vec();
        assert!(im.read(x, "m.xml".into()).is_err());
        let m = k.clone().xml_import(MediaKind::Manga).unwrap();
        assert!(m.row().title == "Import manga list\u{2026}");
    }
}
