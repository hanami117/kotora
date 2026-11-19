//! Feed parsing tests. Every feed here is a made-up fixture: no test ever fetches a real feed.

use super::*;

fn fixture(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/feeds/fixtures").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn nyaa_style_rss() {
    let f = parse(&fixture("nyaa.xml")).unwrap();
    assert_eq!((f.format, f.ttl, f.items.len()), (Format::Rss, Some(15), 3));
    assert_eq!(f.title.as_deref(), Some("Example Tracker - Lanterns - Torrent File RSS"));
    let a = &f.items[0];
    assert_eq!(a.title, "[Hoshi] Lanterns of Kirigaoka - 08 [1080p][A1B2C3D4].mkv");
    assert_eq!(a.torrent.as_deref(), Some("https://tracker.example/download/1000008.torrent"));
    assert_eq!(a.page.as_deref(), Some("https://tracker.example/view/1000008"));
    assert_eq!(a.info_hash.as_deref(), Some("0123456789abcdef0123456789abcdef01234567"));
    assert_eq!((a.seeders, a.leechers), (Some(412), Some(37)));
    assert_eq!(a.size, Some((1.4 * 1024f64.powi(3)).round() as u64));
    assert_eq!(a.published, crate::time::parse_rfc3339("2026-11-17T15:31:02Z"));
    assert_eq!(a.key(), "btih:0123456789abcdef0123456789abcdef01234567");
    // CDATA and entities.
    assert!(f.items[1].title.ends_with("[Batch]"));
    assert_eq!(f.items[2].title, "[Tsuki] Starfall Delivery Service S2 - 05 [720p] & Extras");
    assert_eq!((f.items[2].info_hash.as_deref(), f.items[2].key().as_str()), (None, "guid:https://tracker.example/view/1000010"));
    assert_eq!(f.items[2].size, Some((350.5 * 1024.0 * 1024.0f64).round() as u64));
}

#[test]
fn atom() {
    let f = parse(&fixture("atom.xml")).unwrap();
    assert_eq!((f.format, f.title.as_deref(), f.items.len()), (Format::Atom, Some("Hoshi releases"), 2), "the empty title is dropped");
    let a = &f.items[0];
    assert_eq!((a.guid.as_str(), a.page.as_deref()), ("urn:example:hoshi:4412", Some("https://releases.example/r/4412")));
    assert_eq!((a.torrent.as_deref(), a.size), (Some("https://releases.example/t/4412.torrent"), Some(1_288_490_188)));
    assert_eq!(a.published, crate::time::parse_rfc3339("2026-11-17T15:00:00Z"));
    // Base32 magnet hash, read as hex.
    let b = &f.items[1];
    assert!(b.magnet.as_deref().unwrap().starts_with("magnet:?xt=urn:btih:CIRS"));
    assert_eq!(b.info_hash.as_deref(), Some("12232b6789abcde1024226789abcde1024226789"));
    assert_eq!(b.published, crate::time::parse_rfc3339("2026-11-16T03:00:00Z"));
}

#[test]
fn torznab() {
    let f = parse(&fixture("torznab.xml")).unwrap();
    assert_eq!((f.format, f.items.len()), (Format::Torznab, 1));
    let a = &f.items[0];
    assert_eq!(a.info_hash.as_deref(), Some("00112233445566778899aabbccddeeff00112233"));
    assert_eq!((a.seeders, a.leechers, a.size), (Some(120), Some(15), Some(1_503_238_553)));
    assert!(a.magnet.as_deref().unwrap().starts_with("magnet:?xt=urn:btih:0011"));
    assert_eq!(a.page.as_deref(), Some("https://indexer.example/torrent/55501"));
    // The indexer's download link carries the key: kept to open, masked to show.
    let t = a.torrent.as_deref().unwrap();
    assert!(t.contains("SECRETKEY0000"));
    assert!(has_secret(t));
    assert_eq!(mask(t), "http://127.0.0.1:9117/dl/example/?apikey=•••&path=abc");
}

#[test]
fn indexer_error_and_bad_bodies() {
    assert_eq!(parse(&fixture("indexer_error.xml")), Err(FeedError::Indexer("Invalid API Key".into())));
    assert!(matches!(parse(&fixture("broken.xml")), Err(FeedError::Xml(_))));
    assert!(matches!(parse(&fixture("dtd.xml")), Err(FeedError::Xml(_))), "a DTD is never read");
    assert_eq!(parse("<html><body>hi</body></html>"), Err(FeedError::NotAFeed));
    let big = format!("<rss><channel>{}</channel></rss>", " ".repeat(MAX_BODY));
    assert_eq!(parse(&big), Err(FeedError::TooLarge));
    assert_eq!(FeedError::NotAFeed.to_string(), "this address gives no RSS or Atom feed");
}

#[test]
fn hostile_links_are_dropped() {
    let f = parse(&fixture("hostile.xml")).unwrap();
    assert_eq!(f.items.len(), 4);
    for it in &f.items[..2] {
        assert_eq!((&it.page, &it.magnet, &it.torrent), (&None, &None, &None), "{}", it.title);
    }
    assert_eq!(f.items[2].torrent.as_deref(), Some("https://ok.example/download/6.torrent"));
    // No link: the title is the guid.
    assert_eq!(f.items[3].guid, "No links at all");
    for bad in ["file:///x", "javascript:alert(1)", "itms-apps://x", "https://", "https://a b/", "magnet:?dn=x", "magnet:?xt=urn:btih:zz"] {
        assert!(safe_http(bad).is_none() && safe_magnet(bad).is_none(), "{bad}");
    }
    assert!(safe_magnet("MAGNET:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567").is_some());
}

