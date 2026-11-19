// Portions ported from Taiga (https://github.com/erengy/taiga), Copyright (C) Eren Okka and
// contributors, GPL-3.0: src/ui/dlg/dlg_torrent.cpp (the Torrents page: its columns, check
// states, Check new torrents, Download marked torrents, Discard, Select, the context actions),
// src/ui/dlg/dlg_feed_filter.cpp and dlg_settings_page.cpp (the feed filter manager: presets,
// conditions, actions, options, anime, import and export; the torrent settings: auto-check, new
// episode action, the torrent application, the download folder, the download order) and
// src/track/feed_aggregator.cpp (Download, the archive).

//! Taiga's torrent tab for Kotora: the Torrents page (every feed item, its anime, episode,
//! group, size, video, seeders/leechers and description, checked when selected, dimmed when
//! discarded), the filter manager in Settings → Feeds, per-anime filters, the torrent history
//! and the download options. The engine is `kotora_core::feeds::taiga`.

use std::collections::HashSet;
use std::path::Path;

use kotora_core::config::{self, Feeds};
use kotora_core::feeds::download::{app_command, save_into};
use kotora_core::feeds::poll::download_target;
use kotora_core::feeds::taiga::{self, Action, Category, Condition, Element, Entry, Filter, FilterOption, ItemState, Match, Operator};
use kotora_core::feeds::{Item, mask};
use kotora_core::paths;
use rune::{El, ScrollSpec, TextEdit, TextStyle, Tx, col, el, row, spacer, text, text_input};

use crate::feeds::{ep_text, is_new, size_text, small};
use crate::model::State;
use crate::theme::{self, PAL};
use crate::ui::{card, field_hint};

/// Taiga's filters file, beside config.toml (Taiga's own `<filter>` XML).
pub fn filters_file() -> std::path::PathBuf {
    paths::config_dir().join("feed_filters.xml")
}

/// The filters at start: the file, else none (Taiga's defaults are offered, not forced). The
/// words of the first Kotora feeds ([feeds] include/exclude) become two filters once.
pub fn load_filters(c: &Feeds) -> Vec<Filter> {
    if let Ok(xml) = std::fs::read_to_string(filters_file()) {
        return taiga::import_filters(&xml).unwrap_or_default();
    }
    words_filters(&c.include, &c.exclude)
}

/// The old "must have" and "never" words as Taiga filters.
pub fn words_filters(include: &[String], exclude: &[String]) -> Vec<Filter> {
    let mut out = Vec::new();
    if !exclude.is_empty() {
        let conditions = exclude.iter().map(|w| Condition { element: Element::FileTitle, op: Operator::Contains, value: w.clone() }).collect();
        out.push(Filter { name: "Discard words".into(), matching: Match::Any, conditions, ..Filter::default() });
    }
    if !include.is_empty() {
        let conditions = include.iter().map(|w| Condition { element: Element::FileTitle, op: Operator::NotContains, value: w.clone() }).collect();
        out.push(Filter { name: "Discard without words".into(), matching: Match::Any, conditions, ..Filter::default() });
    }
    out
}

// ------------------------------------------------------------------------------- download

