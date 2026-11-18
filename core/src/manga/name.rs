//! A file name parser for manga and comic files and their folders.
//!
//! It reads names like these (the series, volume, chapter, group, language, year and extras):
//!
//! ```text
//! Berserk v03 c021 (2019) (Digital) (1r0n).cbz
//! [Group] Series Name - Ch. 21.5 [en].cbz
//! Series Name - Chapter 021-025.cbz
//! Library/Series Name (2019)/Volume 03/Chapter 21.cbz
//! ```
//!
//! Rules, in short:
//!
//! - The extension is dropped only when it is a known one (`cbz cbr cb7 cbt zip rar 7z pdf epub`),
//!   so `Ch.21.5` keeps its dot.
//! - `[...]`, `(...)` and `{...}` are lifted out first. A year (`2019`), a language (`en`,
//!   `English`), `Oneshot` and a known extra (`Digital`, `Color`, ...) are recognised wherever they
//!   are. Of the rest, the first `[...]` is the **group**. Everything else is an extra (`1r0n`).
//!   A bracket that holds only a marker (`(v03)`, `[c021]`) is read as that marker.
//! - Markers: volume is `v03`, `Vol.3`, `Volume 03`, `Tome 3`, `第3巻`. Chapter is `c021`, `Ch.21`, `Ch 21.5`,
//!   `Chapter 21`, `第21話`, `#021`, or a number after ` - ` (`Series - 021`). Numbers can be decimal
//!   (`21.5`) or a range (`c001-005`). A trailing letter (`c021a`) or version (`c021v2`) is ignored.
//! - The series is the text before the first marker. The text after the chapter marker is the
//!   chapter's title.
//! - A bare number at the end (`Series 021`) is a chapter only when it has a leading zero. Without
//!   one, it stays in the title (`Mob Psycho 100`), unless the parent folder has the title without
//!   the number ([`parse_path`]).
//! - A name without a series (`c021.cbz`, `Chapter 21 - Name.cbz`) takes it from the nearest parent
//!   folder that has one. Folders that are only a volume or a chapter (`Volume 03`) are skipped
//!   and lend their number.

use std::fmt;

/// One number or a range of numbers: `21`, `21.5`, `1-5`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub first: f64,
    pub last: f64,
}

impl Span {
    fn one(n: f64) -> Span {
        Span { first: n, last: n }
    }

    pub fn is_range(&self) -> bool {
        self.last > self.first
    }

    /// Whether `n` is inside the span.
    pub fn contains(&self, n: f64) -> bool {
        n >= self.first && n <= self.last
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_range() { write!(f, "{}-{}", self.first, self.last) } else { write!(f, "{}", self.first) }
    }
}

/// What a file name says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parsed {
    /// The series' title. Empty when neither the name nor a folder has one.
    pub series: String,
    pub volume: Option<Span>,
    pub chapter: Option<Span>,
    /// The text after the chapter number (`Series - Ch. 21 - The Fight`).
    pub chapter_title: Option<String>,
    /// The scanlation group or ripper, from the first `[...]`.
    pub group: Option<String>,
    /// A short lower-case code: `en`, `ja`, `fr`, `pt-br`, ...
    pub language: Option<String>,
    pub year: Option<u16>,
    /// Everything else in brackets: `Digital`, `1r0n`, `Color`.
    pub extras: Vec<String>,
    /// `[Oneshot]` or `- Oneshot`. A one-shot has no chapter number.
    pub oneshot: bool,
}

impl Parsed {
    /// The highest chapter number, for "the user has read up to this".
    pub fn last_chapter(&self) -> Option<f64> {
        self.chapter.map(|c| c.last)
    }
}

const EXTENSIONS: [&str; 9] = ["cbz", "cbr", "cb7", "cbt", "zip", "rar", "7z", "pdf", "epub"];

