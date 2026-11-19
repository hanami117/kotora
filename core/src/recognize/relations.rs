//! Episode redirection with anime-relations (erengy/anime-relations, public domain / CC0), as
//! Taiga does it. Releases often number a split season on: "Season 2 - 15" is episode 2 of
//! "2nd Season Part 2" on MyAnimeList. A rule says so:
//!
//! ```text
//! # Re:Zero kara Hajimeru Isekai Seikatsu 2nd Season -> ~ Part 2
//! - 39587|42198|108632:14-25 -> 42203|43247|119661:1-12!
//! ```
//!
//! Ids are MyAnimeList | Kitsu | AniList, `?` an unknown id or end of range, `~` the source's id,
//! and `!` adds the same rule for the destination itself ("Part 2 - 15" is its episode 2 too).
//! The comment above a rule names the titles (`~` there is the source's title).
//!
//! A copy of the file is built in ([`BUNDLED`], about 50 KB). The app may download a newer one
//! at most once a week into the cache dir ([`refresh`]); [`load`] takes the newer of the two.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The copy of `anime-relations.txt` built into the app.
pub const BUNDLED: &str = include_str!("../../data/anime-relations.txt");

/// Where the app downloads a newer copy from (GitHub, no account, no cookies).
pub const URL: &str =
    "https://raw.githubusercontent.com/erengy/anime-relations/master/anime-relations.txt";

/// How often the download is tried at most.
pub const REFRESH_SECS: u64 = 7 * 24 * 3600;

/// After a failed download, the next try.
pub const RETRY_SECS: u64 = 24 * 3600;

/// One anime's ids: MyAnimeList, Kitsu, AniList. `None` is `?`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ids {
    pub mal: Option<u32>,
    pub kitsu: Option<u32>,
    pub anilist: Option<u32>,
}

/// An episode or a range of episodes. `last: None` is open (`13-?`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub first: u32,
    pub last: Option<u32>,
}

impl Range {
    fn contains(&self, ep: u32) -> bool {
        ep >= self.first && self.last.is_none_or(|l| ep <= l)
    }

    fn single(&self) -> bool {
        self.last == Some(self.first)
    }
}

/// One rule: `from` episodes of one anime are `to` episodes of another (or the same).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub from: Ids,
    pub from_eps: Range,
    pub to: Ids,
    pub to_eps: Range,
    /// Titles from the comment above the rule, when it names them.
    pub from_title: Option<String>,
    pub to_title: Option<String>,
}

impl Rule {
    /// The destination episode for a source episode, if the rule covers it.
    pub fn map(&self, ep: u32) -> Option<u32> {
        if !self.from_eps.contains(ep) {
            return None;
        }
        // A range to one episode (`1-4 -> 1`): all of them are that episode.
        let out = if self.to_eps.single() && !self.from_eps.single() {
            self.to_eps.first
        } else {
            self.to_eps.first + (ep - self.from_eps.first)
        };
        self.to_eps.contains(out).then_some(out)
    }
}

/// Where an episode goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Redirect {
    /// The destination's MyAnimeList id.
    pub to: u32,
    pub episode: u32,
    /// The destination's title from the file's comment, if any.
    pub to_title: Option<String>,
    pub from_title: Option<String>,
}

/// The parsed file, indexed by MyAnimeList id.
#[derive(Clone, Debug, Default)]
pub struct Relations {
    /// `::meta` `version` (`1.3.0`).
    pub version: String,
    /// `::meta` `last_modified` (`2026-10-19`): which copy is newer.
    pub last_modified: String,
    pub rules: Vec<Rule>,
    /// Lines in `::rules` that could not be read (skipped).
    pub skipped: usize,
    /// Lookups ([`Relations::redirect`]) take and give AniList ids instead of MyAnimeList's:
    /// set while AniList is the active service.
    pub anilist: bool,
    by_mal: HashMap<u32, Vec<usize>>,
    by_anilist: HashMap<u32, Vec<usize>>,
}

