//! Codex: `codex app-server`, `initialize`, `initialized`, then
//! `account/rateLimits/read`. `usedPercent` is percent **spent**: 2 is 2%
//! used, which Codex's own screen shows as "98% left".
use serde_json::{json, Value};
use skiff_host_api::UsageCredits;

use super::{
    run::{with_rpc, Program},
    window::{used, window},
    Read,
};

const LABEL: &str = "codex app-server";

pub async fn read(program: &Program) -> Result<Read, String> {
    let result = with_rpc(program, &["app-server"], LABEL, async |rpc| {
        rpc.call("initialize", json!({ "clientInfo": { "name": "skiff", "title": "Skiff", "version": skiff_core::VERSION } })).await?;
        rpc.notify("initialized").await?;
        rpc.call("account/rateLimits/read", Value::Null).await
    })
    .await?;
    parse(&result)
}

/// The windows of an `account/rateLimits/read` result.
pub fn parse(result: &Value) -> Result<Read, String> {
    let top = result.get("rateLimits").filter(|v| v.is_object());
    let by_id = result.get("rateLimitsByLimitId").and_then(Value::as_object);
    // The `codex` limit first, then the others: a per-model limit comes as
    // its own entry with its own name.
    let mut entries: Vec<(&str, &Value)> = match by_id {
        Some(map) if !map.is_empty() => map.iter().map(|(k, v)| (k.as_str(), v)).collect(),
        _ => top.map(|t| vec![(t.get("limitId").and_then(Value::as_str).unwrap_or("codex"), t)]).unwrap_or_default(),
    };
    if entries.is_empty() {
        return Err(format!("{LABEL} gave no rate limits"));
    }
    entries.sort_by_key(|(id, _)| *id != "codex");
    let main_id = entries[0].0;
    // Codex says the ordinary limit is used up.
    let blocked = result.get("ordinaryUsageAllowed").and_then(Value::as_bool) == Some(false);
    let mut windows = Vec::new();
    for (id, entry) in entries {
        let group = ["limitName", "normalModelSlug"].iter().find_map(|k| entry.get(*k).and_then(Value::as_str)).map(str::to_owned);
        let reached = !entry.get("rateLimitReachedType").is_none_or(Value::is_null) || (blocked && id == main_id);
        let start = windows.len();
        for key in ["primary", "secondary"] {
            let Some(w) = entry.get(key).filter(|w| w.is_object()) else { continue };
            let percent = w.get("usedPercent").and_then(Value::as_f64).ok_or_else(|| format!("{LABEL}: {key} has no usedPercent"))?;
            let minutes = w
                .get("windowDurationMins")
                .and_then(Value::as_u64)
                .and_then(|m| u32::try_from(m).ok())
                .filter(|m| *m > 0)
                .ok_or_else(|| format!("{LABEL}: {key} has no window length"))?;
            let resets_at = w.get("resetsAt").and_then(Value::as_u64).map(|s| s * 1000);
            let used = used(percent, false).ok_or_else(|| format!("{LABEL}: {key} usedPercent is not a number"))?;
            windows.push(window(group.clone(), minutes, used, resets_at));
        }
        // At the limit with no window at 100: the fullest window is the one.
        if reached && !windows[start..].iter().any(|w| w.used == 100) {
            if let Some(w) = windows[start..].iter_mut().max_by_key(|w| w.used) {
                w.used = 100;
            }
        }
    }
    let plan = top.and_then(|t| t.get("planType")).and_then(Value::as_str).map(plan_name);
    let credits = top.and_then(|t| t.get("credits")).and_then(credits);
    Ok(Read { plan, windows, credits })
}

/// The balance, "Unlimited", or nothing when the account has no credits.
fn credits(c: &Value) -> Option<UsageCredits> {
    if c.get("hasCredits").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    if c.get("unlimited").and_then(Value::as_bool) == Some(true) {
        return Some(UsageCredits { balance: None, unlimited: true });
    }
    let balance = match c.get("balance")? {
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        v => v.as_f64()?,
    };
    Some(UsageCredits { balance: Some(balance), unlimited: false })
}

