//! Torrent / RSS feeds (desktop, opt-in, no default sources): reading a feed body.
//!
//! [`parse`] reads RSS 2.0 (and RSS 1.0), Atom 1.0 and Torznab/Newznab (RSS with
//! `torznab:attr`, as Jackett and Prowlarr give it) into [`Item`]s. Known extras are read
//! when there, in any namespace: `enclosure`, `torznab:attr` (`magneturl`, `infohash`, `size`,
//! `seeders`, `peers`), nyaa-style `infoHash`/`seeders`/`leechers`/`size`, and a `magnet:`
//! link. Links pass an allowlist ([`safe_magnet`], [`safe_http`]): a feed can never make
//! Kotora open `file:`, `javascript:` or an app's URL.
//!
//! Nothing here makes a request. URLs that carry a key are masked by [`mask`] wherever they
//! are shown; [`nyaa_defaults`] fills a name for a pasted nyaa.si URL without asking the site.

pub mod marks;
pub mod poll;
pub mod select;

use roxmltree::{Document, Node};

use crate::time::{parse_rfc2822, parse_rfc3339};

/// The largest body [`parse`] reads (5 MB).
pub const MAX_BODY: usize = 5 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Rss,
    Atom,
    /// RSS with `torznab:attr` or `newznab:attr`.
    Torznab,
}

/// One release in a feed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    /// The feed's id for it: `guid`/`id`, else the link, else the title.
    pub guid: String,
    pub title: String,
    /// The web page about it (never a magnet or a .torrent).
    pub page: Option<String>,
    pub magnet: Option<String>,
    /// A .torrent file's URL (http or https).
    pub torrent: Option<String>,
    /// BitTorrent v1 info hash, 40 lower-case hex digits (or a v2 multihash, lower case).
    pub info_hash: Option<String>,
    pub size: Option<u64>,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    /// Unix seconds.
    pub published: Option<i64>,
}

impl Item {
    /// The key that finds the same release in two feeds: the info hash, else the guid.
    pub fn key(&self) -> String {
        match &self.info_hash {
            Some(h) => format!("btih:{h}"),
            None => format!("guid:{}", self.guid),
        }
    }
}

/// A feed as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feed {
    pub format: Format,
    /// The channel's title: the default name of a new feed.
    pub title: Option<String>,
    /// RSS `<ttl>`, in minutes: the feed asks not to be checked more often.
    pub ttl: Option<u32>,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeedError {
    TooLarge,
    /// Not well-formed XML (or a DTD, which is never read).
    Xml(String),
    /// XML, but no RSS or Atom.
    NotAFeed,
    /// A Torznab indexer's `<error>`: its description.
    Indexer(String),
}

impl std::fmt::Display for FeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeedError::TooLarge => write!(f, "the feed is larger than 5 MB"),
            FeedError::Xml(e) => write!(f, "not a readable feed ({e})"),
            FeedError::NotAFeed => write!(f, "this address gives no RSS or Atom feed"),
            FeedError::Indexer(e) => write!(f, "the indexer said: {e}"),
        }
    }
}

impl std::error::Error for FeedError {}

fn local<'a>(n: &Node<'a, '_>) -> &'a str {
    n.tag_name().name()
}

fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.is_element() && local(c).eq_ignore_ascii_case(name))
}

/// All text inside (CDATA too), trimmed; empty is `None`.
fn text_of(n: Node) -> Option<String> {
    let s: String = n.descendants().filter(|d| d.is_text()).filter_map(|d| d.text()).collect();
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

fn child_text(n: Node, name: &str) -> Option<String> {
    child(n, name).and_then(text_of)
}

/// `magnet:?…xt=urn:btih:…` or `urn:btmh:`, nothing else; no spaces or control characters.
pub fn safe_magnet(s: &str) -> Option<String> {
    let s = s.trim();
    let ok = s.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("magnet:?"))
        && !s.chars().any(|c| c.is_whitespace() || c.is_control())
        && magnet_hash(s).is_some();
    ok.then(|| s.to_string())
}

/// An `http`/`https` URL with a host; no spaces or control characters.
pub fn safe_http(s: &str) -> Option<String> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"))?;
    let host_ok = rest.split(['/', '?', '#']).next().is_some_and(|h| !h.is_empty());
    (host_ok && !s.chars().any(|c| c.is_whitespace() || c.is_control())).then(|| s.to_string())
}

const B32: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";

