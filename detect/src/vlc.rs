//! VLC's web interface (macOS, where VLC has no MPRIS). The user turns it on in VLC (Preferences →
//! Show All → Interface → Main interfaces: Web, and a Lua HTTP password) and gives Kotora the
//! password in Settings or `config.toml` (`[vlc] password`, `port`). Off until then.
//!
//! VLC sends no events here, so this is the one source that polls `/requests/status.json`, on
//! 127.0.0.1 only, and only while it is switched on and set up:
//! - VLC doesn't answer (not running): one connection attempt a minute;
//! - VLC answers, nothing playing or paused: every 30 s;
//! - VLC plays: every 15 s (the watch hub works out the position in between).
//!
//! Field names checked against VLC 3.0's `status.json`: `state`, `time`, `length`, `position`,
//! `rate`, `information.category.meta.filename`. (To verify on a live VLC on macOS, stage 4.)

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use serde_json::Value;

use crate::run::{Input, Shared};
use crate::{Led, PlayState, Playing, SourceStatus, Switch, Via};

pub const WAIT_NOT_RUNNING: Duration = Duration::from_secs(60);
pub const WAIT_IDLE: Duration = Duration::from_secs(30);
pub const WAIT_PLAYING: Duration = Duration::from_secs(15);

pub const KEY: &str = "vlc-http";