fn parse_id(s: &str, source: Option<u32>) -> Result<Option<u32>, ()> {
    match s.trim() {
        "?" => Ok(None),
        "~" => Ok(source),
        n => n.parse().map(Some).map_err(|_| ()),
    }
}

fn parse_ids(s: &str, source: Option<&Ids>) -> Result<Ids, ()> {
    let s = s.trim();
    if s == "~" {
        return source.copied().ok_or(());
    }
    let parts: Vec<&str> = s.split('|').collect();
    let [m, k, a] = parts[..] else { return Err(()) };
    Ok(Ids {
        mal: parse_id(m, source.and_then(|i| i.mal))?,
        kitsu: parse_id(k, source.and_then(|i| i.kitsu))?,
        anilist: parse_id(a, source.and_then(|i| i.anilist))?,
    })
}

fn parse_range(s: &str) -> Result<Range, ()> {
    let num = |t: &str| t.trim().parse::<u32>().map_err(|_| ());
    match s.split_once('-').map(|(a, b)| (a, b.trim())) {
        None => {
            let n = num(s)?;
            Ok(Range {
                first: n,
                last: Some(n),
            })
        }
        Some((a, "?")) => Ok(Range {
            first: num(a)?,
            last: None,
        }),
        Some((a, b)) => {
            let (first, last) = (num(a)?, num(b)?);
            if last < first {
                return Err(());
            }
            Ok(Range {
                first,
                last: Some(last),
            })
        }
    }
}

/// `ids:range` → (ids, range).
fn parse_side(s: &str, source: Option<&Ids>) -> Result<(Ids, Range), ()> {
    let (ids, eps) = s.trim().rsplit_once(':').ok_or(())?;
    Ok((parse_ids(ids, source)?, parse_range(eps)?))
}

/// One rule line without its `- ` (and `!` expanded): one or two rules.
fn parse_rule(line: &str) -> Result<Vec<Rule>, ()> {
    let (lhs, rhs) = line.split_once("->").ok_or(())?;
    let (rhs, both) = match rhs.trim().strip_suffix('!') {
        Some(r) => (r, true),
        None => (rhs.trim(), false),
    };
    let (from, from_eps) = parse_side(lhs, None)?;
    let (to, to_eps) = parse_side(rhs, Some(&from))?;
    let mut out = vec![Rule {
        from,
        from_eps,
        to,
        to_eps,
        from_title: None,
        to_title: None,
    }];
    if both && to != from {
        out.push(Rule {
            from: to,
            from_eps,
            to,
            to_eps,
            from_title: None,
            to_title: None,
        });
    }
    Ok(out)
}

/// `# A -> ~ Part 2` → ("A", "A Part 2").
fn comment_titles(c: &str) -> Option<(String, String)> {
    let (a, b) = c.split_once(" -> ")?;
    let a = a.trim().to_string();
    let b = b.trim();
    let b = match b.strip_prefix('~') {
        Some(rest) => format!("{a}{rest}"),
        None => b.to_string(),
    };
    (!a.is_empty() && !b.is_empty()).then_some((a, b))
}

