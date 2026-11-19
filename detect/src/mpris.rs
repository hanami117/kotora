//! MPRIS players on the session bus (Linux): VLC, mpc-qt, mpv with mpv-mpris, and others (Haruna,
//! Celluloid, …) that report a local file in `xesam:url`. Browsers and music players without a
//! `file://` URL are skipped.
//!
//! zbus 5, blocking API, no tokio: the same zbus Rune's AT-SPI accessibility builds. Kotora asks
//! the bus for two kinds of signals only, `NameOwnerChanged` for `org.mpris.MediaPlayer2.*` names
//! and anything sent at `/org/mpris/MediaPlayer2` (`PropertiesChanged`, `Seeked`), and reads a
//! player's properties after one of them. With no player running, nothing arrives.

use std::collections::HashMap;
use std::sync::mpsc::{self, Sender};

use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type;
use zbus::zvariant::{OwnedValue, Value};

use crate::run::Input;
use crate::{Led, PlayState, Playing, SourceStatus, Switch, Via};

pub const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

/// The Settings switch of a bus name: `org.mpris.MediaPlayer2.vlc`, `….MpcQt.instance2`, ….
pub fn switch_for(bus_name: &str) -> Switch {
    let n = bus_name
        .strip_prefix(PREFIX)
        .unwrap_or(bus_name)
        .to_lowercase();
    if n.starts_with("vlc") {
        Switch::Vlc
    } else if n.starts_with("mpcqt") || n.starts_with("mpc-qt") || n.starts_with("mpc_qt") {
        Switch::MpcQt
    } else if n.starts_with("mpv") {
        Switch::Mpv
    } else {
        Switch::OtherMpris
    }
}

fn unbox<'a>(v: &'a Value<'a>) -> &'a Value<'a> {
    match v {
        Value::Value(b) => unbox(b),
        v => v,
    }
}

fn as_str(v: &Value) -> Option<String> {
    match unbox(v) {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        _ => None,
    }
}

fn as_f64(v: &Value) -> Option<f64> {
    match unbox(v) {
        Value::I64(n) => Some(*n as f64),
        Value::U64(n) => Some(*n as f64),
        Value::I32(n) => Some(*n as f64),
        Value::U32(n) => Some(*n as f64),
        Value::F64(n) => Some(*n),
        _ => None,
    }
}

/// The player's `org.mpris.MediaPlayer2.Player` properties → a snapshot. `None` when it plays
/// no local file.
pub fn from_props(
    bus_name: &str,
    identity: &str,
    props: &HashMap<String, OwnedValue>,
    at: f64,
) -> Option<Playing> {
    let mut url = None;
    let mut length = None;
    if let Some(Value::Dict(md)) = props.get("Metadata").map(|v| unbox(v)) {
        for (k, v) in md.iter() {
            match as_str(k).as_deref() {
                Some("xesam:url") => url = as_str(v),
                Some("mpris:length") => length = as_f64(v),
                _ => {}
            }
        }
    }
    let path = crate::file_url_path(url.as_deref()?)?;
    let state = match props
        .get("PlaybackStatus")
        .and_then(|v| as_str(v))
        .as_deref()
    {
        Some("Playing") => PlayState::Playing,
        Some("Paused") => PlayState::Paused,
        _ => PlayState::Stopped,
    };
    Some(Playing {
        key: format!("mpris:{bus_name}"),
        player: if identity.is_empty() {
            bus_name
                .strip_prefix(PREFIX)
                .unwrap_or(bus_name)
                .to_string()
        } else {
            identity.to_string()
        },
        switch: switch_for(bus_name),
        via: Via::Mpris,
        path: Some(path),
        title: None,
        // Microseconds on the bus.
        position: props
            .get("Position")
            .and_then(|v| as_f64(v))
            .map(|us| us / 1e6),
        duration: length.map(|us| us / 1e6).filter(|d| *d > 0.0),
        state,
        rate: props
            .get("Rate")
            .and_then(|v| as_f64(v))
            .filter(|r| *r > 0.0)
            .unwrap_or(1.0),
        at,
    })
}

enum Ev {
    Owner { name: String, new_owner: String },
    Signal { sender: String },
}

struct Player {
    name: String,
    owner: String,
    identity: String,
    /// It reported a playable file last time (so a stop reports it gone).
    shown: bool,
}

