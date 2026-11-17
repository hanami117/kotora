//! `kot`: the command line for a running Kotora.
//!
//! ```text
//! kot            a dashboard: what plays, the Watching list and what comes next, the sync queue
//! kot status     the app's state in two lines (`--short`: one)
//! kot list       the Watching list (`kot list all`, `kot list completed`, …)
//! kot +1 TITLE [--anyway]   one more episode
//! kot set TITLE EP [--status S] [--score SCORE|none] [--anyway]
//! kot queue      changes waiting to sync
//! kot sync       send the queue now, then read the list again
//! kot airing     your anime airing in the next 7 days (also `kot schedule`)
//! kot publishing your Reading manga's latest chapters (MangaUpdates; also `kot chapters`)
//! kot now        what plays, and the episode Kotora offers to record
//! kot confirm    record the offered episode (the banner's Update)
//! kot dismiss    not now
//! kot add QUERY|ID [--status S]
//! kot remove TITLE|ID [--yes]  take it off the list (asks first)
//! kot next TITLE  play the next episode from the library folders
//! kot config client-id ID
//! kot show       shows the window
//! kot quit       quits the app
//! kot --version
//! ```
//!
//! A TITLE is any part of a title (English and Japanese titles too), without quotes; it must
//! match one entry.

use std::io;
use std::process::ExitCode;

mod dash;

use kotora_ctl::{AiringItem, Client, Entry, NowInfo, PublishingItem, QueueItem, ReadingInfo, Reply, Request, Status};

const HELP: &str = "\
kot: talk to a running Kotora

Usage: kot [COMMAND]