/// `status.json` → a snapshot; `None` when the text isn't VLC's status.
pub fn parse_status(body: &str, at: f64) -> Option<Playing> {
    let v: Value = serde_json::from_str(body).ok()?;
    let state = match v.get("state")?.as_str()? {
        "playing" => PlayState::Playing,
        "paused" => PlayState::Paused,
        _ => PlayState::Stopped,
    };
    let meta = v.pointer("/information/category/meta");
    let file = meta
        .and_then(|m| m.get("filename"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let length = v.get("length").and_then(Value::as_f64).filter(|l| *l > 0.0);
    // `position` (0–1) is finer than `time` (whole seconds).
    let position = match (v.get("position").and_then(Value::as_f64), length) {
        (Some(p), Some(l)) if p > 0.0 => Some(p * l),
        _ => v.get("time").and_then(Value::as_f64),
    };
    Some(Playing {
        key: KEY.into(),
        player: "VLC".into(),
        switch: Switch::Vlc,
        via: Via::VlcHttp,
        path: if state == PlayState::Stopped {
            None
        } else {
            file
        },
        title: None,
        position,
        duration: length,
        state,
        rate: v.get("rate").and_then(Value::as_f64).unwrap_or(1.0),
        at,
    })
}

/// `user:password` for HTTP basic auth (VLC's user name is empty).
fn basic(password: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = format!(":{password}").into_bytes();
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    format!("Basic {out}")
}

enum Got {
    Status(Playing),
    NotRunning,
    Refused,
    Odd(String),
}

fn fetch(port: u16, password: &str) -> Got {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    // A refused connect is the cheap "not running" answer; no HTTP needed.
    if std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_err() {
        return Got::NotRunning;
    }
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .http_status_as_error(false)
        .build()
        .new_agent();
    let r = agent
        .get(format!("http://127.0.0.1:{port}/requests/status.json"))
        .header("Authorization", basic(password))
        .call();
    match r {
        Ok(mut resp) if resp.status() == 200 => match resp.body_mut().read_to_string() {
            Ok(body) => match parse_status(&body, crate::clock()) {
                Some(p) => Got::Status(p),
                None => Got::Odd("answered, but not like VLC".into()),
            },
            Err(e) => Got::Odd(e.to_string()),
        },
        Ok(resp) if resp.status() == 401 || resp.status() == 403 => Got::Refused,
        Ok(resp) => Got::Odd(format!("HTTP {}", resp.status())),
        Err(_) => Got::NotRunning,
    }
}

fn status(line: &str, led: Led) -> Input {
    Input::Status(SourceStatus {
        switch: Switch::Vlc,
        via: Via::VlcHttp,
        line: line.into(),
        led,
    })
}

/// The thread. `wake` is pinged when the settings change; while the source is off it blocks
/// there and never polls.
pub fn run(shared: std::sync::Arc<Shared>, wake: Receiver<()>, tx: Sender<Input>) {
    let mut had_player = false;
    loop {
        let (on, port, password) = shared.vlc();
        let wait = if !on || password.is_empty() {
            if had_player {
                had_player = false;
                let _ = tx.send(Input::Gone(KEY.into()));
            }
            let line = if password.is_empty() {
                "web interface not set up (needs VLC's password)"
            } else {
                "web interface: off"
            };
            if tx
                .send(status(
                    line,
                    if password.is_empty() {
                        Led::Problem
                    } else {
                        Led::Off
                    },
                ))
                .is_err()
            {
                return;
            }
            None
        } else {
            let (input, wait) = match fetch(port, &password) {
                Got::Status(p) => {
                    let (line, led, wait) = match p.state {
                        PlayState::Playing => {
                            ("web interface: playing", Led::Playing, WAIT_PLAYING)
                        }
                        _ => ("web interface: running", Led::Idle, WAIT_IDLE),
                    };
                    let _ = tx.send(status(line, led));
                    had_player = true;
                    (Input::Update(p), wait)
                }
                other => {
                    let (line, led) = match &other {
                        Got::Refused => (
                            "web interface refused the password".to_string(),
                            Led::Problem,
                        ),
                        Got::Odd(m) => (format!("web interface: {m}"), Led::Problem),
                        _ => ("web interface: not running".to_string(), Led::Off),
                    };
                    let _ = tx.send(status(&line, led));
                    had_player = false;
                    (Input::Gone(KEY.into()), WAIT_NOT_RUNNING)
                }
            };
            if tx.send(input).is_err() {
                return;
            }
            Some(wait)
        };
        let woke = match wait {
            None => wake.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(d) => wake.recv_timeout(d),
        };
        if woke == Err(RecvTimeoutError::Disconnected) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of VLC 3.0.x's `status.json` (trimmed; made-up file).
    const PLAYING: &str = r#"{"fullscreen":false,"stats":null,"aspectratio":"default","audiodelay":0,
      "apiversion":3,"currentplid":4,"time":731,"volume":256,"length":1421,"random":false,
      "audiofilters":{"filter_0":""},"rate":1,"videoeffects":{"hue":0},"state":"playing","loop":false,
      "version":"3.0.21 Vetinari","position":0.51478,"repeat":false,"subtitledelay":0,"equalizer":[],
      "information":{"chapter":0,"chapters":[],"title":0,"category":{"meta":{"filename":"[Hoshi] Lanterns of Kirigaoka - 06 (1080p).mkv"},
      "Stream 0":{"Type":"Video","Codec":"H264 - MPEG-4 AVC (part 10) (avc1)"}},"titles":[]}}"#;

    #[test]
    fn reads_vlcs_status() {
        let p = parse_status(PLAYING, 3.0).unwrap();
        assert_eq!(
            p.path.as_deref(),
            Some("[Hoshi] Lanterns of Kirigaoka - 06 (1080p).mkv")
        );
        assert_eq!(
            (p.state, p.duration, p.rate, p.at),
            (PlayState::Playing, Some(1421.0), 1.0, 3.0)
        );
        assert!((p.position.unwrap() - 731.5).abs() < 0.1);
        let stopped = parse_status(
            r#"{"state":"stopped","time":0,"length":0,"position":0}"#,
            0.0,
        )
        .unwrap();
        assert_eq!(
            (stopped.state, stopped.path, stopped.duration),
            (PlayState::Stopped, None, None)
        );
        assert!(parse_status("<html>", 0.0).is_none());
        assert!(parse_status(r#"{"x":1}"#, 0.0).is_none());
    }

    #[test]
    fn basic_auth_is_base64_of_colon_password() {
        assert_eq!(basic("secret"), "Basic OnNlY3JldA==");
        assert_eq!(basic("ab"), "Basic OmFi");
        assert_eq!(basic("a"), "Basic OmE=");
    }

    #[test]
    fn the_poll_is_slow() {
        assert!(WAIT_NOT_RUNNING >= Duration::from_secs(60));
        assert!(WAIT_PLAYING >= Duration::from_secs(15));
    }
}
