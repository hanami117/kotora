// Portions ported from Taiga (https://github.com/erengy/taiga), Copyright (C) Eren Okka and
// contributors, GPL-3.0: src/ui/dlg/dlg_torrent.cpp (the Torrents page: its columns, check
// states, Check new torrents, Download marked torrents, Discard, Select, the context actions),
// src/ui/dlg/dlg_feed_filter.cpp and dlg_settings_page.cpp (the feed filter manager: presets,
// conditions, actions, options, anime, import and export; the torrent settings: auto-check, new
// episode action, the torrent application, the download folder, the download order) and
// src/track/feed_aggregator.cpp (Download, the archive) and res/menu.xml (TorrentListRightClick).

//! Taiga's torrent tab for Kotora: the Torrents page (every feed item, its anime, episode,
//! group, size, video, seeders/leechers and description, checked when selected, dimmed when
//! discarded), the filter manager in Settings → Feeds, per-anime filters, the torrent history
//! and the download options. The engine is `kotora_core::feeds::taiga`.

use std::path::Path;

use kotora_core::config::{self, Feeds};
use kotora_core::feeds::download::{app_command, save_into};
use kotora_core::feeds::poll::download_target;
use kotora_core::feeds::taiga::{
    self, Action, Category, Condition, Element, Entry, Filter, FilterOption, ItemState, Match,
    Operator,
};
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
        let conditions = exclude
            .iter()
            .map(|w| Condition {
                element: Element::FileTitle,
                op: Operator::Contains,
                value: w.clone(),
            })
            .collect();
        out.push(Filter {
            name: "Discard words".into(),
            matching: Match::Any,
            conditions,
            ..Filter::default()
        });
    }
    if !include.is_empty() {
        let conditions = include
            .iter()
            .map(|w| Condition {
                element: Element::FileTitle,
                op: Operator::NotContains,
                value: w.clone(),
            })
            .collect();
        out.push(Filter {
            name: "Discard without words".into(),
            matching: Match::Any,
            conditions,
            ..Filter::default()
        });
    }
    out
}

// ------------------------------------------------------------------------------- download

