//! Settings the desktop has and `insights` doesn't (yet): the title language (`[display] titles`)
//! and Publishing's release sources (`[release_data]`). Same keys and rules as the desktop's
//! settings page; kept apart so `insights.rs` and `pages.rs` stay as the iOS branch has them.

use kotora_core::config::{self, Config, TitleLang};
use kotora_core::{Scope, paths};

use std::sync::atomic::{AtomicU8, Ordering};

use crate::{Kotora, KotoraError, MediaKind, other};

/// The title language, read once from `config.toml` and kept up to date by
/// [`Kotora::set_title_language`] (rows ask for it one by one). `u8::MAX`: not read yet.
static TITLES: AtomicU8 = AtomicU8::new(u8::MAX);

/// The title language the rows use: the desktop's `[display] titles`.
pub(crate) fn titles() -> TitleLang {
    let n = TITLES.load(Ordering::Relaxed);
    let l = TitleLang::ALL.get(usize::from(n)).copied().unwrap_or_else(|| load().display.titles);
    if n == u8::MAX {
        remember(l);
    }
    l
}

fn remember(l: TitleLang) {
    let i = TitleLang::ALL.iter().position(|x| *x == l).unwrap_or(0);
    TITLES.store(u8::try_from(i).unwrap_or(0), Ordering::Relaxed);
}

/// Settings → Updates → "Keep the higher progress in conflicts" (`[updates]
/// keep_higher_progress`, on by default as on the desktop): read by every queue run.
pub(crate) fn keep_higher() -> bool {
    load().updates.keep_higher_progress
}

/// Settings → Release data: at least one stays on.
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseSources {
    pub mangaupdates: bool,
    pub mangadex: bool,
}

/// A title in the chosen language.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ShownTitle {
    pub media_id: u32,
    pub title: String,
}

fn load() -> Config {
    Config::load(&paths::config_file()).unwrap_or_default()
}

fn write(section: &str, key: &str, value: &str) -> Result<(), KotoraError> {
    config::write_setting(&paths::config_file(), section, key, value).map_err(|e| other(format!("Can\u{2019}t save the setting: {e}")))
}

#[uniffi::export]
impl Kotora {
    /// `romaji`, `english` or `japanese`.
    pub fn title_language(&self) -> String {
        load().display.titles.key().to_string()
    }

    pub fn set_title_language(&self, value: String) -> Result<(), KotoraError> {
        let l = TitleLang::ALL.into_iter().find(|l| l.key() == value).ok_or_else(|| other(format!("{value:?} is not a title language")))?;
        write("display", "titles", &config::quote(l.key()))?;
        remember(l);
        Ok(())
    }

    /// "Keep the higher progress in conflicts": a progress-only conflict where this phone's
    /// count is higher sends it instead of asking (the desktop's rule and default).
    pub fn keep_higher_progress(&self) -> bool {
        keep_higher()
    }

    pub fn set_keep_higher_progress(&self, on: bool) -> Result<(), KotoraError> {
        write("updates", "keep_higher_progress", &on.to_string())
    }

    /// The active list's titles in the chosen language (romaji when that one is missing, as on
    /// the desktop). Local. Romaji: empty. (Rows already carry the chosen title.)
    pub fn shown_titles(&self, kind: MediaKind) -> Result<Vec<ShownTitle>, KotoraError> {
        let lang = load().display.titles;
        if lang == TitleLang::Romaji {
            return Ok(Vec::new());
        }
        let service = self.active_or_err()?;
        let list = self.store().load_list(Scope::new(service.name(), kind.into()))?;
        Ok(list.iter().map(|it| ShownTitle { media_id: it.media.id, title: it.media.display_title(lang).to_string() }).filter(|t| !t.title.is_empty()).collect())
    }

    pub fn release_sources(&self) -> ReleaseSources {
        let r = load().release_data;
        if !r.mangaupdates && !r.mangadex { ReleaseSources { mangaupdates: true, mangadex: true } } else { ReleaseSources { mangaupdates: r.mangaupdates, mangadex: r.mangadex } }
    }

    /// Saves both switches; turning both off is refused (the desktop keeps one on).
    pub fn set_release_sources(&self, sources: ReleaseSources) -> Result<(), KotoraError> {
        if !sources.mangaupdates && !sources.mangadex {
            return Err(other("At least one release source stays on."));
        }
        write("release_data", "mangaupdates", &sources.mangaupdates.to_string())?;
        write("release_data", "mangadex", &sources.mangadex.to_string())
    }
}

#[cfg(test)]
mod tests {
    use kotora_core::config::Config;

    #[test]
    fn both_sources_off_reads_as_both_on() {
        let c = Config::parse("[release_data]\nmangaupdates = false\nmangadex = false\n").unwrap();
        assert!(c.release_data.mangaupdates && c.release_data.mangadex);
    }
}
