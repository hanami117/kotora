//! The startup intro (a proposal, easy to tune or drop): the window opens under a tiger-orange
//! layer with 小虎 in the middle; three claw slashes rip across it about 90 ms apart, the layer
//! splits along them into four strips that slide away and fade, and the app is underneath.
//! About 0.8 s. Any click or key skips it.
//!
//! - **The clock:** it starts at the first frame the window actually presented, not at process
//!   start or state creation (window, GPU and the first view can take hundreds of ms before
//!   anything shows). Until then the cover sits in its start pose, solid orange, opaque from
//!   that very first frame (`overlay`). Rune's `on_frame_encoded` reports a `Drawn` frame;
//!   `on_frame` turns the first into `presented`, which arms the intro. `poll` then holds the
//!   solid orange `LEAD` longer (nothing runs, so a late frame in that time costs no motion;
//!   Rune charges running motion for a hitch, up to 250 ms), and only then starts the tweens.
//!
//! - **A window that resizes as it opens** (a tiling window manager enlarges it at once): the
//!   hold also waits until the window's size has been the same for `SETTLE`, at most
//!   `MAX_WAIT` after the first frame, solid orange meanwhile (`hold`). The layer sits outside
//!   `root` (`ui::view`), which lands a resize's changes at once, so the tweens keep running if
//!   a resize comes in mid-run; and the geometry is laid out for the window's current size on
//!   every build (`Fit`), as vectors, never a drawing made for one size and stretched.
//!
//! - **When:** only on a real cold start with a window (`at_launch`): not a `--hidden` or login
//!   start in the background, not a test instance, not headless (scenes freeze frames of it), and
//!   never when the window comes back from the tray or menu bar. Settings → Appearance →
//!   Startup animation turns it off (`[display] intro`).
//! - **Reduce Motion** (macOS, GNOME `enable-animations`, KDE; Rune reads them): the orange
//!   layer only fades out.
//! - **Motion:** every piece is a vector shape (`Shape` fills, no images), drawn in one 1600×1000
//!   box that `Fit` covers the window with (one scale, centred: angles and widths hold). Each track below is a tween Rune runs itself, set once when the
//!   intro starts, so the view isn't rebuilt per frame; `poll` only starts and ends it. A scene
//!   evaluates the same tracks at a fixed time instead (`Intro::frozen`).
//!
//! To remove it: drop this file, `mod intro`, the `intro` lines in `ui.rs` (`view`, `poll`,
//! `on_key`), `State::intro`, the Settings switch and `[display] intro`.

use std::cell::Cell;
use std::time::Duration;

use rune::{Anim, AnimExt, Color, Curve, Cx, Easing, El, Shape, Tx, el};

use crate::model::State;
use crate::theme::PAL;

/// The shapes' coordinate box. `Fit` lays it over the window.
const VW: f32 = 1600.0;
const VH: f32 = 1000.0;

/// How the 1600×1000 box lies on a window of `w` × `h` points: one scale that covers the whole
/// window (so no edge ever shows), centred. Claw angles and widths keep their look at any size
/// or shape, and a resize only recomputes the points: they stay vectors.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fit {
    w: f32,
    h: f32,
    k: f32,
}

impl Fit {
    fn new([w, h]: [f32; 2]) -> Fit {
        let (w, h) = (w.max(1.0), h.max(1.0));
        Fit {
            w,
            h,
            k: (w / VW).max(h / VH),
        }
    }

    /// A box point in window points.
    fn pt(self, p: [f32; 2]) -> [f32; 2] {
        [
            (p[0] - VW / 2.0) * self.k + self.w / 2.0,
            (p[1] - VH / 2.0) * self.k + self.h / 2.0,
        ]
    }

    fn pts(self, v: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
        v.into_iter().map(|p| self.pt(p)).collect()
    }

    /// A shape whose points are already in window points.
    fn shape(self, pts: Vec<[f32; 2]>, color: Color) -> Shape {
        Shape::new()
            .fill(self.pts(pts), color)
            .view_box(self.w, self.h)
    }
}

/// One property's change: it starts `at` seconds after the intro starts and takes `dur`.
#[derive(Clone, Copy)]
struct Track {
    at: f32,
    dur: f32,
    ease: Easing,
}

impl Track {
    const fn new(at: f32, dur: f32, ease: Easing) -> Track {
        Track { at, dur, ease }
    }

    /// Eased progress (0 to 1) at time `t`.
    fn at_time(self, t: f32) -> f32 {
        if self.dur <= 0.0 {
            return if t >= self.at { 1.0 } else { 0.0 };
        }
        self.ease.eval(((t - self.at) / self.dur).clamp(0.0, 1.0))
    }

    fn anim(self) -> Anim {
        Curve::tween(self.dur.max(0.01), self.ease).delay(self.at)
    }
}

// ---- the timeline (seconds from the first frame of the intro) ----

/// Each slash rips across in `SLASH`, `SLASH_GAP` after the one before; the first after `HOLD`.
const HOLD: f32 = 0.06;
const SLASH_GAP: f32 = 0.09;
const SLASH: f32 = 0.12;
/// The strips start sliding apart (the last slash is nearly through) and take `SPLIT`.
const SPLIT_AT: f32 = 0.33;
const SPLIT: f32 = 0.45;
/// After the first presented frame the solid orange holds this long before the tweens start.
/// Nothing runs then, so startup work on the UI thread (first sync results, covers) that hitches
/// a frame or two costs no motion; the 0.8 s below is then seen whole.
pub const LEAD: f64 = 0.10;
/// ...and the window's size has been the same this long (a window manager that resizes it as it
/// opens: the intro starts after that settles).
pub const SETTLE: f64 = 0.12;
/// The most the hold waits for the window to settle, from the first presented frame.
pub const MAX_WAIT: f64 = 0.40;
/// The whole intro; it is dropped after this.
pub const TOTAL: f32 = 0.8;
/// Reduce Motion: the layer holds a moment, then fades.
const REDUCED_FADE: Track = Track::new(0.12, 0.25, Easing::EASE_OUT);
pub const REDUCED_TOTAL: f32 = 0.4;

