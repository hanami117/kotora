//! Scores: kept on the 100-point scale, shown in the user's own format.
//!
//! - **[`Score`]** is what the store, the queue and the services hold: 1-100 points (`None` is
//!   "not scored"). AniList gives and takes that scale (`score(format: POINT_100)`, `scoreRaw`),
//!   so a 73 stays 73. MyAnimeList scores 1-10: its 8 is 80 points, and a score goes to MAL
//!   rounded to the nearest whole 1-10 ([`Score::to_ten`]).
//! - **[`ScoreFormat`]** is how the user scores on AniList (`mediaListOptions.scoreFormat`):
//!   100 points, 10 with a decimal, 10, 5 stars or 3 smileys. MyAnimeList is always
//!   [`ScoreFormat::Point10`]. The list, the editor, the details page and `kot` show and pick
//!   scores in it.
//!
//! Picking a score in a coarse format never changes a score that already shows as that choice
//! ([`ScoreFormat::pick`]): a 73 shown as 4 stars stays 73 when 4 stars is picked again.

use serde::{Deserialize, Serialize};

/// A score on the 100-point scale, 1-100.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Score(u8);

impl Score {
    /// `points` on the 100-point scale; 0 is no score, above 100 is 100.
    pub fn from_points(points: u32) -> Option<Score> {
        (points > 0).then(|| Score(points.min(100) as u8))
    }

    /// A 1-10 score (MyAnimeList, a MAL XML file); 0 is no score, above 10 is 10.
    pub fn from_ten(n: u32) -> Option<Score> {
        Score::from_points(n.min(10) * 10)
    }

    /// The 100-point value, 1-100.
    pub fn points(self) -> u8 {
        self.0
    }

    /// The nearest whole 1-10 (half up), for MyAnimeList: 73 is 7, 75 is 8, 4 is 1.
    pub fn to_ten(self) -> u8 {
        (self.0.saturating_add(5) / 10).clamp(1, 10)
    }

    /// The 0-100 column value: `None` is 0.
    pub fn points_or_zero(s: Option<Score>) -> u8 {
        s.map_or(0, Score::points)
    }
}

/// Saved (details JSON) as the plain point number.
impl Serialize for Score {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(self.0)
    }
}

impl<'de> Deserialize<'de> for Score {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Score, D::Error> {
        let n = u32::deserialize(d)?;
        Ok(Score::from_points(n).unwrap_or(Score(1)))
    }
}

impl rusqlite::ToSql for Score {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::from(i64::from(self.0)))
    }
}

/// A column holding 1-100 (0 and NULL are no score: read it as `Option<Score>` through
/// [`Score::from_sql_points`]).
impl rusqlite::types::FromSql for Score {
    fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Score> {
        let n = v.as_i64()?;
        Score::from_points(n.clamp(0, 100) as u32).ok_or(rusqlite::types::FromSqlError::OutOfRange(n))
    }
}

impl Score {
    /// A nullable score column where 0 also means no score.
    pub fn from_sql_points(n: Option<i64>) -> Option<Score> {
        n.and_then(|n| Score::from_points(n.clamp(0, 100) as u32))
    }
}

/// How the user scores (AniList `ScoreFormat`). MyAnimeList is always `Point10`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScoreFormat {
    /// 1-100.
    #[serde(rename = "POINT_100")]
    Point100,
    /// 0.1-10.0.
    #[serde(rename = "POINT_10_DECIMAL")]
    Point10Decimal,
    /// 1-10.
    #[default]
    #[serde(rename = "POINT_10")]
    Point10,
    /// 1-5 stars.
    #[serde(rename = "POINT_5")]
    Point5,
    /// Three smileys: 1 sad, 2 neutral, 3 happy.
    #[serde(rename = "POINT_3")]
    Point3,
}

/// A smiley of [`ScoreFormat::Point3`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Smiley {
    Sad,
    Neutral,
    Happy,
}

impl Smiley {
    /// ":(", ":|", ":)": what `kot` prints.
    pub fn text(self) -> &'static str {
        match self {
            Smiley::Sad => ":(",
            Smiley::Neutral => ":|",
            Smiley::Happy => ":)",
        }
    }

    /// "Disliked", "Neutral", "Liked": what a button or a sentence says.
    pub fn word(self) -> &'static str {
        match self {
            Smiley::Sad => "Disliked",
            Smiley::Neutral => "Neutral",
            Smiley::Happy => "Liked",
        }
    }
}

