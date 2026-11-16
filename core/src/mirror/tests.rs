//! The mirror with two made-up services (no network): a MyAnimeList and an AniList whose ids
//! are the MyAnimeList id + 100000, and which knows the MyAnimeList ids in its catalogue.

use std::collections::{HashMap, HashSet};

use super::*;
use crate::score::ten;
use crate::model::Remote;
use crate::queue::{self, OpState, Outcome, Step};

const AL: u32 = 100_000;

/// A made-up list service: its own copy of the list, a catalogue for MyAnimeList-id lookups, a
/// log of every call, and an optional error for every call.
struct Fake {
    id: ServiceId,
    list: HashMap<u32, (Media, Remote)>,
    /// AniList: the anime it knows, by MyAnimeList id.
    catalogue: HashMap<u32, Media>,
    calls: Vec<String>,
    clock: i64,
    down: Option<ServiceError>,
}

impl Fake {
    fn new(id: ServiceId) -> Fake {
        Fake { id, list: HashMap::new(), catalogue: HashMap::new(), calls: Vec::new(), clock: 1_000, down: None }
    }

    /// The service's media for MyAnimeList id `mal`.
    fn media(&self, mal: u32) -> Media {
        match self.id {
            ServiceId::Mal => Media { id: mal, title: format!("Show {mal}"), episodes: Some(12), format: "tv".into(), ..Media::default() },
            ServiceId::AniList => Media { id: mal + AL, title: format!("Show {mal}"), episodes: Some(12), format: "TV".into(), id_mal: Some(mal), ..Media::default() },
        }
    }

    /// Puts an entry on the list (as made on the website): returns the service's id.
    fn put(&mut self, mal: u32, v: Values) -> u32 {
        let m = self.media(mal);
        self.clock += 1;
        let id = m.id;
        self.list.insert(id, (m, Remote { values: v, updated_at: self.clock }));
        id
    }

    fn values(&self, mal: u32) -> Option<Values> {
        self.list.get(&self.media(mal).id).map(|(_, r)| r.values)
    }

    fn items(&self) -> Vec<ListItem> {
        let mut v: Vec<ListItem> = self
            .list
            .values()
            .map(|(m, r)| ListItem {
                media: m.clone(),
                status: r.values.status,
                progress: r.values.progress,
                score: r.values.score,
                rewatching: r.values.rewatching,
                times_rewatched: r.values.times_rewatched,
                volumes: 0,
                updated_at: r.updated_at,
            })
            .collect();
        v.sort_by_key(|it| it.media.id);
        v
    }

    fn check(&mut self, call: String) -> std::result::Result<(), ServiceError> {
        self.calls.push(call);
        match &self.down {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
}

impl ListService for Fake {
    fn name(&self) -> &'static str {
        self.id.name()
    }
    fn title(&self) -> &'static str {
        self.id.title()
    }
    fn user_name(&mut self) -> std::result::Result<String, ServiceError> {
        Ok("demo_user".into())
    }
    fn fetch_list(&mut self) -> std::result::Result<Vec<ListItem>, ServiceError> {
        self.check("list".into())?;
        Ok(self.items())
    }
    fn get_entry(&mut self, id: u32) -> std::result::Result<Option<Remote>, ServiceError> {
        self.check(format!("get {id}"))?;
        Ok(self.list.get(&id).map(|(_, r)| *r))
    }
    fn update_entry(&mut self, id: u32, change: &Change) -> std::result::Result<Remote, ServiceError> {
        self.check(format!("save {id}"))?;
        self.clock += 1;
        let mal = if self.id == ServiceId::AniList { id - AL } else { id };
        let m = self.media(mal);
        let old = self.list.get(&id).map_or(Values::new(Status::PlanToWatch, 0, None), |(_, r)| r.values);
        let r = Remote { values: change.apply(old), updated_at: self.clock };
        self.list.insert(id, (m, r));
        Ok(r)
    }
    fn delete_entry(&mut self, id: u32) -> std::result::Result<(), ServiceError> {
        self.check(format!("delete {id}"))?;
        self.list.remove(&id);
        Ok(())
    }
    fn media_by_mal_ids(&mut self, ids: &[u32]) -> std::result::Result<Vec<Media>, ServiceError> {
        self.check(format!("lookup {}", ids.len()))?;
        Ok(ids.iter().filter_map(|id| self.catalogue.get(id).cloned()).collect())
    }
}

fn v(status: Status, progress: u32, score: Option<Score>) -> Values {
    Values::new(status, progress, score)
}

fn anilist_knowing(mals: &[u32]) -> Fake {
    let mut f = Fake::new(ServiceId::AniList);
    for &m in mals {
        let media = f.media(m);
        f.catalogue.insert(m, media);
    }
    f
}

