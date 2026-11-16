//! The user's anime list as a MyAnimeList XML file: export and import.
//!
//! The format is the one MyAnimeList's own export writes (`myanimelist` / `myinfo` / `anime`
//! with `series_animedb_id`, `my_watched_episodes`, `my_status`, `my_score`, …). MyAnimeList,
//! AniList, Kitsu and Taiga read it. MyAnimeList ships it gzipped (`.xml.gz`); both are read.
//!
//! - **Export** ([`export`]): only from a MyAnimeList list. AniList's API terms forbid using its
//!   API as a backup and using its data, user data included, in competing list services
//!   (<https://docs.anilist.co/guide/terms-of-use>), so an AniList list is never written out
//!   ([`export_refusal`]). The file holds the user's own entries and only the catalogue fields
//!   the format has (`series_title`, `series_type`, `series_episodes`), as MyAnimeList sent them.
//!   Start and finish dates come from the saved details pages when there are any (the list
//!   itself doesn't carry them); otherwise `0000-00-00`, as MyAnimeList writes an empty date.
//! - **Import** ([`parse`], [`plan`]): read a file, compare it with the active list, and say per
//!   entry: new, different, same or unknown. Applying a plan (`Store::import`, in `queue.rs`)
//!   goes through the sync queue, never straight to the service.
//!
//! Nothing here talks to a service: the caller maps MyAnimeList ids to the active service's
//! media first (AniList: [`crate::service::ListService::media_by_mal_ids`]).

use std::collections::{HashMap, HashSet};
use std::io::Read;

use crate::model::{Change, ListItem, Media, Values};
use crate::service::ServiceId;
use crate::store::Store;
use crate::{Kind, Status};

/// The largest file read, unpacked (a MyAnimeList export of 10,000 anime is about 10 MB).
const MAX_BYTES: u64 = 64 << 20;

/// MyAnimeList's empty date.
pub const NO_DATE: &str = "0000-00-00";

// ------------------------------------------------------------------------------------- the file

/// One `<anime>` of a file, as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlEntry {
    /// `series_animedb_id`; 0 when missing or not a number.
    pub mal_id: u32,
    pub title: String,
    /// `series_type` as written (`TV`, `Movie`, …).
    pub series_type: String,
    /// `series_episodes`; `None` when 0 (unknown) or missing.
    pub episodes: Option<u32>,
    /// `my_status` as written.
    pub raw_status: String,
    /// `None`: a status Kotora doesn't know.
    pub status: Option<Status>,
    pub watched: u32,
    /// 1-10; `None`: 0 (not scored) or missing.
    pub score: Option<u8>,
    /// `my_start_date`, `None` for `0000-00-00`.
    pub start_date: Option<String>,
    pub finish_date: Option<String>,
    pub times_watched: u32,
    pub rewatching: bool,
    pub rewatching_ep: u32,
    /// `update_on_import`: 0 asks importers to only add, never change an entry already there.
    pub update_on_import: bool,
}

impl XmlEntry {
    /// The list values this entry asks for, in Kotora's model: a rewatch is Completed with the
    /// rewatch flag and the rewatch's episode as progress. `None`: unknown status.
    pub fn values(&self) -> Option<Values> {
        let mut status = self.status?;
        let rewatching = self.rewatching && matches!(status, Status::Completed | Status::Watching);
        let mut progress = self.watched;
        if rewatching {
            status = Status::Completed;
            if self.rewatching_ep > 0 {
                progress = self.rewatching_ep;
            }
        }
        if let Some(n) = self.episodes {
            progress = progress.min(n);
        }
        Some(Values { status, progress, score: self.score, rewatching, times_rewatched: self.times_watched, volumes: 0 })
    }

    /// A media row for an anime that is not on the list yet (MyAnimeList: its id is the key;
    /// the next sync replaces the row with what the service has).
    pub fn media(&self) -> Media {
        Media {
            kind: Kind::Anime,
            id: self.mal_id,
            title: if self.title.is_empty() { format!("MyAnimeList {}", self.mal_id) } else { self.title.clone() },
            episodes: self.episodes,
            format: format_from_type(&self.series_type),
            ..Media::default()
        }
    }
}

/// A file, as read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct XmlList {
    /// `myinfo/user_name`, when there is one.
    pub user_name: Option<String>,
    pub entries: Vec<XmlEntry>,
    /// `<manga>` entries: not imported (anime only for now).
    pub manga: usize,
}

