//! Five-field cron in the Host's local time: parse, the next runs, and the
//! words of the When sentence. `@hourly`, `@daily`, `@weekly` and `@monthly`
//! are also taken.

/// A parsed cron line. Each field is a bit set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cron {
    minutes: u64,
    hours: u32,
    /// Bit 1 to 31.
    days: u32,
    /// Bit 1 to 12.
    months: u16,
    /// Bit 0 (Sunday) to 6.
    weekdays: u8,
    /// The day of month field is `*`: only the weekday decides.
    any_day: bool,
    /// The weekday field is `*`: only the day of month decides.
    any_weekday: bool,
}

/// The weekdays, Monday first, with their cron number.
pub const WEEK: [(u8, &str, &str); 7] = [
    (1, "Mon", "Monday"),
    (2, "Tue", "Tuesday"),
    (3, "Wed", "Wednesday"),
    (4, "Thu", "Thursday"),
    (5, "Fri", "Friday"),
    (6, "Sat", "Saturday"),
    (0, "Sun", "Sunday"),
];

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

impl Cron {
    /// Reads `text`. The error is one line for the person.
    pub fn parse(text: &str) -> Result<Cron, String> {
        let text = text.trim();
        let line = match text {
            "@hourly" => "0 * * * *",
            "@daily" | "@midnight" => "0 0 * * *",
            "@weekly" => "0 0 * * 0",
            "@monthly" => "0 0 1 * *",
            t if t.starts_with('@') => return Err(format!("Cron does not take {t}.")),
            t => t,
        };
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() != 5 {
            return Err("Cron needs 5 fields.".into());
        }
        let minutes = field(f[0], 0, 59, "Minute")?;
        let hours = field(f[1], 0, 23, "Hour")? as u32;
        let days = field(f[2], 1, 31, "Day of month")? as u32;
        let months = field(f[3], 1, 12, "Month")? as u16;
        let mut w = field(f[4], 0, 7, "Weekday")?;
        // 7 is Sunday too.
        if w & (1 << 7) != 0 {
            w = (w | 1) & 0x7f;
        }
        Ok(Cron { minutes, hours, days, months, weekdays: w as u8, any_day: f[2] == "*", any_weekday: f[4] == "*" })
    }

    fn day_matches(&self, mday: i32, wday: i32) -> bool {
        let d = self.days & (1 << mday) != 0;
        let w = self.weekdays & (1 << wday) != 0;
        match (self.any_day, self.any_weekday) {
            (true, true) => true,
            (true, false) => w,
            (false, true) => d,
            // Both set: either one, as cron has it.
            (false, false) => d || w,
        }
    }

    /// The first run strictly after `after` (Unix seconds), in local time.
    /// `None`: no run within about 5 years, such as `0 0 31 2 *`.
    pub fn next_after(&self, after: i64) -> Option<i64> {
        // The next whole minute.
        let mut t = (after.div_euclid(60) + 1) * 60;
        for _ in 0..200_000 {
            let tm = local(t)?;
            if self.months & (1 << (tm.tm_mon + 1)) == 0 {
                t = make(tm.tm_year, tm.tm_mon + 1, 1, 0, 0)?;
                continue;
            }
            if !self.day_matches(tm.tm_mday, tm.tm_wday) {
                t = make(tm.tm_year, tm.tm_mon, tm.tm_mday + 1, 0, 0)?;
                continue;
            }
            if self.hours & (1 << tm.tm_hour) == 0 {
                t = make(tm.tm_year, tm.tm_mon, tm.tm_mday, tm.tm_hour + 1, 0)?;
                continue;
            }
            if self.minutes & (1 << tm.tm_min) == 0 {
                t += 60;
                continue;
            }
            return Some(t);
        }
        None
    }

    /// The next `n` runs after `after`.
    pub fn next_runs(&self, after: i64, n: usize) -> Vec<i64> {
        let mut out = Vec::new();
        let mut at = after;
        while out.len() < n {
            let Some(t) = self.next_after(at) else { break };
            out.push(t);
            at = t;
        }
        out
    }
}

/// One field: `*`, `N`, `A-B`, a step `/S` on either, and lists of them.
fn field(text: &str, lo: u32, hi: u32, name: &str) -> Result<u64, String> {
    let bad = || format!("{name} cannot be {text}.");
    let mut set = 0u64;
    for part in text.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u32>().ok().filter(|s| *s > 0).ok_or_else(bad)?),
            None => (part, 1),
        };
        let (a, b) = if range == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            (num(a, lo, hi).ok_or_else(bad)?, num(b, lo, hi).ok_or_else(bad)?)
        } else {
            let a = num(range, lo, hi).ok_or_else(bad)?;
            // `N/S` runs from N to the top.
            (a, if part.contains('/') { hi } else { a })
        };
        if a > b {
            return Err(bad());
        }
        let mut v = a;
        while v <= b {
            set |= 1 << v;
            v += step;
        }
    }
    Ok(set)
}