/// Both lists synced into the store, as Kotora holds them.
fn synced(store: &mut Store, main: &mut Fake, mirror: &mut Fake, now: i64) {
    store.replace_list(main.name(), &main.fetch_list().unwrap(), now).unwrap();
    store.replace_list(mirror.name(), &mirror.fetch_list().unwrap(), now).unwrap();
}

/// Sends everything waiting in `svc`'s queue.
fn drain(store: &mut Store, svc: &mut Fake, now: i64) -> Vec<Outcome> {
    let mut out = Vec::new();
    for _ in 0..50 {
        match queue::step(store, svc, now).unwrap() {
            Step::Handled { outcome, .. } => out.push(outcome),
            _ => break,
        }
    }
    out
}

fn preview(store: &Store, link: Link, main: &mut Fake, mirror: &mut Fake) -> Preview {
    let main_list = store.load_list(link.main.name()).unwrap();
    let mirror_list = store.load_list(link.to.name()).unwrap();
    let mut mirror_media = HashMap::new();
    if link.to == ServiceId::AniList {
        let ids = ids_only_on_main(link, &main_list, &mirror_list);
        mirror_media = mirror.media_by_mal_ids(&ids).unwrap().into_iter().filter_map(|m| Some((m.id_mal?, m))).collect();
    }
    let mut main_media = HashMap::new();
    if link.main == ServiceId::AniList {
        let ids = ids_only_on_mirror(link, &main_list, &mirror_list);
        main_media = main.media_by_mal_ids(&ids).unwrap().into_iter().filter_map(|m| Some((m.id_mal?, m))).collect();
    }
    compare(link, &main_list, &mirror_list, &mirror_media, &main_media)
}

/// MyAnimeList main, AniList mirror:
/// 1 only on MAL, 2 different, 3 the same, 4 only on AniList, 9 on MAL but unknown to AniList,
/// 5 on both with a status the merge can't settle (Dropped against Completed).
fn two_lists() -> (Store, Fake, Fake) {
    let mut store = Store::open_memory().unwrap();
    let mut mal = Fake::new(ServiceId::Mal);
    let mut al = anilist_knowing(&[1, 2, 3, 4, 5]);
    mal.put(1, v(Status::Watching, 3, ten(7)));
    mal.put(2, v(Status::Watching, 5, ten(8)));
    mal.put(3, v(Status::Completed, 12, ten(9)));
    mal.put(5, v(Status::Dropped, 4, None));
    mal.put(9, v(Status::PlanToWatch, 0, None));
    al.put(2, v(Status::Watching, 7, None));
    al.put(3, v(Status::Completed, 12, ten(9)));
    al.put(4, v(Status::OnHold, 2, ten(6)));
    al.put(5, v(Status::Completed, 12, ten(7)));
    synced(&mut store, &mut mal, &mut al, 10);
    (store, mal, al)
}

#[test]
fn the_preview_sorts_both_lists_by_mal_id() {
    let (store, mut mal, mut al) = two_lists();
    let link = Link::of(ServiceId::Mal);
    assert_eq!(link.to, ServiceId::AniList);
    let p = preview(&store, link, &mut mal, &mut al);
    let c = p.counts();
    assert_eq!((c.only_main, c.only_mirror, c.different, c.same, c.unmatched, c.conflicts), (1, 1, 2, 1, 1, 1));
    assert!(p.plan.rows.iter().any(|r| r.entry.mal_id == 9 && r.verdict == Verdict::Unknown("Not on AniList".into())));
    assert_eq!(p.only_mirror[0].mal_id, 4);
    assert_eq!(p.only_mirror[0].main_media.as_ref().map(|m| m.id), Some(4), "MyAnimeList: the id is the MAL id");
    // The lookup asked only for the ids not on AniList's list.
    assert_eq!(al.calls.last().map(String::as_str), Some("lookup 2"));
    // The merge keeps the further progress and fills the empty score; status pairs are asked.
    let r2 = p.merge.iter().find(|r| r.mal_id == 2).unwrap();
    assert_eq!(r2.merged(false), v(Status::Watching, 7, ten(8)));
    let r5 = p.merge.iter().find(|r| r.mal_id == 5).unwrap();
    assert_eq!(r5.conflict, Some(StatusConflict { mine: Status::Dropped, file: Status::Completed }));
}

