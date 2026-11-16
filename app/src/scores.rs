//! Scores in the user's own format (`kotora_core::score`): the mark a list row, a poster and
//! the details page show, and the picker of the row editor and the details editor.
//!
//! - 100-point, 10-point decimal, 10-point: the number ("73", "7.3", "7").
//! - 5 stars: five small stars, the score's filled.
//! - 3 smileys: one face, tinted (sad red, neutral grey, happy green).

use kotora_core::score::Smiley;
use kotora_core::{Score, ScoreFormat};
use rune::{Color, El, TextStyle, Tx, el, icon, row, text};

use crate::icons;
use crate::model::State;
use crate::theme::{self, PAL};
use crate::ui::cell;

/// A smiley's icon and tint.
fn face(s: Smiley) -> (&'static rune::Icon, Color) {
    match s {
        Smiley::Sad => (&icons::FACE_SAD, PAL.st_dropped),
        Smiley::Neutral => (&icons::FACE_NEUTRAL, PAL.dim),
        Smiley::Happy => (&icons::FACE_HAPPY, PAL.st_watching),
    }
}

/// Five stars, the first `filled` lit.
fn stars(filled: u8, size: f32) -> El {
    let mut r = row().gap(1.0).center_y();
    for i in 1..=5u8 {
        r = r.child(icon(&icons::STAR, size).color(if i <= filled { PAL.st_onhold } else { PAL.border_strong }));
    }
    r
}

/// `score` as the format shows it; numbers in `font` and `color`. The caller draws "no score".
pub fn mark(fmt: ScoreFormat, score: Score, font: TextStyle, color: Color) -> El {
    match fmt {
        ScoreFormat::Point5 => stars(fmt.value(score), 10.0).label(fmt.words(score)),
        ScoreFormat::Point3 => {
            let (i, c) = face(fmt.smiley(score).unwrap_or(Smiley::Neutral));
            el().label(fmt.words(score)).child(icon(i, 16.0).color(c))
        }
        _ => text(fmt.show(score)).font(font).color(color),
    }
}

/// A poster's score (grid views): a star and the number, or the stars, or the face.
pub fn poster_mark(fmt: ScoreFormat, score: Score) -> El {
    match fmt {
        ScoreFormat::Point5 | ScoreFormat::Point3 => mark(fmt, score, theme::NUM_MED, PAL.dim),
        _ => row().gap(3.0).center_y().child(icon(&icons::STAR, 12.0).color(PAL.st_onhold)).child(text(fmt.show(score)).font(theme::NUM_MED).color(PAL.dim)),
    }
}

/// Whether `choice` shows as the picked one for `cur`: the same value in the coarse formats; in
/// the 100-point and decimal ones only the exact score (a 73 lights none of 10, 20, … 100).
fn lit(fmt: ScoreFormat, cur: Option<Score>, choice: Score) -> bool {
    match fmt.fine_step() {
        Some(_) => cur == Some(choice),
        None => fmt.same(cur, Some(choice)),
    }
}

/// The editor's − / + of the fine formats.
fn fine_button(key: &str, label: &str, delta: i32) -> El {
    el().key(key.to_string())
        .role("button")
        .label(if delta < 0 { "Lower score" } else { "Higher score" })
        .focusable(true)
        .size(28.0, 28.0)
        .center()
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.editor_score_step(delta);
        })
        .child(text(label.to_string()).font(TextStyle::new(15.0, 500.0)).color(PAL.dim))
}

/// The editor's score field: "No score", then the format's choices (cells `ed/score/1` to
/// `ed/score/N`, low to high), and for the 100-point and decimal formats a fine − value +.
pub fn picker(fmt: ScoreFormat, cur: Option<Score>) -> El {
    let mut cells = row().flex_wrap().gap(4.0).child(cell("ed/score/none".into(), "No score".into(), cur.is_none(), |s| s.editor_score(None)).child(text("\u{2013}").font(theme::LABEL).color(PAL.dim)));
    let shown = cur.map(|c| fmt.value(c));
    for (i, choice) in fmt.choices().into_iter().enumerate() {
        let n = i + 1;
        let on = lit(fmt, cur, choice);
        let c = cell(format!("ed/score/{n}"), format!("Score {}", fmt.words(choice)), on, move |s| s.editor_score(Some(choice))).px(0.0);
        let c = match fmt {
            // A star lights up to the score, like a rating.
            ScoreFormat::Point5 => {
                let filled = shown.is_some_and(|v| v as usize >= n);
                c.child(icon(&icons::STAR, 14.0).color(if filled { PAL.st_onhold } else { PAL.faint }))
            }
            ScoreFormat::Point3 => {
                let (i, tint) = face(fmt.smiley(choice).unwrap_or(Smiley::Neutral));
                c.child(icon(i, 16.0).color(if on { tint } else { PAL.faint }))
            }
            ScoreFormat::Point10Decimal => c.child(text(n.to_string()).font(theme::LABEL_NUM).color(if on { PAL.accent_hi } else { PAL.dim })),
            _ => c.child(text(fmt.show(choice)).font(theme::LABEL_NUM).color(if on { PAL.accent_hi } else { PAL.dim })),
        };
        cells = cells.child(c);
    }
    if fmt.fine_step().is_some() {
        let value = cur.map_or("\u{2013}".to_string(), |c| fmt.show(c));
        cells = cells.child(
            row()
                .key("ed/score/fine")
                .h(28.0)
                .center_y()
                .radius(theme::R_SM)
                .fill(PAL.field)
                .border(1.0, PAL.border)
                .child(fine_button("ed/score/minus", "\u{2212}", -1))
                .child(row().min_w(36.0).center().child(text(value).font(theme::NUM_MED).color(PAL.text)))
                .child(fine_button("ed/score/plus", "+", 1)),
        );
    }
    cells
}

/// The ▸ Score menu's items: (key part, label, score) low to high, after "No score".
pub fn menu_choices(fmt: ScoreFormat) -> Vec<(usize, String, Score)> {
    fmt.choices().into_iter().enumerate().map(|(i, c)| (i + 1, fmt.words(c), c)).collect()
}

/// Whether a menu choice gets the check mark.
pub fn menu_lit(fmt: ScoreFormat, cur: Option<Score>, choice: Score) -> bool {
    lit(fmt, cur, choice)
}