fn slash_track(k: usize) -> Track {
    Track::new(
        HOLD + SLASH_GAP * k as f32,
        SLASH,
        Easing::cubic(0.3, 0.0, 0.2, 1.0),
    )
}
/// The cuts fade as the strips part (the app shows through the gaps then).
const CUT_FADE: Track = Track::new(SPLIT_AT + 0.04, 0.16, Easing::EASE_IN);
/// The strips part and slide away, and fade over their second half.
const STRIP_MOVE: Track = Track::new(SPLIT_AT, SPLIT, Easing::cubic(0.35, 0.0, 0.55, 1.0));
const STRIP_FADE: Track = Track::new(SPLIT_AT + 0.22, SPLIT - 0.22, Easing::EASE_IN);
/// One solid layer under the strips until they part, so their shared edges never show seams.
const BASE_OFF: Track = Track::new(SPLIT_AT, 0.0, Easing::LINEAR);
/// 小虎 fades with the last slash and is gone as the strips part. Opacity only: the glyphs are
/// icons, rasterised per size, so scaling them would raster every frame.
const NAME_FADE: Track = Track::new(SPLIT_AT - 0.04, 0.14, Easing::EASE_IN);

// ---- the geometry, in the 1600×1000 box ----

/// The three claw lines, upper right to lower left, each a quadratic curve bowed a little to
/// the upper left: start, control, end. Long enough to cross the whole box.
fn claw(k: usize) -> [[f32; 2]; 3] {
    let dx = (k as f32 - 1.0) * 210.0;
    let bow = [52.0, 70.0, 58.0][k];
    let (p0, p2) = ([1250.0 + dx, -70.0], [350.0 + dx, 1070.0]);
    let n = normal(p0, p2);
    let mid = [
        (p0[0] + p2[0]) / 2.0 + n[0] * bow,
        (p0[1] + p2[1]) / 2.0 + n[1] * bow,
    ];
    [p0, mid, p2]
}

/// A unit normal of `a`→`b` (for the claws, which run down to the left: up and to the left).
fn normal(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy).max(1e-3);
    [-dy / len, dx / len]
}

fn bezier(c: [[f32; 2]; 3], t: f32) -> [f32; 2] {
    let u = 1.0 - t;
    let f = |i: usize| u * u * c[0][i] + 2.0 * u * t * c[1][i] + t * t * c[2][i];
    [f(0), f(1)]
}

/// The curve's unit normal at `t`.
fn bezier_normal(c: [[f32; 2]; 3], t: f32) -> [f32; 2] {
    let d = |i: usize| 2.0 * (1.0 - t) * (c[1][i] - c[0][i]) + 2.0 * t * (c[2][i] - c[1][i]);
    normal([0.0, 0.0], [d(0), d(1)])
}

const SAMPLES: usize = 28;

/// A claw mark's outline around line `k`: sharp where it starts, widest a third of the way in,
/// a long tail. `width` at the widest; `shift` moves it across the line (in widths).
fn mark(k: usize, width: f32, shift: f32) -> Vec<[f32; 2]> {
    let c = claw(k);
    // t^a (1-t)^b, scaled to 1 at its peak (t = a / (a + b)).
    let (a, b): (f32, f32) = (0.55, 1.15);
    let peak = (a / (a + b)).powf(a) * (b / (a + b)).powf(b);
    let mut left = Vec::with_capacity(SAMPLES + 1);
    let mut right = Vec::with_capacity(SAMPLES + 1);
    for i in 0..=SAMPLES {
        let t = i as f32 / SAMPLES as f32;
        let h = width / 2.0 * t.powf(a) * (1.0 - t).powf(b) / peak;
        let p = bezier(c, t);
        let n = bezier_normal(c, t);
        let o = shift * width;
        left.push([p[0] + n[0] * (o + h), p[1] + n[1] * (o + h)]);
        right.push([p[0] + n[0] * (o - h), p[1] + n[1] * (o - h)]);
    }
    right.reverse();
    left.extend(right);
    left
}

/// How far the strips' edges run on past the box, straight along the claws' ends, so a strip
/// that slides along a claw never shows its end. More than any strip travels (`strip_away`).
const REACH: f32 = 1400.0;

/// Claw line `k` for the strips' edges: the curve, carried on straight past both ends.
fn edge(k: usize) -> Vec<[f32; 2]> {
    let c = claw(k);
    let unit = |a: [f32; 2], b: [f32; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dy).max(1e-3);
        [dx / len, dy / len]
    };
    let (d0, d2) = (unit(c[1], c[0]), unit(c[1], c[2]));
    let mut pts = Vec::with_capacity(SAMPLES + 3);
    pts.push([c[0][0] + d0[0] * REACH, c[0][1] + d0[1] * REACH]);
    pts.extend((0..=SAMPLES).map(|i| bezier(c, i as f32 / SAMPLES as f32)));
    pts.push([c[2][0] + d2[0] * REACH, c[2][1] + d2[1] * REACH]);
    pts
}

