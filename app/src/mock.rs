//! THROWAWAY design mockups (branch `mockups`, never merged). Scene-only code: made-up poster art,
//! a blurred backdrop computed once on the CPU into a small texture, and variant layouts for the
//! list grid, Now Playing hero, details header and sidebar mini card.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use kotora_core::Status;
use rune::{Color, Cx, El, Icon, Image, ImageCache, ScrollSpec, SurfaceFit, TextStyle, col, el, icon, row, spacer, text};

use crate::icons;
use crate::model::{Air, Entry, State};
use crate::theme::{self, PAL};
use crate::ui;

// ------------------------------------------------------------------------------ mode switch

static MODE: Mutex<Option<&'static str>> = Mutex::new(None);

pub fn set(m: &'static str) {
    *MODE.lock().unwrap() = Some(m);
}

pub fn active() -> Option<&'static str> {
    *MODE.lock().unwrap()
}

/// The sidebar mini-card variant, if a mini mockup is active.
pub fn mini_variant() -> Option<char> {
    active().and_then(|m| m.strip_prefix("mini-")).and_then(|v| v.chars().next())
}

// ------------------------------------------------------------------------------ art

thread_local! {
    static CACHE: ImageCache = ImageCache::new(Arc::new(|| {}));
    static IMGS: RefCell<HashMap<(u32, u32, u8), Image>> = RefCell::new(HashMap::new());
}

const PW: u32 = 300;
const PH: u32 = 450;

/// Palettes: sky top, sky bottom, sun, far hills, near hills, night (stars).
const PALS: [([u8; 3], [u8; 3], [u8; 3], [u8; 3], [u8; 3], bool); 10] = [
    ([28, 36, 84], [236, 128, 72], [255, 222, 160], [92, 52, 96], [36, 24, 52], false),
    ([18, 60, 92], [120, 200, 210], [250, 250, 220], [40, 110, 110], [18, 52, 60], false),
    ([12, 14, 40], [60, 52, 120], [240, 236, 255], [40, 36, 90], [16, 14, 40], true),
    ([250, 170, 120], [255, 226, 180], [255, 120, 90], [200, 100, 90], [110, 50, 60], false),
    ([30, 90, 60], [200, 230, 150], [255, 250, 210], [70, 140, 80], [30, 70, 40], false),
    ([70, 20, 60], [230, 90, 110], [255, 200, 140], [120, 40, 80], [50, 16, 40], false),
    ([10, 24, 50], [30, 120, 160], [180, 240, 255], [20, 70, 100], [8, 30, 50], true),
    ([120, 160, 230], [240, 220, 250], [255, 255, 255], [140, 130, 200], [80, 70, 140], false),
    ([40, 20, 10], [210, 110, 40], [255, 210, 120], [110, 50, 20], [50, 20, 10], false),
    ([60, 70, 90], [180, 190, 200], [255, 140, 60], [90, 100, 120], [40, 46, 60], false),
];

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}
fn f3(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32, c[1] as f32, c[2] as f32]
}
fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn hash(x: u32, y: u32, s: u32) -> u32 {
    let mut h = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ s.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

/// A made-up illustrated poster: sky, sun or moon, two hill lines, a fake logo and a title band.
fn poster_rgb(id: u32) -> Vec<[f32; 3]> {
    let (top, bot, sun, far, near, night) = PALS[id as usize % PALS.len()];
    let (top, bot, sun, far, near) = (f3(top), f3(bot), f3(sun), f3(far), f3(near));
    let seed = id as f32 * 1.37;
    let sx = 0.3 + 0.4 * ((id * 7 % 10) as f32 / 10.0);
    let (w, h) = (PW as f32, PH as f32);
    let mut out = Vec::with_capacity((PW * PH) as usize);
    for y in 0..PH {
        for x in 0..PW {
            let (u, v) = (x as f32 / w, y as f32 / h);
            let mut c = lerp3(top, bot, v.powf(0.9).min(1.0));
            if night && hash(x, y, id) % 900 == 0 && v < 0.55 {
                c = lerp3(c, [255.0; 3], 0.8);
            }
            // Sun with glow.
            let d = ((u - sx).powi(2) + ((v - 0.34) * h / w).powi(2)).sqrt();
            let r = 0.15;
            c = lerp3(c, sun, 0.45 * (-(d - r).max(0.0) / 0.10).exp() * (1.0 - smooth(r - 0.005, r + 0.005, d)).max(0.35));
            c = lerp3(c, sun, 1.0 - smooth(r - 0.004, r + 0.004, d));
            // Far hills.
            let yf = 0.60 + 0.05 * (u * 7.5 + seed).sin() + 0.025 * (u * 19.0 + seed * 2.0).sin();
            let af = smooth(yf - 0.003, yf + 0.003, v);
            c = lerp3(c, lerp3(far, near, ((v - yf) * 2.0).clamp(0.0, 0.6)), af);
            // Near hills.
            let yn = 0.74 + 0.06 * (u * 4.2 + seed * 1.7).sin() + 0.015 * (u * 31.0 + seed).sin();
            let an = smooth(yn - 0.003, yn + 0.003, v);
            c = lerp3(c, lerp3(near, [8.0, 8.0, 14.0], ((v - yn) * 1.6).clamp(0.0, 0.7)), an);
            // Fake logo bars near the top.
            let in_logo = (0.865..0.895).contains(&v) && (u - 0.5).abs() < 0.28 || (0.905..0.918).contains(&v) && (u - 0.5).abs() < 0.16;
            if in_logo {
                c = lerp3(c, [250.0, 248.0, 240.0], 0.92);
            }
            // Vignette.
            let vig = 1.0 - 0.35 * (((u - 0.5) * 1.4).powi(2) + ((v - 0.45) * 1.1).powi(2));
            c = [c[0] * vig, c[1] * vig, c[2] * vig];
            out.push(c);
        }
    }
    out
}

fn to_rgba(px: &[[f32; 3]]) -> Vec<u8> {
    let mut v = Vec::with_capacity(px.len() * 4);
    for c in px {
        v.extend([c[0].clamp(0.0, 255.0) as u8, c[1].clamp(0.0, 255.0) as u8, c[2].clamp(0.0, 255.0) as u8, 255]);
    }
    v
}

fn image(key: (u32, u32, u8), make: impl FnOnce() -> (u32, u32, Vec<u8>)) -> Image {
    if let Some(i) = IMGS.with(|m| m.borrow().get(&key).cloned()) {
        return i;
    }
    let (w, h, px) = make();
    let img = CACHE.with(|c| c.rgba(w, h, px));
    let _ = img.wait();
    IMGS.with(|m| m.borrow_mut().insert(key, img.clone()));
    img
}

/// The poster at 300 x 450 (display size x2 for the large cards).
pub fn poster_img(id: u32) -> Image {
    image((id, 0, 0), || (PW, PH, to_rgba(&poster_rgb(id))))
}

/// The blurred, darkened backdrop for an area of `aspect` (w / h): crop the poster's middle to the
/// aspect, area-downscale to 96 px wide, three box-blur passes, darken. Done ONCE per (cover,
/// aspect) off the frame path; the GPU only stretches a ~96 x 30 texture (no per-frame blur).
pub fn backdrop_img(id: u32, aspect: f32, dark: f32) -> Image {
    let key = (id, (aspect * 100.0) as u32, (dark * 100.0) as u8);
    image(key, || {
        let src = poster_rgb(id);
        let ow = 96u32;
        let oh = ((ow as f32 / aspect).round() as u32).max(8);
        // Crop rectangle in poster pixels.
        // The whole poster squashed into the area: a wash of all its colours, top to bottom.
        let (cw, ch, cx, cy) = (PW as f32, PH as f32 * 0.86, 0.0, 0.0);
        let mut buf = vec![[0.0f32; 3]; (ow * oh) as usize];
        for oy in 0..oh {
            for ox in 0..ow {
                let x0 = (cx + ox as f32 * cw / ow as f32) as u32;
                let x1 = ((cx + (ox + 1) as f32 * cw / ow as f32) as u32).max(x0 + 1).min(PW);
                let y0 = (cy + oy as f32 * ch / oh as f32) as u32;
                let y1 = ((cy + (oy + 1) as f32 * ch / oh as f32) as u32).max(y0 + 1).min(PH);
                let mut acc = [0.0; 3];
                let mut n = 0.0;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let p = src[(y * PW + x) as usize];
                        acc = [acc[0] + p[0], acc[1] + p[1], acc[2] + p[2]];
                        n += 1.0;
                    }
                }
                buf[(oy * ow + ox) as usize] = [acc[0] / n, acc[1] / n, acc[2] / n];
            }
        }
        for _ in 0..3 {
            buf = box_blur(&buf, ow, oh, 6);
        }
        let bg = [15.0, 22.0, 38.0];
        for p in &mut buf {
            // Saturate (the blur greys colours out), then darken toward the page.
            let l = 0.3 * p[0] + 0.55 * p[1] + 0.15 * p[2];
            let sat = [l + (p[0] - l) * 1.5, l + (p[1] - l) * 1.5, l + (p[2] - l) * 1.5];
            *p = lerp3(sat, bg, dark);
        }
        (ow, oh, to_rgba(&buf))
    })
}