/// Reads a MyAnimeList XML file, plain or gzipped.
pub fn parse(bytes: &[u8]) -> Result<XmlList, String> {
    let text = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .take(MAX_BYTES + 1)
            .read_to_end(&mut out)
            .map_err(|_| "The file looks gzipped but can't be unpacked.".to_string())?;
        if out.len() as u64 > MAX_BYTES {
            return Err("The file is too large for a list export.".into());
        }
        String::from_utf8(out).map_err(|_| "The file is not UTF-8 text.".to_string())?
    } else {
        if bytes.len() as u64 > MAX_BYTES {
            return Err("The file is too large for a list export.".into());
        }
        String::from_utf8(bytes.to_vec()).map_err(|_| "The file is not UTF-8 text.".to_string())?
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let doc = roxmltree::Document::parse(text).map_err(|e| format!("The file is not valid XML ({e})."))?;
    let root = doc.root_element();
    if root.tag_name().name() != "myanimelist" {
        return Err("This is not a MyAnimeList list export (no <myanimelist>).".into());
    }
    let mut list = XmlList::default();
    for n in root.children().filter(|n| n.is_element()) {
        match n.tag_name().name() {
            "myinfo" => list.user_name = child_text(n, "user_name").filter(|s| !s.is_empty()),
            "anime" => list.entries.push(read_entry(n)),
            "manga" => list.manga += 1,
            _ => {}
        }
    }
    if list.entries.is_empty() && list.manga > 0 {
        return Err("This is a manga list. Kotora imports anime lists for now.".into());
    }
    Ok(list)
}

fn child_text(n: roxmltree::Node, name: &str) -> Option<String> {
    let c = n.children().find(|c| c.is_element() && c.tag_name().name() == name)?;
    // Text and CDATA both come as text nodes.
    Some(c.children().filter_map(|t| t.text()).collect::<String>().trim().to_string())
}

fn num(n: roxmltree::Node, name: &str) -> u32 {
    child_text(n, name).and_then(|s| s.parse::<f64>().ok()).filter(|v| v.is_finite() && *v >= 0.0).map_or(0, |v| v.min(u32::MAX as f64) as u32)
}

fn read_entry(n: roxmltree::Node) -> XmlEntry {
    let raw_status = child_text(n, "my_status").unwrap_or_default();
    let date = |name| child_text(n, name).filter(|d| !d.is_empty() && !d.starts_with("0000"));
    let score = num(n, "my_score");
    XmlEntry {
        mal_id: num(n, "series_animedb_id"),
        title: child_text(n, "series_title").unwrap_or_default(),
        series_type: child_text(n, "series_type").unwrap_or_default(),
        episodes: Some(num(n, "series_episodes")).filter(|e| *e > 0),
        status: status_from_xml(&raw_status),
        raw_status,
        watched: num(n, "my_watched_episodes"),
        score: (1..=10).contains(&score).then_some(score as u8),
        start_date: date("my_start_date"),
        finish_date: date("my_finish_date"),
        times_watched: num(n, "my_times_watched"),
        rewatching: num(n, "my_rewatching") == 1,
        rewatching_ep: num(n, "my_rewatching_ep"),
        // Missing: MyAnimeList's default, update.
        update_on_import: child_text(n, "update_on_import").is_none_or(|v| v.trim() != "0"),
    }
}

/// `my_status`: the words MyAnimeList writes (`Plan to Watch`, `On-Hold`), the API's names
/// (`plan_to_watch`) and the old numbers (1-4, 6).
pub fn status_from_xml(s: &str) -> Option<Status> {
    let w: String = s.trim().to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    Some(match w.as_str() {
        "watching" | "1" => Status::Watching,
        "completed" | "2" => Status::Completed,
        "onhold" | "3" => Status::OnHold,
        "dropped" | "4" => Status::Dropped,
        "plantowatch" | "6" => Status::PlanToWatch,
        _ => return None,
    })
}

/// `my_status` as MyAnimeList's export writes it.
pub fn status_to_xml(st: Status) -> &'static str {
    match st {
        Status::Watching => "Watching",
        Status::Completed => "Completed",
        Status::OnHold => "On-Hold",
        Status::Dropped => "Dropped",
        Status::PlanToWatch => "Plan to Watch",
    }
}