#[test]
fn main_wins_makes_the_mirror_a_copy_and_keeps_what_only_the_mirror_has() {
    let (mut store, mut mal, mut al) = two_lists();
    let link = Link::of(ServiceId::Mal);
    let p = preview(&store, link, &mut mal, &mut al);
    let (to, main) = align_actions(&p, Align::MainWins, &HashSet::new());
    assert!(main.is_empty(), "the main list is not touched");
    let (a, b) = store.start_mirror(link, &to, &main, 20).unwrap();
    assert_eq!((a.len(), b.len()), (3, 0), "add 1, update 2 and 5");
    assert_eq!(store.mirror_link().unwrap(), Some(link));
    assert!(mal.calls.iter().all(|c| c == "list"), "nothing sent to the main service");
    drain(&mut store, &mut al, 30);
    for m in [1, 2, 3, 5] {
        assert_eq!(al.values(m), mal.values(m), "anime {m}");
    }
    assert_eq!(al.values(4), Some(v(Status::OnHold, 2, ten(6))), "only on AniList: kept");
    assert_eq!(al.values(9), None);
}

#[test]
fn merge_fills_both_sides_and_asks_about_unclear_statuses() {
    let (mut store, mut mal, mut al) = two_lists();
    let link = Link::of(ServiceId::Mal);
    let p = preview(&store, link, &mut mal, &mut al);
    // The user takes AniList's Completed for anime 5.
    let (to, main) = align_actions(&p, Align::Merge, &HashSet::from([5]));
    store.start_mirror(link, &to, &main, 20).unwrap();
    drain(&mut store, &mut al, 30);
    drain(&mut store, &mut mal, 30);
    assert_eq!(mal.values(2), Some(v(Status::Watching, 7, ten(8))));
    assert_eq!(al.values(2), Some(v(Status::Watching, 7, ten(8))));
    assert_eq!(mal.values(5), Some(v(Status::Completed, 12, ten(7))), "AniList's status, as picked");
    assert_eq!(al.values(5), mal.values(5));
    assert_eq!(mal.values(4), Some(v(Status::OnHold, 2, ten(6))), "only on AniList: added to MAL");
    assert_eq!(al.values(1), mal.values(1), "only on MAL: added to AniList");
    // Nothing more waits: the main list's merge changes needed nothing more on the mirror.
    assert_eq!(store.open_count("anilist").unwrap(), 0);
    assert_eq!(store.open_count("mal").unwrap(), 0);
}

#[test]
fn dont_align_mirrors_only_new_edits_and_only_what_they_change() {
    let (mut store, _mal, mut al) = two_lists();
    let link = Link::of(ServiceId::Mal);
    let (a, b) = store.start_mirror(link, &[], &[], 20).unwrap();
    assert!(a.is_empty() && b.is_empty());
    assert_eq!(store.open_count("anilist").unwrap(), 0, "nothing queued");
    // Anime 2: MAL has 5 / score 8, AniList 7 / no score. An edit of the progress only sends
    // the progress: AniList's missing score stays missing.
    store.enqueue("mal", 2, Change { progress: Some(6), ..Change::default() }, 40).unwrap();
    let ops = store.queue("anilist", 40).unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].change, Change { progress: Some(6), ..Change::default() });
    drain(&mut store, &mut al, 50);
    assert_eq!(al.values(2), Some(v(Status::Watching, 6, None)));
    // Anime 1 is not on AniList: its first edit adds it there with all its values, once
    // AniList's id for it is looked up.
    store.enqueue("mal", 1, Change { progress: Some(4), ..Change::default() }, 60).unwrap();
    assert_eq!(store.mirror_waits().unwrap()[0].mal_id, 1);
    assert_eq!(map_waits(&mut al, &mut store, 65).unwrap(), 1);
    drain(&mut store, &mut al, 70);
    assert_eq!(al.values(1), Some(v(Status::Watching, 4, ten(7))));
}

#[test]
fn a_failure_on_one_side_never_blocks_the_other() {
    let (mut store, mut mal, mut al) = two_lists();
    store.start_mirror(Link::of(ServiceId::Mal), &[], &[], 20).unwrap();
    store.enqueue("mal", 3, Change { score: Some(ten(10)), ..Change::default() }, 40).unwrap();
    al.down = Some(ServiceError::Offline("no network".into()));
    assert!(matches!(drain(&mut store, &mut al, 50)[..], [Outcome::Retry(_)]));
    assert_eq!(drain(&mut store, &mut mal, 50), [Outcome::Sent], "MAL goes on alone");
    assert_eq!(mal.values(3).unwrap().score, ten(10));
    // AniList back: its change goes out at its next try.
    al.down = None;
    assert_eq!(drain(&mut store, &mut al, 50 + queue::MAX_BACKOFF), [Outcome::Sent]);
    assert_eq!(al.values(3).unwrap().score, ten(10));
    // The other way round: AniList refusing doesn't stop MAL either.
    store.enqueue("mal", 3, Change { score: Some(ten(9)), ..Change::default() }, 2_000).unwrap();
    al.down = Some(ServiceError::Other("Validation error".into()));
    assert!(matches!(drain(&mut store, &mut al, 2_010)[..], [Outcome::Failed(_)]));
    assert_eq!(drain(&mut store, &mut mal, 2_010), [Outcome::Sent]);
}

