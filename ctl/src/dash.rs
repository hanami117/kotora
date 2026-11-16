//! The bare `kot`: a compact dashboard. [`render`] turns the app's [`Dashboard`] into text for a
//! given width, with or without colour; it reads no terminal and no clock, so tests feed it fakes.

use kotora_ctl::{DashRow, Dashboard, NowInfo};

/// Width of the progress bar, in cells (dropped on a narrow terminal).
const BAR: usize = 10;
/// Below this many columns the bar goes, so the title keeps room.
const NARROW: usize = 56;
const GAP: &str = "  ";

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Plain,
    Bold,
    Dim,
    Green,
    Yellow,
}

/// Wraps `text` in the style's escape codes (nothing when colour is off).
fn paint(text: &str, style: Style, color: bool) -> String {
    let code = match (color, style) {
        (false, _) | (_, Style::Plain) => return text.to_string(),
        (_, Style::Bold) => "1",
        (_, Style::Dim) => "2",
        (_, Style::Green) => "32",
        (_, Style::Yellow) => "33",
    };
    format!("\x1b[{code}m{text}\x1b[0m")
}

/// Columns a character takes: 2 for East Asian wide ones (titles are often Japanese), else 1.
fn char_width(c: char) -> usize {
    match c as u32 {
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1FAFF | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

pub fn width_of(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// `s` cut to at most `max` columns, ending in `…` when something was cut.
pub fn truncate(s: &str, max: usize) -> String {
    if width_of(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = char_width(c);
        if w + cw > max - 1 {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('\u{2026}');
    out
}

/// `s` padded with spaces to `w` columns (left aligned).
fn pad(s: &str, w: usize) -> String {
    format!("{s}{}", " ".repeat(w.saturating_sub(width_of(s))))
}

fn bar(progress: u32, total: Option<u32>) -> String {
    let filled = match total {
        Some(t) if t > 0 => ((u64::from(progress.min(t)) * BAR as u64 + u64::from(t) / 2) / u64::from(t)) as usize,
        _ => 0,
    };
    // A started show shows at least one cell.
    let filled = if progress > 0 && filled == 0 && total.is_some() { 1 } else { filled };
    format!("{}{}", "\u{2588}".repeat(filled), "\u{2591}".repeat(BAR - filled))
}

/// "in 2d", "in 5h", "in 40m": the short form for the table.
pub fn countdown(secs: i64) -> String {
    let m = secs.max(0) / 60;
    match m {
        0 => "now".into(),
        1..=59 => format!("in {m}m"),
        60..=2879 => format!("in {}h", m / 60),
        _ => format!("in {}d", m / 1440),
    }
}

/// "just now", "5 min ago", "3 h ago", "2 d ago".
pub fn ago(secs: i64) -> String {
    let m = secs.max(0) / 60;
    match m {
        0 => "just now".into(),
        1..=59 => format!("{m} min ago"),
        60..=2879 => format!("{} h ago", m / 60),
        _ => format!("{} d ago", m / 1440),
    }
}

fn clock(secs: u32) -> String {
    match secs / 3600 {
        0 => format!("{}:{:02}", secs / 60, secs % 60),
        h => format!("{h}:{:02}:{:02}", (secs / 60) % 60, secs % 60),
    }
}

/// The now-playing line: "▶ mpv · Title · ep 6 · 16:08/23:40 · Update to ep 6? (kot confirm)".
fn now_line(n: &NowInfo, width: usize, color: bool) -> String {
    let icon = if n.playing { "\u{25b6}" } else { "\u{23f8}" };
    let ep = n.episode.map(|e| format!(" \u{b7} ep {e}")).unwrap_or_default();
    let time = match (n.position, n.duration) {
        (Some(p), Some(d)) if d > 0 => format!(" \u{b7} {}/{}", clock(p), clock(d)),
        (Some(p), _) => format!(" \u{b7} {}", clock(p)),
        _ => n.percent.map(|p| format!(" \u{b7} {p}%")).unwrap_or_default(),
    };
    let offer = match (n.recorded, n.offer) {
        (Some(e), _) => format!(" \u{b7} recorded ep {e}"),
        (None, Some(e)) if n.dismissed => format!(" \u{b7} ep {e} dismissed"),
        (None, Some(e)) if n.reached => format!(" \u{b7} update to ep {e}? kot confirm"),
        (None, Some(e)) => format!(" \u{b7} will offer ep {e}"),
        (None, None) => String::new(),
    };
    let head = format!("{icon} {}", n.player);
    let tail = format!("{ep}{time}{offer}");
    // The title takes what is left, but at least 8 columns.
    let room = width.saturating_sub(width_of(&head) + width_of(&tail) + 3).max(8);
    let title = truncate(&n.title, room);
    // Even with the title at its minimum the line may be too wide: cut the end.
    let rest = truncate(&format!(" \u{b7} {title}{tail}"), width.saturating_sub(width_of(&head)));
    format!("{}{rest}", paint(&head, Style::Green, color))
}

/// The table's rows in order: next episode on disk first, then the soonest to air, then the
/// ones with something aired or nothing known, each group by most recently updated.
fn sorted(rows: &[DashRow]) -> Vec<&DashRow> {
    let mut v: Vec<&DashRow> = rows.iter().collect();
    v.sort_by_key(|r| (!r.on_disk, r.air_at.filter(|_| !r.on_disk).unwrap_or(i64::MAX), std::cmp::Reverse(r.updated_at)));
    v
}

fn next_cell(r: &DashRow, now: i64) -> (String, Style) {
    if r.on_disk {
        (format!("\u{25b6} Ep {} on disk", r.progress + 1), Style::Green)
    } else if let Some(at) = r.air_at {
        (format!("{} ({})", r.air_slot, countdown(at - now)), Style::Plain)
    } else if r.aired {
        ("aired \u{b7} not on disk".into(), Style::Yellow)
    } else {
        ("\u{2014}".into(), Style::Dim)
    }
}

/// The whole dashboard for `width` columns. Ends with a newline.
pub fn render(d: &Dashboard, width: usize, color: bool) -> String {
    let width = width.max(30);
    let mut out = String::new();
    if let Some(n) = &d.now {
        out.push_str(&now_line(n, width, color));
        out.push_str("\n\n");
    }
    if d.rows.is_empty() {
        out.push_str(&paint("Nothing in Watching.", Style::Dim, color));
        out.push('\n');
    } else {
        out.push_str(&paint(&format!("WATCHING ({})", d.rows.len()), Style::Bold, color));
        out.push('\n');
        let rows = sorted(&d.rows);
        let with_bar = width >= NARROW;
        let progress: Vec<String> = rows.iter().map(|r| format!("{}/{}", r.progress, r.episodes.map_or("?".to_string(), |n| n.to_string()))).collect();
        let nexts: Vec<(String, Style)> = rows.iter().map(|r| next_cell(r, d.now_unix)).collect();
        let pw = progress.iter().map(|p| width_of(p)).max().unwrap_or(0);
        let bw = if with_bar { BAR + 1 } else { 0 };
        let nw_want = nexts.iter().map(|(t, _)| width_of(t)).max().unwrap_or(0);
        let tw_want = rows.iter().map(|r| width_of(&r.title)).max().unwrap_or(0);
        // Title gets what the other columns leave; NEXT yields before the title drops under 12.
        let fixed = bw + pw + 2 * GAP.len();
        let mut nw = nw_want.min(width.saturating_sub(fixed + 12).max(8));
        let mut tw = tw_want.min(width.saturating_sub(fixed + nw));
        if tw < 12.min(tw_want) {
            tw = 12.min(tw_want);
            nw = width.saturating_sub(fixed + tw).max(1);
        }
        for ((r, p), (next, style)) in rows.iter().zip(&progress).zip(&nexts) {
            let mut line = pad(&truncate(&r.title, tw), tw);
            line.push_str(GAP);
            if with_bar {
                let b = bar(r.progress, r.episodes);
                let (done, rest) = b.split_at(b.find('\u{2591}').unwrap_or(b.len()));
                line.push_str(&paint(done, Style::Green, color));
                line.push_str(&paint(rest, Style::Dim, color));
                line.push(' ');
            }
            line.push_str(&" ".repeat(pw - width_of(p)));
            line.push_str(p);
            line.push_str(GAP);
            line.push_str(&paint(&truncate(next, nw), *style, color));
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.push('\n');
    out.push_str(&paint(&truncate(&footer(d), width), Style::Dim, color));
    out.push('\n');
    out
}

fn footer(d: &Dashboard) -> String {
    let queue = match d.queue {
        0 => "queue empty".to_string(),
        1 => "1 change waiting to sync".to_string(),
        n => format!("{n} changes waiting to sync"),
    };
    let last = d.synced_at.map_or("not synced yet".to_string(), |t| format!("last sync {}", ago(d.now_unix - t)));
    let offline = if d.offline { " \u{b7} offline" } else { "" };
    format!("{queue} \u{b7} {last}{offline}")
}

#[cfg(test)]
mod tests;