/// The orange strip `k` (0 to 3, left to right): between claw lines `k - 1` and `k`, and out to
/// `REACH` past the box on the outer two.
fn strip(k: usize) -> Vec<[f32; 2]> {
    let far = REACH;
    let mut pts: Vec<[f32; 2]> = Vec::with_capacity(2 * SAMPLES + 8);
    match k {
        0 => {
            let e = edge(0);
            let (first, last) = (e[0], e[e.len() - 1]);
            pts.extend(e);
            pts.extend([[-far, last[1]], [-far, first[1]]]);
        }
        3 => {
            let e = edge(2);
            let (first, last) = (e[0], e[e.len() - 1]);
            pts.extend(e);
            pts.extend([[VW + far, last[1]], [VW + far, first[1]]]);
        }
        _ => {
            pts.extend(edge(k - 1));
            let mut back = edge(k);
            back.reverse();
            pts.extend(back);
        }
    }
    pts
}

/// Where strip `k` goes, in points: across the claws away from the middle, and along them,
/// alternately up and down, as if torn off.
fn strip_away(k: usize) -> [f32; 2] {
    let c = claw(1);
    let n = normal(c[0], c[2]);
    let d = [n[1], -n[0]]; // along the claws, down and to the left (`n` is up and to the left)
    let across = [360.0, 140.0, -140.0, -360.0][k];
    let along = [700.0, -560.0, 600.0, -680.0][k];
    [n[0] * across + d[0] * along, n[1] * across + d[1] * along]
}

/// A claw mark: a lighter rim on the orange, the cut (the window's background, as if torn
/// through) and a bright glint along its leading edge.
/// Each piece is its own element with one fill: a fading shape of several pieces would paint
/// through a window-sized layer.
fn slash_shapes(k: usize, fit: Fit) -> [(&'static str, Shape); 3] {
    let w = [30.0, 38.0, 32.0][k];
    let one = |pts: Vec<[f32; 2]>, color: Color| fit.shape(pts, color);
    [
        ("rim", one(mark(k, w * 1.5, 0.0), rim())),
        ("cut", one(mark(k, w, 0.0), PAL.bg)),
        (
            "glint",
            one(mark(k, w * 0.26, 0.24), hex("#fff6e6").alpha(0.95)),
        ),
    ]
}

/// The torn rim round a cut: a lighter orange than the layer.
fn rim() -> Color {
    hex("#ffb866")
}

fn hex(s: &str) -> Color {
    rune::color::hex(s).unwrap_or(Color::WHITE)
}

fn strip_shape(k: usize, fit: Fit) -> Shape {
    fit.shape(strip(k), PAL.accent)
}

/// 小虎's colour over the orange: cream in the dark theme, the dark brown text on the accent
/// has in the light one.
fn name_color() -> Color {
    if PAL.light {
        PAL.on_accent
    } else {
        hex("#fff7ec")
    }
}

// ---- state ----

pub struct Intro {
    phase: Phase,
    /// The first frame was presented on screen with the cover in it (`presented`). Nothing
    /// counts down before this.
    armed: bool,
    /// Poll time the running phase began; in `Start`, the poll time the intro was armed.
    since: Option<f64>,
    /// Scenes: arm at this poll time instead of on a presented frame (a slow first frame).
    arm_at: Option<f64>,
    /// Scenes: the time it shows, held (no poll timing headless).
    frozen: Option<f32>,
    /// What the view last drew: the Reduce Motion fade (shorter, so `poll` ends it sooner).
    reduced: Cell<bool>,
    /// Scenes: draw the Reduce Motion fade whatever the system says.
    force_reduced: bool,
    /// The window's size at the last poll, and the poll time it last changed (the hold waits
    /// for it to settle).
    size: Option<[f32; 2]>,
    changed: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Everything in its start pose, until the first frame is presented and `LEAD` has gone, so
    /// the tweens have something to run from.
    Start,
    Run,
}

/// What `main` knows about this start.
#[derive(Clone, Copy, Debug, Default)]
pub struct Launch {
    /// Settings → Appearance → Startup animation.
    pub enabled: bool,
    /// `--hidden`: no window (a login start in the background).
    pub hidden: bool,
    /// `KOTORA_TEST_INSTANCE`: a second copy for tests.
    pub test_instance: bool,
    /// `--snap`, `--tree`, `--script` and friends.
    pub headless: bool,
}

/// The intro for this start, if it gets one: a real cold start that opens a window.
pub fn at_launch(l: Launch) -> Option<Intro> {
    (l.enabled && !l.hidden && !l.test_instance && !l.headless).then(Intro::new)
}

impl Intro {
    fn new() -> Intro {
        Intro {
            phase: Phase::Start,
            armed: false,
            since: None,
            arm_at: None,
            frozen: None,
            reduced: Cell::new(false),
            force_reduced: false,
            size: None,
            changed: None,
        }
    }

    /// Scenes: the intro held at `progress` (0 to 1) of its run.
    pub fn frozen(progress: f32, reduced: bool) -> Intro {
        let total = if reduced { REDUCED_TOTAL } else { TOTAL };
        Intro {
            frozen: Some(progress.clamp(0.0, 1.0) * total),
            force_reduced: reduced,
            ..Intro::new()
        }
    }

    /// Scenes: the live intro, which a headless `--video` records (its clock runs there).
    pub fn live() -> Intro {
        Intro {
            armed: true,
            ..Intro::new()
        }
    }

    /// Scenes: the live intro whose first frame comes `late` seconds in (a slow window or first
    /// view): it must wait in the start pose, then play whole.
    pub fn live_late(late: f64) -> Intro {
        Intro {
            arm_at: Some(late),
            ..Intro::new()
        }
    }

    /// Scenes: the live Reduce Motion fade.
    pub fn live_reduced() -> Intro {
        Intro {
            armed: true,
            force_reduced: true,
            ..Intro::new()
        }
    }
}

/// How often the hold looks at the window's size.
const WATCH: f64 = 0.04;

impl Intro {
    /// Notes the window's size at poll time `t`; a different size is a resize just now. The
    /// first real size is not one, and nor is "not laid out yet" (zero).
    fn watch(&mut self, size: [f32; 2], t: f64) {
        if size[0] < 1.0 || size[1] < 1.0 {
            return;
        }
        if let Some(old) = self.size
            && ((old[0] - size[0]).abs() > 0.5 || (old[1] - size[1]).abs() > 0.5)
        {
            self.changed = Some(t);
        }
        self.size = Some(size);
    }
}

/// What the hold before the run does at poll time `now`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hold {
    /// The run starts now.
    Go,
    /// Not yet: at least this many seconds more.
    Wait(f64),
}

