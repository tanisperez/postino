//! Durations (`30s`, `500ms`, `2m`) and sizes (`50MB`, `512kB`) as typed on the command line.
//! Sizes are decimal, like the sizes the app shows.

use std::time::Duration;

/// `500ms`, `30s`, `2m`, `1h`, or a bare number of seconds. Decimals are fine: `1.5s`.
pub fn parse_duration(text: &str) -> Result<Duration, String> {
    let text = text.trim();
    let (number, unit) = split_number(text);
    let value: f64 = number
        .parse()
        .map_err(|_| format!("expected a time such as 30s, 500ms or 2m, got {text:?}"))?;
    let seconds = match unit.to_ascii_lowercase().as_str() {
        "ms" => value / 1000.0,
        "" | "s" => value,
        "m" => value * 60.0,
        "h" => value * 3600.0,
        _ => return Err(format!("unknown time unit {unit:?}, use ms, s, m or h")),
    };
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(format!("{text:?} is not a valid time"));
    }
    Ok(Duration::from_secs_f64(seconds))
}

/// `50MB`, `512kB`, `1.5GB`, `100B`, or a bare number of bytes. Decimal units.
pub fn parse_size(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let (number, unit) = split_number(text);
    let value: f64 = number
        .parse()
        .map_err(|_| format!("expected a size such as 50MB, got {text:?}"))?;
    let factor = match unit.to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kb" | "k" => 1e3,
        "mb" | "m" => 1e6,
        "gb" | "g" => 1e9,
        _ => return Err(format!("unknown size unit {unit:?}, use B, kB, MB or GB")),
    };
    let bytes = value * factor;
    if !bytes.is_finite() || bytes < 1.0 {
        return Err(format!("{text:?} is not a valid size"));
    }
    // Rounded down to whole bytes; the limit is far below u64::MAX in practice.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(bytes as u64)
}

/// The leading number and the unit after it.
fn split_number(text: &str) -> (&str, &str) {
    let end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    (&text[..end], text[end..].trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn durations_take_a_unit() {
        assert_eq!(parse_duration("500ms"), Ok(Duration::from_millis(500)));
        assert_eq!(parse_duration("30s"), Ok(Duration::from_secs(30)));
        assert_eq!(parse_duration("30"), Ok(Duration::from_secs(30)));
        assert_eq!(parse_duration("2m"), Ok(Duration::from_secs(120)));
        assert_eq!(parse_duration("1.5s"), Ok(Duration::from_millis(1500)));
        assert!(parse_duration("fast").is_err());
        assert!(parse_duration("3d").is_err());
    }

    #[test]
    fn sizes_are_decimal() {
        assert_eq!(parse_size("50MB"), Ok(50_000_000));
        assert_eq!(parse_size("512kB"), Ok(512_000));
        assert_eq!(parse_size("1.5GB"), Ok(1_500_000_000));
        assert_eq!(parse_size("100"), Ok(100));
        assert!(parse_size("0").is_err());
        assert!(parse_size("big").is_err());
        assert!(parse_size("3TB").is_err());
    }
}