fn status_for(players: &[Player], playing: &HashMap<String, bool>, switch: Switch) -> SourceStatus {
    let mine: Vec<&Player> = players
        .iter()
        .filter(|p| switch_for(&p.name) == switch)
        .collect();
    let (line, led) = if mine.is_empty() {
        ("not running".to_string(), Led::Off)
    } else if mine
        .iter()
        .any(|p| playing.get(&p.name).copied().unwrap_or(false))
    {
        ("playing".to_string(), Led::Playing)
    } else {
        ("running, nothing playing".to_string(), Led::Idle)
    };
    SourceStatus {
        switch,
        via: Via::Mpris,
        line,
        led,
    }
}

/// The thread: follows MPRIS players until the bus goes away.
pub fn run(tx: Sender<Input>) {
    let fail = |m: String| {
        for sw in [Switch::Mpv, Switch::Vlc, Switch::MpcQt, Switch::OtherMpris] {
            let _ = tx.send(Input::Status(SourceStatus {
                switch: sw,
                via: Via::Mpris,
                line: m.clone(),
                led: Led::Problem,
            }));
        }
    };
    let conn = match Connection::session() {
        Ok(c) => c,
        Err(e) => return fail(format!("no session bus: {e}")),
    };
    if let Err(e) = follow(&conn, &tx) {
        fail(format!("session bus: {e}"));
    }
}

fn follow(conn: &Connection, tx: &Sender<Input>) -> zbus::Result<()> {
    let (etx, erx) = mpsc::channel::<Ev>();
    let owners = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg0ns("org.mpris.MediaPlayer2")?
        .build();
    let owners = MessageIterator::for_match_rule(owners, conn, Some(64))?;
    let signals = MatchRule::builder()
        .msg_type(Type::Signal)
        .path(PATH)?
        .build();
    let signals = MessageIterator::for_match_rule(signals, conn, Some(64))?;
    let t1 = etx.clone();
    std::thread::Builder::new()
        .name("kotora-mpris-names".into())
        .spawn(move || {
            for m in owners.flatten() {
                if let Ok((name, _old, new_owner)) =
                    m.body().deserialize::<(String, String, String)>()
                    && t1.send(Ev::Owner { name, new_owner }).is_err()
                {
                    return;
                }
            }
        })?;
    std::thread::Builder::new()
        .name("kotora-mpris-signals".into())
        .spawn(move || {
            for m in signals.flatten() {
                let sender = m
                    .header()
                    .sender()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                if etx.send(Ev::Signal { sender }).is_err() {
                    return;
                }
            }
        })?;

    let dbus = zbus::blocking::fdo::DBusProxy::new(conn)?;
    let mut players: Vec<Player> = Vec::new();
    let mut playing: HashMap<String, bool> = HashMap::new();
    for name in dbus.list_names()? {
        let name = name.to_string();
        if name.starts_with(PREFIX) {
            let owner = dbus
                .get_name_owner(name.as_str().try_into()?)
                .map(|o| o.to_string())
                .unwrap_or_default();
            players.push(Player {
                identity: identity(conn, &name),
                name,
                owner,
                shown: false,
            });
        }
    }
    let names: Vec<String> = players.iter().map(|p| p.name.clone()).collect();
    for n in names {
        refresh(conn, &mut players, &mut playing, &n, tx);
    }
    send_states(&players, &playing, tx);
    for ev in erx {
        match ev {
            Ev::Owner { name, new_owner } => {
                if let Some(i) = players.iter().position(|p| p.name == name) {
                    let p = players.remove(i);
                    playing.remove(&p.name);
                    if tx.send(Input::Gone(format!("mpris:{}", p.name))).is_err() {
                        return Ok(());
                    }
                }
                if !new_owner.is_empty() {
                    players.push(Player {
                        identity: identity(conn, &name),
                        name: name.clone(),
                        owner: new_owner,
                        shown: false,
                    });
                    refresh(conn, &mut players, &mut playing, &name, tx);
                }
            }
            Ev::Signal { sender } => {
                let Some(name) = players
                    .iter()
                    .find(|p| p.owner == sender)
                    .map(|p| p.name.clone())
                else {
                    continue;
                };
                refresh(conn, &mut players, &mut playing, &name, tx);
            }
        }
        send_states(&players, &playing, tx);
    }
    Ok(())
}

fn send_states(players: &[Player], playing: &HashMap<String, bool>, tx: &Sender<Input>) {
    for sw in [Switch::Mpv, Switch::Vlc, Switch::MpcQt, Switch::OtherMpris] {
        let _ = tx.send(Input::Status(status_for(players, playing, sw)));
    }
}