/// One Download: the .torrent saved into the folder, or the magnet / .torrent file handed to the
/// torrent app (the system's or the one chosen). Returns what to say.
pub fn open_one(it: &Item, anime: Option<&str>, o: &Feeds) -> Result<String, String> {
    let cache = paths::cache_dir().join("torrents");
    if o.save_to_folder && it.torrent.is_some() {
        let only = Item {
            magnet: None,
            ..it.clone()
        };
        let file = download_target(&only, &cache)?;
        let to = save_into(
            Path::new(&file),
            Path::new(o.folder.trim()),
            o.subfolders,
            anime,
            &it.title,
        )?;
        return Ok(format!(
            "Saved {}",
            to.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        ));
    }
    let target = download_target(it, &cache)?;
    if o.app_mode == "custom" {
        let (prog, args) = app_command(&o.app_path, &target)
            .ok_or("Choose the torrent app in Settings \u{2192} Feeds.")?;
        let mut child = std::process::Command::new(&prog)
            .args(&args)
            .spawn()
            .map_err(|e| format!("Can't open the torrent app: {e}"))?;
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
        let _ = std::thread::Builder::new()
            .name("kotora-settings".into())
            .spawn(move || {
                let p = filters_file();
                if let Some(d) = p.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let _ = std::fs::write(p, xml);
            });
    }

    fn feeds_set(&mut self, key: &'static str, value: String) {
        let Some(h) = self.handle.clone() else { return };
        let _ = std::thread::Builder::new()
            .name("kotora-settings".into())
            .spawn(move || {
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
        let Some(e) = self.feeds_table().iter().find(|e| e.item.key() == key) else {
            return;
        };
        // Taiga's LVN_ITEMCHANGED: checked = Selected, unchecked = DiscardedNormal.
        let to = if e.state == ItemState::Selected {
            ItemState::DiscardedNormal
        } else {
            ItemState::Selected
        };
        self.feeds_mark(vec![key.to_string()], to);
        let n = self.feeds_checked().len();
        self.feeds.status = Some(if n == 1 {
            "Marked 1 torrent.".into()
        } else {
            format!("Marked {n} torrents.")
        });
    }

    /// Taiga's Discard (menu) and Discard all (the checked ones): discarded and into the
    /// archive, so they stay discarded.
    pub fn feeds_discard_into_archive(&mut self, keys: Vec<String>) {
        let titles: Vec<String> = self
            .feeds_table()
            .iter()
            .filter(|e| keys.contains(&e.item.key()))
            .map(|e| e.item.title.clone())
            .collect();
        let a = self.feeds.archive.get_or_insert_with(Default::default);
        for t in &titles {
            a.add(t);
        }
        self.feeds_save_archive();
        self.feeds_mark(keys, ItemState::DiscardedNormal);
    }

    /// Taiga's "Select this fansub group for this anime": the anime's items of other groups (or
    /// another resolution) are discarded, and a fansub filter remembers it.
    pub fn feeds_select_fansub(&mut self, key: &str) {
        let Some(e) = self
            .feeds_table()
            .iter()
            .find(|e| e.item.key() == key)
            .cloned()
        else {
            return;
        };
        let (Some(id), Some(group)) = (
            e.anime_id,
            e.release.parsed.group.clone().filter(|g| !g.is_empty()),
        ) else {
            return;
        };
        let res = e.release.parsed.resolution.clone().unwrap_or_default();
        let others: Vec<String> = self
            .feeds_table()
            .iter()
            .filter(|x| x.anime_id == Some(id))
            .filter(|x| {
                !x.release
                    .parsed
                    .group
                    .as_deref()
                    .unwrap_or("")
                    .eq_ignore_ascii_case(&group)
                    || (!res.is_empty()
                        && !x
                            .release
                            .parsed
                            .resolution
                            .as_deref()
                            .unwrap_or("")
                            .eq_ignore_ascii_case(&res))
            })
            .map(|x| x.item.key())
            .collect();
        let title = self.feeds_title(id);
        taiga::set_fansub_filter(&mut self.feeds.filters, id, &title, &group, &res);
        self.feeds_filters_changed();
        self.feeds_mark(others, ItemState::DiscardedNormal);
    }

    pub fn feeds_mark(&mut self, keys: Vec<String>, to: ItemState) {
        for k in keys {
            self.feeds.manual.insert(k, to);
        }
        self.feeds_reselect();
    }

    pub fn feeds_checked(&self) -> Vec<String> {
        self.feeds_table()
            .iter()
            .filter(|e| e.state == ItemState::Selected)
            .map(|e| e.item.key())
            .collect()
    }

    /// Download marked torrents, in Taiga's order (by anime, then episode or date).
    pub fn feeds_download_marked(&mut self) {
        let t = self.feeds_table();
        let order = taiga::download_order(
            t,
            self.feeds.cfg.sort_by == "date",
            self.feeds.cfg.sort_desc,
        );
        let keys = order.into_iter().map(|i| t[i].item.key()).collect();
        self.feeds_download(keys);
    }

    /// Taiga's "Discard all": the checked items, into the archive.
    pub fn feeds_discard_marked(&mut self) {
        let keys = self.feeds_checked();
        self.feeds_discard_into_archive(keys);
    }

    /// "Select all new": every new episode of an anime on the list.
    pub fn feeds_select_new(&mut self) {
        let keys = self
            .feeds_table()
            .iter()
            .filter(|e| is_new(e))
            .map(|e| e.item.key())
            .collect();
        self.feeds_mark(keys, ItemState::Selected);
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
        self.feeds.editor = FilterEditor {
            open: true,
            presets: true,
            import: std::mem::take(&mut self.feeds.editor.import),
            ..FilterEditor::default()
        };
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
        e.filter.conditions.push(Condition {
            element: Element::FileTitle,
            op: Operator::Contains,
            value: String::new(),
        });
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
        e.picker = if e.picker.as_deref() == Some(&key) {
            None
        } else {
            Some(key)
        };
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
        let dir = paths::home_dir()
            .map(|h| h.join("Downloads"))
            .filter(|d| d.is_dir())
            .unwrap_or_else(paths::config_dir);
        let to = dir.join("kotora-feed-filters.xml");
        self.feeds.editor.said = Some(if self.handle.is_none() {
            format!("Exported to {}", to.display())
        } else {
            match std::fs::write(&to, xml) {
                Ok(()) => format!(
                    "Exported {} filters to {}",
                    self.feeds.filters.len(),
                    to.display()
                ),
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
        let r = std::fs::read_to_string(&p)
            .map_err(|e| e.to_string())
            .and_then(|x| taiga::import_filters(&x));
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
    if v == "download" {
        "Download them"
    } else {
        "Notify me"
    }
}

fn app_mode_label(v: &'static str) -> &'static str {
    if v == "custom" {
        "This app"
    } else {
        "The system\u{2019}s torrent app"
    }
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
    crate::settings::choice_if(
        "set/feeds-order",
        &ORDERS,
        on,
        c.enabled,
        order_label,
        slug,
        State::set_feeds_sort,
    )
}

fn field(
    key: String,
    edit: &TextEdit,
    hint: &str,
    mono: bool,
    get: impl for<'a> Fn(&'a mut State) -> &'a mut TextEdit + 'static,
    submit: impl Fn(&mut State) + 'static,
) -> El {
    let style = if mono {
        TextStyle::new(12.0, 400.0).mono()
    } else {
        TextStyle::new(12.0, 400.0)
    };
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
        .child(crate::settings::choice_if(
            "set/feeds-app-mode",
            &APP_MODES,
            pick(&APP_MODES, &c.app_mode),
            c.enabled,
            app_mode_label,
            slug,
            State::set_feeds_app_mode,
        ))
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(field(
                    "set/feeds/app-path".into(),
                    &s.feeds.app_edit,
                    hint,
                    true,
                    |s| &mut s.feeds.app_edit,
                    State::feeds_save_app,
                ))
                .child(small(
                    "set/feeds/app-save".into(),
                    "Save",
                    false,
                    State::feeds_save_app,
                )),
        );
    if c.enabled { b } else { b.opacity(0.4) }
}

pub fn folder_block(s: &State) -> El {
    let c = &s.feeds.cfg;
    let hint = if cfg!(windows) {
        "C:\\Users\\you\\Downloads\\torrents"
    } else {
        "/Users/you/Downloads/torrents"
    };
    let mut b = col()
        .key("set/feeds/folder")
        .gap(8.0)
        .w_full()
        .child(
            row()
                .gap(10.0)
                .center_y()
                .child(
                    text("Save .torrent files to a folder")
                        .font(theme::LABEL)
                        .color(PAL.text),
                )
                .child(spacer())
                .child(crate::ui::switch_if(
                    "set/feeds-save-folder".into(),
                    "Save to a folder",
                    c.save_to_folder,
                    c.enabled,
                    State::set_feeds_save_to_folder,
                )),
        )
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(field(
                    "set/feeds/folder-path".into(),
                    &s.feeds.folder_edit,
                    hint,
                    true,
                    |s| &mut s.feeds.folder_edit,
                    State::feeds_save_folder,
                ))
                .child(small(
                    "set/feeds/folder-save".into(),
                    "Save",
                    false,
                    State::feeds_save_folder,
                )),
        )
        .child(
            row()
                .gap(10.0)
                .center_y()
                .child(
                    text("A folder for each anime")
                        .font(theme::LABEL)
                        .color(PAL.text),
                )
                .child(spacer())
                .child(crate::ui::switch_if(
                    "set/feeds-subfolders".into(),
                    "A folder for each anime",
                    c.subfolders,
                    c.enabled && c.save_to_folder,
                    State::set_feeds_subfolders,
                )),
        );
    if let Some(e) = &s.feeds.error {
        b = b.child(text(e.clone()).font(theme::LABEL).color(PAL.warn()).wrap());
    }
    if c.enabled { b } else { b.opacity(0.4) }
}

pub fn history_note(s: &State) -> String {
    let n = s.feeds.archive.as_ref().map_or(0, |a| a.titles.len());
    format!(
        "{n} torrent{} downloaded. A feed showing one again discards it. Keeps the last {}.",
        if n == 1 { "" } else { "s" },
        s.feeds.cfg.archive_max
    )
}

pub fn history_button(s: &State) -> El {
    let has = s
        .feeds
        .archive
        .as_ref()
        .is_some_and(|a| !a.titles.is_empty());
    let b = small(
        "set/feeds/history-clear".into(),
        "Clear history",
        false,
        move |s| {
            if has {
                s.feeds_clear_history();
            }
        },
    );
    if has { b } else { b.opacity(0.4) }
}

/// A short line for a filter: "Discard · any of 7 conditions · 2 anime".
fn filter_summary(f: &Filter) -> String {
    let mut v = vec![
        match f.action {
            Action::Discard => "Discard",
            Action::Select => "Select",
            Action::Prefer => "Prefer",
        }
        .to_string(),
    ];
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

pub(crate) fn check_box(
    key: String,
    label: &str,
    on: bool,
    f: impl Fn(&mut State) + 'static,
) -> El {
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
    if on {
        b.fill(PAL.accent).child(
            text("\u{2713}")
                .font(TextStyle::new(11.0, 700.0))
                .color(PAL.on_accent),
        )
    } else {
        b.fill(PAL.field).border(1.0, PAL.border_strong)
    }
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
                .child(check_box(
                    format!("set/feeds/filter/{i}/on"),
                    "Enabled",
                    on,
                    move |s| s.feeds_filter_enabled(i, !on),
                ))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(1.0)
                        .child(
                            text(flt.name.clone())
                                .font(theme::BODY_MED)
                                .color(if on { PAL.text } else { PAL.dim })
                                .truncate(),
                        )
                        .child(
                            text(filter_summary(flt))
                                .font(theme::LABEL)
                                .color(PAL.dim)
                                .truncate(),
                        ),
                )
                .child(
                    small(
                        format!("set/feeds/filter/{i}/up"),
                        "\u{2191}",
                        false,
                        move |s| s.feeds_filter_move(i, true),
                    )
                    .opacity(if i == 0 { 0.4 } else { 1.0 }),
                )
                .child(
                    small(
                        format!("set/feeds/filter/{i}/down"),
                        "\u{2193}",
                        false,
                        move |s| s.feeds_filter_move(i, false),
                    )
                    .opacity(if i + 1 == n { 0.4 } else { 1.0 }),
                )
                .child(small(
                    format!("set/feeds/filter/{i}/edit"),
                    "Edit",
                    false,
                    move |s| s.feeds_filter_edit(i),
                ))
                .child(small(
                    format!("set/feeds/filter/{i}/delete"),
                    "Delete",
                    false,
                    move |s| s.feeds_filter_delete(i),
                )),
        );
    }
    c = c.child(
        row()
            .gap(8.0)
            .pt(8.0)
            .flex_wrap()
            .child(small(
                "set/feeds/filter-add".into(),
                "Add filter\u{2026}",
                true,
                State::feeds_filter_new,
            ))
            .child(small(
                "set/feeds/filter-defaults".into(),
                "Add Taiga\u{2019}s default filters",
                false,
                State::feeds_add_defaults,
            ))
            .child(small(
                "set/feeds/filter-export".into(),
                "Export",
                false,
                State::feeds_filters_export,
            )),
    );
    c = c.child(
        row()
            .gap(8.0)
            .pt(4.0)
            .center_y()
            .child(field(
                "set/feeds/filter-import".into(),
                &f.editor.import,
                "A Taiga filter file (.xml) to import",
                true,
                |s| &mut s.feeds.editor.import,
                State::feeds_filters_import,
            ))
            .child(small(
                "set/feeds/filter-import-go".into(),
                "Import",
                false,
                State::feeds_filters_import,
            )),
    );
    if let Some(m) = &f.editor.said {
        c = c.child(
            text(m.clone())
                .key("set/feeds/filters/said")
                .font(theme::LABEL)
                .color(PAL.dim)
                .wrap(),
        );
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
    let mut c = col()
        .key("set/feeds/editor")
        .gap(10.0)
        .w_full()
        .mt(10.0)
        .px(14.0)
        .py(14.0)
        .radius(theme::R_MD)
        .fill(PAL.raised)
        .border(1.0, PAL.border_strong);
    if e.presets {
        c = c.child(
            text("Add filter: choose a preset")
                .font(theme::BODY_MED)
                .color(PAL.text),
        );
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
                            .child(
                                text(if p.is_default {
                                    format!("{} (default)", p.filter.name)
                                } else {
                                    p.filter.name.clone()
                                })
                                .font(theme::BODY_MED)
                                .color(PAL.text)
                                .truncate(),
                            )
                            .child(text(p.description).font(theme::LABEL).color(PAL.dim).wrap()),
                    ),
            );
        }
        return c.child(row().child(spacer()).child(small(
            "set/feeds/editor/cancel".into(),
            "Cancel",
            false,
            State::feeds_filter_cancel,
        )));
    }
    let flt = &e.filter;
    c = c.child(
        text(if e.index.is_some() {
            "Edit filter"
        } else {
            "New filter"
        })
        .font(theme::BODY_MED)
        .color(PAL.text),
    );
    c = c.child(
        row()
            .gap(10.0)
            .center_y()
            .child(text("Name").font(theme::LABEL).color(PAL.dim).w(70.0))
            .child(field(
                "set/feeds/editor/name".into(),
                &e.name,
                "New Filter",
                false,
                |s| &mut s.feeds.editor.name,
                |_| {},
            )),
    );
    let mut m = row()
        .key("set/feeds/editor/match")
        .gap(6.0)
        .flex_wrap()
        .center_y()
        .child(text("Match").font(theme::LABEL).color(PAL.dim).w(70.0));
    for x in Match::ALL {
        m = m.child(pill(
            format!("set/feeds/editor/match/{}", x.code()),
            x.label().into(),
            flt.matching == x,
            move |s| s.feeds.editor.filter.matching = x,
        ));
    }
    let mut a = row()
        .key("set/feeds/editor/action")
        .gap(6.0)
        .flex_wrap()
        .center_y()
        .child(text("Action").font(theme::LABEL).color(PAL.dim).w(70.0));
    for x in Action::ALL {
        a = a.child(pill(
            format!("set/feeds/editor/action/{}", x.code()),
            x.label().into(),
            flt.action == x,
            move |s| s.feeds.editor.filter.action = x,
        ));
    }
    c = c.child(m).child(a);
    if flt.action != Action::Select {
        let mut o = row()
            .key("set/feeds/editor/option")
            .gap(6.0)
            .flex_wrap()
            .center_y()
            .child(text("Option").font(theme::LABEL).color(PAL.dim).w(70.0));
        for x in FilterOption::ALL {
            o = o.child(pill(
                format!("set/feeds/editor/option/{}", x.code()),
                x.label().into(),
                flt.option == x,
                move |s| s.feeds.editor.filter.option = x,
            ));
        }
        c = c.child(o);
    }
    // Conditions.
    c = c.child(
        text("Conditions")
            .font(theme::LABEL_BTN)
            .color(PAL.text)
            .pt(4.0),
    );
    let title_of = |id: u32| {
        s.entries
            .iter()
            .find(|x| x.id == id)
            .map(|x| x.title.clone())
    };
    for (i, cond) in flt.conditions.iter().enumerate() {
        let el_key = format!("el/{i}");
        let op_key = format!("op/{i}");
        let mut line = row()
            .key(format!("set/feeds/editor/cond/{i}"))
            .gap(6.0)
            .center_y()
            .flex_wrap()
            .child(small(
                format!("set/feeds/editor/cond/{i}/element"),
                cond.element.label(),
                false,
                {
                    let k = el_key.clone();
                    move |s| s.feeds_picker(k.clone())
                },
            ))
            .child(small(
                format!("set/feeds/editor/cond/{i}/op"),
                cond.op.label(),
                false,
                {
                    let k = op_key.clone();
                    move |s| s.feeds_picker(k.clone())
                },
            ));
        let choices: Option<Vec<(String, String)>> = match cond.element {
            Element::UserStatus => Some(
                (0..=5)
                    .map(|n| {
                        (
                            n.to_string(),
                            taiga::value_label(cond.element, &n.to_string(), &title_of),
                        )
                    })
                    .collect(),
            ),
            Element::MetaStatus => Some(
                (1..=3)
                    .map(|n| {
                        (
                            n.to_string(),
                            taiga::value_label(cond.element, &n.to_string(), &title_of),
                        )
                    })
                    .collect(),
            ),
            Element::MetaType => Some(
                (1..=6)
                    .map(|n| {
                        (
                            n.to_string(),
                            taiga::value_label(cond.element, &n.to_string(), &title_of),
                        )
                    })
                    .collect(),
            ),
            Element::LocalEpisodeAvailable => Some(vec![
                ("True".into(), "True".into()),
                ("False".into(), "False".into()),
            ]),
            _ => None,
        };
        match choices {
            Some(v) => {
                for (val, lab) in v {
                    let on = cond.value.eq_ignore_ascii_case(&val);
                    line = line.child(pill(
                        format!("set/feeds/editor/cond/{i}/v/{val}"),
                        lab,
                        on,
                        move |s| s.feeds_cond_value(i, val.clone()),
                    ));
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
                line = line.child(
                    field(
                        format!("set/feeds/editor/cond/{i}/value"),
                        &edit,
                        hint,
                        false,
                        move |s| value_edit(s, i),
                        |_| {},
                    )
                    .min_w(140.0),
                );
            }
        }
        line = line.child(small(
            format!("set/feeds/editor/cond/{i}/remove"),
            "\u{d7}",
            false,
            move |s| s.feeds_cond_remove(i),
        ));
        c = c.child(line);
        if e.picker.as_deref() == Some(&el_key) {
            let mut p = row()
                .key(format!("set/feeds/editor/cond/{i}/elements"))
                .gap(6.0)
                .flex_wrap()
                .pl(12.0);
            for x in Element::ALL {
                p = p.child(pill(
                    format!("set/feeds/editor/cond/{i}/element/{}", x.code()),
                    x.label().into(),
                    cond.element == x,
                    move |s| s.feeds_cond_element(i, x),
                ));
            }
            c = c.child(p);
        }
        if e.picker.as_deref() == Some(&op_key) {
            let mut p = row()
                .key(format!("set/feeds/editor/cond/{i}/ops"))
                .gap(6.0)
                .flex_wrap()
                .pl(12.0);
            for x in Operator::ALL {
                p = p.child(pill(
                    format!("set/feeds/editor/cond/{i}/op/{}", x.code()),
                    x.label().into(),
                    cond.op == x,
                    move |s| s.feeds_cond_op(i, x),
                ));
            }
            c = c.child(p);
        }
    }
    c = c.child(row().child(small(
        "set/feeds/editor/cond-add".into(),
        "Add condition",
        false,
        State::feeds_cond_add,
    )));
    c = c.child(text("Values may use %title%, %watched%, %total%, %episode%, %group%, %resolution%, %version%, %status%, %id%.").font(theme::LABEL).color(PAL.dim).wrap());
    // Anime.
    c = c.child(
        text("Limit to anime (none: every anime)")
            .font(theme::LABEL_BTN)
            .color(PAL.text)
            .pt(4.0),
    );
    let mut chips = row().key("set/feeds/editor/anime").gap(6.0).flex_wrap();
    for &id in &flt.anime_ids {
        let t = title_of(id).unwrap_or_else(|| format!("Anime {id}"));
        chips = chips.child(pill(
            format!("set/feeds/editor/anime/{id}"),
            format!("{t} \u{d7}"),
            true,
            move |s| s.feeds_filter_anime(id, false),
        ));
    }
    c = c.child(chips);
    c = c.child(row().gap(8.0).center_y().child(field(
        "set/feeds/editor/anime-find".into(),
        &e.anime_query,
        "Find an anime on your list",
        false,
        |s| &mut s.feeds.editor.anime_query,
        |_| {},
    )));
    let q = e.anime_query.text.trim().to_lowercase();
    if !q.is_empty() {
        let mut hits = row()
            .key("set/feeds/editor/anime-hits")
            .gap(6.0)
            .flex_wrap();
        for it in s
            .feeds
            .list
            .iter()
            .filter(|it| {
                it.media.title.to_lowercase().contains(&q) && !flt.anime_ids.contains(&it.media.id)
            })
            .take(8)
        {
            let id = it.media.id;
            hits = hits.child(pill(
                format!("set/feeds/editor/anime-add/{id}"),
                it.media.title.clone(),
                false,
                move |s| s.feeds_filter_anime(id, true),
            ));
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
            .child(small(
                "set/feeds/editor/cancel".into(),
                "Cancel",
                false,
                State::feeds_filter_cancel,
            ))
            .child(small(
                "set/feeds/editor/save".into(),
                "Save filter",
                true,
                State::feeds_filter_save,
            )),
    )
}

// ---------------------------------------------------------------------------- the page

const W_CHECK: f32 = 22.0;
const W_EP: f32 = 64.0;
const W_GROUP: f32 = 110.0;
const W_SIZE: f32 = 70.0;
const W_VIDEO: f32 = 120.0;
const W_NUM: f32 = 34.0;
const W_DATE: f32 = 96.0;
const ROW_H: f32 = 34.0;

/// The quick chips over the table, remembered in `[feeds] view` (Kotora's, beside Taiga's search).
pub const VIEWS: [&str; 5] = ["all", "list", "new", "selected", "discarded"];

pub fn view_label(v: &str) -> &'static str {
    match v {
        "list" => "On my list",
        "new" => "New",
        "selected" => "Selected",
        "discarded" => "Discarded",
        _ => "All",
    }
}

fn in_view(v: &str, e: &Entry) -> bool {
    match v {
        "list" => e.anime_id.is_some(),
        "new" => is_new(e),
        "selected" => e.state == ItemState::Selected,
        "discarded" => e.state.is_discarded(),
        _ => true,
    }
}

/// Taiga's columns (`TorrentListColumn`); `State` is its own order (Selected, blank, discarded).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortCol {
    #[default]
    State,
    Release,
    Episode,
    Group,
    Size,
    Video,
    Seeders,
    Leechers,
    Downloads,
    Description,
    Date,
}

