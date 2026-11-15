//! Design tokens from `docs/design-spec.md`: colours, type, sizes. View code uses these, never
//! literals, so a light theme can come later.

use std::sync::LazyLock;

use rune::{Color, TextStyle};
use kotora_core::Status;

fn hx(s: &str) -> Color {
    rune::color::hex(s).expect("bad hex")
}

/// The full token set of design-spec §1; some tokens wait for later pages (the banner's `cream`).
#[allow(dead_code)]
pub struct Pal {
    pub bg: Color,
    pub side: Color,
    pub surface: Color,
    pub raised: Color,
    pub field: Color,
    pub border: Color,
    pub border_strong: Color,
    pub text: Color,
    pub dim: Color,
    pub faint: Color,
    pub accent: Color,
    pub accent_hi: Color,
    pub accent_lo: Color,
    pub on_accent: Color,
    pub cream: Color,
    pub st_watching: Color,
    pub st_completed: Color,
    pub st_onhold: Color,
    pub st_dropped: Color,
    pub st_plan: Color,
    pub airing: Color,
}

pub static PAL: LazyLock<Pal> = LazyLock::new(|| Pal {
    bg: hx("#0f1626"),
    side: hx("#121b2e"),
    surface: hx("#182238"),
    raised: hx("#1f2b45"),
    field: hx("#141d31"),
    border: hx("#26324d"),
    border_strong: hx("#34446a"),
    text: hx("#edf0f7"),
    dim: hx("#a4afc6"),
    faint: hx("#7d89a4"),
    accent: hx("#f28a22"),
    accent_hi: hx("#ff9d3f"),
    accent_lo: hx("#e57a18"),
    on_accent: hx("#1d1208"),
    cream: hx("#fff0dc"),
    st_watching: hx("#4cc38a"),
    st_completed: hx("#6ea8ff"),
    st_onhold: hx("#f2c94c"),
    st_dropped: hx("#f0646c"),
    st_plan: hx("#a78bfa"),
    airing: hx("#4fd1e0"),
});

impl Pal {
    pub fn status(&self, s: Status) -> Color {
        match s {
            Status::Watching => self.st_watching,
            Status::PlanToWatch => self.st_plan,
            Status::Completed => self.st_completed,
            Status::OnHold => self.st_onhold,
            Status::Dropped => self.st_dropped,
        }
    }
    pub fn ok(&self) -> Color {
        self.st_watching
    }
    pub fn warn(&self) -> Color {
        self.st_onhold
    }
    pub fn hover(&self) -> Color {
        Color::WHITE.alpha(0.05)
    }
    pub fn press(&self) -> Color {
        Color::WHITE.alpha(0.09)
    }
}

// Type (design-spec §2). Numbers use tabular figures.
pub const TITLE: TextStyle = TextStyle::new(20.0, 600.0);
pub const TITLE_NUM: TextStyle = TextStyle::new(20.0, 400.0).features("'tnum' 1");
pub const SECTION: TextStyle = TextStyle::new(15.0, 600.0);
pub const ROW_TITLE: TextStyle = TextStyle::new(14.0, 500.0);
pub const BODY: TextStyle = TextStyle::new(13.0, 400.0);
pub const BODY_MED: TextStyle = TextStyle::new(13.0, 500.0);
pub const NUM: TextStyle = TextStyle::new(13.0, 400.0).features("'tnum' 1");
pub const NUM_MED: TextStyle = TextStyle::new(13.0, 500.0).features("'tnum' 1");
pub const LABEL: TextStyle = TextStyle::new(12.0, 400.0);
pub const LABEL_NUM: TextStyle = TextStyle::new(12.0, 400.0).features("'tnum' 1");
pub const LABEL_BTN: TextStyle = TextStyle::new(12.0, 600.0);
pub const CAPTION: TextStyle = TextStyle::new(11.0, 500.0).tracking(0.5);
pub const SMALL: TextStyle = TextStyle::new(11.0, 400.0);
pub const BADGE: TextStyle = TextStyle::new(11.0, 600.0).features("'tnum' 1");

// Sizes (design-spec §3).
pub const SIDEBAR_W: f32 = 224.0;
pub const PAGE_PAD: f32 = 24.0;
pub const ROW_H: f32 = 56.0;
pub const NAV_H: f32 = 32.0;
pub const COL_GAP: f32 = 14.0;
pub const COL_PROGRESS: f32 = 250.0;
pub const COL_SCORE: f32 = 64.0;
pub const COL_TYPE: f32 = 92.0;
pub const COL_UPDATED: f32 = 76.0;
/// Below this width the type column hides.
pub const NARROW_W: f32 = 1100.0;

pub const R_XS: f32 = 4.0;
pub const R_SM: f32 = 6.0;
pub const R_MD: f32 = 8.0;
pub const R_LG: f32 = 12.0;

/// macOS: the band kept clear at the sidebar's top for the traffic lights, draggable like a
/// title bar (as in Uzumaki). Linux keeps the desktop's own frame and has no band.
pub const TITLEBAR_BAND: f32 = 28.0;
/// macOS: the close button's top-left in points (`rune::Chrome::traffic_lights`): lined up with
/// the brand logo's left edge (sidebar padding 10 + brand padding 8) and centred in the band.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const TRAFFIC_LIGHTS: (f32, f32) = (18.0, (TITLEBAR_BAND - rune::Chrome::TRAFFIC_LIGHT) / 2.0);