/// The solid-orange hold, from `armed` (the poll time the first frame was presented). The run
/// starts once `LEAD` has passed and the window's size has not changed for `SETTLE` (`changed`
/// is when it last did), or `MAX_WAIT` after `armed`, whichever comes first.
fn hold(armed: f64, changed: Option<f64>, now: f64) -> Hold {
    const EPS: f64 = 1e-3;
    let waited = now - armed;
    if waited >= MAX_WAIT - EPS {
        return Hold::Go;
    }
    let lead = LEAD - waited;
    let settle = changed.map_or(0.0, |c| SETTLE - (now - c));
    let left = lead.max(settle);
    if left <= EPS {
        Hold::Go
    } else {
        Hold::Wait(left.min(MAX_WAIT - waited))
    }
}

/// Any click or key: the layer goes at once (its own fade-out, 0.15 s).
pub fn skip(s: &mut State) {
    s.intro = None;
}

/// `AppSpec::poll`: starts the run once the window shows, and drops the intro when it is over.
/// The window going (closed to the tray mid-intro) drops it too: it never replays later.
pub fn poll(s: &mut State, t: f64, tx: &mut Tx) -> bool {
    let size = s.main_size.get();
    let Some(i) = s.intro.as_mut() else {
        return false;
    };
    if i.frozen.is_some() {
        return false;
    }
    i.watch(size, t);
    if !s.window {
        // Closed (to the tray) after it began: it never replays. Not shown yet (a macOS launch
        // checked for a login start): it waits for the window.
        if i.since.is_some() {
            s.intro = None;
        }
        return false;
    }
    if i.arm_at.is_some_and(|at| t >= at) {
        i.armed = true;
    }
    match (i.phase, i.since) {
        // No frame has been presented with the cover yet: the clock doesn't run. `presented`
        // wakes this poll (a scene's late arming sets its own wake).
        (Phase::Start, None) if !i.armed => {
            if let Some(at) = i.arm_at {
                tx.wake_after(Duration::from_secs_f64((at - t).max(0.0)));
            }
            false
        }
        // The frame is up: the solid orange holds `LEAD`, then the tweens start.
        (Phase::Start, None) => {
            i.since = Some(t);
            tx.wake_after(Duration::from_secs_f64(LEAD.min(WATCH)));
            false
        }
        (Phase::Start, Some(since)) => match hold(since, i.changed, t) {
            Hold::Go => {
                i.phase = Phase::Run;
                i.since = Some(t);
                tx.animated();
                tx.wake_after(Duration::from_secs_f32(TOTAL));
                true
            }
            // A resize shows in `main_size` only at the next build, so look often.
            Hold::Wait(left) => {
                tx.wake_after(Duration::from_secs_f64(left.min(WATCH)));
                false
            }
        },
        (Phase::Run, Some(since)) => {
            let total = if i.reduced.get() || i.force_reduced {
                REDUCED_TOTAL
            } else {
                TOTAL
            } as f64;
            if t - since >= total {
                s.intro = None;
                tx.instant();
                true
            } else {
                tx.wake_after(Duration::from_secs_f64(total - (t - since)));
                false
            }
        }
        (Phase::Run, None) => false,
    }
}

/// A frame was presented on screen (`App::on_frame_encoded`, `Drawn`) with the cover in it:
/// the intro's clock may start. Only once the window shows, and only the first such frame.
/// Returns true once, so the poll that starts the hold runs.
pub fn presented(s: &mut State) -> bool {
    let window = s.window;
    match s.intro.as_mut() {
        Some(i) if window && !i.armed && i.frozen.is_none() => {
            i.armed = true;
            true
        }
        _ => false,
    }
}

/// `App::on_frame_encoded`'s hook: tells the state about the first frame presented with the
/// intro up, then goes quiet (no work per frame after that).
pub fn on_frame(handle: rune::AppHandle<State>) -> impl FnMut(&rune::FrameEncoded) + 'static {
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    move |f| {
        use std::sync::atomic::Ordering::Relaxed;
        if f.outcome() != rune::FrameOutcome::Drawn || done.load(Relaxed) {
            return;
        }
        let done = done.clone();
        handle.update_if(move |s: &mut State, _| {
            // Not shown yet (a macOS launch not yet known to be a login start): the next frame
            // tries again. No intro: nothing to arm.
            if s.intro.is_none() || s.intro.as_ref().is_some_and(|i| i.armed) || s.window {
                done.store(true, Relaxed);
            }
            presented(s)
        });
    }
}

