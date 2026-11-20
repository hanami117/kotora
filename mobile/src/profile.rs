//! The Profile page, as the desktop's (`app/src/profile.rs`): the user name and when they joined,
//! the service's numbers for the user's anime (days, episodes, mean score, the count per status)
//! and how the user scores, counted from the saved list. Read only: the numbers come with the
//! user answer every sync already asks for; before the first one the counts come from the saved
//! list and the rest is `None` (the app shows a dash).

use kotora_core::service::AnimeStats;

use super::*;

/// One status and its count (MyAnimeList's when known, else the saved list's).
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ProfileCount {
    pub status: ListStatus,
    pub count: u32,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ProfileInfo {
    pub service: Service,
    pub user_name: String,
    pub picture_url: Option<String>,
    /// "MyAnimeList · Joined Mar 2, 2019", or just "MyAnimeList".
    pub joined_line: String,
    /// "Open on MyAnimeList".
    pub open_label: String,
    pub profile_url: String,
    /// `None` before the first sync: the app shows a dash.
    pub days: Option<f64>,
    pub episodes: Option<u32>,
    /// `None` also when the service has no mean (0).
    pub mean_score: Option<f64>,
    /// Watching, Completed, On hold, Dropped, Plan to watch (the desktop's order).
    pub counts: Vec<ProfileCount>,
    pub total: u32,
    /// "Rewatched 4 times", when the service counted any.
    pub rewatched_line: Option<String>,
    /// "MyAnimeList's numbers come with the next sync; …" before the first one.
    pub pending_line: Option<String>,
    /// How many entries have each score, 1 to 10 (index 0 is score 1).
    pub score_spread: Vec<u32>,
    /// "123 scored · mean 7.62", or "No scores on your list yet."
    pub scored_line: String,
    /// "Signed in as hana on MyAnimeList."
    pub signed_in_line: String,
}

/// 98765 → "98,765" (the desktop's `details::thousands`).
pub fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "2019-03-02" → "Mar 2, 2019" (the desktop's `details::date_text`).
fn date_text(d: &str) -> String {
    let p: Vec<&str> = d.split('-').collect();
    let month = |m: &str| m.parse::<usize>().ok().filter(|m| (1..=12).contains(m)).map(|m| MONTHS[m - 1]);
    match p.as_slice() {
        [y, m, day] => match (month(m), day.parse::<u32>()) {
            (Some(m), Ok(day)) => format!("{m} {day}, {y}"),
            _ => d.to_string(),
        },
        [y, m] => month(m).map_or_else(|| d.to_string(), |m| format!("{m} {y}")),
        _ => d.to_string(),
    }
}

fn spread(items: &[ListItem]) -> [u32; 10] {
    let mut n = [0u32; 10];
    for it in items {
        if let Some(sc) = it.score {
            n[usize::from(sc.to_ten().clamp(1, 10)) - 1] += 1;
        }
    }
    n
}

fn spread_mean(n: &[u32; 10]) -> Option<f64> {
    let count: u32 = n.iter().sum();
    (count > 0).then(|| n.iter().enumerate().map(|(i, c)| f64::from(*c) * (i as f64 + 1.0)).sum::<f64>() / f64::from(count))
}

fn info(service: Service, name: String, picture: Option<String>, joined: Option<&str>, stats: Option<AnimeStats>, items: &[ListItem]) -> ProfileInfo {
    let title = service.id().title();
    let order = [Status::Watching, Status::Completed, Status::OnHold, Status::Dropped, Status::PlanToWatch];
    let counts: Vec<ProfileCount> = order
        .iter()
        .map(|st| ProfileCount { status: (*st).into(), count: stats.map_or_else(|| items.iter().filter(|i| i.status == *st).count() as u32, |x| x.count(*st)) })
        .collect();
    let n = spread(items);
    let scored: u32 = n.iter().sum();
    ProfileInfo {
        service,
        joined_line: joined.map(|j| date_text(j.get(..10).unwrap_or(j))).map_or_else(|| title.to_string(), |j| format!("{title} \u{b7} Joined {j}")),
        open_label: format!("Open on {title}"),
        profile_url: service.id().profile_url(&name),
        days: stats.map(|x| x.days),
        episodes: stats.map(|x| x.episodes),
        mean_score: stats.filter(|x| x.mean_score > 0.0).map(|x| x.mean_score),
        total: counts.iter().map(|c| c.count).sum(),
        counts,
        rewatched_line: stats.filter(|x| x.rewatched > 0).map(|x| format!("Rewatched {}", kotora_core::queue::times(x.rewatched))),
        pending_line: stats.is_none().then(|| format!("{title}'s numbers come with the next sync; the counts are from your saved list.")),
        score_spread: n.to_vec(),
        scored_line: spread_mean(&n).map_or_else(|| "No scores on your list yet.".to_string(), |m| format!("{} scored \u{b7} mean {m:.2}", thousands(scored))),
        signed_in_line: format!("Signed in as {name} on {title}."),
        user_name: name,
        picture_url: picture,
    }
}

#[uniffi::export]
impl Kotora {
    /// The Profile page of a signed-in service (the active one in the app). `None`: not signed in.
    pub fn profile(&self, service: Service) -> Option<ProfileInfo> {
        let account = self.accounts().into_iter().find(|a| a.service == service)?;
        let store = self.store();
        let p = store.profile(service.name()).ok().flatten().unwrap_or_default();
        let items = store.load_list(Scope::new(service.name(), Kind::Anime)).unwrap_or_default();
        Some(info(service, account.user_name, account.picture_url, p.joined.as_deref(), p.stats, &items))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_says_what_the_desktop_says() {
        let stats = AnimeStats { days: 63.1, episodes: 3790, mean_score: 7.62, rewatched: 4, watching: 11, completed: 236, on_hold: 7, dropped: 2, plan_to_watch: 48 };
        let p = info(Service::Mal, "hana".into(), None, Some("2019-03-02T10:20:30+00:00"), Some(stats), &[]);
        assert_eq!(p.joined_line, "MyAnimeList \u{b7} Joined Mar 2, 2019");
        assert_eq!(p.open_label, "Open on MyAnimeList");
        assert_eq!(p.profile_url, "https://myanimelist.net/profile/hana");
        assert_eq!(p.counts[1], ProfileCount { status: ListStatus::Completed, count: 236 });
        assert_eq!(p.total, 304);
        assert_eq!(p.rewatched_line.as_deref(), Some("Rewatched 4 times"));
        assert_eq!(p.pending_line, None);
        assert_eq!(p.scored_line, "No scores on your list yet.");
        assert_eq!(p.signed_in_line, "Signed in as hana on MyAnimeList.");
        let q = info(Service::AniList, "hana".into(), None, None, None, &[]);
        assert_eq!(q.joined_line, "AniList");
        assert_eq!(q.days, None);
        assert_eq!(q.pending_line.as_deref(), Some("AniList's numbers come with the next sync; the counts are from your saved list."));
        assert_eq!(thousands(98765), "98,765");
        assert_eq!(spread_mean(&[1, 0, 0, 0, 0, 0, 1, 2, 0, 1]).map(|m| (m * 10.0).round()), Some(68.0));
    }
}