impl SortCol {
    /// Taiga starts these descending (`LVN_COLUMNCLICK`).
    fn starts_descending(self) -> bool {
        matches!(
            self,
            SortCol::Episode
                | SortCol::Size
                | SortCol::Seeders
                | SortCol::Leechers
                | SortCol::Downloads
                | SortCol::Date
        )
    }
}

/// A line of the table: a category header (Taiga's list groups), an anime header (Kotora: the
/// anime once), an item, or the open item's actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Slot {
    Category(Category, usize),
    Anime(String, usize),
    Item(usize),
    Actions(usize),
}

fn matches_query(s: &State, e: &Entry, words: &[String]) -> bool {
    if words.is_empty() {
        return true;
    }
    let hay = format!(
        "{} {} {} {}",
        anime_text(s, e),
        e.item.title,
        e.release.parsed.group.as_deref().unwrap_or(""),
        e.episode_text()
    )
    .to_lowercase();
    words.iter().all(|w| hay.contains(w.as_str()))
}

/// Taiga's `RefreshList`: unidentified items with no anime title go to Other.
fn category(e: &Entry) -> Category {
    if e.anime_id.is_none() && e.anime_title.is_empty() {
        Category::Other
    } else {
        e.category
    }
}

impl State {
    /// The table's lines: chip and filter words, Taiga's groups (Anime, Batch, Other), the anime
    /// once inside them while in Taiga's order or by release, the sort, the open row's actions.
    pub fn feeds_refilter(&mut self) {
        let words: Vec<String> = self
            .feeds
            .query
            .text
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let view = self.feeds.cfg.view.clone();
        let t = self.feeds_table();
        let mut v: Vec<usize> = (0..t.len())
            .filter(|&i| {
                t[i].state != ItemState::DiscardedHidden
                    && in_view(&view, &t[i])
                    && matches_query(self, &t[i], &words)
            })
            .collect();
        let (col, desc) = self.feeds.sort;
        let by_anime = matches!(col, SortCol::State | SortCol::Release);
        let num = |e: &Entry| -> u64 {
            match col {
                SortCol::Episode => {
                    u64::from(e.release.episodes.map_or(0, |x| x.0)) * 100
                        + u64::from(e.release.version.unwrap_or(1))
                }
                SortCol::Size => e.item.size.unwrap_or(0),
                SortCol::Seeders => u64::from(e.item.seeders.unwrap_or(0)),
                SortCol::Leechers => u64::from(e.item.leechers.unwrap_or(0)),
                SortCol::Downloads => u64::from(e.item.downloads.unwrap_or(0)),
                SortCol::Date => e.item.published.unwrap_or(0) as u64,
                SortCol::Video => u64::from(
                    e.release
                        .parsed
                        .resolution
                        .as_deref()
                        .and_then(taiga::resolution_height)
                        .unwrap_or(0),
                ),
                _ => 0,
            }
        };
        let text = |e: &Entry| -> String {
            match col {
                SortCol::Release => e.item.title.to_lowercase(),
                SortCol::Group => e
                    .release
                    .parsed
                    .group
                    .clone()
                    .unwrap_or_default()
                    .to_lowercase(),
                SortCol::Description => e.item.description.to_lowercase(),
                _ => String::new(),
            }
        };
        let cat_rank = |c: Category| Category::ALL.iter().position(|x| *x == c).unwrap_or(0);
        // Stable: Taiga's state order stays inside equal keys.
        v.sort_by(|&a, &b| {
            let (x, y) = (&t[a], &t[b]);
            let mut o = cat_rank(category(x)).cmp(&cat_rank(category(y)));
            if by_anime {
                o = o.then_with(|| {
                    anime_text(self, x)
                        .to_lowercase()
                        .cmp(&anime_text(self, y).to_lowercase())
                });
            }
            if col != SortCol::State {
                let k = (text(x), num(x)).cmp(&(text(y), num(y)));
                o = o.then(if desc { k.reverse() } else { k });
            }
            o
        });
        let mut slots = Vec::with_capacity(v.len() + 8);
        let mut last_cat = None;
        let mut last_anime = None;
        for &i in &v {
            let e = &t[i];
            let c = category(e);
            if last_cat != Some(c) {
                slots.push(Slot::Category(
                    c,
                    v.iter().filter(|&&j| category(&t[j]) == c).count(),
                ));
                last_cat = Some(c);
                last_anime = None;
            }
            if by_anime && c != Category::Other {
                let a = anime_text(self, e);
                if last_anime.as_deref() != Some(a.as_str()) {
                    let n = v
                        .iter()
                        .filter(|&&j| category(&t[j]) == c && anime_text(self, &t[j]) == a)
                        .count();
                    slots.push(Slot::Anime(a.clone(), n));
                    last_anime = Some(a);
                }
            }
            slots.push(Slot::Item(i));
            if self.feeds.open_row.as_deref() == Some(e.item.key().as_str()) {
                slots.push(Slot::Actions(i));
            }
        }
        self.feeds.visible = v;
        self.feeds.slots = slots;
    }

