//! Covers in the list (design-spec §3, Uzumaki's `art.rs` rules, made small).
//!
//! `get` runs in the view and only touches memory. A miss queues a job; worker threads (at most
//! `WORKERS`, started only when there is a job, and gone once the queue is empty) read the disk
//! cache or download the file (`kotora_core::covers`), decode it shrunk to the exact pixels it is
//! drawn at, and hand it to the UI thread (`Covers::loaded`), which gives it to Rune's
//! `ImageCache`. Decoded covers are kept within `MEMORY_BUDGET`, least recently drawn first out.
//! The view draws the generated tile under every cover, so a cover still loading, failed or
//! missing shows the tile. Nothing runs at rest: no timers, no idle threads.
//!
//! Headless runs never download: the app has no handle there. The `list-covers` scene asks for
//! made-up covers instead (`Covers::fake`).
//!
//! Blurred backdrops (the sidebar card, Now Playing, the details banner, Home) come from the same
//! file: [`Covers::blurred`] queues one job that squashes the whole cover into [`BLUR_W`] x
//! [`BLUR_H`] pixels, blurs and saturates it once on the worker, and keeps that tiny image like any
//! cover. The GPU only stretches it; nothing blurs per frame. It is the same for both themes: the
//! view lays the palette's scrim over it.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use kotora_core::covers;
use rune::{Image, ImageCache};

use crate::model::{Handle, State};

/// Download threads at most. Covers are small; this only keeps a screenful from queueing.
const WORKERS: usize = 4;
/// Bytes of decoded covers kept (Rune's mips add a third). A row cover at 2x is 68 x 96 px,
/// about 35 KB: some 450 covers, more than a long list's screenfuls. An Airing card's cover at 2x
/// is 256 x 362 px, about 0.5 MB, and the grid builds some 50 cards (the rows on screen and two
/// more each way): 32 MB keeps them all, so scrolling the grid never drops a cover it still draws.
const MEMORY_BUDGET: usize = 32 << 20;

/// A cover in memory: the media id, its size in pixels, and whether it is the blurred backdrop.
type Key = (u32, u32, u32, bool);

/// The blurred backdrop's pixels: the whole cover squashed to 2:1. Wider areas crop its middle.
pub const BLUR_W: u32 = 96;
pub const BLUR_H: u32 = 48;

struct Job {
    key: Key,
    url: String,
}

#[derive(Default)]
struct Queue {
    jobs: Vec<Job>,
    workers: usize,
}

#[derive(Default)]
struct Inner {
    /// Image, last use, cost in bytes.
    images: HashMap<Key, (Image, u64, usize)>,
    tick: u64,
    total: usize,
    in_flight: HashSet<Key>,
    /// Media ids whose cover failed this session (not tried again until a restart).
    failed: HashSet<u32>,
}

pub struct Covers {
    cache: Option<ImageCache>,
    handle: Option<Handle>,
    /// Network allowed (`KOTORA_OFFLINE` reads the disk only).
    online: bool,
    /// The `list-covers` scene: made-up covers, drawn at once.
    fake: bool,
    /// The display scale the view last drew at.
    scale: Cell<f32>,
    inner: RefCell<Inner>,
    queue: Arc<Mutex<Queue>>,
    /// The disk folder was trimmed this session (by the first worker).
    pruned: Cell<bool>,
}

impl Default for Covers {
    fn default() -> Self {
        Covers {
            cache: None,
            handle: None,
            online: false,
            fake: false,
            scale: Cell::new(1.0),
            inner: RefCell::default(),
            queue: Arc::default(),
            pruned: Cell::new(false),
        }
    }
}

impl Covers {
    /// `setup`: Rune's image cache.
    pub fn attach(&mut self, cache: ImageCache) {
        self.cache = Some(cache);
    }

    /// The real app: downloads may start (never in headless runs, which have no handle).
    pub fn go_live(&mut self, handle: Handle, online: bool) {
        self.handle = Some(handle);
        self.online = online;
    }

    /// Scenes: made-up covers instead of downloads.
    pub fn fake(&mut self) {
        self.fake = true;
    }

