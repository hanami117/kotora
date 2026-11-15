//! `kot`: the command line for a running Kotora.
//!
//! ```text
//! kot            same as `kot status`
//! kot status     the app's state in two lines
//! kot list       the Watching list (`kot list all`, `kot list completed`, …)
//! kot +1 TITLE   one more episode
//! kot set TITLE EP [--status S] [--score N|none]
//! kot queue      changes waiting to sync
//! kot sync       send the queue now, then read the list again
//! kot show       shows the window
//! kot quit       quits the app
//! kot --version
//! ```
//!
//! A TITLE is any part of a title (English and Japanese titles too), without quotes; it must
//! match one entry.

use std::io;
use std::process::ExitCode;

use kotora_ctl::{Client, Entry, QueueItem, Reply, Request, Status};

const HELP: &str = "\
kot: talk to a running Kotora

Usage: kot [COMMAND]

Commands:
  status                what Kotora is doing (the default)
  list [STATUS|all]     your list; STATUS: watching (the default), plan,
                        completed, on-hold, dropped
  +1 TITLE              one more episode of TITLE
  set TITLE EP          set the episodes watched
      [--status STATUS] [--score N|none]
  queue                 changes waiting to sync
  sync                  send the queue now, then read the list again
  show                  show the window
  quit                  quit Kotora
  help                  this text

TITLE is any part of a title, no quotes needed; it must match one entry.
Changes go to the sync queue first, then to MyAnimeList.

Options:
  -V, --version   print the version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let req = match words.as_slice() {
        [] | ["status"] => Request::Status,
        ["show"] => Request::Show,
        ["quit"] => Request::Quit,
        ["queue"] => Request::Queue,
        ["sync"] => Request::Sync,
        ["list"] => Request::List { status: Some("watching".into()) },
        ["list", "all"] => Request::List { status: None },
        ["list", st] => Request::List { status: Some((*st).to_string()) },
        ["+1", title @ ..] if !title.is_empty() => Request::PlusOne { title: title.join(" ") },
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
    match ask(&req) {
        Ok(Reply::Status(st)) => {
            if req == Request::Sync {
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
        Ok(Reply::Error { message }) if message.starts_with("bad request") => {
            eprintln!("kot: this Kotora is older than kot and doesn't know that command; update Kotora");
            ExitCode::FAILURE
        }
        Ok(Reply::Error { message }) => {
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

/// Two lines: the version, then the account and the list.
fn status_text(st: &Status) -> String {
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
    format!("Kotora {}\n{line}\n", st.version)
}

/// `set TITLE… [EP] [--status S] [--score N|none]`: the last bare number is the episode.
fn parse_set(words: &[&str]) -> Result<Request, String> {
    let (mut title, mut progress, mut status, mut score) = (Vec::new(), None, None, None);
    let mut i = 0;
    while i < words.len() {
        match words[i] {
            "--status" => {
                status = Some(words.get(i + 1).ok_or("--status needs a status")?.to_string());
                i += 1;
            }
            "--score" => {
                let v = words.get(i + 1).ok_or("--score needs a number from 1 to 10, or none")?;
                score = Some(match *v {
                    "none" | "0" | "-" => 0,
                    v => v.parse::<u8>().ok().filter(|n| (1..=10).contains(n)).ok_or("the score is a number from 1 to 10, or none")?,
                });
                i += 1;
            }
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
    if progress.is_none() && status.is_none() && score.is_none() {
        return Err("set needs an episode number, --status or --score".into());
    }
    Ok(Request::Set { title: title.join(" "), progress, status, score })
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
    for e in entries {
        if grouped && e.status != last {
            if !last.is_empty() {
                out.push('\n');
            }
            let n = entries.iter().filter(|x| x.status == e.status).count();
            out.push_str(&format!("{} ({n})\n", e.status));
            last = &e.status;
        }
        let score = e.score.map_or("\u{2013}".to_string(), |n| n.to_string());
        let mut line = format!("  {:>9}  {score:>2}  {}", progress_text(e), e.title);
        if let Some(w) = &e.waiting {
            line.push_str(&format!("  ({})", w.to_lowercase()));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn edited_text(e: &Entry) -> String {
    let score = e.score.map_or("no score".to_string(), |n| format!("score {n}"));
    let state = e.waiting.as_deref().map_or("synced".to_string(), str::to_lowercase);
    format!("{}: {} · {} · {score} ({state})", e.title, progress_text(e), e.status)
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

fn sync_text(st: &Status) -> String {
    if st.offline {
        return "Offline: the queue is kept and goes out when MyAnimeList can be reached.".into();
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
            Ok(Request::Set { title: "Lanterns of Kirigaoka".into(), progress: Some(6), status: None, score: None })
        );
        assert_eq!(
            parse_set(&["Neon", "Ronin", "2099", "--status", "completed", "--score", "9"]),
            Ok(Request::Set { title: "Neon Ronin".into(), progress: Some(2099), status: Some("completed".into()), score: Some(9) }),
            "a number at the end is the episode; the app refuses one past the end"
        );
        assert_eq!(parse_set(&["86", "--score", "none"]), Ok(Request::Set { title: "86".into(), progress: None, status: None, score: Some(0) }));
        assert!(parse_set(&["Neon"]).is_err());
        assert!(parse_set(&["6"]).is_err(), "a lone number is a title with nothing to set");
        assert!(parse_set(&["Neon", "--score", "11"]).is_err());
        assert!(parse_set(&["Neon", "--status"]).is_err());
    }

    fn e(title: &str, status: &str, progress: u32, score: Option<u8>, waiting: Option<&str>) -> Entry {
        Entry { id: 1, title: title.into(), status: status.into(), progress, episodes: Some(12), score, waiting: waiting.map(Into::into) }
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
}