/// One Download: the .torrent saved into the folder, or the magnet / .torrent file handed to the
/// torrent app (the system's or the one chosen). Returns what to say.
pub fn open_one(it: &Item, anime: Option<&str>, o: &Feeds) -> Result<String, String> {
    let cache = paths::cache_dir().join("torrents");
    if o.save_to_folder && it.torrent.is_some() {
        let only = Item { magnet: None, ..it.clone() };
        let file = download_target(&only, &cache)?;
        let to = save_into(Path::new(&file), Path::new(o.folder.trim()), o.subfolders, anime, &it.title)?;
        return Ok(format!("Saved {}", to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    }
    let target = download_target(it, &cache)?;
    if o.app_mode == "custom" {
        let (prog, args) = app_command(&o.app_path, &target).ok_or("Choose the torrent app in Settings \u{2192} Feeds.")?;
        let mut child = std::process::Command::new(&prog).args(&args).spawn().map_err(|e| format!("Can't open the torrent app: {e}"))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        return Ok("Opened in your torrent app.".into());
    }
    crate::online::open_system(&target)?;
    Ok("Opened in your torrent app.".into())
}

// --------------------------------------------------------------------------- the editor

/// The filter dialog (Taiga's `FeedFilterDialog`): first the presets, then the filter.
#[derive(Default)]
pub struct FilterEditor {
    /// The dialog is open.
    pub open: bool,
    /// Page one: choose a preset.
    pub presets: bool,
    /// Which filter is edited (`None`: a new one).
    pub index: Option<usize>,
    pub filter: Filter,
    pub name: TextEdit,
    pub values: Vec<TextEdit>,
    pub anime_query: TextEdit,
    /// The open picker: `el/N`, `op/N`.
    pub picker: Option<String>,
    pub import: TextEdit,
    pub said: Option<String>,
}

fn value_edit(s: &mut State, i: usize) -> &mut TextEdit {
    let v = &mut s.feeds.editor.values;
    while v.len() <= i {
        v.push(TextEdit::default());
    }
    &mut v[i]
}

impl State {
    /// The filters changed: they decide again (the hand-set states go, as Taiga re-examines).
    fn feeds_filters_changed(&mut self) {
        self.feeds.manual.clear();
        self.feeds_reselect();
        let xml = taiga::export_filters(&self.feeds.filters);
        if self.handle.is_none() {
            return;
        }
        let _ = std::thread::Builder::new().name("kotora-settings".into()).spawn(move || {
            let p = filters_file();
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, xml);
        });
    }

    fn feeds_set(&mut self, key: &'static str, value: String) {
        let Some(h) = self.handle.clone() else { return };
        let _ = std::thread::Builder::new().name("kotora-settings".into()).spawn(move || {
            if let Err(e) = config::write_setting(&paths::config_file(), "feeds", key, &value) {
                h.update_if(move |s, _| {
                    s.feeds.error = Some(format!("Can't save the setting: {e}"));
                    true
                });
            }
        });
    }

    pub fn set_feeds_autocheck(&mut self, on: bool) {
        self.feeds.cfg.autocheck = on;
        self.feeds_set("autocheck", on.to_string());
        self.feeds_tell_worker();
    }

    pub fn set_feeds_new_action(&mut self, a: &'static str) {
        self.feeds.cfg.new_action = a.into();
        self.feeds_set("new_action", config::quote(a));
    }

    pub fn set_feeds_filters_on(&mut self, on: bool) {
        self.feeds.cfg.filters_enabled = on;
        self.feeds_set("filters_enabled", on.to_string());
        self.feeds_reselect();
    }

    pub fn set_feeds_app_mode(&mut self, m: &'static str) {
        self.feeds.cfg.app_mode = m.into();
        self.feeds_set("app_mode", config::quote(m));
    }

    pub fn feeds_save_app(&mut self) {
        let t = self.feeds.app_edit.text.trim().to_string();
        self.feeds.cfg.app_path = t.clone();
        self.feeds_set("app_path", config::quote(&t));
        if !t.is_empty() {
            self.set_feeds_app_mode("custom");
        }
    }

    pub fn set_feeds_save_to_folder(&mut self, on: bool) {
        self.feeds.cfg.save_to_folder = on;
        self.feeds_set("save_to_folder", on.to_string());
    }

    pub fn set_feeds_subfolders(&mut self, on: bool) {
        self.feeds.cfg.subfolders = on;
        self.feeds_set("subfolders", on.to_string());
    }

    pub fn feeds_save_folder(&mut self) {
        let t = self.feeds.folder_edit.text.trim().to_string();
        if !t.is_empty() && !Path::new(&t).is_absolute() {
            self.feeds.error = Some("The download folder must be a full path.".into());
            return;
        }
        self.feeds.error = None;
        self.feeds.cfg.folder = t.clone();
        self.feeds_set("folder", config::quote(&t));
    }

    pub fn set_feeds_sort(&mut self, by: &'static str) {
        let (sort, desc) = match by {
            "episode-desc" => ("episode", true),
            "date" => ("date", false),
            "date-desc" => ("date", true),
            _ => ("episode", false),
        };
        self.feeds.cfg.sort_by = sort.into();
        self.feeds.cfg.sort_desc = desc;
        self.feeds_set("sort_by", config::quote(sort));
        self.feeds_set("sort_desc", desc.to_string());
    }

    pub fn feeds_clear_history(&mut self) {
        self.feeds.archive = Some(taiga::Archive::default());
        self.feeds_save_archive();
        self.feeds_reselect();
    }

    pub fn feeds_forget(&mut self, title: &str) {
        if let Some(a) = self.feeds.archive.as_mut() {
            a.titles.retain(|t| t != title);
        }
        self.feeds_save_archive();
        self.feeds_reselect();
    }

    // ------------------------------------------------------------------------ the table

    /// The table shown: the search's results while a search is open, else the feeds'.
    pub fn feeds_table(&self) -> &[Entry] {
        match &self.feeds.search {
            Some(sv) => &sv.entries,
            None => &self.feeds.entries,
        }
    }

    /// The check box: Selected ⇄ blank (a discarded item comes back blank, as in Taiga).
    pub fn feeds_toggle(&mut self, key: &str) {
        let Some(e) = self.feeds_table().iter().find(|e| e.item.key() == key) else { return };
        let to = if e.state == ItemState::Selected { ItemState::Blank } else { ItemState::Selected };
        self.feeds_mark(vec![key.to_string()], to);
    }

    pub fn feeds_mark(&mut self, keys: Vec<String>, to: ItemState) {
        for k in keys {
            self.feeds.manual.insert(k, to);
        }
        self.feeds_reselect();
    }

    pub fn feeds_checked(&self) -> Vec<String> {
        self.feeds_table().iter().filter(|e| e.state == ItemState::Selected).map(|e| e.item.key()).collect()
    }

    /// Download marked torrents, in Taiga's order (by anime, then episode or date).
    pub fn feeds_download_marked(&mut self) {
        let t = self.feeds_table();
        let order = taiga::download_order(t, self.feeds.cfg.sort_by == "date", self.feeds.cfg.sort_desc);
        let keys = order.into_iter().map(|i| t[i].item.key()).collect();
        self.feeds_download(keys);
    }

    pub fn feeds_discard_marked(&mut self) {
        let keys = self.feeds_checked();
        self.feeds_mark(keys, ItemState::DiscardedNormal);
    }

    /// "Select all new": every new episode of an anime on the list.
    pub fn feeds_select_new(&mut self) {
        let keys = self.feeds_table().iter().filter(|e| is_new(e)).map(|e| e.item.key()).collect();
        self.feeds_mark(keys, ItemState::Selected);
    }

    /// Discard everything not checked (Taiga's "Discard all" leaves the selected ones).
    pub fn feeds_discard_rest(&mut self) {
        let keys = self.feeds_table().iter().filter(|e| e.state == ItemState::Blank).map(|e| e.item.key()).collect();
        self.feeds_mark(keys, ItemState::DiscardedNormal);
    }

    // -------------------------------------------------------------- per-anime filters

    /// Taiga's "Select fansub group": prefer `group` for the anime (empty: none again).
    pub fn feeds_prefer_group(&mut self, id: u32, group: &str) {
        let title = self.feeds_title(id);
        taiga::set_fansub_filter(&mut self.feeds.filters, id, &title, group, "");
        self.feeds_filters_changed();
    }

    /// Taiga's "Discard all items of this anime" (a discard filter for it).
    pub fn feeds_discard_anime(&mut self, id: u32) {
        let title = self.feeds_title(id);
        self.feeds.filters.push(taiga::discard_filter(id, &title));
        self.feeds_filters_changed();
    }

    // ------------------------------------------------------------------- filter manager

    pub fn feeds_add_defaults(&mut self) {
        for f in taiga::default_filters() {
            if !self.feeds.filters.iter().any(|g| g.name == f.name) {
                self.feeds.filters.push(f);
            }
        }
        self.feeds_filters_changed();
    }

    pub fn feeds_filter_enabled(&mut self, i: usize, on: bool) {
        if let Some(f) = self.feeds.filters.get_mut(i) {
            f.enabled = on;
            self.feeds_filters_changed();
        }
    }

    pub fn feeds_filter_move(&mut self, i: usize, up: bool) {
        let j = if up { i.checked_sub(1) } else { Some(i + 1) };
        if let Some(j) = j.filter(|j| *j < self.feeds.filters.len()) {
            self.feeds.filters.swap(i, j);
            self.feeds_filters_changed();
        }
    }

    pub fn feeds_filter_delete(&mut self, i: usize) {
        if i < self.feeds.filters.len() {
            self.feeds.filters.remove(i);
            self.feeds.editor.open = false;
            self.feeds_filters_changed();
        }
    }

    /// Add: the dialog opens on the presets.
    pub fn feeds_filter_new(&mut self) {
        self.feeds.editor = FilterEditor { open: true, presets: true, import: std::mem::take(&mut self.feeds.editor.import), ..FilterEditor::default() };
    }

    fn feeds_editor_load(&mut self, index: Option<usize>, f: Filter) {
        let e = &mut self.feeds.editor;
        e.open = true;
        e.presets = false;
        e.index = index;
        e.name = TextEdit::default();
        e.name.text = f.name.clone();
        e.values = f
            .conditions
            .iter()
            .map(|c| {
                let mut t = TextEdit::default();
                t.text = c.value.clone();
                t
            })
            .collect();
        e.filter = f;
        e.picker = None;
        e.anime_query = TextEdit::default();
    }

    pub fn feeds_filter_preset(&mut self, n: usize) {
        if let Some(p) = taiga::presets().into_iter().nth(n) {
            self.feeds_editor_load(None, p.filter);
        }
    }

    pub fn feeds_filter_edit(&mut self, i: usize) {
        if let Some(f) = self.feeds.filters.get(i).cloned() {
            self.feeds_editor_load(Some(i), f);
        }
    }

    pub fn feeds_filter_cancel(&mut self) {
        self.feeds.editor.open = false;
    }

    /// The values typed into the condition fields go back into the filter.
    fn feeds_editor_sync(&mut self) {
        let e = &mut self.feeds.editor;
        for (i, c) in e.filter.conditions.iter_mut().enumerate() {
            if let Some(v) = e.values.get(i) {
                c.value = v.text.clone();
            }
        }
        e.filter.name = e.name.text.trim().to_string();
    }

    pub fn feeds_filter_save(&mut self) {
        self.feeds_editor_sync();
        let e = &mut self.feeds.editor;
        if e.filter.name.is_empty() {
            e.filter.name = "New Filter".into();
        }
        if e.filter.conditions.is_empty() {
            e.said = Some("A filter needs a condition.".into());
            return;
        }
        let f = e.filter.clone();
        match e.index {
            Some(i) if i < self.feeds.filters.len() => self.feeds.filters[i] = f,
            _ => self.feeds.filters.push(f),
        }
        self.feeds.editor.open = false;
        self.feeds.editor.said = None;
        self.feeds_filters_changed();
    }

    pub fn feeds_cond_add(&mut self) {
        self.feeds_editor_sync();
        let e = &mut self.feeds.editor;
        e.filter.conditions.push(Condition { element: Element::FileTitle, op: Operator::Contains, value: String::new() });
        e.values.push(TextEdit::default());
    }

    pub fn feeds_cond_remove(&mut self, i: usize) {
        self.feeds_editor_sync();
        let e = &mut self.feeds.editor;
        if i < e.filter.conditions.len() {
            e.filter.conditions.remove(i);
            if i < e.values.len() {
                e.values.remove(i);
            }
        }
    }

    pub fn feeds_cond_element(&mut self, i: usize, el: Element) {
        self.feeds_editor_sync();
        let e = &mut self.feeds.editor;
        if let Some(c) = e.filter.conditions.get_mut(i) {
            c.element = el;
        }
        e.picker = None;
    }

    pub fn feeds_cond_op(&mut self, i: usize, op: Operator) {
        self.feeds_editor_sync();
        let e = &mut self.feeds.editor;
        if let Some(c) = e.filter.conditions.get_mut(i) {
            c.op = op;
        }
        e.picker = None;
    }

    pub fn feeds_cond_value(&mut self, i: usize, v: String) {
        self.feeds_editor_sync();
        value_edit(self, i).text = v.clone();
        if let Some(c) = self.feeds.editor.filter.conditions.get_mut(i) {
            c.value = v;
        }
    }

    pub fn feeds_picker(&mut self, key: String) {
        let e = &mut self.feeds.editor;
        e.picker = if e.picker.as_deref() == Some(&key) { None } else { Some(key) };
    }

    pub fn feeds_filter_anime(&mut self, id: u32, add: bool) {
        let ids = &mut self.feeds.editor.filter.anime_ids;
        ids.retain(|x| *x != id);
        if add {
            ids.push(id);
            self.feeds.editor.anime_query = TextEdit::default();
        }
    }

    /// Export: Taiga's XML into the Downloads folder (or the config folder).
    pub fn feeds_filters_export(&mut self) {
        let xml = taiga::export_filters(&self.feeds.filters);
        let dir = paths::home_dir().map(|h| h.join("Downloads")).filter(|d| d.is_dir()).unwrap_or_else(paths::config_dir);
        let to = dir.join("kotora-feed-filters.xml");
        self.feeds.editor.said = Some(if self.handle.is_none() {
            format!("Exported to {}", to.display())
        } else {
            match std::fs::write(&to, xml) {
                Ok(()) => format!("Exported {} filters to {}", self.feeds.filters.len(), to.display()),
                Err(e) => format!("Can't export: {e}"),
            }
        });
    }

    /// Import: a Taiga filter file (Taiga's settings.xml works too); its filters are added.
    pub fn feeds_filters_import(&mut self) {
        let p = self.feeds.editor.import.text.trim().to_string();
        if p.is_empty() {
            return;
        }
        let r = std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|x| taiga::import_filters(&x));
        self.feeds.editor.said = Some(match r {
            Ok(v) => {
                let n = v.len();
                self.feeds.filters.extend(v);
                self.feeds_filters_changed();
                self.feeds.editor.import = TextEdit::default();
                format!("Imported {n} filter{}.", if n == 1 { "" } else { "s" })
            }
            Err(e) => format!("Can't import: {e}"),
        });
    }
}