fn box_blur(src: &[[f32; 3]], w: u32, h: u32, r: i32) -> Vec<[f32; 3]> {
    let (w, h) = (w as i32, h as i32);
    let at = |b: &[[f32; 3]], x: i32, y: i32| b[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize];
    let n = (2 * r + 1) as f32;
    let mut tmp = vec![[0.0; 3]; src.len()];
    for y in 0..h {
        for x in 0..w {
            let mut a = [0.0; 3];
            for k in -r..=r {
                let p = at(src, x + k, y);
                a = [a[0] + p[0], a[1] + p[1], a[2] + p[2]];
            }
            tmp[(y * w + x) as usize] = [a[0] / n, a[1] / n, a[2] / n];
        }
    }
    let mut out = vec![[0.0; 3]; src.len()];
    for y in 0..h {
        for x in 0..w {
            let mut a = [0.0; 3];
            for k in -r..=r {
                let p = at(&tmp, x, y + k);
                a = [a[0] + p[0], a[1] + p[1], a[2] + p[2]];
            }
            out[(y * w + x) as usize] = [a[0] / n, a[1] / n, a[2] / n];
        }
    }
    out
}

fn poster(id: u32, w: f32, h: f32, r: f32) -> El {
    el().size(w, h).radius(r).clip_rounded(r).fill(PAL.raised).pixels_fit(poster_img(id).pixels(), SurfaceFit::Fill)
}

/// A blurred backdrop filling its parent (pin all edges).
fn backdrop(id: u32, aspect: f32, dark: f32) -> El {
    el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).pixels_fit(backdrop_img(id, aspect, dark).pixels(), SurfaceFit::Fill)
}

// ------------------------------------------------------------------------------ icons

static I_LIST: Icon = Icon {
    name: "mock-list",
    view: 24.0,
    body: r#"<path d="M8 6h12M8 12h12M8 18h12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><circle cx="4.5" cy="6" r="1" fill="currentColor"/><circle cx="4.5" cy="12" r="1" fill="currentColor"/><circle cx="4.5" cy="18" r="1" fill="currentColor"/>"#,
};
static I_GRID: Icon = Icon {
    name: "mock-grid",
    view: 24.0,
    body: r#"<rect x="4" y="4" width="6.5" height="6.5" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.6"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.6"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.6"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.6"/>"#,
};
static I_SHELF: Icon = Icon {
    name: "mock-shelf",
    view: 24.0,
    body: r#"<rect x="3.5" y="5" width="17" height="5.5" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.6"/><rect x="3.5" y="13.5" width="17" height="5.5" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.6"/>"#,
};
static I_STAR: Icon = Icon {
    name: "mock-star",
    view: 24.0,
    body: r#"<path d="M12 3.5l2.6 5.4 5.9.8-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.8z" fill="currentColor"/>"#,
};
static I_BACK: Icon = Icon {
    name: "mock-back",
    view: 24.0,
    body: r#"<path d="M15 6l-6 6 6 6" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
static I_PENCIL: Icon = Icon {
    name: "mock-pencil",
    view: 24.0,
    body: r#"<path d="M4 20h4L19 9l-4-4L4 16z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>"#,
};

// ------------------------------------------------------------------------------ shell

pub fn view(s: &State, m: &'static str, cx: &mut Cx<State>) -> El {
    s.covers.set_scale(cx.env().scale);
    let w = cx.layout_size("root").map_or(1280.0, |[w, _]| w);
    let content_w = w - theme::SIDEBAR_W - 1.0;
    let page = match m {
        "grid-a" => grid_page(s, content_w, GridKind::Dense),
        "grid-b" => grid_page(s, content_w, GridKind::Large),
        "grid-c" => shelf_page(s, content_w),
        "now-a" => now_a(s),
        "now-b" => now_b(s),
        "details-a" => details_a(s),
        "details-b" => details_b(s),
        _ => ui::main_pub(s, w >= theme::NARROW_W),
    };
    row()
        .key("root")
        .w_full()
        .h_full()
        .fill(PAL.bg)
        .child(ui::sidebar_pub(s))
        .child(el().w(1.0).h_full().fill(PAL.border))
        .child(col().key("main").grow(1.0).min_w(0.0).h_full().child(page))
}

fn watching(s: &State) -> Vec<&Entry> {
    s.rows.iter().map(|&k| &s.entries[k]).collect()
}

// ------------------------------------------------------------------------------ 1. grid

#[derive(Clone, Copy, PartialEq)]
enum GridKind {
    Dense,
    Large,
}

fn toggle(mode: u8) -> El {
    let seg = |k: &str, ic: &'static Icon, label: &str, on: bool| {
        let r = row().key(k.to_string()).h(26.0).px(8.0).gap(6.0).center_y().radius(theme::R_XS);
        let r = if on { r.fill(PAL.raised).border(1.0, PAL.border_strong) } else { r.hover(PAL.hover()) };
        r.child(icon(ic, 15.0).color(if on { PAL.accent_hi } else { PAL.faint }))
            .child(text(label.to_string()).font(theme::LABEL_BTN).color(if on { PAL.text } else { PAL.faint }))
    };
    row()
        .key("view-toggle")
        .h(32.0)
        .padding(2.0)
        .gap(2.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, PAL.border)
        .child(seg("vt/list", &I_LIST, "List", mode == 0))
        .child(seg("vt/grid", &I_GRID, "Grid", mode == 1))
        .child(if mode == 2 { seg("vt/shelves", &I_SHELF, "Shelves", true) } else { el() })
}