/// A v1 info hash as 40 lower-case hex digits: from hex, or from the 32-letter base32 form.
/// A v2 multihash (`1220…`, 68 hex digits) stays as it is, lower case.
pub fn normalize_hash(h: &str) -> Option<String> {
    let h = h.trim().to_ascii_lowercase();
    if (h.len() == 40 || h.len() == 68) && h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(h);
    }
    if h.len() == 32 {
        let mut bits: u64 = 0;
        let mut n = 0;
        let mut out = String::with_capacity(40);
        for b in h.bytes() {
            bits = (bits << 5) | B32.iter().position(|c| *c == b)? as u64;
            n += 5;
            while n >= 8 {
                n -= 8;
                out.push_str(&format!("{:02x}", (bits >> n) & 0xff));
            }
        }
        return Some(out);
    }
    None
}

/// The info hash a magnet link names.
pub fn magnet_hash(m: &str) -> Option<String> {
    let q = m.split_once('?')?.1;
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        if !k.eq_ignore_ascii_case("xt") && !k.to_ascii_lowercase().starts_with("xt.") {
            return None;
        }
        let v = v.to_ascii_lowercase();
        v.strip_prefix("urn:btih:").or_else(|| v.strip_prefix("urn:btmh:")).and_then(normalize_hash)
    })
}

/// "1.2 GiB", "734.5 MiB", "700MB", "123456" (bytes).
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let cut = s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).unwrap_or(s.len());
    let n: f64 = s[..cut].replace(',', "").parse().ok()?;
    let unit = s[cut..].trim().to_ascii_lowercase();
    let m: f64 = match unit.as_str() {
        "" | "b" | "bytes" => 1.0,
        "kib" => 1024.0,
        "kb" => 1000.0,
        "mib" => 1024.0 * 1024.0,
        "mb" => 1e6,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        "gb" => 1e9,
        "tib" => 1024f64.powi(4),
        "tb" => 1e12,
        _ => return None,
    };
    (n >= 0.0).then(|| (n * m).round() as u64)
}

fn looks_like_torrent(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    path.ends_with(".torrent") || path.contains("/download/")
}

/// Puts a link where it belongs: a magnet, a .torrent, or the page.
fn place(it: &mut Item, url: &str, torrent_hint: bool) {
    if let Some(m) = safe_magnet(url) {
        it.magnet.get_or_insert(m);
    } else if let Some(u) = safe_http(url) {
        if torrent_hint || looks_like_torrent(&u) {
            it.torrent.get_or_insert(u);
        } else {
            it.page.get_or_insert(u);
        }
    }
}

fn number<T: std::str::FromStr>(s: &str) -> Option<T> {
    s.trim().parse().ok()
}

/// The extras of one item, whatever their namespace (`nyaa:`, `torznab:`, `newznab:`, …).
fn extras(n: Node, it: &mut Item, torznab: &mut bool) {
    for c in n.children().filter(Node::is_element) {
        let name = local(&c).to_ascii_lowercase();
        match name.as_str() {
            "attr" => {
                *torznab = true;
                let (Some(k), Some(v)) = (c.attribute("name"), c.attribute("value")) else { continue };
                match k.to_ascii_lowercase().as_str() {
                    "magneturl" => place(it, v, false),
                    "infohash" => it.info_hash = it.info_hash.take().or_else(|| normalize_hash(v)),
                    "size" => it.size = it.size.or_else(|| number(v)),
                    "seeders" => it.seeders = number(v).or(it.seeders),
                    "leechers" => it.leechers = number(v).or(it.leechers),
                    // Torznab's peers count the seeders too.
                    "peers" if it.leechers.is_none() => {
                        if let (Some(p), Some(s)) = (number::<u32>(v), it.seeders) {
                            it.leechers = Some(p.saturating_sub(s));
                        }
                    }
                    _ => {}
                }
            }
            "infohash" => it.info_hash = it.info_hash.take().or_else(|| text_of(c).and_then(|t| normalize_hash(&t))),
            "seeders" => it.seeders = it.seeders.or_else(|| text_of(c).and_then(|t| number(&t))),
            "leechers" => it.leechers = it.leechers.or_else(|| text_of(c).and_then(|t| number(&t))),
            "size" | "contentlength" => it.size = it.size.or_else(|| text_of(c).and_then(|t| parse_size(&t))),
            "magneturi" => {
                if let Some(t) = text_of(c) {
                    place(it, &t, false);
                }
            }
            _ => {}
        }
    }
}

fn finish(mut it: Item) -> Option<Item> {
    if it.title.is_empty() {
        return None;
    }
    if it.info_hash.is_none() {
        it.info_hash = it.magnet.as_deref().and_then(magnet_hash);
    }
    if it.guid.is_empty() {
        it.guid = it.page.clone().or_else(|| it.torrent.clone()).or_else(|| it.magnet.clone()).unwrap_or_else(|| it.title.clone());
    }
    Some(it)
}