impl ScoreFormat {
    pub const ALL: [ScoreFormat; 5] = [ScoreFormat::Point100, ScoreFormat::Point10Decimal, ScoreFormat::Point10, ScoreFormat::Point5, ScoreFormat::Point3];

    /// AniList's name: `POINT_100`, `POINT_10_DECIMAL`, `POINT_10`, `POINT_5`, `POINT_3`.
    pub fn anilist_name(self) -> &'static str {
        match self {
            ScoreFormat::Point100 => "POINT_100",
            ScoreFormat::Point10Decimal => "POINT_10_DECIMAL",
            ScoreFormat::Point10 => "POINT_10",
            ScoreFormat::Point5 => "POINT_5",
            ScoreFormat::Point3 => "POINT_3",
        }
    }

    pub fn from_anilist(s: &str) -> Option<ScoreFormat> {
        ScoreFormat::ALL.into_iter().find(|f| f.anilist_name() == s)
    }

    /// "100-point", "10-point decimal", "10-point", "5 stars", "3 smileys" (Settings, help).
    pub fn label(self) -> &'static str {
        match self {
            ScoreFormat::Point100 => "100-point",
            ScoreFormat::Point10Decimal => "10-point decimal",
            ScoreFormat::Point10 => "10-point",
            ScoreFormat::Point5 => "5 stars",
            ScoreFormat::Point3 => "3 smileys",
        }
    }

    /// The whole value the format shows for `s`: points (100), tenths (10 decimal), 1-10, stars
    /// 1-5, smileys 1-3. AniList's own rounding: to the nearest (half up); smileys are 1-35,
    /// 36-60, 61-100.
    pub fn value(self, s: Score) -> u8 {
        let p = s.points();
        match self {
            ScoreFormat::Point100 | ScoreFormat::Point10Decimal => p,
            ScoreFormat::Point10 => s.to_ten(),
            ScoreFormat::Point5 => (p.saturating_add(10) / 20).clamp(1, 5),
            ScoreFormat::Point3 => match p {
                0..=35 => 1,
                36..=60 => 2,
                _ => 3,
            },
        }
    }

    /// The score that shows as `value` ([`ScoreFormat::value`]'s inverse): 3 stars is 60, a
    /// happy smiley 85 (AniList's own values). `None` for 0 or a value out of range.
    pub fn score_of(self, value: u32) -> Option<Score> {
        let points = match (self, value) {
            (_, 0) => return None,
            (ScoreFormat::Point100 | ScoreFormat::Point10Decimal, 1..=100) => value,
            (ScoreFormat::Point10, 1..=10) => value * 10,
            (ScoreFormat::Point5, 1..=5) => value * 20,
            (ScoreFormat::Point3, 1) => 35,
            (ScoreFormat::Point3, 2) => 60,
            (ScoreFormat::Point3, 3) => 85,
            _ => return None,
        };
        Score::from_points(points)
    }

    /// The smiley of `s`, in the smiley format.
    pub fn smiley(self, s: Score) -> Option<Smiley> {
        (self == ScoreFormat::Point3).then(|| match self.value(s) {
            1 => Smiley::Sad,
            2 => Smiley::Neutral,
            _ => Smiley::Happy,
        })
    }

    /// `s` as the user reads it: "73", "7.3", "7", "★★★★☆", ":)".
    pub fn show(self, s: Score) -> String {
        let v = self.value(s);
        match self {
            ScoreFormat::Point100 | ScoreFormat::Point10 => v.to_string(),
            ScoreFormat::Point10Decimal => format!("{}.{}", v / 10, v % 10),
            ScoreFormat::Point5 => "\u{2605}".repeat(v as usize) + &"\u{2606}".repeat(5 - v as usize),
            ScoreFormat::Point3 => self.smiley(s).map_or("", Smiley::text).to_string(),
        }
    }

    /// [`ScoreFormat::show`] for a sentence or a queue line: "73", "7.3", "4 stars", "Liked".
    pub fn words(self, s: Score) -> String {
        match self {
            ScoreFormat::Point5 => match self.value(s) {
                1 => "1 star".into(),
                n => format!("{n} stars"),
            },
            ScoreFormat::Point3 => self.smiley(s).map_or("", Smiley::word).to_string(),
            _ => self.show(s),
        }
    }

    /// The scores a menu or the editor offers, low to high: 10 whole steps for the 100 and 10
    /// point formats (the editor steps finer with [`ScoreFormat::fine_step`]), 5 stars, 3 smileys.
    pub fn choices(self) -> Vec<Score> {
        let n = match self {
            ScoreFormat::Point100 | ScoreFormat::Point10Decimal | ScoreFormat::Point10 => 10,
            ScoreFormat::Point5 => 5,
            ScoreFormat::Point3 => 3,
        };
        (1..=n).filter_map(|i| match self {
            ScoreFormat::Point100 | ScoreFormat::Point10Decimal => Score::from_points(i * 10),
            _ => self.score_of(i),
        })
        .collect()
    }

    /// The editor's − / + step in points, for the formats finer than the choices: 1 point (100)
    /// or 0.1 (10 decimal).
    pub fn fine_step(self) -> Option<u8> {
        matches!(self, ScoreFormat::Point100 | ScoreFormat::Point10Decimal).then_some(1)
    }

    /// `cur` moved by `delta` fine steps, 1-100; from no score it starts at the middle (50).
    pub fn step(self, cur: Option<Score>, delta: i32) -> Option<Score> {
        let from = cur.map_or(50 - delta, |s| i32::from(s.points()));
        Score::from_points((from + delta).clamp(1, 100) as u32)
    }

    /// Whether `a` and `b` show the same ([`ScoreFormat::value`]).
    pub fn same(self, a: Option<Score>, b: Option<Score>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => self.value(a) == self.value(b),
            (None, None) => true,
            _ => false,
        }
    }

    /// The score a pick of `choice` leaves when the entry has `cur`: `cur` itself when it already
    /// shows as `choice` (no change, no precision lost), else `choice`.
    pub fn pick(self, cur: Option<Score>, choice: Option<Score>) -> Option<Score> {
        if self.same(cur, choice) { cur } else { choice }
    }

    /// What a score typed in this format means (`kot set --score`): `none` (or 0) removes the
    /// score; else 1-100, 0.1-10.0, 1-10, 1-5 (or `***`, `★★★`) or 1-3 (or `:(` `:|` `:)`).
    pub fn parse(self, text: &str) -> Result<Option<Score>, String> {
        let t = text.trim();
        if matches!(t.to_ascii_lowercase().as_str(), "none" | "0" | "-" | "") {
            return Ok(None);
        }
        let bad = || format!("the score is {}, or none", self.range_text());
        let got = match self {
            ScoreFormat::Point10Decimal => {
                let x: f64 = t.parse().map_err(|_| bad())?;
                if !(0.05..=10.0).contains(&x) {
                    return Err(bad());
                }
                Score::from_points((x * 10.0).round() as u32)
            }
            ScoreFormat::Point5 if t.chars().all(|c| matches!(c, '*' | '\u{2605}' | '\u{2606}')) => {
                self.score_of(t.chars().filter(|c| *c != '\u{2606}').count() as u32)
            }
            ScoreFormat::Point3 if Smiley::ALL.iter().any(|s| s.text() == t) => {
                let i = Smiley::ALL.iter().position(|s| s.text() == t).unwrap_or(0) as u32 + 1;
                self.score_of(i)
            }
            _ => self.score_of(t.parse().map_err(|_| bad())?),
        };
        got.map(Some).ok_or_else(bad)
    }

    /// "a number from 1 to 100", … (`kot` help and errors).
    pub fn range_text(self) -> &'static str {
        match self {
            ScoreFormat::Point100 => "a number from 1 to 100",
            ScoreFormat::Point10Decimal => "a number from 0.1 to 10.0",
            ScoreFormat::Point10 => "a number from 1 to 10",
            ScoreFormat::Point5 => "1 to 5 stars (a number, or ***)",
            ScoreFormat::Point3 => "1 to 3 (or :( :| :))",
        }
    }
}