fn grid_top(s: &State, title: &str, count: Option<usize>, mode: u8) -> El {
    row()
        .key("top")
        .gap(12.0)
        .center_y()
        .px(theme::PAGE_PAD)
        .pt(18.0)
        .pb(14.0)
        .child(
            row()
                .gap(6.0)
                .items_end()
                .child(text(title.to_string()).font(theme::TITLE).color(PAL.text))
                .child(text(count.map_or(String::new(), |c| c.to_string())).font(theme::TITLE_NUM).color(PAL.faint)),
        )
        .child(spacer())
        .child(ui::search_box_pub(s).w(220.0))
        .child(ui::sort_select_pub(s.sort))
        .child(toggle(mode))
}

/// Badge for the top-left of a poster: dark glass pill with a coloured dot.
fn poster_badge(label: &str, c: Color) -> El {
    row()
        .h(20.0)
        .px(7.0)
        .gap(5.0)
        .center_y()
        .radius(10.0)
        .fill(Color::BLACK.alpha(0.62))
        .child(el().size(6.0, 6.0).radius(3.0).fill(c))
        .child(text(label.to_string()).font(theme::BADGE).color(Color::WHITE.alpha(0.92)))
}

fn new_ep_badge(n: u32) -> El {
    row().h(20.0).px(7.0).center_y().radius(10.0).fill(PAL.accent).child(text(format!("Ep {n} out")).font(theme::BADGE).color(PAL.on_accent))
}

fn badges(e: &Entry) -> El {
    let mut c = col().absolute(6.0, 6.0).gap(4.0).items_start();
    if e.rewatching {
        c = c.child(poster_badge("Rewatching", PAL.st_completed));
    } else if e.air == Air::Airing {
        c = c.child(poster_badge(e.airing_note.map_or("Airing", |n| n.strip_prefix("Ep ").map_or(n, |_| n)), PAL.airing));
    }
    if let Some(n) = e.new_episode {
        c = c.child(new_ep_badge(n));
    }
    c
}

fn ep_text(e: &Entry) -> String {
    match e.episodes {
        Some(t) => format!("Ep {} / {t}", e.progress),
        None => format!("Ep {} / ?", e.progress),
    }
}

fn bar(frac: f32, c: Color, h: f32, track: Color) -> El {
    row().w_full().h(h).clip_rounded(h / 2.0).fill(track).child(el().grow(frac.max(0.001)).h_full().radius(h / 2.0).fill(c)).child(el().grow((1.0 - frac).max(0.001)))
}

fn score_el(sc: Option<u8>) -> El {
    match sc {
        Some(v) => row().gap(3.0).center_y().child(icon(&I_STAR, 12.0).color(PAL.st_onhold)).child(text(v.to_string()).font(theme::NUM_MED).color(PAL.dim)),
        None => el(),
    }
}

fn plus_big(hot: bool) -> El {
    let b = el().size(46.0, 32.0).radius(theme::R_SM).center();
    let b = if hot { b.fill(PAL.accent) } else { b.fill(Color::BLACK.alpha(0.55)).border(1.0, Color::WHITE.alpha(0.25)) };
    b.child(text("+1").font(TextStyle::new(14.0, 600.0)).color(if hot { PAL.on_accent } else { Color::WHITE }))
}

/// The hover layer (shown forced on one card; real code: `hover_group` + `group_opacity(0, 1)`).
fn hover_layer(e: &Entry) -> El {
    col()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .linear_gradient(std::f32::consts::PI, [(0.0, Color::BLACK.alpha(0.15)), (0.55, Color::BLACK.alpha(0.35)), (1.0, Color::BLACK.alpha(0.85))])
        .padding(8.0)
        .child(spacer())
        .child(
            row()
                .gap(6.0)
                .center_y()
                .child(
                    el().size(32.0, 32.0)
                        .radius(theme::R_SM)
                        .fill(Color::BLACK.alpha(0.55))
                        .border(1.0, Color::WHITE.alpha(0.25))
                        .center()
                        .child(icon(&I_PENCIL, 15.0).color(Color::WHITE)),
                )
                .child(spacer())
                .child(plus_big(true)),
        )
}

fn card_dense(e: &Entry, cw: f32, hot: bool) -> El {
    let ph = (cw * 1.5).round();
    let st = if e.rewatching { PAL.st_completed } else { PAL.status(e.status) };
    let mut p = poster(e.id, cw, ph, theme::R_MD).child(badges(e)).child(
        el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(4.0).fill(Color::BLACK.alpha(0.55)).child(el().w(cw * e.fraction()).h_full().fill(st)),
    );
    if hot {
        p = p.child(hover_layer(e)).outline(2.0, 2.0, PAL.accent.alpha(0.7));
    }
    col()
        .key(format!("card/{}", e.id))
        .w(cw)
        .gap(6.0)
        .child(p)
        .child(col().h(34.0).child(text(e.title.clone()).font(TextStyle::new(13.0, 500.0)).color(PAL.text).wrap().max_lines(2)))
        .child(row().center_y().child(text(ep_text(e)).font(theme::LABEL_NUM).color(PAL.faint)).child(spacer()).child(score_el(e.score)))
}

