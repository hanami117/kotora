//! The Profile page (the sidebar's account card): the picture, the user name and when they
//! joined, a link to the profile on MyAnimeList, MyAnimeList's numbers for the user's anime
//! (days, episodes, mean score, the count per status as a stacked bar) and how the user scores,
//! counted from the saved list. Sign out at the bottom does what Settings' Sign out does.
//!
//! Read only. The numbers come with the user answer a sync already asks for
//! (`online::refresh_user`, one request) and are saved in the store; before the first one the
//! counts come from the saved list and the rest shows a dash.

use kotora_core::Status;
use kotora_core::service::{AnimeStats, Profile};
use rune::{El, ScrollSpec, TextStyle, col, el, row, spacer, text};

use crate::model::{Entry, State};
use crate::theme::{self, PAL};
use crate::ui::{AVATAR_ID, card, hex, small_button};

/// How many entries have each score, 1 to 10 (index 0 is score 1). Rewatches count once, by
/// their own entry; unscored entries are left out.
pub fn score_spread(entries: &[Entry]) -> [u32; 10] {
    let mut n = [0u32; 10];
    for e in entries {
        if let Some(sc @ 1..=10) = e.score {
            n[usize::from(sc) - 1] += 1;
        }
    }
    n
}

/// The mean of the scores in a spread, `None` with no score.
pub fn spread_mean(n: &[u32; 10]) -> Option<f64> {
    let count: u32 = n.iter().sum();
    (count > 0).then(|| n.iter().enumerate().map(|(i, c)| f64::from(*c) * (i as f64 + 1.0)).sum::<f64>() / f64::from(count))
}

/// The counts per status: MyAnimeList's when known, else the saved list's.
fn counts(s: &State, stats: Option<&AnimeStats>) -> [(Status, u32); 5] {
    Status::ALL.map(|st| (st, stats.map_or_else(|| s.count(st) as u32, |x| x.count(st))))
}

/// "1,234".
fn grouped(n: u32) -> String {
    crate::details::thousands(n)
}

/// "Mar 2, 2019" from MAL's date-time.
fn joined_text(p: &Profile) -> Option<String> {
    let j = p.joined.as_deref()?;
    Some(crate::details::date_text(j.get(..10).unwrap_or(j)))
}

pub fn page(s: &State, wide: bool) -> El {
    let Some((name, service)) = &s.account else { return el() };
    let profile = s.profile.clone().unwrap_or_default();
    let stats = profile.stats.as_ref();
    let mut body = col().px(theme::PAGE_PAD).pt(4.0).pb(theme::PAGE_PAD).gap(16.0).max_w(1120.0);
    body = body.child(head(s, name, service, &profile));
    let anime = anime_card(s, stats);
    let scores = scores_card(s);
    body = body.child(if wide {
        row().gap(16.0).w_full().items_start().child(anime.grow(1.0).min_w(0.0)).child(scores.w(400.0))
    } else {
        col().gap(16.0).w_full().child(anime).child(scores)
    });
    body = body.child(
        row()
            .key("profile/account")
            .gap(12.0)
            .center_y()
            .px(16.0)
            .py(12.0)
            .radius(theme::R_LG)
            .border(1.0, PAL.border)
            .child(text(format!("Signed in as {name} on {service}.")).font(theme::BODY).color(PAL.dim).grow(1.0).min_w(0.0).wrap())
            .child(small_button("profile/sign-out", "Sign out", false, Some(crate::online::sign_out))),
    );
    col()
        .key("page/profile")
        .w_full()
        .h_full()
        .child(crate::ui::top(s, "Profile", None, false))
        .child(col().key("profile-scroll").scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(body))
}