Commands:
  (none)                a dashboard: what plays, your Watching list with
                        what comes next, the sync queue
  status [--short]      what Kotora is doing (--short: one line)
  list [STATUS|all]     your list; STATUS: watching (the default), plan,
                        completed, on-hold, dropped
  +1 TITLE              one more episode of TITLE
  set TITLE EP          set the episodes watched
      [--status STATUS] [--score SCORE|none]
                        SCORE in your score format: 7 (MyAnimeList),
                        or on AniList 73, 7.3, *** (stars), :) (smileys)
  set --manga TITLE|ID [CH]
      [--volumes N] [--status STATUS] [--score SCORE|none]
                        set a manga's chapters read (CH), volumes, status
                        or score; at least one. TITLE|ID: a title, or the
                        manga's id when it is only digits (your list
                        service's id; on AniList also the MyAnimeList id).
                        The last word is CH when it is a number:
                        kot set --manga 24 258 = manga 24, chapter 258
  --anyway              with +1 and set: mark an episode that hasn't
                        aired yet (by the estimate) without asking
  queue                 changes waiting to sync
  sync                  send the queue now, then read the list again
  now                   what plays, and the episode Kotora offers
  airing, schedule      your anime airing in the next 7 days (estimated)
  publishing, chapters  your Reading manga: latest chapter, to read, next
                        release (estimated). Release data: MangaUpdates
  confirm               record the offered episode
  dismiss               not now: hide the offer for this file
  rewatch TITLE|ID      watch a completed anime again from episode 1
  next TITLE            play the next episode of TITLE from your
                        library folders (Settings \u{2192} Library)
  remove TITLE|ID       take an anime off your list (asks first; --yes
      [--yes] [--manga]   doesn't; --manga: a manga). Undo it in Kotora's
                          sync queue
  add QUERY|ID          put an anime on your list: a title to search
      [--status STATUS]   your list service for (it must find one), or its id;
      [--manga]           --manga: a manga (Manga must be on)
  list --manga [STATUS|all]
                        your manga list (read only); STATUS:
                        reading (the default), plan, completed, on-hold,
                        dropped. Needs manga on
  manga [on|off]        whether syncs also read your manga list (MyAnimeList
                        or AniList; off by default); on syncs now
  fill-volumes [--yes]  set manga volumes read from chapters read, with
                        MangaDex's volume tables: prints what would change
                        and asks first (--yes doesn't); never lowers. Volume
                        data: MangaDex
  export [--manga] FILE write your anime list (--manga: your manga list) as a
                        MyAnimeList XML file (MyAnimeList lists only), as
                        Settings \u{2192} Account \u{2192} Export anime/manga list\u{2026}
  config client-id ID   use your own Client ID for your list service
                        (MyAnimeList or AniList, as chosen in Kotora; optional)
  show                  show the window
  quit                  quit Kotora
  help                  this text

TITLE is any part of a title, no quotes needed; it must match one entry.
Changes go to the sync queue first, then to MyAnimeList or AniList.

Options:
  -V, --version   print the version
";

/// `file` as an absolute path: the app runs elsewhere, so a relative path means here.
fn absolute(file: &str) -> String {
    let p = std::path::Path::new(file);
    if p.is_absolute() {
        return file.to_string();
    }
    std::env::current_dir().map(|d| d.join(p).to_string_lossy().into_owned()).unwrap_or_else(|_| file.to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let req = match words.as_slice() {
        [] => Request::Dash,
        ["status"] | ["status", "--short"] => Request::Status,
        ["show"] => Request::Show,
        ["quit"] => Request::Quit,
        ["queue"] => Request::Queue,
        ["sync"] => Request::Sync,
        ["now"] => Request::Now,
        ["airing" | "schedule"] => Request::Airing,
        ["publishing" | "chapters"] => Request::Publishing,
        ["confirm"] => Request::Confirm,
        ["dismiss"] => Request::Dismiss,
        ["list", "--manga"] | ["--manga", "list"] => Request::MangaList { status: Some("watching".into()) },
        ["list", "--manga", "all"] | ["list", "all", "--manga"] => Request::MangaList { status: None },
        ["list", "--manga", st] | ["list", st, "--manga"] => Request::MangaList { status: Some((*st).to_string()) },
        ["manga"] => Request::Manga { on: None },
        ["manga", "on"] => Request::Manga { on: Some(true) },
        ["manga", "off"] => Request::Manga { on: Some(false) },
        ["manga", ..] => {
            eprintln!("kot: use kot manga [on|off] (try kot help)");
            return ExitCode::from(2);
        }
        ["list"] => Request::List { status: Some("watching".into()) },
        ["list", "all"] => Request::List { status: None },
        ["list", st] => Request::List { status: Some((*st).to_string()) },
        ["+1", rest @ ..] => match parse_plus_one(rest) {
            Ok(r) => r,
            Err(m) => {
                eprintln!("kot: {m} (try kot help)");
                return ExitCode::from(2);
            }
        },
        ["rewatch", title @ ..] if !title.is_empty() => Request::Rewatch { title: title.join(" ") },
        ["remove", rest @ ..] => match parse_remove(rest) {
            Ok(r) => r,
            Err(m) => {
                eprintln!("kot: {m} (try kot help)");
                return ExitCode::from(2);
            }
        },
        ["next", title @ ..] if !title.is_empty() => Request::Next { title: title.join(" ") },
        ["fill-volumes"] => Request::FillVolumes { yes: false },
        ["fill-volumes", "--yes"] | ["fill-volumes", "-y"] => Request::FillVolumes { yes: true },
        ["export", "--manga", file] | ["export", file, "--manga"] if !file.starts_with("--") => Request::ExportManga { path: absolute(file) },
        ["export", file] if !file.starts_with("--") => Request::Export { path: absolute(file) },
        ["export", ..] => {
            eprintln!("kot: use kot export [--manga] FILE (try kot help)");
            return ExitCode::from(2);
        }
        ["config", "client-id", id] => Request::SetClientId { id: (*id).to_string() },
        ["config", ..] => {
            eprintln!("kot: use kot config client-id ID (try kot help)");
            return ExitCode::from(2);
        }
        ["add", rest @ ..] => match parse_add(rest) {
            Ok(r) => r,
            Err(m) => {
                eprintln!("kot: {m} (try kot help)");
                return ExitCode::from(2);
            }
        },
        ["set", rest @ ..] => match parse_set(rest) {
            Ok(r) => r,
            Err(m) => {
                eprintln!("kot: {m} (try kot help)");
                return ExitCode::from(2);
            }
        },
        ["help" | "-h" | "--help"] => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        ["-V" | "--version"] => {
            println!("kot {}", kotora_ctl::version_line());
            return ExitCode::SUCCESS;
        }
        _ => {
            eprintln!("kot: unknown command '{}' (try kot help)", args.join(" "));
            return ExitCode::from(2);
        }
    };
    let mut reply = ask(&req);
    // Held: the episode hasn't aired yet by the estimate, or a removal asks first. Ask on a
    // terminal, else say how.
    if let Ok(Reply::Error { message, anyway: true }) = &reply {
        eprintln!("kot: {message}");
        let removing = matches!(req, Request::Remove { .. } | Request::MangaRemove { .. });
        let filling = matches!(req, Request::FillVolumes { .. });
        let question = if filling { "Fill them? [y/N] " } else if removing { "Remove it? [y/N] " } else { "Mark anyway? [y/N] " };
        if !confirm(question) {
            if !interactive() {
                eprintln!(
                    "{}",
                    if filling {
                        "kot: run it again with --yes to fill them."
                    } else if removing {
                        "kot: run it again with --yes to remove it."
                    } else {
                        "kot: run it again with --anyway to mark it."
                    }
                );
            }
            return ExitCode::FAILURE;
        }
        reply = ask(&with_anyway(req.clone()));
    }
    // The dashboard needs a newer app, or a signed-in one: fall back to the status lines.
    if req == Request::Dash && matches!(reply, Ok(Reply::Error { .. })) {
        reply = ask(&Request::Status);
    }
    match reply {
        Ok(Reply::Dash(d)) => {
            let color = stdout_is_tty() && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
            print!("{}", dash::render(&d, terminal_width(), color));
            ExitCode::SUCCESS
        }
        Ok(Reply::Status(st)) => {
            if words == ["status", "--short"] {
                println!("{}", status_short(&st));
            } else if req == Request::Sync {
                println!("{}", sync_text(&st));
            } else {
                print!("{}", status_text(&st));
            }
            ExitCode::SUCCESS
        }
        Ok(Reply::List { entries, note }) => {
            print!("{}", list_text(&entries, matches!(req, Request::List { status: None } | Request::MangaList { status: None })));
            if let Some(n) = note {
                eprintln!("kot: {n}");
            }
            ExitCode::SUCCESS
        }
        Ok(Reply::Edited { entry }) => {
            println!("{}", edited_text(&entry));
            ExitCode::SUCCESS
        }
        Ok(Reply::Queue { ops }) => {
            print!("{}", queue_text(&ops));
            ExitCode::SUCCESS
        }
        Ok(Reply::Now { now, reading }) => {
            print!("{}", now_text(now.as_ref()));
            if let Some(r) = &reading {
                print!("{}", reading_text(r));
            }
            ExitCode::SUCCESS
        }
        Ok(Reply::Airing { items, note }) => {
            print!("{}", airing_text(&items, note.as_deref()));
            ExitCode::SUCCESS
        }
        Ok(Reply::Publishing { items, note }) => {
            print!("{}", publishing_text(&items, note.as_deref()));
            ExitCode::SUCCESS
        }
        Ok(Reply::Done { message }) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Ok(Reply::Error { message, .. }) if message.starts_with("bad request") => {
            eprintln!("kot: this Kotora is older than kot and doesn't know that command; update Kotora");
            ExitCode::FAILURE
        }
        Ok(Reply::Error { message, .. }) => {
            eprintln!("kot: {message}");
            ExitCode::FAILURE
        }
        Err(e) if matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => {
            eprintln!("kot: Kotora isn't running");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("kot: {e}");
            ExitCode::FAILURE
        }
    }
}

fn ask(req: &Request) -> io::Result<Reply> {
    let mut c = Client::connect()?;
    // Looking manga up on MangaDex can take minutes on a long list (under 3 requests a second).
    if matches!(req, Request::FillVolumes { .. }) {
        c.set_reply_timeout(std::time::Duration::from_secs(600))?;
    }
    c.request(req)
}

fn stdout_is_tty() -> bool {
    use std::io::IsTerminal;
    io::stdout().is_terminal()
}

/// The terminal's columns: asked from stdout, else `$COLUMNS`, else 80 (a pipe).
fn terminal_width() -> usize {
    if stdout_is_tty()
        && let Some(cols) = console_width()
    {
        return cols;
    }
    std::env::var("COLUMNS").ok().and_then(|v| v.parse().ok()).filter(|&n| n > 0).unwrap_or(80)
}

/// The columns of the terminal on stdout, from the system.
#[cfg(unix)]
fn console_width() -> Option<usize> {
    let mut ws = libc::winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: TIOCGWINSZ fills the `winsize` we pass; stdout is a terminal.
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) } == 0;
    (ok && ws.ws_col > 0).then(|| usize::from(ws.ws_col))
}

/// The console window's width on Windows (the visible columns, not the buffer's).
#[cfg(windows)]
fn console_width() -> Option<usize> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Console::{CONSOLE_SCREEN_BUFFER_INFO, GetConsoleScreenBufferInfo};
    // SAFETY: an all-zero CONSOLE_SCREEN_BUFFER_INFO is a valid plain-data value; the call fills it
    // for stdout's handle, and fails (returns 0) when stdout isn't a console.
    let mut info: CONSOLE_SCREEN_BUFFER_INFO = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetConsoleScreenBufferInfo(io::stdout().as_raw_handle() as _, &mut info) } != 0;
    let cols = i32::from(info.srWindow.Right) - i32::from(info.srWindow.Left) + 1;
    (ok && cols > 0).then_some(cols as usize)
}