    pub fn set_feeds_view(&mut self, v: &'static str) {
        self.feeds.cfg.view = v.into();
        self.feeds_set("view", config::quote(v));
        self.feeds_refilter();
    }

    /// A header click (Taiga's `LVN_COLUMNCLICK`): the column in its first order, again the
    /// other way.
    pub fn feeds_sort_by(&mut self, c: SortCol) {
        self.feeds.sort = match self.feeds.sort {
            (x, d) if x == c => (c, !d),
            _ => (c, c.starts_descending()),
        };
        self.feeds_refilter();
    }

    pub fn feeds_clear_query(&mut self) {
        self.feeds.query = TextEdit::default();
        self.feeds_refilter();
    }

    /// A row click opens its actions under it; again (or Esc) closes them.
    pub fn feeds_open_row(&mut self, key: &str) {
        self.feeds.open_row = if self.feeds.open_row.as_deref() == Some(key) {
            None
        } else {
            Some(key.to_string())
        };
        self.feeds.ctx = None;
        self.feeds_refilter();
    }

    pub fn feeds_close_row(&mut self) -> bool {
        if self.feeds.ctx.take().is_some() {
            return true;
        }
        if self.feeds.open_row.take().is_some() {
            self.feeds_refilter();
            return true;
        }
        false
    }