    /// The view records the window's scale before it draws covers.
    pub fn set_scale(&self, scale: f32) {
        if scale.is_finite() && scale > 0.0 {
            self.scale.set(scale.clamp(1.0, 3.0));
        }
    }

    /// The cover of media `id` at `w` x `h` points, if it is in memory (it may still be in
    /// Rune's decoder; the tile drawn under it shows until then). Otherwise queues it once and
    /// returns None. Cheap: callable from the view.
    pub fn get(&self, id: u32, url: Option<&str>, w: f32, h: f32) -> Option<Image> {
        let scale = self.scale.get();
        self.get_key((id, (w * scale).ceil() as u32, (h * scale).ceil() as u32, false), url)
    }

    /// The blurred backdrop of media `id` ([`BLUR_W`] x [`BLUR_H`] pixels whatever the scale),
    /// as [`Covers::get`]: from memory, else queued once.
    pub fn blurred(&self, id: u32, url: Option<&str>) -> Option<Image> {
        self.get_key((id, BLUR_W, BLUR_H, true), url)
    }

    fn get_key(&self, key: Key, url: Option<&str>) -> Option<Image> {
        let url = url.filter(|u| !u.is_empty())?;
        let id = key.0;
        let mut inner = self.inner.borrow_mut();
        inner.tick += 1;
        let tick = inner.tick;
        if let Some(e) = inner.images.get_mut(&key) {
            e.1 = tick;
            return Some(e.0.clone());
        }
        let cache = self.cache.as_ref()?;
        if self.fake {
            let px = if key.3 { blur(&fake_pixels(id, 64, 96), 64, 96) } else { fake_pixels(id, key.1, key.2) };
            let img = cache.rgba(key.1, key.2, px);
            // Headless snaps are taken at once: wait for the tiny decode, as for the logo.
            let _ = img.wait();
            put(&mut inner, key, img.clone());
            return Some(img);
        }
        let handle = self.handle.as_ref()?;
        if inner.failed.contains(&id) || !inner.in_flight.insert(key) {
            return None;
        }
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        q.jobs.push(Job { key, url: url.to_string() });
        if q.workers < WORKERS {
            q.workers += 1;
            spawn_worker(self.queue.clone(), handle.clone(), self.online, !self.pruned.replace(true));
        }
        None
    }

    /// UI thread: a decoded cover arrived. Returns whether the screen changes.
    fn loaded(&mut self, key: Key, w: u32, h: u32, rgba: Vec<u8>) -> bool {
        let inner = self.inner.get_mut();
        inner.in_flight.remove(&key);
        let Some(cache) = &self.cache else { return false };
        let img = cache.rgba(w, h, rgba);
        put(inner, key, img);
        true
    }

    /// UI thread: no cover for this id this session.
    fn failed(&mut self, key: Key) {
        let inner = self.inner.get_mut();
        inner.in_flight.remove(&key);
        inner.failed.insert(key.0);
    }

    /// An image whose URL changed (the profile picture): drawn again from the new URL.
    pub fn forget(&mut self, id: u32) {
        let inner = self.inner.get_mut();
        let gone: Vec<Key> = inner.images.keys().filter(|k| k.0 == id).copied().collect();
        for k in gone {
            if let Some(e) = inner.images.remove(&k) {
                inner.total -= e.2;
            }
        }
        inner.failed.remove(&id);
    }

    /// Sign out: covers leave memory (the disk folder is deleted by `online::sign_out`).
    pub fn clear(&mut self) {
        let inner = self.inner.get_mut();
        inner.images.clear();
        inner.total = 0;
        inner.failed.clear();
    }

    /// Covers in memory and their bytes (the budget test).
    #[cfg(test)]
    pub fn usage(&self) -> (usize, usize) {
        let inner = self.inner.borrow();
        (inner.images.len(), inner.total)
    }
}