// ---------------------------------------------------------------------------- settings

pub const NEW_ACTIONS: [&str; 2] = ["notify", "download"];
pub const APP_MODES: [&str; 2] = ["default", "custom"];
pub const ORDERS: [&str; 4] = ["episode", "episode-desc", "date", "date-desc"];

/// The `&'static str` of `all` equal to `v` (the first when none is).
pub fn pick(all: &[&'static str], v: &str) -> &'static str {
    all.iter().copied().find(|a| *a == v).unwrap_or(all[0])
}

pub fn slug(v: &'static str) -> &'static str {
    v
}

pub fn new_action_label(v: &'static str) -> &'static str {
    if v == "download" { "Download them" } else { "Notify me" }
}

fn app_mode_label(v: &'static str) -> &'static str {
    if v == "custom" { "This app" } else { "The system\u{2019}s torrent app" }
}

fn order_label(v: &'static str) -> &'static str {
    match v {
        "episode-desc" => "Episode, last first",
        "date" => "Date, oldest first",
        "date-desc" => "Date, newest first",
        _ => "Episode",
    }
}

pub fn order_choice(s: &State) -> El {
    let c = &s.feeds.cfg;
    let on = match (c.sort_by.as_str(), c.sort_desc) {
        ("date", false) => "date",
        ("date", true) => "date-desc",
        (_, true) => "episode-desc",
        _ => "episode",
    };
    crate::settings::choice_if("set/feeds-order", &ORDERS, on, c.enabled, order_label, slug, State::set_feeds_sort)
}

fn field(key: String, edit: &TextEdit, hint: &str, mono: bool, get: impl for<'a> Fn(&'a mut State) -> &'a mut TextEdit + 'static, submit: impl Fn(&mut State) + 'static) -> El {
    let style = if mono { TextStyle::new(12.0, 400.0).mono() } else { TextStyle::new(12.0, 400.0) };
    row()
        .key(format!("{key}/box"))
        .role("textbox")
        .grow(1.0)
        .min_w(0.0)
        .h(30.0)
        .px(10.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, PAL.border_strong)
        .child(
            text_input(edit, style)
                .key(key)
                .grow(1.0)
                .min_w(0.0)
                .single_line()
                .color(PAL.text)
                .placeholder(field_hint(edit, hint), PAL.faint)
                .on_edit(get)
                .on_input_key(crate::ui::field_page_key)
                .on_submit(move |s: &mut State, tx: &mut Tx| {
                    tx.blur();
                    submit(s);
                }),
        )
}

pub fn app_block(s: &State) -> El {
    let c = &s.feeds.cfg;
    let hint = if cfg!(target_os = "macos") {
        "/Applications/qBittorrent.app"
    } else if cfg!(windows) {
        "C:\\Program Files\\qBittorrent\\qbittorrent.exe"
    } else {
        "/usr/bin/qbittorrent"
    };
    let b = col()
        .key("set/feeds/app")
        .gap(8.0)
        .w_full()
        .child(crate::settings::choice_if("set/feeds-app-mode", &APP_MODES, pick(&APP_MODES, &c.app_mode), c.enabled, app_mode_label, slug, State::set_feeds_app_mode))
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(field("set/feeds/app-path".into(), &s.feeds.app_edit, hint, true, |s| &mut s.feeds.app_edit, State::feeds_save_app))
                .child(small("set/feeds/app-save".into(), "Save", false, State::feeds_save_app)),
        );
    if c.enabled { b } else { b.opacity(0.4) }
}

pub fn folder_block(s: &State) -> El {
    let c = &s.feeds.cfg;
    let hint = if cfg!(windows) { "C:\\Users\\you\\Downloads\\torrents" } else { "/Users/you/Downloads/torrents" };
    let mut b = col()
        .key("set/feeds/folder")
        .gap(8.0)
        .w_full()
        .child(row().gap(10.0).center_y().child(text("Save .torrent files to a folder").font(theme::LABEL).color(PAL.text)).child(spacer()).child(crate::ui::switch_if("set/feeds-save-folder".into(), "Save to a folder", c.save_to_folder, c.enabled, State::set_feeds_save_to_folder)))
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(field("set/feeds/folder-path".into(), &s.feeds.folder_edit, hint, true, |s| &mut s.feeds.folder_edit, State::feeds_save_folder))
                .child(small("set/feeds/folder-save".into(), "Save", false, State::feeds_save_folder)),
        )
        .child(row().gap(10.0).center_y().child(text("A folder for each anime").font(theme::LABEL).color(PAL.text)).child(spacer()).child(crate::ui::switch_if("set/feeds-subfolders".into(), "A folder for each anime", c.subfolders, c.enabled && c.save_to_folder, State::set_feeds_subfolders)));
    if let Some(e) = &s.feeds.error {
        b = b.child(text(e.clone()).font(theme::LABEL).color(PAL.warn()).wrap());
    }
    if c.enabled { b } else { b.opacity(0.4) }
}

pub fn history_note(s: &State) -> String {
    let n = s.feeds.archive.as_ref().map_or(0, |a| a.titles.len());
    format!("{n} torrent{} downloaded. A feed showing one again discards it. Keeps the last {}.", if n == 1 { "" } else { "s" }, s.feeds.cfg.archive_max)
}

pub fn history_button(s: &State) -> El {
    let has = s.feeds.archive.as_ref().is_some_and(|a| !a.titles.is_empty());
    let b = small("set/feeds/history-clear".into(), "Clear history", false, move |s| {
        if has {
            s.feeds_clear_history();
        }
    });
    if has { b } else { b.opacity(0.4) }
}

/// A short line for a filter: "Discard · any of 7 conditions · 2 anime".
fn filter_summary(f: &Filter) -> String {
    let mut v = vec![match f.action {
        Action::Discard => "Discard",
        Action::Select => "Select",
        Action::Prefer => "Prefer",
    }
    .to_string()];
    let n = f.conditions.len();
    v.push(match (f.matching, n) {
        (_, 1) => "1 condition".into(),
        (Match::All, n) => format!("all of {n} conditions"),
        (Match::Any, n) => format!("any of {n} conditions"),
    });
    if !f.anime_ids.is_empty() {
        v.push(format!("{} anime", f.anime_ids.len()));
    }
    if f.option != FilterOption::Default {
        v.push(f.option.label().to_lowercase());
    }
    v.join(" \u{b7} ")
}

pub(crate) fn check_box(key: String, label: &str, on: bool, f: impl Fn(&mut State) + 'static) -> El {
    let b = el()
        .key(key)
        .role("checkbox")
        .label(label.to_string())
        .focusable(true)
        .size(16.0, 16.0)
        .radius(theme::R_XS)
        .center()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            f(s);
        });
    if on { b.fill(PAL.accent).child(text("\u{2713}").font(TextStyle::new(11.0, 700.0)).color(PAL.on_accent)) } else { b.fill(PAL.field).border(1.0, PAL.border_strong) }
}

/// Settings → Feeds → Filters: Taiga's filter manager.
pub fn filters_block(s: &State) -> El {
    let f = &s.feeds;
    let mut c = col().key("set/feeds/filters").gap(4.0).w_full();
    if f.filters.is_empty() {
        c = c.child(text("No filters: every item stays unmarked until you check it. Taiga\u{2019}s defaults select what you watch and discard the rest of your list\u{2019}s noise.").key("set/feeds/filters/none").font(theme::LABEL).color(PAL.dim).wrap());
    }
    let n = f.filters.len();
    for (i, flt) in f.filters.iter().enumerate() {
        let on = flt.enabled;
        c = c.child(
            row()
                .key(format!("set/feeds/filter/{i}"))
                .gap(10.0)
                .py(4.0)
                .center_y()
                .child(check_box(format!("set/feeds/filter/{i}/on"), "Enabled", on, move |s| s.feeds_filter_enabled(i, !on)))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(1.0)
                        .child(text(flt.name.clone()).font(theme::BODY_MED).color(if on { PAL.text } else { PAL.dim }).truncate())
                        .child(text(filter_summary(flt)).font(theme::LABEL).color(PAL.dim).truncate()),
                )
                .child(small(format!("set/feeds/filter/{i}/up"), "\u{2191}", false, move |s| s.feeds_filter_move(i, true)).opacity(if i == 0 { 0.4 } else { 1.0 }))
                .child(small(format!("set/feeds/filter/{i}/down"), "\u{2193}", false, move |s| s.feeds_filter_move(i, false)).opacity(if i + 1 == n { 0.4 } else { 1.0 }))
                .child(small(format!("set/feeds/filter/{i}/edit"), "Edit", false, move |s| s.feeds_filter_edit(i)))
                .child(small(format!("set/feeds/filter/{i}/delete"), "Delete", false, move |s| s.feeds_filter_delete(i))),
        );
    }
    c = c.child(
        row()
            .gap(8.0)
            .pt(8.0)
            .flex_wrap()
            .child(small("set/feeds/filter-add".into(), "Add filter\u{2026}", true, State::feeds_filter_new))
            .child(small("set/feeds/filter-defaults".into(), "Add Taiga\u{2019}s default filters", false, State::feeds_add_defaults))
            .child(small("set/feeds/filter-export".into(), "Export", false, State::feeds_filters_export)),
    );
    c = c.child(
        row()
            .gap(8.0)
            .pt(4.0)
            .center_y()
            .child(field("set/feeds/filter-import".into(), &f.editor.import, "A Taiga filter file (.xml) to import", true, |s| &mut s.feeds.editor.import, State::feeds_filters_import))
            .child(small("set/feeds/filter-import-go".into(), "Import", false, State::feeds_filters_import)),
    );
    if let Some(m) = &f.editor.said {
        c = c.child(text(m.clone()).key("set/feeds/filters/said").font(theme::LABEL).color(PAL.dim).wrap());
    }
    if f.editor.open {
        c = c.child(editor(s));
    }
    c
}

