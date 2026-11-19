// Portions ported from Taiga (https://github.com/erengy/taiga), Copyright (C) Eren Okka and
// contributors, GPL-3.0: src/track/feed_aggregator.cpp (HandleFeedDownload: the torrent file
// name, the download folder with a folder per anime, the torrent application and its arguments).

//! Taiga's download options: open with the system's app or one the user picks, or save the
//! .torrent file into a folder (a folder per anime). Kotora never downloads video: these only
//! place a .torrent file or hand a magnet/file to the user's torrent app.

use std::path::{Path, PathBuf};

/// Taiga's `ValidateFileName`: no characters a file system refuses, no trailing dots or spaces.
pub fn valid_file_name(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let out = out.trim().trim_end_matches(['.', ' ']).to_string();
    let out: String = out.chars().take(200).collect();
    if out.is_empty() {
        "torrent".into()
    } else {
        out
    }
}

/// Where a .torrent file goes when saved into `folder`: `folder/<anime>/<title>.torrent`
/// (the anime folder only with `subfolders` and a known anime).
pub fn saved_path(folder: &Path, subfolders: bool, anime: Option<&str>, title: &str) -> PathBuf {
    let mut p = folder.to_path_buf();
    if subfolders && let Some(a) = anime.filter(|a| !a.trim().is_empty()) {
        p.push(valid_file_name(a));
    }
    let name = valid_file_name(title);
    let name = if name.to_ascii_lowercase().ends_with(".torrent") {
        name
    } else {
        format!("{name}.torrent")
    };
    p.push(name);
    p
}

/// Copies a fetched .torrent file into the download folder (made when missing).
pub fn save_into(
    file: &Path,
    folder: &Path,
    subfolders: bool,
    anime: Option<&str>,
    title: &str,
) -> Result<PathBuf, String> {
    if folder.as_os_str().is_empty() || !folder.is_absolute() {
        return Err("Choose a download folder in Settings \u{2192} Feeds.".into());
    }
    let to = saved_path(folder, subfolders, anime, title);
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Can't make the folder: {e}"))?;
    }
    std::fs::copy(file, &to).map_err(|e| format!("Can't save the .torrent file: {e}"))?;
    Ok(to)
}

/// The command that opens `target` (a magnet or a file) in the app at `app`: an argument vector,
/// never a shell line. macOS app bundles go through `open -a`.
pub fn app_command(app: &str, target: &str) -> Option<(String, Vec<String>)> {
    let app = app.trim();
    if app.is_empty() {
        return None;
    }
    if cfg!(target_os = "macos") && app.trim_end_matches('/').ends_with(".app") {
        return Some((
            "open".into(),
            vec!["-a".into(), app.to_string(), target.to_string()],
        ));
    }
    Some((app.to_string(), vec![target.to_string()]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_and_folders() {
        assert_eq!(
            valid_file_name("[Hoshi] A/B: C? - 01 [1080p]."),
            "[Hoshi] A_B_ C_ - 01 [1080p]"
        );
        assert_eq!(valid_file_name("  "), "torrent");
        let p = saved_path(Path::new("/dl"), true, Some("Show: Two"), "[G] Show - 02");
        assert_eq!(p, PathBuf::from("/dl/Show_ Two/[G] Show - 02.torrent"));
        assert_eq!(
            saved_path(Path::new("/dl"), false, Some("Show"), "x.torrent"),
            PathBuf::from("/dl/x.torrent")
        );
        assert_eq!(
            saved_path(Path::new("/dl"), true, None, "x"),
            PathBuf::from("/dl/x.torrent")
        );
    }

    #[test]
    fn saving_needs_an_absolute_folder() {
        let dir = std::env::temp_dir().join(format!("kotora-dl-{}", std::process::id()));
        let src = dir.join("src.torrent");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&src, b"d4:infoe").unwrap();
        assert!(save_into(&src, Path::new("relative"), true, None, "x").is_err());
        let to = save_into(&src, &dir.join("out"), true, Some("Show"), "[G] Show - 01").unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"d4:infoe");
        assert!(to.ends_with("Show/[G] Show - 01.torrent"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn app_commands_are_argument_vectors() {
        assert_eq!(app_command(" ", "magnet:?x"), None);
        let (p, a) = app_command("/usr/bin/qbittorrent", "magnet:?xt=urn:btih:a b").unwrap();
        assert_eq!(
            (p.as_str(), a),
            (
                "/usr/bin/qbittorrent",
                vec!["magnet:?xt=urn:btih:a b".to_string()]
            )
        );
        if cfg!(target_os = "macos") {
            assert_eq!(
                app_command("/Applications/Transmission.app", "/t.torrent")
                    .unwrap()
                    .1,
                vec!["-a", "/Applications/Transmission.app", "/t.torrent"]
            );
        }
    }
}