fn card_large(e: &Entry, cw: f32, hot: bool) -> El {
    let ph = (cw * 1.42).round();
    let st = if e.rewatching { PAL.st_completed } else { PAL.status(e.status) };
    let mut p = poster(e.id, cw, ph, 0.0).child(badges(e));
    if hot {
        p = p.child(hover_layer(e));
    }
    let plus = el()
        .size(38.0, 26.0)
        .radius(theme::R_SM)
        .center()
        .child(text("+1").font(theme::LABEL_BTN).color(if hot { PAL.on_accent } else { PAL.dim }));
    let plus = if hot { plus.fill(PAL.accent) } else { plus.border(1.0, PAL.border_strong) };
    let c = col()
        .key(format!("card/{}", e.id))
        .w(cw)
        .radius(theme::R_LG)
        .clip_rounded(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, if hot { PAL.border_strong } else { PAL.border })
        .child(p)
        .child(
            col()
                .padding(12.0)
                .gap(8.0)
                .child(col().h(38.0).child(text(e.title.clone()).font(TextStyle::new(14.0, 600.0)).color(PAL.text).wrap().max_lines(2)))
                .child(bar(e.fraction(), st, 5.0, PAL.border))
                .child(
                    row()
                        .center_y()
                        .gap(8.0)
                        .child(text(ep_text(e)).font(theme::NUM_MED).color(PAL.dim))
                        .child(score_el(e.score))
                        .child(spacer())
                        .child(plus),
                ),
        );
    if hot { c.shadow(10.0, 24.0, Color::BLACK.alpha(0.45)) } else { c }
}

fn grid_page(s: &State, content_w: f32, kind: GridKind) -> El {
    let avail = content_w - 2.0 * theme::PAGE_PAD;
    let (min, gap) = match kind {
        GridKind::Dense => (120.0, 16.0),
        GridKind::Large => (180.0, 20.0),
    };
    let n = (((avail + gap) / (min + gap)).floor() as usize).max(2);
    let cw = ((avail - gap * (n as f32 - 1.0)) / n as f32).floor();
    let items = watching(s);
    let mut rows = col().gap(if kind == GridKind::Dense { 22.0 } else { 20.0 }).px(theme::PAGE_PAD).pb(theme::PAGE_PAD);
    for (ri, chunk) in items.chunks(n).enumerate() {
        let mut r = row().key(format!("grow/{ri}")).gap(gap).items_start();
        for (ci, e) in chunk.iter().enumerate() {
            let hot = ri == 0 && ci == 1;
            r = r.child(match kind {
                GridKind::Dense => card_dense(e, cw, hot),
                GridKind::Large => card_large(e, cw, hot),
            });
        }
        rows = rows.child(r);
    }
    col()
        .key("page/grid")
        .w_full()
        .h_full()
        .child(grid_top(s, "Watching", Some(s.count(Status::Watching)), 1))
        .child(ui::chips_pub(s, Status::Watching))
        .child(col().key("grid-scroll").scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(rows.pt(6.0)))
}

// ---- 1C shelves

fn shelf_head(title: &str, sub: &str, n: usize) -> El {
    row()
        .px(theme::PAGE_PAD)
        .pb(10.0)
        .gap(8.0)
        .items_end()
        .child(text(title.to_string()).font(TextStyle::new(16.0, 600.0)).color(PAL.text))
        .child(text(n.to_string()).font(theme::NUM).color(PAL.faint).pb(1.0))
        .child(text(sub.to_string()).font(theme::LABEL).color(PAL.faint).pb(1.0).pl(4.0))
        .child(spacer())
        .child(row().gap(4.0).center_y().child(text("See all").font(theme::LABEL_BTN).color(PAL.dim)).child(icon(&icons::ARROW, 13.0).color(PAL.dim)))
}

/// A shelf row: cards overflow to the right and fade out (horizontal scroll in the real thing).
fn shelf(cards: Vec<El>, h: f32) -> El {
    let mut r = row().gap(14.0).items_start().pl(theme::PAGE_PAD);
    for c in cards {
        r = r.child(c);
    }
    el().w_full()
        .h(h)
        .clip_rounded(0.0)
        .child(r)
        .child(el().pin(None, Some(0.0), Some(0.0), Some(0.0)).w(80.0).linear_gradient(std::f32::consts::PI / 2.0, [(0.0, PAL.bg.alpha(0.0)), (1.0, PAL.bg)]))
}

/// Continue watching: a wide card with the blurred cover behind, poster, next episode and +1.
fn continue_card(e: &Entry, hot: bool) -> El {
    let (w, h) = (312.0, 132.0);
    let st = if e.rewatching { PAL.st_completed } else { PAL.status(e.status) };
    let next = e.progress + 1;
    let plus = el()
        .h(28.0)
        .px(10.0)
        .radius(theme::R_SM)
        .center()
        .child(text(format!("+1  Ep {next}")).font(theme::LABEL_BTN).color(if hot { PAL.on_accent } else { Color::WHITE }));
    let plus = if hot { plus.fill(PAL.accent) } else { plus.fill(Color::WHITE.alpha(0.12)).border(1.0, Color::WHITE.alpha(0.18)) };
    let mut sub = ep_text(e);
    if let Some(n) = e.airing_note {
        sub = format!("{sub} · {n}");
    }
    el().key(format!("cont/{}", e.id))
        .size(w, h)
        .radius(theme::R_LG)
        .clip_rounded(theme::R_LG)
        .border(1.0, if hot { PAL.border_strong } else { Color::WHITE.alpha(0.06) })
        .child(backdrop(e.id, w / h, 0.45))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).linear_gradient(std::f32::consts::PI / 2.0, [(0.0, Color::BLACK.alpha(0.1)), (1.0, Color::BLACK.alpha(0.45))]))
        .child(
            row()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .padding(10.0)
                .gap(12.0)
                .child(poster(e.id, 74.0, 111.0, theme::R_SM).shadow(4.0, 12.0, Color::BLACK.alpha(0.5)))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(4.0)
                        .py(2.0)
                        .child(text(e.title.clone()).font(TextStyle::new(14.0, 600.0)).color(Color::WHITE).wrap().max_lines(2))
                        .child(text(sub).font(theme::LABEL_NUM).color(Color::WHITE.alpha(0.7)).truncate())
                        .child(spacer())
                        .child(row().gap(8.0).center_y().child(col().grow(1.0).child(bar(e.fraction(), st, 4.0, Color::WHITE.alpha(0.18)))).child(plus)),
                ),
        )
}

fn small_card(e: &Entry, cw: f32, extra: Option<El>, sub: Option<String>) -> El {
    let mut p = poster(e.id, cw, (cw * 1.5).round(), theme::R_MD);
    if let Some(x) = extra {
        p = p.child(x);
    }
    col()
        .key(format!("sc/{}", e.id))
        .w(cw)
        .gap(6.0)
        .child(p)
        .child(col().h(34.0).child(text(e.title.clone()).font(TextStyle::new(13.0, 500.0)).color(PAL.text).wrap().max_lines(2)))
        .child(match sub {
            Some(t) => text(t).font(theme::LABEL_NUM).color(PAL.faint),
            None => el(),
        })
}

