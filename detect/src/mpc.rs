//! MPC-HC's and MPC-BE's web interface (Windows): the file's full path, the position, the
//! length and the play state, from `http://127.0.0.1:13579/variables.html`.
//!
//! The web interface is off in both players until the user turns it on (Options → Player → Web
//! Interface → Listen on port). Kotora can do that with one click, with the user's OK:
//! [`reg`] sets the player's own setting in the registry. Without it the players are still seen
//! by their window title (`winplayers.rs`), with no position.
//!
//! Like VLC's, this source polls (the players send no events), on 127.0.0.1 only:
//! - no MPC-HC / MPC-BE process (the process scan says so): nothing is asked at all;
//! - the process runs but the web interface doesn't answer: one attempt every 30 s;
//! - it answers, nothing playing or paused: every 30 s;
//! - it plays: every 15 s (the watch hub works out the position in between).
//!
//! Field names read in MPC-HC's source (`WebClientSocket.cpp`, `OnVariables`): `file`,
//! `filepath`, `state` (2 playing, 1 paused, 0 stopped, -1 nothing loaded), `position` and
//! `duration` in milliseconds, `playbackrate`. MPC-BE keeps the same page.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use crate::run::{Input, MpcProcess, Shared};
use crate::{Led, PlayState, Playing, SourceStatus, Switch, Via};

/// The port both players use unless the user changed it.
pub const PORT: u16 = 13579;

pub const WAIT_NO_ANSWER: Duration = Duration::from_secs(30);
pub const WAIT_IDLE: Duration = Duration::from_secs(30);
pub const WAIT_PLAYING: Duration = Duration::from_secs(15);

pub const KEY: &str = "mpc-http";

