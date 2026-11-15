//! `config.toml`: the settings the user edits. Only `[mal] client_id` so far.
//!
//! The MAL Client ID is public by design but still never goes into the repo, tests or logs, so
//! `Debug` hides it. It is read from the user's file only (no build-time default yet).

use std::path::Path;

use serde::Deserialize;

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub mal_client_id: Option<String>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config").field("mal_client_id", &self.mal_client_id.as_ref().map(|_| "<set>")).finish()
    }
}

#[derive(Deserialize, Default)]
struct File {
    #[serde(default)]
    mal: Mal,
}

#[derive(Deserialize, Default)]
struct Mal {
    client_id: Option<String>,
}

impl Config {
    /// Parses the text of a `config.toml`. A blank Client ID counts as none.
    pub fn parse(text: &str) -> Result<Config, String> {
        let f: File = toml::from_str(text).map_err(|e| format!("config.toml: {}", e.message()))?;
        Ok(Config { mal_client_id: f.mal.client_id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) })
    }

    /// Reads the file. A missing file is an empty config; a broken one is an error message.
    pub fn load(path: &Path) -> Result<Config, String> {
        match std::fs::read_to_string(path) {
            Ok(t) => Config::parse(&t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("can't read config.toml: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_client_id() {
        let c = Config::parse("[mal]\nclient_id = \"  fake-id  \"\n").unwrap();
        assert_eq!(c.mal_client_id.as_deref(), Some("fake-id"));
    }

    #[test]
    fn missing_or_blank_means_none() {
        assert_eq!(Config::parse("").unwrap().mal_client_id, None);
        assert_eq!(Config::parse("[mal]\nclient_id = \" \"").unwrap().mal_client_id, None);
        assert_eq!(Config::load(Path::new("/nonexistent/kotora/config.toml")).unwrap(), Config::default());
    }

    #[test]
    fn broken_file_is_an_error_and_debug_hides_the_id() {
        assert!(Config::parse("[mal\n").is_err());
        let c = Config { mal_client_id: Some("fake-secret-looking-id".into()) };
        assert!(!format!("{c:?}").contains("fake-secret"));
    }
}