/// The picture, the name, when they joined, and the link to the profile on MyAnimeList.
fn head(s: &State, name: &str, service: &str, p: &Profile) -> El {
    const SIZE: f32 = 72.0;
    let initial: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    // As in the sidebar: MAL's blue in both themes, the picture over it once loaded.
    let mut face = el()
        .key("profile/avatar")
        .size(SIZE, SIZE)
        .radius(SIZE / 2.0)
        .linear_gradient(160.0 * std::f32::consts::PI / 180.0, [(0.0, hex("#2e51a2")), (1.0, hex("#1d3170"))])
        .center()
        .child(text(initial).font(TextStyle::new(28.0, 600.0)).color(hex("#edf0f7")));
    if let Some(img) = s.covers.get(AVATAR_ID, s.avatar.as_deref(), SIZE, SIZE) {
        face = face.child(el().absolute(0.0, 0.0).size(SIZE, SIZE).radius(SIZE / 2.0).pixels(img.pixels()));
    }
    let joined = joined_text(p).map_or_else(|| service.to_string(), |j| format!("{service} \u{b7} Joined {j}"));
    let url = s.service.profile_url(name);
    row()
        .key("profile/head")
        .gap(18.0)
        .padding(18.0)
        .center_y()
        .radius(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.border)
        .child(face)
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(text(name.to_string()).key("profile/name").font(TextStyle::new(22.0, 600.0)).color(PAL.text).truncate())
                .child(text(joined).key("profile/joined").font(theme::BODY).color(PAL.dim)),
        )
        .child(
            row()
                .key("profile/open-mal")
                .role("link")
                .label(format!("Open on {}", s.service.title()))
                .focusable(true)
                .h(32.0)
                .px(12.0)
                .gap(8.0)
                .center_y()
                .radius(theme::R_SM)
                .fill(PAL.raised)
                .border(1.0, PAL.border_strong)
                .hover(PAL.hover())
                .press(PAL.press())
                .cursor(rune::Cursor::Pointer)
                .on_press(move |s: &mut State| crate::online::open_url(s, url.clone()))
                .child(text(format!("Open on {}", s.service.title())).font(theme::LABEL_BTN).color(PAL.text))
                .child(rune::icon(&crate::icons::EXTERNAL, 14.0).color(PAL.dim)),
        )
}

/// A big number and its caption.
fn figure(key: &str, big: String, small: &str) -> El {
    col()
        .key(key.to_string())
        .grow(1.0)
        .gap(2.0)
        .child(text(big).font(TextStyle::new(24.0, 600.0).features("'tnum' 1")).color(PAL.text))
        .child(text(small.to_string()).font(theme::LABEL).color(PAL.faint))
}

/// MyAnimeList's numbers: days, episodes, mean score; then the statuses as one stacked bar.
fn anime_card(s: &State, stats: Option<&AnimeStats>) -> El {
    let dash = || "\u{2013}".to_string();
    let figures = row()
        .key("profile/figures")
        .gap(16.0)
        .pt(6.0)
        .child(figure("profile/days", stats.map_or_else(dash, |x| format!("{:.1}", x.days)), "Days watched"))
        .child(figure("profile/episodes", stats.map_or_else(dash, |x| grouped(x.episodes)), "Episodes"))
        .child(figure("profile/mean", stats.filter(|x| x.mean_score > 0.0).map_or_else(dash, |x| format!("{:.2}", x.mean_score)), "Mean score"));
    let counts = counts(s, stats);
    let total: u32 = counts.iter().map(|c| c.1).sum();
    let mut bar = row().key("profile/bar").w_full().h(12.0).gap(2.0).radius(6.0).clip_rounded(6.0);
    if total == 0 {
        bar = bar.fill(PAL.border);
    }
    for (st, n) in counts.iter().filter(|c| c.1 > 0) {
        bar = bar.child(el().key(format!("profile/bar/{}", st.slug())).grow(*n as f32).h_full().fill(PAL.status(*st)));
    }
    let mut legend = row().key("profile/legend").flex_wrap().gap(8.0).pt(4.0);
    for (st, n) in counts {
        legend = legend.child(
            row()
                .key(format!("profile/count/{}", st.slug()))
                .w(150.0)
                .gap(8.0)
                .py(3.0)
                .center_y()
                .child(el().size(8.0, 8.0).radius(4.0).fill(PAL.status(st)))
                .child(text(st.label()).font(theme::BODY).color(PAL.dim))
                .child(spacer())
                .child(text(grouped(n)).font(theme::NUM_MED).color(PAL.text)),
        );
    }
    let mut c = card("profile/anime", "Anime").gap(12.0).child(figures).child(
        col()
            .gap(6.0)
            .pt(4.0)
            .child(row().child(text("By status").font(theme::CAPTION).color(PAL.faint)).child(spacer()).child(text(format!("{} entries", grouped(total))).font(theme::LABEL_NUM).color(PAL.dim)))
            .child(bar),
    );
    c = c.child(legend);
    match stats {
        Some(x) if x.rewatched > 0 => c.child(text(format!("Rewatched {}", kotora_core::queue::times(x.rewatched))).font(theme::LABEL).color(PAL.faint)),
        Some(_) => c,
        None => c.child(text(format!("{}'s numbers come with the next sync; the counts are from your saved list.", s.service.title())).key("profile/pending").font(theme::LABEL).color(PAL.faint).wrap()),
    }
}