fn pill(key: String, label: String, on: bool, f: impl Fn(&mut State) + 'static) -> El {
    crate::ui::cell(key, label.clone(), on, f).child(text(label).font(theme::LABEL).color(PAL.text))
}

/// The filter dialog, inline under the list.
fn editor(s: &State) -> El {
    let e = &s.feeds.editor;
    let mut c = col().key("set/feeds/editor").gap(10.0).w_full().mt(10.0).px(14.0).py(14.0).radius(theme::R_MD).fill(PAL.raised).border(1.0, PAL.border_strong);
    if e.presets {
        c = c.child(text("Add filter: choose a preset").font(theme::BODY_MED).color(PAL.text));
        for (n, p) in taiga::presets().iter().enumerate() {
            c = c.child(
                row()
                    .key(format!("set/feeds/preset/{n}"))
                    .role("button")
                    .label(p.filter.name.clone())
                    .gap(10.0)
                    .py(5.0)
                    .px(8.0)
                    .radius(theme::R_SM)
                    .hover(PAL.hover())
                    .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                        tx.instant();
                        s.feeds_filter_preset(n);
                    })
                    .child(
                        col()
                            .grow(1.0)
                            .min_w(0.0)
                            .gap(1.0)
                            .child(text(if p.is_default { format!("{} (default)", p.filter.name) } else { p.filter.name.clone() }).font(theme::BODY_MED).color(PAL.text).truncate())
                            .child(text(p.description).font(theme::LABEL).color(PAL.dim).wrap()),
                    ),
            );
        }
        return c.child(row().child(spacer()).child(small("set/feeds/editor/cancel".into(), "Cancel", false, State::feeds_filter_cancel)));
    }
    let flt = &e.filter;
    c = c.child(text(if e.index.is_some() { "Edit filter" } else { "New filter" }).font(theme::BODY_MED).color(PAL.text));
    c = c.child(row().gap(10.0).center_y().child(text("Name").font(theme::LABEL).color(PAL.dim).w(70.0)).child(field("set/feeds/editor/name".into(), &e.name, "New Filter", false, |s| &mut s.feeds.editor.name, |_| {})));
    let mut m = row().key("set/feeds/editor/match").gap(6.0).flex_wrap().center_y().child(text("Match").font(theme::LABEL).color(PAL.dim).w(70.0));
    for x in Match::ALL {
        m = m.child(pill(format!("set/feeds/editor/match/{}", x.code()), x.label().into(), flt.matching == x, move |s| s.feeds.editor.filter.matching = x));
    }
    let mut a = row().key("set/feeds/editor/action").gap(6.0).flex_wrap().center_y().child(text("Action").font(theme::LABEL).color(PAL.dim).w(70.0));
    for x in Action::ALL {
        a = a.child(pill(format!("set/feeds/editor/action/{}", x.code()), x.label().into(), flt.action == x, move |s| s.feeds.editor.filter.action = x));
    }
    c = c.child(m).child(a);
    if flt.action != Action::Select {
        let mut o = row().key("set/feeds/editor/option").gap(6.0).flex_wrap().center_y().child(text("Option").font(theme::LABEL).color(PAL.dim).w(70.0));
        for x in FilterOption::ALL {
            o = o.child(pill(format!("set/feeds/editor/option/{}", x.code()), x.label().into(), flt.option == x, move |s| s.feeds.editor.filter.option = x));
        }
        c = c.child(o);
    }
    // Conditions.
    c = c.child(text("Conditions").font(theme::LABEL_BTN).color(PAL.text).pt(4.0));
    let title_of = |id: u32| s.entries.iter().find(|x| x.id == id).map(|x| x.title.clone());
    for (i, cond) in flt.conditions.iter().enumerate() {
        let el_key = format!("el/{i}");
        let op_key = format!("op/{i}");
        let mut line = row()
            .key(format!("set/feeds/editor/cond/{i}"))
            .gap(6.0)
            .center_y()
            .flex_wrap()
            .child(small(format!("set/feeds/editor/cond/{i}/element"), cond.element.label(), false, {
                let k = el_key.clone();
                move |s| s.feeds_picker(k.clone())
            }))
            .child(small(format!("set/feeds/editor/cond/{i}/op"), cond.op.label(), false, {
                let k = op_key.clone();
                move |s| s.feeds_picker(k.clone())
            }));
        let choices: Option<Vec<(String, String)>> = match cond.element {
            Element::UserStatus => Some((0..=5).map(|n| (n.to_string(), taiga::value_label(cond.element, &n.to_string(), &title_of))).collect()),
            Element::MetaStatus => Some((1..=3).map(|n| (n.to_string(), taiga::value_label(cond.element, &n.to_string(), &title_of))).collect()),
            Element::MetaType => Some((1..=6).map(|n| (n.to_string(), taiga::value_label(cond.element, &n.to_string(), &title_of))).collect()),
            Element::LocalEpisodeAvailable => Some(vec![("True".into(), "True".into()), ("False".into(), "False".into())]),
            _ => None,
        };
        match choices {
            Some(v) => {
                for (val, lab) in v {
                    let on = cond.value.eq_ignore_ascii_case(&val);
                    line = line.child(pill(format!("set/feeds/editor/cond/{i}/v/{val}"), lab, on, move |s| s.feeds_cond_value(i, val.clone())));
                }
            }
            None => {
                let edit = e.values.get(i).cloned().unwrap_or_default();
                let hint = match cond.element {
                    Element::EpisodeNumber => "%watched%",
                    Element::EpisodeVideoResolution => "1080p",
                    Element::FileSize => "1 GB",
                    Element::MetaId => "anime id",
                    _ => "value",
                };
                line = line.child(field(format!("set/feeds/editor/cond/{i}/value"), &edit, hint, false, move |s| value_edit(s, i), |_| {}).min_w(140.0));
            }
        }
        line = line.child(small(format!("set/feeds/editor/cond/{i}/remove"), "\u{d7}", false, move |s| s.feeds_cond_remove(i)));
        c = c.child(line);
        if e.picker.as_deref() == Some(&el_key) {
            let mut p = row().key(format!("set/feeds/editor/cond/{i}/elements")).gap(6.0).flex_wrap().pl(12.0);
            for x in Element::ALL {
                p = p.child(pill(format!("set/feeds/editor/cond/{i}/element/{}", x.code()), x.label().into(), cond.element == x, move |s| s.feeds_cond_element(i, x)));
            }
            c = c.child(p);
        }
        if e.picker.as_deref() == Some(&op_key) {
            let mut p = row().key(format!("set/feeds/editor/cond/{i}/ops")).gap(6.0).flex_wrap().pl(12.0);
            for x in Operator::ALL {
                p = p.child(pill(format!("set/feeds/editor/cond/{i}/op/{}", x.code()), x.label().into(), cond.op == x, move |s| s.feeds_cond_op(i, x)));
            }
            c = c.child(p);
        }
    }
    c = c.child(row().child(small("set/feeds/editor/cond-add".into(), "Add condition", false, State::feeds_cond_add)));
    c = c.child(text("Values may use %title%, %watched%, %total%, %episode%, %group%, %resolution%, %version%, %status%, %id%.").font(theme::LABEL).color(PAL.dim).wrap());
    // Anime.
    c = c.child(text("Limit to anime (none: every anime)").font(theme::LABEL_BTN).color(PAL.text).pt(4.0));
    let mut chips = row().key("set/feeds/editor/anime").gap(6.0).flex_wrap();
    for &id in &flt.anime_ids {
        let t = title_of(id).unwrap_or_else(|| format!("Anime {id}"));
        chips = chips.child(pill(format!("set/feeds/editor/anime/{id}"), format!("{t} \u{d7}"), true, move |s| s.feeds_filter_anime(id, false)));
    }
    c = c.child(chips);
    c = c.child(row().gap(8.0).center_y().child(field("set/feeds/editor/anime-find".into(), &e.anime_query, "Find an anime on your list", false, |s| &mut s.feeds.editor.anime_query, |_| {})));
    let q = e.anime_query.text.trim().to_lowercase();
    if !q.is_empty() {
        let mut hits = row().key("set/feeds/editor/anime-hits").gap(6.0).flex_wrap();
        for it in s.feeds.list.iter().filter(|it| it.media.title.to_lowercase().contains(&q) && !flt.anime_ids.contains(&it.media.id)).take(8) {
            let id = it.media.id;
            hits = hits.child(pill(format!("set/feeds/editor/anime-add/{id}"), it.media.title.clone(), false, move |s| s.feeds_filter_anime(id, true)));
        }
        c = c.child(hits);
    }
    if let Some(m) = &e.said {
        c = c.child(text(m.clone()).font(theme::LABEL).color(PAL.warn()).wrap());
    }
    c.child(
        row()
            .gap(8.0)
            .pt(4.0)
            .child(spacer())
            .child(small("set/feeds/editor/cancel".into(), "Cancel", false, State::feeds_filter_cancel))
            .child(small("set/feeds/editor/save".into(), "Save filter", true, State::feeds_filter_save)),
    )
}