fn day_chip(day: &str, today: bool) -> El {
    let r = row().absolute(6.0, 6.0).h(20.0).px(8.0).center_y().radius(10.0);
    let r = if today { r.fill(PAL.airing) } else { r.fill(Color::BLACK.alpha(0.62)) };
    r.child(text(day.to_string()).font(theme::BADGE).color(if today { PAL.on_accent } else { Color::WHITE }))
}

fn shelf_page(s: &State, _content_w: f32) -> El {
    let w = watching(s);
    let cont: Vec<El> = w.iter().take(4).enumerate().map(|(i, e)| continue_card(e, i == 0)).collect();
    let airing: Vec<&Entry> = s.entries.iter().filter(|e| e.air == Air::Airing && e.on_page(Status::Watching)).collect();
    let air_cards: Vec<El> = airing
        .iter()
        .map(|e| {
            let day = e.airing_note.and_then(|n| n.rsplit(' ').next()).unwrap_or("Today");
            small_card(e, 116.0, Some(day_chip(day, day == "Sat")), Some(format!("Ep {} next · {}", e.progress + 1, ep_text(e).trim_start_matches("Ep "))))
        })
        .collect();
    let plan: Vec<&Entry> = s.entries.iter().filter(|e| e.status == Status::PlanToWatch).collect();
    let plan_cards: Vec<El> = if plan.is_empty() {
        s.entries.iter().rev().take(9).map(|e| small_card(e, 116.0, None, None)).collect()
    } else {
        plan.iter().take(9).map(|e| small_card(e, 116.0, None, None)).collect()
    };
    let body = col()
        .gap(22.0)
        .pt(4.0)
        .pb(theme::PAGE_PAD)
        .child(col().child(shelf_head("Continue watching", "next episode first", w.len())).child(shelf(cont, 134.0)))
        .child(col().child(shelf_head("Airing this week", "by broadcast day", airing.len())).child(shelf(air_cards, 238.0)))
        .child(col().child(shelf_head("Plan to watch", "", s.count(Status::PlanToWatch))).child(shelf(plan_cards, 220.0)));
    col()
        .key("page/shelves")
        .w_full()
        .h_full()
        .child(grid_top(s, "Home", None, 2))
        .child(col().key("shelf-scroll").scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(body))
}

// ------------------------------------------------------------------------------ 2. now playing

struct NowInfo {
    id: u32,
    title: String,
    ep: u32,
    total: Option<u32>,
    kind: &'static str,
    frac: f32,
    times: String,
    player: String,
}

fn now_info(s: &State) -> NowInfo {
    let n = s.now.as_ref().expect("fake now");
    let (id, title) = ui::now_title(s, n);
    let e = s.entries.iter().find(|e| e.id == id);
    let times = match (n.position_now(), n.duration) {
        (Some(p), Some(d)) => format!("{} / {}", crate::now::clock_text(p), crate::now::clock_text(d)),
        _ => String::new(),
    };
    NowInfo {
        id,
        title,
        ep: n.episode().unwrap_or(1),
        total: e.and_then(|e| e.episodes),
        kind: e.map_or("TV", |e| e.kind),
        frac: n.fraction().unwrap_or(0.0),
        times,
        player: n.player.clone(),
    }
}

fn big_button(label: &str, primary: bool) -> El {
    let b = el().h(40.0).px(20.0).radius(theme::R_MD).center();
    let b = if primary { b.fill(PAL.accent) } else { b.fill(Color::WHITE.alpha(0.10)).border(1.0, Color::WHITE.alpha(0.20)) };
    b.child(text(label.to_string()).font(TextStyle::new(14.0, 600.0)).color(if primary { PAL.on_accent } else { Color::WHITE }))
}

fn ghost_link(label: &str) -> El {
    el().h(40.0).px(10.0).center().child(text(label.to_string()).font(TextStyle::new(13.0, 500.0)).color(Color::WHITE.alpha(0.65)))
}

fn now_bar(frac: f32, threshold: f32) -> El {
    el().w_full()
        .h(10.0)
        .child(el().pin(Some(0.0), Some(2.0), Some(0.0), None).h(6.0).child(bar(frac, PAL.accent, 6.0, Color::WHITE.alpha(0.16))))
        .child(el().absolute(0.0, 0.0).w_frac(threshold).h(10.0).child(spacer()).child(el().w(2.0).h(10.0).radius(1.0).fill(PAL.cream)))
}

fn caption(t: &str, c: Color) -> El {
    text(t.to_string()).font(theme::CAPTION).color(c)
}

fn live_dot() -> El {
    el().size(8.0, 8.0).radius(4.0).fill(PAL.ok()).outline(3.0, 0.0, PAL.ok().alpha(0.25))
}

fn recog_card(s: &State) -> El {
    let n = s.now.as_ref().unwrap();
    let kv = |k: &str, v: &str, c: Color| ui::kv(k, text(v.to_string()).font(theme::BODY).color(c));
    ui::card("np-recog", "Recognised from")
        .gap(2.0)
        .child(row().gap(6.0).center_y().pb(6.0).child(icon(&icons::FILE, 14.0).color(PAL.faint)).child(text(n.file.clone()).font(theme::LABEL).color(PAL.dim).truncate()))
        .child(kv("Parsed title", "Lanterns of Kirigaoka", PAL.text))
        .child(kv("Group", "Hoshi · 1080p", PAL.text))
        .child(kv("Match", "In your list (Watching), sure", PAL.ok()))
}