/// The layer over the whole window, while there is an intro.
pub fn overlay(s: &State, cx: &Cx<State>) -> Option<El> {
    let i = s.intro.as_ref()?;
    let reduced = i.force_reduced || cx.is_reduced_motion();
    i.reduced.set(reduced);
    // A held frame shows time `t`; live, the start pose, then the end pose with the tracks'
    // tweens.
    let (t, motion) = match (i.frozen, i.phase) {
        (Some(t), _) => (t, false),
        (None, Phase::Start) => (0.0, false),
        (None, Phase::Run) => (f32::INFINITY, true),
    };
    // The geometry follows the window's size as it is now (every build; a resize rebuilds).
    let fit = Fit::new(cx.layout_size("root").unwrap_or([1280.0, 800.0]));
    let pin = Some(0.0);
    let layer = el()
        .key("intro")
        .role("image")
        .label("Kotora")
        .pin(pin, pin, pin, pin)
        // Esc, any click or key skips; the app underneath takes none until the layer is gone.
        .modal(false, skip)
        .on_press(skip);
    let name = el()
        .key("intro/name")
        .pin(pin, pin, pin, pin)
        .center()
        .child(crate::kanji::name(132.0, true, name_color()));

    if reduced {
        let fade =
            |e: El, track: Track| set_opacity(e, 1.0 - track.at_time(t), motion.then_some(track));
        let layer = fade(layer.fill(PAL.accent), REDUCED_FADE);
        return Some(layer.child(name));
    }

    let base = el()
        .key("intro/base")
        .pin(pin, pin, pin, pin)
        .fill(PAL.accent);
    let mut layer = layer.child(set_opacity(
        base,
        1.0 - BASE_OFF.at_time(t),
        motion.then_some(BASE_OFF),
    ));
    for k in 0..4 {
        let away = strip_away(k);
        let m = STRIP_MOVE.at_time(t);
        let e = el()
            .key(format!("intro/strip/{k}"))
            .pin(pin, pin, pin, pin)
            .shape(strip_shape(k, fit));
        let e = set_offset(e, [away[0] * m, away[1] * m], motion.then_some(STRIP_MOVE));
        layer = layer.child(set_opacity(
            e,
            1.0 - STRIP_FADE.at_time(t),
            motion.then_some(STRIP_FADE),
        ));
    }
    let name = set_opacity(
        name,
        1.0 - NAME_FADE.at_time(t),
        motion.then_some(NAME_FADE),
    );
    layer = layer.child(name);
    for k in 0..3 {
        let track = slash_track(k);
        let c = claw(k);
        // The mark grows out of its starting point, so it rips across from the top right.
        let [ox, oy] = fit.pt(c[0]);
        for (part, shape) in slash_shapes(k, fit) {
            let e = el()
                .key(format!("intro/slash/{k}/{part}"))
                .pin(pin, pin, pin, pin)
                .shape(shape)
                .origin(ox / fit.w, oy / fit.h);
            let e = set_scale(e, track.at_time(t).max(0.001), motion.then_some(track));
            layer = layer.child(set_opacity(
                e,
                1.0 - CUT_FADE.at_time(t),
                motion.then_some(CUT_FADE),
            ));
        }
    }
    Some(layer)
}

// A held value lands at once; a running one runs on its track.

fn set_opacity(e: El, v: f32, track: Option<Track>) -> El {
    match track {
        Some(tr) => e.opacity(v).opacity_motion(tr.anim()),
        None => e.opacity(v).opacity_motion(Anim::INSTANT),
    }
}

fn set_scale(e: El, v: f32, track: Option<Track>) -> El {
    match track {
        Some(tr) => e.scale(v).scale_motion(tr.anim()),
        None => e.scale(v).scale_motion(Anim::INSTANT),
    }
}

