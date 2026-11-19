//! Cover files: downloaded once from the service's image server and kept on disk
//! (`$XDG_CACHE_HOME/kotora/covers`), so later starts and offline starts read them from there.
//! Blocking: call these from worker threads only. Decoding is the app's job (`app/src/art.rs`).
//!
//! The MAL API licence asks to keep data only as long as needed: the folder has a size budget
//! (oldest files go first) and is deleted on sign out (`clear`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// The largest cover body accepted (MAL's are 20-60 KB).
const MAX_BODY: u64 = 2 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);
/// Disk budget of the cover folder; `prune` trims it to this.
pub const DISK_BUDGET: u64 = 64 * 1024 * 1024;

/// `$XDG_CACHE_HOME/kotora/covers`.
pub fn dir() -> PathBuf {
    crate::paths::cache_dir().join("covers")
}

/// One HTTP client for the cover workers (connections are reused).
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .build()
        .into()
}

/// The file a cover URL is kept in: a stable FNV-1a hash of the URL.
pub fn path_for(dir: &Path, url: &str) -> PathBuf {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    dir.join(format!("{h:016x}"))
}

/// The cover's bytes: from disk, else downloaded (when `online`) and saved. Only `https` URLs.
pub fn fetch(agent: &ureq::Agent, dir: &Path, url: &str, online: bool) -> Result<Vec<u8>, String> {
    let path = path_for(dir, url);
    if let Ok(bytes) = std::fs::read(&path)
        && !bytes.is_empty()
    {
        // Touched, so `prune` (oldest first) keeps what is in use.
        if let Ok(f) = std::fs::File::options().write(true).open(&path) {
            let _ = f.set_modified(std::time::SystemTime::now());
        }
        return Ok(bytes);
    }
    if !online {
        return Err("offline and not on disk".into());
    }
    if !url.starts_with("https://") {
        return Err("not an https URL".into());
    }
    let resp = agent.get(url).call().map_err(|e| e.to_string())?;
    let bytes = resp
        .into_body()
        .with_config()
        .limit(MAX_BODY)
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Err("empty body".into());
    }
    if let Err(e) = write_atomic(&path, &bytes) {
        eprintln!("kotora: can't save a cover: {e}");
    }
    Ok(bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        crate::paths::create_private_dir(dir)?;
    }
    // Its own temp name per write: two workers may save the same cover at once.
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let tmp = path.with_extension(format!(
        "tmp{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Deletes the oldest files until the folder fits `budget` bytes.
pub fn prune(dir: &Path, budget: u64) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then(|| {
                (
                    m.modified().unwrap_or(std::time::UNIX_EPOCH),
                    m.len(),
                    e.path(),
                )
            })
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort_by_key(|f| f.0);
    for (_, len, path) in files {
        if total <= budget {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

/// Sign out: every saved cover goes.
pub fn clear(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kotora-covers-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn reads_from_disk_and_never_goes_online_when_told() {
        let dir = scratch("disk");
        let url = "https://cdn.example.invalid/1001m.jpg";
        let agent = agent();
        assert!(fetch(&agent, &dir, url, false).is_err(), "offline miss");
        write_atomic(&path_for(&dir, url), b"jpeg bytes").unwrap();
        assert_eq!(fetch(&agent, &dir, url, false).unwrap(), b"jpeg bytes");
        assert!(
            fetch(&agent, &dir, "http://cdn.example.invalid/x.jpg", true).is_err(),
            "https only"
        );
        clear(&dir);
        assert!(!dir.exists());
    }

    #[test]
    fn names_are_stable_and_distinct() {
        let d = Path::new("/c");
        assert_eq!(
            path_for(d, "https://a/1.jpg"),
            path_for(d, "https://a/1.jpg")
        );
        assert_ne!(
            path_for(d, "https://a/1.jpg"),
            path_for(d, "https://a/2.jpg")
        );
    }

    #[test]
    fn prune_drops_the_oldest_first() {
        let dir = scratch("prune");
        for (i, name) in ["old", "mid", "new"].iter().enumerate() {
            let p = dir.join(name);
            write_atomic(&p, &[0u8; 100]).unwrap();
            let t = std::time::UNIX_EPOCH + Duration::from_secs(1_000 + i as u64);
            std::fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(t)
                .unwrap();
        }
        prune(&dir, 200);
        assert!(!dir.join("old").exists() && dir.join("mid").exists() && dir.join("new").exists());
        clear(&dir);
    }
}