/// 2A: full-width hero band at the top, blurred cover behind, cards below.
fn now_a(s: &State) -> El {
    let i = now_info(s);
    let band_h = 380.0;
    let sub = match i.total {
        Some(t) => format!("Episode {} of {t} · {}", i.ep, i.kind),
        None => format!("Episode {} · {}", i.ep, i.kind),
    };
    let hero = el()
        .key("np-hero")
        .w_full()
        .h(band_h)
        .child(backdrop(i.id, 1055.0 / band_h, 0.35))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).linear_gradient(std::f32::consts::PI, [(0.0, PAL.bg.alpha(0.15)), (0.6, PAL.bg.alpha(0.35)), (1.0, PAL.bg)]))
        .child(
            row()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .px(theme::PAGE_PAD + 8.0)
                .pt(40.0)
                .pb(28.0)
                .gap(32.0)
                .items_end()
                .child(poster(i.id, 196.0, 294.0, theme::R_LG).border(1.0, Color::WHITE.alpha(0.12)).shadow(16.0, 40.0, Color::BLACK.alpha(0.6)))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(8.0)
                        .max_w(620.0)
                        .child(row().gap(8.0).center_y().child(live_dot()).child(caption(&format!("PLAYING IN {} · 68%", i.player.to_uppercase()), Color::WHITE.alpha(0.75))))
                        .child(text(i.title.clone()).font(TextStyle::new(34.0, 600.0).tracking(-0.4)).color(Color::WHITE).wrap().max_lines(2))
                        .child(text(sub).font(TextStyle::new(17.0, 500.0)).color(Color::WHITE.alpha(0.78)))
                        .child(col().pt(14.0).gap(6.0).child(now_bar(i.frac, 0.8)).child(
                            row()
                                .child(text(i.times.clone()).font(theme::NUM_MED).color(Color::WHITE.alpha(0.85)))
                                .child(spacer())
                                .child(text("Offer at 80%").font(theme::LABEL).color(Color::WHITE.alpha(0.55))),
                        ))
                        .child(
                            row()
                                .pt(14.0)
                                .gap(10.0)
                                .center_y()
                                .child(big_button(&format!("Update to episode {}", i.ep), true))
                                .child(big_button("Not now", false))
                                .child(ghost_link("Wrong anime…")),
                        ),
                ),
        );
    let lower = row()
        .gap(16.0)
        .px(theme::PAGE_PAD)
        .pt(8.0)
        .items_start()
        .child(recog_card(s).grow(1.0).min_w(0.0))
        .child(ui::players_status_card_pub(s).w(380.0));
    col().key("page/now").w_full().h_full().child(col().key("now-scroll").scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(hero).child(lower))
}

fn player_chip(name: &str, line: &str, c: Color) -> El {
    row()
        .h(30.0)
        .px(12.0)
        .gap(8.0)
        .center_y()
        .radius(15.0)
        .fill(Color::BLACK.alpha(0.35))
        .border(1.0, Color::WHITE.alpha(0.10))
        .child(el().size(7.0, 7.0).radius(3.5).fill(c))
        .child(text(name.to_string()).font(theme::BODY_MED).color(Color::WHITE))
        .child(text(line.to_string()).font(theme::LABEL).color(Color::WHITE.alpha(0.6)))
}

/// 2B: the whole page is the hero: blurred cover full-bleed, everything centred, players as chips.
fn now_b(s: &State) -> El {
    let i = now_info(s);
    let sub = match i.total {
        Some(t) => format!("Episode {} of {t}", i.ep),
        None => format!("Episode {}", i.ep),
    };
    let content = col()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .items_center()
        .px(theme::PAGE_PAD)
        .pt(44.0)
        .pb(20.0)
        .child(poster(i.id, 210.0, 315.0, theme::R_LG).border(1.0, Color::WHITE.alpha(0.14)).shadow(20.0, 50.0, Color::BLACK.alpha(0.65)))
        .child(row().pt(22.0).gap(8.0).center_y().child(live_dot()).child(caption(&format!("PLAYING IN {}", i.player.to_uppercase()), Color::WHITE.alpha(0.7))))
        .child(text(i.title.clone()).font(TextStyle::new(32.0, 600.0).tracking(-0.4)).color(Color::WHITE).pt(6.0))
        .child(
            row()
                .gap(10.0)
                .center_y()
                .pt(4.0)
                .child(text(sub).font(TextStyle::new(16.0, 500.0)).color(Color::WHITE.alpha(0.8)))
                .child(text("·").font(theme::BODY).color(Color::WHITE.alpha(0.4)))
                .child(text(format!("your progress 5 / {}", i.total.unwrap_or(0))).font(theme::BODY).color(Color::WHITE.alpha(0.6))),
        )
        .child(
            col().w(540.0).pt(18.0).gap(6.0).child(now_bar(i.frac, 0.8)).child(
                row()
                    .child(text(i.times.clone()).font(theme::NUM_MED).color(Color::WHITE.alpha(0.85)))
                    .child(spacer())
                    .child(text("Asks at 80%").font(theme::LABEL).color(Color::WHITE.alpha(0.55))),
            ),
        )
        .child(row().pt(18.0).gap(10.0).center_y().child(big_button(&format!("Update to episode {}", i.ep), true)).child(big_button("Not now", false)))
        .child(ghost_link("Wrong anime…").h(30.0).mt(4.0))
        .child(spacer())
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(caption("PLAYERS", Color::WHITE.alpha(0.5)).pr(4.0))
                .child(player_chip("mpv", "IPC socket · playing", PAL.ok()))
                .child(player_chip("VLC", "not running", PAL.faint))
                .child(player_chip("mpc-qt", "nothing playing", PAL.warn())),
        );
    el().key("page/now")
        .w_full()
        .h_full()
        .clip_rounded(0.0)
        .child(backdrop(i.id, 1055.0 / 800.0, 0.4))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).linear_gradient(std::f32::consts::PI, [(0.0, PAL.bg.alpha(0.25)), (0.7, PAL.bg.alpha(0.55)), (1.0, PAL.bg.alpha(0.92))]))
        .child(content)
}

// ------------------------------------------------------------------------------ 3. details

fn chip_stat(big: &str, small: &str, on_dark: bool) -> El {
    let r = row().h(30.0).px(10.0).gap(6.0).center_y().radius(15.0);
    let r = if on_dark { r.fill(Color::BLACK.alpha(0.38)).border(1.0, Color::WHITE.alpha(0.12)) } else { r.fill(PAL.surface).border(1.0, PAL.border) };
    r.child(text(big.to_string()).font(TextStyle::new(14.0, 600.0).features("'tnum' 1")).color(Color::WHITE))
        .child(text(small.to_string()).font(theme::LABEL).color(Color::WHITE.alpha(0.6)))
}

fn score_chip(on_dark: bool) -> El {
    let r = row().h(30.0).px(10.0).gap(6.0).center_y().radius(15.0).fill(PAL.st_onhold.alpha(0.16)).border(1.0, PAL.st_onhold.alpha(0.35));
    let _ = on_dark;
    r.child(icon(&I_STAR, 14.0).color(PAL.st_onhold))
        .child(text("8.44").font(TextStyle::new(14.0, 600.0).features("'tnum' 1")).color(Color::WHITE))
        .child(text("MAL").font(theme::LABEL).color(Color::WHITE.alpha(0.6)))
}

fn meta_tags() -> El {
    row()
        .gap(6.0)
        .child(ui::tag_tinted("TV", PAL.text, Color::WHITE.alpha(0.14)))
        .child(ui::tag_tinted("12 episodes", PAL.text, Color::WHITE.alpha(0.10)))
        .child(ui::tag("Airing", PAL.airing, true))
        .child(ui::tag_tinted("Fall 2026", PAL.text, Color::WHITE.alpha(0.10)))
        .child(ui::tag_tinted("Studio Example", PAL.text, Color::WHITE.alpha(0.10)))
}

