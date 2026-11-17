//! Claude Code: `claude -p /usage --output-format json --no-session-persistence`
//! (decision 12). It uses no turn, no token and writes no transcript. The
//! windows are text lines in `result`:
//!
//! ```text
//! Current week (all models): 5% used · resets Nov 23 at 9am (America/Chicago)
//! Current week (Fable): 0% used · resets Nov 23 at 9am (America/Chicago)
//! ```
//!
//! "N% used" is percent **spent**. A changed format is a failed read, never a
//! number: no line that matches, or a matched line that does not parse in
//! full.
use jiff::{civil, tz::TimeZone, Timestamp, ToSpan};
use serde_json::Value;

use super::{
    run::{json_output, Program},
    window::{minutes_of, used, window},
    Read,
};

const LABEL: &str = "claude -p /usage";

pub async fn read(program: &Program) -> Result<Read, String> {
    let out = json_output(program, &["-p", "/usage", "--output-format", "json", "--no-session-persistence"], &[], LABEL).await?;
    parse(&out, now_ms())
}

fn now_ms() -> u64 {
    u64::try_from(Timestamp::now().as_millisecond()).unwrap_or(0)
}

/// The windows of the command's JSON, read at `now` (unix ms).
pub fn parse(out: &Value, now: u64) -> Result<Read, String> {
    let result = out.get("result").and_then(Value::as_str).ok_or_else(|| format!("{LABEL} output has no result"))?;
    if out.get("is_error").and_then(Value::as_bool) == Some(true) {
        let first = result.lines().find(|l| !l.trim().is_empty()).unwrap_or("an error");
        return Err(format!("{LABEL} failed: {first}"));
    }
    let mut windows = Vec::new();
    for line in result.lines().map(str::trim) {
        if !line.starts_with("Current ") {
            // The tips after the window lines are Claude Code's own: skipped.
            if windows.is_empty() {
                continue;
            }
            break;
        }
        windows.push(parse_line(line, now).ok_or_else(|| format!("{LABEL} line does not parse: {line}"))?);
    }
    if windows.is_empty() {
        return Err(format!("{LABEL} output has no usage line."));
    }
    // Claude Code gives no plan name (decision 15).
    Ok(Read { plan: None, windows, credits: None })
}

/// `Current <period> (<scope>): <n>% used · resets <date> at <time> (<zone>)`.
/// "all models" is the main window; another scope is its group.
fn parse_line(line: &str, now: u64) -> Option<skiff_host_api::UsageWindow> {
    let rest = line.strip_prefix("Current ")?;
    let (head, rest) = rest.split_once(": ")?;
    let (period, scope) = match head.split_once(" (") {
        Some((period, scope)) => (period, Some(scope.strip_suffix(')')?)),
        None => (head, None),
    };
    let minutes = minutes_of(period)?;
    let group = scope.filter(|s| !s.eq_ignore_ascii_case("all models")).map(str::to_owned);
    let (percent, rest) = rest.split_once("% used")?;
    let percent: f64 = percent.trim().parse().ok()?;
    let used = used(percent, false)?;
    let rest = rest.trim();
    let resets_at = if rest.is_empty() {
        None
    } else {
        let rest = rest.strip_prefix('·')?.trim().strip_prefix("resets ")?;
        let (when, zone) = rest.strip_suffix(')')?.rsplit_once(" (")?;
        Some(reset_time(when, zone, now)?)
    };
    Some(window(group, minutes, used, resets_at))
}

