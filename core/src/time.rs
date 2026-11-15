//! Just enough time code: RFC 3339 parsing and "2 h" style ages.

/// Parses `2018-02-24T05:34:20+00:00` or `...Z` (fractions are ignored) to unix seconds.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !(b[10] == b'T' || b[10] == b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let mut rest = &s[19..];
    if let Some(r) = rest.strip_prefix('.') {
        rest = r.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest {
        "Z" | "z" => 0,
        r if r.len() == 6 && (r.starts_with('+') || r.starts_with('-')) && &r[3..4] == ":" => {
            let v = r[1..3].parse::<i64>().ok()? * 3600 + r[4..6].parse::<i64>().ok()? * 60;
            if r.starts_with('-') { -v } else { v }
        }
        _ => return None,
    };
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se - offset)
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// "now", "2 h", "3 days", "1 week", "2 months", "1 year" (the list's Updated column).
pub fn age(then: i64, now: i64) -> String {
    let s = (now - then).max(0);
    let (n, unit) = match s {
        0..=3_599 => return "now".into(),
        3_600..=86_399 => (s / 3600, "h"),
        86_400..=604_799 => (s / 86_400, "day"),
        604_800..=2_591_999 => (s / 604_800, "week"),
        2_592_000..=31_535_999 => (s / 2_592_000, "month"),
        _ => (s / 31_536_000, "year"),
    };
    match unit {
        "h" => format!("{n} h"),
        u if n == 1 => format!("1 {u}"),
        u => format!("{n} {u}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc3339() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00+00:00"), Some(0));
        assert_eq!(parse_rfc3339("2018-02-24T05:34:20+00:00"), Some(1_519_450_460));
        assert_eq!(parse_rfc3339("2018-02-24T05:34:20Z"), Some(1_519_450_460));
        assert_eq!(parse_rfc3339("2018-02-24T07:34:20.123+02:00"), Some(1_519_450_460));
        assert_eq!(parse_rfc3339("2018-02-24"), None);
        assert_eq!(parse_rfc3339("2018-13-24T05:34:20Z"), None);
    }

    #[test]
    fn ages() {
        assert_eq!(age(100, 100), "now");
        assert_eq!(age(0, 2 * 3600), "2 h");
        assert_eq!(age(0, 86_400), "1 day");
        assert_eq!(age(0, 3 * 86_400), "3 days");
        assert_eq!(age(0, 14 * 86_400), "2 weeks");
        assert_eq!(age(0, 100 * 86_400), "3 months");
        assert_eq!(age(0, 800 * 86_400), "2 years");
        assert_eq!(age(500, 0), "now");
    }
}