fn back_float() -> El {
    row()
        .h(30.0)
        .pl(6.0)
        .pr(12.0)
        .gap(2.0)
        .center_y()
        .radius(15.0)
        .fill(Color::BLACK.alpha(0.40))
        .border(1.0, Color::WHITE.alpha(0.14))
        .child(icon(&I_BACK, 16.0).color(Color::WHITE))
        .child(text("Back").font(theme::LABEL_BTN).color(Color::WHITE))
}

fn mal_float() -> El {
    row()
        .h(30.0)
        .px(12.0)
        .gap(6.0)
        .center_y()
        .radius(15.0)
        .fill(Color::BLACK.alpha(0.40))
        .border(1.0, Color::WHITE.alpha(0.14))
        .child(text("MyAnimeList").font(theme::LABEL_BTN).color(Color::WHITE))
        .child(icon(&icons::EXTERNAL, 13.0).color(Color::WHITE.alpha(0.8)))
}

fn your_entry(glass: bool) -> El {
    let c = col().gap(10.0).padding(16.0).radius(theme::R_LG);
    let c = if glass { c.fill(Color::BLACK.alpha(0.38)).border(1.0, Color::WHITE.alpha(0.12)) } else { c.fill(PAL.surface).border(1.0, PAL.border) };
    let plus = el().size(38.0, 28.0).radius(theme::R_SM).border(1.0, PAL.border_strong).center().child(text("+1").font(theme::LABEL_BTN).color(PAL.text));
    c.child(row().center_y().child(caption("YOUR LIST", PAL.faint)).child(spacer()).child(ui::tag("Waiting to sync", PAL.warn(), false)))
        .child(row().gap(8.0).center_y().child(el().size(8.0, 8.0).radius(4.0).fill(PAL.st_watching)).child(text("Watching").font(TextStyle::new(15.0, 600.0)).color(PAL.text)))
        .child(
            row()
                .gap(10.0)
                .center_y()
                .child(col().grow(1.0).child(bar(5.0 / 12.0, PAL.st_watching, 6.0, Color::WHITE.alpha(0.12))))
                .child(text("5 / 12").font(theme::NUM_MED).color(PAL.text))
                .child(plus),
        )
        .child(
            row()
                .gap(18.0)
                .child(col().gap(2.0).child(caption("SCORE", PAL.faint)).child(row().gap(4.0).center_y().child(icon(&I_STAR, 13.0).color(PAL.st_onhold)).child(text("8").font(theme::NUM_MED).color(PAL.text))))
                .child(col().gap(2.0).child(caption("STARTED", PAL.faint)).child(text("Oct 4, 2026").font(theme::BODY).color(PAL.text)))
                .child(col().gap(2.0).child(caption("FINISHED", PAL.faint)).child(text("–").font(theme::BODY).color(PAL.dim))),
        )
        .child(row().gap(8.0).child(soft_button("Edit entry")).child(soft_button("Rewatch")))
}

fn soft_button(label: &str) -> El {
    el().h(28.0).px(12.0).radius(theme::R_SM).center().fill(Color::WHITE.alpha(0.08)).border(1.0, Color::WHITE.alpha(0.14)).child(text(label.to_string()).font(theme::LABEL_BTN).color(PAL.text))
}

fn info_cards() -> El {
    let syn = "Every autumn the hill town of Kirigaoka hangs a thousand paper lanterns for the festival of returning souls. This year one lantern will not go out, and the girl who lit it has no memory of doing so. A made-up story written for Kotora's screenshots; no real anime is described here.";
    let kv = |k: &str, v: &str| ui::kv(k, text(v.to_string()).font(theme::BODY).color(PAL.text));
    col()
        .gap(16.0)
        .child(ui::card("d-syn", "Synopsis").child(text(syn).font(theme::BODY).color(PAL.dim).wrap()))
        .child(
            ui::card("d-info", "Information")
                .child(kv("Aired", "Since Oct 2, 2026"))
                .child(kv("Studios", "Studio Example"))
                .child(kv("Genres", "Drama · Mystery · Slice of Life"))
                .child(kv("Source", "Light novel · 24 min per episode")),
        )
}

/// 3A: full-bleed banner, cover overlapping its lower edge, title on the banner; entry card right.
fn details_a(_s: &State) -> El {
    let id = 0;
    let banner_h = 230.0;
    let banner = el()
        .key("d-banner")
        .w_full()
        .h(banner_h)
        .child(backdrop(id, 1055.0 / banner_h, 0.25))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).linear_gradient(std::f32::consts::PI, [(0.0, PAL.bg.alpha(0.1)), (0.55, PAL.bg.alpha(0.3)), (1.0, PAL.bg)]))
        .child(row().pin(Some(theme::PAGE_PAD), Some(18.0), Some(theme::PAGE_PAD), None).child(back_float()).child(spacer()).child(mal_float()));
    let head = row()
        .px(theme::PAGE_PAD + 4.0)
        .gap(26.0)
        .items_start()
        .mt(-150.0)
        .child(poster(id, 176.0, 264.0, theme::R_LG).border(1.0, Color::WHITE.alpha(0.12)).shadow(14.0, 36.0, Color::BLACK.alpha(0.6)))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .pt(44.0)
                .gap(4.0)
                .child(text("Lanterns of Kirigaoka").font(TextStyle::new(32.0, 600.0).tracking(-0.4)).color(Color::WHITE))
                .child(row().gap(10.0).center_y().child(text("The Lanterns on Kirigaoka Hill").font(TextStyle::new(15.0, 500.0)).color(Color::WHITE.alpha(0.8))).child(text("霧ヶ丘のランタン").font(TextStyle::new(15.0, 400.0)).color(Color::WHITE.alpha(0.55))))
                .child(row().pt(30.0).gap(8.0).child(score_chip(true)).child(chip_stat("#212", "ranked", false)).child(chip_stat("#1,840", "popular", false)).child(chip_stat("312k", "members", false)))
                .child(meta_tags().pt(12.0)),
        )
        .child(your_entry(false).w(300.0).mt(64.0));
    col().key("page/details").w_full().h_full().child(
        col()
            .key("d-scroll")
            .scroll(ScrollSpec { stick_bottom: false })
            .grow(1.0)
            .min_h(0.0)
            .child(banner)
            .child(head)
            .child(info_cards().px(theme::PAGE_PAD).pt(24.0).pb(theme::PAGE_PAD).max_w(720.0)),
    )
}

