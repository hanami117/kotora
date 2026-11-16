//! Library tests. Every title, group and path here is made up.

use super::*;
use crate::Status;
use crate::store::tests::item;

#[test]
fn name_rules_skip_extras() {
    for name in [
        "[Hoshi] Lanterns of Kirigaoka - 03 [1080p].mkv",
        "Lanterns of Kirigaoka S02E07.mp4",
        "Clockwork Tanuki - 12v2.MKV",
        "06.webm",
        "Edge of the Opal Sea - 04.mkv",
        "Operation Teddy - 02.mkv",
    ] {
        assert!(is_episode_file(name), "{name} is an episode");
    }
    for name in [
        "[Hoshi] Lanterns of Kirigaoka - NCOP [1080p].mkv",
        "[Hoshi] Lanterns of Kirigaoka - NCED2 [1080p].mkv",
        "Lanterns of Kirigaoka - NC OP 01.mkv",
        "Lanterns of Kirigaoka - OP1.mkv",
        "Lanterns of Kirigaoka - ED02.mkv",
        "Lanterns of Kirigaoka - PV2.mkv",
        "Lanterns of Kirigaoka - CM01.mkv",
        "Lanterns of Kirigaoka - Menu1.mkv",
        "lanterns.of.kirigaoka.sample.mkv",
        "Lanterns of Kirigaoka - Creditless Opening.mkv",
        "Lanterns of Kirigaoka - Trailer.mp4",
        "Lanterns of Kirigaoka - 03.ass",
        "Lanterns of Kirigaoka - 03.mkv.part",
        "._Lanterns of Kirigaoka - 03.mkv",
        ".hidden.mkv",
        "noextension",
    ] {
        assert!(!is_episode_file(name), "{name} is skipped");
    }
    for d in ["Extras", "SAMPLES", "NCOP", "Creditless", ".git", "@eaDir", "Featurettes"] {
        assert!(skip_dir(d), "{d}");
    }
    for d in ["Season 1", "Specials", "Lanterns of Kirigaoka"] {
        assert!(!skip_dir(d), "{d}");
    }
}

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn tmp(name: &str) -> Tmp {
    let d = std::env::temp_dir().join(format!("kot-lib-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Tmp(d)
}
fn touch(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"x").unwrap();
}

#[test]
fn walk_is_recursive_and_skips() {
    let t = tmp("walk");
    let r = &t.0;
    touch(&r.join("Lanterns of Kirigaoka/[Hoshi] Lanterns of Kirigaoka - 01.mkv"));
    touch(&r.join("Lanterns of Kirigaoka/Season 2/Lanterns of Kirigaoka S2 - 01.mkv"));
    touch(&r.join("Lanterns of Kirigaoka/Extras/Lanterns of Kirigaoka - 01.mkv"));
    touch(&r.join("Lanterns of Kirigaoka/Lanterns of Kirigaoka - NCOP.mkv"));
    touch(&r.join("Lanterns of Kirigaoka/notes.txt"));
    let mut got: Vec<String> = walk(r).unwrap().into_iter().map(|(p, ..)| p.strip_prefix(r).unwrap().to_string_lossy().into_owned()).collect();
    got.sort();
    assert_eq!(got, ["Lanterns of Kirigaoka/Season 2/Lanterns of Kirigaoka S2 - 01.mkv", "Lanterns of Kirigaoka/[Hoshi] Lanterns of Kirigaoka - 01.mkv"]);
}

#[cfg(unix)]
#[test]
fn walk_survives_a_link_loop() {
    let t = tmp("loop");
    touch(&t.0.join("A/A - 01.mkv"));
    std::os::unix::fs::symlink(&t.0, t.0.join("A/back")).unwrap();
    assert_eq!(walk(&t.0).unwrap().len(), 1);
}

#[test]
fn missing_or_empty_folders_are_offline_and_keep_their_files() {
    let t = tmp("offline");
    let folder = t.0.to_string_lossy().into_owned();
    touch(&t.0.join("Clockwork Tanuki - 01.mkv"));
    touch(&t.0.join("Clockwork Tanuki - 02.mkv"));
    let mut st = Store::open_memory().unwrap();
    let s = scan(&mut st, &folder).unwrap();
    assert_eq!(s, Scan { online: true, files: 2, added: 2, removed: 0 });
    // A rescan with nothing new parses nothing.
    assert_eq!(scan(&mut st, &folder).unwrap(), Scan { online: true, files: 2, added: 0, removed: 0 });
    // An empty folder (an unmounted mount point) is offline, not "everything deleted".
    for f in std::fs::read_dir(&t.0).unwrap() {
        std::fs::remove_file(f.unwrap().path()).unwrap();
    }
    assert_eq!(scan(&mut st, &folder).unwrap(), Scan { online: false, files: 2, added: 0, removed: 0 });
    // A missing folder (the drive unplugged): the same.
    std::fs::remove_dir_all(&t.0).unwrap();
    assert_eq!(scan(&mut st, &folder).unwrap(), Scan { online: false, files: 2, added: 0, removed: 0 });
    assert_eq!(load_folder(&st, &folder).unwrap().len(), 2);
    // Back, with one file gone: that one is removed.
    touch(&t.0.join("Clockwork Tanuki - 02.mkv"));
    let s = scan(&mut st, &folder).unwrap();
    assert_eq!((s.online, s.files, s.removed), (true, 1, 1));
    forget_folder(&st, &folder).unwrap();
    assert!(load_all(&st).unwrap().is_empty());
}

#[test]
fn rescans_parse_only_changed_files() {
    let mut st = Store::open_memory().unwrap();
    let f = "/media/anime";
    let a = PathBuf::from("/media/anime/Clockwork Tanuki - 01.mkv");
    apply(&mut st, f, Some(vec![(a.clone(), 10, 100)])).unwrap();
    assert_eq!(load_all(&st).unwrap()[0].episode, Some(1));
    // A changed size: parsed again, still one row.
    let s = apply(&mut st, f, Some(vec![(a, 20, 100)])).unwrap();
    assert_eq!((s.added, s.files), (0, 1));
    assert_eq!(load_all(&st).unwrap()[0].size, 20);
    assert_eq!(keep_only(&st, &[]).unwrap(), 1);
}

fn rows(list: &[(&str, &str)]) -> Vec<FileRow> {
    list.iter()
        .map(|(folder, path)| {
            let p = recognize::parse(path).unwrap();
            FileRow { path: path.to_string(), folder: folder.to_string(), size: 1, mtime: 0, title: p.title, episode: p.episode, season: p.season, group: p.group }
        })
        .collect()
}

fn none(_: &str) -> Cached {
    None
}

#[test]
fn files_match_the_list_by_title_and_season() {
    let items = vec![
        item(1, "Lanterns of Kirigaoka", Status::Watching, 2, 0),
        item(2, "Lanterns of Kirigaoka 2nd Season", Status::PlanToWatch, 0, 0),
        item(3, "Clockwork Tanuki", Status::Watching, 0, 0),
    ];
    let r = rows(&[
        ("/a", "/a/Lanterns/[Hoshi] Lanterns of Kirigaoka - 01 [1080p].mkv"),
        ("/a", "/a/Lanterns/[Hoshi] Lanterns of Kirigaoka - 03 [1080p].mkv"),
        ("/a", "/a/Lanterns S2/[Hoshi] Lanterns of Kirigaoka S2 - 01 [1080p].mkv"),
        ("/a", "/a/Clockwork Tanuki/06.mkv"),
        ("/a", "/a/Other/Paper Crane Detective - 01.mkv"),
    ]);
    let idx = Index::build(&r, &items, &none, &Relations::default());
    assert_eq!(idx.episodes(1), [1, 3]);
    assert_eq!(idx.episodes(2), [1]);
    assert_eq!(idx.episodes(3), [6], "the folder names the show");
    assert_eq!(idx.unmatched, 1, "not on the list: skipped, no search");
}

#[test]
fn the_users_pick_wins() {
    let items = vec![item(1, "Lanterns of Kirigaoka", Status::Watching, 0, 0), item(7, "Kirigaoka no Chouchin", Status::Watching, 0, 0)];
    let r = rows(&[("/a", "/a/Kirigaoka Lanterns - 01.mkv")]);
    let picked = |key: &str| (key == r[0].parsed().key()).then(|| (7, "user".to_string(), String::new()));
    let idx = Index::build(&r, &items, &picked, &Relations::default());
    assert_eq!(idx.episodes(7), [1]);
    // "Not an anime" keeps it out.
    let ignored = |_: &str| Some((0, "ignore".to_string(), String::new()));
    assert!(Index::build(&r, &items, &ignored, &Relations::default()).by_media.is_empty());
}

#[test]
fn relations_move_episodes_numbered_on() {
    let rel = Relations::parse("::meta\n- version: 1.3.0\n\n::rules\n- 31240|30654|31240:14-25 -> 42203|38734|42203:1-12\n").unwrap();
    let mut a = item(31240, "Lanterns of Kirigaoka 2nd Season", Status::Completed, 13, 0);
    a.media.episodes = Some(13);
    let b = item(42203, "Lanterns of Kirigaoka 2nd Season Part 2", Status::Watching, 1, 0);
    let r = rows(&[("/a", "/a/Lanterns of Kirigaoka 2nd Season - 15.mkv"), ("/a", "/a/Lanterns of Kirigaoka 2nd Season - 05.mkv")]);
    let idx = Index::build(&r, &[a, b], &none, &rel);
    assert_eq!(idx.episodes(42203), [2]);
    assert_eq!(idx.episodes(31240), [5]);
}

#[test]
fn a_movie_without_a_number_is_episode_one() {
    let mut m = item(5, "Starfall Delivery Service", Status::Watching, 0, 0);
    m.media.episodes = Some(1);
    let r = rows(&[("/a", "/a/Starfall Delivery Service (2026) [1080p].mkv")]);
    assert_eq!(Index::build(&r, &[m], &none, &Relations::default()).episodes(5), [1]);
}

fn local(path: &str, folder: &str, group: Option<&str>, mtime: i64) -> Local {
    Local { path: path.into(), folder: folder.into(), group: group.map(str::to_string), mtime }
}

#[test]
fn next_file_choice() {
    let mut idx = Index::default();
    let eps = idx.by_media.entry(1).or_default();
    eps.insert(2, vec![local("/a/02 Hoshi.mkv", "/a", Some("Hoshi"), 5)]);
    eps.insert(3, vec![local("/a/03 Tsuki.mkv", "/a", Some("Tsuki"), 9), local("/a/03 Hoshi.mkv", "/a", Some("Hoshi"), 1)]);
    eps.insert(4, vec![local("/d/04.mkv", "/Volumes/Disk", None, 1)]);
    eps.insert(5, vec![local("/d/05.mkv", "/Volumes/Disk", None, 1), local("/a/05.mkv", "/a", None, 1)]);
    let online = |f: &str| f == "/a";
    // The same group as the episode before wins over a newer file.
    assert_eq!(idx.next(1, 2, Some(12), &online), Next::Ready { episode: 3, path: "/a/03 Hoshi.mkv".into() });
    // No episode before: the newest.
    assert_eq!(idx.next(1, 2, Some(12), &online).episode(), Some(3));
    let mut no_before = idx.clone();
    no_before.by_media.get_mut(&1).unwrap().remove(&2);
    assert_eq!(no_before.next(1, 2, None, &online), Next::Ready { episode: 3, path: "/a/03 Tsuki.mkv".into() });
    // Only on an offline drive.
    let off = idx.next(1, 3, Some(12), &online);
    assert_eq!(off, Next::Offline { episode: 4, folder: "/Volumes/Disk".into() });
    assert_eq!(off.reason().unwrap(), "Episode 4 is on Disk, which is offline");
    // An online copy beats an offline one.
    assert_eq!(idx.next(1, 4, Some(12), &online), Next::Ready { episode: 5, path: "/a/05.mkv".into() });
    let missing = idx.next(1, 6, Some(12), &online);
    assert_eq!(missing.reason().unwrap(), "Episode 7 not in your folders");
    assert_eq!(idx.next(1, 12, Some(12), &online), Next::Done);
    assert_eq!(idx.next(9, 0, None, &online), Next::Missing { episode: 1 });
}

#[test]
fn short_ranges_and_overlaps() {
    assert_eq!(ranges(&[1, 2, 3, 4, 5, 6, 8, 10, 11, 12]), "1\u{2013}6, 8, 10\u{2013}12");
    assert_eq!(ranges(&[1, 2]), "1, 2");
    assert_eq!(ranges(&[]), "");
    assert!(overlaps("/m/anime", "/m/anime/Show"));
    assert!(overlaps("/m/anime/Show", "/m/anime"));
    assert!(!overlaps("/m/anime", "/m/anime2"));
    assert_eq!(place_name("/Volumes/Archive/Anime"), "Archive");
    assert_eq!(place_name("/run/media/someone/Archive/Anime"), "Archive");
    assert_eq!(place_name("/media/someone/Archive"), "Archive");
    assert_eq!(place_name("/srv/anime"), "anime");
}