// ---------------------------------------------------------------------------- the page

const W_CHECK: f32 = 22.0;
const W_EP: f32 = 58.0;
const W_GROUP: f32 = 110.0;
const W_SIZE: f32 = 70.0;
const W_VIDEO: f32 = 110.0;
const W_PEERS: f32 = 70.0;

struct Cols {
    group: bool,
    video: bool,
    peers: bool,
    desc: bool,
}

fn cols(width: f32) -> Cols {
    Cols { group: width > 640.0, video: width > 760.0, peers: width > 860.0, desc: width > 1040.0 }
}

fn head_cell(t: &str, w: Option<f32>) -> El {
    let x = text(t.to_string()).font(theme::CAPTION).color(PAL.faint).truncate();
    match w {
        Some(w) => x.w(w),
        None => x.grow(1.0).min_w(0.0),
    }
}

fn cell_text(t: String, w: Option<f32>, color: rune::Color) -> El {
    let x = text(t).font(theme::LABEL).color(color).truncate();
    match w {
        Some(w) => x.w(w),
        None => x.grow(1.0).min_w(0.0),
    }
}

fn header(c: &Cols) -> El {
    let mut r = row().key("feeds/head").gap(theme::COL_GAP).h(28.0).center_y().px(10.0).child(el().w(W_CHECK)).child(head_cell("ANIME", None)).child(head_cell("EPISODE", Some(W_EP)));
    if c.group {
        r = r.child(head_cell("GROUP", Some(W_GROUP)));
    }
    r = r.child(head_cell("SIZE", Some(W_SIZE)));
    if c.video {
        r = r.child(head_cell("VIDEO", Some(W_VIDEO)));
    }
    if c.peers {
        r = r.child(head_cell("S / L", Some(W_PEERS)));
    }
    if c.desc {
        r = r.child(head_cell("DESCRIPTION", None));
    }
    r
}

