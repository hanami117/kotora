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

#[uniffi::export]
impl Kotora {
    /// "Open on …" for an entry of the active service: its page there. (The desktop adds the
    /// mirror service's page when the mirror is on; phones have no mirror yet.)
    pub fn service_links(&self, kind: MediaKind, id: u32) -> Vec<ServiceLink> {
        let Some(service) = self.active() else { return Vec::new() };
        vec![ServiceLink { service, label: format!("Open on {}", service.id().title()), url: media_url(service, kind, id) }]
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
        assert_eq!((site_url().as_str(), source_url().as_str(), support_url().as_str()), ("https://kotora.moe", "https://github.com/hanami117/kotora", "https://ko-fi.com/hanami117"));
    }
}