/// Adds a cover, then lets the least recently drawn go until the total fits.
fn put(inner: &mut Inner, key: Key, img: Image) {
    let cost = (key.1 * key.2 * 4) as usize * 4 / 3;
    inner.tick += 1;
    if let Some(old) = inner.images.insert(key, (img, inner.tick, cost)) {
        inner.total -= old.2;
    }
    inner.total += cost;
    while inner.total > MEMORY_BUDGET && inner.images.len() > 1 {
        let Some(oldest) = inner.images.iter().filter(|(k, _)| **k != key).min_by_key(|(_, e)| e.1).map(|(k, _)| *k) else { break };
        if let Some(e) = inner.images.remove(&oldest) {
            inner.total -= e.2;
        }
    }
}

/// One worker: takes the newest job (what is on screen now) until none is left, then ends.
/// `prune`: the first worker of the session trims the disk folder first.
fn spawn_worker(queue: Arc<Mutex<Queue>>, handle: Handle, online: bool, prune: bool) {
    let spawned = std::thread::Builder::new().name("covers".into()).spawn({
        let queue = queue.clone();
        move || {
            let dir = covers::dir();
            if prune {
                covers::prune(&dir, covers::DISK_BUDGET);
            }
            let agent = covers::agent();
            loop {
                let job = {
                    let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
                    match q.jobs.pop() {
                        Some(j) => j,
                        None => {
                            q.workers -= 1;
                            return;
                        }
                    }
                };
                let (key, w, h) = (job.key, job.key.1, job.key.2);
                let blurred = key.3;
                let got = covers::fetch(&agent, &dir, &job.url, online).and_then(|b| if blurred { decode_blurred(&b) } else { decode(&b, w, h) });
                match got {
                    Ok(rgba) => handle.update_if(move |s: &mut State, _| s.covers.loaded(key, w, h, rgba)),
                    Err(_) => handle.update_if(move |s: &mut State, _| {
                        s.covers.failed(key);
                        false
                    }),
                }
            }
        }
    });
    if spawned.is_err() {
        queue.lock().unwrap_or_else(|e| e.into_inner()).workers -= 1;
    }
}

/// Decodes a cover and shrinks it to fill `w` x `h` pixels exactly (cropping the overhang), so
/// memory holds only what is drawn. Straight-alpha sRGB RGBA, as `ImageCache::rgba` takes it.
fn decode(bytes: &[u8], w: u32, h: u32) -> Result<Vec<u8>, String> {
    let img = if is_webp(bytes) { decode_webp(bytes)? } else { image::load_from_memory(bytes).map_err(|e| e.to_string())? };
    Ok(img.resize_to_fill(w, h, image::imageops::FilterType::Triangle).to_rgba8().into_raw())
}

/// Decodes a cover into its blurred backdrop: the whole cover squashed to [`BLUR_W`] x
/// [`BLUR_H`], then [`blur`]. Runs once per cover on a worker thread.
fn decode_blurred(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = if is_webp(bytes) { decode_webp(bytes)? } else { image::load_from_memory(bytes).map_err(|e| e.to_string())? };
    // A small cover first (box filter: cheap, and the blur hides it).
    let small = img.resize_exact(64, 96, image::imageops::FilterType::Triangle).to_rgba8().into_raw();
    Ok(blur(&small, 64, 96))
}

/// A 64 x 96 RGBA cover squashed to [`BLUR_W`] x [`BLUR_H`], blurred (three box passes, close
/// to a Gaussian), its colours lifted a little (a blur greys them) and its lightness kept in a
/// middle band, so the palette's scrim over it gives text the same contrast on any cover.
fn blur(src: &[u8], sw: u32, sh: u32) -> Vec<u8> {
    let (w, h) = (BLUR_W as usize, BLUR_H as usize);
    let (sw, sh) = (sw as usize, sh as usize);
    // Area average into the target grid (the squash).
    let mut buf = vec![[0.0f32; 3]; w * h];
    for (oy, line) in buf.chunks_mut(w).enumerate() {
        let (y0, y1) = (oy * sh / h, ((oy + 1) * sh / h).max(oy * sh / h + 1));
        for (ox, px) in line.iter_mut().enumerate() {
            let (x0, x1) = (ox * sw / w, ((ox + 1) * sw / w).max(ox * sw / w + 1));
            let mut acc = [0.0f32; 3];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * sw + x) * 4;
                    for c in 0..3 {
                        acc[c] += f32::from(src[i + c]);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as f32;
            *px = [acc[0] / n, acc[1] / n, acc[2] / n];
        }
    }
    for _ in 0..3 {
        buf = box_pass(&buf, w, h, 5, true);
        buf = box_pass(&buf, w, h, 5, false);
    }
    let mut out = Vec::with_capacity(w * h * 4);
    for p in buf {
        let l = 0.3 * p[0] + 0.55 * p[1] + 0.15 * p[2];
        // Lightness into 60..190 of 255; chroma lifted by half.
        let target = 60.0 + l * (130.0 / 255.0);
        for c in p {
            out.push((target + (c - l) * 1.5).clamp(0.0, 255.0) as u8);
        }
        out.push(255);
    }
    out
}