/// The Anime column: the list's title, else the parsed one.
fn anime_text(s: &State, e: &Entry) -> String {
    match e.anime_id {
        Some(id) => s.feeds_title(id),
        None if e.anime_title.is_empty() => e.item.title.clone(),
        None => e.anime_title.clone(),
    }
}

fn table_row(s: &State, e: &Entry, c: &Cols) -> El {
    let key = e.item.key();
    let discarded = e.state.is_discarded();
    let checked = e.state == ItemState::Selected;
    let open = s.feeds.open_row.as_deref() == Some(&key);
    let busy = s.feeds.busy.contains(&key);
    let fg = if discarded { PAL.faint } else { PAL.text };
    let mut anime = row().gap(6.0).grow(1.0).min_w(0.0).center_y().child(text(anime_text(s, e)).font(if is_new(e) { theme::BODY_MED } else { theme::BODY }).color(fg).truncate().min_w(0.0));
    if e.category != Category::Anime {
        anime = anime.child(crate::ui::tag(e.category.label(), PAL.dim, false));
    }
    if busy {
        anime = anime.child(text("Downloading\u{2026}").font(theme::LABEL).color(PAL.accent_hi));
    }
    let ep = e.episode_text();
    let ep_color = if is_new(e) { PAL.accent_hi } else { PAL.dim };
    let k2 = key.clone();
    let k3 = key.clone();
    let mut r = row()
        .key(format!("feeds/item/{key}"))
        .role("row")
        .label(e.item.title.clone())
        .gap(theme::COL_GAP)
        .h(34.0)
        .px(10.0)
        .center_y()
        .radius(theme::R_SM)
        .hover(PAL.hover())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.feeds.open_row = if s.feeds.open_row.as_deref() == Some(&k3) { None } else { Some(k3.clone()) };
        })
        .child(check_box(format!("feeds/check/{key}"), "Download", checked, move |s| s.feeds_toggle(&k2)))
        .child(anime)
        .child(cell_text(ep, Some(W_EP), ep_color));
    if checked {
        r = r.fill(PAL.tint(PAL.accent, 0.10));
    }
    if open {
        r = r.fill(PAL.tint(PAL.accent, 0.16));
    }
    if c.group {
        r = r.child(cell_text(e.release.parsed.group.clone().unwrap_or_default(), Some(W_GROUP), PAL.dim));
    }
    r = r.child(cell_text(e.item.size.map(size_text).unwrap_or_default(), Some(W_SIZE), PAL.dim));
    if c.video {
        r = r.child(cell_text(e.video_text(), Some(W_VIDEO), PAL.dim));
    }
    if c.peers {
        let p = match (e.item.seeders, e.item.leechers) {
            (None, None) => String::new(),
            (a, b) => format!("{}/{}", a.map_or("?".into(), |n| n.to_string()), b.map_or("?".into(), |n| n.to_string())),
        };
        r = r.child(cell_text(p, Some(W_PEERS), PAL.dim));
    }
    if c.desc {
        r = r.child(cell_text(e.item.description.clone(), None, PAL.dim));
    }
    if discarded {
        r = r.opacity(if e.state == ItemState::DiscardedInactive { 0.4 } else { 0.6 });
    }
    r
}

