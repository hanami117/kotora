//! The token file: `$XDG_DATA_HOME/kotora/tokens.json`, mode 0600, in a 0700 folder. Not encrypted.
//! Never in the database, never in logs: `Debug` hides the values. One token per service (`mal`,
//! `anilist`); saving or forgetting one keeps the other.
//!
//! Kotora for Android keeps the tokens elsewhere: a [`TokenVault`] the app supplies (Keystore-
//! wrapped files), through [`TokenStore::with_vault`]. Same API for the services either way.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Token {
    pub access_token: String,
    /// Empty for AniList (its tokens can't be refreshed: they last a year).
    #[serde(default)]
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Token(<hidden>, expires_at {})", self.expires_at)
    }
}

impl Token {
    /// Whether it should be refreshed before use (a minute of slack).
    pub fn is_stale(&self, now: i64) -> bool {
        self.expires_at - 60 <= now
    }
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mal: Option<Token>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    anilist: Option<Token>,
}

impl File {
    fn slot(&mut self, service: &str) -> Option<&mut Option<Token>> {
        match service {
            "mal" => Some(&mut self.mal),
            "anilist" => Some(&mut self.anilist),
            _ => None,
        }
    }
}

/// Somewhere else to keep the tokens than the file: one opaque blob per service (`mal`,
/// `anilist`), the token as JSON. The Android app encrypts it with a Keystore key.
pub trait TokenVault: Send + Sync {
    fn load(&self, service: &str) -> Option<Vec<u8>>;
    fn save(&self, service: &str, bytes: &[u8]) -> std::io::Result<()>;
    /// Forgetting a token that isn't there is fine.
    fn delete(&self, service: &str) -> std::io::Result<()>;
}

const SERVICES: [&str; 2] = ["mal", "anilist"];

#[derive(Clone)]
pub struct TokenStore {
    path: PathBuf,
    vault: Option<Arc<dyn TokenVault>>,
}

impl TokenStore {
    pub fn new(path: PathBuf) -> Self {
        TokenStore { path, vault: None }
    }

    /// The tokens in `vault` instead of a file; [`TokenStore::path`] is then empty.
    pub fn with_vault(vault: Arc<dyn TokenVault>) -> Self {
        TokenStore { path: PathBuf::new(), vault: Some(vault) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Option<File> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        serde_json::from_str::<File>(&text).ok()
    }

    /// The token of `service` (`mal`, `anilist`).
    pub fn load(&self, service: &str) -> Option<Token> {
        if let Some(v) = &self.vault {
            return serde_json::from_slice(&v.load(service)?).ok();
        }
        self.read()?.slot(service)?.clone()
    }

    pub fn load_mal(&self) -> Option<Token> {
        self.load("mal")
    }

    pub fn load_anilist(&self) -> Option<Token> {
        self.load("anilist")
    }

    /// Saves the token of `service`; the other service's token stays.
    pub fn save(&self, service: &str, t: &Token) -> std::io::Result<()> {
        if let Some(v) = &self.vault {
            if !SERVICES.contains(&service) {
                return Err(std::io::Error::other(format!("no token slot for {service}")));
            }
            return v.save(service, &serde_json::to_vec(t).map_err(std::io::Error::other)?);
        }
        let mut file = self.read().unwrap_or_default();
        *file.slot(service).ok_or_else(|| std::io::Error::other(format!("no token slot for {service}")))? = Some(t.clone());
        self.write(&file)
    }

    pub fn save_mal(&self, t: &Token) -> std::io::Result<()> {
        self.save("mal", t)
    }

    pub fn save_anilist(&self, t: &Token) -> std::io::Result<()> {
        self.save("anilist", t)
    }

    fn write(&self, file: &File) -> std::io::Result<()> {
        if file.mal.is_none() && file.anilist.is_none() {
            return self.delete();
        }
        if let Some(dir) = self.path.parent() {
            crate::paths::create_private_dir(dir)?;
        }
        let json = serde_json::to_vec_pretty(file).map_err(std::io::Error::other)?;
        // Write a temp file that is 0600 from the start, then rename over the old one.
        let tmp = self.path.with_extension("json.tmp");
        let _ = std::fs::remove_file(&tmp);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        let mut f = opts.open(&tmp)?;
        f.write_all(&json)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, &self.path)
    }

