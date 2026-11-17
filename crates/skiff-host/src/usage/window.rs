//! The rules every reader shares: one window from a used share and a length,
//! the threshold from the length, and the rounding.
use skiff_host_api::UsageWindow;

/// Minutes in 5 hours, a day and a week.
pub const FIVE_HOURS: u32 = 300;
pub const DAY: u32 = 1440;
pub const WEEK: u32 = 10080;

/// The percent used from which a window is near its limit (decision 2):
/// 90 for a window of 5 hours or less, 80 for a longer one.
pub fn threshold(minutes: u32) -> u8 {
    if minutes <= FIVE_HOURS { 90 } else { 80 }
}

/// Percent used, rounded to the nearest whole percent. 100 only when the
/// source says the window is used up (`full`, or a value of 100 or more):
/// 99.6 is 99. A value that is not a number is refused.
pub fn used(percent: f64, full: bool) -> Option<u8> {
    if !percent.is_finite() {
        return None;
    }
    if full || percent >= 100.0 {
        return Some(100);
    }
    Some(percent.round().clamp(0.0, 99.0) as u8)
}

/// One window, `used` already percent used.
pub fn window(group: Option<String>, minutes: u32, used: u8, resets_at: Option<u64>) -> UsageWindow {
    UsageWindow { group: group.filter(|g| !g.trim().is_empty()), minutes, credits: false, used, resets_at, threshold: threshold(minutes) }
}

/// A window's length from a source's word for it: `5h`, `weekly`,
/// `seven_day`, `daily`, `90m`, `2d`. `None`: a word no rule knows.
pub fn minutes_of(word: &str) -> Option<u32> {
    let w = word.trim().to_ascii_lowercase();
    match w.as_str() {
        "weekly" | "week" | "seven_day" | "7d" => return Some(WEEK),
        "daily" | "day" | "one_day" => return Some(DAY),
        "five_hour" | "session" => return Some(FIVE_HOURS),
        "hourly" | "hour" => return Some(60),
        _ => {}
    }
    let (n, unit) = w.split_at(w.find(|c: char| !c.is_ascii_digit())?);
    let n: u32 = n.parse().ok().filter(|n| *n > 0)?;
    match unit {
        "m" | "min" | "mins" => Some(n),
        "h" | "hr" | "hour" | "hours" => n.checked_mul(60),
        "d" | "day" | "days" => n.checked_mul(DAY),
        "w" | "week" | "weeks" => n.checked_mul(WEEK),
        _ => None,
    }
}

/// Unix ms of an RFC 3339 time: "2026-11-24T01:50:00Z".
pub fn rfc3339_ms(text: &str) -> Option<u64> {
    let t: jiff::Timestamp = text.trim().parse().ok()?;
    u64::try_from(t.as_millisecond()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_from_length() {
        assert_eq!(threshold(FIVE_HOURS), 90);
        assert_eq!(threshold(60), 90);
        assert_eq!(threshold(301), 80);
        assert_eq!(threshold(DAY), 80);
        assert_eq!(threshold(WEEK), 80);
    }

    #[test]
    fn rounding_keeps_100_for_used_up() {
        assert_eq!(used(2.0, false), Some(2));
        assert_eq!(used(2.4, false), Some(2));
        assert_eq!(used(99.6, false), Some(99));
        assert_eq!(used(100.0, false), Some(100));
        assert_eq!(used(40.0, true), Some(100));
        assert_eq!(used(-1.0, false), Some(0));
        assert_eq!(used(f64::NAN, false), None);
    }

    #[test]
    fn lengths_from_words() {
        assert_eq!(minutes_of("5h"), Some(300));
        assert_eq!(minutes_of("weekly"), Some(WEEK));
        assert_eq!(minutes_of("seven_day"), Some(WEEK));
        assert_eq!(minutes_of("daily"), Some(DAY));
        assert_eq!(minutes_of("2d"), Some(2 * DAY));
        assert_eq!(minutes_of("90m"), Some(90));
        assert_eq!(minutes_of("fortnightly"), None);
        assert_eq!(minutes_of("0h"), None);
    }
}
