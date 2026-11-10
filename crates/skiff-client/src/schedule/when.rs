//! The When sentence: the shapes a schedule takes, their cron line and their
//! words. A cron line no shape can show is `Custom`. The app's edit page
//! (`settings/schedules.ts`) reads and writes the same shapes.

use super::cron::{Cron, WEEK};

/// All seven weekdays, as a bit set on cron numbers (bit 0 is Sunday).
pub const ALL_DAYS: u8 = 0x7f;
/// Monday to Friday.
pub const MON_FRI: u8 = 0b0011_1110;
/// The interval values, in minutes: they divide the hour or the day evenly.
pub const INTERVALS: [u32; 12] = [5, 10, 15, 20, 30, 60, 120, 180, 240, 360, 480, 720];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum When {
    Day { at: (u8, u8) },
    Weekday { at: (u8, u8) },
    Week { days: u8, at: (u8, u8) },
    /// Day 1 to 28, so that each month has it.
    Month { day: u8, at: (u8, u8) },
    /// Every `minutes`, from hour `from` to hour `to` (no run starts at `to`),
    /// on `days`. No window: all day.
    Interval { minutes: u32, window: Option<(u8, u8)>, days: u8 },
    Custom(String),
}

fn one(text: &str, hi: u8) -> Option<u8> {
    text.parse::<u8>().ok().filter(|n| *n <= hi)
}

/// A weekday field as a bit set: `*`, numbers, ranges and lists. `None`
/// for anything else, such as a step.
fn days_of(text: &str) -> Option<u8> {
    if text == "*" {
        return Some(ALL_DAYS);
    }
    let mut set = 0u8;
    for part in text.split(',') {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (one(a, 7)?, one(b, 7)?),
            None => (one(part, 7)?, one(part, 7)?),
        };
        if a > b {
            return None;
        }
        for d in a..=b {
            set |= 1 << (d % 7);
        }
    }
    Some(set)
}

fn days_field(days: u8) -> String {
    match days {
        ALL_DAYS => "*".into(),
        MON_FRI => "1-5".into(),
        d => WEEK.iter().filter(|(n, ..)| d & (1 << n) != 0).map(|(n, ..)| n.to_string()).collect::<Vec<_>>().join(","),
    }
}

/// `Mon–Fri`, `Monday`, `Mon, Thu`, or empty for every day.
fn days_words(days: u8, long_one: bool) -> String {
    match days {
        ALL_DAYS => String::new(),
        MON_FRI => "Mon–Fri".into(),
        d => {
            let names: Vec<&(u8, &str, &str)> = WEEK.iter().filter(|(n, ..)| d & (1 << n) != 0).collect();
            match names.as_slice() {
                [(_, _, long)] if long_one => long.to_string(),
                list => list.iter().map(|(_, short, _)| *short).collect::<Vec<_>>().join(", "),
            }
        }
    }
}

pub fn hhmm((h, m): (u8, u8)) -> String {
    format!("{h:02}:{m:02}")
}