#[test]
fn a_website_edit_on_the_mirror_is_a_conflict_never_overwritten() {
    let (mut store, mut mal, mut al) = two_lists();
    store.start_mirror(Link::of(ServiceId::Mal), &[], &[], 20).unwrap();
    // Edited on anilist.co, then in Kotora before the mirror's next sync.
    let before = al.values(3).unwrap();
    al.put(3, Values { score: ten(5), ..before });
    store.enqueue("mal", 3, Change { progress: Some(11), ..Change::default() }, 40).unwrap();
    assert_eq!(drain(&mut store, &mut al, 50), [Outcome::Conflict]);
    assert_eq!(al.values(3).unwrap(), Values { score: ten(5), ..before }, "untouched");
    assert_eq!(drain(&mut store, &mut mal, 50), [Outcome::Sent], "the main list still gets it");
    let op = store.queue("anilist", 50).unwrap().into_iter().find(|o| o.state == OpState::Conflict).unwrap();
    assert_eq!(op.remote.unwrap().values.score, ten(5));
}

#[test]
fn the_mirrors_sync_lists_website_edits_and_the_user_settles_them() {
    let (mut store, mut mal, mut al) = two_lists();
    let link = Link::of(ServiceId::Mal);
    let p = preview(&store, link, &mut mal, &mut al);
    let (to, main) = align_actions(&p, Align::MainWins, &HashSet::new());
    store.start_mirror(link, &to, &main, 20).unwrap();
    drain(&mut store, &mut al, 30);
    // The first mirror sync after the alignment finds nothing.
    assert_eq!(sync_mirror(&mut al, &mut store, 40).unwrap().web_edits, 0);
    // On anilist.co: 2 changed, 7 added, 1 removed.
    al.put(2, v(Status::Watching, 9, ten(8)));
    al.put(7, v(Status::PlanToWatch, 0, None));
    al.list.remove(&(1 + AL));
    let r = sync_mirror(&mut al, &mut store, 50).unwrap();
    assert_eq!(r.web_edits, 3);
    let edits = store.web_edits().unwrap();
    let e2 = edits.iter().find(|e| e.mal_id == 2).unwrap();
    assert_eq!((e2.main.as_ref().unwrap().progress, e2.mirror.as_ref().unwrap().progress), (5, 9));
    assert!(edits.iter().any(|e| e.mal_id == 7 && e.main.is_none()), "added on AniList");
    assert!(edits.iter().any(|e| e.mal_id == 1 && e.mirror.is_none()), "removed on AniList");
    assert_eq!(store.open_count("anilist").unwrap() + store.open_count("mal").unwrap(), 0, "nothing queued by itself");
    // Settle: 2 takes AniList's progress on MAL, 7 is added to MAL, 1 goes back on AniList.
    store.settle_web_edit(2, Settle::UseMirror, None, 60).unwrap();
    store.settle_web_edit(7, Settle::UseMirror, None, 60).unwrap();
    store.settle_web_edit(1, Settle::UseMain, None, 60).unwrap();
    assert!(store.web_edits().unwrap().is_empty());
    // AniList's saved list no longer has 1: its id is looked up again.
    assert_eq!(map_waits(&mut al, &mut store, 65).unwrap(), 1);
    drain(&mut store, &mut mal, 70);
    drain(&mut store, &mut al, 70);
    assert_eq!(mal.values(2), Some(v(Status::Watching, 9, ten(8))));
    assert_eq!(mal.values(7), Some(v(Status::PlanToWatch, 0, None)));
    assert_eq!(al.values(1), mal.values(1));
    // Leave: asked again only when AniList changes it again.
    al.put(3, v(Status::Completed, 12, ten(4)));
    sync_mirror(&mut al, &mut store, 80).unwrap();
    store.settle_web_edit(3, Settle::Leave, None, 90).unwrap();
    assert_eq!(sync_mirror(&mut al, &mut store, 100).unwrap().web_edits, 0);
    assert_eq!(mal.values(3).unwrap().score, ten(9), "left as it was");
    // A difference that can't be settled that way says why.
    al.list.remove(&(3 + AL));
    sync_mirror(&mut al, &mut store, 110).unwrap();
    assert!(store.settle_web_edit(3, Settle::UseMirror, None, 120).is_err(), "Kotora can't remove from MAL");
}

