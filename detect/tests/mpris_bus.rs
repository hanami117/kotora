//! A fake MPRIS player on a private session bus: the source finds it when its name appears,
//! reads its file, and reports it gone when it leaves.
//!
//! Runs only inside `dbus-run-session` with `KOTORA_PRIVATE_BUS=1` (the Linux Docker check does
//! this), so it never touches the user's own bus or players.

#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;

use kotora_detect::run::Input;
use kotora_detect::{PlayState, Switch};
use zbus::zvariant::{OwnedValue, Value};

struct Root;

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        "Fake Player".into()
    }
}

struct Player;

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        "Playing".into()
    }
    #[zbus(property)]
    fn position(&self) -> i64 {
        300_000_000
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let mut m = HashMap::new();
        m.insert(
            "xesam:url".into(),
            OwnedValue::try_from(Value::new(
                "file:///v/Starfall%20Delivery%20Service%20S2%20-%2003.mkv",
            ))
            .unwrap(),
        );
        m.insert(
            "mpris:length".into(),
            OwnedValue::try_from(Value::new(1_400_000_000i64)).unwrap(),
        );
        m
    }
}

#[test]
fn a_fake_player_comes_and_goes() {
    if std::env::var_os("KOTORA_PRIVATE_BUS").is_none() {
        eprintln!("skipped: run inside dbus-run-session with KOTORA_PRIVATE_BUS=1");
        return;
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || kotora_detect::mpris::run(tx));
    // Let the source subscribe first, so the player arrives through NameOwnerChanged.
    std::thread::sleep(Duration::from_millis(300));
    let player = zbus::blocking::connection::Builder::session()
        .unwrap()
        .name("org.mpris.MediaPlayer2.fakeplayer")
        .unwrap()
        .serve_at("/org/mpris/MediaPlayer2", Root)
        .unwrap()
        .serve_at("/org/mpris/MediaPlayer2", Player)
        .unwrap()
        .build()
        .unwrap();
    let p = loop {
        match rx
            .recv_timeout(Duration::from_secs(10))
            .expect("an update from the fake player")
        {
            Input::Update(p) => break p,
            _ => continue,
        }
    };
    assert_eq!(p.key, "mpris:org.mpris.MediaPlayer2.fakeplayer");
    assert_eq!(p.player, "Fake Player");
    assert_eq!(p.switch, Switch::OtherMpris);
    assert_eq!(
        p.path.as_deref(),
        Some("/v/Starfall Delivery Service S2 - 03.mkv")
    );
    assert_eq!(
        (p.state, p.position, p.duration),
        (PlayState::Playing, Some(300.0), Some(1400.0))
    );
    drop(player);
    let gone = loop {
        match rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the player leaves")
        {
            Input::Gone(k) => break k,
            _ => continue,
        }
    };
    assert_eq!(gone, "mpris:org.mpris.MediaPlayer2.fakeplayer");
}