impl When {
    /// The shape `cron` shows, else `Custom`.
    pub fn from_cron(cron: &str) -> When {
        let custom = || When::Custom(cron.trim().to_string());
        let f: Vec<&str> = cron.split_whitespace().collect();
        if f.len() != 5 || f[3] != "*" || Cron::parse(cron).is_err() {
            return custom();
        }
        let at = one(f[1], 23).zip(one(f[0], 59));
        if let Some(at) = at {
            if f[2] == "*" {
                return match days_of(f[4]) {
                    Some(ALL_DAYS) => When::Day { at },
                    Some(MON_FRI) => When::Weekday { at },
                    Some(days) if days != 0 => When::Week { days, at },
                    _ => custom(),
                };
            }
            return match one(f[2], 28).filter(|d| *d >= 1) {
                Some(day) if f[4] == "*" => When::Month { day, at },
                _ => custom(),
            };
        }
        if f[2] != "*" {
            return custom();
        }
        let Some(days) = days_of(f[4]).filter(|d| *d != 0) else { return custom() };
        // Hours: `* `, `A-B`, `*/N` or `A-B/N`. The window is A to B + 1.
        let hours = |text: &str| -> Option<(u32, Option<(u8, u8)>)> {
            let (range, step) = match text.split_once('/') {
                Some((r, s)) => (r, s.parse::<u32>().ok()?),
                None => (text, 1),
            };
            let window = match range {
                "*" => None,
                r => {
                    let (a, b) = r.split_once('-')?;
                    let (a, b) = (one(a, 23)?, one(b, 23)?);
                    (a <= b).then_some((a, b + 1))?.into()
                }
            };
            Some((step, window))
        };
        if let Some(step) = f[0].strip_prefix("*/").and_then(|s| s.parse::<u32>().ok()) {
            let (hstep, window) = hours(f[1]).unwrap_or((0, None));
            if [5, 10, 15, 20, 30].contains(&step) && hstep == 1 {
                return When::Interval { minutes: step, window, days };
            }
            return custom();
        }
        if f[0] == "0" {
            if let Some((step, window)) = hours(f[1]) {
                let minutes = step * 60;
                if INTERVALS.contains(&minutes) {
                    return When::Interval { minutes, window, days };
                }
            }
        }
        custom()
    }

    pub fn cron(&self) -> String {
        match self {
            When::Day { at } => format!("{} {} * * *", at.1, at.0),
            When::Weekday { at } => format!("{} {} * * 1-5", at.1, at.0),
            When::Week { days, at } => format!("{} {} * * {}", at.1, at.0, days_field(*days)),
            When::Month { day, at } => format!("{} {} {day} * *", at.1, at.0),
            When::Interval { minutes, window, days } => {
                let hours = match window {
                    Some((a, b)) if b - a > 1 => format!("{a}-{}", b - 1),
                    Some((a, _)) => a.to_string(),
                    None => "*".into(),
                };
                let d = days_field(*days);
                if *minutes < 60 {
                    format!("*/{minutes} {hours} * * {d}")
                } else {
                    let step = minutes / 60;
                    match (step, window) {
                        (1, _) => format!("0 {hours} * * {d}"),
                        (_, None) => format!("0 */{step} * * {d}"),
                        (_, Some(_)) => format!("0 {hours}/{step} * * {d}"),
                    }
                }
            }
            When::Custom(c) => c.clone(),
        }
    }

    /// The When sentence in words, as `skiff schedule ls` and the app show it.
    pub fn words(&self) -> String {
        match self {
            When::Day { at } => format!("Every day at {}", hhmm(*at)),
            When::Weekday { at } => format!("Weekdays at {}", hhmm(*at)),
            When::Week { days, at } => format!("Every {} at {}", days_words(*days, true), hhmm(*at)),
            When::Month { day, at } => format!("Every month on day {day} at {}", hhmm(*at)),
            When::Interval { minutes, window, days } => {
                let mut s = if *minutes < 60 { format!("Every {minutes} min") } else { format!("Every {} h", minutes / 60) };
                if let Some((a, b)) = window {
                    s += &format!(", {}–{}", hhmm((*a, 0)), hhmm((*b % 24, 0)));
                }
                let d = days_words(*days, false);
                if !d.is_empty() {
                    s += &format!(", {d}");
                }
                s
            }
            When::Custom(c) => custom_words(c).unwrap_or_else(|| c.clone()),
        }
    }

    pub fn is_custom(&self) -> bool {
        matches!(self, When::Custom(_))
    }
}

