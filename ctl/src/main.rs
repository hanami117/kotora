//! `kot`: the command line for a running Kotora.
//!
//! ```text
//! kot            same as `kot status`
//! kot status     the app's state in two lines
//! kot show       shows the window
//! kot quit       quits the app
//! kot --version
//! ```

use std::io;
use std::process::ExitCode;

use kotora_ctl::{Client, Reply, Request, Status};

const HELP: &str = "\
kot: talk to a running Kotora

Usage: kot [COMMAND]

Commands:
  status      what Kotora is doing (the default)
  show        show the window
  quit        quit Kotora
  help        this text

Options:
  -V, --version   print the version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let req = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] | ["status"] => Request::Status,
        ["show"] => Request::Show,
        ["quit"] => Request::Quit,
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
            print!("{}", status_text(&st));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reads_plainly() {
        let st = Status { version: "0.1.0 (build 1)".into(), account: Some("demo_user".into()), service: "MyAnimeList".into(), entries: 303, queue: 4, offline: false, window: true };
        assert_eq!(status_text(&st), "Kotora 0.1.0 (build 1)\ndemo_user on MyAnimeList · 303 entries · 4 changes waiting\n");
        let st = Status { version: "0.1.0 (build 1)".into(), entries: 1, offline: true, ..Default::default() };
        assert_eq!(status_text(&st), "Kotora 0.1.0 (build 1)\nnot signed in · 1 entry · synced · offline · hidden\n");
    }
}