#[test]
fn an_add_waits_for_its_anilist_id_then_goes() {
    let (mut store, mal, mut al) = two_lists();
    store.start_mirror(Link::of(ServiceId::Mal), &[], &[], 20).unwrap();
    // Search and add on MAL: AniList's id of 6 isn't known yet.
    al.catalogue.insert(6, al.media(6));
    store.add_to_list("mal", &mal.media(6), Status::Watching, Some(2), 30).unwrap();
    store.add_to_list("mal", &mal.media(8), Status::PlanToWatch, None, 30).unwrap();
    assert_eq!(store.mirror_waits().unwrap().iter().map(|w| w.mal_id).collect::<Vec<_>>(), [6, 8]);
    assert_eq!(store.open_count("anilist").unwrap(), 0);
    assert_eq!(map_waits(&mut al, &mut store, 40).unwrap(), 1);
    let waits = store.mirror_waits().unwrap();
    assert_eq!((waits.len(), waits[0].mal_id, waits[0].error.as_deref()), (1, 8, Some("Not on AniList")));
    // A failed lookup isn't asked again by itself.
    let n = al.calls.len();
    assert_eq!(map_waits(&mut al, &mut store, 41).unwrap(), 0);
    assert_eq!(al.calls.len(), n);
    drain(&mut store, &mut al, 50);
    assert_eq!(al.values(6), Some(v(Status::Watching, 2, None)));
    // A discarded add on MAL takes its waiting mirror add with it.
    store.add_to_list("mal", &mal.media(4), Status::Watching, Some(1), 60).unwrap();
    assert_eq!(store.open_count("anilist").unwrap(), 1, "4 is on AniList already: a change");
    let id = store.queue("mal", 60).unwrap().into_iter().find(|o| o.media_id == 4 && o.state.is_open()).unwrap().id;
    store.discard("mal", id, 61).unwrap();
    assert_eq!(store.open_count("anilist").unwrap(), 0, "back where AniList was");
}

#[test]
fn a_website_edit_on_the_main_list_is_mirrored_at_the_sync() {
    let (mut store, mut mal, mut al) = two_lists();
    store.start_mirror(Link::of(ServiceId::Mal), &[], &[], 20).unwrap();
    let old = store.load_list("mal").unwrap();
    mal.put(2, v(Status::Watching, 6, ten(8)));
    store.replace_list("mal", &mal.fetch_list().unwrap(), 30).unwrap();
    assert_eq!(after_main_sync(&mut store, "mal", &old, 30).unwrap(), 1);
    let ops = store.queue("anilist", 30).unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].change, Change { progress: Some(6), ..Change::default() }, "only what changed on MAL");
    // Nothing changed: nothing more.
    let old = store.load_list("mal").unwrap();
    store.replace_list("mal", &mal.fetch_list().unwrap(), 40).unwrap();
    assert_eq!(after_main_sync(&mut store, "mal", &old, 40).unwrap(), 0);
    assert_eq!(after_main_sync(&mut store, "anilist", &old, 40).unwrap(), 0, "not the main list");
    drain(&mut store, &mut al, 50);
    assert_eq!(al.values(2).unwrap().progress, 6);
    assert_eq!(after_main_sync(&mut store, "mal", &[], 40).unwrap(), 0, "no saved list before: never the whole list");
    // An edit made on MAL before the mirror was turned on is not mirrored.
    store.stop_mirror().unwrap();
    mal.put(3, v(Status::Completed, 12, ten(1)));
    let old = store.load_list("mal").unwrap();
    store.start_mirror(Link::of(ServiceId::Mal), &[], &[], mal.clock + 10).unwrap();
    store.replace_list("mal", &mal.fetch_list().unwrap(), 45).unwrap();
    assert_eq!(after_main_sync(&mut store, "mal", &old, 45).unwrap(), 0);
}

#[test]
fn anilist_main_entries_without_a_mal_id_are_not_mirrored() {
    let mut store = Store::open_memory().unwrap();
    let mut al = anilist_knowing(&[1, 2]);
    let mut mal = Fake::new(ServiceId::Mal);
    al.put(1, v(Status::Watching, 1, None));
    // An AniList-only anime: no idMal.
    al.list.insert(555, (Media { id: 555, title: "Web Short".into(), ..Media::default() }, Remote { values: v(Status::Watching, 2, None), updated_at: 5 }));
    mal.put(2, v(Status::Completed, 12, ten(8)));
    synced(&mut store, &mut al, &mut mal, 10);
    let link = Link::of(ServiceId::AniList);
    let p = preview(&store, link, &mut al, &mut mal);
    let c = p.counts();
    assert_eq!((c.only_main, c.only_mirror, c.unmatched), (1, 1, 1));
    assert!(p.plan.rows.iter().any(|r| r.verdict == Verdict::Unknown("No MyAnimeList id".into())));
    assert_eq!(p.only_mirror[0].main_media.as_ref().map(|m| m.id), Some(2 + AL), "looked up on AniList");
    let (to, main) = align_actions(&p, Align::Merge, &HashSet::new());
    store.start_mirror(link, &to, &main, 20).unwrap();
    drain(&mut store, &mut mal, 30);
    drain(&mut store, &mut al, 30);
    assert_eq!(mal.values(1), Some(v(Status::Watching, 1, None)));
    assert_eq!(al.values(2), Some(v(Status::Completed, 12, ten(8))));
    // Edits of the anime with no MyAnimeList id stay on AniList.
    store.enqueue("anilist", 555, Change { progress: Some(3), ..Change::default() }, 40).unwrap();
    assert_eq!(store.open_count("mal").unwrap(), 0);
    assert!(store.mirror_waits().unwrap().is_empty());
}