/// `series_type` from MyAnimeList's `media_type` (`tv` → `TV`).
pub fn type_to_xml(format: &str) -> &'static str {
    match format {
        "tv" => "TV",
        "movie" => "Movie",
        "ova" => "OVA",
        "ona" => "ONA",
        "special" => "Special",
        "tv_special" => "TV Special",
        "music" => "Music",
        "cm" => "CM",
        "pv" => "PV",
        _ => "Unknown",
    }
}

fn format_from_type(t: &str) -> String {
    match t.trim().to_ascii_lowercase().as_str() {
        "" | "unknown" => String::new(),
        "tv special" => "tv_special".into(),
        other => other.to_string(),
    }
}

// --------------------------------------------------------------------------------------- export

/// Why the list of `service` can't be exported, if it can't.
pub fn export_refusal(service: ServiceId) -> Option<&'static str> {
    match service {
        ServiceId::Mal => None,
        ServiceId::AniList => Some("AniList\u{2019}s API terms don\u{2019}t allow exporting list data from AniList, so Kotora exports MyAnimeList lists only."),
    }
}

/// A written export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub xml: String,
    pub written: usize,
    /// Titles left out (no MyAnimeList id).
    pub skipped: Vec<String>,
}

/// The saved anime list of `service` as a MyAnimeList XML file (see the module notes).
pub fn export(store: &Store, service: ServiceId) -> Result<Export, String> {
    if let Some(why) = export_refusal(service) {
        return Err(why.into());
    }
    let name = service.name();
    let mut items = store.load_list(name).map_err(|e| e.to_string())?;
    items.sort_by(|a, b| a.media.title.to_lowercase().cmp(&b.media.title.to_lowercase()).then(a.media.id.cmp(&b.media.id)));
    let user = store.account(name).map_err(|e| e.to_string())?.unwrap_or_default();
    let mut skipped = Vec::new();
    let mut rows = Vec::new();
    for it in items {
        if it.media.id == 0 {
            skipped.push(it.media.title.clone());
            continue;
        }
        let mine = store.details(name, it.media.id).ok().flatten().and_then(|(d, _)| d.mine);
        let (start, finish) = mine.map_or((None, None), |m| (m.start_date, m.finish_date));
        rows.push(ExportRow { item: it, start, finish });
    }
    let xml = write(&user, &rows);
    Ok(Export { xml, written: rows.len(), skipped })
}

/// One entry to write.
pub struct ExportRow {
    pub item: ListItem,
    pub start: Option<String>,
    pub finish: Option<String>,
}

