# Kotora design spec

Stage 0.5. The look of Kotora v0.1, as tokens and rules the Rune code can copy. The mockup is
`mockup/kotora.html` (open it in a browser); screenshots are in `mockup/shots/`. All titles, names and
files in the mockup are made up, and covers are generated tiles.

Dark theme first. A light theme comes later (DESIGN.md §5.7), so every colour below is a token, never a
literal in view code.

## 1. Palette

Derived from the C2 icon: the navy tile (`#2a3a5c` → `#1b2640`) gives the surfaces, the tiger orange
(`#f28a22`, shade `#e57a18`) gives the accent, and the muzzle cream (`#fff0dc`) gives one highlight.

### Surfaces

| token | hex | use |
|---|---|---|
| `bg` | `#0f1626` | window background, list page |
| `side` | `#121b2e` | sidebar, macOS title bar |
| `surface` | `#182238` | cards, banner base, inline editor |
| `raised` | `#1f2b45` | selected nav item, secondary buttons, skeletons |
| `field` | `#141d31` | inputs, selects, steppers |
| `border` | `#26324d` | card borders, row dividers, progress track |
| `border_strong` | `#34446a` | button and input borders, switch off |
| `hover` | white at 0.05 | `.hover()` overlay |
| `press` | white at 0.09 | `.press()` overlay |

### Text

| token | hex | use |
|---|---|---|
| `text` | `#edf0f7` | titles, body |
| `dim` | `#a4afc6` | secondary text, inactive nav |
| `faint` | `#7d89a4` | hints, column headers, timestamps |

### Accent

