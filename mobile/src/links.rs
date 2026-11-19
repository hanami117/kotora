//! External links, as on the desktop: an entry's page on its service ("Open on MyAnimeList" in
//! details and the long-press menu), the user's profile page, and About's links (kotora.moe, the
//! source code, Ko-fi). The URLs are built here so both apps say exactly what the desktop says.

use super::*;

/// The website (desktop Settings → About → Links).
pub const SITE_URL: &str = "https://kotora.moe";
/// The source code (desktop Settings → About → Links, "Source code").
pub const SOURCE_URL: &str = "https://github.com/hanami117/kotora";
/// Where "Support on Ko-fi" goes (desktop `settings::SUPPORT_URL`).
pub const SUPPORT_URL: &str = "https://ko-fi.com/hanami117";

/// One "Open on {service}" link.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ServiceLink {
    pub service: Service,
    /// "Open on MyAnimeList".
    pub label: String,
    pub url: String,
}

/// "MyAnimeList" / "AniList".
#[uniffi::export]
pub fn service_title(service: Service) -> String {
    service.id().title().to_string()
}

/// An anime's (or manga's) page on the service's website.
#[uniffi::export]
pub fn media_url(service: Service, kind: MediaKind, id: u32) -> String {
    service.id().media_url(Kind::from(kind), id)
}

/// The user's profile page on the service.
#[uniffi::export]
pub fn profile_url(service: Service, user_name: String) -> String {
    service.id().profile_url(&user_name)
}

#[uniffi::export]
pub fn site_url() -> String {
    SITE_URL.into()
}

#[uniffi::export]
pub fn source_url() -> String {
    SOURCE_URL.into()
}

#[uniffi::export]
pub fn support_url() -> String {
    SUPPORT_URL.into()
}

/// One line of About → Licences: what, and under which licence.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct LicenceLine {
    pub name: String,
    pub licence: String,
}

/// About → Licences: every part the phone app ships, in the desktop's words (`settings.rs`).
/// `android`: the Android app's own parts (JNA, AndroidX) instead of iOS's none.
#[uniffi::export]
pub fn licence_lines(android: bool) -> Vec<LicenceLine> {
    let l = |name: &str, licence: &str| LicenceLine { name: name.into(), licence: licence.into() };
    let mut out = vec![
        l("Kotora", "GPL-3.0-or-later"),
        l("Inter (the font)", "SIL Open Font License 1.1"),
        l("\u{5c0f}\u{864e}, drawn from Noto Sans JP", "SIL Open Font License 1.1"),
        l("UniFFI (Rust to the app)", "MPL-2.0"),
        l("SQLite (through rusqlite)", "Public domain (rusqlite: MIT)"),
        l("ureq, rustls, serde and the other Rust crates", "MIT or Apache-2.0"),
        l("Volume data for \u{201c}Fill volumes from chapters\u{201d}", "MangaDex"),
        l("Release data for Publishing", "MangaUpdates and MangaDex"),
    ];
    if android {
        out.push(l("JNA (Java to Rust)", "Apache-2.0 or LGPL-2.1-or-later"));
        out.push(l("AndroidX, Jetpack Compose, Kotlin", "Apache-2.0"));
    }
    out.push(l("Not affiliated with MyAnimeList, AniList, MangaDex or MangaUpdates.", ""));
    out
}

/// The welcome screen's small print (the desktop's `setup.rs`).
#[uniffi::export]
pub fn welcome_note() -> String {
    "Kotora is a tribute to Taiga, the Windows anime tracker. It is not affiliated with MyAnimeList or AniList.".into()
}

#[uniffi::export]
impl Kotora {
    /// "Open on …" for an entry of the active service: its page there, and with the mirror on
    /// the mirror service's page too when its id is known (`mirror.rs`).
    pub fn service_links(&self, kind: MediaKind, id: u32) -> Vec<ServiceLink> {
        let Some(service) = self.active() else { return Vec::new() };
        let mut out = vec![ServiceLink { service, label: format!("Open on {}", service.id().title()), url: media_url(service, kind, id) }];
        out.extend(self.mirror_service_link(kind, id));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_match_the_desktop() {
        assert_eq!(media_url(Service::Mal, MediaKind::Anime, 61), "https://myanimelist.net/anime/61");
        assert_eq!(media_url(Service::AniList, MediaKind::Manga, 99), "https://anilist.co/manga/99");
        assert_eq!(profile_url(Service::Mal, "hana".into()), "https://myanimelist.net/profile/hana");
        assert_eq!(profile_url(Service::AniList, "hana".into()), "https://anilist.co/user/hana");
        assert_eq!(service_title(Service::AniList), "AniList");
        assert!(licence_lines(true).len() > licence_lines(false).len());
        assert!(welcome_note().starts_with("Kotora is a tribute to Taiga"));
        assert_eq!((site_url().as_str(), source_url().as_str(), support_url().as_str()), ("https://kotora.moe", "https://github.com/hanami117/kotora", "https://ko-fi.com/hanami117"));
    }
}