/// `2024-10` → `2024-10-00`, anything else odd → `0000-00-00`.
fn xml_date(d: Option<&str>) -> String {
    let Some(d) = d.map(str::trim).filter(|d| !d.is_empty()) else { return NO_DATE.into() };
    let parts: Vec<&str> = d.split('-').collect();
    let ok = |p: &&str, len: usize| p.len() == len && p.bytes().all(|b| b.is_ascii_digit());
    match parts.as_slice() {
        [y] if ok(y, 4) => format!("{y}-00-00"),
        [y, m] if ok(y, 4) && ok(m, 2) => format!("{y}-{m}-00"),
        [y, m, day] if ok(y, 4) && ok(m, 2) && ok(day, 2) => format!("{y}-{m}-{day}"),
        _ => NO_DATE.into(),
    }
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Control characters are not allowed in XML 1.0.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// `<![CDATA[…]]>`, split where the text itself holds `]]>`.
fn cdata(s: &str) -> String {
    let clean: String = s.chars().filter(|c| (*c as u32) >= 0x20 || matches!(c, '\t' | '\n' | '\r')).collect();
    format!("<![CDATA[{}]]>", clean.replace("]]>", "]]]]><![CDATA[>"))
}

/// The file, as MyAnimeList's export lays it out (tabs, one element per line).
pub fn write(user_name: &str, rows: &[ExportRow]) -> String {
    let count = |st: Status| rows.iter().filter(|r| r.item.status == st).count();
    let mut x = String::with_capacity(512 + rows.len() * 900);
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n");
    x.push_str("<!-- Written by Kotora (https://kotora.moe) in MyAnimeList's export format. -->\n");
    x.push_str("<myanimelist>\n\n\t<myinfo>\n");
    x.push_str("\t\t<user_id>0</user_id>\n");
    x.push_str(&format!("\t\t<user_name>{}</user_name>\n", esc(user_name)));
    x.push_str("\t\t<user_export_type>1</user_export_type>\n");
    x.push_str(&format!("\t\t<user_total_anime>{}</user_total_anime>\n", rows.len()));
    x.push_str(&format!("\t\t<user_total_watching>{}</user_total_watching>\n", count(Status::Watching)));
    x.push_str(&format!("\t\t<user_total_completed>{}</user_total_completed>\n", count(Status::Completed)));
    x.push_str(&format!("\t\t<user_total_onhold>{}</user_total_onhold>\n", count(Status::OnHold)));
    x.push_str(&format!("\t\t<user_total_dropped>{}</user_total_dropped>\n", count(Status::Dropped)));
    x.push_str(&format!("\t\t<user_total_plantowatch>{}</user_total_plantowatch>\n", count(Status::PlanToWatch)));
    x.push_str("\t</myinfo>\n\n");
    for r in rows {
        let it = &r.item;
        let m = &it.media;
        x.push_str("\t<anime>\n");
        x.push_str(&format!("\t\t<series_animedb_id>{}</series_animedb_id>\n", m.id));
        x.push_str(&format!("\t\t<series_title>{}</series_title>\n", cdata(&m.title)));
        x.push_str(&format!("\t\t<series_type>{}</series_type>\n", type_to_xml(&m.format)));
        x.push_str(&format!("\t\t<series_episodes>{}</series_episodes>\n", m.episodes.unwrap_or(0)));
        x.push_str("\t\t<my_id>0</my_id>\n");
        x.push_str(&format!("\t\t<my_watched_episodes>{}</my_watched_episodes>\n", it.progress));
        x.push_str(&format!("\t\t<my_start_date>{}</my_start_date>\n", xml_date(r.start.as_deref())));
        x.push_str(&format!("\t\t<my_finish_date>{}</my_finish_date>\n", xml_date(r.finish.as_deref())));
        x.push_str("\t\t<my_rated></my_rated>\n");
        x.push_str(&format!("\t\t<my_score>{}</my_score>\n", it.score.unwrap_or(0)));
        x.push_str("\t\t<my_storage></my_storage>\n");
        x.push_str("\t\t<my_storage_value>0.00</my_storage_value>\n");
        x.push_str(&format!("\t\t<my_status>{}</my_status>\n", status_to_xml(it.status)));
        // Kotora doesn't keep notes.
        x.push_str("\t\t<my_comments><![CDATA[]]></my_comments>\n");
        x.push_str(&format!("\t\t<my_times_watched>{}</my_times_watched>\n", it.times_rewatched));
        x.push_str("\t\t<my_rewatch_value></my_rewatch_value>\n");
        x.push_str("\t\t<my_priority>LOW</my_priority>\n");
        x.push_str("\t\t<my_tags><![CDATA[]]></my_tags>\n");
        x.push_str(&format!("\t\t<my_rewatching>{}</my_rewatching>\n", u8::from(it.rewatching)));
        x.push_str(&format!("\t\t<my_rewatching_ep>{}</my_rewatching_ep>\n", if it.rewatching { it.progress } else { 0 }));
        x.push_str("\t\t<my_discuss>1</my_discuss>\n");
        x.push_str("\t\t<my_sns>default</my_sns>\n");
        x.push_str("\t\t<update_on_import>1</update_on_import>\n");
        x.push_str("\t</anime>\n\n");
    }
    x.push_str("</myanimelist>\n");
    x
}

/// A file name for the save dialog: `animelist-2026-11-16.xml`.
pub fn default_file_name(now: i64) -> String {
    let (y, m, d) = crate::time::civil_from_days(now.div_euclid(86_400));
    format!("animelist-{y:04}-{m:02}-{d:02}.xml")
}

// ---------------------------------------------------------------------------------------- import

/// What one entry of the file means for the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Not on the list: an add.
    New,
    /// On the list with other values.
    Different,
    /// On the list with the same values.
    Same,
    /// Can't be imported, and why.
    Unknown(String),
}

/// A status the plan can't settle alone: both sides hold one, and neither is clearly further
/// along (Dropped or On hold against anything else). The user picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusConflict {
    pub mine: Status,
    pub file: Status,
}