#[test]
fn off_drops_what_waits_and_signing_out_turns_it_off() {
    let (mut store, mut mal, mut al) = two_lists();
    let link = Link::of(ServiceId::Mal);
    let p = preview(&store, link, &mut mal, &mut al);
    let (to, _) = align_actions(&p, Align::MainWins, &HashSet::new());
    store.start_mirror(link, &to, &[], 20).unwrap();
    assert_eq!(store.stop_mirror().unwrap(), 3);
    assert_eq!(store.open_count("anilist").unwrap(), 0);
    assert_eq!(store.mirror_link().unwrap(), None);
    assert_eq!(store.load_item("anilist", 2 + AL).unwrap().unwrap().progress, 7, "AniList's saved entry back as it was");
    // Edits no longer go to AniList.
    store.enqueue("mal", 3, Change { score: Some(ten(1)), ..Change::default() }, 30).unwrap();
    assert_eq!(store.open_count("anilist").unwrap(), 0);
    // On again, then the mirror signs out: off.
    store.start_mirror(link, &[], &[], 40).unwrap();
    store.clear_service("anilist").unwrap();
    assert_eq!(store.mirror_link().unwrap(), None);
    // Another key with a service-like prefix leaves a link alone.
    store.start_mirror(link, &[], &[], 50).unwrap();
    store.clear_service("other").unwrap();
    assert_eq!(store.mirror_link().unwrap(), Some(link));
}

#[test]
fn manga_and_the_mirrors_own_edits_are_not_followed() {
    let (mut store, _mal, _al) = two_lists();
    store.start_mirror(Link::of(ServiceId::Mal), &[], &[], 20).unwrap();
    // An edit on the mirror's list stays there.
    store.enqueue("anilist", 2 + AL, Change { progress: Some(8), ..Change::default() }, 30).unwrap();
    assert_eq!(store.open_count("mal").unwrap(), 0);
    let manga = Media { kind: Kind::Manga, id: 2, title: "Paper Lanterns".into(), ..Media::default() };
    store.add_to_list("mal", &manga, Status::Watching, Some(3), 30).unwrap();
    assert_eq!(store.open_count("anilist").unwrap(), 1, "only the AniList edit itself");
}

#[test]
fn links_read_and_write() {
    assert_eq!(Link::parse(&Link::of(ServiceId::AniList).text()), Some(Link { main: ServiceId::AniList, to: ServiceId::Mal }));
    assert_eq!(Link::parse("mal>mal"), None);
    assert_eq!(Link::parse("nope"), None);
    let s = Saved::from(Values { rewatching: true, times_rewatched: 2, ..v(Status::Completed, 3, ten(9)) });
    assert_eq!(s.values(), Values { rewatching: true, times_rewatched: 2, ..v(Status::Completed, 3, ten(9)) });
}

// ------------------------------------------------------------------- scores across services