fn rss_item(n: Node, torznab: &mut bool) -> Option<Item> {
    let mut it = Item { title: child_text(n, "title").unwrap_or_default(), ..Item::default() };
    // Enclosures first: Jackett's link is its enclosure, with no ".torrent" to tell.
    for e in n.children().filter(|c| c.is_element() && local(c) == "enclosure") {
        let Some(url) = e.attribute("url") else { continue };
        let kind = e.attribute("type").unwrap_or("").to_ascii_lowercase();
        place(&mut it, url, kind.contains("bittorrent"));
        if kind.contains("bittorrent") || safe_magnet(url).is_some() {
            it.size = it.size.or_else(|| e.attribute("length").and_then(number).filter(|l| *l > 0));
        }
    }
    if let Some(l) = child_text(n, "link")
        && it.torrent.as_deref() != Some(l.trim())
    {
        place(&mut it, &l, false);
    }
    if let Some(g) = child(n, "guid") {
        let t = text_of(g).unwrap_or_default();
        // A permalink guid is the page (nyaa's view page).
        if g.attribute("isPermaLink").is_none_or(|p| p != "false") && safe_http(&t).is_some() {
            place(&mut it, &t, false);
        }
        it.guid = t;
    }
    if it.page.is_none()
        && let Some(c) = child_text(n, "comments")
    {
        it.page = safe_http(&c);
    }
    extras(n, &mut it, torznab);
    it.published = child_text(n, "pubDate").or_else(|| child_text(n, "date")).and_then(|d| parse_rfc2822(&d).or_else(|| parse_rfc3339(&d)));
    finish(it)
}

fn atom_entry(n: Node, torznab: &mut bool) -> Option<Item> {
    let mut it = Item { title: child_text(n, "title").unwrap_or_default(), ..Item::default() };
    for l in n.children().filter(|c| c.is_element() && local(c) == "link") {
        let Some(href) = l.attribute("href") else { continue };
        let rel = l.attribute("rel").unwrap_or("alternate");
        let kind = l.attribute("type").unwrap_or("").to_ascii_lowercase();
        place(&mut it, href, rel == "enclosure" && (kind.contains("bittorrent") || kind.is_empty()));
        if rel == "enclosure" {
            it.size = it.size.or_else(|| l.attribute("length").and_then(number).filter(|l| *l > 0));
        }
    }
    it.guid = child_text(n, "id").unwrap_or_default();
    extras(n, &mut it, torznab);
    it.published = child_text(n, "published").or_else(|| child_text(n, "updated")).and_then(|d| parse_rfc3339(&d).or_else(|| parse_rfc2822(&d)));
    finish(it)
}

/// Reads a feed body. Items without a title are dropped; the rest keep the feed's order.
pub fn parse(body: &str) -> Result<Feed, FeedError> {
    if body.len() > MAX_BODY {
        return Err(FeedError::TooLarge);
    }
    let body = body.trim_start_matches('\u{feff}');
    // roxmltree refuses a DTD by default: no entity tricks.
    let doc = Document::parse(body).map_err(|e| FeedError::Xml(e.to_string()))?;
    let root = doc.root_element();
    let mut torznab = false;
    match local(&root).to_ascii_lowercase().as_str() {
        "error" => Err(FeedError::Indexer(root.attribute("description").unwrap_or("an error").chars().take(200).collect())),
        "feed" => {
            let items = root.children().filter(|c| c.is_element() && local(c) == "entry").filter_map(|e| atom_entry(e, &mut torznab)).collect();
            Ok(Feed { format: Format::Atom, title: child_text(root, "title"), ttl: None, items })
        }
        "rss" | "rdf" => {
            let channel = child(root, "channel");
            // RSS 1.0 keeps its items beside the channel.
            let is_item = |c: &Node| c.is_element() && local(c) == "item";
            let mut nodes: Vec<Node> = channel.map(|c| c.children().filter(is_item).collect()).unwrap_or_default();
            nodes.extend(root.children().filter(is_item));
            let items = nodes.into_iter().filter_map(|n| rss_item(n, &mut torznab)).collect();
            let title = channel.and_then(|c| child_text(c, "title"));
            let ttl = channel.and_then(|c| child_text(c, "ttl")).and_then(|t| number(&t)).filter(|t| *t > 0);
            Ok(Feed { format: if torznab { Format::Torznab } else { Format::Rss }, title, ttl, items })
        }
        _ => Err(FeedError::NotAFeed),
    }
}

/// Query keys whose values are secrets.
const SECRET_KEYS: [&str; 9] = ["apikey", "api_key", "api-key", "passkey", "key", "token", "auth", "authkey", "torrent_pass"];

/// The URL carries a secret: a user:password, or a key in its query.
pub fn has_secret(url: &str) -> bool {
    mask(url) != url
}