const LANGUAGES: [(&str, &str); 44] = [
    ("en", "en"),
    ("eng", "en"),
    ("english", "en"),
    ("ja", "ja"),
    ("jp", "ja"),
    ("jpn", "ja"),
    ("japanese", "ja"),
    ("fr", "fr"),
    ("fre", "fr"),
    ("fra", "fr"),
    ("french", "fr"),
    ("es", "es"),
    ("spa", "es"),
    ("spanish", "es"),
    ("de", "de"),
    ("ger", "de"),
    ("deu", "de"),
    ("german", "de"),
    ("it", "it"),
    ("ita", "it"),
    ("italian", "it"),
    ("pt", "pt"),
    ("por", "pt"),
    ("portuguese", "pt"),
    ("pt-br", "pt-br"),
    ("ptbr", "pt-br"),
    ("ru", "ru"),
    ("rus", "ru"),
    ("russian", "ru"),
    ("zh", "zh"),
    ("chi", "zh"),
    ("cn", "zh"),
    ("chinese", "zh"),
    ("ko", "ko"),
    ("kor", "ko"),
    ("korean", "ko"),
    ("id", "id"),
    ("vi", "vi"),
    ("pl", "pl"),
    ("tr", "tr"),
    ("th", "th"),
    ("ar", "ar"),
    ("es-la", "es"),
    ("es-es", "es"),
];

const EXTRAS: [&str; 24] = [
    "digital",
    "digital-hd",
    "digital hd",
    "web",
    "scan",
    "scans",
    "scanned",
    "color",
    "colour",
    "colored",
    "colorized",
    "fan colored",
    "full color",
    "official",
    "premium",
    "hq",
    "raw",
    "raws",
    "complete",
    "uncensored",
    "decensored",
    "fixed",
    "retail",
    "mixed",
];

fn language_code(s: &str) -> Option<&'static str> {
    let l = s.trim().to_lowercase();
    LANGUAGES.iter().find(|(k, _)| *k == l).map(|(_, v)| *v)
}

fn is_extra(s: &str) -> bool {
    let l = s.trim().to_lowercase();
    EXTRAS.contains(&l.as_str())
}

fn is_oneshot(s: &str) -> bool {
    matches!(s.trim().to_lowercase().as_str(), "oneshot" | "one-shot" | "one shot" | "one_shot")
}

fn year_of(s: &str) -> Option<u16> {
    let s = s.trim();
    if s.len() == 4 && s.bytes().all(|b| b.is_ascii_digit()) {
        let y: u16 = s.parse().ok()?;
        if (1900..=2099).contains(&y) {
            return Some(y);
        }
    }
    None
}

/// The name without a known extension.
fn strip_ext(name: &str) -> &str {
    if let Some((stem, ext)) = name.rsplit_once('.')
        && EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
        && !stem.is_empty()
    {
        return stem;
    }
    name
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Volume,
    Chapter,
}

#[derive(Clone, Copy, Debug)]
struct Marker {
    kind: Kind,
    start: usize,
    end: usize,
    span: Span,
}

/// The result of reading one name, with the one thing only the path can settle.
#[derive(Debug)]
struct Inner {
    parsed: Parsed,
    /// A bare trailing number without a leading zero (`Berserk 364`): the title without the
    /// number, and the number. Only a matching parent folder makes it a chapter.
    bare: Option<(String, f64)>,
}

fn is_sep(c: char) -> bool {
    c.is_whitespace() || matches!(c, '-' | '_' | ',' | ':' | ';' | '~' | '|' | '/')
}