fn set_offset(e: El, v: [f32; 2], track: Option<Track>) -> El {
    match track {
        Some(tr) => e.offset(v[0], v[1]).offset_motion(tr.anim()),
        None => e.offset(v[0], v[1]).offset_motion(Anim::INSTANT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch() -> Launch {
        Launch {
            enabled: true,
            hidden: false,
            test_instance: false,
            headless: false,
        }
    }

    #[test]
    fn only_a_real_cold_start_with_a_window_plays_it() {
        assert!(
            at_launch(launch()).is_some(),
            "a cold start with the window"
        );
        assert!(
            at_launch(Launch {
                enabled: false,
                ..launch()
            })
            .is_none(),
            "Settings switch off"
        );
        assert!(
            at_launch(Launch {
                hidden: true,
                ..launch()
            })
            .is_none(),
            "--hidden (login start)"
        );
        assert!(
            at_launch(Launch {
                test_instance: true,
                ..launch()
            })
            .is_none(),
            "test instance"
        );
        assert!(
            at_launch(Launch {
                headless: true,
                ..launch()
            })
            .is_none(),
            "headless"
        );
    }

    fn poll_at(s: &mut State, t: f64) -> bool {
        let mut tx = Tx::default();
        poll(s, t, &mut tx)
    }

    fn running() -> State {
        let mut s = State::fake();
        s.window = true;
        s.intro = at_launch(launch());
        s
    }

    /// The first frame was presented at the poll time `t` (as `on_frame` reports it).
    fn present(s: &mut State) {
        assert!(presented(s), "arms once");
    }

    #[test]
    fn it_runs_once_then_goes() {
        let mut s = running();
        present(&mut s);
        assert!(
            !poll_at(&mut s, 10.0),
            "armed: the hold starts, the cover stays in its start pose"
        );
        assert_eq!(s.intro.as_ref().map(|i| i.phase), Some(Phase::Start));
        assert!(!poll_at(&mut s, 10.0 + LEAD * 0.5));
        assert!(poll_at(&mut s, 10.0 + LEAD), "then it runs");
        assert_eq!(s.intro.as_ref().map(|i| i.phase), Some(Phase::Run));
        assert!(!poll_at(&mut s, 10.4));
        assert!(poll_at(&mut s, 10.0 + LEAD + TOTAL as f64));
        assert!(s.intro.is_none());
        const { assert!(TOTAL < 1.0, "under a second") };
        // Nothing brings it back: the window reopened from the tray only sets `window`.
        s.window = false;
        s.window = true;
        assert!(!poll_at(&mut s, 20.0) && s.intro.is_none());
    }

    #[test]
    fn the_clock_starts_at_the_first_presented_frame_not_before() {
        // A slow start: polls come (state made, window made) but no frame is presented for a
        // second. The cover stays in its start pose and nothing counts down.
        let mut s = running();
        for t in [0.0, 0.2, 0.5, 0.9, 1.0] {
            assert!(!poll_at(&mut s, t));
            let i = s.intro.as_ref().expect("still up");
            assert!(
                i.phase == Phase::Start && i.since.is_none() && !i.armed,
                "no clock at {t}"
            );
        }
        // The first frame lands at 1.0: the hold and the run are timed from there.
        present(&mut s);
        assert!(!poll_at(&mut s, 1.0));
        assert!(!poll_at(&mut s, 1.0 + LEAD * 0.9), "still holding");
        assert!(
            poll_at(&mut s, 1.0 + LEAD),
            "runs from the first frame + the hold"
        );
        assert!(
            !poll_at(&mut s, 1.0 + LEAD + TOTAL as f64 - 0.01),
            "the whole run is seen"
        );
        assert!(poll_at(&mut s, 1.0 + LEAD + TOTAL as f64));
        assert!(s.intro.is_none());
    }

    #[test]
    fn a_frame_before_the_window_shows_does_not_arm_it() {
        let mut s = running();
        s.window = false;
        assert!(!presented(&mut s), "no window yet");
        s.window = true;
        assert!(presented(&mut s), "the first frame with the window");
        assert!(!presented(&mut s), "once");
        s.intro = None;
        assert!(!presented(&mut s));
        // A scene's frozen frame never arms.
        s.intro = Some(Intro::frozen(0.5, false));
        assert!(!presented(&mut s));
    }

    #[test]
    fn the_cover_is_opaque_from_the_first_frame() {
        // The very first frame, before any poll, is the start pose: solid base, strips and name
        // at full opacity, claws unscaled-in; the app is not visible through it.
        let mut rt = runtime(live_late(), false);
        // A second of slow frames with no frame presented: still the start pose, still opaque.
        for _ in 0..30 {
            rt.frame(1.0 / 30.0);
        }
        let all = rt.elements();
        let op = |key: &str| {
            all.iter()
                .find(|e| e.key.as_deref() == Some(key))
                .map(|e| e.opacity)
        };
        for key in ["intro/base", "intro/strip/0", "intro/strip/3", "intro/name"] {
            assert!(
                op(key).is_some_and(|o| (o - 1.0).abs() < 1e-3),
                "{key}: {:?}",
                op(key)
            );
        }
        assert!(rt.state.intro.is_some(), "not dropped by the time passing");
    }

    #[test]
    fn closing_the_window_mid_intro_drops_it() {
        let mut s = running();
        present(&mut s);
        poll_at(&mut s, 0.0);
        s.window = false; // closed to the tray
        poll_at(&mut s, 0.1);
        assert!(s.intro.is_none());
    }

    #[test]
    fn a_windowless_start_waits_for_the_window() {
        // macOS: a launch checked for a login start has no window until it is known; a user's
        // launch then opens the window and plays it (a login start drops it: `login_launch`).
        let mut s = running();
        s.window = false;
        poll_at(&mut s, 0.0);
        assert!(
            s.intro
                .as_ref()
                .is_some_and(|i| i.since.is_none() && !i.armed),
            "waits, not started"
        );
        s.window = true;
        present(&mut s);
        poll_at(&mut s, 1.0);
        assert!(
            poll_at(&mut s, 1.0 + LEAD),
            "plays once the window shows and a frame is up"
        );
    }

    #[test]
    fn a_click_or_key_skips() {
        let mut s = running();
        present(&mut s);
        poll_at(&mut s, 0.0);
        skip(&mut s);
        assert!(s.intro.is_none());
    }

    #[test]
    fn reduce_motion_ends_sooner() {
        let mut s = running();
        if let Some(i) = &mut s.intro {
            i.reduced.set(true);
        }
        present(&mut s);
        poll_at(&mut s, 0.0);
        assert!(poll_at(&mut s, LEAD));
        assert!(poll_at(&mut s, LEAD + REDUCED_TOTAL as f64));
        assert!(s.intro.is_none());
    }

    fn runtime(s: State, reduced: bool) -> rune::Runtime<State> {
        let mut rt = rune::Runtime::new(
            s,
            crate::ui::view,
            (1280.0, 800.0),
            rune::Env {
                system_dark: true,
                scale: 1.0,
            },
        );
        rt.poll = Some(crate::ui::poll);
        rt.on_key = Some(crate::ui::on_key);
        rt.set_system_reduced_motion(reduced);
        rt.frame(1.0 / 60.0);
        rt
    }

    fn live() -> State {
        let mut s = State::fake();
        s.window = true;
        s.intro = Some(Intro::live());
        s
    }

    fn live_late() -> State {
        let mut s = State::fake();
        s.window = true;
        s.intro = Some(Intro::live_late(1.0));
        s
    }

    #[test]
    fn reduce_motion_draws_only_the_fade() {
        let rt = runtime(live(), true);
        assert!(rt.find("intro").is_some() && rt.find("intro/name").is_some());
        assert!(
            rt.find("intro/strip/0").is_none() && rt.find("intro/slash/0/cut").is_none(),
            "no claws, no strips"
        );
        let rt = runtime(live(), false);
        assert!(rt.find("intro/strip/3").is_some() && rt.find("intro/slash/2/glint").is_some());
    }

    #[test]
    fn a_click_skips_and_the_app_takes_it_not() {
        let mut rt = runtime(live(), false);
        let page = rt.state.page;
        rt.pointer_down(60.0, 300.0); // over the sidebar's items
        rt.pointer_up(60.0, 300.0);
        rt.frame(1.0 / 60.0);
        assert!(rt.state.intro.is_none(), "skipped");
        assert_eq!(rt.state.page, page, "the click didn't reach the sidebar");
    }

    #[test]
    fn a_key_skips() {
        let mut rt = runtime(live(), false);
        rt.key(rune::KeyEvent {
            key: rune::Key::Text("k".into()),
            mods: Default::default(),
        });
        rt.frame(1.0 / 60.0);
        assert!(rt.state.intro.is_none());
    }

    #[test]
    fn the_settings_switch_is_on_by_default() {
        assert!(crate::now::Prefs::default().intro);
        assert!(crate::now::Prefs::from_config(&Default::default()).intro);
    }

    #[test]
    fn the_strips_and_marks_are_simple_shapes() {
        for k in 0..4 {
            assert!(strip(k).len() >= SAMPLES, "strip {k}");
        }
        for k in 0..3 {
            let m = mark(k, 30.0, 0.0);
            assert_eq!(m.len(), 2 * (SAMPLES + 1));
            assert!(m.iter().all(|p| p[0].is_finite() && p[1].is_finite()));
        }
        // The claws' edges never cross, out to their far ends: each strip is a simple polygon.
        for i in 0..SAMPLES + 3 {
            let [a, b, c] = [0, 1, 2].map(|k| edge(k)[i][0]);
            assert!(a < b && b < c, "at {i}: {a} {b} {c}");
        }
        // They reach further than any strip travels.
        for k in 0..4 {
            let [x, y] = strip_away(k);
            assert!(x.hypot(y) < REACH, "strip {k}");
        }
    }

    #[test]
    fn tracks_finish_within_the_total() {
        let mut end: f32 = 0.0;
        for tr in [
            slash_track(2),
            CUT_FADE,
            STRIP_MOVE,
            STRIP_FADE,
            NAME_FADE,
            BASE_OFF,
        ] {
            end = end.max(tr.at + tr.dur);
        }
        assert!(end <= TOTAL + 1e-3, "{end}");
        const { assert!(REDUCED_FADE.at + REDUCED_FADE.dur <= REDUCED_TOTAL + 1e-3) };
        // Slashes about 90 ms apart.
        assert!((slash_track(1).at - slash_track(0).at - 0.09).abs() < 1e-3);
    }

    // ---- a window that resizes as it opens ----

    #[test]
    fn the_hold_waits_for_the_size_to_settle_up_to_a_cap() {
        const EPS: f64 = 1e-6;
        // No resize: the approved hold, `LEAD`, and not a millisecond more.
        assert!(matches!(hold(10.0, None, 10.0), Hold::Wait(l) if (l - LEAD).abs() < EPS));
        assert_eq!(hold(10.0, None, 10.0 + LEAD), Hold::Go);
        // A resize at 50 ms: the run waits for it to be still for SETTLE (until 170 ms).
        let changed = Some(10.05);
        assert!(
            matches!(hold(10.0, changed, 10.0 + LEAD), Hold::Wait(l) if (l - 0.07).abs() < EPS)
        );
        assert_eq!(hold(10.0, changed, 10.0 + 0.05 + SETTLE), Hold::Go);
        // A window that keeps changing: gone at the cap, from the first frame.
        assert!(
            matches!(hold(10.0, Some(10.39), 10.39), Hold::Wait(l) if l <= MAX_WAIT - 0.39 + EPS)
        );
        assert_eq!(hold(10.0, Some(10.39), 10.0 + MAX_WAIT), Hold::Go);
        // A resize before the first frame that has settled by then costs nothing.
        assert_eq!(hold(10.0, Some(9.8), 10.0 + LEAD), Hold::Go);
        const { assert!(SETTLE > 0.0 && MAX_WAIT > LEAD + SETTLE) };
    }

    fn resize_to(s: &mut State, size: [f32; 2]) {
        s.main_size.set(size);
    }

    #[test]
    fn a_resize_at_open_delays_the_start_and_the_run_still_plays_whole() {
        let mut s = running();
        resize_to(&mut s, [1280.0, 800.0]);
        present(&mut s);
        assert!(!poll_at(&mut s, 5.0));
        // The window manager enlarges it in steps at 40, 80 and 120 ms.
        for (t, size) in [
            (5.04, [1400.0, 860.0]),
            (5.08, [1550.0, 930.0]),
            (5.12, [1700.0, 1000.0]),
        ] {
            resize_to(&mut s, size);
            assert!(!poll_at(&mut s, t), "still solid orange at {t}");
            assert_eq!(s.intro.as_ref().map(|i| i.phase), Some(Phase::Start));
        }
        // LEAD alone has long passed, but the size changed at 5.12.
        assert!(
            !poll_at(&mut s, 5.12 + SETTLE - 0.02),
            "waits for SETTLE after the last change"
        );
        assert!(
            poll_at(&mut s, 5.12 + SETTLE),
            "starts once the size has been still"
        );
        assert_eq!(s.intro.as_ref().map(|i| i.phase), Some(Phase::Run));
        // Played whole from there, unchanged.
        assert!(!poll_at(&mut s, 5.12 + SETTLE + TOTAL as f64 - 0.01));
        assert!(poll_at(&mut s, 5.12 + SETTLE + TOTAL as f64));
        assert!(s.intro.is_none());
    }

    #[test]
    fn a_window_that_never_settles_starts_at_the_cap() {
        let mut s = running();
        resize_to(&mut s, [1000.0, 700.0]);
        present(&mut s);
        poll_at(&mut s, 2.0);
        let mut t = 2.0;
        let mut w = 1000.0;
        while t < 2.0 + MAX_WAIT - 0.05 {
            t += 0.03;
            w += 10.0;
            resize_to(&mut s, [w, 700.0]);
            assert!(!poll_at(&mut s, t), "waiting at {t}");
        }
        assert!(poll_at(&mut s, 2.0 + MAX_WAIT), "the cap");
    }

    #[test]
    fn the_first_layout_and_an_unlaid_out_window_are_not_resizes() {
        let mut s = running();
        present(&mut s);
        poll_at(&mut s, 3.0); // main_size is still [0, 0]
        resize_to(&mut s, [1280.0, 800.0]); // the first real size
        poll_at(&mut s, 3.03);
        assert!(s.intro.as_ref().is_some_and(|i| i.changed.is_none()));
        assert!(poll_at(&mut s, 3.0 + LEAD), "the plain hold");
    }

    #[test]
    fn a_resize_mid_run_does_not_cut_the_run_short() {
        // The run is not restarted or held by a late resize either.
        let mut s = running();
        resize_to(&mut s, [1280.0, 800.0]);
        present(&mut s);
        poll_at(&mut s, 1.0);
        assert!(poll_at(&mut s, 1.0 + LEAD));
        resize_to(&mut s, [1700.0, 1000.0]);
        assert!(!poll_at(&mut s, 1.0 + LEAD + 0.2));
        assert_eq!(s.intro.as_ref().map(|i| i.phase), Some(Phase::Run));
        assert!(poll_at(&mut s, 1.0 + LEAD + TOTAL as f64));
        assert!(s.intro.is_none());
    }

    #[test]
    fn the_resize_rule_does_not_snap_the_intro() {
        // `root` lands a window resize's changes at once; the intro is outside it, so a strip
        // that is mid-slide when the window grows keeps sliding, where it would jump to the end.
        let mut rt = runtime(live(), false);
        for _ in 0..42 {
            rt.frame(1.0 / 60.0); // 0.1 s hold + about 0.6 s of the run
        }
        let info = |rt: &rune::Runtime<State>| {
            rt.elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("intro/strip/3"))
                .expect("the strip")
        };
        let before = info(&rt);
        assert!(
            before.rect != before.target,
            "mid-slide: {:?} {:?}",
            before.rect,
            before.target
        );
        rt.set_viewport((1700.0, 1000.0), 1.0);
        rt.frame(1.0 / 60.0);
        assert!(rt.state.intro.is_some(), "still playing");
        let after = info(&rt);
        assert!(
            after.rect != after.target,
            "not snapped to its end: {:?} {:?}",
            after.rect,
            after.target
        );
        assert!(after.moving, "its tween runs on");
        // While the page under it did reflow at once (the rule still holds for `root`).
        let root = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("root"))
            .expect("root");
        assert_eq!(root.target[2].round() as i32, 1700);
    }

    // ---- the geometry follows the window ----

    #[test]
    fn the_box_covers_the_window_at_any_size_and_shape() {
        for size in [
            [1280.0, 800.0],
            [1700.0, 1000.0],
            [960.0, 600.0],
            [2560.0, 1300.0],
            [900.0, 1400.0],
            [3440.0, 1440.0],
        ] {
            let f = Fit::new(size);
            let (a, b) = (f.pt([0.0, 0.0]), f.pt([VW, VH]));
            assert!(
                a[0] <= 0.5 && a[1] <= 0.5 && b[0] >= size[0] - 0.5 && b[1] >= size[1] - 0.5,
                "{size:?} covered"
            );
            let m = f.pt([VW / 2.0, VH / 2.0]);
            assert!(
                (m[0] - size[0] / 2.0).abs() < 1e-3 && (m[1] - size[1] / 2.0).abs() < 1e-3,
                "centred"
            );
            // One scale on both axes: a claw keeps its angle.
            let (p, q) = (f.pt([100.0, 100.0]), f.pt([200.0, 300.0]));
            assert!(
                ((q[0] - p[0]) / (q[1] - p[1]) - 0.5).abs() < 1e-4,
                "angle kept"
            );
        }
        // The same window: the identity at the design size.
        assert_eq!(Fit::new([VW, VH]).pt([321.0, 654.0]), [321.0, 654.0]);
    }

    #[test]
    fn the_shapes_are_drawn_for_the_current_size_not_the_first() {
        // The shapes' view box is the window itself (1:1, nothing stretched), at the first
        // size and after a resize.
        let mut rt = runtime(live(), false);
        rt.set_viewport((1700.0, 1000.0), 1.0);
        rt.frame(1.0 / 60.0);
        let cover = |rt: &rune::Runtime<State>, key: &str| {
            rt.elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key))
                .map(|e| e.target)
        };
        for key in ["intro", "intro/strip/0", "intro/slash/1/cut"] {
            let [x, y, w, h] = cover(&rt, key).expect(key);
            assert!(
                x.abs() < 0.5
                    && y.abs() < 0.5
                    && (w - 1700.0).abs() < 0.5
                    && (h - 1000.0).abs() < 0.5,
                "{key}: {x} {y} {w} {h}"
            );
        }
    }
}
