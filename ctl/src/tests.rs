use super::*;

fn temp_socket(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kotora-ctl-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("ctl.sock")
}

fn sample() -> Status {
    Status {
        version: "0.1.0 (build 1)".into(),
        account: Some("demo_user".into()),
        service: "MyAnimeList".into(),
        entries: 303,
        queue: 4,
        offline: false,
        window: true,
    }
}

#[test]
fn wire_format_is_readable() {
    assert_eq!(serde_json::to_string(&Request::Status).unwrap(), r#"{"cmd":"status"}"#);
    assert_eq!(serde_json::to_string(&Request::Show).unwrap(), r#"{"cmd":"show"}"#);
    assert_eq!(serde_json::to_string(&Request::Quit).unwrap(), r#"{"cmd":"quit"}"#);
    let text = serde_json::to_string(&Reply::Status(sample())).unwrap();
    assert_eq!(
        text,
        r#"{"reply":"status","version":"0.1.0 (build 1)","account":"demo_user","service":"MyAnimeList","entries":303,"queue":4,"offline":false,"window":true}"#
    );
    let err = Reply::Error { message: "nope".into(), anyway: false };
    assert_eq!(serde_json::to_string(&err).unwrap(), r#"{"reply":"error","message":"nope"}"#);
    // Old bytes stay the same; the new fields show only when set.
    assert_eq!(serde_json::to_string(&Request::PlusOne { title: "x".into(), anyway: false }).unwrap(), r#"{"cmd":"plus_one","title":"x"}"#);
    assert_eq!(serde_json::to_string(&Request::PlusOne { title: "x".into(), anyway: true }).unwrap(), r#"{"cmd":"plus_one","title":"x","anyway":true}"#);
    let held = Reply::Error { message: "Episode 9 hasn't aired yet.".into(), anyway: true };
    assert_eq!(serde_json::to_string(&held).unwrap(), r#"{"reply":"error","message":"Episode 9 hasn't aired yet.","anyway":true}"#);
}

#[test]
fn requests_and_replies_round_trip() {
    let set = Request::Set { title: "Lanterns".into(), progress: Some(6), status: None, score: Some(0), anyway: true };
    for req in [Request::Status, Request::Show, Request::Quit, Request::List { status: None }, Request::PlusOne { title: "x".into(), anyway: false }, Request::PlusOne { title: "x".into(), anyway: true }, set, Request::Queue, Request::Sync, Request::Now, Request::Confirm, Request::Dismiss, Request::SetClientId { id: "0f".into() }, Request::Add { query: "Lanterns".into(), status: Some("plan".into()) }, Request::Add { query: "9001".into(), status: None }, Request::Rewatch { title: "Neon".into() }] {
        let text = serde_json::to_string(&req).unwrap();
        assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), req);
    }
    let signed_out = Status { account: None, service: String::new(), ..sample() };
    for reply in [Reply::Status(sample()), Reply::Status(signed_out), Reply::Status(Status::default()), Reply::Error { message: "x".into(), anyway: false }, Reply::Done { message: "Saved.".into() }] {
        let text = serde_json::to_string(&reply).unwrap();
        assert_eq!(serde_json::from_str::<Reply>(&text).unwrap(), reply);
    }
    let now = NowInfo { player: "mpv".into(), file: "x - 06.mkv".into(), title: "X".into(), episode: Some(6), offer: Some(6), percent: Some(82), ..Default::default() };
    for reply in [Reply::Now { now: None }, Reply::Now { now: Some(now) }] {
        let text = serde_json::to_string(&reply).unwrap();
        assert_eq!(serde_json::from_str::<Reply>(&text).unwrap(), reply);
    }
    assert_eq!(serde_json::to_string(&Request::Now).unwrap(), r#"{"cmd":"now"}"#);
    // Signed out: no account field on the wire.
    let text = serde_json::to_string(&Reply::Status(Status::default())).unwrap();
    assert!(!text.contains("account"), "{text}");
}

/// A new `kot` reads an old app's status (fields missing) and ignores fields it doesn't know (a
/// newer app's).
#[test]
fn a_new_client_reads_an_old_or_newer_app() {
    let old = r#"{"reply":"status","version":"0.1.0 (build 1)"}"#;
    let Reply::Status(st) = serde_json::from_str::<Reply>(old).unwrap() else { panic!("not a status") };
    assert_eq!(st, Status { version: "0.1.0 (build 1)".into(), ..Default::default() });
    let newer = r#"{"reply":"status","version":"9.0.0 (build 1)","entries":2,"something_new":[1,2],"now":{"title":"x"}}"#;
    let Reply::Status(st) = serde_json::from_str::<Reply>(newer).unwrap() else { panic!("not a status") };
    assert_eq!(st.entries, 2);
}

/// An old `kot`, which knew fewer fields, still reads this app's status.
#[test]
fn an_old_client_reads_a_new_app() {
    #[derive(Deserialize)]
    #[serde(tag = "reply", rename_all = "snake_case")]
    #[allow(dead_code)]
    enum OldReply {
        Status { version: String },
        Error { message: String },
    }
    let text = serde_json::to_string(&Reply::Status(sample())).unwrap();
    let OldReply::Status { version } = serde_json::from_str::<OldReply>(&text).unwrap() else { panic!("not a status") };
    assert_eq!(version, "0.1.0 (build 1)");
}

/// An app that doesn't know a request answers "bad request" (the server's parse error), which
/// `kot` shows as "update Kotora".
#[test]
fn an_old_app_refuses_an_unknown_request() {
    assert!(serde_json::from_str::<Request>(r#"{"cmd":"plus_one","id":1}"#).is_err());
}

#[test]
fn serve_answers_in_order_and_cleans_up() {
    let path = temp_socket("order");
    let server = serve(&path, |req| match req {
        Request::Status => Reply::Status(sample()),
        _ => Reply::Error { message: "nope".into(), anyway: false },
    })
    .unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    let mut c = Client::connect_to(&path).unwrap();
    assert_eq!(c.request(&Request::Status).unwrap(), Reply::Status(sample()));
    assert_eq!(c.request(&Request::Show).unwrap(), Reply::Error { message: "nope".into(), anyway: false });
    // A second app on the same path is refused while the first answers: single instance.
    assert_eq!(serve(&path, |_| Reply::Error { message: String::new(), anyway: false }).err().map(|e| e.kind()), Some(io::ErrorKind::AddrInUse));
    drop(server);
    assert!(!path.exists());
    assert!(Client::connect_to(&path).is_err(), "nobody answers after the app ends");
}

#[test]
fn stale_socket_is_replaced_and_bad_lines_get_an_error() {
    let path = temp_socket("stale");
    {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let _dead = UnixListener::bind(&path).unwrap(); // bound, then dropped: nobody answers
    }
    assert!(path.exists());
    let _server = serve(&path, |_| Reply::Status(Status::default())).unwrap();
    let mut s = UnixStream::connect(&path).unwrap();
    s.write_all(b"not json\n").unwrap();
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).unwrap();
    assert!(line.contains(r#""reply":"error""#), "{line}");
    assert!(line.contains("bad request"), "{line}");
}

#[test]
fn the_socket_path_follows_the_override() {
    // Only reads the variable; tests in this binary don't set it, so the default applies.
    let p = socket_path();
    match std::env::var_os("KOTORA_CTL_SOCKET").filter(|v| !v.is_empty()) {
        Some(v) => assert_eq!(p, PathBuf::from(v)),
        None => assert_eq!(p, runtime_dir().join("kotora.sock")),
    }
}

/// The Dock-started app and `kot` over ssh agree on the folder even when `$TMPDIR` is unset, and
/// the socket path fits a macOS `sun_path` (104 bytes).
#[test]
#[cfg(target_os = "macos")]
fn macos_socket_folder_comes_from_the_system() {
    let temp = darwin_user_temp_dir().expect("confstr(_CS_DARWIN_USER_TEMP_DIR)");
    assert!(temp.is_dir(), "{} is a folder", temp.display());
    let dir = runtime_dir();
    assert_eq!(dir, temp.join(format!("kotora-{}", uid())));
    assert!(dir.join("kotora.sock").as_os_str().len() < 104);
}

#[test]
fn the_build_file_reads_as_a_number() {
    assert_eq!(parse_build("1\n"), 1);
    assert_eq!(parse_build(" 12 \r\n"), 12);
    assert_eq!(BUILD.to_string(), include_str!("../../BUILD").trim());
    assert_eq!(version_line(), format!("{} (build {BUILD}){}", env!("CARGO_PKG_VERSION"), dev_suffix()));
}

#[test]
#[should_panic]
fn two_numbers_are_refused() {
    parse_build("1 2");
}

#[test]
#[should_panic]
fn zero_is_refused() {
    parse_build("0");
}

fn facts(branch: &'static str, hash: &'static str, dirty: bool, at_release_tag: bool) -> Option<GitFacts<'static>> {
    Some(GitFacts { branch, hash, dirty, at_release_tag })
}

#[test]
fn the_dev_suffix_follows_the_rule() {
    assert_eq!(git_suffix(None), "");
    assert_eq!(git_suffix(facts("main", "20c1fe9", false, true)), "");
    assert_eq!(git_suffix(facts("main", "20c1fe9", false, false)), " \u{b7} main 20c1fe9");
    assert_eq!(git_suffix(facts("stage-1", "1a2b3c4", true, false)), " \u{b7} stage-1 1a2b3c4 dirty");
    assert_eq!(git_suffix(facts("main", "20c1fe9", true, true)), " \u{b7} main 20c1fe9 dirty");
}

#[test]
fn the_build_script_facts_parse() {
    assert_eq!(parse_facts(""), None);
    assert_eq!(parse_facts("main\t20c1fe9\t1\t0"), facts("main", "20c1fe9", true, false));
    assert_eq!(parse_facts("main\t20c1fe9"), None);
}

#[test]
fn stage_3_wire_format() {
    assert_eq!(serde_json::to_string(&Request::List { status: Some("watching".into()) }).unwrap(), r#"{"cmd":"list","status":"watching"}"#);
    assert_eq!(serde_json::to_string(&Request::List { status: None }).unwrap(), r#"{"cmd":"list"}"#);
    assert_eq!(serde_json::to_string(&Request::PlusOne { title: "Neon".into(), anyway: false }).unwrap(), r#"{"cmd":"plus_one","title":"Neon"}"#);
    assert_eq!(
        serde_json::to_string(&Request::Set { title: "Neon".into(), progress: Some(3), status: None, score: None, anyway: false }).unwrap(),
        r#"{"cmd":"set","title":"Neon","progress":3}"#
    );
    assert_eq!(serde_json::to_string(&Request::Queue).unwrap(), r#"{"cmd":"queue"}"#);
    assert_eq!(serde_json::to_string(&Request::Sync).unwrap(), r#"{"cmd":"sync"}"#);
    let e = Entry { id: 7, title: "Neon".into(), status: "Watching".into(), progress: 3, episodes: Some(12), score: None, waiting: None, rewatching: false, times_rewatched: 0 };
    let text = serde_json::to_string(&Reply::Edited { entry: e.clone() }).unwrap();
    assert_eq!(text, r#"{"reply":"edited","entry":{"id":7,"title":"Neon","status":"Watching","progress":3,"episodes":12}}"#);
    assert_eq!(serde_json::from_str::<Reply>(&text).unwrap(), Reply::Edited { entry: e });
    let q = Reply::Queue { ops: vec![QueueItem { title: "Neon".into(), change: "Progress 2 \u{2192} 3".into(), state: "Waiting".into(), note: None }] };
    assert_eq!(serde_json::from_str::<Reply>(&serde_json::to_string(&q).unwrap()).unwrap(), q);
}

/// An app from stage 2 answers a stage 3 request with "bad request", which `kot` explains.
#[test]
fn an_old_app_refuses_new_requests_plainly() {
    #[derive(Deserialize)]
    #[serde(tag = "cmd", rename_all = "snake_case")]
    enum OldRequest {
        Status,
        Show,
        Quit,
    }
    let line = serde_json::to_string(&Request::Queue).unwrap();
    assert!(serde_json::from_str::<OldRequest>(&line).is_err());
    assert!(matches!(serde_json::from_str::<OldRequest>(r#"{"cmd":"quit"}"#), Ok(OldRequest::Quit)));
    let _ = (OldRequest::Status, OldRequest::Show);
}