fn clean(s: &str) -> String {
    let t = s.trim_matches(is_sep);
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A number at `at`: `21`, `21.5`, then an optional `-N` range. Returns the span and the end.
/// The number must end the word, or be followed by one letter (`c021a`) or a version (`c021v2`).
/// `letter` is the marker's own letter (`c001-c005` is a range too).
fn number(ch: &[char], at: usize, letter: Option<char>) -> Option<(Span, usize)> {
    let (first, mut i) = plain_number(ch, at)?;
    let mut span = Span::one(first);
    if i < ch.len() && ch[i] == '-' {
        let mut j = i + 1;
        if let Some(l) = letter
            && j < ch.len()
            && ch[j].to_ascii_lowercase() == l
        {
            j += 1;
        }
        if let Some((last, e)) = plain_number(ch, j)
            && last >= first
        {
            span.last = last;
            i = e;
        }
    }
    // What may follow: the end, a non-alphanumeric, `a`..`z` once, or `v<digits>`.
    if i < ch.len() && ch[i].is_alphanumeric() {
        let c = ch[i].to_ascii_lowercase();
        if c == 'v' && i + 1 < ch.len() && ch[i + 1].is_ascii_digit() {
            i += 1;
            while i < ch.len() && ch[i].is_ascii_digit() {
                i += 1;
            }
        } else if c.is_ascii_lowercase() && (i + 1 >= ch.len() || !ch[i + 1].is_alphanumeric()) {
            i += 1;
        } else {
            return None;
        }
        if i < ch.len() && ch[i].is_alphanumeric() {
            return None;
        }
    }
    Some((span, i))
}

fn plain_number(ch: &[char], at: usize) -> Option<(f64, usize)> {
    let mut i = at;
    while i < ch.len() && ch[i].is_ascii_digit() {
        i += 1;
    }
    if i == at {
        return None;
    }
    if i + 1 < ch.len() && ch[i] == '.' && ch[i + 1].is_ascii_digit() {
        i += 1;
        while i < ch.len() && ch[i].is_ascii_digit() {
            i += 1;
        }
    }
    let s: String = ch[at..i].iter().collect();
    s.parse().ok().map(|n| (n, i))
}

fn starts_with_ci(ch: &[char], at: usize, word: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    at + w.len() <= ch.len() && ch[at..at + w.len()].iter().zip(&w).all(|(a, b)| a.to_ascii_lowercase() == *b)
}

/// A marker that starts at `at` (a word start).
fn marker_at(ch: &[char], at: usize) -> Option<Marker> {
    // Japanese: 第21話 (chapter 21), 第3巻 (volume 3).
    if ch.get(at) == Some(&'第') {
        let (first, i) = plain_number(ch, at + 1)?;
        let kind = match ch.get(i)? {
            '話' | '话' => Kind::Chapter,
            '巻' | '卷' => Kind::Volume,
            _ => return None,
        };
        return Some(Marker { kind, start: at, end: i + 1, span: Span::one(first) });
    }
    // (keyword, kind, digit must follow at once)
    const KEYWORDS: [(&str, Kind, bool); 10] = [
        ("volume", Kind::Volume, false),
        ("tome", Kind::Volume, false),
        ("vol", Kind::Volume, false),
        ("v", Kind::Volume, true),
        ("chapters", Kind::Chapter, false),
        ("chapter", Kind::Chapter, false),
        ("chap", Kind::Chapter, false),
        ("ch", Kind::Chapter, false),
        ("c", Kind::Chapter, true),
        ("#", Kind::Chapter, false),
    ];
    for (word, kind, tight) in KEYWORDS {
        if !starts_with_ci(ch, at, word) {
            continue;
        }
        let mut i = at + word.chars().count();
        if !tight {
            if i < ch.len() && ch[i] == '.' {
                i += 1;
            }
            while i < ch.len() && ch[i] == ' ' {
                i += 1;
            }
        }
        let letter = word.chars().next().filter(|c| c.is_alphabetic());
        if let Some((span, end)) = number(ch, i, letter.map(|c| c.to_ascii_lowercase())) {
            return Some(Marker { kind, start: at, end, span });
        }
    }
    None
}

/// All markers, left to right, and the weak chapter (a number after ` - `) if there is one.
fn find_markers(ch: &[char]) -> (Vec<Marker>, Option<Marker>) {
    let mut found = Vec::new();
    let mut weak = None;
    let mut i = 0;
    while i < ch.len() {
        let word_start = i == 0 || !ch[i - 1].is_alphanumeric();
        if (word_start || ch[i] == '第') && let Some(m) = marker_at(ch, i) {
            i = m.end;
            found.push(m);
            continue;
        }
        if word_start
            && weak.is_none()
            && ch[i].is_ascii_digit()
            && after_dash(ch, i)
            && let Some((span, end)) = number(ch, i, None)
            && year_of(&ch[i..end].iter().collect::<String>()).is_none()
        {
            weak = Some(Marker { kind: Kind::Chapter, start: i, end, span });
            i = end;
            continue;
        }
        i += 1;
    }
    (found, weak)
}

/// Whether `at` follows ` - ` (a dash with a space on both sides).
fn after_dash(ch: &[char], at: usize) -> bool {
    at >= 3 && ch[at - 1] == ' ' && ch[at - 2] == '-' && ch[at - 3] == ' '
}

/// `[..]`, `(..)`, `{..}` out of `s`: the text without them, and what they held.
fn lift_brackets(s: &str) -> (String, Vec<(char, String)>) {
    let ch: Vec<char> = s.chars().collect();
    let mut rest = String::new();
    let mut found = Vec::new();
    let mut i = 0;
    while i < ch.len() {
        let open = ch[i];
        let close = match open {
            '[' => ']',
            '(' => ')',
            '{' => '}',
            _ => {
                rest.push(open);
                i += 1;
                continue;
            }
        };
        // The matching close, with nesting of the same kind.
        let mut depth = 0;
        let mut end = None;
        for (j, &c) in ch.iter().enumerate().skip(i) {
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    end = Some(j);
                    break;
                }
            }
        }
        match end {
            Some(j) => {
                let content: String = ch[i + 1..j].iter().collect();
                found.push((open, content));
                rest.push(' ');
                i = j + 1;
            }
            None => {
                rest.push(open);
                i += 1;
            }
        }
    }
    (rest, found)
}