/// `09:00 on day 1 and 15` for a fixed time on listed days of the month.
fn custom_words(cron: &str) -> Option<String> {
    let f: Vec<&str> = cron.split_whitespace().collect();
    if f.len() != 5 || f[3] != "*" || f[4] != "*" {
        return None;
    }
    let at = (one(f[1], 23)?, one(f[0], 59)?);
    let days: Vec<u8> = f[2].split(',').map(|d| one(d, 31).filter(|d| *d >= 1)).collect::<Option<_>>()?;
    let list = match days.as_slice() {
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.iter().map(u8::to_string).collect::<Vec<_>>().join(", ")),
        [] => return None,
    };
    Some(format!("{} on day {list}", hhmm(at)))
}

/// `HH:MM`, 00:00 to 23:59.
pub fn parse_time(text: &str) -> Result<(u8, u8), String> {
    let bad = || format!("Time is HH:MM, such as 08:00, not {text}.");
    let (h, m) = text.trim().split_once(':').ok_or_else(bad)?;
    if m.len() != 2 || h.is_empty() || h.len() > 2 {
        return Err(bad());
    }
    let h = one(h, 23).ok_or_else(bad)?;
    let m = one(m, 59).ok_or_else(bad)?;
    Ok((h, m))
}

/// `mon,thu`, `mon-fri`, `sat-sun` or `every-day`.
pub fn parse_days(text: &str) -> Result<u8, String> {
    let bad = || format!("Days are names such as mon,thu or mon-fri, not {text}.");
    let t = text.trim().to_ascii_lowercase();
    if matches!(t.as_str(), "every-day" | "everyday" | "all" | "mon-sun") {
        return Ok(ALL_DAYS);
    }
    let index = |name: &str| WEEK.iter().position(|(_, short, _)| short.to_ascii_lowercase() == name.get(..3).unwrap_or(name)).ok_or_else(bad);
    let mut set = 0u8;
    for part in t.split(',') {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (index(a)?, index(b)?),
            None => (index(part)?, index(part)?),
        };
        if a > b {
            return Err(bad());
        }
        for i in a..=b {
            set |= 1 << WEEK[i].0;
        }
    }
    Ok(set)
}

/// The words flags of `skiff schedule add` and `edit`.
#[derive(Debug, Default, Clone)]
pub struct Words {
    pub every: Option<String>,
    pub at: Option<String>,
    pub on: Option<String>,
    pub day: Option<u8>,
    pub between: Option<String>,
}

impl Words {
    pub fn is_empty(&self) -> bool {
        self.every.is_none() && self.at.is_none() && self.on.is_none() && self.day.is_none() && self.between.is_none()
    }
}

/// The shape the flags give. Each error is one line that names the fault.
pub fn from_words(w: &Words) -> Result<When, String> {
    let every = w.every.as_deref().ok_or("--at, --on, --day and --between go with --every.")?.to_ascii_lowercase();
    let at = || w.at.as_deref().map(parse_time).unwrap_or(Err(format!("--every {every} needs --at HH:MM.")));
    let no = |flag: &str, given: bool| if given { Err(format!("--every {every} does not take {flag}.")) } else { Ok(()) };
    match every.as_str() {
        "day" | "weekday" => {
            no("--on", w.on.is_some())?;
            no("--day", w.day.is_some())?;
            no("--between", w.between.is_some())?;
            let at = at()?;
            Ok(if every == "day" { When::Day { at } } else { When::Weekday { at } })
        }
        "week" => {
            no("--day", w.day.is_some())?;
            no("--between", w.between.is_some())?;
            let days = parse_days(w.on.as_deref().ok_or("--every week needs --on, such as --on mon,thu.")?)?;
            let at = at()?;
            Ok(match days {
                ALL_DAYS => When::Day { at },
                MON_FRI => When::Weekday { at },
                days => When::Week { days, at },
            })
        }
        "month" => {
            no("--on", w.on.is_some())?;
            no("--between", w.between.is_some())?;
            let day = w.day.ok_or("--every month needs --day, 1 to 28.")?;
            if !(1..=28).contains(&day) {
                return Err("--day is 1 to 28, so that each month has the day.".into());
            }
            Ok(When::Month { day, at: at()? })
        }
        n => {
            let minutes = interval(n).ok_or_else(|| format!("--every is day, weekday, week, month, 5m, 10m, 15m, 20m, 30m, 1h, 2h, 3h, 4h, 6h, 8h or 12h, not {n}."))?;
            no("--at", w.at.is_some())?;
            no("--day", w.day.is_some())?;
            let days = w.on.as_deref().map(parse_days).transpose()?.unwrap_or(ALL_DAYS);
            let window = match w.between.as_deref() {
                None => None,
                Some(b) => {
                    let bad = || format!("--between is two whole hours, such as 09:00-18:00, not {b}.");
                    let (a, z) = b.split_once('-').ok_or_else(bad)?;
                    let (a, z) = (parse_time(a).map_err(|_| bad())?, parse_time(z).or_else(|e| if z.trim() == "24:00" { Ok((24, 0)) } else { Err(e) }).map_err(|_| bad())?);
                    if a.1 != 0 || z.1 != 0 || a.0 >= z.0 {
                        return Err(bad());
                    }
                    Some((a.0, z.0))
                }
            };
            Ok(When::Interval { minutes, window, days })
        }
    }
}