/// The agreed rule: MyAnimeList scores 1-10 and AniList keeps 100 points, so the two are
/// compared on the 1-10 scale. A MAL 7 and an AniList 73 are the same (7.3 rounds to 7); a 75
/// is an 8 and differs.
#[test]
fn a_mal_7_and_an_anilist_73_are_the_same_score() {
    let mut store = Store::open_memory().unwrap();
    let mut mal = Fake::new(ServiceId::Mal);
    let mut al = anilist_knowing(&[1, 2]);
    mal.put(1, v(Status::Watching, 3, ten(7)));
    mal.put(2, v(Status::Watching, 3, ten(7)));
    al.put(1, v(Status::Watching, 3, Score::from_points(73)));
    al.put(2, v(Status::Watching, 3, Score::from_points(75)));
    synced(&mut store, &mut mal, &mut al, 10);
    let link = Link::of(ServiceId::Mal);
    let p = preview(&store, link, &mut mal, &mut al);
    let c = p.counts();
    assert_eq!((c.same, c.different), (1, 1), "73 is a 7, 75 an 8");
    assert!(p.plan.rows.iter().any(|r| r.entry.mal_id == 1 && r.verdict == Verdict::Same));
    // Main wins: only 2's score goes, as MAL's 7 (70); 1's 73 is left alone.
    let (to, main) = align_actions(&p, Align::MainWins, &HashSet::new());
    assert!(main.is_empty());
    assert_eq!(to.len(), 1);
    store.start_mirror(link, &to, &main, 20).unwrap();
    drain(&mut store, &mut al, 30);
    assert_eq!(al.values(1).unwrap().score, Score::from_points(73), "kept: the same on MAL's scale");
    assert_eq!(al.values(2).unwrap().score, ten(7));
    // A later edit: the progress goes alone (AniList's 73 stays), a new score goes as MAL's.
    store.enqueue("mal", 1, Change { progress: Some(4), ..Change::default() }, 40).unwrap();
    drain(&mut store, &mut al, 45);
    assert_eq!(al.values(1), Some(v(Status::Watching, 4, Score::from_points(73))));
    store.enqueue("mal", 1, Change { score: Some(ten(8)), ..Change::default() }, 50).unwrap();
    drain(&mut store, &mut al, 60);
    assert_eq!(al.values(1), Some(v(Status::Watching, 4, ten(8))));
}

/// AniList main, MyAnimeList mirror: a 73 goes to MAL as a 7, and the merge never rounds the
/// AniList 73 down to 70.
#[test]
fn an_anilist_73_goes_to_mal_as_7_and_stays_73_on_anilist() {
    let mut store = Store::open_memory().unwrap();
    let mut al = anilist_knowing(&[1, 2]);
    let mut mal = Fake::new(ServiceId::Mal);
    al.put(1, v(Status::Watching, 5, Score::from_points(73)));
    al.put(2, v(Status::Watching, 2, Score::from_points(73)));
    mal.put(1, v(Status::Watching, 3, None));
    mal.put(2, v(Status::Watching, 2, ten(7)));
    synced(&mut store, &mut al, &mut mal, 10);
    let link = Link::of(ServiceId::AniList);
    let p = preview(&store, link, &mut al, &mut mal);
    assert_eq!((p.counts().same, p.counts().different), (1, 1));
    let (to, main) = align_actions(&p, Align::Merge, &HashSet::new());
    assert!(main.is_empty(), "AniList keeps its 73 and its progress: {main:?}");
    store.start_mirror(link, &to, &main, 20).unwrap();
    drain(&mut store, &mut mal, 30);
    assert_eq!(mal.values(1), Some(v(Status::Watching, 5, Score::from_points(73))));
    assert_eq!(al.values(1).unwrap().score, Score::from_points(73));
    // The fake keeps what it is sent; MAL itself answers 7 (70), as a sync then saves it.
    mal.put(1, v(Status::Watching, 5, ten(7)));
    store.replace_list("mal", &mal.fetch_list().unwrap(), 35).unwrap();
    // Fine-tuning on AniList (73 to 71) is still a 7 on MAL: nothing goes there.
    store.enqueue("anilist", 1 + AL, Change { score: Some(Score::from_points(71)), ..Change::default() }, 40).unwrap();
    assert_eq!(store.open_count("mal").unwrap(), 0);
    store.enqueue("anilist", 1 + AL, Change { score: Some(Score::from_points(76)), ..Change::default() }, 41).unwrap();
    drain(&mut store, &mut mal, 50);
    assert_eq!(mal.values(1).unwrap().score.map(Score::to_ten), Some(8));
}

// -------------------------------------------------------------------------------- removals

/// Remove from list on the main list goes to the mirror too, through its queue.
#[test]
fn a_removal_on_the_main_list_is_mirrored() {
    let (mut store, mut mal, mut al) = two_lists();
    store.start_mirror(Link::of(ServiceId::Mal), &[], &[], 20).unwrap();
    store.remove_from_list("mal", 3, 40).unwrap();
    let ops = store.queue("anilist", 40).unwrap();
    assert_eq!(ops.len(), 1);
    assert!(ops[0].removed && ops[0].media_id == 3 + AL);
    assert!(entry_values(&store.conn, Scope::new("anilist", Kind::Anime), 3 + AL).unwrap().map(|(v, _)| v).is_none(), "off the saved mirror list at once");
    assert_eq!(drain(&mut store, &mut mal, 50), [Outcome::Sent]);
    assert_eq!(drain(&mut store, &mut al, 50), [Outcome::Sent]);
    assert_eq!((mal.values(3), al.values(3)), (None, None));
    assert!(al.calls.contains(&format!("delete {}", 3 + AL)));
    // The mirror's next sync doesn't call it a website edit.
    assert_eq!(sync_mirror(&mut al, &mut store, 60).unwrap().web_edits, 0);
    assert!(store.web_edits().unwrap().is_empty());
}

