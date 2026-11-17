//! Grok: `grok agent stdio`, `initialize`, then `_x.ai/billing`.
//! `creditUsagePercent` is percent **spent**. One window: weekly credits.
use serde_json::{json, Value};

use super::{
    run::{with_rpc, Program},
    window::{minutes_of, rfc3339_ms, used, window},
    Read,
};

const LABEL: &str = "grok agent stdio";

pub async fn read(program: &Program) -> Result<Read, String> {
    let result = with_rpc(program, &["agent", "stdio"], LABEL, async |rpc| {
        rpc.call("initialize", json!({ "protocolVersion": 1, "clientCapabilities": {} })).await?;
        rpc.call("_x.ai/billing", json!({})).await
    })
    .await?;
    parse(&result)
}

/// The window of a `_x.ai/billing` result.
pub fn parse(result: &Value) -> Result<Read, String> {
    let config = result.get("config").ok_or_else(|| format!("{LABEL}: billing has no config"))?;
    let plan = result.get("subscription_tier").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(str::to_owned);
    let Some(percent) = config.get("creditUsagePercent").and_then(Value::as_f64) else {
        // No credit use: no window.
        return Ok(Read { plan, windows: Vec::new(), credits: None });
    };
    let period = config.get("currentPeriod");
    let at = |k: &str| period.and_then(|p| p.get(k)).and_then(Value::as_str).and_then(rfc3339_ms);
    let (start, end) = (at("start"), at("end"));
    // The length from the period's type, else from its two ends.
    let minutes = period
        .and_then(|p| p.get("type"))
        .and_then(Value::as_str)
        .and_then(|t| minutes_of(t.strip_prefix("USAGE_PERIOD_TYPE_").unwrap_or(t)))
        .or_else(|| match (start, end) {
            (Some(s), Some(e)) if e > s => u32::try_from((e - s) / 60_000).ok().filter(|m| *m > 0),
            _ => None,
        })
        .ok_or_else(|| format!("{LABEL}: the billing period has no length"))?;
    let used = used(percent, false).ok_or_else(|| format!("{LABEL}: creditUsagePercent is not a number"))?;
    let mut w = window(None, minutes, used, end);
    w.credits = true;
    Ok(Read { plan, windows: vec![w], credits: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::window::WEEK;

    fn fixture() -> Value {
        let v: Value = serde_json::from_str(include_str!("../../tests/fixtures/usage/grok-billing.json")).unwrap();
        v["billing"]["result"].clone()
    }

    #[test]
    fn credit_usage_is_spent() {
        let read = parse(&fixture()).unwrap();
        assert_eq!(read.windows.len(), 1);
        let w = &read.windows[0];
        assert!(w.credits);
        assert_eq!((w.minutes, w.used, w.threshold), (WEEK, 21, 80));
        assert_eq!(w.resets_at, rfc3339_ms("2026-11-18T08:31:28.960+00:00"));
        assert_eq!(read.plan.as_deref(), Some("SuperGrok"));
        assert_eq!(read.credits, None);
    }

    #[test]
    fn length_from_the_period_ends() {
        let mut v = fixture();
        v["config"]["currentPeriod"]["type"] = json!("USAGE_PERIOD_TYPE_UNKNOWN");
        assert_eq!(parse(&v).unwrap().windows[0].minutes, WEEK);
    }
}