| token | hex | use |
|---|---|---|
| `accent` | `#f28a22` | primary button, `+1` hover, badges, switch on, focus ring |
| `accent_hi` | `#ff9d3f` | primary hover, links, accent text on tint |
| `accent_lo` | `#e57a18` | primary press |
| `accent_tint` | `#f28a22` at 0.14 | selected chip, "new episode" tag |
| `on_accent` | `#1d1208` | text on orange (dark brown, from the icon's stripes) |
| `cream` | `#fff0dc` | the banner question, the threshold marker |

Orange means "act here". Use it for one primary action per view, not for decoration. White text on
orange fails contrast (2.5:1), so text on orange is always `on_accent`.

### Status (list statuses)

Close to MyAnimeList's conventions, so users know them at a glance, but tuned for navy.

| status | token | hex |
|---|---|---|
| Watching | `st_watching` | `#4cc38a` (green) |
| Completed | `st_completed` | `#6ea8ff` (blue) |
| On hold | `st_onhold` | `#f2c94c` (yellow) |
| Dropped | `st_dropped` | `#f0646c` (red) |
| Plan to watch | `st_plan` | `#a78bfa` (lilac) |

Other: `airing` `#4fd1e0` (cyan; airing tag), semantic `ok` = `st_watching`, `warn` = `st_onhold`,
`err` = `st_dropped`, `info` = `st_completed`. Tinted backgrounds for tags and alerts use the colour at
0.08–0.16 alpha, with a 0.3 alpha border for alerts.

Status colour is never the only signal: the status name is always written next to it.

### Contrast (WCAG ratio, sRGB)

| text | on `bg` | on `side` | on `surface` | on `raised` |
|---|---|---|---|---|
| `text` | 15.8 | 15.1 | 13.9 | 12.4 |
| `dim` | 8.2 | 7.8 | 7.2 | 6.4 |
| `faint` | 5.1 | 4.9 | 4.5 | 4.0 |
| `accent` | 7.3 | 6.9 | 6.4 | 5.7 |
| `st_watching` | 8.2 | 7.8 | 7.2 | 6.4 |
| `st_completed` | 7.5 | 7.1 | 6.6 | 5.8 |
| `st_onhold` | 11.4 | 10.8 | 10.0 | 8.9 |
| `st_dropped` | 5.8 | 5.5 | 5.1 | 4.5 |
| `st_plan` | 6.6 | 6.3 | 5.8 | 5.2 |
| `airing` | 9.9 | 9.4 | 8.7 | 7.7 |

`on_accent` on `accent`: 7.4. Everything passes AA (4.5) except `faint` on `raised` (4.0): don't put
`faint` text on `raised`; use `dim` there.

### Light (2026-11-16)

Same roles, Settings → Appearance (`[display] theme = "system" | "light" | "dark"`). System follows
the desktop through Rune (`cx.system_dark()`: macOS appearance; on Linux the XDG portal's
`color-scheme`, dark when there is none). Headless: `--theme light`. Navy-tinted paper and ink;
the tiger orange stays the fill, with `on_accent` text; every colour that is also text is darkened.

| token | hex | | token | hex |
|---|---|---|---|---|
| `bg` | `#f6f7fb` | | `text` | `#131a2b` |
| `side` | `#eceff6` | | `dim` | `#454f68` |
| `surface` | `#ffffff` | | `faint` | `#5f6982` |
| `raised` | `#e4e8f1` | | `accent` / `_lo` | `#f28a22` / `#e57a18` |
| `field` | `#ffffff` | | `accent_hi` | `#a64a00` |
| `border` | `#dbe0ea` | | `on_accent` | `#1d1208` |
| `border_strong` | `#b9c1d3` | | `cream` | `#7a3a00` (brown: the banner question) |
| `knob` | `#ffffff` (dark: `#edf0f7`) | | `airing` | `#0a6d7a` |
| `st_watching` | `#13704a` | | `st_completed` | `#2559c4` |
| `st_onhold` | `#8a5d00` | | `st_dropped` | `#c02a35` |
| `st_plan` | `#6b45d1` | | hover / press | black at 0.04 / 0.08 |

Contrast as in the dark table: `text` and `dim` 6.6+ everywhere; `faint`, `accent_hi`, the status
colours and `airing` 4.5+ on `bg`, `side` and `surface` (4.4+ on `raised`); tags on their own 0.14
tint and the banner text 4.5+. `app/src/theme.rs` tests both palettes. Rune blends alpha in linear
light, which makes tints on navy brighter than sRGB math says: measured that way, dark's tag text
on its tint is 3.5–4.7 (err lowest) and `dim` on the banner 3.9; to retune later.

View code never asks which theme is on: `PAL.x` reads the palette in force.

## 2. Typography

**Inter, the copy that Rune bundles.** Rune always embeds Inter 4.1 (400, 500, 600) and does not scan
system fonts for the UI font (Uzumaki `docs/rune-notes.md`). So Inter costs nothing, looks the same on
Arch and macOS, and needs no font setup. We don't bundle another font. Only 400/500/600 exist, so no
bold 700 and no italics. CJK titles fall back to the system CJK font, which Rune finds itself.
The mockup asks for Inter and falls back to the system font, so the screenshots use the Mac system font.

| role | size / weight | use |
|---|---|---|
| `display` | 28 / 600 | first-run welcome |
| `title` | 20 / 600 | page title (24 on the detail page) |
| `section` | 15 / 600 | card titles |
| `row_title` | 14 / 500 | list row title |
| `body` | 13 / 400 | default |
| `label` | 12 / 400–500 | row subtitles, buttons `sm`, settings help |
| `caption` | 11 / 500 | column headers and nav headings (upper case, +0.5 tracking), tags, badges |
| `mono` | 12 / 400 `Family::SystemMono` | file names, paths, config snippets |

Numbers (progress, scores, counts, times) use tabular figures: `.features("'tnum' 1")`.

## 3. Spacing, sizes, radii

- **Spacing scale (pt):** 2, 4, 6, 8, 10, 12, 14, 16, 18, 24, 28. Page padding 24; card padding 16;
  gaps between cards 16.
- **Sidebar:** 224 wide. Nav items 32 high, radius 6.
- **List rows:** 56 high (`virtual_rows(n, 56.0)`), cover 34 × 48, row padding 24 left and right.
  Columns: cover 34, title (fills), progress 250, score 64, type 92, updated 76, gap 14.
- **Progress bar:** 4 high, radius 2, track `border`, fill = the status colour.
- **Controls:** button, input and select 32 high; `sm` 26; first-run main button 40. `+1` is 38 × 26.
- **Covers:** 2:3. Row 34 × 48, banner 36 × 52, Now Playing 120 × 170, detail 200 × 284.
  Kept at display size (Uzumaki `art.rs`). While a cover loads, or if it has none, draw the tile: a
  160° gradient between two colours picked from the anime id, with the title's initials.
- **Radii:** `xs` 4 (tags, covers in rows), `sm` 6 (buttons, inputs, nav), `md` 8 (alerts, inline
  cards), `lg` 12 (cards, banner, big covers), `xl` 16 (dialogs). Pills (chips, switches) use half the
  height.
- **Window:** 1280 × 800 default; minimum about 960 × 600. Below 1100 wide, the type column hides.

## 4. Components

- **Sidebar:** brand, account chip, "My list" statuses with a dot and count, then Now Playing, Sync
  queue (orange badge with the count of waiting, failed and conflicted ops) and Settings. The Now Playing
  mini card sits at the bottom while something plays. Last line: sync state ("Synced 2 min ago",
  "Offline · 4 changes waiting").
- **List row:** cover, title with one line of tags (Airing + next episode day, "Ep N out", Rewatching,
  Waiting to sync), progress bar with `5 / 12` and `+1`, score (`–` when none), type, updated.
  Click opens the inline editor under the row (status, progress stepper, score, rewatching).
- **Now Playing banner:** on top of every page while a recognised episode passes the threshold. Orange
  tinted, with a dot, cover, "Watching *Title* episode N in *player*", watched % and match, the question
  in `cream`, then Update (primary), Not now (secondary), Wrong anime… (ghost). One banner at a time.
- **Low-confidence card:** "Not sure: is this *X* season 2, episode 3?" with Yes / Pick another.
- **Queue:** offline alert on top; conflicts as cards with "yours" and "MyAnimeList" side by side and
  Keep mine / Take theirs; then Waiting, Failed (with the service's message, Edit, Discard), Sent today
  (dimmed).
- **Settings:** four cards: Accounts, Updates, Players (Linux / macOS tab), About (Taiga credit,
  licences and notices).
- **Switch:** 36 × 20; off `border_strong`, on `accent`. The auto-update switch is OFF by default.

## 5. Icons

Line icons on a 24 grid, 1.6 stroke, round caps and joins, drawn at 16 (13–14 inside small buttons) and
tinted with `.color()`. They are Rune `Icon` statics with an SVG subset body (`path`, `rect`, `circle`),
as in Uzumaki. The set: search, chevron, play, sync, sliders (settings), external link, check, x, offline,
alert, back, file, lock, cloud. The app icon is C2; the sidebar shows it at 28, so it needs the small
variant (STATUS.md).

## 6. Motion

- Short: 120 ms for hover and press colours (Rune's built-in hover overlay), 160 ms ease-out for the
  banner sliding in and the inline editor opening, 200 ms for page cross-fades. Nothing loops except the
  loading skeleton.
- `+1` updates the number at once (optimistic); no counting animation.
- **Reduce Motion** (`cx.is_reduced_motion()`): no slides or fades; the skeleton is static. State still
  changes at once.
- Animation never delays input, and `poll` returns true only when something visible changed.

## 7. States

| state | what the user sees |
|---|---|
| Empty status | Title "No anime in Dropped" and one line of help, centred. No illustration. |
| Loading (first sync) | Four skeleton rows. With a cache, the cached list shows at once and a small "Syncing…" line sits in the sidebar instead. |
| Offline | Yellow alert on the queue page; sidebar line "Offline · N changes waiting". The list works. |
| Service error | Red alert "Can't reach MyAnimeList. Showing your saved list from 21:30." with Retry. Never a blank page. |
| Sign in again (401) | Yellow alert with a Sign in button; the queue is kept. |
| No Client ID | First-run screen with a red alert that shows the `config.toml` lines, sign-in disabled. |
| Conflict | Orange-bordered card in the queue; the row in the list says "Waiting to sync". |

Errors say what failed and what still works, in plain words.

## 8. Window chrome

- **Linux (KDE and others):** server-side decorations from the desktop; Kotora draws nothing in the title
  bar. Title "Kotora", `app_id` `io.github.hanami117.kotora`.
- **macOS:** the standard title bar with traffic lights, coloured like `side` if Rune allows it; title
  "Kotora". The mockup has a "macOS" toggle; `mockup/shots/list-macos.png` shows it.

## 9. Tokens in Rune

`rune::Color` is linear sRGB, so the tokens are written as sRGB hex and converted once with
`rune::color::hex`:

```rust
use std::sync::LazyLock;
use rune::Color;

fn hx(s: &str) -> Color { rune::color::hex(s).expect("bad hex") }

pub struct Pal {
    pub bg: Color, pub side: Color, pub surface: Color, pub raised: Color, pub field: Color,
    pub border: Color, pub border_strong: Color,
    pub text: Color, pub dim: Color, pub faint: Color,
    pub accent: Color, pub accent_hi: Color, pub accent_lo: Color, pub on_accent: Color, pub cream: Color,
    pub st_watching: Color, pub st_completed: Color, pub st_onhold: Color,
    pub st_dropped: Color, pub st_plan: Color, pub airing: Color,
}

pub static PAL: LazyLock<Pal> = LazyLock::new(|| Pal {
    bg: hx("#0f1626"), side: hx("#121b2e"), surface: hx("#182238"), raised: hx("#1f2b45"),
    field: hx("#141d31"), border: hx("#26324d"), border_strong: hx("#34446a"),
    text: hx("#edf0f7"), dim: hx("#a4afc6"), faint: hx("#7d89a4"),
    accent: hx("#f28a22"), accent_hi: hx("#ff9d3f"), accent_lo: hx("#e57a18"),
    on_accent: hx("#1d1208"), cream: hx("#fff0dc"),
    st_watching: hx("#4cc38a"), st_completed: hx("#6ea8ff"), st_onhold: hx("#f2c94c"),
    st_dropped: hx("#f0646c"), st_plan: hx("#a78bfa"), airing: hx("#4fd1e0"),
});

// Tints and overlays are alpha on a token:
// el().fill(PAL.surface).hover(Color::WHITE.alpha(0.05)).press(Color::WHITE.alpha(0.09))
// tag background: PAL.airing.alpha(0.12); accent tint: PAL.accent.alpha(0.14)
```

The mockup uses only what Rune draws: rounded rects (`.radius`), borders (`.border`), linear gradients
(`.linear_gradient` for cover tiles and the banner), soft shadows, text with truncation, and SVG-subset
icons. No blur and no backdrop blur. The episode chips' dashed border maps to `.border_dash`.
