//! Just enough time code: RFC 3339 parsing, "2 h" style ages, calendar dates and the local time.
//!
//! The local time comes from the C library (`localtime_r`, which reads `TZ` and
//! `/etc/localtime` with the system's zone data, DST included); everything else is plain
//! arithmetic on unix seconds and UTC offsets, so no time zone crate is needed.

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
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day number (days since 1970-01-01): year, month, day.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Monday 0 … Sunday 6, of a day number.
pub fn weekday(days: i64) -> u8 {
    // 1970-01-01 was a Thursday.
    (days + 3).rem_euclid(7) as u8
}

/// `2026-10-04` → its day number. A shorter date (`2026-10`, `2026`) is `None`.
pub fn parse_date(s: &str) -> Option<i64> {
    let mut it = s.trim().splitn(3, '-');
    let (y, m, d) = (it.next()?.parse::<i64>().ok()?, it.next()?.parse::<i64>().ok()?, it.next()?.parse::<i64>().ok()?);
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then(|| days_from_civil(y, m, d))
}

/// A moment as a clock on the wall shows it, at a UTC offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    /// Days since 1970-01-01 in that zone.
    pub day: i64,
    pub year: i64,
    pub month: u32,
    pub date: u32,
    /// Monday 0 … Sunday 6.
    pub weekday: u8,
    /// Minutes after midnight.
    pub minute: u32,
    /// Seconds east of UTC.
    pub offset: i64,
}

impl Clock {
    /// `t` (unix seconds) at `offset` seconds east of UTC.
    pub fn at(t: i64, offset: i64) -> Clock {
        let local = t + offset;
        let day = local.div_euclid(86_400);
        let (year, month, date) = civil_from_days(day);
        Clock { day, year, month, date, weekday: weekday(day), minute: (local.rem_euclid(86_400) / 60) as u32, offset }
    }

    /// `t` in the user's time zone (the C library's `localtime_r`).
    pub fn local(t: i64) -> Clock {
        Clock::at(t, local_offset(t))
    }

    /// Unix seconds of this zone's midnight that starts the day.
    pub fn midnight(&self) -> i64 {
        self.day * 86_400 - self.offset
    }

    /// "23:30".
    pub fn hhmm(&self) -> String {
        format!("{:02}:{:02}", self.minute / 60, self.minute % 60)
    }
}

/// The user's UTC offset at `t`, in seconds east (DST included). 0 when the C library can't say.
pub fn local_offset(t: i64) -> i64 {
    #[cfg(unix)]
    {
        let tt = t as libc::time_t;
        // SAFETY: `tm` is plain data the call fills in; `localtime_r` is the thread-safe form.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        let ok = unsafe { !libc::localtime_r(&tt, &mut tm).is_null() };
        if ok {
            return tm.tm_gmtoff as i64;
        }
    }
    #[cfg(windows)]
    if let Some(offset) = windows_offset(t) {
        return offset;
    }
    let _ = t;
    0
}

/// Windows: the offset from the system's time zone rules for the year of `t` (DST included).
#[cfg(windows)]
fn windows_offset(t: i64) -> Option<i64> {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::Time::{GetTimeZoneInformationForYear, SystemTimeToTzSpecificLocalTime, TIME_ZONE_INFORMATION};
    let day = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(day);
    // SYSTEMTIME holds the years 1601 to 30827.
    if !(1601..=30_000).contains(&y) {
        return None;
    }
    let utc = SYSTEMTIME {
        wYear: y as u16,
        wMonth: m as u16,
        wDayOfWeek: 0,
        wDay: d as u16,
        wHour: (secs / 3600) as u16,
        wMinute: (secs % 3600 / 60) as u16,
        wSecond: (secs % 60) as u16,
        wMilliseconds: 0,
    };
    // SAFETY: plain data structs the calls fill in; a null zone means the current one.
    let local = unsafe {
        let mut zone: TIME_ZONE_INFORMATION = std::mem::zeroed();
        if GetTimeZoneInformationForYear(utc.wYear, std::ptr::null(), &mut zone) == 0 {
            return None;
        }
        let mut local: SYSTEMTIME = std::mem::zeroed();
        if SystemTimeToTzSpecificLocalTime(&zone, &utc, &mut local) == 0 {
            return None;
        }
        local
    };
    let wall = days_from_civil(i64::from(local.wYear), i64::from(local.wMonth), i64::from(local.wDay)) * 86_400 + i64::from(local.wHour) * 3600 + i64::from(local.wMinute) * 60 + i64::from(local.wSecond);
    Some(wall - t)
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
    fn dates_and_clocks() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        for d in [-1_000, 0, 59, 365, 11_016, 20_000, 20_759, 30_000] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, i64::from(m), i64::from(dd)), d);
        }
        assert_eq!(parse_date("2026-10-04"), Some(days_from_civil(2026, 10, 4)));
        assert_eq!(parse_date("2026-10"), None);
        assert_eq!(parse_date("2026-13-01"), None);
        // 2026-10-04 is a Sunday.
        assert_eq!(weekday(parse_date("2026-10-04").unwrap()), 6);
        assert_eq!(weekday(parse_date("2026-10-05").unwrap()), 0);
        // Saturday 2026-10-03 23:30 JST is 14:30 UTC, and 16:30 in Central Europe (summer time).
        let t = parse_date("2026-10-03").unwrap() * 86_400 + (23 * 60 + 30) * 60 - 9 * 3600;
        let jst = Clock::at(t, 9 * 3600);
        assert_eq!((jst.weekday, jst.hhmm().as_str(), jst.date), (5, "23:30", 3));
        let cest = Clock::at(t, 2 * 3600);
        assert_eq!((cest.weekday, cest.hhmm().as_str()), (5, "16:30"));
        // Across midnight: 01:00 JST Sunday is Saturday evening in New York (UTC-4).
        let t = parse_date("2026-10-04").unwrap() * 86_400 + 3600 - 9 * 3600;
        let ny = Clock::at(t, -4 * 3600);
        assert_eq!((ny.weekday, ny.hhmm().as_str(), ny.date), (5, "12:00", 3));
        assert_eq!(Clock::at(t, -4 * 3600).midnight(), parse_date("2026-10-03").unwrap() * 86_400 + 4 * 3600);
    }

    #[test]
    fn the_local_clock_agrees_with_its_offset() {
        let t = 1_800_000_000;
        let c = Clock::local(t);
        assert_eq!(c, Clock::at(t, local_offset(t)));
        assert!(c.midnight() <= t && t - c.midnight() < 86_400);
        // A real zone: within UTC-12 and UTC+14, on a quarter hour.
        let o = local_offset(t);
        assert!((-12 * 3600..=14 * 3600).contains(&o) && o % 900 == 0, "{o}");
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