/// 3B: inset banner card holding everything: cover, titles, chips and a glass "your list" panel.
fn details_b(_s: &State) -> El {
    let id = 0;
    let h = 330.0;
    let banner = el()
        .key("d-banner")
        .w_full()
        .h(h)
        .radius(16.0)
        .clip_rounded(16.0)
        .border(1.0, PAL.border)
        .child(backdrop(id, 1007.0 / h, 0.3))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).linear_gradient(std::f32::consts::PI / 2.0, [(0.0, Color::BLACK.alpha(0.45)), (0.6, Color::BLACK.alpha(0.15)), (1.0, Color::BLACK.alpha(0.35))]))
        .child(
            row()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .padding(24.0)
                .gap(24.0)
                .child(poster(id, 188.0, 282.0, theme::R_MD).border(1.0, Color::WHITE.alpha(0.14)).shadow(12.0, 30.0, Color::BLACK.alpha(0.55)))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(4.0)
                        .child(meta_tags().pb(8.0))
                        .child(text("Lanterns of Kirigaoka").font(TextStyle::new(30.0, 600.0).tracking(-0.4)).color(Color::WHITE).wrap())
                        .child(text("The Lanterns on Kirigaoka Hill").font(TextStyle::new(15.0, 500.0)).color(Color::WHITE.alpha(0.82)))
                        .child(text("霧ヶ丘のランタン").font(TextStyle::new(14.0, 400.0)).color(Color::WHITE.alpha(0.55)))
                        .child(spacer())
                        .child(row().gap(8.0).child(score_chip(true)).child(chip_stat("#212", "ranked", true)).child(chip_stat("#1,840", "popular", true)))
                        .child(row().pt(8.0).gap(8.0).child(chip_stat("312,455", "members", true))),
                )
                .child(your_entry(true).w(300.0)),
        );
    let top = row()
        .px(theme::PAGE_PAD)
        .pt(18.0)
        .pb(14.0)
        .gap(12.0)
        .center_y()
        .child(row().h(30.0).pl(6.0).pr(12.0).gap(2.0).center_y().radius(theme::R_SM).border(1.0, PAL.border_strong).child(icon(&I_BACK, 16.0).color(PAL.text)).child(text("Back").font(theme::LABEL_BTN).color(PAL.text)))
        .child(text("Anime information").font(theme::SECTION).color(PAL.dim))
        .child(spacer())
        .child(row().h(30.0).px(12.0).gap(6.0).center_y().radius(theme::R_SM).border(1.0, PAL.border_strong).child(text("Open on MyAnimeList").font(theme::LABEL_BTN).color(PAL.text)).child(icon(&icons::EXTERNAL, 13.0).color(PAL.dim)));
    col().key("page/details").w_full().h_full().child(top).child(
        col()
            .key("d-scroll")
            .scroll(ScrollSpec { stick_bottom: false })
            .grow(1.0)
            .min_h(0.0)
            .child(col().px(theme::PAGE_PAD).gap(16.0).pb(theme::PAGE_PAD).child(banner).child(info_cards().max_w(720.0))),
    )
}

// ------------------------------------------------------------------------------ 4. sidebar mini

pub fn mini(s: &State, v: char) -> El {
    let i = now_info(s);
    let pct = format!("{:.0}%", i.frac * 100.0);
    match v {
        // A: full-width cover strip, text over a gradient, progress along the bottom edge.
        'a' => {
            let (w, h) = (theme::SIDEBAR_W - 20.0, 112.0);
            el().key("np-mini")
                .w_full()
                .h(h)
                .radius(theme::R_LG)
                .clip_rounded(theme::R_LG)
                .border(1.0, Color::WHITE.alpha(0.08))
                .fill(PAL.surface)
                // The poster itself, cropped wide (Fill takes its middle band); sharp, not blurred.
                .pixels_fit(poster_img(i.id).pixels(), SurfaceFit::Fill)
                .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).linear_gradient(std::f32::consts::PI, [(0.0, Color::BLACK.alpha(0.35)), (0.35, Color::BLACK.alpha(0.05)), (0.55, Color::BLACK.alpha(0.35)), (1.0, Color::BLACK.alpha(0.88))]))
                .child(
                    row()
                        .pin(Some(10.0), Some(9.0), Some(10.0), None)
                        .gap(6.0)
                        .center_y()
                        .child(el().size(6.0, 6.0).radius(3.0).fill(PAL.ok()))
                        .child(caption("NOW PLAYING", Color::WHITE.alpha(0.9)))
                        .child(spacer())
                        .child(text(format!("{} · {}", i.player, pct)).font(theme::SMALL).color(Color::WHITE.alpha(0.75))),
                )
                .child(
                    col()
                        .pin(Some(10.0), None, Some(10.0), Some(11.0))
                        .gap(1.0)
                        .child(text(i.title.clone()).font(TextStyle::new(14.0, 600.0)).color(Color::WHITE).truncate())
                        .child(text(format!("Episode {} of {}", i.ep, i.total.unwrap_or(0))).font(theme::SMALL).color(Color::WHITE.alpha(0.75))),
                )
                .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(3.0).fill(Color::WHITE.alpha(0.15)).child(el().w(w * i.frac).h_full().fill(PAL.accent)))
        }
        // B: a taller card, a bigger poster and the text beside it, faint blurred tint behind.
        _ => {
            let (w, h) = (theme::SIDEBAR_W - 20.0, 124.0);
            el().key("np-mini")
                .w_full()
                .h(h)
                .radius(theme::R_LG)
                .clip_rounded(theme::R_LG)
                .border(1.0, PAL.border)
                .child(backdrop(i.id, w / h, 0.55))
                .child(
                    row()
                        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                        .padding(10.0)
                        .gap(10.0)
                        .child(poster(i.id, 68.0, 102.0, theme::R_SM).border(1.0, Color::WHITE.alpha(0.12)).shadow(4.0, 10.0, Color::BLACK.alpha(0.5)))
                        .child(
                            col()
                                .grow(1.0)
                                .min_w(0.0)
                                .gap(3.0)
                                .child(row().gap(5.0).center_y().child(el().size(6.0, 6.0).radius(3.0).fill(PAL.ok())).child(caption(&i.player.to_uppercase(), Color::WHITE.alpha(0.7))))
                                .child(text(i.title.clone()).font(TextStyle::new(13.0, 600.0)).color(Color::WHITE).wrap().max_lines(2))
                                .child(text(format!("Episode {}", i.ep)).font(theme::SMALL).color(Color::WHITE.alpha(0.72)))
                                .child(spacer())
                                .child(row().gap(6.0).center_y().child(col().grow(1.0).child(bar(i.frac, PAL.accent, 4.0, Color::WHITE.alpha(0.18)))).child(text(pct).font(theme::LABEL_NUM).color(Color::WHITE.alpha(0.75)))),
                        ),
                )
        }
    }
}
