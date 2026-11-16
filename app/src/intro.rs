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
//! - **When:** only on a real cold start with a window (`at_launch`): not a `--hidden` or login
//!   start in the background, not a test instance, not headless (scenes freeze frames of it), and
//!   never when the window comes back from the tray or menu bar. Settings → Appearance →
//!   Startup animation turns it off (`[display] intro`).
//! - **Reduce Motion** (macOS, GNOME `enable-animations`, KDE; Rune reads them): the orange
//!   layer only fades out.
//! - **Motion:** every piece is a vector shape (`Shape` fills, no images), in one 1600×1000 box
//!   stretched to the window. Each track below is a tween Rune runs itself, set once when the
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

/// The shapes' coordinate box, stretched to the window.
const VW: f32 = 1600.0;
const VH: f32 = 1000.0;

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
/// The whole intro; it is dropped after this.
pub const TOTAL: f32 = 0.8;
/// Reduce Motion: the layer holds a moment, then fades.
const REDUCED_FADE: Track = Track::new(0.12, 0.25, Easing::EASE_OUT);
pub const REDUCED_TOTAL: f32 = 0.4;

fn slash_track(k: usize) -> Track {
    Track::new(HOLD + SLASH_GAP * k as f32, SLASH, Easing::cubic(0.3, 0.0, 0.2, 1.0))
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
    let mid = [(p0[0] + p2[0]) / 2.0 + n[0] * bow, (p0[1] + p2[1]) / 2.0 + n[1] * bow];
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
fn slash_shapes(k: usize) -> [(&'static str, Shape); 3] {
    let w = [30.0, 38.0, 32.0][k];
    let one = |pts: Vec<[f32; 2]>, color: Color| Shape::new().fill(pts, color).view_box(VW, VH);
    [
        ("rim", one(mark(k, w * 1.5, 0.0), rim())),
        ("cut", one(mark(k, w, 0.0), PAL.bg)),
        ("glint", one(mark(k, w * 0.26, 0.24), hex("#fff6e6").alpha(0.95))),
    ]
}

/// The torn rim round a cut: a lighter orange than the layer.
fn rim() -> Color {
    hex("#ffb866")
}

fn hex(s: &str) -> Color {
    rune::color::hex(s).unwrap_or(Color::WHITE)
}

fn strip_shape(k: usize) -> Shape {
    Shape::new().fill(strip(k), PAL.accent).view_box(VW, VH)
}

/// 小虎's colour over the orange: cream in the dark theme, the dark brown text on the accent
/// has in the light one.
fn name_color() -> Color {
    if PAL.light { PAL.on_accent } else { hex("#fff7ec") }
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
        Intro { phase: Phase::Start, armed: false, since: None, arm_at: None, frozen: None, reduced: Cell::new(false), force_reduced: false }
    }

    /// Scenes: the intro held at `progress` (0 to 1) of its run.
    pub fn frozen(progress: f32, reduced: bool) -> Intro {
        let total = if reduced { REDUCED_TOTAL } else { TOTAL };
        Intro { frozen: Some(progress.clamp(0.0, 1.0) * total), force_reduced: reduced, ..Intro::new() }
    }

    /// Scenes: the live intro, which a headless `--video` records (its clock runs there).
    pub fn live() -> Intro {
        Intro { armed: true, ..Intro::new() }
    }

    /// Scenes: the live intro whose first frame comes `late` seconds in (a slow window or first
    /// view): it must wait in the start pose, then play whole.
    pub fn live_late(late: f64) -> Intro {
        Intro { arm_at: Some(late), ..Intro::new() }
    }

    /// Scenes: the live Reduce Motion fade.
    pub fn live_reduced() -> Intro {
        Intro { armed: true, force_reduced: true, ..Intro::new() }
    }
}

/// Any click or key: the layer goes at once (its own fade-out, 0.15 s).
pub fn skip(s: &mut State) {
    s.intro = None;
}

/// `AppSpec::poll`: starts the run once the window shows, and drops the intro when it is over.
/// The window going (closed to the tray mid-intro) drops it too: it never replays later.
pub fn poll(s: &mut State, t: f64, tx: &mut Tx) -> bool {
    let Some(i) = s.intro.as_mut() else { return false };
    if i.frozen.is_some() {
        return false;
    }
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
            tx.wake_after(Duration::from_secs_f64(LEAD));
            false
        }
        (Phase::Start, Some(since)) if t - since >= LEAD - 1e-3 => {
            i.phase = Phase::Run;
            i.since = Some(t);
            tx.animated();
            tx.wake_after(Duration::from_secs_f32(TOTAL));
            true
        }
        (Phase::Start, Some(since)) => {
            tx.wake_after(Duration::from_secs_f64((LEAD - (t - since)).max(0.0)));
            false
        }
        (Phase::Run, Some(since)) => {
            let total = if i.reduced.get() || i.force_reduced { REDUCED_TOTAL } else { TOTAL } as f64;
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
    let pin = Some(0.0);
    let layer = el().key("intro").role("image").label("Kotora").pin(pin, pin, pin, pin)
        // Esc, any click or key skips; the app underneath takes none until the layer is gone.
        .modal(false, skip)
        .on_press(skip);
    let name = el().key("intro/name").pin(pin, pin, pin, pin).center().child(crate::kanji::name(132.0, true, name_color()));

    if reduced {
        let fade = |e: El, track: Track| set_opacity(e, 1.0 - track.at_time(t), motion.then_some(track));
        let layer = fade(layer.fill(PAL.accent), REDUCED_FADE);
        return Some(layer.child(name));
    }

    let base = el().key("intro/base").pin(pin, pin, pin, pin).fill(PAL.accent);
    let mut layer = layer.child(set_opacity(base, 1.0 - BASE_OFF.at_time(t), motion.then_some(BASE_OFF)));
    for k in 0..4 {
        let away = strip_away(k);
        let m = STRIP_MOVE.at_time(t);
        let e = el().key(format!("intro/strip/{k}")).pin(pin, pin, pin, pin).shape(strip_shape(k));
        let e = set_offset(e, [away[0] * m, away[1] * m], motion.then_some(STRIP_MOVE));
        layer = layer.child(set_opacity(e, 1.0 - STRIP_FADE.at_time(t), motion.then_some(STRIP_FADE)));
    }
    let name = set_opacity(name, 1.0 - NAME_FADE.at_time(t), motion.then_some(NAME_FADE));
    layer = layer.child(name);
    for k in 0..3 {
        let track = slash_track(k);
        let c = claw(k);
        // The mark grows out of its starting point, so it rips across from the top right.
        for (part, shape) in slash_shapes(k) {
            let e = el().key(format!("intro/slash/{k}/{part}")).pin(pin, pin, pin, pin).shape(shape).origin(c[0][0] / VW, c[0][1] / VH);
            let e = set_scale(e, track.at_time(t).max(0.001), motion.then_some(track));
            layer = layer.child(set_opacity(e, 1.0 - CUT_FADE.at_time(t), motion.then_some(CUT_FADE)));
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
        Launch { enabled: true, hidden: false, test_instance: false, headless: false }
    }

    #[test]
    fn only_a_real_cold_start_with_a_window_plays_it() {
        assert!(at_launch(launch()).is_some(), "a cold start with the window");
        assert!(at_launch(Launch { enabled: false, ..launch() }).is_none(), "Settings switch off");
        assert!(at_launch(Launch { hidden: true, ..launch() }).is_none(), "--hidden (login start)");
        assert!(at_launch(Launch { test_instance: true, ..launch() }).is_none(), "test instance");
        assert!(at_launch(Launch { headless: true, ..launch() }).is_none(), "headless");
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
        assert!(!poll_at(&mut s, 10.0), "armed: the hold starts, the cover stays in its start pose");
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
            assert!(i.phase == Phase::Start && i.since.is_none() && !i.armed, "no clock at {t}");
        }
        // The first frame lands at 1.0: the hold and the run are timed from there.
        present(&mut s);
        assert!(!poll_at(&mut s, 1.0));
        assert!(!poll_at(&mut s, 1.0 + LEAD * 0.9), "still holding");
        assert!(poll_at(&mut s, 1.0 + LEAD), "runs from the first frame + the hold");
        assert!(!poll_at(&mut s, 1.0 + LEAD + TOTAL as f64 - 0.01), "the whole run is seen");
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
        let op = |key: &str| all.iter().find(|e| e.key.as_deref() == Some(key)).map(|e| e.opacity);
        for key in ["intro/base", "intro/strip/0", "intro/strip/3", "intro/name"] {
            assert!(op(key).is_some_and(|o| (o - 1.0).abs() < 1e-3), "{key}: {:?}", op(key));
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
        assert!(s.intro.as_ref().is_some_and(|i| i.since.is_none() && !i.armed), "waits, not started");
        s.window = true;
        present(&mut s);
        poll_at(&mut s, 1.0);
        assert!(poll_at(&mut s, 1.0 + LEAD), "plays once the window shows and a frame is up");
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
        let mut rt = rune::Runtime::new(s, crate::ui::view, (1280.0, 800.0), rune::Env { system_dark: true, scale: 1.0 });
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
        assert!(rt.find("intro/strip/0").is_none() && rt.find("intro/slash/0/cut").is_none(), "no claws, no strips");
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
        rt.key(rune::KeyEvent { key: rune::Key::Text("k".into()), mods: Default::default() });
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
        for tr in [slash_track(2), CUT_FADE, STRIP_MOVE, STRIP_FADE, NAME_FADE, BASE_OFF] {
            end = end.max(tr.at + tr.dur);
        }
        assert!(end <= TOTAL + 1e-3, "{end}");
        const { assert!(REDUCED_FADE.at + REDUCED_FADE.dur <= REDUCED_TOTAL + 1e-3) };
        // Slashes about 90 ms apart.
        assert!((slash_track(1).at - slash_track(0).at - 0.09).abs() < 1e-3);
    }
}