/// `30m`, `30min`, `2h` in minutes, when it is an interval value.
fn interval(text: &str) -> Option<u32> {
    let (n, unit) = text.find(|c: char| !c.is_ascii_digit()).map(|i| text.split_at(i))?;
    let n: u32 = n.parse().ok()?;
    let minutes = match unit.trim() {
        "m" | "min" | "mins" => n,
        "h" | "hour" | "hours" => n * 60,
        _ => return None,
    };
    INTERVALS.contains(&minutes).then_some(minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_shape_round_trips_through_its_cron() {
        let table = [
            ("30 1 * * *", "Every day at 01:30"),
            ("0 8 * * 1-5", "Weekdays at 08:00"),
            ("0 9 * * 1,4", "Every Mon, Thu at 09:00"),
            ("0 9 * * 1", "Every Monday at 09:00"),
            ("0 6 1 * *", "Every month on day 1 at 06:00"),
            ("*/30 9-17 * * 1-5", "Every 30 min, 09:00–18:00, Mon–Fri"),
            ("0 */2 * * *", "Every 2 h"),
            ("0 9-17/2 * * *", "Every 2 h, 09:00–18:00"),
            ("0 9 1,15 * *", "09:00 on day 1 and 15"),
        ];
        for (cron, words) in table {
            let w = When::from_cron(cron);
            assert_eq!(w.words(), words, "{cron}");
            assert_eq!(w.cron(), cron, "{cron}");
        }
        assert!(When::from_cron("0 9 1,15 * *").is_custom());
    }

    #[test]
    fn the_flags_give_the_cron() {
        let w = |every: &str, at: Option<&str>, on: Option<&str>, day: Option<u8>, between: Option<&str>| {
            from_words(&Words { every: Some(every.into()), at: at.map(Into::into), on: on.map(Into::into), day, between: between.map(Into::into) }).map(|w| w.cron())
        };
        assert_eq!(w("day", Some("02:00"), None, None, None).unwrap(), "0 2 * * *");
        assert_eq!(w("week", Some("09:00"), Some("mon,thu"), None, None).unwrap(), "0 9 * * 1,4");
        assert_eq!(w("week", Some("09:00"), Some("mon-fri"), None, None).unwrap(), "0 9 * * 1-5");
        assert_eq!(w("month", Some("06:00"), None, Some(1), None).unwrap(), "0 6 1 * *");
        assert_eq!(w("30m", None, Some("mon-fri"), None, Some("09:00-18:00")).unwrap(), "*/30 9-17 * * 1-5");
        assert_eq!(w("2h", None, None, None, None).unwrap(), "0 */2 * * *");
        assert!(w("7m", None, None, None, None).is_err());
        assert!(w("month", Some("06:00"), None, Some(30), None).is_err());
        assert!(w("day", Some("25:00"), None, None, None).is_err());
    }
}