fn interactive() -> bool {
    use std::io::IsTerminal;
    io::stdin().is_terminal() && io::stderr().is_terminal()
}

/// A yes/no question on the terminal; no terminal (a script): no.
fn confirm(question: &str) -> bool {
    use std::io::Write;
    if !interactive() {
        return false;
    }
    eprint!("{question}");
    let _ = io::stderr().flush();
    let mut line = String::new();
    io::stdin().read_line(&mut line).is_ok() && matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

/// The same request, marked "anyway".
fn with_anyway(req: Request) -> Request {
    match req {
        Request::PlusOne { title, .. } => Request::PlusOne { title, anyway: true },
        Request::Set { title, progress, status, score, score_text, .. } => Request::Set { title, progress, status, score, score_text, anyway: true },
        Request::Remove { title, .. } => Request::Remove { title, yes: true },
        Request::MangaRemove { title, .. } => Request::MangaRemove { title, yes: true },
        Request::FillVolumes { .. } => Request::FillVolumes { yes: true },
        other => other,
    }
}

/// `remove TITLE…|ID [--yes] [--manga]`.
fn parse_remove(words: &[&str]) -> Result<Request, String> {
    let yes = words.iter().any(|w| matches!(*w, "--yes" | "-y"));
    let manga = words.contains(&"--manga");
    let title: Vec<&str> = words.iter().copied().filter(|w| !matches!(*w, "--yes" | "-y" | "--manga")).collect();
    if title.is_empty() {
        return Err("remove needs a title or an id".into());
    }
    let title = title.join(" ");
    Ok(if manga { Request::MangaRemove { title, yes } } else { Request::Remove { title, yes } })
}

/// `+1 TITLE… [--anyway]`.
fn parse_plus_one(words: &[&str]) -> Result<Request, String> {
    let anyway = words.contains(&"--anyway");
    let title: Vec<&str> = words.iter().copied().filter(|w| *w != "--anyway").collect();
    if title.is_empty() {
        return Err("+1 needs a title".into());
    }
    Ok(Request::PlusOne { title: title.join(" "), anyway })
}

/// Two lines: the version, then the account and the list.
fn status_text(st: &Status) -> String {
    format!("Kotora {}\n{}\n", st.version, status_short(st))
}

/// The second line of `status_text` on its own: the account and the list.
fn status_short(st: &Status) -> String {
    let who = match &st.account {
        Some(name) if st.service.is_empty() => name.clone(),
        Some(name) => match &st.mirror {
            Some(m) => format!("{name} on {} \u{21c4} {}", st.service, m.to),
            None => format!("{name} on {}", st.service),
        },
        None => "not signed in".to_string(),
    };
    let queue = match st.queue {
        0 => "synced".to_string(),
        1 => "1 change waiting".to_string(),
        n => format!("{n} changes waiting"),
    };
    let mut line = format!("{who} · {} {} · {queue}", st.entries, if st.entries == 1 { "entry" } else { "entries" });
    if let Some(m) = &st.mirror {
        line.push_str(" · mirror: ");
        line.push_str(&mirror_short(m));
    }
    if st.offline {
        line.push_str(" · offline");
    }
    if !st.window {
        line.push_str(" · hidden");
    }
    line
}

/// The mirror part of the status line: "synced", or what is not on the mirror yet.
fn mirror_short(m: &kotora_ctl::MirrorStatus) -> String {
    let plural = |n: u32, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let parts: Vec<String> = [
        (m.waiting > 0).then(|| format!("{} waiting", m.waiting)),
        (m.conflicts > 0).then(|| plural(m.conflicts, "conflict", "conflicts")),
        (m.failed > 0).then(|| format!("{} failed", m.failed)),
        (m.web_edits > 0).then(|| plural(m.web_edits, "website edit", "website edits")),
    ]
    .into_iter()
    .flatten()
    .collect();
    if parts.is_empty() { "synced".to_string() } else { parts.join(", ") }
}

/// `set TITLE… [EP] [--status S] [--score SCORE|none]`: the last bare number is the episode.
/// The score goes as typed: the app reads it in the user's score format.
fn parse_set(words: &[&str]) -> Result<Request, String> {
    let (mut title, mut progress, mut status, mut score_text, mut anyway) = (Vec::new(), None, None, None, false);
    let (mut manga, mut volumes) = (false, None);
    let mut i = 0;
    while i < words.len() {
        match words[i] {
            "--manga" => manga = true,
            "--volumes" | "--vol" => {
                let v = words.get(i + 1).ok_or("--volumes needs a number")?;
                volumes = Some(v.parse::<u32>().map_err(|_| format!("--volumes needs a number, not \u{201c}{v}\u{201d}"))?);
                i += 1;
            }
            "--status" => {
                status = Some(words.get(i + 1).ok_or("--status needs a status")?.to_string());
                i += 1;
            }
            "--score" => {
                let v = words.get(i + 1).ok_or("--score needs a score (in your score format), or none")?;
                score_text = Some(v.to_string());
                i += 1;
            }
            "--anyway" => anyway = true,
            w => title.push(w),
        }
        i += 1;
    }
    // The last word is the episode if it is a number and something is left for the title.
    if title.len() > 1
        && let Some(n) = title.last().and_then(|w| w.parse::<u32>().ok())
    {
        progress = Some(n);
        title.pop();
    }
    if title.is_empty() {
        return Err("set needs a title".into());
    }
    if volumes.is_some() && !manga {
        return Err("--volumes is for manga: add --manga".into());
    }
    if manga {
        if progress.is_none() && volumes.is_none() && status.is_none() && score_text.is_none() {
            return Err("set --manga needs a chapter number, --volumes, --status or --score".into());
        }
        return Ok(Request::MangaSet { title: title.join(" "), progress, volumes, status, score_text });
    }
    if progress.is_none() && status.is_none() && score_text.is_none() {
        return Err("set needs an episode number, --status or --score".into());
    }
    Ok(Request::Set { title: title.join(" "), progress, status, score: None, score_text, anyway })
}

/// `add QUERY… [--status S]`.
fn parse_add(words: &[&str]) -> Result<Request, String> {
    let (mut query, mut status, mut manga) = (Vec::new(), None, false);
    let mut i = 0;
    while i < words.len() {
        match words[i] {
            "--manga" => manga = true,
            "--status" => {
                status = Some(words.get(i + 1).ok_or("--status needs a status")?.to_string());
                i += 1;
            }
            w => query.push(w),
        }
        i += 1;
    }
    if query.is_empty() {
        return Err("add needs a title or an id".into());
    }
    let query = query.join(" ");
    Ok(if manga { Request::MangaAdd { query, status } } else { Request::Add { query, status } })
}

fn progress_text(e: &Entry) -> String {
    let n = |c: Option<u32>| c.map_or("?".to_string(), |n| n.to_string());
    match e.volumes {
        // Manga: chapters, volumes or both (`manga_shows`).
        Some(v) => {
            let (ch, vol) = manga_shows(e.progress, v);
            let parts = [ch.then(|| format!("ch {} / {}", e.progress, n(e.episodes))), vol.then(|| format!("vol {v} / {}", n(e.volumes_total)))];
            parts.into_iter().flatten().collect::<Vec<_>>().join("  ")
        }
        None => format!("{} / {}", e.progress, n(e.episodes)),
    }
}

/// Which counts a manga shows (the app's rule too): chapters when read, volumes when read, both
/// when both are; nothing read yet shows chapters.
fn manga_shows(chapters: u32, volumes: u32) -> (bool, bool) {
    (chapters > 0 || volumes == 0, volumes > 0)
}

/// The progress cells of a list, one string each, laid out in columns: episodes right-aligned;
/// manga as a chapters column and a volumes column (either may be blank on a row), each number
/// aligned on the slash.
/// A manga's (read, total) texts of its chapters and volumes, each when shown.
type MangaParts = [Option<(String, String)>; 2];

fn progress_cells(entries: &[Entry]) -> Vec<String> {
    let n = |c: Option<u32>| c.map_or("?".to_string(), |n| n.to_string());
    // (read, total) of a manga's chapters and volumes, when shown.
    let parts: Vec<Option<MangaParts>> = entries
        .iter()
        .map(|e| {
            let v = e.volumes?;
            let (ch, vol) = manga_shows(e.progress, v);
            Some([ch.then(|| (e.progress.to_string(), n(e.episodes))), vol.then(|| (v.to_string(), n(e.volumes_total)))])
        })
        .collect();
    // Per manga column: the widest read count and total, or `None` when no row shows it.
    let width = |i: usize| -> Option<(usize, usize)> {
        parts.iter().flatten().filter_map(|p| p[i].as_ref()).map(|(a, b)| (a.chars().count(), b.chars().count())).reduce(|x, y| (x.0.max(y.0), x.1.max(y.1)))
    };
    let widths = [width(0), width(1)];
    let words = ["ch", "vol"];
    let cells: Vec<String> = entries
        .iter()
        .zip(&parts)
        .map(|(e, p)| match p {
            None => format!("{} / {}", e.progress, n(e.episodes)),
            Some(p) => {
                let mut cols = Vec::new();
                for i in 0..2 {
                    let Some((a, b)) = widths[i] else { continue };
                    let full = words[i].len() + 1 + a + 3 + b;
                    cols.push(match &p[i] {
                        Some((r, t)) => format!("{} {r:>a$} / {t:<b$}", words[i]),
                        None => " ".repeat(full),
                    });
                }
                cols.join("  ")
            }
        })
        .collect();
    // Right-aligned in one column (episodes and the manga block alike).
    let w = cells.iter().map(|c| c.chars().count()).max().unwrap_or(0).max(9);
    cells.into_iter().map(|c| format!("{c:>w$}")).collect()
}

/// One line per entry: progress, score, title, and the queue state. `grouped` adds a heading
/// per status (`kot list all`).
fn list_text(entries: &[Entry], grouped: bool) -> String {
    if entries.is_empty() {
        return "Nothing here.\n".into();
    }
    let mut out = String::new();
    let mut last = "";
    // The score column fits the widest score ("73", "7.3", "★★★★☆").
    let width = entries.iter().filter_map(score_text).map(|t| t.chars().count()).max().unwrap_or(1).max(2);
    let progress = progress_cells(entries);
    for (e, p) in entries.iter().zip(&progress) {
        if grouped && e.status != last {
            if !last.is_empty() {
                out.push('\n');
            }
            let n = entries.iter().filter(|x| x.status == e.status).count();
            out.push_str(&format!("{} ({n})\n", e.status));
            last = &e.status;
        }
        let score = score_text(e).unwrap_or_else(|| "\u{2013}".to_string());
        let mut line = format!("  {p}  {score:>w$}  {}", e.title, w = width);
        if e.rewatching {
            line.push_str(if e.volumes.is_some() { "  (rereading)" } else { "  (rewatching)" });
        }
        if let Some(w) = &e.waiting {
            line.push_str(&format!("  ({})", w.to_lowercase()));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// The score as the app wrote it in the user's format, else (an older app) its 1-10.
fn score_text(e: &Entry) -> Option<String> {
    e.score_text.clone().or_else(|| e.score.map(|n| n.to_string()))
}

fn edited_text(e: &Entry) -> String {
    let score = score_text(e).map_or("no score".to_string(), |t| format!("score {t}"));
    let state = e.waiting.as_deref().map_or("synced".to_string(), str::to_lowercase);
    let status = if e.rewatching { format!("{}, rewatching", e.status) } else { e.status.clone() };
    format!("{}: {} · {status} · {score} ({state})", e.title, progress_text(e))
}

fn queue_text(ops: &[QueueItem]) -> String {
    if ops.is_empty() {
        return "Nothing waiting.\n".into();
    }
    let w = ops.iter().map(|o| o.title.chars().count()).max().unwrap_or(0).min(40);
    let mut out = String::new();
    for o in ops {
        let title: String = o.title.chars().take(40).collect();
        let mut line = format!("{:<9} {title:<w$}  {}", o.state, o.change);
        if let Some(n) = &o.note {
            line.push_str(&format!(" · {n}"));
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn now_text(now: Option<&NowInfo>) -> String {
    let Some(n) = now else { return "Nothing is playing.\n".into() };
    let state = if n.playing { "Playing" } else { "Paused" };
    let pct = n.percent.map(|p| format!(", {p}%")).unwrap_or_default();
    let ep = n.episode.map(|e| format!(" episode {e}")).unwrap_or_default();
    let mut out = format!("{state} in {} ({}{pct}): {}{ep}\n  {}\n  {}\n", n.player, n.via, n.title, n.file, n.matched);
    out.push_str(&match (n.recorded, n.offer) {
        (Some(ep), _) => format!("  Recorded episode {ep}.\n"),
        (None, Some(ep)) if n.dismissed => format!("  Episode {ep} offered; you said not now (kot confirm records it).\n"),
        (None, Some(ep)) if n.reached => format!("  Update to episode {ep}? kot confirm\n"),
        (None, Some(ep)) => format!("  Episode {ep} is offered once it counts as watched (kot confirm records it now).\n"),
        (None, None) => "  Nothing to record.\n".into(),
    });
    if let (None, Some(note)) = (n.recorded, &n.note) {
        out.push_str(&format!("  {note}\n"));
    }
    out
}

/// The comic being read (Manga reading detection), after what plays.
fn reading_text(r: &ReadingInfo) -> String {
    let pages = match (r.page, r.pages) {
        _ if r.finished => " (read to the end)".to_string(),
        (Some(p), Some(n)) if n > 0 => format!(" (page {p} of {n})"),
        _ => String::new(),
    };
    let lead = if r.open { "Reading in" } else { "Closed in" };
    let mut out = format!("{lead} {}: {} {}{pages}\n  {}\n  {}\n", r.reader, r.title, r.label, r.file, r.matched);
    out.push_str(&match (&r.recorded, &r.offer) {
        (Some(what), _) => format!("  Marked {what} as read.\n"),
        (None, Some(what)) if r.dismissed => format!("  {what} offered; you said not now (kot confirm marks it).\n"),
        (None, Some(what)) if r.finished => format!("  Mark {what} as read? kot confirm\n"),
        (None, Some(what)) => format!("  {what} is offered once it is read to the end (kot confirm marks it now).\n"),
        (None, None) => "  Nothing to mark.\n".into(),
    });
    if let (None, Some(note)) = (&r.recorded, &r.note) {
        out.push_str(&format!("  {note}\n"));
    }
    out
}

/// Grouped by day: "Today", then "  15:00  Mecha Bento Club  ep 11  (in 6 h)".
fn airing_text(items: &[AiringItem], note: Option<&str>) -> String {
    if items.is_empty() {
        return format!("{}\n", note.unwrap_or("Nothing on your list airs in the next 7 days."));
    }
    let mut out = String::from("Airing on your list, next 7 days (some times are estimates):\n");
    let w = items.iter().map(|i| i.title.chars().count()).max().unwrap_or(0).min(40);
    let mut day = "";
    for i in items {
        if i.day != day {
            day = &i.day;
            out.push_str(&format!("{day}\n"));
        }
        let title: String = i.title.chars().take(40).collect();
        let behind = if i.behind > 0 { format!(", {}{} to watch", if i.estimate { "~" } else { "" }, i.behind) } else { String::new() };
        out.push_str(&format!("  {}  {title:<w$}  ep {}  ({}{behind})\n", i.time, i.episode, i.countdown));
    }
    out
}

fn publishing_text(items: &[PublishingItem], note: Option<&str>) -> String {
    if items.is_empty() {
        return format!("{}\n", note.unwrap_or("No manga in Reading."));
    }
    let cut = |s: &str, n: usize| s.chars().take(n).collect::<String>();
    let w = items.iter().map(|i| i.title.chars().count()).max().unwrap_or(0).min(36);
    let lw = items.iter().map(|i| i.latest.chars().count()).max().unwrap_or(0).max(6);
    let mut out = note.map_or_else(String::new, |n| format!("{n}\n"));
    out.push_str("Reading, by the next release (Release data: MangaUpdates; next dates are estimates):\n");
    for i in items {
        let latest = if i.latest.is_empty() { "\u{2013}".to_string() } else { i.latest.clone() };
        let when = if i.when.is_empty() { String::new() } else { format!(" ({})", i.when) };
        let behind = match i.behind {
            Some(0) => "caught up".to_string(),
            Some(n) => format!("{n} to read"),
            None => String::new(),
        };
        let pace = match (i.pace.as_str(), i.next.as_str()) {
            ("", _) => String::new(),
            (p, "") => p.to_string(),
            (p, n) => format!("{p}, {n}"),
        };
        let flag = if i.flag.is_empty() { String::new() } else { format!("  [{}]", i.flag) };
        let line = format!("  {:<w$}  {latest:<lw$}{when:<16}  {behind:<11}  {pace}{flag}", cut(&i.title, 36));
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn sync_text(st: &Status) -> String {
    if st.offline {
        return "Offline: the queue is kept and goes out when your list service can be reached.".into();
    }
    match st.queue {
        0 => "Reading your list again.".into(),
        1 => "Sending 1 change, then reading your list again.".into(),
        n => format!("Sending {n} changes, then reading your list again."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_reads_the_title_and_the_episode() {
        assert_eq!(
            parse_set(&["Lanterns", "of", "Kirigaoka", "6"]),
            Ok(Request::Set { title: "Lanterns of Kirigaoka".into(), progress: Some(6), status: None, score: None, score_text: None, anyway: false })
        );
        assert_eq!(
            parse_set(&["Neon", "Ronin", "2099", "--status", "completed", "--score", "9"]),
            Ok(Request::Set { title: "Neon Ronin".into(), progress: Some(2099), status: Some("completed".into()), score: None, score_text: Some("9".into()), anyway: false }),
            "a number at the end is the episode; the app refuses one past the end"
        );
        assert_eq!(parse_set(&["86", "--score", "none"]), Ok(Request::Set { title: "86".into(), progress: None, status: None, score: None, score_text: Some("none".into()), anyway: false }));
        // Any score goes as typed: the app knows the user's format (73, 7.3, ***, :)).
        assert_eq!(parse_set(&["Neon", "--score", "7.3"]).map(|r| matches!(r, Request::Set { score_text: Some(t), .. } if t == "7.3")), Ok(true));
        assert!(parse_set(&["Neon"]).is_err());
        assert!(parse_set(&["6"]).is_err(), "a lone number is a title with nothing to set");
        assert!(parse_set(&["Neon", "--status"]).is_err());
    }

    #[test]
    fn anyway_marks_an_episode_not_aired_yet() {
        assert_eq!(parse_plus_one(&["Lanterns", "--anyway"]), Ok(Request::PlusOne { title: "Lanterns".into(), anyway: true }));
        assert_eq!(parse_plus_one(&["Neon", "Ronin"]), Ok(Request::PlusOne { title: "Neon Ronin".into(), anyway: false }));
        assert!(parse_plus_one(&["--anyway"]).is_err());
        assert!(parse_plus_one(&[]).is_err());
        assert_eq!(
            parse_set(&["Lanterns", "9", "--anyway"]),
            Ok(Request::Set { title: "Lanterns".into(), progress: Some(9), status: None, score: None, score_text: None, anyway: true })
        );
        // A held request is sent again as it was, plus "anyway".
        let set = Request::Set { title: "Lanterns".into(), progress: Some(9), status: Some("watching".into()), score: None, score_text: None, anyway: false };
        assert_eq!(with_anyway(set), Request::Set { title: "Lanterns".into(), progress: Some(9), status: Some("watching".into()), score: None, score_text: None, anyway: true });
    }

    #[test]
    fn remove_reads_the_title_and_yes() {
        assert_eq!(parse_remove(&["Neon", "Ronin"]), Ok(Request::Remove { title: "Neon Ronin".into(), yes: false }));
        assert_eq!(parse_remove(&["--yes", "9001"]), Ok(Request::Remove { title: "9001".into(), yes: true }));
        assert!(parse_remove(&["-y"]).is_err());
        // Held for the question: sent again with yes.
        assert_eq!(with_anyway(Request::Remove { title: "Neon".into(), yes: false }), Request::Remove { title: "Neon".into(), yes: true });
    }

    #[test]
    fn add_reads_the_query_and_the_status() {
        assert_eq!(parse_add(&["Velvet", "Orchard"]), Ok(Request::Add { query: "Velvet Orchard".into(), status: None }));
        assert_eq!(parse_add(&["9001", "--status", "plan"]), Ok(Request::Add { query: "9001".into(), status: Some("plan".into()) }));
        // kot set --manga: a title or a bare id, the last number the chapter, --volumes.
        let ms = |title: &str, progress: Option<u32>, volumes: Option<u32>, status: Option<&str>, score: Option<&str>| Request::MangaSet { title: title.into(), progress, volumes, status: status.map(Into::into), score_text: score.map(Into::into) };
        assert_eq!(parse_set(&["--manga", "Drama", "Queen", "40"]), Ok(ms("Drama Queen", Some(40), None, None, None)));
        assert_eq!(parse_set(&["--manga", "24", "258"]), Ok(ms("24", Some(258), None, None, None)), "id 24, chapter 258");
        assert_eq!(parse_set(&["24", "--manga", "--volumes", "3"]), Ok(ms("24", None, Some(3), None, None)), "a lone number is the id, not a chapter");
        assert_eq!(parse_set(&["--manga", "Salt", "--status", "reading", "--score", "8", "--vol", "2"]), Ok(ms("Salt", None, Some(2), Some("reading"), Some("8"))));
        assert!(parse_set(&["--manga", "Salt"]).is_err(), "nothing to set");
        assert!(parse_set(&["Salt", "--volumes", "2"]).unwrap_err().contains("--manga"));
        assert!(parse_set(&["--manga", "Salt", "--volumes", "x"]).is_err());
        assert_eq!(parse_add(&["--manga", "Salt", "Road", "--status", "reading"]), Ok(Request::MangaAdd { query: "Salt Road".into(), status: Some("reading".into()) }));
        assert_eq!(parse_remove(&["Salt", "--manga", "--yes"]), Ok(Request::MangaRemove { title: "Salt".into(), yes: true }));
        assert_eq!(parse_remove(&["Salt"]), Ok(Request::Remove { title: "Salt".into(), yes: false }));
        assert_eq!(with_anyway(Request::FillVolumes { yes: false }), Request::FillVolumes { yes: true });
        assert_eq!(with_anyway(Request::MangaRemove { title: "Salt".into(), yes: false }), Request::MangaRemove { title: "Salt".into(), yes: true });
        assert!(parse_add(&[]).is_err());
        assert!(parse_add(&["--status", "plan"]).is_err());
        assert!(parse_add(&["x", "--status"]).is_err());
    }

    fn e(title: &str, status: &str, progress: u32, score: Option<u8>, waiting: Option<&str>) -> Entry {
        Entry { id: 1, title: title.into(), status: status.into(), progress, episodes: Some(12), score, score_text: None, waiting: waiting.map(Into::into), rewatching: false, times_rewatched: 0, ..Default::default() }
    }

    #[test]
    fn lists_read_plainly() {
        let entries = [e("Lanterns of Kirigaoka", "Watching", 5, Some(8), Some("Waiting to sync")), e("Neon Ronin", "Completed", 12, None, None)];
        assert_eq!(list_text(&entries[..1], false), "     5 / 12   8  Lanterns of Kirigaoka  (waiting to sync)\n");
        assert_eq!(
            list_text(&entries, true),
            "Watching (1)\n     5 / 12   8  Lanterns of Kirigaoka  (waiting to sync)\n\nCompleted (1)\n    12 / 12   \u{2013}  Neon Ronin\n"
        );
        assert_eq!(list_text(&[], false), "Nothing here.\n");
        // Manga: chapters and volumes, the column as wide as the widest.
        let m = |title: &str, ch: u32, total: Option<u32>, vol: u32, vols: Option<u32>| Entry { title: title.into(), status: "Reading".into(), progress: ch, episodes: total, volumes: Some(vol), volumes_total: vols, ..Default::default() };
        assert_eq!(
            list_text(&[m("Paper Lanterns", 21, None, 3, None), m("Iron Garden", 120, Some(120), 12, Some(12))], false),
            "  ch  21 / ?    vol  3 / ?    \u{2013}  Paper Lanterns\n  ch 120 / 120  vol 12 / 12   \u{2013}  Iron Garden\n"
        );
        // Only what was read shows (chapters when nothing was), the columns still line up.
        let rows = [m("Both", 21, None, 3, None), m("Chapters only", 7, Some(40), 0, Some(4)), m("Volumes only", 0, None, 11, Some(14)), m("Nothing yet", 0, Some(9), 0, None)];
        assert_eq!(
            list_text(&rows, false),
            [
                "  ch 21 / ?   vol  3 / ?    \u{2013}  Both",
                "  ch  7 / 40                \u{2013}  Chapters only",
                "              vol 11 / 14   \u{2013}  Volumes only",
                "  ch  0 / 9                 \u{2013}  Nothing yet\n",
            ]
            .join("\n")
        );
        assert_eq!(progress_text(&rows[2]), "vol 11 / 14");
        assert_eq!(progress_text(&rows[0]), "ch 21 / ?  vol 3 / ?");
        assert_eq!(edited_text(&entries[0]), "Lanterns of Kirigaoka: 5 / 12 · Watching · score 8 (waiting to sync)");
        assert_eq!(edited_text(&entries[1]), "Neon Ronin: 12 / 12 · Completed · no score (synced)");
        // The app's own text in the user's format wins; the column fits the widest.
        let stars = |n: usize| "\u{2605}".repeat(n) + &"\u{2606}".repeat(5 - n);
        let mut a = e("Lanterns", "Watching", 5, Some(7), None);
        a.score_text = Some(stars(4));
        let b = e("Neon", "Watching", 2, None, None);
        assert_eq!(list_text(&[a.clone(), b], false), format!("     5 / 12  {}  Lanterns
     2 / 12      \u{2013}  Neon
", stars(4)));
        assert_eq!(edited_text(&a), format!("Lanterns: 5 / 12 · Watching · score {} (synced)", stars(4)));
    }

    #[test]
    fn the_queue_reads_plainly() {
        let ops = [
            QueueItem { title: "Mikan".into(), change: "Progress 6 \u{2192} 7".into(), state: "Conflict".into(), note: Some("MyAnimeList has 8 / 24".into()) },
            QueueItem { title: "Lanterns".into(), change: "Progress 3 \u{2192} 5".into(), state: "Waiting".into(), note: None },
        ];
        assert_eq!(queue_text(&ops), "Conflict  Mikan     Progress 6 \u{2192} 7 · MyAnimeList has 8 / 24\nWaiting   Lanterns  Progress 3 \u{2192} 5\n");
        assert_eq!(queue_text(&[]), "Nothing waiting.\n");
        assert_eq!(sync_text(&Status { queue: 2, ..Default::default() }), "Sending 2 changes, then reading your list again.");
        assert!(sync_text(&Status { queue: 2, offline: true, ..Default::default() }).starts_with("Offline"));
    }

    #[test]
    fn status_reads_plainly() {
        let st = Status { version: "0.1.0 (build 1)".into(), account: Some("demo_user".into()), service: "MyAnimeList".into(), entries: 303, queue: 4, offline: false, window: true, mirror: None };
        assert_eq!(status_text(&st), "Kotora 0.1.0 (build 1)\ndemo_user on MyAnimeList · 303 entries · 4 changes waiting\n");
        let st = Status { version: "0.1.0 (build 1)".into(), entries: 1, offline: true, ..Default::default() };
        assert_eq!(status_text(&st), "Kotora 0.1.0 (build 1)\nnot signed in · 1 entry · synced · offline · hidden\n");
    }

    #[test]
    fn status_shows_the_mirror() {
        let mirror = |waiting, conflicts, failed, web_edits| Some(kotora_ctl::MirrorStatus { to: "AniList".into(), waiting, conflicts, failed, web_edits });
        let st = Status { version: "0.3.0 (build 1)".into(), account: Some("demo_user".into()), service: "MyAnimeList".into(), entries: 329, mirror: mirror(0, 0, 0, 0), window: true, ..Default::default() };
        assert_eq!(status_short(&st), "demo_user on MyAnimeList \u{21c4} AniList · 329 entries · synced · mirror: synced");
        let st = Status { mirror: mirror(3, 0, 0, 0), ..st };
        assert_eq!(status_short(&st), "demo_user on MyAnimeList \u{21c4} AniList · 329 entries · synced · mirror: 3 waiting");
        let st = Status { queue: 1, mirror: mirror(2, 1, 2, 1), ..st };
        assert_eq!(status_short(&st), "demo_user on MyAnimeList \u{21c4} AniList · 329 entries · 1 change waiting · mirror: 2 waiting, 1 conflict, 2 failed, 1 website edit");
        // Not signed in: no mirror name in the account part.
        let st = Status { account: None, ..st };
        assert!(status_short(&st).starts_with("not signed in · "));
    }

    #[test]
    fn publishing_reads_plainly() {
        let items = [
            PublishingItem { title: "Inkwell Cartographers".into(), latest: "Ch 121".into(), when: "23 h ago".into(), behind: Some(2), pace: "Weekly".into(), next: "next ~Thu".into(), flag: String::new() },
            PublishingItem { title: "Moth Orchestra".into(), latest: "Night 259".into(), when: "4 months ago".into(), behind: Some(0), pace: "On hiatus".into(), next: String::new(), flag: "check match".into() },
            PublishingItem { title: "Kumo Atelier".into(), flag: "no match".into(), ..Default::default() },
        ];
        let t = publishing_text(&items, None);
        let lines: Vec<&str> = t.lines().collect();
        assert!(lines[0].contains("MangaUpdates") && lines[0].contains("estimates"));
        assert_eq!(lines[1], "  Inkwell Cartographers  Ch 121    (23 h ago)       2 to read    Weekly, next ~Thu");
        assert_eq!(lines[2], "  Moth Orchestra         Night 259 (4 months ago)   caught up    On hiatus  [check match]");
        assert_eq!(lines[3], "  Kumo Atelier           \u{2013}                                         [no match]");
        assert_eq!(publishing_text(&[], Some("Turn Manga on first.")), "Turn Manga on first.\n");
        // Offline with saved data: the notice, then the saved rows.
        let t = publishing_text(&items[..1], Some("Offline: MangaUpdates data from 3 hours ago; can\u{2019}t refresh now."));
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "Offline: MangaUpdates data from 3 hours ago; can\u{2019}t refresh now.");
        assert!(lines[1].starts_with("Reading, by the next release"));
        assert!(lines[2].contains("Inkwell Cartographers"));
    }

    #[test]
    fn airing_reads_plainly() {
        let it = |title: &str, episode, day: &str, time: &str, countdown: &str, behind| AiringItem {
            title: title.into(),
            episode,
            at: 0,
            day: day.into(),
            time: time.into(),
            countdown: countdown.into(),
            behind,
            estimate: false,
        };
        let text = airing_text(
            &[it("Mecha Bento Club", 11, "Today", "15:00", "in 6 h", 0), it("Lanterns", 7, "Saturday", "15:30", "in 1 d 6 h", 1), it("Hoshizora", 4, "Saturday", "17:00", "in 1 d 8 h", 0)],
            None,
        );
        assert_eq!(
            text,
            "Airing on your list, next 7 days (some times are estimates):\nToday\n  15:00  Mecha Bento Club  ep 11  (in 6 h)\nSaturday\n  15:30  Lanterns          ep 7  (in 1 d 6 h, 1 to watch)\n  17:00  Hoshizora         ep 4  (in 1 d 8 h)\n"
        );
        // An estimate (MyAnimeList's timetable) is marked.
        let est = AiringItem { estimate: true, ..it("Lanterns", 7, "Saturday", "15:30", "in 1 d 6 h", 3) };
        assert!(airing_text(&[est], None).contains("(in 1 d 6 h, ~3 to watch)"));
        assert_eq!(airing_text(&[], None), "Nothing on your list airs in the next 7 days.\n");
        assert_eq!(airing_text(&[], Some("Loading.")), "Loading.\n");
    }

    #[test]
    fn now_reads_plainly() {
        assert_eq!(now_text(None), "Nothing is playing.\n");
        let n = NowInfo {
            player: "mpv".into(),
            via: "IPC socket".into(),
            file: "[Hoshi] Lanterns of Kirigaoka - 06 (1080p).mkv".into(),
            playing: true,
            percent: Some(82),
            title: "Lanterns of Kirigaoka".into(),
            episode: Some(6),
            matched: "In your list (Watching), sure".into(),
            offer: Some(6),
            reached: true,
            ..Default::default()
        };
        assert_eq!(
            now_text(Some(&n)),
            "Playing in mpv (IPC socket, 82%): Lanterns of Kirigaoka episode 6\n  [Hoshi] Lanterns of Kirigaoka - 06 (1080p).mkv\n  In your list (Watching), sure\n  Update to episode 6? kot confirm\n"
        );
        let skip = NowInfo { offer: Some(8), episode: Some(8), note: Some("Skips episode 7: Kotora asks before it records.".into()), ..n.clone() };
        assert!(now_text(Some(&skip)).ends_with("  Update to episode 8? kot confirm\n  Skips episode 7: Kotora asks before it records.\n"));
        assert!(now_text(Some(&NowInfo { recorded: Some(6), ..n.clone() })).ends_with("Recorded episode 6.\n"));
        assert!(now_text(Some(&NowInfo { offer: None, ..n })).ends_with("Nothing to record.\n"));
    }

    #[test]
    fn reading_says_what_to_mark() {
        let r = ReadingInfo {
            reader: "YACReader".into(),
            file: "Glass Lantern Club v35 c362.cbz".into(),
            title: "Glass Lantern Club".into(),
            label: "Ch 362 (Vol 35)".into(),
            page: Some(12),
            pages: Some(40),
            matched: "In your list (Reading), sure".into(),
            offer: Some("Ch 362".into()),
            ..Default::default()
        };
        assert_eq!(
            reading_text(&r),
            "Reading in YACReader: Glass Lantern Club Ch 362 (Vol 35) (page 12 of 40)\n  Glass Lantern Club v35 c362.cbz\n  In your list (Reading), sure\n  Ch 362 is offered once it is read to the end (kot confirm marks it now).\n"
        );
        let done = ReadingInfo { finished: true, ..r.clone() };
        assert!(reading_text(&done).contains("(read to the end)") && reading_text(&done).ends_with("Mark Ch 362 as read? kot confirm\n"));
        assert!(reading_text(&ReadingInfo { recorded: Some("Ch 362".into()), ..r.clone() }).ends_with("Marked Ch 362 as read.\n"));
        // Closed, still asking (the question stays in the banner).
        assert!(reading_text(&ReadingInfo { open: false, finished: true, ..r }).starts_with("Closed in YACReader: "));
        // An older Kotora sends no `open`: open.
        let old: ReadingInfo = serde_json::from_str(r#"{"reader":"YACReader"}"#).unwrap();
        assert!(old.open);
    }
}
