//! Release names (torrent and feed titles) for the feeds: what [`super::parse`] reads, plus the
//! version, codec, source, languages, batch and episode range. A release name is not a path, so
//! a "/" in it stays part of the title.

use anitomy_ng::{ElementKind, Options};

use super::{Parsed, last_number};

/// The video codec a release names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Avc,
    Hevc,
    Av1,
}

/// What a release name says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Release {
    /// Title, season, group, resolution; `episode` is the last of a range.
    pub parsed: Parsed,
    /// First and last episode: `(8, 8)` for one, `(1, 12)` for "01-12". `None`: no number (a
    /// season pack, a movie, or a batch without numbers).
    pub episodes: Option<(u32, u32)>,
    /// A range, "Batch", "Complete", or a season with no episode.
    pub batch: bool,
    /// "v2": 2. `None` is the first version.
    pub version: Option<u32>,
    pub codec: Option<Codec>,
    pub ten_bit: bool,
    /// "WEB-DL", "BD", … as written.
    pub source: Option<String>,
    pub audio: Vec<String>,
    pub languages: Vec<String>,
    pub subtitles: Vec<String>,
    /// "A1B2C3D4", upper case.
    pub checksum: Option<String>,
    /// "Movie" or "Film" after the title (cut from it).
    pub movie: bool,
    /// "END" or "Final" after the episode: the last one.
    pub last: bool,
}

impl Release {
    /// The release holds `ep` (a single episode, or inside a range).
    pub fn covers(&self, ep: u32) -> bool {
        self.episodes.is_some_and(|(a, b)| (a..=b).contains(&ep))
    }

    /// The resolution's height: "1080p" and "1920x1080" are 1080, "4K" 2160.
    pub fn height(&self) -> Option<u32> {
        let r = self.parsed.resolution.as_deref()?.to_ascii_lowercase();
        if r == "4k" {
            return Some(2160);
        }
        let h = r.rsplit('x').next()?.trim_end_matches(['p', 'i']);
        h.parse().ok()
    }
}

fn numbers(s: &str) -> Vec<u32> {
    s.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect()
}

fn codec_of(term: &str) -> Option<Codec> {
    let t = term.to_ascii_lowercase().replace(['.', '-', ' '], "");
    match t.as_str() {
        "h264" | "x264" | "avc" | "h264hi10p" | "hi10p" | "hi10" => Some(Codec::Avc),
        "h265" | "x265" | "hevc" | "hevc2" => Some(Codec::Hevc),
        "av1" => Some(Codec::Av1),
        _ => None,
    }
}

fn is_ten_bit(term: &str) -> bool {
    let t = term.to_ascii_lowercase().replace(['-', ' '], "");
    t == "10bit" || t == "10bits" || t.contains("hi10") || t == "yuv420p10"
}

/// Words anitomy leaves in the title that only say "the whole thing".
const BATCH_WORDS: [&str; 4] = ["batch", "complete", "complete series", "complete season"];

/// Reads a release name (a feed item's title).
pub fn parse_release(title: &str) -> Release {
    // Some feeds keep the extension (".mkv"); anitomy takes it either way. A trailing ".torrent"
    // is not the video's.
    let name = title.trim();
    let name = name.strip_suffix(".torrent").unwrap_or(name);
    let els = anitomy_ng::parse(name, Options::default());
    let all = |k: ElementKind| els.iter().filter(move |e| e.kind == k).map(|e| e.value.trim().to_string()).filter(|v| !v.is_empty());
    let first = |k: ElementKind| all(k).next();

    let eps: Vec<u32> = all(ElementKind::Episode).flat_map(|v| numbers(&v)).collect();
    let episodes = match (eps.first(), eps.last()) {
        (Some(&a), Some(&b)) => Some((a.min(b), a.max(b))),
        _ => None,
    };
    let info: Vec<String> = all(ElementKind::ReleaseInformation).map(|v| v.to_lowercase()).collect();
    let mut title_s = first(ElementKind::Title).unwrap_or_default();
    // "Title (Batch)" or "Title Complete" can stay in the title.
    let mut said_batch = info.iter().any(|i| BATCH_WORDS.contains(&i.as_str()));
    let lower = title_s.to_lowercase();
    for w in BATCH_WORDS.iter().rev() {
        if let Some(cut) = lower.strip_suffix(w)
            && cut.ends_with([' ', '-', '('])
        {
            title_s = title_s[..cut.len()].trim_end_matches([' ', '-', '(']).to_string();
            said_batch = true;
            break;
        }
    }
    let mut movie = false;
    for w in [" - movie", " movie", " - film", " the movie"] {
        if title_s.to_lowercase().ends_with(w) && title_s.len() > w.len() {
            title_s.truncate(title_s.len() - w.len());
            movie = true;
            break;
        }
    }
    movie |= all(ElementKind::Type).any(|t| t.eq_ignore_ascii_case("movie") || t.eq_ignore_ascii_case("film"));
    let words: Vec<String> = name.split(|c: char| !c.is_alphanumeric()).map(str::to_lowercase).collect();
    let last = info.iter().any(|i| i == "end" || i == "final") || words.iter().any(|w| w == "end" || w == "final");
    let season = first(ElementKind::Season).and_then(|s| last_number(&s));
    let batch = said_batch || episodes.is_some_and(|(a, b)| a != b) || (episodes.is_none() && season.is_some());

    let terms: Vec<String> = all(ElementKind::VideoTerm).collect();
    let version = first(ElementKind::ReleaseVersion).and_then(|v| last_number(&v)).filter(|v| *v > 1);
    Release {
        parsed: Parsed {
            file_name: title.to_string(),
            title: title_s,
            episode: episodes.map(|(_, b)| b),
            season,
            group: first(ElementKind::ReleaseGroup),
            resolution: first(ElementKind::VideoResolution),
        },
        episodes,
        batch,
        version,
        codec: terms.iter().find_map(|t| codec_of(t)),
        ten_bit: terms.iter().any(|t| is_ten_bit(t)),
        source: first(ElementKind::Source),
        audio: all(ElementKind::AudioTerm).collect(),
        languages: all(ElementKind::Language).collect(),
        subtitles: all(ElementKind::Subtitles).collect(),
        checksum: first(ElementKind::FileChecksum).map(|c| c.to_ascii_uppercase()),
        movie,
        last,
    }
}
