//! The token file: `$XDG_DATA_HOME/kotora/tokens.json`, mode 0600, in a 0700 folder. Not encrypted
//! (DESIGN.md §11). Never in the database, never in logs: `Debug` hides the values.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Token {
    pub access_token: String,
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
}

pub struct TokenStore {
    path: PathBuf,
}

impl TokenStore {
    pub fn new(path: PathBuf) -> Self {
        TokenStore { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_mal(&self) -> Option<Token> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        serde_json::from_str::<File>(&text).ok()?.mal
    }

    pub fn save_mal(&self, t: &Token) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            crate::paths::create_private_dir(dir)?;
        }
        let json = serde_json::to_vec_pretty(&File { mal: Some(t.clone()) }).map_err(std::io::Error::other)?;
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

    /// Sign out: the file goes away.
    pub fn delete(&self) -> std::io::Result<()> {
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
    fn debug_hides_values_and_staleness() {
        assert!(!format!("{:?}", tok()).contains("fake-"));
        assert!(tok().is_stale(950));
        assert!(!tok().is_stale(900));
    }
}
