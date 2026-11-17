//! A panic in a copy with no terminal (a `--hidden` or login start) would leave no trace: its
//! message goes to stderr, which nobody reads there. This keeps the usual message and also
//! appends it to `<state>/kotora/crash.log` (`~/.local/state/kotora`, on macOS too). Windows
//! has no need: its stderr is `kotora.log` already when there is no console (`win::attach_output`).

use std::io::Write;
use std::path::Path;

/// Past this size the file starts again (one panic is a few hundred bytes).
const MAX: u64 = 256 * 1024;

/// Installs the hook. Not for headless runs (snaps, tests): they have a terminal.
pub fn install() {
    if cfg!(windows) {
        return;
    }
    let Some(dir) = kotora_core::paths::state_dir() else { return };
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let text = format!("--- kotora {} · unix time {} · thread {thread}\n{info}\n", kotora_ctl::version_line(), kotora_core::time::now_unix());
        let _ = append(&dir, &text);
    }));
}

/// Appends `text` to `dir/crash.log` (the folder made private if it is new).
fn append(dir: &Path, text: &str) -> std::io::Result<()> {
    if !dir.exists() {
        kotora_core::paths::create_private_dir(dir)?;
    }
    let path = dir.join("crash.log");
    let big = std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX);
    let mut o = std::fs::OpenOptions::new();
    o.create(true);
    if big { o.write(true).truncate(true) } else { o.append(true) };
    o.open(&path)?.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panics_are_appended_and_the_file_stays_small() {
        let dir = std::env::temp_dir().join(format!("kotora-crashlog-{}", std::process::id())).join("kotora");
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
        append(&dir, "one\n").unwrap();
        append(&dir, "two\n").unwrap();
        let path = dir.join("crash.log");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700, "a new folder is private");
        }
        std::fs::write(&path, vec![b'x'; MAX as usize + 1]).unwrap();
        append(&dir, "three\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "three\n", "past the limit it starts again");
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
