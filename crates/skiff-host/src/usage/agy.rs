//! Antigravity: `agy -p /usage --output-format json`. It uses no turn and no
//! token. `remaining_fraction` is the share **left**: used % is
//! `(1 - remaining_fraction) * 100`.
use serde_json::Value;

use super::{
    run::{json_output, Program},
    window::{minutes_of, rfc3339_ms, used, window},
    Read,
};

const LABEL: &str = "agy -p /usage";

pub async fn read(program: &Program) -> Result<Read, String> {
    let out = json_output(
        program,
        &["-p", "/usage", "--output-format", "json", "--print-timeout", "20s"],
        &[("AGY_CLI_DISABLE_AUTO_UPDATE", "true")],
        LABEL,
    )
    .await?;
    parse(&out)
}

/// The windows of the command's JSON: one per bucket, in each group.
pub fn parse(out: &Value) -> Result<Read, String> {
    if let Some(status) = out.get("status").and_then(Value::as_str).filter(|s| *s != "SUCCESS") {
        return Err(format!("{LABEL} answered {status}"));
    }
    let groups = out
        .pointer("/command/data/groups")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{LABEL} output has no usage groups"))?;
    let mut windows = Vec::new();
    for group in groups {
        let name = group.get("name").and_then(Value::as_str).map(str::to_owned);
        for bucket in group.get("buckets").and_then(Value::as_array).into_iter().flatten() {
            // The length from `window`, never from the bucket's name.
            let word = bucket.get("window").and_then(Value::as_str).unwrap_or("");
            let minutes = minutes_of(word).ok_or_else(|| format!("{LABEL}: window \"{word}\" has no known length"))?;
            let left = bucket
                .get("remaining_fraction")
                .and_then(Value::as_f64)
                .filter(|f| (0.0..=1.0).contains(f))
                .ok_or_else(|| format!("{LABEL}: a bucket has no remaining_fraction"))?;
            let used = used((1.0 - left) * 100.0, left <= 0.0).ok_or_else(|| format!("{LABEL}: remaining_fraction is not a number"))?;
            let resets_at = bucket.get("reset_time").and_then(Value::as_str).and_then(rfc3339_ms);
            windows.push(window(name.clone(), minutes, used, resets_at));
        }
    }
    // Antigravity gives no plan tier.
    Ok(Read { plan: None, windows, credits: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::window::{FIVE_HOURS, WEEK};
    use serde_json::json;

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/usage/agy-usage.json")).unwrap()
    }

    #[test]
    fn remaining_fraction_is_left() {
        let read = parse(&fixture()).unwrap();
        // 5 windows in 3 groups, with the vendor's group names.
        assert_eq!(read.windows.len(), 5);
        let shape: Vec<(&str, u32, u8)> = read.windows.iter().map(|w| (w.group.as_deref().unwrap(), w.minutes, w.used)).collect();
        assert_eq!(
            shape,
            [
                // remaining_fraction 0.9776 is 2% used.
                ("Gemini Models", WEEK, 2),
                // 1 is 0% used.
                ("Gemini Models", FIVE_HOURS, 0),
                ("Claude and GPT models", WEEK, 0),
                ("Claude and GPT models", FIVE_HOURS, 0),
                ("Gemini 4 Argon", WEEK, 0),
            ]
        );
        assert_eq!(read.windows[1].threshold, 90);
        assert_eq!(read.windows[0].threshold, 80);
        assert_eq!(read.windows[0].resets_at, rfc3339_ms("2026-11-24T01:50:00Z"));
        assert_eq!(read.plan, None);
    }

    #[test]
    fn zero_left_is_100_used() {
        let mut v = fixture();
        v["command"]["data"]["groups"][0]["buckets"][0]["remaining_fraction"] = json!(0);
        assert_eq!(parse(&v).unwrap().windows[0].used, 100);
        v["command"]["data"]["groups"][0]["buckets"][0]["remaining_fraction"] = json!(0.004);
        assert_eq!(parse(&v).unwrap().windows[0].used, 99);
    }

    #[test]
    fn no_windows_and_bad_shapes() {
        let mut v = fixture();
        v["command"]["data"]["groups"] = json!([]);
        assert!(parse(&v).unwrap().windows.is_empty());
        let mut v = fixture();
        v["command"]["data"]["groups"][0]["buckets"][0]["window"] = json!("fortnightly");
        assert!(parse(&v).is_err());
        let mut v = fixture();
        v["status"] = json!("ERROR");
        assert!(parse(&v).is_err());
        assert!(parse(&json!({ "status": "SUCCESS" })).is_err());
    }
}