/// The open row's line: the file name and the actions (Taiga's context menu).
fn row_actions(s: &State, e: &Entry) -> El {
    let key = e.item.key();
    let mut facts = vec![e.item.title.clone()];
    if !e.item.category.is_empty() {
        facts.push(e.item.category.clone());
    }
    if let Some(n) = e.item.downloads {
        facts.push(format!("{n} downloads"));
    }
    let (k1, k2, k3) = (key.clone(), key.clone(), key.clone());
    let mut b = row()
        .gap(6.0)
        .flex_wrap()
        .child(small(format!("feeds/act/download/{key}"), "Download", true, move |s| s.feeds_download(vec![k1.clone()])))
        .child(small(format!("feeds/act/select/{key}"), "Select", false, move |s| s.feeds_mark(vec![k2.clone()], ItemState::Selected)))
        .child(small(format!("feeds/act/discard/{key}"), "Discard", false, move |s| s.feeds_mark(vec![k3.clone()], ItemState::DiscardedNormal)));
    if let Some(page) = e.item.page.clone() {
        b = b.child(small(format!("feeds/act/page/{key}"), "Open page", false, move |s| crate::online::open_url(s, page.clone())));
    }
    if let Some(id) = e.anime_id {
        if let Some(g) = e.release.parsed.group.clone().filter(|g| !g.is_empty()) {
            let on = taiga::fansub_groups(&s.feeds.filters, id).iter().any(|x| x.eq_ignore_ascii_case(&g));
            let label = if on { format!("Stop preferring {g}") } else { format!("Prefer {g} for this anime") };
            b = b.child(small(format!("feeds/act/prefer/{key}"), &label, false, move |s| s.feeds_prefer_group(id, if on { "" } else { &g })));
        }
        b = b.child(small(format!("feeds/act/discard-anime/{key}"), "Discard this anime", false, move |s| s.feeds_discard_anime(id)));
        b = b.child(small(format!("feeds/act/details/{key}"), "Anime details", false, move |s| {
            if let Some(e) = s.entries.iter().find(|e| e.id == id) {
                let stub = crate::details::Stub::from_entry(e);
                s.open_details(stub);
            }
        }));
    }
    col().key(format!("feeds/open/{key}")).gap(6.0).px(42.0).pb(8.0).child(text(facts.join(" \u{b7} ")).font(theme::LABEL).color(PAL.dim).wrap()).child(b)
}

fn history_view(s: &State) -> El {
    let mut c = card("feeds/history", "Torrent history");
    let titles = s.feeds.archive.as_ref().map(|a| a.titles.clone()).unwrap_or_default();
    if titles.is_empty() {
        return c.child(text("Nothing downloaded yet.").key("feeds/history/none").font(theme::BODY).color(PAL.dim).py(8.0));
    }
    c = c.child(text("Feeds discard these when they show them again. Remove one to see it again.").font(theme::LABEL).color(PAL.dim));
    for (n, t) in titles.iter().rev().take(300).enumerate() {
        let t2 = t.clone();
        c = c.child(row().key(format!("feeds/history/{n}")).gap(10.0).py(3.0).center_y().child(text(t.clone()).font(theme::BODY).color(PAL.text).truncate().grow(1.0).min_w(0.0)).child(small(format!("feeds/history/{n}/remove"), "Remove", false, move |s| s.feeds_forget(&t2))));
    }
    c.child(row().pt(8.0).child(spacer()).child(small("feeds/history/clear".into(), "Clear history", false, State::feeds_clear_history)))
}

fn tab(key: &str, label: &str, on: bool, f: impl Fn(&mut State) + 'static) -> El {
    crate::ui::cell(key.to_string(), label.to_string(), on, f).child(text(label.to_string()).font(theme::LABEL_BTN).color(if on { PAL.text } else { PAL.dim }))
}

