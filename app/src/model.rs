//! App state. Stage 1 has fake data only: no store, no network, no detection.
//!
//! Every title here is made up (the same ones as `mockup/kotora.html`).

use kotora_core::Status;
use rune::{AppHandle, Image, ImageCache};

pub type Handle = AppHandle<State>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    List(Status),
    NowPlaying,
    Queue,
    Settings,
}

/// The chips above the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Airing,
    Finished,
    NewEpisode,
}

impl Filter {
    pub const ALL: [Filter; 4] = [Filter::All, Filter::Airing, Filter::Finished, Filter::NewEpisode];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Airing => "Airing",
            Filter::Finished => "Finished airing",
            Filter::NewEpisode => "New episode",
        }
    }

    pub fn keeps(self, e: &Entry) -> bool {
        match self {
            Filter::All => true,
            Filter::Airing => e.airing.is_some(),
            Filter::Finished => e.airing.is_none(),
            Filter::NewEpisode => e.new_episode.is_some(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    /// The service's media id (fake here). Picks the cover tile's colours.
    pub id: u32,
    pub title: String,
    pub status: Status,
    pub progress: u32,
    /// `None`: unknown (still airing with no count).
    pub episodes: Option<u32>,
    /// 1-10; `None`: not scored.
    pub score: Option<u8>,
    pub kind: &'static str,
    /// "now", "2 h", … (a timestamp once the store exists).
    pub updated: &'static str,
    /// "Ep 7 on Sat" while airing.
    pub airing: Option<&'static str>,
    /// A released episode past the progress.
    pub new_episode: Option<u32>,
    pub rewatching: bool,
    /// A change for this entry waits in the queue.
    pub waiting: Option<&'static str>,
}

impl Entry {
    pub fn fraction(&self) -> f32 {
        match self.episodes {
            Some(n) if n > 0 => (self.progress as f32 / n as f32).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

/// The Now Playing card at the bottom of the sidebar.
#[derive(Clone, Debug)]
pub struct NowMini {
    pub id: u32,
    pub title: String,
    pub line: String,
}

pub struct State {
    pub page: Page,
    pub filter: Filter,
    /// The signed-in user name and service; `None` when signed out.
    pub account: Option<(String, &'static str)>,
    pub entries: Vec<Entry>,
    /// The first sync is running and nothing is saved yet: skeleton rows.
    pub loading: bool,
    pub queue: u32,
    pub offline: bool,
    pub synced: &'static str,
    pub now: Option<NowMini>,
    /// The window is open (false after a `--hidden` start until `kot show`).
    pub window: bool,
    /// The C2 icon at sidebar size, set in `setup`.
    pub logo: Option<Image>,
    pub images: Option<ImageCache>,
    pub handle: Option<Handle>,
}

impl State {
    /// The first state: the fake list (stage 1 has no store yet).
    pub fn fake() -> Self {
        State {
            page: Page::List(Status::Watching),
            filter: Filter::All,
            account: Some(("demo_user".into(), "MyAnimeList")),
            entries: fake_entries(),
            loading: false,
            queue: 4,
            offline: false,
            synced: "Synced 2 min ago",
            now: Some(NowMini { id: 0, title: "Lanterns of Kirigaoka".into(), line: "Ep 6 · mpv · 82%".into() }),
            window: true,
            logo: None,
            images: None,
            handle: None,
        }
    }

    pub fn count(&self, s: Status) -> usize {
        self.entries.iter().filter(|e| e.status == s).count()
    }

    /// The rows the list page shows: the page's status, then the chip.
    pub fn visible(&self) -> Vec<usize> {
        let Page::List(st) = self.page else { return Vec::new() };
        (0..self.entries.len()).filter(|&i| self.entries[i].status == st && self.filter.keeps(&self.entries[i])).collect()
    }

    pub fn go(&mut self, page: Page) {
        if self.page != page {
            self.page = page;
            self.filter = Filter::All;
        }
    }

    /// `+1`: optimistic, at once (the queue comes in stage 3).
    pub fn plus_one(&mut self, id: u32) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id)
            && e.episodes.is_none_or(|n| e.progress < n)
        {
            e.progress += 1;
            e.updated = "now";
            if e.new_episode.is_some_and(|n| e.progress >= n) {
                e.new_episode = None;
            }
        }
    }

    pub fn ctl_status(&self) -> kotora_ctl::Status {
        kotora_ctl::Status {
            version: kotora_ctl::version_line(),
            account: self.account.as_ref().map(|(n, _)| n.clone()),
            service: self.account.as_ref().map(|(_, s)| s.to_string()).unwrap_or_default(),
            entries: self.entries.len() as u32,
            queue: self.queue,
            offline: self.offline,
            window: self.window,
        }
    }
}

struct Fake {
    title: &'static str,
    progress: u32,
    episodes: u32,
    score: Option<u8>,
    kind: &'static str,
    updated: &'static str,
    airing: Option<&'static str>,
    new_episode: Option<u32>,
    rewatching: bool,
    waiting: Option<&'static str>,
}

const fn fk(title: &'static str, progress: u32, episodes: u32, score: Option<u8>, updated: &'static str, airing: Option<&'static str>) -> Fake {
    Fake { title, progress, episodes, score, kind: "TV", updated, airing, new_episode: None, rewatching: false, waiting: None }
}

/// The Watching page of the mockup, in "Last updated" order, plus two more off screen.
const WATCHING: [Fake; 12] = [
    Fake { new_episode: Some(6), ..fk("Lanterns of Kirigaoka", 5, 12, Some(8), "now", Some("Ep 7 on Sat")) },
    fk("Starfall Delivery Service", 8, 13, Some(9), "2 h", Some("Ep 9 on Wed")),
    Fake { waiting: Some("Conflict in queue"), ..fk("Mikan Street Detectives", 7, 24, None, "yesterday", None) },
    fk("Slow Life on Hoshizora Island", 3, 12, Some(7), "2 days", Some("Ep 4 on Sun")),
    fk("Neon Ronin 2099", 22, 26, Some(8), "3 days", None),
    Fake { rewatching: true, ..fk("The Ninth Shrine Maiden", 6, 12, Some(10), "5 days", None) },
    fk("Mecha Bento Club", 4, 10, None, "1 week", Some("Ep 11 on Fri")),
    fk("Ghost of the Library Stacks", 11, 12, Some(7), "2 weeks", None),
    fk("Paper Crane Racing: Second Lap", 1, 12, None, "1 month", None),
    Fake { kind: "Movie", ..fk("Kumo to Kaze no Ryokan", 0, 1, None, "2 months", None) },
    Fake { new_episode: Some(9), ..fk("Tidepool Radio", 8, 12, Some(6), "3 months", Some("Ep 10 on Tue")) },
    Fake { new_episode: Some(3), ..fk("Clockwork Bakery Wars", 2, 12, None, "4 months", None) },
];

const WORDS_A: [&str; 16] = [
    "Moonlit", "Paper", "Crimson", "Quiet", "Silver", "Hidden", "Northern", "Electric", "Gentle", "Lost", "Velvet", "Second",
    "Endless", "Hollow", "Bright", "Winter",
];
const WORDS_B: [&str; 16] = [
    "Harbor", "Lantern", "Garden", "Signal", "Orchard", "Station", "Archive", "Comet", "Festival", "Lighthouse", "Workshop",
    "Valley", "Market", "Bridge", "Theatre", "Academy",
];
const WORDS_C: [&str; 8] = ["", " Season 2", " Rebirth", ": Side Stories", " Final Act", " Zero", "", " Movie"];

/// Made-up entries for the other statuses: the counts of the mockup's sidebar (48, 236, 7, 0).
fn fake_entries() -> Vec<Entry> {
    let mut out: Vec<Entry> = WATCHING
        .iter()
        .enumerate()
        .map(|(i, f)| Entry {
            id: i as u32,
            title: f.title.into(),
            status: Status::Watching,
            progress: f.progress,
            episodes: Some(f.episodes),
            score: f.score,
            kind: f.kind,
            updated: f.updated,
            airing: f.airing,
            new_episode: f.new_episode,
            rewatching: f.rewatching,
            waiting: f.waiting,
        })
        .collect();
    const UPDATED: [&str; 6] = ["3 days", "1 week", "2 weeks", "1 month", "6 months", "1 year"];
    let mut id = out.len() as u32;
    for (status, n) in [(Status::PlanToWatch, 48), (Status::Completed, 236), (Status::OnHold, 7)] {
        for k in 0..n {
            let h = id.wrapping_mul(2_654_435_761);
            let c = WORDS_C[(h >> 8) as usize % WORDS_C.len()];
            let title = format!("{} {}{}", WORDS_A[(h >> 16) as usize % 16], WORDS_B[(h >> 24) as usize % 16], c);
            let movie = c == " Movie";
            let episodes = if movie { 1 } else { [12, 13, 24, 25, 10, 26][h as usize % 6] };
            let progress = match status {
                Status::PlanToWatch => 0,
                Status::Completed => episodes,
                _ => (h >> 4) % episodes,
            };
            out.push(Entry {
                id,
                title,
                status,
                progress,
                episodes: Some(episodes),
                score: (status == Status::Completed && k % 5 != 4).then_some(5 + (h % 6) as u8),
                kind: if movie { "Movie" } else { "TV" },
                updated: UPDATED[(k as usize * 6 / n as usize).min(5)],
                airing: None,
                new_episode: None,
                rewatching: false,
                waiting: None,
            });
            id += 1;
        }
    }
    out
}