/// One box-blur pass of radius `r` along x (`horizontal`) or y, edges clamped.
fn box_pass(src: &[[f32; 3]], w: usize, h: usize, r: isize, horizontal: bool) -> Vec<[f32; 3]> {
    let n = (2 * r + 1) as f32;
    let mut out = vec![[0.0f32; 3]; src.len()];
    for y in 0..h {
        for x in 0..w {
            let mut a = [0.0f32; 3];
            for k in -r..=r {
                let (sx, sy) = if horizontal { ((x as isize + k).clamp(0, w as isize - 1) as usize, y) } else { (x, (y as isize + k).clamp(0, h as isize - 1) as usize) };
                let p = src[sy * w + sx];
                a = [a[0] + p[0], a[1] + p[1], a[2] + p[2]];
            }
            out[y * w + x] = [a[0] / n, a[1] / n, a[2] / n];
        }
    }
    out
}

/// A RIFF container holding WebP (MAL serves some covers as `.webp`).
fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
}

/// WebP through `image-webp`'s decoder alone: `image`'s `webp` feature would also link the
/// encoder (about 0.5 MB more in the release binary for the same decoder).
fn decode_webp(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    let mut d = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let (w, h) = d.dimensions();
    let alpha = d.has_alpha();
    let mut buf = vec![0; d.output_buffer_size().ok_or("WebP too large")?];
    d.read_image(&mut buf).map_err(|e| e.to_string())?;
    let img = if alpha {
        image::RgbaImage::from_raw(w, h, buf).map(image::DynamicImage::ImageRgba8)
    } else {
        image::RgbImage::from_raw(w, h, buf).map(image::DynamicImage::ImageRgb8)
    };
    img.ok_or_else(|| "bad WebP buffer".into())
}