/// Codex's plan id as its name: "prolite" is "Pro Lite".
pub fn plan_name(id: &str) -> String {
    match id {
        "prolite" => "Pro Lite".into(),
        "edu" => "Edu".into(),
        other => other
            .split(['_', '-', ' '])
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut c = w.chars();
                c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::window::{FIVE_HOURS, WEEK};

    fn fixture() -> Value {
        let v: Value = serde_json::from_str(include_str!("../../tests/fixtures/usage/codex-rateLimits.json")).unwrap();
        v["rateLimits"]["result"].clone()
    }

    #[test]
    fn used_percent_is_spent() {
        let read = parse(&fixture()).unwrap();
        // One weekly window: usedPercent 2 is 2% used, 98% left. No 5-hour window.
        assert_eq!(read.windows.len(), 1);
        let w = &read.windows[0];
        assert_eq!((w.minutes, w.used, w.group.as_deref()), (WEEK, 2, None));
        assert_eq!(100 - w.used, 98);
        assert_eq!(w.resets_at, Some(1_795_016_698_000));
        assert_eq!(w.threshold, 80);
        assert!(!read.windows.iter().any(|w| w.minutes == FIVE_HOURS));
        assert_eq!(read.plan.as_deref(), Some("Pro Lite"));
        assert_eq!(read.credits, Some(UsageCredits { balance: Some(75_000.0), unlimited: false }));
    }

    #[test]
    fn a_secondary_window_adds_a_row() {
        let mut v = fixture();
        v["rateLimitsByLimitId"]["codex"]["secondary"] = json!({ "usedPercent": 91, "windowDurationMins": 300, "resetsAt": 1_795_000_000 });
        let read = parse(&v).unwrap();
        assert_eq!(read.windows.len(), 2);
        assert_eq!(read.windows[0].minutes, WEEK);
        let five = &read.windows[1];
        assert_eq!((five.minutes, five.used, five.threshold), (FIVE_HOURS, 91, 90));
    }

    #[test]
    fn at_the_limit_shows_100() {
        let mut v = fixture();
        v["ordinaryUsageAllowed"] = json!(false);
        assert_eq!(parse(&v).unwrap().windows[0].used, 100);
        let mut v = fixture();
        v["rateLimitsByLimitId"]["codex"]["rateLimitReachedType"] = json!("rate_limit");
        assert_eq!(parse(&v).unwrap().windows[0].used, 100);
        let mut v = fixture();
        v["rateLimitsByLimitId"]["codex"]["primary"]["usedPercent"] = json!(99.6);
        assert_eq!(parse(&v).unwrap().windows[0].used, 99);
    }

    #[test]
    fn no_reset_time_and_no_windows() {
        let mut v = fixture();
        v["rateLimitsByLimitId"]["codex"]["primary"]["resetsAt"] = Value::Null;
        assert_eq!(parse(&v).unwrap().windows[0].resets_at, None);
        let mut v = fixture();
        v["rateLimitsByLimitId"]["codex"]["primary"] = Value::Null;
        assert!(parse(&v).unwrap().windows.is_empty());
    }

    #[test]
    fn credits_rows() {
        let mut v = fixture();
        v["rateLimits"]["credits"] = json!({ "hasCredits": true, "unlimited": true, "balance": null });
        assert_eq!(parse(&v).unwrap().credits, Some(UsageCredits { balance: None, unlimited: true }));
        v["rateLimits"]["credits"] = json!({ "hasCredits": false, "unlimited": false, "balance": "0" });
        assert_eq!(parse(&v).unwrap().credits, None);
    }

    #[test]
    fn plan_names() {
        assert_eq!(plan_name("prolite"), "Pro Lite");
        assert_eq!(plan_name("plus"), "Plus");
        assert_eq!(plan_name("team_plus"), "Team Plus");
    }
}
