//! The times the CLI prints, as the app shows them: in the time zone and
//! clock of Settings › Appearance (`[appearance] time_zone` and `clock` in
//! client.toml), else the system's zone and the clock of the language.
//! `--json` and every stored value stay UTC.

use std::sync::OnceLock;

use jiff::{tz::TimeZone, Timestamp, Zoned};

pub struct Clock {
    tz: TimeZone,
    hour12: bool,
}

/// The settings, read once per run.
pub fn clock() -> &'static Clock {
    static CLOCK: OnceLock<Clock> = OnceLock::new();
    CLOCK.get_or_init(|| {
        let a = skiff_core::config::client::load().map(|c| c.appearance).unwrap_or_default();
        let env = ["LC_ALL", "LC_TIME", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()));
        Clock::new(a.time_zone.as_deref(), a.clock.as_deref(), a.language.as_deref(), env.as_deref())
    })
}

impl Clock {
    /// `zone`: an IANA name; unknown or absent gives the system's. `clock`:
    /// `12`, `24`, or absent for the language's: the app's language when
    /// set, with the region of `env` (`en_GB.UTF-8`) when it speaks it.
    pub fn new(zone: Option<&str>, clock: Option<&str>, language: Option<&str>, env: Option<&str>) -> Clock {
        let tz = zone.and_then(|z| TimeZone::get(z).ok()).unwrap_or_else(TimeZone::system);
        let hour12 = match clock {
            Some("12") => true,
            Some("24") => false,
            _ => language_hour12(language, env),
        };
        Clock { tz, hour12 }
    }

    fn zoned(&self, t: Timestamp) -> Zoned {
        t.to_zoned(self.tz.clone())
    }

    fn hm(&self, z: &Zoned) -> String {
        if self.hour12 {
            let h = z.hour() % 12;
            format!("{}:{:02} {}", if h == 0 { 12 } else { h }, z.minute(), if z.hour() < 12 { "AM" } else { "PM" })
        } else {
            format!("{:02}:{:02}", z.hour(), z.minute())
        }
    }

    /// `14:05` or `2:05 PM`.
    pub fn time(&self, t: Timestamp) -> String {
        self.hm(&self.zoned(t))
    }

    /// The time of Unix ms: `14:05`. `?` when out of range.
    pub fn time_ms(&self, ms: u64) -> String {
        i64::try_from(ms).ok().and_then(|ms| Timestamp::from_millisecond(ms).ok()).map_or_else(|| "?".into(), |t| self.time(t))
    }

    /// A table cell: today `14:05`, this year `Nov 11 14:05`, else `2025-11-11 14:05`.
    pub fn stamp(&self, t: Timestamp, now: Timestamp) -> String {
        let (z, n) = (self.zoned(t), self.zoned(now));
        if z.date() == n.date() {
            self.hm(&z)
        } else if z.year() == n.year() {
            format!("{} {} {}", z.strftime("%b"), z.day(), self.hm(&z))
        } else {
            format!("{} {}", z.strftime("%Y-%m-%d"), self.hm(&z))
        }
    }

    /// [`Clock::stamp`] of a stored UTC stamp, `2026-11-11T01:30:00Z`. Other text prints as it is.
    pub fn stamp_utc(&self, text: &str) -> String {
        match text.parse::<Timestamp>() {
            Ok(t) => self.stamp(t, Timestamp::now()),
            Err(_) => text.to_string(),
        }
    }
}

/// Regions whose clock is 12-hour, as the app's Intl reads them.
const HOUR12_REGIONS: [&str; 15] = ["US", "CA", "AU", "NZ", "IN", "PH", "MX", "CO", "PK", "EG", "SA", "KR", "TW", "HK", "MY"];

/// The language's clock: `en` and the regions above are 12-hour, else 24-hour.
fn language_hour12(language: Option<&str>, env: Option<&str>) -> bool {
    let split = |s: &str| {
        let s = s.split(['.', '@']).next().unwrap_or_default();
        let mut parts = s.split(['-', '_']);
        (parts.next().unwrap_or_default().to_ascii_lowercase(), parts.next().map(str::to_ascii_uppercase))
    };
    let from_env = env.map(split);
    let (lang, region) = match language.map(split) {
        // The system's region, when the system speaks the app's language.
        Some((l, r)) => {
            let r = r.or_else(|| from_env.as_ref().filter(|(el, _)| *el == l).and_then(|(_, er)| er.clone()));
            (l, r)
        }
        None => from_env.unwrap_or_else(|| ("en".into(), None)),
    };
    match region {
        Some(r) => HOUR12_REGIONS.contains(&r.as_str()),
        None => lang == "en",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[test]
    fn zone_and_clock_shape_the_time() {
        let t = at("2026-11-17T15:05:00Z");
        assert_eq!(Clock::new(Some("Asia/Tokyo"), Some("24"), None, None).time(t), "00:05");
        assert_eq!(Clock::new(Some("America/New_York"), Some("12"), None, None).time(t), "10:05 AM");
        assert_eq!(Clock::new(Some("UTC"), Some("12"), None, None).time(at("2026-11-17T00:30:00Z")), "12:30 AM");
        // Auto: English is 12-hour, en_GB and Spanish are 24-hour.
        assert!(Clock::new(Some("UTC"), None, Some("en"), Some("en_US.UTF-8")).hour12);
        assert!(!Clock::new(Some("UTC"), None, Some("en"), Some("en_GB.UTF-8")).hour12);
        assert!(!Clock::new(Some("UTC"), None, Some("es"), Some("en_US.UTF-8")).hour12);
    }

    #[test]
    fn a_stamp_is_short_today_and_dated_otherwise_by_the_zone_day() {
        let c = Clock::new(Some("America/Panama"), Some("24"), None, None);
        let now = at("2026-11-17T20:00:00Z");
        // 03:00 UTC on Nov 18 is 22:00 on Nov 17 in Panama: still today.
        assert_eq!(c.stamp(at("2026-11-18T03:00:00Z"), now), "22:00");
        assert_eq!(c.stamp(at("2026-11-11T06:30:00Z"), now), "Nov 11 01:30");
        assert_eq!(c.stamp(at("2025-11-11T06:30:00Z"), now), "2025-11-11 01:30");
        assert_eq!(c.stamp_utc("not a time"), "not a time");
    }
}