    /// Check new torrents (Taiga: clears the search box and reads the source).
    pub fn feeds_check_new(&mut self) {
        self.feeds.search_box = TextEdit::default();
        if self.feeds.search.take().is_some() {
            self.feeds_reselect();
        }
        self.feeds_check_now();
        self.shows_refresh(true);
    }

    /// One of the right-click menu's (or the action row's) actions, by Taiga's names.
    pub fn feeds_act(&mut self, key: &str, action: &str) {
        let Some(e) = self
            .feeds_table()
            .iter()
            .find(|e| e.item.key() == key)
            .cloned()
        else {
            return;
        };
        self.feeds.ctx = None;
        match action {
            "DownloadTorrent" => self.feeds_download(vec![key.to_string()]),
            "Info" => {
                if let Some(x) = e
                    .anime_id
                    .and_then(|id| self.entries.iter().find(|x| x.id == id))
                {
                    let stub = crate::details::Stub::from_entry(x);
                    self.open_details(stub);
                }
            }
            "TorrentInfo" => {
                if let Some(p) = e.item.page.clone() {
                    crate::online::open_url(self, p);
                }
            }
            "DiscardTorrent" => self.feeds_discard_into_archive(vec![key.to_string()]),
            "DiscardTorrents" => {
                if let Some(id) = e.anime_id {
                    let keys = self
                        .feeds_table()
                        .iter()
                        .filter(|x| x.anime_id == Some(id))
                        .map(|x| x.item.key())
                        .collect();
                    self.feeds_discard_anime(id);
                    self.feeds_mark(keys, ItemState::DiscardedNormal);
                }
            }
            "SelectFansub" => self.feeds_select_fansub(key),
            "MoreTorrents" => {
                let t = e
                    .anime_id
                    .map(|id| self.feeds_title(id))
                    .unwrap_or(e.anime_title.clone());
                self.feeds_search_title(t, e.anime_id.unwrap_or(0));
            }
            "SearchService" => {
                let t = if e.anime_title.is_empty() {
                    e.item.title.clone()
                } else {
                    e.anime_title.clone()
                };
                self.find.replace_text(&t);
                self.open_search();
            }
            "Select" => self.feeds_mark(vec![key.to_string()], ItemState::Selected),
            _ => {}
        }
    }
}

/// Taiga's right-click menu (`res/menu.xml` TorrentListRightClick), its order and enabling.
pub fn menu_items(e: &Entry) -> Vec<Option<(&'static str, &'static str, bool)>> {
    let id = e.anime_id.is_some();
    let group = e
        .release
        .parsed
        .group
        .as_deref()
        .is_some_and(|g| !g.is_empty());
    vec![
        Some(("DownloadTorrent", "Download torrent", true)),
        Some(("Info", "View anime information", id)),
        Some((
            "TorrentInfo",
            "View torrent information",
            e.item.page.is_some(),
        )),
        None,
        Some(("DiscardTorrent", "Discard", true)),
        Some((
            "DiscardTorrents",
            "Quick filters: Discard all torrents for this anime",
            id,
        )),
        Some((
            "SelectFansub",
            "Quick filters: Select this fansub group for this anime",
            id && group,
        )),
        None,
        Some((
            "MoreTorrents",
            "Search for more torrents with this title",
            true,
        )),
        Some(("SearchService", "Search for anime with this title", true)),
    ]
}

/// The right-click menu as an overlay (drawn over the page, closed by a click outside).
pub fn ctx_view(s: &State) -> Option<El> {
    let (key, x, y) = s.feeds.ctx.clone()?;
    let e = s.feeds_table().iter().find(|e| e.item.key() == key)?;
    let mut m = col()
        .key("feeds/ctx")
        .w(330.0)
        .py(4.0)
        .radius(theme::R_MD)
        .fill(PAL.raised)
        .border(1.0, PAL.border_strong)
        .absolute(x, y);
    m = m.child(
        text(e.item.title.clone())
            .font(theme::CAPTION.tracking(0.0))
            .color(PAL.faint)
            .truncate()
            .px(10.0)
            .py(4.0),
    );
    for it in menu_items(e) {
        m = match it {
            None => m.child(el().h(1.0).w_full().my(4.0).fill(PAL.border)),
            Some((action, label, on)) => {
                let k = key.clone();
                let r = row()
                    .key(format!("feeds/ctx/{action}"))
                    .role("menuitem")
                    .label(label)
                    .h(26.0)
                    .px(10.0)
                    .center_y()
                    .radius(theme::R_SM)
                    .child(
                        text(label)
                            .font(if action == "DownloadTorrent" {
                                theme::BODY_MED
                            } else {
                                theme::BODY
                            })
                            .color(if on { PAL.text } else { PAL.faint }),
                    );
                m.child(if on {
                    r.hover(PAL.hover())
                        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                            tx.instant();
                            s.feeds_act(&k, action);
                        })
                } else {
                    r
                })
            }
        };
    }
    Some(m.modal(true, |s: &mut State| s.feeds.ctx = None))
}