/// [`Score::from_ten`] in short: `ten(8)` is a 1-10 score of 8 (80 points).
pub fn ten(n: u32) -> Option<Score> {
    Score::from_ten(n)
}

impl Smiley {
    pub const ALL: [Smiley; 3] = [Smiley::Sad, Smiley::Neutral, Smiley::Happy];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(n: u32) -> Score {
        Score::from_points(n).unwrap()
    }

    #[test]
    fn points_and_tens() {
        assert_eq!(Score::from_points(0), None);
        assert_eq!(Score::from_points(150).map(Score::points), Some(100));
        assert_eq!(Score::from_ten(8).map(Score::points), Some(80));
        assert_eq!(Score::from_ten(0), None);
        // MyAnimeList takes the nearest whole: the 73 → 7 case, half up.
        assert_eq!((p(73).to_ten(), p(75).to_ten(), p(4).to_ten(), p(100).to_ten()), (7, 8, 1, 10));
    }

    #[test]
    fn each_format_shows_a_73() {
        let s = p(73);
        let shown: Vec<String> = ScoreFormat::ALL.iter().map(|f| f.show(s)).collect();
        assert_eq!(shown, ["73", "7.3", "7", "\u{2605}\u{2605}\u{2605}\u{2605}\u{2606}", ":)"]);
        assert_eq!(ScoreFormat::Point10Decimal.show(p(80)), "8.0");
        assert_eq!(ScoreFormat::Point10Decimal.show(p(5)), "0.5");
        assert_eq!(ScoreFormat::Point5.words(p(20)), "1 star");
        assert_eq!(ScoreFormat::Point3.words(p(50)), "Neutral");
    }