/// The text of `<p id="ID">…</p>`, with HTML entities undone.
fn field(body: &str, id: &str) -> Option<String> {
    let at = body.find(&format!("id=\"{id}\""))?;
    let rest = &body[at..];
    let rest = &rest[rest.find('>')? + 1..];
    Some(unescape(&rest[..rest.find('<')?]))
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let known = [
            ("&amp;", '&'),
            ("&lt;", '<'),
            ("&gt;", '>'),
            ("&quot;", '"'),
            ("&#39;", '\''),
            ("&#039;", '\''),
            ("&apos;", '\''),
        ];
        match known.iter().find(|(e, _)| rest.starts_with(e)) {
            Some((e, c)) => {
                out.push(*c);
                rest = &rest[e.len()..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `variables.html` → a snapshot; `None` when the page isn't MPC's.
pub fn parse_variables(body: &str, player: &str, at: f64) -> Option<Playing> {
    let state = match field(body, "state")?.trim().parse::<i32>().ok()? {
        2 => PlayState::Playing,
        1 => PlayState::Paused,
        _ => PlayState::Stopped,
    };
    let text = |id| {
        field(body, id)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let ms = |id| {
        field(body, id)
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map(|v| v / 1000.0)
    };
    let file = text("filepath").or_else(|| text("file"));
    Some(Playing {
        key: KEY.into(),
        player: player.into(),
        switch: Switch::Mpc,
        via: Via::MpcHttp,
        path: if state == PlayState::Stopped {
            None
        } else {
            file
        },
        title: None,
        position: ms("position"),
        duration: ms("duration").filter(|d| *d > 0.0),
        state,
        rate: field(body, "playbackrate")
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|r| *r > 0.0)
            .unwrap_or(1.0),
        at,
    })
}

enum Got {
    Status(Playing),
    NoAnswer,
    Odd(String),
}

fn fetch(port: u16, player: &str) -> Got {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    // A refused connect is the cheap "web interface off" answer; no HTTP needed.
    if std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3)).is_err() {
        return Got::NoAnswer;
    }
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .http_status_as_error(false)
        .build()
        .new_agent();
    match agent
        .get(format!("http://127.0.0.1:{port}/variables.html"))
        .call()
    {
        Ok(mut resp) if resp.status() == 200 => match resp.body_mut().read_to_string() {
            Ok(body) => match parse_variables(&body, player, crate::clock()) {
                Some(p) => Got::Status(p),
                None => Got::Odd("answered, but not like MPC".into()),
            },
            Err(e) => Got::Odd(e.to_string()),
        },
        Ok(resp) => Got::Odd(format!("HTTP {}", resp.status())),
        Err(_) => Got::NoAnswer,
    }
}

fn status(line: &str, led: Led) -> Input {
    Input::Status(SourceStatus {
        switch: Switch::Mpc,
        via: Via::MpcHttp,
        line: line.into(),
        led,
    })
}

/// The thread. `wake` is pinged when the settings change and when an MPC process starts or
/// ends; while there is nothing to ask it blocks there and never polls.
pub fn run(shared: std::sync::Arc<Shared>, wake: Receiver<()>, tx: Sender<Input>) {
    let mut had_player = false;
    loop {
        let (on, port) = shared.mpc();
        let process = shared.mpc_process();
        let wait = if !on || process == MpcProcess::NotRunning {
            if had_player {
                had_player = false;
                let _ = tx.send(Input::Gone(KEY.into()));
            }
            if tx
                .send(status(
                    if on {
                        "web interface: not running"
                    } else {
                        "web interface: off"
                    },
                    Led::Off,
                ))
                .is_err()
            {
                return;
            }
            None
        } else {
            let player = match process {
                MpcProcess::Running(name) => name,
                _ => "MPC-HC",
            };
            let (input, wait) = match fetch(port, player) {
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
                        Got::Odd(m) => (format!("web interface: {m}"), Led::Problem),
                        // The player runs and nothing listens: its web interface is off.
                        _ if matches!(process, MpcProcess::Running(_)) => {
                            (format!("web interface is off in {player}"), Led::Off)
                        }
                        _ => ("web interface: not running".to_string(), Led::Off),
                    };
                    let _ = tx.send(status(&line, led));
                    had_player = false;
                    (Input::Gone(KEY.into()), WAIT_NO_ANSWER)
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

/// The players' own "web interface" setting, in the registry (they keep it there unless the
/// user chose an `.ini` file beside the program).
#[cfg(windows)]
pub mod reg {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_DWORD, RRF_RT_REG_DWORD,
        RegCloseKey, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
    };

    /// Where one player keeps the setting, under `HKEY_CURRENT_USER`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Place {
        pub player: &'static str,
        pub key: &'static str,
        /// 1: the web interface starts with the player.
        pub enable: &'static str,
        pub port: &'static str,
        /// 1: only this computer may connect.
        pub local_only: &'static str,
    }

    /// MPC-HC (`SettingsDefines.h`: `IDS_R_SETTINGS`, `IDS_RS_ENABLEWEBSERVER`, …).
    pub const MPC_HC: Place = Place {
        player: "MPC-HC",
        key: r"Software\MPC-HC\MPC-HC\Settings",
        enable: "EnableWebServer",
        port: "WebServerPort",
        local_only: "WebServerLocalhostOnly",
    };
    /// MPC-BE (`IDS_R_WEBSERVER`).
    pub const MPC_BE: Place = Place {
        player: "MPC-BE",
        key: r"Software\MPC-BE\WebServer",
        enable: "EnableWebServer",
        port: "Port",
        local_only: "LocalhostOnly",
    };

    /// What the registry says.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Web {
        pub enabled: bool,
        /// The port, when the player saved one (else its default, 13579).
        pub port: Option<u16>,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn io(code: u32) -> std::io::Error {
        std::io::Error::from_raw_os_error(code as i32)
    }

    fn dword(key: &[u16], name: &str) -> std::io::Result<Option<u32>> {
        let (mut value, mut size) = (0u32, 4u32);
        // SAFETY: the strings are NUL-terminated; `value` is 4 bytes and `size` says so.
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                wide(name).as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut value as *mut u32).cast(),
                &mut size,
            )
        };
        match r {
            ERROR_SUCCESS => Ok(Some(value)),
            ERROR_FILE_NOT_FOUND => Ok(None),
            e => Err(io(e)),
        }
    }

    fn exists(key: &[u16]) -> std::io::Result<bool> {
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: the key name is NUL-terminated; the handle is closed when it opened.
        let r =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_QUERY_VALUE, &mut h) };
        match r {
            ERROR_SUCCESS => {
                // SAFETY: `h` was just opened.
                unsafe { RegCloseKey(h) };
                Ok(true)
            }
            ERROR_FILE_NOT_FOUND => Ok(false),
            e => Err(io(e)),
        }
    }

    /// The setting now. `None`: the player has no settings in the registry (never started, or
    /// it keeps them in an `.ini` file).
    pub fn read(place: &Place) -> std::io::Result<Option<Web>> {
        let key = wide(place.key);
        if !exists(&key)? {
            return Ok(None);
        }
        let enabled = dword(&key, place.enable)?.is_some_and(|v| v != 0);
        let port = dword(&key, place.port)?
            .and_then(|p| u16::try_from(p).ok())
            .filter(|p| *p != 0);
        Ok(Some(Web { enabled, port }))
    }

    /// Turns the web interface on for the player's next start, reachable from this computer
    /// only. Nothing is written when it is on already, and no key is created: `Ok(None)` when
    /// the player has no settings in the registry. The player must not be running (it writes
    /// its settings back when it closes).
    pub fn enable(place: &Place) -> std::io::Result<Option<Web>> {
        let Some(now) = read(place)? else {
            return Ok(None);
        };
        if now.enabled {
            return Ok(Some(now));
        }
        let key = wide(place.key);
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: the key name is NUL-terminated; the handle is closed below.
        let r = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_SET_VALUE, &mut h) };
        if r != ERROR_SUCCESS {
            return Err(io(r));
        }
        let one = 1u32.to_le_bytes();
        let mut result = Ok(());
        for name in [place.local_only, place.enable] {
            // SAFETY: `h` is open for writing; the name is NUL-terminated; the data is 4 bytes.
            let r =
                unsafe { RegSetValueExW(h, wide(name).as_ptr(), 0, REG_DWORD, one.as_ptr(), 4) };
            if r != ERROR_SUCCESS {
                result = Err(io(r));
                break;
            }
        }
        // SAFETY: `h` was opened above.
        unsafe { RegCloseKey(h) };
        result?;
        read(place)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of MPC-HC's `variables.html` (made-up file).
    const PLAYING: &str = r#"<!DOCTYPE html>