/// The URL as Kotora shows and logs it: the password and key values become `•••`.
/// `https://u:pw@x/rss?apikey=abc&q=1` → `https://u:•••@x/rss?apikey=•••&q=1`.
pub fn mask(url: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (format!("{s}://"), r),
        None => (String::new(), url),
    };
    let auth_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(auth_end);
    let authority = match authority.rsplit_once('@') {
        Some((user, host)) => match user.split_once(':') {
            Some((u, _)) => format!("{u}:•••@{host}"),
            None => format!("•••@{host}"),
        },
        None => authority.to_string(),
    };
    let (path, query) = match tail.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (tail, None),
    };
    let mut out = format!("{scheme}{authority}{path}");
    if let Some(q) = query {
        let (q, frag) = match q.split_once('#') {
            Some((q, f)) => (q, Some(f)),
            None => (q, None),
        };
        let parts: Vec<String> = q
            .split('&')
            .map(|kv| match kv.split_once('=') {
                Some((k, v)) if !v.is_empty() && SECRET_KEYS.contains(&k.to_ascii_lowercase().as_str()) => format!("{k}=•••"),
                _ => kv.to_string(),
            })
            .collect();
        out.push('?');
        out.push_str(&parts.join("&"));
        if let Some(f) = frag {
            out.push('#');
            out.push_str(f);
        }
    }
    out
}

/// What a pasted nyaa.si address becomes: the RSS form of it and a name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NyaaDefaults {
    pub url: String,
    pub name: String,
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(v) => {
                    out.push(v);
                    i += 2;
                }
                None => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// nyaa.si's search, as Taiga's default (`rss/torrent/search/address`).
pub const NYAA_SEARCH: &str = "https://nyaa.si/?page=rss&c=1_2&f=0&q=%title%";

/// The search address for a title: `%title%` replaced, URL-encoded. `None`: no `%title%`, or
/// not http(s).
pub fn search_url(template: &str, title: &str) -> Option<String> {
    let t = template.trim();
    if !t.contains("%title%") {
        return None;
    }
    let mut enc = String::new();
    for b in title.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => enc.push(b as char),
            b' ' => enc.push('+'),
            _ => enc.push_str(&format!("%{b:02X}")),
        }
    }
    safe_http(&t.replace("%title%", &enc))
}

/// The Anime category nyaa.si files English-translated releases under.
pub const NYAA_ANIME_ENGLISH: &str = "1_2";

/// A nyaa.si address (a search page or its RSS) as a feed: `page=rss` added, the category
/// "Anime - English-translated" when it names none, and a name from the search words or the
/// uploader ("nyaa.si: Lanterns 1080p"). Only reads the text: no request. Other sites: `None`.
pub fn nyaa_defaults(url: &str) -> Option<NyaaDefaults> {
    let u = url.trim();
    let rest = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://")).unwrap_or(u);
    let (host, tail) = rest.split_at(rest.find(['/', '?']).unwrap_or(rest.len()));
    let host = host.to_ascii_lowercase();
    if host != "nyaa.si" && host != "www.nyaa.si" {
        return None;
    }
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
    let query = query.split('#').next().unwrap_or("");
    let mut pairs: Vec<(String, String)> = query.split('&').filter(|p| !p.is_empty()).map(|p| p.split_once('=').map_or((p.to_string(), String::new()), |(k, v)| (k.to_string(), v.to_string()))).collect();
    pairs.retain(|(k, _)| k != "page");
    pairs.insert(0, ("page".into(), "rss".into()));
    if !pairs.iter().any(|(k, v)| k == "c" && !v.is_empty()) {
        pairs.retain(|(k, _)| k != "c");
        pairs.push(("c".into(), NYAA_ANIME_ENGLISH.into()));
    }
    let q = pairs.iter().find(|(k, _)| k == "q").map(|(_, v)| url_decode(v).trim().to_string()).filter(|v| !v.is_empty());
    let user = path.strip_prefix("/user/").map(|u| url_decode(u.trim_end_matches('/'))).filter(|u| !u.is_empty());
    let name = match (user, q) {
        (Some(u), Some(q)) => format!("nyaa.si: {u}, {q}"),
        (Some(u), None) => format!("nyaa.si: {u}"),
        (None, Some(q)) => format!("nyaa.si: {q}"),
        (None, None) => "nyaa.si".to_string(),
    };
    let path = if path.is_empty() { "/" } else { path };
    let query: Vec<String> = pairs.iter().map(|(k, v)| if v.is_empty() { k.clone() } else { format!("{k}={v}") }).collect();
    Some(NyaaDefaults { url: format!("https://nyaa.si{path}?{}", query.join("&")), name })
}

#[cfg(test)]
mod tests;