/// One entry of the file, compared with the list.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanRow {
    pub entry: XmlEntry,
    /// The anime's id on the active service (`None`: unknown).
    pub target: Option<u32>,
    /// New: the media row to add (titles as the service or the file give them).
    pub media: Option<Media>,
    /// The entry on the list now.
    pub current: Option<Values>,
    pub verdict: Verdict,
    /// Different: what an update sends without a status conflict decided (may be empty: the
    /// list is already further along).
    pub update: Change,
    /// Different: the status the user must pick, if any.
    pub conflict: Option<StatusConflict>,
    /// Different: what an update sends when the user takes the file's status.
    pub with_file_status: Change,
}

impl PlanRow {
    /// The title to show: the list's, the service's, else the file's.
    pub fn title(&self) -> &str {
        self.media.as_ref().map(|m| m.title.as_str()).filter(|t| !t.is_empty()).unwrap_or(&self.entry.title)
    }
}

/// The whole comparison.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub service: String,
    pub rows: Vec<PlanRow>,
    /// `<manga>` entries in the file (not imported).
    pub manga: usize,
}

/// What the user chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    NewOnly,
    NewAndDifferent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub new: usize,
    pub different: usize,
    /// Different, with something to send (the rest: the list is already further along).
    pub to_update: usize,
    pub same: usize,
    pub unknown: usize,
    pub conflicts: usize,
}

impl Plan {
    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for r in &self.rows {
            match r.verdict {
                Verdict::New => c.new += 1,
                Verdict::Different => {
                    c.different += 1;
                    if r.conflict.is_some() {
                        c.conflicts += 1;
                    }
                    if !r.update.is_empty() || r.conflict.is_some() {
                        c.to_update += 1;
                    }
                }
                Verdict::Same => c.same += 1,
                Verdict::Unknown(_) => c.unknown += 1,
            }
        }
        c
    }

    pub fn conflicts(&self) -> impl Iterator<Item = &PlanRow> {
        self.rows.iter().filter(|r| r.conflict.is_some())
    }

    /// The MyAnimeList ids of the file that are not on the list, for the service to map
    /// (AniList). Entries without an id or a known status are left out.
    pub fn ids_to_map(file: &XmlList, list: &[ListItem], service: &str) -> Vec<u32> {
        let on_list: HashSet<u32> = list.iter().filter_map(|it| mal_id_of(service, &it.media)).collect();
        let mut seen = HashSet::new();
        file.entries.iter().filter(|e| e.mal_id > 0 && e.status.is_some() && !on_list.contains(&e.mal_id)).map(|e| e.mal_id).filter(|id| seen.insert(*id)).collect()
    }
}

/// An entry's MyAnimeList id: its own id on MyAnimeList, `idMal` elsewhere.
fn mal_id_of(service: &str, m: &Media) -> Option<u32> {
    if service == ServiceId::Mal.name() { Some(m.id) } else { m.id_mal.filter(|id| *id > 0) }
}

/// How far along a status is, where that is clear: Plan to watch, Watching, Completed, then a
/// rewatch. On hold and Dropped have no place: against anything else they are a conflict.
fn rank(v: &Values) -> Option<u8> {
    match (v.status, v.rewatching) {
        (Status::PlanToWatch, _) => Some(0),
        (Status::Watching, _) => Some(1),
        (Status::Completed, false) => Some(2),
        (Status::Completed, true) => Some(3),
        _ => None,
    }
}

/// The update for an entry on the list (`cur`) from the file (`file`), and a status conflict if
/// there is one. Rules (ROADMAP "merge"): the status further along wins (unclear pairs are
/// asked); the progress further along wins when the status stays; a score fills an empty one,
/// a score already there is kept; the rewatch count takes the higher.
pub(crate) fn resolve(cur: &Values, file: &Values, episodes: Option<u32>) -> (Change, Option<StatusConflict>, Change) {
    let same_slot = cur.status == file.status && cur.rewatching == file.rewatching;
    let higher = |mut v: Values| {
        v.times_rewatched = cur.times_rewatched.max(file.times_rewatched);
        if v.score.is_none() {
            v.score = file.score.or(cur.score);
        }
        v
    };
    // The file's status (and its progress with it).
    let take_file = {
        let mut v = higher(Values { score: cur.score, ..*file });
        if v.status == Status::Completed && !v.rewatching
            && let Some(n) = episodes
        {
            v.progress = n;
        }
        v
    };
    let keep_mine = higher(*cur);
    let change_to = |v: Values| Change {
        status: Some(v.status),
        progress: Some(v.progress),
        score: Some(v.score),
        rewatching: Some(v.rewatching),
        times_rewatched: Some(v.times_rewatched),
        volumes: None,
    }
    .without_same(cur);
    if same_slot {
        let v = Values { progress: cur.progress.max(file.progress), ..keep_mine };
        return (change_to(v), None, change_to(v));
    }
    match (rank(cur), rank(file)) {
        (Some(a), Some(b)) if b > a => (change_to(take_file), None, change_to(take_file)),
        (Some(_), Some(_)) => (change_to(keep_mine), None, change_to(take_file)),
        _ => (change_to(keep_mine), Some(StatusConflict { mine: cur.status, file: file.status }), change_to(take_file)),
    }
}