/// The Torrents page (Taiga's torrent tab).
pub fn page(s: &State, width: f32) -> El {
    let f = &s.feeds;
    let inner = (width - 2.0 * theme::PAGE_PAD).max(320.0);
    let c = cols(inner);
    let table = s.feeds_table();
    let checked = table.iter().filter(|e| e.state == ItemState::Selected).count();
    let news: HashSet<_> = table.iter().filter(|e| is_new(e)).map(|e| e.item.key()).collect();
    let mut tools = row()
        .key("feeds/tools")
        .gap(8.0)
        .px(theme::PAGE_PAD)
        .pb(8.0)
        .center_y()
        .flex_wrap()
        .child(tab("feeds/tab/torrents", "Torrents", !f.history, |s| s.feeds.history = false))
        .child(tab("feeds/tab/history", "History", f.history, |s| s.feeds.history = true))
        .child(spacer());
    if !f.history {
        if !f.cfg.sources.is_empty() {
            tools = tools.child(small("feeds/check".into(), "Check new torrents", false, |s| s.feeds_check_now()));
        }
        tools = tools
            .child(small("feeds/select-new".into(), &format!("Select all new ({})", news.len()), false, State::feeds_select_new))
            .child(small("feeds/discard".into(), "Discard marked", false, State::feeds_discard_marked))
            .child(small("feeds/discard-rest".into(), "Discard unmarked", false, State::feeds_discard_rest))
            .child(small("feeds/download".into(), &format!("Download marked ({checked})"), checked > 0, move |s| {
                if checked > 0 {
                    s.feeds_download_marked();
                }
            }))
            .child(small("feeds/hide".into(), if f.hide_discarded { "Show discarded" } else { "Hide discarded" }, false, |s| s.feeds.hide_discarded = !s.feeds.hide_discarded));
    }
    let mut body = col().gap(12.0).w_full();
    if f.history {
        body = body.child(history_view(s));
    } else {
        if let Some(sv) = &f.search {
            let mut bar = row().key("feeds/search").gap(10.0).center_y().child(text(format!("Search: {}", sv.title)).font(theme::BODY_MED).color(PAL.text).truncate().min_w(0.0));
            if sv.loading {
                bar = bar.child(text("Searching\u{2026}").key("feeds/search/loading").font(theme::LABEL).color(PAL.dim));
            } else if let Some(e) = &sv.error {
                bar = bar.child(text(mask(e)).key("feeds/search/error").font(theme::LABEL).color(PAL.warn()).truncate().min_w(0.0));
            } else {
                bar = bar.child(text(format!("{} results", sv.entries.len())).font(theme::LABEL).color(PAL.dim));
            }
            body = body.child(bar.child(spacer()).child(small("feeds/search/close".into(), "Back to feeds", false, |s| {
                s.feeds.search = None;
                s.feeds_reselect();
            })));
        }
        if f.cfg.sources.is_empty() && f.search.is_none() {
            body = body.child(
                col()
                    .key("feeds/empty")
                    .gap(10.0)
                    .py(30.0)
                    .items_center()
                    .w_full()
                    .child(text("No feeds yet").font(theme::BODY_MED).color(PAL.text))
                    .child(text("Add an RSS feed in Settings \u{2192} Feeds. Kotora checks only the feeds you add.").font(theme::BODY).color(PAL.dim).wrap())
                    .child(small("feeds/open-settings".into(), "Open Settings", true, |s| crate::settings::open(s, crate::settings::Section::Feeds))),
            );
        } else {
            let shown: Vec<&Entry> = table.iter().filter(|e| e.state != ItemState::DiscardedHidden && !(f.hide_discarded && e.state.is_discarded())).collect();
            let mut t = col().key("feeds/table").w_full().child(header(&c)).child(el().h(1.0).w_full().fill(PAL.border));
            if shown.is_empty() {
                let waiting = f.search.as_ref().map_or(f.lines.is_empty() || f.lines.values().any(|l| l.checking), |sv| sv.loading);
                t = t.child(text(if waiting { "Checking your feeds\u{2026}" } else { "No torrents." }).key("feeds/none").font(theme::BODY).color(PAL.dim).py(12.0).px(10.0));
            }
            for e in shown {
                t = t.child(table_row(s, e, &c));
                if f.open_row.as_deref() == Some(&e.item.key()) {
                    t = t.child(row_actions(s, e));
                }
            }
            body = body.child(t);
            if f.search.is_none() && !f.cfg.sources.is_empty() {
                let mut feeds = card("feeds/sources", "Your feeds");
                for src in &f.cfg.sources {
                    let (dot, line) = crate::feeds::status_line(s, &src.url);
                    feeds = feeds.child(row().gap(10.0).py(4.0).center_y().child(el().size(7.0, 7.0).radius(3.5).fill(dot)).child(text(src.name.clone()).font(theme::BODY).color(PAL.text).truncate().min_w(0.0)).child(spacer()).child(text(line).font(theme::LABEL).color(PAL.dim)));
                }
                body = body.child(feeds);
            }
        }
    }
    if let Some(said) = &f.said {
        body = body.child(text(said.clone()).key("feeds/said").font(theme::LABEL).color(PAL.dim).wrap());
    }
    col()
        .key("page/feeds")
        .w_full()
        .h_full()
        .child(crate::ui::top(s, "Torrents", Some(s.feeds_count()), false))
        .child(tools)
        .child(col().key("feeds-scroll").scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(col().px(theme::PAGE_PAD).pt(4.0).pb(theme::PAGE_PAD).w(inner + 2.0 * theme::PAGE_PAD).child(body)))
}

/// Details → "Torrents": the next new episode's items, Download, the preferred group, Search.
pub fn details_card(s: &State, id: u32) -> Option<El> {
    if !s.feeds.cfg.enabled {
        return None;
    }
    let mine: Vec<&Entry> = s.feeds.entries.iter().filter(|e| e.anime_id == Some(id) && !e.state.is_discarded()).collect();
    let search = s.feeds_can_search(id).then(|| small(format!("det/search-releases/{id}"), "Search torrents", false, move |s| s.feeds_search(id)));
    let groups = taiga::fansub_groups(&s.feeds.filters, id);
    if mine.is_empty() && search.is_none() && groups.is_empty() {
        return None;
    }
    let next = s.feeds_badge(id);
    let mut c = card("det-feeds", "Torrents");
    match next {
        Some((a, b)) => {
            c = c.child(text(format!("{} is in your feeds.", ep_text(a, b))).font(theme::LABEL).color(PAL.dim));
            for e in mine.iter().filter(|e| e.release.episodes == Some((a, b))).take(4) {
                let key = e.item.key();
                let busy = s.feeds.busy.contains(&key);
                let facts: Vec<String> = [e.release.parsed.group.clone(), Some(e.video_text()).filter(|v| !v.is_empty()), e.item.size.map(size_text), e.item.seeders.map(|n| format!("{n} seeders"))].into_iter().flatten().collect();
                c = c.child(
                    row()
                        .key(format!("det/release/{key}"))
                        .gap(12.0)
                        .py(5.0)
                        .center_y()
                        .child(col().grow(1.0).min_w(0.0).gap(2.0).child(text(e.item.title.clone()).font(theme::BODY).color(PAL.text).truncate()).child(text(facts.join(" \u{b7} ")).font(theme::LABEL).color(PAL.dim).truncate()))
                        .child(small(format!("det/download/{key}"), if busy { "Downloading\u{2026}" } else { "Download" }, e.state == ItemState::Selected, move |s| s.feeds_download(vec![key.clone()]))),
                );
            }
        }
        None => c = c.child(text("No new episode in your feeds.").font(theme::LABEL).color(PAL.dim)),
    }
    if !groups.is_empty() {
        let mut r = row().gap(8.0).pt(6.0).center_y().flex_wrap().child(text(format!("Preferred group: {}", groups.join(", "))).font(theme::LABEL).color(PAL.text));
        r = r.child(small(format!("det/unprefer/{id}"), "Clear", false, move |s| s.feeds_prefer_group(id, "")));
        c = c.child(r);
    }
    Some(c.children(search.map(|b| row().pt(8.0).child(b))))
}