fn identity(conn: &Connection, name: &str) -> String {
    conn.call_method(
        Some(name),
        PATH,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &("org.mpris.MediaPlayer2", "Identity"),
    )
    .ok()
    .and_then(|r| r.body().deserialize::<OwnedValue>().ok())
    .and_then(|v| as_str(&v))
    .unwrap_or_default()
}

fn refresh(
    conn: &Connection,
    players: &mut [Player],
    playing: &mut HashMap<String, bool>,
    name: &str,
    tx: &Sender<Input>,
) {
    let Some(p) = players.iter_mut().find(|p| p.name == name) else {
        return;
    };
    let props = conn
        .call_method(
            Some(name),
            PATH,
            Some("org.freedesktop.DBus.Properties"),
            "GetAll",
            &(PLAYER,),
        )
        .ok()
        .and_then(|r| r.body().deserialize::<HashMap<String, OwnedValue>>().ok());
    let snap = props.and_then(|props| from_props(name, &p.identity, &props, crate::clock()));
    playing.insert(
        name.to_string(),
        snap.as_ref().is_some_and(|s| s.state == PlayState::Playing),
    );
    match snap {
        Some(s) => {
            p.shown = true;
            let _ = tx.send(Input::Update(s));
        }
        None if p.shown => {
            p.shown = false;
            let _ = tx.send(Input::Gone(format!("mpris:{name}")));
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Dict;

    fn ov(v: Value<'static>) -> OwnedValue {
        OwnedValue::try_from(v).unwrap()
    }

    /// Properties as VLC 3 answers `GetAll` (recorded shape; made-up file).
    fn vlc_props(url: &str) -> HashMap<String, OwnedValue> {
        let mut md: HashMap<String, Value> = HashMap::new();
        md.insert("xesam:url".into(), Value::new(url.to_string()));
        md.insert(
            "xesam:title".into(),
            Value::new("Lanterns of Kirigaoka - 06".to_string()),
        );
        md.insert("mpris:length".into(), Value::new(1_420_500_000i64));
        md.insert(
            "mpris:trackid".into(),
            Value::new(
                zbus::zvariant::ObjectPath::try_from("/org/videolan/vlc/playlist/4").unwrap(),
            ),
        );
        let md = Dict::from(md);
        let mut p = HashMap::new();
        p.insert("Metadata".into(), ov(Value::Dict(md)));
        p.insert(
            "PlaybackStatus".into(),
            ov(Value::new("Playing".to_string())),
        );
        p.insert("Position".into(), ov(Value::new(731_000_000i64)));
        p.insert("Rate".into(), ov(Value::new(1.0f64)));
        p
    }

    #[test]
    fn reads_a_players_properties() {
        let p = from_props(
            "org.mpris.MediaPlayer2.vlc",
            "VLC media player",
            &vlc_props("file:///v/%5BHoshi%5D%20Lanterns%20of%20Kirigaoka%20-%2006.mkv"),
            2.0,
        )
        .unwrap();
        assert_eq!(p.key, "mpris:org.mpris.MediaPlayer2.vlc");
        assert_eq!(p.player, "VLC media player");
        assert_eq!(
            p.path.as_deref(),
            Some("/v/[Hoshi] Lanterns of Kirigaoka - 06.mkv")
        );
        assert_eq!(
            (p.state, p.position, p.duration, p.switch),
            (PlayState::Playing, Some(731.0), Some(1420.5), Switch::Vlc)
        );
        // A stream or a browser tab: skipped.
        assert!(
            from_props(
                "org.mpris.MediaPlayer2.firefox",
                "",
                &vlc_props("https://example.invalid/watch"),
                0.0
            )
            .is_none()
        );
        assert!(from_props("org.mpris.MediaPlayer2.x", "", &HashMap::new(), 0.0).is_none());
    }

    #[test]
    fn bus_names_pick_their_switch() {
        assert_eq!(switch_for("org.mpris.MediaPlayer2.vlc"), Switch::Vlc);
        assert_eq!(
            switch_for("org.mpris.MediaPlayer2.MpcQt.instance2"),
            Switch::MpcQt
        );
        assert_eq!(switch_for("org.mpris.MediaPlayer2.mpv"), Switch::Mpv);
        assert_eq!(
            switch_for("org.mpris.MediaPlayer2.haruna"),
            Switch::OtherMpris
        );
    }
}