/// "Search for torrents" (Taiga's main toolbar search on Torrents) and "Filter these results".
fn search_row(s: &State) -> El {
    let f = &s.feeds;
    let boxed = |key: &'static str, w: f32, input: El| {
        row()
            .key(format!("{key}/box"))
            .role("textbox")
            .w(w)
            .min_w(0.0)
            .h(28.0)
            .px(10.0)
            .center_y()
            .radius(theme::R_SM)
            .fill(PAL.field)
            .border(1.0, PAL.border_strong)
            .child(input)
    };
    let hint = if cfg!(target_os = "macos") {
        "Search for torrents  \u{2318}F"
    } else {
        "Search for torrents  Ctrl+F"
    };
    let search = boxed(
        "feeds/search-box",
        280.0,
        text_input(&f.search_box, TextStyle::new(12.0, 400.0))
            .key("feeds/search-box")
            .grow(1.0)
            .min_w(0.0)
            .single_line()
            .color(PAL.text)
            .placeholder(field_hint(&f.search_box, hint), PAL.faint)
            .on_edit(|s: &mut State| &mut s.feeds.search_box)
            .on_input_key(crate::ui::field_page_key)
            .on_submit(|s: &mut State, tx: &mut Tx| {
                tx.blur();
                let t = s.feeds.search_box.text.clone();
                s.feeds_search_title(t, 0);
            }),
    );
    let filter = boxed(
        "feeds/filter",
        220.0,
        text_input(&f.query, TextStyle::new(12.0, 400.0))
            .key("feeds/filter")
            .grow(1.0)
            .min_w(0.0)
            .single_line()
            .color(PAL.text)
            .placeholder(field_hint(&f.query, "Filter these results"), PAL.faint)
            .on_edit(|s: &mut State| &mut s.feeds.query)
            .on_input_key(|s: &mut State, k: &rune::KeyEvent, tx: &mut Tx| {
                if k.key == rune::Key::Escape {
                    s.feeds_clear_query();
                    tx.blur();
                    return true;
                }
                false
            })
            .on_input_change(|s: &mut State, _tx: &mut Tx| s.feeds_refilter()),
    );
    let t = s.feeds_table();
    let mut r = row()
        .key("feeds/filter-bar")
        .gap(6.0)
        .pb(8.0)
        .center_y()
        .flex_wrap()
        .child(search)
        .child(filter);
    for v in VIEWS {
        let n = t
            .iter()
            .filter(|e| e.state != ItemState::DiscardedHidden && in_view(v, e))
            .count();
        let label = format!("{} {n}", view_label(v));
        r = r.child(
            crate::ui::cell(
                format!("feeds/view/{v}"),
                label.clone(),
                f.cfg.view == v,
                move |s| s.set_feeds_view(v),
            )
            .child(text(label).font(theme::LABEL).color(PAL.text)),
        );
    }
    r
}

#[derive(Clone)]
struct Cols {
    group: bool,
    video: bool,
    peers: bool,
    desc: bool,
    date: bool,
}

fn cols(width: f32) -> Cols {
    Cols {
        group: width > 640.0,
        video: width > 760.0,
        peers: width > 880.0,
        desc: width > 1180.0,
        date: width > 1020.0,
    }
}

fn cell_text(t: String, w: Option<f32>, color: rune::Color) -> El {
    let x = text(t).font(theme::LABEL).color(color).truncate();
    match w {
        Some(w) => x.w(w),
        None => x.grow(1.0).min_w(0.0),
    }
}

fn head_sort(s: &State, t: &str, w: Option<f32>, c: SortCol) -> El {
    let (on, desc) = s.feeds.sort;
    let label = if on == c {
        format!("{t} {}", if desc { "\u{2193}" } else { "\u{2191}" })
    } else {
        t.to_string()
    };
    let x = text(label)
        .font(theme::CAPTION)
        .color(if on == c { PAL.text } else { PAL.faint })
        .truncate();
    let b = row()
        .key(format!("feeds/sort/{c:?}"))
        .role("button")
        .label(format!("Sort by {t}"))
        .center_y()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.feeds_sort_by(c);
        });
    match w {
        Some(w) => b.w(w).child(x),
        None => b.grow(1.0).min_w(0.0).child(x),
    }
}

fn header(s: &State, c: &Cols) -> El {
    let mut r = row()
        .key("feeds/head")
        .gap(theme::COL_GAP)
        .h(28.0)
        .center_y()
        .px(10.0)
        .child(el().w(W_CHECK))
        .child(head_sort(s, "RELEASE", None, SortCol::Release))
        .child(head_sort(s, "EPISODE", Some(W_EP), SortCol::Episode));
    if c.group {
        r = r.child(head_sort(s, "GROUP", Some(W_GROUP), SortCol::Group));
    }
    r = r.child(head_sort(s, "SIZE", Some(W_SIZE), SortCol::Size));
    if c.video {
        r = r.child(head_sort(s, "VIDEO", Some(W_VIDEO), SortCol::Video));
    }
    if c.peers {
        r = r
            .child(head_sort(s, "S", Some(W_NUM), SortCol::Seeders))
            .child(head_sort(s, "L", Some(W_NUM), SortCol::Leechers))
            .child(head_sort(s, "D", Some(W_NUM), SortCol::Downloads));
    }
    if c.desc {
        r = r.child(head_sort(
            s,
            "DESCRIPTION",
            Some(200.0),
            SortCol::Description,
        ));
    }
    if c.date {
        r = r.child(head_sort(s, "DATE", Some(W_DATE), SortCol::Date));
    }
    r
}

/// Why an item is dimmed or kept: the filter that discarded it, the history, no anime.
pub fn reason(e: &Entry) -> Option<String> {
    if !e.state.is_discarded() && !e.why.is_empty() {
        return Some(e.why.clone());
    }
    if e.state.is_discarded() {
        return Some(if e.why.is_empty() {
            "Discarded".into()
        } else {
            e.why.clone()
        });
    }
    e.anime_id.is_none().then(|| "Not on your list".into())
}

/// The anime: the list's title, else the parsed one (Taiga's Anime title column).
fn anime_text(s: &State, e: &Entry) -> String {
    match e.anime_id {
        Some(id) => s.feeds_title(id),
        None if e.anime_title.is_empty() => e.item.title.clone(),
        None => e.anime_title.clone(),
    }
}

fn date_text(t: Option<i64>) -> String {
    let Some(t) = t else { return String::new() };
    let c = kotora_core::time::Clock::at(t, kotora_core::time::local_offset(t));
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        c.year,
        c.month,
        c.day,
        c.minute / 60,
        c.minute % 60
    )
}

fn table_row(s: &State, e: &Entry, c: &Cols, grouped: bool) -> El {
    let key = e.item.key();
    let checked = e.state == ItemState::Selected;
    let open = s.feeds.open_row.as_deref() == Some(&key);
    let busy = s.feeds.busy.contains(&key);
    // Taiga greys only the deactivated ones.
    let fg = if e.state == ItemState::DiscardedInactive {
        PAL.faint
    } else {
        PAL.text
    };
    let mut name = row().gap(6.0).grow(1.0).min_w(0.0).center_y().child(
        text(e.item.title.clone())
            .font(theme::LABEL)
            .color(fg)
            .truncate()
            .min_w(0.0),
    );
    if !grouped && category(e) != Category::Other {
        name = name.child(
            text(anime_text(s, e))
                .font(theme::LABEL)
                .color(PAL.faint)
                .truncate()
                .max_w(180.0),
        );
    }
    if let Some(why) = reason(e) {
        name = name.child(
            text(why)
                .key(format!("feeds/why/{key}"))
                .font(theme::SMALL)
                .color(PAL.faint)
                .truncate()
                .max_w(220.0),
        );
    }
    if busy {
        name = name.child(
            text("Downloading\u{2026}")
                .font(theme::LABEL)
                .color(PAL.accent_hi),
        );
    }
    let (k2, k3, k4) = (key.clone(), key.clone(), key.clone());
    let mut r = row()
        .key(format!("feeds/item/{key}"))
        .role("row")
        .label(e.item.title.clone())
        .gap(theme::COL_GAP)
        .h(ROW_H)
        .px(10.0)
        .center_y()
        .radius(theme::R_SM)
        .hover(PAL.hover())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.feeds_open_row(&k3);
        })
        .on_secondary_press(move |s: &mut State, p: rune::PointerPress, tx: &mut Tx| {
            tx.instant();
            s.feeds.ctx = Some((k4.clone(), p.x, p.y));
        })
        .child(check_box(
            format!("feeds/check/{key}"),
            "Download",
            checked,
            move |s| s.feeds_toggle(&k2),
        ))
        .child(name)
        .child(cell_text(
            e.episode_text(),
            Some(W_EP),
            if e.new_episode {
                PAL.accent_hi
            } else {
                PAL.dim
            },
        ));
    if checked {
        r = r.fill(PAL.tint(PAL.accent, 0.10));
    }
    if open {
        r = r.fill(PAL.tint(PAL.accent, 0.16));
    }
    if c.group {
        r = r.child(cell_text(
            e.release.parsed.group.clone().unwrap_or_default(),
            Some(W_GROUP),
            PAL.dim,
        ));
    }
    r = r.child(cell_text(
        e.item.size.map(size_text).unwrap_or_default(),
        Some(W_SIZE),
        PAL.dim,
    ));
    if c.video {
        r = r.child(cell_text(e.video_text(), Some(W_VIDEO), PAL.dim));
    }
    if c.peers {
        let n = |v: Option<u32>| v.map_or(String::new(), |n| n.to_string());
        r = r
            .child(cell_text(n(e.item.seeders), Some(W_NUM), PAL.dim))
            .child(cell_text(n(e.item.leechers), Some(W_NUM), PAL.dim))
            .child(cell_text(n(e.item.downloads), Some(W_NUM), PAL.dim));
    }
    if c.desc {
        r = r.child(cell_text(e.item.description.clone(), Some(200.0), PAL.dim));
    }
    if c.date {
        r = r.child(cell_text(
            date_text(e.item.published),
            Some(W_DATE),
            PAL.dim,
        ));
    }
    r
}