impl Relations {
    /// Parses the file. Unknown sections and lines are skipped; a file with no `::rules` or no
    /// version is an error.
    pub fn parse(text: &str) -> Result<Relations, String> {
        let mut r = Relations::default();
        let mut section = "";
        let mut titles: Option<(String, String)> = None;
        let mut saw_rules = false;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                titles = None;
                continue;
            }
            if let Some(name) = line.strip_prefix("::") {
                section = match name.trim() {
                    "meta" => "meta",
                    "rules" => {
                        saw_rules = true;
                        "rules"
                    }
                    _ => "",
                };
                continue;
            }
            if let Some(c) = line.strip_prefix('#') {
                if section == "rules" {
                    titles = comment_titles(c.trim());
                }
                continue;
            }
            let Some(body) = line.strip_prefix('-').map(str::trim) else {
                if section == "rules" {
                    r.skipped += 1;
                }
                continue;
            };
            match section {
                "meta" => {
                    if let Some((k, v)) = body.split_once(':') {
                        match k.trim() {
                            "version" => r.version = v.trim().to_string(),
                            "last_modified" => r.last_modified = v.trim().to_string(),
                            _ => {}
                        }
                    }
                }
                "rules" => match parse_rule(body) {
                    Ok(rules) => {
                        for (i, mut rule) in rules.into_iter().enumerate() {
                            if let Some((a, b)) = &titles {
                                // The `!` copy goes from the destination to itself.
                                rule.from_title = Some(if i == 0 { a.clone() } else { b.clone() });
                                rule.to_title = Some(b.clone());
                            }
                            r.rules.push(rule);
                        }
                    }
                    Err(()) => r.skipped += 1,
                },
                _ => {}
            }
        }
        if !saw_rules {
            return Err("anime-relations: no ::rules section".into());
        }
        if r.version.split('.').next() != Some("1") {
            return Err(format!(
                "anime-relations: version {:?} is not one Kotora reads",
                r.version
            ));
        }
        for (i, rule) in r.rules.iter().enumerate() {
            if let (Some(_), Some(id)) = (rule.to.mal, rule.from.mal) {
                r.by_mal.entry(id).or_default().push(i);
            }
            if let (Some(_), Some(id)) = (rule.to.anilist, rule.from.anilist) {
                r.by_anilist.entry(id).or_default().push(i);
            }
        }
        Ok(r)
    }

    /// The built-in copy.
    pub fn bundled() -> Relations {
        Relations::parse(BUNDLED).unwrap_or_default()
    }

    /// Where episode `ep` of MyAnimeList anime `id` goes, if a rule moves it.
    pub fn mal(&self, id: u32, ep: u32) -> Option<Redirect> {
        self.by_mal
            .get(&id)?
            .iter()
            .map(|&i| &self.rules[i])
            .find_map(|rule| {
                let episode = rule.map(ep)?;
                let to = rule.to.mal?;
                // A rule that maps an episode to itself changes nothing.
                (to != id || episode != ep).then(|| Redirect {
                    to,
                    episode,
                    to_title: rule.to_title.clone(),
                    from_title: rule.from_title.clone(),
                })
            })
    }

    /// Where episode `ep` of anime `id` goes, in the ids of the active service (MyAnimeList's,
    /// or AniList's when [`Relations::anilist`] is set).
    pub fn redirect(&self, id: u32, ep: u32) -> Option<Redirect> {
        if !self.anilist {
            return self.mal(id, ep);
        }
        self.by_anilist
            .get(&id)?
            .iter()
            .map(|&i| &self.rules[i])
            .find_map(|rule| {
                let episode = rule.map(ep)?;
                let to = rule.to.anilist?;
                (to != id || episode != ep).then(|| Redirect {
                    to,
                    episode,
                    to_title: rule.to_title.clone(),
                    from_title: rule.from_title.clone(),
                })
            })
    }

    /// Rules usable with MyAnimeList ids.
    pub fn mal_rules(&self) -> usize {
        self.by_mal.values().map(Vec::len).sum()
    }
}

// ------------------------------------------------------------------------- the downloaded copy

/// The downloaded copy in the cache dir.
pub fn cache_file(cache_dir: &Path) -> PathBuf {
    cache_dir.join("anime-relations.txt")
}

/// When the download was last tried (unix seconds) and the `ETag` to send next time.
fn state_file(cache_dir: &Path) -> PathBuf {
    cache_dir.join("anime-relations.state")
}

fn read_state(cache_dir: &Path) -> (u64, Option<String>) {
    let text = std::fs::read_to_string(state_file(cache_dir)).unwrap_or_default();
    let mut lines = text.lines();
    let at = lines
        .next()
        .and_then(|l| l.trim().parse().ok())
        .unwrap_or(0);
    let etag = lines
        .next()
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(str::to_string);
    (at, etag)
}