/// "Nov 23 at 9am", "Nov 23 at 8:59am" or "Nov 23, 8:59am" in `zone`, with no year: the year
/// that puts it in the next 8 days. "9am" alone is its next time after now.
fn reset_time(when: &str, zone: &str, now: u64) -> Option<u64> {
    let tz = TimeZone::get(zone.trim()).ok()?;
    let now_ts = Timestamp::from_millisecond(i64::try_from(now).ok()?).ok()?;
    let today = now_ts.to_zoned(tz.clone());
    let (date, time) = match when.split_once(" at ").or_else(|| when.split_once(", ")) {
        Some((date, time)) => (Some(date), time),
        None => (None, when),
    };
    let time = clock(time)?;
    let window = |ms: i64| ms >= now as i64 - 86_400_000 && ms <= now as i64 + 8 * 86_400_000;
    let candidates: Vec<civil::Date> = match date {
        Some(date) => {
            let (month, day) = month_day(date)?;
            (-1..=1).filter_map(|dy| civil::Date::new(today.year() + dy, month, day).ok()).collect()
        }
        None => (0..=1).filter_map(|dd: i64| today.date().checked_add(dd.days()).ok()).collect(),
    };
    candidates.into_iter().find_map(|d| {
        let ms = d.to_datetime(time).to_zoned(tz.clone()).ok()?.timestamp().as_millisecond();
        (window(ms) && (date.is_some() || ms >= now as i64)).then(|| u64::try_from(ms).ok()).flatten()
    })
}

/// "Nov 23" as month and day.
fn month_day(text: &str) -> Option<(i8, i8)> {
    let (month, day) = text.trim().split_once(' ')?;
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let m = month.to_ascii_lowercase();
    let index = MONTHS.iter().position(|p| m.len() >= 3 && p.starts_with(&m[..3]) && (m.len() == 3 || is_month_name(&m)))?;
    let day: i8 = day.trim().parse().ok().filter(|d| (1..=31).contains(d))?;
    Some((index as i8 + 1, day))
}

fn is_month_name(m: &str) -> bool {
    ["january", "february", "march", "april", "may", "june", "july", "august", "september", "sept", "october", "november", "december"].contains(&m)
}