    #[test]
    fn smileys_and_stars_follow_anilist_bounds() {
        let f = ScoreFormat::Point3;
        assert_eq!([1, 35, 36, 60, 61, 100].map(|n| f.value(p(n))), [1, 1, 2, 2, 3, 3]);
        let f = ScoreFormat::Point5;
        assert_eq!([1, 29, 30, 50, 89, 90, 100].map(|n| f.value(p(n))), [1, 1, 2, 3, 4, 5, 5]);
    }

    #[test]
    fn choices_round_trip_through_value() {
        for f in ScoreFormat::ALL {
            let c = f.choices();
            assert!(!c.is_empty() && c.windows(2).all(|w| w[0] < w[1]), "{f:?}");
            if !matches!(f, ScoreFormat::Point100 | ScoreFormat::Point10Decimal) {
                for (i, s) in c.iter().enumerate() {
                    assert_eq!(f.value(*s) as usize, i + 1, "{f:?}");
                }
            }
        }
        assert_eq!(ScoreFormat::Point3.choices().iter().map(|s| s.points()).collect::<Vec<_>>(), [35, 60, 85]);
    }

    #[test]
    fn picking_what_already_shows_keeps_the_precise_score() {
        let f = ScoreFormat::Point5;
        assert_eq!(f.pick(Some(p(73)), f.score_of(4)), Some(p(73)), "4 stars again: still 73");
        assert_eq!(f.pick(Some(p(73)), f.score_of(5)), Some(p(100)));
        assert_eq!(f.pick(Some(p(73)), None), None);
        assert_eq!(ScoreFormat::Point100.pick(Some(p(73)), Some(p(70))), Some(p(70)));
    }

    #[test]
    fn fine_steps_stay_in_range() {
        let f = ScoreFormat::Point100;
        assert_eq!(f.step(None, 1), Some(p(50)));
        assert_eq!(f.step(Some(p(73)), 1), Some(p(74)));
        assert_eq!(f.step(Some(p(1)), -1), Some(p(1)));
        assert_eq!(f.step(Some(p(100)), 1), Some(p(100)));
        assert_eq!(ScoreFormat::Point10.fine_step(), None);
    }

    #[test]
    fn parses_what_kot_users_type() {
        assert_eq!(ScoreFormat::Point100.parse("73"), Ok(Some(p(73))));
        assert_eq!(ScoreFormat::Point10Decimal.parse("7.3"), Ok(Some(p(73))));
        assert_eq!(ScoreFormat::Point10Decimal.parse("8"), Ok(Some(p(80))));
        assert_eq!(ScoreFormat::Point10.parse("7"), Ok(Some(p(70))));
        assert_eq!(ScoreFormat::Point5.parse("***"), Ok(Some(p(60))));
        assert_eq!(ScoreFormat::Point5.parse("\u{2605}\u{2605}\u{2605}\u{2605}\u{2606}"), Ok(Some(p(80))));
        assert_eq!(ScoreFormat::Point3.parse(":)"), Ok(Some(p(85))));
        assert_eq!(ScoreFormat::Point3.parse("2"), Ok(Some(p(60))));
        for f in ScoreFormat::ALL {
            assert_eq!(f.parse("none"), Ok(None));
        }
        assert!(ScoreFormat::Point10.parse("73").is_err());
        assert!(ScoreFormat::Point10Decimal.parse("10.5").is_err());
        assert!(ScoreFormat::Point5.parse("6").is_err());
        assert!(ScoreFormat::Point3.parse("great").is_err());
    }

    #[test]
    fn serde_names_are_anilists() {
        for f in ScoreFormat::ALL {
            assert_eq!(serde_json::to_string(&f).unwrap(), format!("\"{}\"", f.anilist_name()));
            assert_eq!(ScoreFormat::from_anilist(f.anilist_name()), Some(f));
        }
        assert_eq!(serde_json::from_str::<Option<Score>>("73").unwrap(), Some(p(73)));
    }
}