/// The open row's actions, right under it: the menu's actions as buttons.
fn row_actions(e: &Entry) -> El {
    let key = e.item.key();
    let mut b = row()
        .key(format!("feeds/open/{key}"))
        .h(ROW_H)
        .gap(6.0)
        .pl(42.0)
        .center_y();
    for it in menu_items(e).into_iter().flatten() {
        let (action, label, on) = it;
        if !on {
            continue;
        }
        let short = match action {
            "DownloadTorrent" => "Download",
            "Info" => "Anime info",
            "TorrentInfo" => "Torrent page",
            "DiscardTorrent" => "Discard",
            "DiscardTorrents" => "Discard this anime",
            "SelectFansub" => "Select this group",
            "MoreTorrents" => "More torrents",
            "SearchService" => "Search anime",
            _ => label,
        };
        let k = key.clone();
        b = b.child(small(
            format!("feeds/act/{action}/{key}"),
            short,
            action == "DownloadTorrent",
            move |s| s.feeds_act(&k, action),
        ));
    }
    b
}

fn slot_el(s: &State, slot: Option<&Slot>, c: &Cols) -> El {
    let t = s.feeds_table();
    let grouped = matches!(s.feeds.sort.0, SortCol::State | SortCol::Release);
    match slot {
        Some(Slot::Category(cat, n)) => row()
            .h(ROW_H)
            .px(10.0)
            .gap(8.0)
            .center_y()
            .child(
                text(cat.label().to_string())
                    .font(theme::SECTION)
                    .color(PAL.text),
            )
            .child(text(n.to_string()).font(theme::LABEL).color(PAL.dim)),
        Some(Slot::Anime(a, n)) => row()
            .h(ROW_H)
            .pl(42.0)
            .gap(8.0)
            .center_y()
            .child(
                text(a.clone())
                    .font(theme::BODY_MED)
                    .color(PAL.text)
                    .truncate()
                    .min_w(0.0),
            )
            .child(text(format!("{n}")).font(theme::LABEL).color(PAL.dim)),
        Some(Slot::Item(i)) => t
            .get(*i)
            .map_or_else(|| el().h(ROW_H), |e| table_row(s, e, c, grouped)),
        Some(Slot::Actions(i)) => t.get(*i).map_or_else(|| el().h(ROW_H), row_actions),
        None => el().h(ROW_H),
    }
}

fn slot_key(s: &State, r: usize) -> String {
    let t = s.feeds_table();
    match s.feeds.slots.get(r) {
        Some(Slot::Category(c, _)) => format!("feeds/cat/{}", c.label()),
        Some(Slot::Anime(a, _)) => format!("feeds/anime/{a}"),
        Some(Slot::Item(i)) => t.get(*i).map_or_else(
            || format!("gone-{r}"),
            |e| format!("feeds/item/{}", e.item.key()),
        ),
        Some(Slot::Actions(i)) => t.get(*i).map_or_else(
            || format!("gone-{r}"),
            |e| format!("feeds/actions/{}", e.item.key()),
        ),
        None => format!("gone-{r}"),
    }
}

