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
//! kot now        what plays, and the episode Kotora offers to record
//! kot confirm    record the offered episode (the banner's Update)
//! kot dismiss    not now
//! kot add QUERY|ID [--status S]
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

use kotora_ctl::{AiringItem, Client, Entry, NowInfo, QueueItem, Reply, Request, Status};

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
  --anyway              with +1 and set: mark an episode that hasn't
                        aired yet (by the estimate) without asking
  queue                 changes waiting to sync
  sync                  send the queue now, then read the list again
  now                   what plays, and the episode Kotora offers
  airing, schedule      your anime airing in the next 7 days (estimated)
  confirm               record the offered episode
  dismiss               not now: hide the offer for this file
  rewatch TITLE|ID      watch a completed anime again from episode 1
  next TITLE            play the next episode of TITLE from your
                        library folders (Settings \u{2192} Library)
  add QUERY|ID          put an anime on your list: a title to search
      [--status STATUS]   your list service for (it must find one), or its id
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
        ["confirm"] => Request::Confirm,
        ["dismiss"] => Request::Dismiss,
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
        ["next", title @ ..] if !title.is_empty() => Request::Next { title: title.join(" ") },
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
    // Held: the episode hasn't aired yet by the estimate. Ask on a terminal, else say how.
    if let Ok(Reply::Error { message, anyway: true }) = &reply {
        eprintln!("kot: {message}");
        if !confirm("Mark anyway? [y/N] ") {
            if !interactive() {
                eprintln!("kot: run it again with --anyway to mark it.");
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
        Ok(Reply::List { entries }) => {
            print!("{}", list_text(&entries, matches!(req, Request::List { status: None })));
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
        Ok(Reply::Now { now }) => {
            print!("{}", now_text(now.as_ref()));
            ExitCode::SUCCESS
        }
        Ok(Reply::Airing { items, note }) => {
            print!("{}", airing_text(&items, note.as_deref()));
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
    Client::connect()?.request(req)
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

/// No ioctl here (Windows): `$COLUMNS`, else 80.
#[cfg(not(unix))]
fn console_width() -> Option<usize> {
    None
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
        other => other,
    }
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
        Some(name) => format!("{name} on {}", st.service),
        None => "not signed in".to_string(),
    };
    let queue = match st.queue {
        0 => "synced".to_string(),
        1 => "1 change waiting".to_string(),
        n => format!("{n} changes waiting"),
    };
    let mut line = format!("{who} · {} {} · {queue}", st.entries, if st.entries == 1 { "entry" } else { "entries" });
    if st.offline {
        line.push_str(" · offline");
    }
    if !st.window {
        line.push_str(" · hidden");
    }
    line
}

/// `set TITLE… [EP] [--status S] [--score SCORE|none]`: the last bare number is the episode.
/// The score goes as typed: the app reads it in the user's score format.
fn parse_set(words: &[&str]) -> Result<Request, String> {
    let (mut title, mut progress, mut status, mut score_text, mut anyway) = (Vec::new(), None, None, None, false);
    let mut i = 0;
    while i < words.len() {
        match words[i] {
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
    if progress.is_none() && status.is_none() && score_text.is_none() {
        return Err("set needs an episode number, --status or --score".into());
    }
    Ok(Request::Set { title: title.join(" "), progress, status, score: None, score_text, anyway })
}

/// `add QUERY… [--status S]`.
fn parse_add(words: &[&str]) -> Result<Request, String> {
    let (mut query, mut status) = (Vec::new(), None);
    let mut i = 0;
    while i < words.len() {
        match words[i] {
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
    Ok(Request::Add { query: query.join(" "), status })
}

fn progress_text(e: &Entry) -> String {
    format!("{} / {}", e.progress, e.episodes.map_or("?".to_string(), |n| n.to_string()))
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
    for e in entries {
        if grouped && e.status != last {
            if !last.is_empty() {
                out.push('\n');
            }
            let n = entries.iter().filter(|x| x.status == e.status).count();
            out.push_str(&format!("{} ({n})\n", e.status));
            last = &e.status;
        }
        let score = score_text(e).unwrap_or_else(|| "\u{2013}".to_string());
        let mut line = format!("  {:>9}  {score:>w$}  {}", progress_text(e), e.title, w = width);
        if e.rewatching {
            line.push_str("  (rewatching)");
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
        let behind = if i.behind > 0 { format!(", {} aired not watched", i.behind) } else { String::new() };
        out.push_str(&format!("  {}  {title:<w$}  ep {}  ({}{behind})\n", i.time, i.episode, i.countdown));
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
    fn add_reads_the_query_and_the_status() {
        assert_eq!(parse_add(&["Velvet", "Orchard"]), Ok(Request::Add { query: "Velvet Orchard".into(), status: None }));
        assert_eq!(parse_add(&["9001", "--status", "plan"]), Ok(Request::Add { query: "9001".into(), status: Some("plan".into()) }));
        assert!(parse_add(&[]).is_err());
        assert!(parse_add(&["--status", "plan"]).is_err());
        assert!(parse_add(&["x", "--status"]).is_err());
    }

    fn e(title: &str, status: &str, progress: u32, score: Option<u8>, waiting: Option<&str>) -> Entry {
        Entry { id: 1, title: title.into(), status: status.into(), progress, episodes: Some(12), score, score_text: None, waiting: waiting.map(Into::into), rewatching: false, times_rewatched: 0 }
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
        let st = Status { version: "0.1.0 (build 1)".into(), account: Some("demo_user".into()), service: "MyAnimeList".into(), entries: 303, queue: 4, offline: false, window: true };
        assert_eq!(status_text(&st), "Kotora 0.1.0 (build 1)\ndemo_user on MyAnimeList · 303 entries · 4 changes waiting\n");
        let st = Status { version: "0.1.0 (build 1)".into(), entries: 1, offline: true, ..Default::default() };
        assert_eq!(status_text(&st), "Kotora 0.1.0 (build 1)\nnot signed in · 1 entry · synced · offline · hidden\n");
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
        };
        let text = airing_text(
            &[it("Mecha Bento Club", 11, "Today", "15:00", "in 6 h", 0), it("Lanterns", 7, "Saturday", "15:30", "in 1 d 6 h", 1), it("Hoshizora", 4, "Saturday", "17:00", "in 1 d 8 h", 0)],
            None,
        );
        assert_eq!(
            text,
            "Airing on your list, next 7 days (some times are estimates):\nToday\n  15:00  Mecha Bento Club  ep 11  (in 6 h)\nSaturday\n  15:30  Lanterns          ep 7  (in 1 d 6 h, 1 aired not watched)\n  17:00  Hoshizora         ep 4  (in 1 d 8 h)\n"
        );
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
}