/// Compares `file` with the saved `list` of `service`. `mapped`: for a service other than
/// MyAnimeList, the service's media for MyAnimeList ids not on the list (by MyAnimeList id).
pub fn plan(file: &XmlList, service: &str, list: &[ListItem], mapped: &HashMap<u32, Media>) -> Plan {
    let is_mal = service == ServiceId::Mal.name();
    let title = ServiceId::from_name(service).map_or("the service", ServiceId::title);
    let by_mal: HashMap<u32, &ListItem> = list.iter().filter_map(|it| Some((mal_id_of(service, &it.media)?, it))).collect();
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for e in &file.entries {
        let mut row = PlanRow {
            entry: e.clone(),
            target: None,
            media: None,
            current: None,
            verdict: Verdict::New,
            update: Change::default(),
            conflict: None,
            with_file_status: Change::default(),
        };
        let unknown = |mut row: PlanRow, why: String| {
            row.verdict = Verdict::Unknown(why);
            row
        };
        if e.mal_id == 0 {
            rows.push(unknown(row, "No MyAnimeList id".into()));
            continue;
        }
        let Some(want) = e.values() else {
            rows.push(unknown(row, format!("Unknown status \u{201c}{}\u{201d}", e.raw_status)));
            continue;
        };
        if !seen.insert(e.mal_id) {
            rows.push(unknown(row, "Twice in the file".into()));
            continue;
        }
        if let Some(it) = by_mal.get(&e.mal_id) {
            let cur = it.values();
            row.target = Some(it.media.id);
            row.media = Some(it.media.clone());
            row.current = Some(cur);
            let same = cur.status == want.status && cur.progress == want.progress && cur.score == want.score && cur.rewatching == want.rewatching && cur.times_rewatched == want.times_rewatched;
            if same {
                row.verdict = Verdict::Same;
            } else {
                row.verdict = Verdict::Different;
                if e.update_on_import {
                    let (update, conflict, with_file) = resolve(&cur, &want, it.media.episodes.or(e.episodes));
                    row.update = update;
                    row.conflict = conflict;
                    row.with_file_status = with_file;
                }
            }
            rows.push(row);
            continue;
        }
        let media = if is_mal { Some(e.media()) } else { mapped.get(&e.mal_id).cloned() };
        match media {
            Some(m) => {
                row.target = Some(m.id);
                row.media = Some(m);
                rows.push(row);
            }
            None => rows.push(unknown(row, format!("Not on {title}"))),
        }
    }
    Plan { service: service.to_string(), rows, manga: file.manga }
}

/// One step of applying a plan: what `Store::import` does for one entry.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Put `media` on the list with these values.
    Add { media: Box<Media>, values: Values },
    /// Change the entry `media_id`.
    Update { media_id: u32, change: Change },
}

/// The steps for `mode`. `use_file_status`: target ids whose status conflict the user settled
/// for the file's status (the others keep the list's).
pub fn actions(plan: &Plan, mode: Mode, use_file_status: &HashSet<u32>) -> Vec<Action> {
    let mut out = Vec::new();
    for r in &plan.rows {
        let Some(id) = r.target else { continue };
        match r.verdict {
            Verdict::New => {
                if let (Some(media), Some(values)) = (r.media.clone(), r.entry.values()) {
                    out.push(Action::Add { media: Box::new(media), values });
                }
            }
            Verdict::Different if mode == Mode::NewAndDifferent => {
                let change = if r.conflict.is_some() && use_file_status.contains(&id) { r.with_file_status } else { r.update };
                if !change.is_empty() {
                    out.push(Action::Update { media_id: id, change });
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests;