fn history_view(s: &State) -> El {
    let mut c = card("feeds/history", "Torrent history");
    let titles = s
        .feeds
        .archive
        .as_ref()
        .map(|a| a.titles.clone())
        .unwrap_or_default();
    if titles.is_empty() {
        return c.child(
            text("Nothing downloaded yet.")
                .key("feeds/history/none")
                .font(theme::BODY)
                .color(PAL.dim)
                .py(8.0),
        );
    }
    c = c.child(
        text("Feeds discard these when they show them again. Remove one to see it again.")
            .font(theme::LABEL)
            .color(PAL.dim),
    );
    for (n, t) in titles.iter().rev().take(300).enumerate() {
        let t2 = t.clone();
        c = c.child(
            row()
                .key(format!("feeds/history/{n}"))
                .gap(10.0)
                .py(3.0)
                .center_y()
                .child(
                    text(t.clone())
                        .font(theme::BODY)
                        .color(PAL.text)
                        .truncate()
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(small(
                    format!("feeds/history/{n}/remove"),
                    "Remove",
                    false,
                    move |s| s.feeds_forget(&t2),
                )),
        );
    }
    c.child(row().pt(8.0).child(spacer()).child(small(
        "feeds/history/clear".into(),
        "Clear history",
        false,
        State::feeds_clear_history,
    )))
}

fn tab(key: &str, label: &str, on: bool, f: impl Fn(&mut State) + 'static) -> El {
    crate::ui::cell(key.to_string(), label.to_string(), on, f).child(
        text(label.to_string())
            .font(theme::LABEL_BTN)
            .color(if on { PAL.text } else { PAL.dim }),
    )
}

/// While a search's results replace the feeds': its title, the count, Back to feeds.
fn search_bar(s: &State) -> Option<El> {
    let sv = s.feeds.search.as_ref()?;
    let mut bar = row()
        .key("feeds/search")
        .gap(10.0)
        .center_y()
        .px(theme::PAGE_PAD)
        .pb(8.0)
        .child(
            text(format!("Search: {}", sv.title))
                .font(theme::BODY_MED)
                .color(PAL.text)
                .truncate()
                .min_w(0.0),
        );
    bar = if sv.loading {
        bar.child(
            text("Searching torrents\u{2026}")
                .key("feeds/search/loading")
                .font(theme::LABEL)
                .color(PAL.dim),
        )
    } else if let Some(e) = &sv.error {
        bar.child(
            text(mask(e))
                .key("feeds/search/error")
                .font(theme::LABEL)
                .color(PAL.warn())
                .truncate()
                .min_w(0.0),
        )
    } else {
        bar.child(
            text(format!("{} results", sv.entries.len()))
                .font(theme::LABEL)
                .color(PAL.dim),
        )
    };
    Some(bar.child(spacer()).child(small(
        "feeds/search/close".into(),
        "Back to feeds",
        false,
        |s| {
            s.feeds.search = None;
            s.feeds.search_box = TextEdit::default();
            s.feeds_reselect();
        },
    )))
}

fn scroller(key: &str, inner: f32, body: El) -> El {
    col()
        .key(key.to_string())
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .grow(1.0)
        .min_h(0.0)
        .child(
            col()
                .px(theme::PAGE_PAD)
                .pt(4.0)
                .pb(theme::PAGE_PAD)
                .w(inner + 2.0 * theme::PAGE_PAD)
                .child(body),
        )
}

/// The Torrents page: Your shows (Kotora's), All releases (Taiga's torrent list), History.
pub fn page(s: &State, width: f32) -> El {
    let f = &s.feeds;
    let inner = (width - 2.0 * theme::PAGE_PAD).max(320.0);
    let mut tools = row()
        .key("feeds/tools")
        .gap(8.0)
        .px(theme::PAGE_PAD)
        .pb(8.0)
        .center_y()
        .flex_wrap()
        .child(tab("feeds/tab/shows", "Your shows", f.tab == 0, |s| {
            s.feeds.tab = 0
        }))
        .child(tab("feeds/tab/all", "All releases", f.tab == 1, |s| {
            s.feeds.tab = 1
        }))
        .child(tab("feeds/tab/history", "History", f.tab == 2, |s| {
            s.feeds.tab = 2
        }))
        .child(spacer());
    let can_check = !f.cfg.sources.is_empty() || !f.cfg.search.trim().is_empty();
    if f.tab != 2 && can_check {
        tools = tools.child(small(
            "feeds/check".into(),
            "Check new torrents",
            false,
            State::feeds_check_new,
        ));
    }
    let mut page = col()
        .key("page/feeds")
        .w_full()
        .h_full()
        .child(crate::ui::top(s, "Torrents", Some(s.feeds_count()), false));
    let status = f.status.as_ref().or(f.said.as_ref()).map(|t| {
        text(t.clone())
            .key("feeds/said")
            .font(theme::LABEL)
            .color(PAL.dim)
            .wrap()
            .px(theme::PAGE_PAD)
            .pb(6.0)
    });
    match f.tab {
        2 => page
            .child(tools)
            .child(scroller("feeds-scroll", inner, history_view(s))),
        0 => page.child(tools).children(status).child(scroller(
            "feeds-scroll",
            inner,
            crate::shows::view(s, inner),
        )),
        _ => {
            let checked = s
                .feeds_table()
                .iter()
                .filter(|e| e.state == ItemState::Selected)
                .count();
            tools = tools
                .child(small(
                    "feeds/download".into(),
                    &format!("Download marked torrents ({checked})"),
                    checked > 0,
                    move |s| {
                        if checked > 0 {
                            s.feeds_download_marked();
                        }
                    },
                ))
                .child(small(
                    "feeds/discard".into(),
                    "Discard all",
                    false,
                    State::feeds_discard_marked,
                ))
                .child(small(
                    "feeds/select-new".into(),
                    "Select all new",
                    false,
                    State::feeds_select_new,
                ))
                .child(small("feeds/settings".into(), "Settings", false, |s| {
                    crate::settings::open(s, crate::settings::Section::Feeds)
                }));
            page = page.child(tools).children(search_bar(s)).children(status);
            let c = cols(inner);
            let mut fixed = col()
                .key("feeds/table")
                .px(theme::PAGE_PAD)
                .w(inner + 2.0 * theme::PAGE_PAD)
                .child(search_row(s))
                .child(header(s, &c))
                .child(el().h(1.0).w_full().fill(PAL.border));
            let t = s.feeds_table();
            if f.cfg.sources.is_empty() && f.search.is_none() {
                fixed = fixed.child(text("No feeds yet. Search for torrents above, or add a feed in Settings \u{2192} Feeds.").key("feeds/empty").font(theme::BODY).color(PAL.dim).py(12.0).px(10.0));
            } else if f.slots.is_empty() {
                let waiting = f.search.as_ref().map_or(
                    f.lines.is_empty() || f.lines.values().any(|l| l.checking),
                    |sv| sv.loading,
                );
                let line = if t.is_empty() && waiting {
                    "Checking new torrents\u{2026}"
                } else if t.is_empty() {
                    "No new torrents found."
                } else {
                    "No torrents match."
                };
                fixed = fixed.child(
                    text(line)
                        .key("feeds/none")
                        .font(theme::BODY)
                        .color(PAL.dim)
                        .py(12.0)
                        .px(10.0),
                );
            }
            page = page.child(fixed);
            let n = f.slots.len();
            page.child(
                col()
                    .key("feeds-scroll")
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .grow(1.0)
                    .min_h(0.0)
                    .child(
                        col()
                            .key("feeds/rows")
                            .px(theme::PAGE_PAD)
                            .w(inner + 2.0 * theme::PAGE_PAD)
                            .growth(rune::GrowthSpec::default())
                            .virtual_rows(
                                n,
                                ROW_H,
                                |s: &State, r| slot_key(s, r).into(),
                                move |s: &State, r| slot_el(s, s.feeds.slots.get(r), &c),
                            ),
                    ),
            )
        }
    }
}

/// Details → "Torrents": the next new episode's items, Download, the preferred group, Search.
pub fn details_card(s: &State, id: u32) -> Option<El> {
    if !s.feeds.cfg.enabled {
        return None;
    }
    let mine: Vec<&Entry> = s
        .feeds
        .entries
        .iter()
        .filter(|e| e.anime_id == Some(id) && !e.state.is_discarded())
        .collect();
    let search = s.feeds_can_search(id).then(|| {
        small(
            format!("det/search-releases/{id}"),
            "Search torrents",
            false,
            move |s| s.feeds_search(id),
        )
    });
    let groups = taiga::fansub_groups(&s.feeds.filters, id);
    if mine.is_empty() && search.is_none() && groups.is_empty() {
        return None;
    }
    let next = s.feeds_badge(id);
    let mut c = card("det-feeds", "Torrents");
    match next {
        Some((a, b)) => {
            c = c.child(
                text(format!("{} is in your feeds.", ep_text(a, b)))
                    .font(theme::LABEL)
                    .color(PAL.dim),
            );
            for e in mine
                .iter()
                .filter(|e| e.release.episodes == Some((a, b)))
                .take(4)
            {
                let key = e.item.key();
                let busy = s.feeds.busy.contains(&key);
                let facts: Vec<String> = [
                    e.release.parsed.group.clone(),
                    Some(e.video_text()).filter(|v| !v.is_empty()),
                    e.item.size.map(size_text),
                    e.item.seeders.map(|n| format!("{n} seeders")),
                ]
                .into_iter()
                .flatten()
                .collect();
                c = c.child(
                    row()
                        .key(format!("det/release/{key}"))
                        .gap(12.0)
                        .py(5.0)
                        .center_y()
                        .child(
                            col()
                                .grow(1.0)
                                .min_w(0.0)
                                .gap(2.0)
                                .child(
                                    text(e.item.title.clone())
                                        .font(theme::BODY)
                                        .color(PAL.text)
                                        .truncate(),
                                )
                                .child(
                                    text(facts.join(" \u{b7} "))
                                        .font(theme::LABEL)
                                        .color(PAL.dim)
                                        .truncate(),
                                ),
                        )
                        .child(small(
                            format!("det/download/{key}"),
                            if busy {
                                "Downloading\u{2026}"
                            } else {
                                "Download"
                            },
                            e.state == ItemState::Selected,
                            move |s| s.feeds_download(vec![key.clone()]),
                        )),
                );
            }
        }
        None => {
            c = c.child(
                text("No new episode in your feeds.")
                    .font(theme::LABEL)
                    .color(PAL.dim),
            )
        }
    }
    if !groups.is_empty() {
        let mut r = row().gap(8.0).pt(6.0).center_y().flex_wrap().child(
            text(format!("Preferred group: {}", groups.join(", ")))
                .font(theme::LABEL)
                .color(PAL.text),
        );
        r = r.child(small(
            format!("det/unprefer/{id}"),
            "Clear",
            false,
            move |s| s.feeds_prefer_group(id, ""),
        ));
        c = c.child(r);
    }
    Some(c.children(search.map(|b| row().pt(8.0).child(b))))
}