/// Undo (Discard) of the main removal takes back the mirror's waiting one; adding the anime
/// again does too.
#[test]
fn undo_or_add_again_takes_back_the_mirrors_removal() {
    let (mut store, _mal, mut al) = two_lists();
    store.start_mirror(Link::of(ServiceId::Mal), &[], &[], 20).unwrap();
    let id = store.remove_from_list("mal", 3, 40).unwrap();
    store.discard("mal", id, 41).unwrap();
    assert_eq!(store.open_count("anilist").unwrap(), 0, "nothing left for AniList");
    assert_eq!(entry_values(&store.conn, Scope::new("anilist", Kind::Anime), 3 + AL).unwrap().map(|(v, _)| v), Some(v(Status::Completed, 12, ten(9))), "back on the mirror's saved list");
    // Remove again, then add it again from Search.
    store.remove_from_list("mal", 3, 50).unwrap();
    let media = Fake::new(ServiceId::Mal).media(3);
    store.add_to_list("mal", &media, Status::Completed, None, 51).unwrap();
    assert!(store.queue("anilist", 51).unwrap().iter().all(|o| !o.removed), "the mirror's removal is gone");
    drain(&mut store, &mut al, 60);
    assert_eq!(al.values(3), Some(v(Status::Completed, 12, ten(9))), "never removed on AniList");
    assert!(!al.calls.iter().any(|c| c.starts_with("delete")));
}

/// A mirrored removal whose entry changed on the mirror's website stops as a conflict:
/// "Remove anyway" (keep mine) removes it, "Keep it" (take theirs) puts it back.
#[test]
fn a_mirrored_removal_stops_when_the_mirror_changed() {
    let (mut store, _mal, mut al) = two_lists();
    store.start_mirror(Link::of(ServiceId::Mal), &[], &[], 20).unwrap();
    al.put(3, v(Status::Completed, 12, ten(10)));
    al.put(2, v(Status::Watching, 8, None));
    store.remove_from_list("mal", 3, 40).unwrap();
    store.remove_from_list("mal", 2, 40).unwrap();
    assert_eq!(drain(&mut store, &mut al, 50), [Outcome::Conflict, Outcome::Conflict]);
    assert!(al.values(3).is_some() && al.values(2).is_some(), "nothing removed on AniList");
    let ops = store.queue("anilist", 50).unwrap();
    let c3 = ops.iter().find(|o| o.media_id == 3 + AL).unwrap();
    let c2 = ops.iter().find(|o| o.media_id == 2 + AL).unwrap();
    assert!(c3.removed && c3.state == OpState::Conflict);
    // Remove anyway.
    store.keep_mine(c3.id).unwrap();
    // Keep it: AniList's entry comes back on the saved mirror list; the main list stays without it.
    store.take_theirs("anilist", c2.id, 55).unwrap();
    assert_eq!(drain(&mut store, &mut al, 60), [Outcome::Sent]);
    assert_eq!(al.values(3), None);
    assert_eq!(entry_values(&store.conn, Scope::new("anilist", Kind::Anime), 2 + AL).unwrap().map(|(v, _)| v), Some(v(Status::Watching, 8, None)));
    assert!(entry_values(&store.conn, Scope::new("mal", Kind::Anime), 2).unwrap().map(|(v, _)| v).is_none());
    assert_eq!(store.open_count("mal").unwrap(), 2, "the main removals still go");
}

/// Not on the mirror: nothing to remove there, and an add that waited for its AniList id is
/// dropped. The mirror's own removals and manga are not followed.
#[test]
fn a_removal_not_on_the_mirror_only_drops_what_waits() {
    let (mut store, _mal, mut al) = two_lists();
    store.start_mirror(Link::of(ServiceId::Mal), &[], &[], 20).unwrap();
    store.enqueue("mal", 1, Change { progress: Some(4), ..Change::default() }, 30).unwrap();
    assert_eq!(store.mirror_waits().unwrap().len(), 1);
    store.remove_from_list("mal", 1, 40).unwrap();
    assert!(store.mirror_waits().unwrap().is_empty());
    assert_eq!(store.open_count("anilist").unwrap(), 0);
    assert_eq!(map_waits(&mut al, &mut store, 50).unwrap(), 0);
    // The mirror's own removal (from its queue page, say) goes nowhere else.
    store.remove_from_list("anilist", 4 + AL, 60).unwrap();
    assert!(store.queue("mal", 60).unwrap().iter().all(|o| o.media_id == 1), "only the main list's own changes of 1");
}