/// "9am", "8:59am", "12pm", "9:00 AM" or "21:00".
fn clock(text: &str) -> Option<civil::Time> {
    let t = text.trim().to_ascii_lowercase().replace(' ', "");
    let (digits, half) = match (t.strip_suffix("am"), t.strip_suffix("pm")) {
        (Some(d), _) => (d, Some(false)),
        (_, Some(d)) => (d, Some(true)),
        _ => (t.as_str(), None),
    };
    let (h, m) = match digits.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h.parse::<i8>().ok()?, m.parse::<i8>().ok()?),
        Some(_) => return None,
        None => (digits.parse::<i8>().ok()?, 0),
    };
    let h = match half {
        Some(pm) if (1..=12).contains(&h) => h % 12 + if pm { 12 } else { 0 },
        Some(_) => return None,
        None if (0..24).contains(&h) => h,
        None => return None,
    };
    civil::Time::new(h, m, 0, 0).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::window::WEEK;
    use serde_json::json;

    /// 2026-11-17 05:00 in Chicago, when the fixtures were read.
    fn now() -> u64 {
        let t: Timestamp = "2026-11-17T11:00:00Z".parse().unwrap();
        t.as_millisecond() as u64
    }

    fn at(text: &str) -> u64 {
        let z: jiff::Zoned = text.parse().unwrap();
        z.timestamp().as_millisecond() as u64
    }

    fn fixture(name: &str) -> Value {
        match name {
            "8:59am" => serde_json::from_str(include_str!("../../tests/fixtures/usage/claude-p-usage.json")).unwrap(),
            _ => serde_json::from_str(include_str!("../../tests/fixtures/usage/claude-p-usage-9am.json")).unwrap(),
        }
    }

    fn with_result(text: &str) -> Value {
        let mut v = fixture("9am");
        v["result"] = json!(text);
        v
    }

    #[test]
    fn nine_am_fixture() {
        let read = parse(&fixture("9am"), now()).unwrap();
        let shape: Vec<(Option<&str>, u32, u8)> = read.windows.iter().map(|w| (w.group.as_deref(), w.minutes, w.used)).collect();
        // "Weekly" 5% and "Fable weekly" 0%: N% used is percent spent.
        assert_eq!(shape, [(None, WEEK, 5), (Some("Fable"), WEEK, 0)]);
        // Mon 23 Nov 09:00 in Chicago, this year.
        let reset = at("2026-11-23T09:00[America/Chicago]");
        assert!(read.windows.iter().all(|w| w.resets_at == Some(reset)));
        assert_eq!(read.plan, None);
        assert_eq!(read.credits, None);
    }

    #[test]
    fn eight_fifty_nine_fixture() {
        let read = parse(&fixture("8:59am"), now()).unwrap();
        assert_eq!(read.windows.len(), 2);
        assert_eq!((read.windows[0].used, read.windows[1].used), (5, 0));
        let reset = at("2026-11-23T08:59[America/Chicago]");
        assert!(read.windows.iter().all(|w| w.resets_at == Some(reset)));
    }

    #[test]
    fn the_year_puts_the_reset_in_the_next_8_days() {
        let dec: Timestamp = "2026-12-29T12:00:00Z".parse().unwrap();
        let read = parse(&with_result("Current week (all models): 40% used · resets Jan 2 at 9am (America/Chicago)"), dec.as_millisecond() as u64).unwrap();
        assert_eq!(read.windows[0].resets_at, Some(at("2027-01-02T09:00[America/Chicago]")));
    }

    #[test]
    fn a_changed_format_is_a_failed_read() {
        // No window line.
        let e = parse(&with_result("You are currently using your subscription\n\nWhat's contributing to your usage?"), now()).unwrap_err();
        assert_eq!(e, "claude -p /usage output has no usage line.");
        // A changed line.
        assert!(parse(&with_result("5% of weekly limit used"), now()).is_err());
        // A window line that does not parse in full.
        for line in [
            "Current week (all models): five% used · resets Nov 23 at 9am (America/Chicago)",
            "Current week (all models): 5% used · resets Nov 23 at 9am (Mars/Olympus)",
            "Current week (all models): 5% used · resets Nov 23 at 25am (America/Chicago)",
            "Current week (all models): 5% used · resets Smarch 23 at 9am (America/Chicago)",
            "Current fortnight (all models): 5% used · resets Nov 23 at 9am (America/Chicago)",
            "Current week (all models): 5% used, resets Nov 23 at 9am (America/Chicago)",
            // Far from the next 8 days in any year.
            "Current week (all models): 5% used · resets Jun 1 at 9am (America/Chicago)",
        ] {
            assert!(parse(&with_result(line), now()).is_err(), "{line}");
        }
        let mut err = fixture("9am");
        err["is_error"] = json!(true);
        assert!(parse(&err, now()).is_err());
    }

    #[test]
    fn a_comma_between_date_and_time() {
        let read = parse(&with_result("Current week (all models): 9% used · resets Nov 23, 8:59am (America/Chicago)"), now()).unwrap();
        let w = &read.windows[0];
        assert_eq!((w.group.as_deref(), w.minutes, w.used), (None, WEEK, 9));
        assert_eq!(w.resets_at, Some(at("2026-11-23T08:59[America/Chicago]")));
    }

    #[test]
    fn a_window_with_no_reset_time() {
        let read = parse(&with_result("Current week (all models): 100% used"), now()).unwrap();
        assert_eq!((read.windows[0].used, read.windows[0].resets_at), (100, None));
    }

    #[test]
    fn clock_forms() {
        assert_eq!(clock("9am"), civil::Time::new(9, 0, 0, 0).ok());
        assert_eq!(clock("8:59am"), civil::Time::new(8, 59, 0, 0).ok());
        assert_eq!(clock("12am"), civil::Time::new(0, 0, 0, 0).ok());
        assert_eq!(clock("12pm"), civil::Time::new(12, 0, 0, 0).ok());
        assert_eq!(clock("9:00 PM"), civil::Time::new(21, 0, 0, 0).ok());
        assert_eq!(clock("13pm"), None);
        assert_eq!(clock("9:5am"), None);
    }
}
