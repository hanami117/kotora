//! Pieces the redesigned pages share: posters, blurred backdrops, progress bars, the live dot.
//!
//! A backdrop is the cover's blurred image (`art.rs`, made once per cover, the same in both
//! themes) under a scrim of the page colour, so text on it reads in `PAL.text` in dark and in
//! light alike. While the blurred image is loading, or for a show with no cover, the scrim lies
//! over the generated tile's two colours instead.

use std::f32::consts::PI;

use rune::{Color, El, SurfaceFit, el, row};

use crate::model::State;
use crate::theme::PAL;
use crate::ui::{TILES, cover, hex};

/// A poster: the generated tile, with the real cover over it once it is in memory, at `w` x `h`
/// points (kept at display size by `art.rs`).
pub fn poster(s: &State, id: u32, title: &str, url: Option<&str>, w: f32, h: f32, r: f32) -> El {
    let size = (w * 0.22).clamp(9.0, 40.0);
    let tile = cover(id, title, w, h, size).radius(r);
    match s.covers.get(id, url, w, h) {
        Some(img) => tile.child(el().key("poster-img").absolute(0.0, 0.0).size(w, h).radius(r).pixels(img.pixels())),
        None => tile,
    }
}

/// How much of the page colour lies over a backdrop: `top` at the top edge, `bottom` at the
/// bottom (1.0 melts into the page). Light covers need more in the dark theme and dark covers in
/// the light one; the blurred image keeps its lightness in a middle band (`art::blur`), so one
/// scrim per theme works for every cover.
#[derive(Clone, Copy)]
pub struct Scrim {
    pub top: f32,
    pub mid: f32,
    pub bottom: f32,
}

impl Scrim {
    pub const fn even(a: f32) -> Scrim {
        Scrim { top: a, mid: a, bottom: a }
    }
}

/// The blurred cover of `id`, filling its parent (pinned to every edge), under the scrim.
pub fn backdrop(s: &State, id: u32, url: Option<&str>, scrim: Scrim) -> El {
    let base = el().key("backdrop").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0));
    let base = match s.covers.blurred(id, url) {
        Some(img) => base.pixels_fit(img.pixels(), SurfaceFit::Fill),
        None => {
            let (a, b) = TILES[id as usize % TILES.len()];
            base.linear_gradient(110.0 * PI / 180.0, [(0.0, hex(a)), (1.0, hex(b))])
        }
    };
    let bg = PAL.bg;
    base.child(
        el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .linear_gradient(PI, [(0.0, bg.alpha(scrim.top)), (0.55, bg.alpha(scrim.mid)), (1.0, bg.alpha(scrim.bottom))]),
    )
}

/// A rounded progress bar, `frac` of it in `c` over `track`.
pub fn bar(frac: f32, c: Color, h: f32, track: Color) -> El {
    let frac = frac.clamp(0.0, 1.0);
    row()
        .w_full()
        .h(h)
        .radius(h / 2.0)
        .clip_rounded(h / 2.0)
        .fill(track)
        .child(el().grow(frac.max(0.0001)).h_full().radius(h / 2.0).fill(c))
        .child(el().grow((1.0 - frac).max(0.0001)))
}

/// The dot before "PLAYING IN MPV": green while playing, grey while paused.
pub fn live_dot(playing: bool, size: f32) -> El {
    let c = if playing { PAL.ok() } else { PAL.faint };
    let d = el().size(size, size).radius(size / 2.0).fill(c);
    if playing { d.outline(size * 0.35, 0.0, c.alpha(0.25)) } else { d }
}

/// The scrims each theme uses on a card-sized backdrop (sidebar card, Home's wide cards): mostly
/// page colour, the cover a tint.
pub fn card_scrim() -> Scrim {
    if PAL.light { Scrim::even(0.66) } else { Scrim::even(0.62) }
}

/// A border that reads on a backdrop in both themes.
pub fn glass_border() -> Color {
    if PAL.light { Color::BLACK.alpha(0.08) } else { Color::WHITE.alpha(0.08) }
}

/// Large type for heroes and banners.
pub const HERO_TITLE: rune::TextStyle = rune::TextStyle::new(32.0, 600.0).tracking(-0.4);