/// `Berserk.v03.c021` → `Berserk v03 c021`: names with no spaces use dots as spaces.
fn unglue(s: &str) -> String {
    let s = s.replace('_', " ");
    if s.trim().contains(' ') {
        return s;
    }
    let ch: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in ch.iter().enumerate() {
        let between_digits = i > 0 && i + 1 < ch.len() && ch[i - 1].is_ascii_digit() && ch[i + 1].is_ascii_digit();
        out.push(if c == '.' && !between_digits { ' ' } else { c });
    }
    out
}

/// Whether the whole of `s` is one marker (`v03`, `Vol. 3`, `c021`).
fn lone_marker(s: &str) -> bool {
    let ch: Vec<char> = s.trim().chars().collect();
    marker_at(&ch, 0).is_some_and(|m| m.end == ch.len())
}

fn parse_name(name: &str) -> Inner {
    let stem = strip_ext(name);
    let (rest, brackets) = lift_brackets(stem);
    let mut p = Parsed::default();
    let mut inline = String::new(); // markers that were in brackets: `(v03)`
    for (open, content) in brackets {
        let c = content.trim();
        if c.is_empty() {
            continue;
        }
        if lone_marker(c) {
            inline.push(' ');
            inline.push_str(c);
        } else if let Some(y) = year_of(c) {
            p.year = p.year.or(Some(y));
        } else if let Some(l) = language_code(c) {
            p.language = p.language.take().or(Some(l.to_string()));
        } else if is_oneshot(c) {
            p.oneshot = true;
        } else if open == '[' && p.group.is_none() && !is_extra(c) {
            p.group = Some(c.to_string());
        } else {
            p.extras.push(c.to_string());
        }
    }
    // Inline markers go last: `Title - Ch. 21 - Name (v03)` keeps the chapter's title clean.
    let text = format!("{}{}", unglue(&rest), unglue(&inline));
    let ch: Vec<char> = text.chars().collect();
    let (markers, weak) = find_markers(&ch);

    let volume = markers.iter().find(|m| m.kind == Kind::Volume).copied();
    let strong_chapter = markers.iter().find(|m| m.kind == Kind::Chapter).copied();
    let chapter = strong_chapter.or(weak);
    p.volume = volume.map(|m| m.span);
    p.chapter = chapter.map(|m| m.span);

    let mut used: Vec<Marker> = volume.into_iter().chain(chapter).collect();
    used.sort_by_key(|m| m.start);
    let title_end = used.first().map_or(ch.len(), |m| m.start);
    // The chapter's title: after the chapter marker, up to the next marker (a bracketed volume
    // was put at the end).
    if let Some(c) = chapter {
        let stop = used.iter().filter(|m| m.start >= c.end).map(|m| m.start).min().unwrap_or(ch.len());
        let sub = clean(&ch[c.end.min(stop)..stop].iter().collect::<String>());
        if is_oneshot(&sub) {
            p.oneshot = true;
        } else if !sub.is_empty() {
            p.chapter_title = Some(sub);
        }
    }
    let mut title: String = ch[..title_end].iter().collect();
    let mut bare = None;

    // No marker: a number at the end, or the whole name is a number.
    if used.is_empty() {
        let t = clean(&title);
        if let Some((head, tail)) = t.rsplit_once(' ') {
            let tc: Vec<char> = tail.chars().collect();
            if let Some((span, e)) = number(&tc, 0, None)
                && e == tc.len()
                && year_of(tail).is_none()
            {
                if tail.starts_with('0') && tail.len() > 1 {
                    p.chapter = Some(span);
                    title = head.to_string();
                } else if !span.is_range() {
                    bare = Some((clean(head), span.first));
                }
            }
        } else {
            let tc: Vec<char> = t.chars().collect();
            if let Some((span, e)) = number(&tc, 0, None)
                && e == tc.len()
                && year_of(&t).is_none()
            {
                p.chapter = Some(span);
                title = String::new();
            } else if let Some(y) = year_of(&t) {
                p.year = p.year.or(Some(y));
                title = String::new();
            }
        }
    }

    let mut series = clean(&title);
    // `Series - Oneshot`, `Series Oneshot`.
    for tail in ["oneshot", "one-shot", "one shot"] {
        let low = series.to_lowercase();
        if low.ends_with(tail) && (low.len() == tail.len() || !series.as_bytes()[low.len() - tail.len() - 1].is_ascii_alphanumeric()) {
            series = clean(&series[..series.len() - tail.len()]);
            p.oneshot = true;
            break;
        }
    }
    p.series = series;
    Inner { parsed: p, bare }
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Parses a file name (no folder).
pub fn parse_file_name(name: &str) -> Parsed {
    parse_name(name).parsed
}

/// Parses a path: the file name, and its folders for what the name lacks. The path may be
/// relative to a library; the folders above the library must not be part of it (the library's own
/// folder is not a series). Folders are split at `/` and at `\` (Windows paths).
pub fn parse_path(path: &str) -> Parsed {
    let comps: Vec<&str> = path.split(['/', '\\']).filter(|c| !c.is_empty()).collect();
    let Some((file, dirs)) = comps.split_last() else {
        return Parsed::default();
    };
    let Inner { mut parsed, bare } = parse_name(file);
    for dir in dirs.iter().rev().take(3) {
        let d = parse_name(dir).parsed;
        if parsed.volume.is_none() {
            parsed.volume = d.volume;
        }
        if parsed.year.is_none() {
            parsed.year = d.year;
        }
        if parsed.language.is_none() {
            parsed.language.clone_from(&d.language);
        }
        if d.series.is_empty() {
            // A "Volume 03" or "Chapter 21" folder: lend the number, look higher.
            if parsed.chapter.is_none() && !parsed.oneshot {
                parsed.chapter = d.chapter;
            }
            continue;
        }
        if parsed.series.is_empty() {
            parsed.series = d.series;
        } else if parsed.chapter.is_none()
            && let Some((head, n)) = &bare
            && norm(head) == norm(&d.series)
        {
            parsed.series = head.clone();
            parsed.chapter = Some(Span::one(*n));
        }
        break;
    }
    parsed
}

#[cfg(test)]
mod tests;