fn num(text: &str, lo: u32, hi: u32) -> Option<u32> {
    text.parse::<u32>().ok().filter(|n| (lo..=hi).contains(n))
}

fn local(t: i64) -> Option<libc::tm> {
    let tt = t as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the call.
    let ok = unsafe { libc::localtime_r(&tt, &mut tm) };
    (!ok.is_null()).then_some(tm)
}

/// Local time to Unix seconds. Out-of-range fields roll over, as `mktime`
/// does: day 32 is the next month's first day.
fn make(year: i32, mon: i32, mday: i32, hour: i32, min: i32) -> Option<i64> {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = year;
    tm.tm_mon = mon;
    tm.tm_mday = mday;
    tm.tm_hour = hour;
    tm.tm_min = min;
    tm.tm_isdst = -1;
    // SAFETY: `tm` is a valid, owned struct.
    let t = unsafe { libc::mktime(&mut tm) };
    (t != -1).then_some(t as i64)
}

/// `Nov 11 01:30` in local time.
pub fn short_time(t: i64) -> String {
    match local(t) {
        Some(tm) => format!("{} {} {:02}:{:02}", MONTHS[tm.tm_mon as usize], tm.tm_mday, tm.tm_hour, tm.tm_min),
        None => String::new(),
    }
}

/// `2026-11-11T01:30:00Z`: UTC, as the ledger stores times.
pub fn utc(t: i64) -> String {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// Unix seconds from [`utc`]'s form. `None` for any other text.
pub fn parse_utc(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[10] != b'T' || b[19] != b'Z' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, mo: i32, d: i32, h: i32, mi: i32) -> i64 {
        make(y - 1900, mo - 1, d, h, mi).unwrap()
    }

    #[test]
    fn the_next_run_follows_each_field() {
        let c = Cron::parse("30 1 * * *").unwrap();
        assert_eq!(c.next_after(at(2026, 11, 10, 12, 0)), Some(at(2026, 11, 11, 1, 30)));
        // Exactly at a run time: the next one, not this one.
        assert_eq!(c.next_after(at(2026, 11, 11, 1, 30)), Some(at(2026, 11, 12, 1, 30)));
        let wd = Cron::parse("0 8 * * 1-5").unwrap();
        // Friday 2026-11-13 09:00 -> Monday 16th 08:00.
        assert_eq!(wd.next_after(at(2026, 11, 13, 9, 0)), Some(at(2026, 11, 16, 8, 0)));
        let m = Cron::parse("0 6 1 * *").unwrap();
        assert_eq!(m.next_after(at(2026, 11, 10, 0, 0)), Some(at(2026, 12, 1, 6, 0)));
        assert_eq!(m.next_after(at(2026, 12, 1, 6, 0)), Some(at(2027, 1, 1, 6, 0)));
        let win = Cron::parse("*/30 9-17 * * 1-5").unwrap();
        assert_eq!(win.next_runs(at(2026, 11, 10, 17, 10), 2), vec![at(2026, 11, 10, 17, 30), at(2026, 11, 11, 9, 0)]);
        // Day of month and weekday both set: either one.
        let either = Cron::parse("0 9 1 * 1").unwrap();
        assert_eq!(either.next_after(at(2026, 11, 10, 10, 0)), Some(at(2026, 11, 16, 9, 0)));
        assert_eq!(Cron::parse("0 0 31 2 *").unwrap().next_after(at(2026, 1, 1, 0, 0)), None);
    }

    #[test]
    fn a_bad_line_says_what_is_wrong() {
        assert_eq!(Cron::parse("0 9 1,15 *").unwrap_err(), "Cron needs 5 fields.");
        assert_eq!(Cron::parse("61 * * * *").unwrap_err(), "Minute cannot be 61.");
        assert!(Cron::parse("@daily").is_ok());
        assert_eq!(Cron::parse("0 0 * * 7").unwrap(), Cron::parse("0 0 * * 0").unwrap());
    }

    #[test]
    fn utc_round_trips() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        let t = 1_794_355_200 + 3_723;
        assert_eq!(parse_utc(&utc(t)), Some(t));
    }
}