<html lang="en">
    <head><meta charset="utf-8"><title>MPC-HC WebServer - Variables</title></head>
    <body class="page-variables">
        <p id="file">[Hoshi] Lanterns &amp; Rain - 06 (1080p).mkv</p>
        <p id="filepatharg">D:%5cAnime%5c%5bHoshi%5d%20Lanterns%20%26%20Rain%20-%2006%20(1080p).mkv</p>
        <p id="filepath">D:\Anime\[Hoshi] Lanterns &amp; Rain - 06 (1080p).mkv</p>
        <p id="filedirarg">D:%5cAnime</p>
        <p id="filedir">D:\Anime</p>
        <p id="state">2</p>
        <p id="statestring">Playing</p>
        <p id="position">731500</p>
        <p id="positionstring">00:12:11</p>
        <p id="duration">1421000</p>
        <p id="durationstring">00:23:41</p>
        <p id="volumelevel">100</p>
        <p id="muted">0</p>
        <p id="playbackrate">1</p>
        <p id="size">1.2 GB</p>
        <p id="reloadtime">0</p>
        <p id="version">2.3.9.0</p>
    </body>
</html>"#;

    #[test]
    fn reads_mpcs_variables() {
        let p = parse_variables(PLAYING, "MPC-HC", 3.0).unwrap();
        assert_eq!(
            p.path.as_deref(),
            Some(r"D:\Anime\[Hoshi] Lanterns & Rain - 06 (1080p).mkv")
        );
        assert_eq!(
            (p.state, p.position, p.duration, p.rate, p.at),
            (PlayState::Playing, Some(731.5), Some(1421.0), 1.0, 3.0)
        );
        assert_eq!(
            (p.player.as_str(), p.switch, p.via),
            ("MPC-HC", Switch::Mpc, Via::MpcHttp)
        );
        let paused = parse_variables(
            &PLAYING.replace(r#"<p id="state">2"#, r#"<p id="state">1"#),
            "MPC-BE",
            0.0,
        )
        .unwrap();
        assert_eq!(
            (paused.state, paused.player.as_str()),
            (PlayState::Paused, "MPC-BE")
        );
        // Nothing loaded.
        let none = parse_variables(r#"<p id="file"></p><p id="filepath"></p><p id="state">-1</p><p id="position">0</p><p id="duration">0</p><p id="playbackrate">1</p>"#, "MPC-HC", 0.0).unwrap();
        assert_eq!(
            (none.state, none.path, none.duration),
            (PlayState::Stopped, None, None)
        );
        assert!(parse_variables("<html>hello</html>", "MPC-HC", 0.0).is_none());
        assert!(parse_variables(r#"<p id="state">Playing</p>"#, "MPC-HC", 0.0).is_none());
    }

    #[test]
    fn html_entities_are_undone() {
        assert_eq!(
            unescape("A &amp; B &lt;3&gt; &quot;x&quot; it&#39;s R&D &"),
            "A & B <3> \"x\" it's R&D &"
        );
    }

    /// A fake web interface on a free local port: the real request and answer.
    #[test]
    fn asks_a_web_interface() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            // The first connection is the "does anything listen" probe.
            for _ in 0..2 {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = [0u8; 2048];
                let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
                let n = s.read(&mut buf).unwrap_or(0);
                if n > 0 {
                    let asked = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let _ = write!(
                        s,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{PLAYING}",
                        PLAYING.len()
                    );
                    return asked;
                }
            }
            String::new()
        });
        let Got::Status(p) = fetch(port, "MPC-HC") else {
            panic!("no status")
        };
        assert_eq!(p.position, Some(731.5));
        assert!(server.join().unwrap().starts_with("GET /variables.html "));
        // Nothing listens there any more.
        assert!(matches!(fetch(port, "MPC-HC"), Got::NoAnswer));
    }

    /// The thread asks nothing while the process scan says no MPC runs, and names the player
    /// when its web interface is off.
    #[test]
    fn the_thread_waits_for_an_mpc_process() {
        let shared = std::sync::Arc::new(Shared::for_tests(crate::Settings {
            mpc_port: 1,
            ..Default::default()
        }));
        shared.set_mpc_process(MpcProcess::NotRunning);
        let (tx, rx) = std::sync::mpsc::channel();
        let (wake, wrx) = std::sync::mpsc::channel();
        let sh = shared.clone();
        std::thread::spawn(move || run(sh, wrx, tx));
        let line = |i: Input| match i {
            Input::Status(s) => s.line,
            other => format!("{other:?}"),
        };
        assert_eq!(
            line(rx.recv_timeout(Duration::from_secs(5)).unwrap()),
            "web interface: not running"
        );
        assert!(
            rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "blocked: nothing polls"
        );
        // The player starts (port 1: nothing listens).
        shared.set_mpc_process(MpcProcess::Running("MPC-BE"));
        wake.send(()).unwrap();
        assert_eq!(
            line(rx.recv_timeout(Duration::from_secs(10)).unwrap()),
            "web interface is off in MPC-BE"
        );
    }

    /// The registry setting, on a scratch key (never the players' own).
    #[cfg(windows)]
    #[test]
    fn the_registry_setting_is_turned_on_once() {
        use windows_sys::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_ALL_ACCESS, REG_DWORD, RegCloseKey, RegCreateKeyExW,
            RegDeleteTreeW, RegSetValueExW,
        };
        let wide = |s: &str| {
            s.encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        };
        let root = format!(r"Software\KotoraTest-{}", std::process::id());
        let key: &'static str = Box::leak(format!(r"{root}\Settings").into_boxed_str());
        let place = reg::Place {
            player: "MPC-HC",
            key,
            ..reg::MPC_HC
        };
        // No key: nothing is created.
        assert_eq!(reg::read(&place).unwrap(), None);
        assert_eq!(reg::enable(&place).unwrap(), None);
        assert_eq!(reg::read(&place).unwrap(), None);
        // The player's settings exist, web interface off, a port of the user's.
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: plain registry calls on a scratch key of this test; strings are NUL-terminated.
        unsafe {
            assert_eq!(
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    wide(key).as_ptr(),
                    0,
                    std::ptr::null(),
                    0,
                    KEY_ALL_ACCESS,
                    std::ptr::null(),
                    &mut h,
                    std::ptr::null_mut()
                ),
                0
            );
            for (name, v) in [("EnableWebServer", 0u32), ("WebServerPort", 13580)] {
                assert_eq!(
                    RegSetValueExW(
                        h,
                        wide(name).as_ptr(),
                        0,
                        REG_DWORD,
                        v.to_le_bytes().as_ptr(),
                        4
                    ),
                    0
                );
            }
            RegCloseKey(h);
        }
        assert_eq!(
            reg::read(&place).unwrap(),
            Some(reg::Web {
                enabled: false,
                port: Some(13580)
            })
        );
        assert_eq!(
            reg::enable(&place).unwrap(),
            Some(reg::Web {
                enabled: true,
                port: Some(13580)
            })
        );
        assert_eq!(
            reg::enable(&place).unwrap(),
            Some(reg::Web {
                enabled: true,
                port: Some(13580)
            })
        );
        // SAFETY: deletes the scratch key made above.
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(&root).as_ptr()) };
        assert_eq!(reg::read(&place).unwrap(), None);
    }
}