    /// Sign out of one service: its token goes, the other's stays (the file goes with the last).
    pub fn forget(&self, service: &str) -> std::io::Result<()> {
        if let Some(v) = &self.vault {
            return v.delete(service);
        }
        let Some(mut file) = self.read() else { return self.delete() };
        if let Some(slot) = file.slot(service) {
            *slot = None;
        }
        self.write(&file)
    }

    /// The whole file goes.
    pub fn delete(&self) -> std::io::Result<()> {
        if let Some(v) = &self.vault {
            return SERVICES.iter().try_for_each(|s| v.delete(s));
        }
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok() -> Token {
        Token { access_token: "fake-access".into(), refresh_token: "fake-refresh".into(), expires_at: 1000 }
    }

    #[test]
    fn saves_loads_deletes_with_private_modes() {
        let dir = std::env::temp_dir().join(format!("kotora-tok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = TokenStore::new(dir.join("data/tokens.json"));
        assert_eq!(store.load_mal(), None);
        store.save_mal(&tok()).unwrap();
        store.save_mal(&tok()).unwrap(); // overwrite works
        assert_eq!(store.load_mal(), Some(tok()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(store.path()), 0o600);
            assert_eq!(mode(store.path().parent().unwrap()), 0o700);
        }
        store.delete().unwrap();
        store.delete().unwrap();
        assert_eq!(store.load_mal(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_service_keeps_its_own_token() {
        let dir = std::env::temp_dir().join(format!("kotora-tok2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = TokenStore::new(dir.join("tokens.json"));
        let al = Token { access_token: "fake-anilist".into(), refresh_token: String::new(), expires_at: 2000 };
        store.save_mal(&tok()).unwrap();
        store.save_anilist(&al).unwrap();
        assert_eq!((store.load_mal(), store.load_anilist()), (Some(tok()), Some(al.clone())), "saving one keeps the other");
        store.forget("mal").unwrap();
        assert_eq!((store.load_mal(), store.load_anilist()), (None, Some(al)));
        store.forget("anilist").unwrap();
        assert!(!store.path().exists(), "the last one takes the file with it");
        // A file from 0.1 (MAL only, no AniList key) still reads.
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(store.path(), r#"{"mal":{"access_token":"a","refresh_token":"r","expires_at":5}}"#).unwrap();
        assert_eq!(store.load_mal().map(|t| t.expires_at), Some(5));
        assert_eq!(store.load_anilist(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A vault in memory, as the Android app's Keystore-backed one behaves.
    #[derive(Default)]
    struct MemVault(std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>);

    impl TokenVault for MemVault {
        fn load(&self, service: &str) -> Option<Vec<u8>> {
            self.0.lock().unwrap().get(service).cloned()
        }
        fn save(&self, service: &str, bytes: &[u8]) -> std::io::Result<()> {
            self.0.lock().unwrap().insert(service.into(), bytes.to_vec());
            Ok(())
        }
        fn delete(&self, service: &str) -> std::io::Result<()> {
            self.0.lock().unwrap().remove(service);
            Ok(())
        }
    }

    #[test]
    fn a_vault_keeps_each_service_apart_and_no_file_is_written() {
        let vault = Arc::new(MemVault::default());
        let store = TokenStore::with_vault(vault.clone());
        let al = Token { access_token: "fake-anilist".into(), refresh_token: String::new(), expires_at: 2000 };
        store.save_mal(&tok()).unwrap();
        store.save_anilist(&al).unwrap();
        assert_eq!((store.load_mal(), store.load_anilist()), (Some(tok()), Some(al.clone())));
        assert!(store.save("nope", &tok()).is_err());
        store.forget("mal").unwrap();
        assert_eq!((store.load_mal(), store.load_anilist()), (None, Some(al)));
        store.delete().unwrap();
        assert!(vault.0.lock().unwrap().is_empty());
        assert_eq!(store.path(), Path::new(""));
        // A blob that isn't a token (a wrong key, a damaged file) reads as signed out.
        vault.save("mal", b"garbage").unwrap();
        assert_eq!(store.load_mal(), None);
    }

    #[test]
    fn debug_hides_values_and_staleness() {
        assert!(!format!("{:?}", tok()).contains("fake-"));
        assert!(tok().is_stale(950));
        assert!(!tok().is_stale(900));
    }
}