#[test]
fn magnet_only_enclosure_only_rss1() {
    let f = parse(&fixture("magnet_only.xml")).unwrap();
    let a = &f.items[0];
    assert_eq!((a.torrent.as_deref(), a.page.as_deref()), (None, None));
    assert_eq!(a.info_hash.as_deref(), Some("aaaabbbbccccddddeeeeffff0000111122223333"));
    assert_eq!(a.guid, a.magnet.clone().unwrap());

    let f = parse(&fixture("enclosure_only.xml")).unwrap();
    let a = &f.items[0];
    assert_eq!((a.torrent.as_deref(), a.size), (Some("https://files.example/get?id=7"), Some(734_003_200)));
    assert_eq!(a.published, crate::time::parse_rfc3339("2026-11-18T01:02:03Z"));

    let f = parse(&fixture("rss1.xml")).unwrap();
    assert_eq!((f.format, f.title.as_deref(), f.items.len()), (Format::Rss, Some("Old style"), 1));
    assert_eq!(f.items[0].torrent.as_deref(), Some("https://old.example/download/2.torrent"));
    assert_eq!(f.items[0].published, crate::time::parse_rfc3339("2026-11-10T08:00:00Z"));
}

#[test]
fn every_fixture_title_reads_as_a_release() {
    // The feed's titles go through the release parser: the series comes back.
    let f = parse(&fixture("nyaa.xml")).unwrap();
    let r: Vec<_> = f.items.iter().map(|i| crate::recognize::parse_release(&i.title)).collect();
    assert_eq!((r[0].parsed.title.as_str(), r[0].episodes), ("Lanterns of Kirigaoka", Some((8, 8))));
    assert_eq!((r[1].episodes, r[1].batch), (Some((1, 12)), true));
    assert_eq!((r[2].parsed.season, r[2].parsed.episode), (Some(2), Some(5)));
}

#[test]
fn sizes_and_hashes() {
    assert_eq!(parse_size("700MB"), Some(700_000_000));
    assert_eq!(parse_size("1,024 KiB"), Some(1_048_576));
    assert_eq!(parse_size("123"), Some(123));
    assert_eq!(parse_size("lots"), None);
    assert_eq!(normalize_hash("not a hash"), None);
    assert_eq!(magnet_hash("magnet:?dn=x&xt=urn:btih:0123456789ABCDEF0123456789ABCDEF01234567").as_deref(), Some("0123456789abcdef0123456789abcdef01234567"));
}

#[test]
fn masking() {
    assert_eq!(mask("https://u:pw@x.example/rss?passkey=abc&q=1#f"), "https://u:•••@x.example/rss?passkey=•••&q=1#f");
    assert_eq!(mask("https://token@x.example/rss"), "https://•••@x.example/rss");
    assert_eq!(mask("https://x.example/rss?q=lanterns&c=1_2"), "https://x.example/rss?q=lanterns&c=1_2");
    assert!(!has_secret("https://x.example/rss?q=key"));
    assert!(has_secret("https://x.example/rss?ApiKey=1"));
}

#[test]
fn nyaa_addresses_get_defaults() {
    let d = nyaa_defaults("https://nyaa.si/?f=0&c=0_0&q=lanterns+of+kirigaoka+1080p").unwrap();
    assert_eq!(d.name, "nyaa.si: lanterns of kirigaoka 1080p");
    assert_eq!(d.url, "https://nyaa.si/?page=rss&f=0&c=0_0&q=lanterns+of+kirigaoka+1080p");
    let d = nyaa_defaults("nyaa.si/?q=Hoshi%20Lanterns").unwrap();
    assert_eq!((d.name.as_str(), d.url.as_str()), ("nyaa.si: Hoshi Lanterns", "https://nyaa.si/?page=rss&q=Hoshi%20Lanterns&c=1_2"));
    let d = nyaa_defaults("https://nyaa.si/user/Hoshi?page=rss").unwrap();
    assert_eq!((d.name.as_str(), d.url.as_str()), ("nyaa.si: Hoshi", "https://nyaa.si/user/Hoshi?page=rss&c=1_2"));
    assert_eq!(nyaa_defaults("https://nyaa.si").unwrap().url, "https://nyaa.si/?page=rss&c=1_2");
    assert_eq!(nyaa_defaults("https://nyaa.si/?q=%E3%81%82%").unwrap().name, "nyaa.si: あ%");
    assert!(nyaa_defaults("https://tracker.example/?page=rss").is_none());
    assert!(nyaa_defaults("https://nyaa.si.example/").is_none());
}

#[test]
fn search_addresses() {
    assert_eq!(search_url(NYAA_SEARCH, "Black Clover").as_deref(), Some("https://nyaa.si/?page=rss&c=1_2&f=0&q=Black+Clover"));
    assert_eq!(search_url(NYAA_SEARCH, "Re:Bloom & Co").as_deref(), Some("https://nyaa.si/?page=rss&c=1_2&f=0&q=Re%3ABloom+%26+Co"));
    assert_eq!(search_url("https://x.example/rss", "a"), None, "no %title%");
    assert_eq!(search_url("file:///%title%", "a"), None);
}