/// A made-up cover for the scenes: a two-colour diagonal poster with a pale band, unlike the
/// generated tiles, so a snap shows which rows have covers.
fn fake_pixels(id: u32, w: u32, h: u32) -> Vec<u8> {
    const PAIRS: [([u8; 3], [u8; 3]); 4] =
        [([40, 70, 140], [230, 140, 60]), ([30, 110, 90], [240, 220, 160]), ([120, 40, 80], [250, 170, 120]), ([60, 50, 120], [130, 200, 230])];
    let (a, b) = PAIRS[id as usize % PAIRS.len()];
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let t = ((x as f32 / w as f32) * 0.4 + (y as f32 / h as f32) * 0.6).clamp(0.0, 1.0);
            let band = (y as f32 / h as f32 - 0.62).abs() < 0.06;
            for c in 0..3 {
                let v = a[c] as f32 * (1.0 - t) + b[c] as f32 * t;
                out.push(if band { (v * 0.4 + 255.0 * 0.6) as u8 } else { v as u8 });
            }
            out.push(255);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_fills_the_display_size_exactly() {
        // A 225 x 320 JPEG, MAL's medium cover size.
        let src = image::RgbImage::from_pixel(225, 320, image::Rgb([200, 100, 50]));
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgb8(src).write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg).unwrap();
        let rgba = decode(&jpeg, 68, 96).unwrap();
        assert_eq!(rgba.len(), 68 * 96 * 4);
        assert!(decode(b"not an image", 68, 96).is_err());
    }

    /// MAL serves some covers as WebP: lossy (VP8) and lossless with alpha (VP8L), both 24 x 36.
    #[test]
    fn decode_webp_covers() {
        for (bytes, name) in [(&include_bytes!("../tests/cover.webp")[..], "lossy"), (&include_bytes!("../tests/cover-alpha.webp")[..], "alpha")] {
            assert!(is_webp(bytes), "{name}");
            let rgba = decode(bytes, 68, 96).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(rgba.len(), 68 * 96 * 4, "{name}");
            // The top is the orange end of the gradient.
            assert!(rgba[0] > rgba[2], "{name}: {:?}", &rgba[..4]);
        }
        let alpha = decode(include_bytes!("../tests/cover-alpha.webp"), 24, 36).unwrap();
        assert!((100..160).contains(&alpha[3]), "half transparent: {}", alpha[3]);
        assert!(decode(b"RIFF\0\0\0\0WEBPVP8 broken", 68, 96).is_err());
    }

    /// Covers are kept at display size, and the memory budget holds however many are asked for.
    #[test]
    fn kept_at_display_size_within_the_budget() {
        let mut c = Covers::default();
        c.attach(ImageCache::new(Arc::new(|| {})));
        c.fake();
        c.set_scale(2.0);
        let url = Some("https://cdn.example.invalid/x.jpg");
        let img = c.get(7, url, 34.0, 48.0).unwrap();
        assert_eq!(img.size(), Some([68, 96]), "34 x 48 pt at 2x");
        assert_eq!(c.usage(), (1, 68 * 96 * 4 * 4 / 3));
        for id in 0..1000 {
            c.get(id, url, 34.0, 48.0);
        }
        let (n, bytes) = c.usage();
        assert!(bytes <= MEMORY_BUDGET && n < 1000, "{n} covers, {bytes} bytes");
        let mut c = Covers::default();
        c.attach(ImageCache::new(Arc::new(|| {})));
        c.fake();
        c.get(u32::MAX, url, 24.0, 24.0);
        c.get(3, url, 34.0, 48.0);
        c.forget(u32::MAX);
        assert_eq!(c.usage(), (1, 34 * 48 * 4 * 4 / 3), "only the forgotten one goes");
    }

    /// The backdrop is one small image per cover, the same at any scale, smooth and mid-light.
    #[test]
    fn blurred_backdrop_is_small_smooth_and_kept_once() {
        let mut c = Covers::default();
        c.attach(ImageCache::new(Arc::new(|| {})));
        c.fake();
        c.set_scale(2.0);
        let url = Some("https://cdn.example.invalid/x.jpg");
        let img = c.blurred(7, url).unwrap();
        assert_eq!(img.size(), Some([BLUR_W, BLUR_H]));
        c.set_scale(1.0);
        c.blurred(7, url);
        assert_eq!(c.usage().0, 1, "one backdrop whatever the scale");
        c.get(7, url, 34.0, 48.0);
        assert_eq!(c.usage().0, 2, "the cover and its backdrop are apart");
        // A hard black and white edge comes out as a gentle ramp in the middle band.
        let mut src = vec![0u8; 64 * 96 * 4];
        for (i, px) in src.chunks_mut(4).enumerate() {
            let v = if i % 64 < 32 { 0 } else { 255 };
            px.copy_from_slice(&[v, v, v, 255]);
        }
        let out = blur(&src, 64, 96);
        assert_eq!(out.len(), (BLUR_W * BLUR_H * 4) as usize);
        let row: Vec<u8> = out.chunks(4).take(BLUR_W as usize).map(|p| p[0]).collect();
        assert!(row.iter().all(|v| (60..=190).contains(v)), "{row:?}");
        assert!(row.windows(2).all(|w| w[0].abs_diff(w[1]) <= 12), "no hard edge left: {row:?}");
        assert!(decode_blurred(b"not an image").is_err());
    }

    #[test]
    fn headless_without_a_handle_never_queues() {
        let c = Covers::default();
        assert!(c.get(1, Some("https://cdn.example.invalid/1.jpg"), 34.0, 48.0).is_none());
        assert!(c.queue.lock().unwrap().jobs.is_empty());
        assert!(c.get(1, None, 34.0, 48.0).is_none());
    }
}
