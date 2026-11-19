//! Checking the user's feeds: one HTTP GET per feed with `If-None-Match`/`If-Modified-Since`
//! (a 304 parses nothing), a 5 MB cap, 30 s timeout, and the schedule: every interval (at
//! least 10 min, or the feed's `<ttl>`), ±10 % jitter, doubling back-off on errors up to 6 h.
//! Errors never show the URL's key ([`super::mask`]).

use super::{Feed, FeedError, MAX_BODY, mask, parse};

/// The answer to one check.
#[derive(Debug, PartialEq, Eq)]
pub enum Fetched {
    /// 304: nothing new.
    Same,
    New {
        body: String,
        etag: Option<String>,
        modified: Option<String>,
    },
}

/// One GET. `Err`: a message without the URL's secrets.
pub fn fetch(url: &str, etag: Option<&str>, modified: Option<&str>) -> Result<Fetched, String> {
    if super::safe_http(url).is_none() {
        return Err("only http and https feed addresses work".into());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .http_status_as_error(false)
        .user_agent(concat!("Kotora/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut req = agent.get(url).header(
        "Accept",
        "application/rss+xml, application/atom+xml, application/xml, text/xml;q=0.9, */*;q=0.5",
    );
    if let Some(e) = etag {
        req = req.header("If-None-Match", e);
    }
    if let Some(m) = modified {
        req = req.header("If-Modified-Since", m);
    }
    let clean = |e: String| e.replace(url, &mask(url));
    let mut resp = req.call().map_err(|e| clean(e.to_string()))?;
    match resp.status().as_u16() {
        304 => Ok(Fetched::Same),
        200 => {
            let header = |k: &str| {
                resp.headers()
                    .get(k)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
            };
            let (etag, modified) = (header("etag"), header("last-modified"));
            let body = resp
                .body_mut()
                .with_config()
                .limit(MAX_BODY as u64)
                .read_to_string()
                .map_err(|e| clean(e.to_string()))?;
            Ok(Fetched::New {
                body,
                etag,
                modified,
            })
        }
        s => Err(format!("HTTP {s}")),
    }
}

/// What Download hands to the system: the magnet, else the .torrent file fetched into `dir`
/// (5 MB at most, it must be bencode). Files there older than 7 days are deleted first.
pub fn download_target(item: &super::Item, dir: &std::path::Path) -> Result<String, String> {
    if let Some(m) = item.magnet.as_deref().and_then(super::safe_magnet) {
        return Ok(m);
    }
    let Some(url) = item.torrent.as_deref().and_then(super::safe_http) else {
        return Err("This release has no magnet link or .torrent file.".into());
    };
    std::fs::create_dir_all(dir).map_err(|e| format!("Can't make the torrent folder: {e}"))?;
    if let Ok(rd) = std::fs::read_dir(dir) {
        let week = std::time::Duration::from_secs(7 * 86_400);
        for f in rd.flatten() {
            if f.metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > week)
            {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .http_status_as_error(false)
        .user_agent(concat!("Kotora/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let clean = |e: String| e.replace(&url, &mask(&url));
    let mut resp = agent.get(&url).call().map_err(|e| clean(e.to_string()))?;
    if resp.status().as_u16() != 200 {
        return Err(format!(
            "The .torrent file: HTTP {}",
            resp.status().as_u16()
        ));
    }
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(MAX_BODY as u64)
        .read_to_vec()
        .map_err(|e| clean(e.to_string()))?;
    if bytes.first() != Some(&b'd') {
        return Err("The link gave no .torrent file.".into());
    }
    let name = item
        .info_hash
        .clone()
        .unwrap_or_else(|| format!("{:016x}", fnv(&url)));
    let path = dir.join(format!("{name}.torrent"));
    std::fs::write(&path, bytes).map_err(|e| format!("Can't save the .torrent file: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

/// What Kotora keeps about one feed between checks (in memory).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeedState {
    pub etag: Option<String>,
    pub modified: Option<String>,
    /// The last check that got an answer, unix seconds.
    pub checked: Option<i64>,
    /// The next check, unix seconds.
    pub next: i64,
    pub error: Option<String>,
    /// Errors in a row (the back-off).
    pub failures: u32,
    /// The feed's `<ttl>`, minutes.
    pub ttl: Option<u32>,
    /// The items last read.
    pub items: usize,
}

/// The longest back-off.
pub const MAX_BACKOFF: i64 = 6 * 3600;

/// ±10 % of `secs`, from a cheap seed (no random crate).
fn jitter(secs: i64, seed: u64) -> i64 {
    let span = secs / 10;
    if span == 0 {
        return 0;
    }
    let x = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407)
        >> 33;
    (x % (2 * span as u64 + 1)) as i64 - span
}

impl FeedState {
    /// The interval for this feed, seconds: the user's, raised by the feed's ttl.
    pub fn interval(&self, minutes: u32) -> i64 {
        i64::from(
            minutes
                .max(crate::config::Feeds::MIN_MINUTES)
                .max(self.ttl.unwrap_or(0).min(24 * 60)),
        ) * 60
    }

    pub fn due(&self, now: i64) -> bool {
        now >= self.next
    }

    /// After a check that got an answer (new or 304).
    pub fn ok(&mut self, now: i64, minutes: u32, seed: u64) {
        self.checked = Some(now);
        self.error = None;
        self.failures = 0;
        let i = self.interval(minutes);
        self.next = now + i + jitter(i, seed);
    }

    /// After a failed check: back off (interval × 2ⁿ, at most 6 h).
    pub fn failed(&mut self, now: i64, minutes: u32, error: String) {
        self.failures = self.failures.saturating_add(1);
        self.error = Some(error);
        let wait = (self.interval(minutes) << self.failures.min(6)).min(MAX_BACKOFF);
        self.next = now + wait;
    }

    /// Reads a body into this state (ttl) and gives the feed.
    pub fn read(&mut self, body: &str) -> Result<Feed, FeedError> {
        let f = parse(body)?;
        self.ttl = f.ttl;
        self.items = f.items.len();
        Ok(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_and_backoff() {
        let mut st = FeedState::default();
        assert!(st.due(0));
        st.ok(1000, 30, 7);
        let i = 30 * 60;
        assert!(
            (1000 + i - i / 10..=1000 + i + i / 10).contains(&st.next),
            "{}",
            st.next
        );
        // The floor is 10 minutes; a ttl raises it.
        assert_eq!(st.interval(1), 600);
        st.ttl = Some(60);
        assert_eq!(st.interval(30), 3600);
        st.ttl = None;
        st.failed(2000, 30, "HTTP 503".into());
        assert_eq!(
            (st.next, st.failures, st.error.as_deref()),
            (2000 + 2 * i, 1, Some("HTTP 503"))
        );
        for _ in 0..10 {
            st.failed(2000, 30, "HTTP 503".into());
        }
        assert_eq!(st.next, 2000 + MAX_BACKOFF);
        st.ok(3000, 30, 1);
        assert_eq!((st.failures, st.error.clone()), (0, None));
    }

    #[test]
    fn jitter_stays_within_ten_percent() {
        for seed in 0..500 {
            assert!(jitter(1800, seed).abs() <= 180);
        }
    }

    #[test]
    fn magnets_go_as_they_are() {
        let it = super::super::Item {
            magnet: Some("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567".into()),
            torrent: Some("https://x.example/a.torrent".into()),
            ..Default::default()
        };
        assert!(
            download_target(&it, std::path::Path::new("/nonexistent"))
                .unwrap()
                .starts_with("magnet:")
        );
        let none = super::super::Item {
            torrent: Some("file:///x.torrent".into()),
            ..Default::default()
        };
        assert!(download_target(&none, std::path::Path::new("/nonexistent")).is_err());
    }

    #[test]
    fn non_http_addresses_never_go_out() {
        assert!(fetch("file:///etc/passwd", None, None).is_err());
        assert!(fetch("javascript:alert(1)", None, None).is_err());
    }

    #[test]
    fn read_keeps_the_ttl() {
        let mut st = FeedState::default();
        let body = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/feeds/fixtures/nyaa.xml"),
        )
        .unwrap();
        let f = st.read(&body).unwrap();
        assert_eq!((st.ttl, st.items, f.items.len()), (Some(15), 3, 3));
    }
}