/// How the user scores: a small bar chart of the scores 1 to 10, from the saved list.
fn scores_card(s: &State) -> El {
    const CHART_H: f32 = 110.0;
    let n = score_spread(&s.entries);
    let max = n.iter().copied().max().unwrap_or(0).max(1);
    let scored: u32 = n.iter().sum();
    let mut chart = row().key("profile/chart").w_full().h(CHART_H + 36.0).gap(6.0).items_end();
    for (i, c) in n.iter().enumerate() {
        let h = if *c == 0 { 2.0 } else { (CHART_H * *c as f32 / max as f32).max(4.0) };
        chart = chart.child(
            col()
                .key(format!("profile/score/{}", i + 1))
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .items_center()
                .child(text(if *c == 0 { String::new() } else { c.to_string() }).font(theme::SMALL.features("'tnum' 1")).color(PAL.dim))
                .child(el().w_full().h(h).radius(3.0).fill(if *c == 0 { PAL.border } else { PAL.accent }))
                .child(text((i + 1).to_string()).font(theme::LABEL_NUM).color(PAL.faint)),
        );
    }
    let line = match spread_mean(&n) {
        Some(m) => format!("{} scored \u{b7} mean {m:.2}", grouped(scored)),
        None => "No scores on your list yet.".to_string(),
    };
    card("profile/scores", "Your scores").gap(10.0).child(chart.pt(6.0)).child(text(line).key("profile/scored").font(theme::LABEL).color(PAL.dim))
}

/// Made-up numbers for the scenes and the fake state.
pub fn fake() -> Profile {
    Profile {
        joined: Some("2019-03-02T10:20:30+00:00".into()),
        stats: Some(AnimeStats { days: 63.1, episodes: 3790, mean_score: 7.62, rewatched: 4, watching: 11, completed: 236, on_hold: 7, dropped: 2, plan_to_watch: 48 }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_score_spread_counts_each_score_once() {
        let mut s = State::fake();
        for e in &mut s.entries {
            e.score = None;
        }
        assert_eq!(score_spread(&s.entries), [0; 10]);
        assert_eq!(spread_mean(&[0; 10]), None);
        let scores = [Some(8), Some(8), Some(10), Some(1), None, Some(7)];
        for (e, sc) in s.entries.iter_mut().zip(scores) {
            e.score = sc;
        }
        let n = score_spread(&s.entries);
        assert_eq!(n, [1, 0, 0, 0, 0, 0, 1, 2, 0, 1]);
        assert!((spread_mean(&n).unwrap() - 34.0 / 5.0).abs() < 1e-9);
        // A score out of range (never from MAL) is left out rather than miscounted.
        s.entries[4].score = Some(11);
        assert_eq!(score_spread(&s.entries).iter().sum::<u32>(), 5);
    }

    #[test]
    fn counts_fall_back_to_the_saved_list() {
        let s = State::fake();
        let mine = counts(&s, None);
        assert_eq!(mine[0], (Status::Watching, s.count(Status::Watching) as u32));
        let x = fake().stats.unwrap();
        assert_eq!(counts(&s, Some(&x))[2], (Status::Completed, 236));
        assert_eq!(joined_text(&fake()).as_deref(), Some("Mar 2, 2019"));
    }
}