fn write_state(cache_dir: &Path, now: u64, etag: Option<&str>) -> std::io::Result<()> {
    std::fs::write(
        state_file(cache_dir),
        format!("{now}\n{}\n", etag.unwrap_or("")),
    )
}

/// The newer of the downloaded copy (if it reads) and the built-in one.
pub fn load(cache_dir: &Path) -> Relations {
    let bundled = Relations::bundled();
    match std::fs::read_to_string(cache_file(cache_dir))
        .ok()
        .and_then(|t| Relations::parse(&t).ok())
    {
        Some(c) if c.last_modified > bundled.last_modified && c.mal_rules() > 0 => c,
        _ => bundled,
    }
}

/// True when the last try is a week old or more (or never happened).
pub fn due(cache_dir: &Path, now: u64) -> bool {
    let (at, _) = read_state(cache_dir);
    now.saturating_sub(at) >= REFRESH_SECS || at > now
}

/// A download's answer.
pub enum Fetched {
    /// 200 with the body and its `ETag`.
    New { body: String, etag: Option<String> },
    /// 304: the copy we have is current.
    Same,
}

/// Downloads a newer copy when [`due`]: `fetch` gets the saved `ETag` and does the request (the
/// app passes [`fetch_github`]). A file that does not read, or has fewer rules than the
/// built-in copy's half, is refused and the last good copy stays. `Ok(Some)`: a newer copy was
/// saved.
pub fn refresh(
    cache_dir: &Path,
    now: u64,
    fetch: impl FnOnce(Option<&str>) -> Result<Fetched, String>,
) -> Result<Option<Relations>, String> {
    if !due(cache_dir, now) {
        return Ok(None);
    }
    crate::paths::create_private_dir(cache_dir).map_err(|e| e.to_string())?;
    let (_, etag) = read_state(cache_dir);
    // Only send the tag when the copy it belongs to is still there.
    let etag = etag.filter(|_| cache_file(cache_dir).exists());
    let answer = fetch(etag.as_deref());
    // Tried: wait a week, or a day after no answer at all (offline for a moment).
    let at = if answer.is_err() {
        now.saturating_sub(REFRESH_SECS - RETRY_SECS)
    } else {
        now
    };
    let _ = write_state(cache_dir, at, etag.as_deref());
    let (body, new_etag) = match answer? {
        Fetched::Same => return Ok(None),
        Fetched::New { body, etag } => (body, etag),
    };
    let rel = Relations::parse(&body)?;
    if rel.mal_rules() < Relations::bundled().mal_rules() / 2 {
        return Err(format!(
            "anime-relations: only {} rules, kept the last good copy",
            rel.mal_rules()
        ));
    }
    let tmp = cache_dir.join("anime-relations.txt.tmp");
    std::fs::write(&tmp, &body)
        .and_then(|_| std::fs::rename(&tmp, cache_file(cache_dir)))
        .map_err(|e| e.to_string())?;
    let _ = write_state(cache_dir, now, new_etag.as_deref());
    Ok(Some(rel))
}

/// The real download: one `GET` of [`URL`], with `If-None-Match` when we have a copy. No cookies,
/// no account, only Kotora's version in the `User-Agent`.
pub fn fetch_github(etag: Option<&str>) -> Result<Fetched, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .http_status_as_error(false)
        .user_agent(concat!("Kotora/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut req = agent.get(URL);
    if let Some(e) = etag {
        req = req.header("If-None-Match", e);
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    match resp.status().as_u16() {
        304 => Ok(Fetched::Same),
        200 => {
            let etag = resp
                .headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let body = resp
                .body_mut()
                .with_config()
                .limit(2 * 1024 * 1024)
                .read_to_string()
                .map_err(|e| e.to_string())?;
            Ok(Fetched::New { body, etag })
        }
        s => Err(format!("anime-relations: HTTP {s}")),
    }
}

#[cfg(test)]
mod tests;
