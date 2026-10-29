//! `lemusync pause [--for <duration>]` and `lemusync resume`: the tray's Pause items from the
//! terminal (P5). They need the daemon; without one there is nothing to pause.

use lemusync_engine::EngineError;

use crate::i18n::I18n;
use crate::output;

/// The longest pause `--for` takes: 30 days.
const MAX_MS: u64 = 30 * 24 * 3_600_000;

/// `1h`, `30m`, `90s`, `1h30m`, `2d`: whole numbers, each with a unit, at least one. In ms.
///
/// # Errors
/// `Usage("duration_invalid:<text>")`.
pub fn parse_duration(text: &str) -> Result<u64, EngineError> {
    let bad = || EngineError::Usage(format!("duration_invalid:{text}"));
    let mut total: u64 = 0;
    let mut digits = String::new();
    let mut parts = 0;
    for c in text.trim().chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let unit: u64 = match c {
            's' => 1_000,
            'm' => 60_000,
            'h' => 3_600_000,
            'd' => 24 * 3_600_000,
            _ => return Err(bad()),
        };
        let n: u64 = digits.parse().map_err(|_| bad())?;
        digits.clear();
        total = n
            .checked_mul(unit)
            .and_then(|v| total.checked_add(v))
            .ok_or_else(bad)?;
        parts += 1;
    }
    if !digits.is_empty() || parts == 0 || total == 0 || total > MAX_MS {
        return Err(bad());
    }
    Ok(total)
}

/// Local time of day for `until_ms` is not known without a time-zone crate: show minutes
/// from now instead.
pub fn done(t: &I18n, json: bool, until_ms: Option<u64>) {
    if json {
        output::json(&serde_json::json!({
            "api_version": lemusync_api::API_VERSION,
            "paused": true,
            "until_ms": until_ms,
        }));
        return;
    }
    match until_ms {
        Some(until) => {
            let now = crate::now_ms_u64();
            let minutes = until.saturating_sub(now).div_ceil(60_000);
            output::line(
                t,
                "pause-done-for",
                &[("minutes", fluent_bundle::FluentValue::from(minutes))],
            );
        }
        None => output::line(t, "pause-done", &[]),
    }
}

pub fn resumed(t: &I18n, json: bool) {
    if json {
        output::json(&serde_json::json!({
            "api_version": lemusync_api::API_VERSION,
            "paused": false,
        }));
    } else {
        output::line(t, "resume-done", &[]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("1h").unwrap(), 3_600_000);
        assert_eq!(parse_duration("30m").unwrap(), 1_800_000);
        assert_eq!(parse_duration("1h30m").unwrap(), 5_400_000);
        assert_eq!(parse_duration("90s").unwrap(), 90_000);
        assert_eq!(parse_duration("2d").unwrap(), 2 * 86_400_000);
        for bad in [
            "",
            "h",
            "10",
            "1x",
            "0m",
            "-1h",
            "31d",
            "99999999999999999999h",
        ] {
            assert!(
                matches!(parse_duration(bad), Err(EngineError::Usage(k)) if k.starts_with("duration_invalid:")),
                "{bad}"
            );
        }
    }

    #[test]
    fn texts_in_english_and_spanish() {
        for key in ["pause-done", "pause-done-for", "resume-done"] {
            let args = [("minutes", fluent_bundle::FluentValue::from(5))];
            let en = I18n::new("en").get(key, &args);
            let es = I18n::new("es").get(key, &args);
            assert!(en.is_some() && es.is_some(), "{key}");
            assert_ne!(en, es, "{key}");
        }
    }
}
