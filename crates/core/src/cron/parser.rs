//! Schedule parsing: cron expressions and "every X" interval syntax.

use anyhow::{Result, bail};
use chrono::{DateTime, Local};
use croner::Cron;
use std::time::Duration;

/// A parsed schedule that can determine the next run time.
pub enum Schedule {
    /// Standard cron expression (5 or 6 fields)
    Cron(Box<Cron>),
    /// Simple interval (e.g., "every 30m", "every 2h")
    Interval(Duration),
}

impl Schedule {
    /// Parse a schedule string. Accepts:
    /// - "every 30m", "every 2h", "every 1d"
    /// - Standard cron expressions: "0 */6 * * *"
    pub fn parse(s: &str) -> Result<Self> {
        let trimmed = s.trim();

        if let Some(interval_str) = trimmed.strip_prefix("every ") {
            let duration = parse_interval(interval_str.trim())?;
            return Ok(Schedule::Interval(duration));
        }

        let cron = Cron::new(trimmed)
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid cron expression '{}': {}", trimmed, e))?;
        Ok(Schedule::Cron(Box::new(cron)))
    }

    /// Get the next run time after `after`.
    pub fn next_after(&self, after: DateTime<Local>) -> Option<DateTime<Local>> {
        match self {
            Schedule::Cron(cron) => cron.find_next_occurrence(&after, false).ok(),
            Schedule::Interval(duration) => {
                Some(after + chrono::Duration::from_std(*duration).ok()?)
            }
        }
    }
}

/// Parse an interval string like "30m", "2h", "1d", "90s".
fn parse_interval(s: &str) -> Result<Duration> {
    let s = s.trim();
    if s.is_empty() {
        bail!("Empty interval");
    }

    let (suffix_start, suffix) = s
        .char_indices()
        .next_back()
        .ok_or_else(|| anyhow::anyhow!("Empty interval"))?;
    let num_str = &s[..suffix_start];
    if num_str.is_empty() {
        bail!("Missing interval number");
    }

    let num: u64 = num_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid interval number: '{}'", num_str))?;
    if num == 0 {
        bail!("Interval must be greater than zero");
    }

    let multiplier = match suffix {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86400,
        _ => bail!("Unknown interval suffix '{}'. Use s, m, h, or d.", suffix),
    };

    let seconds = num
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow::anyhow!("Interval is too large"))?;
    let duration = Duration::from_secs(seconds);
    chrono::Duration::from_std(duration).map_err(|_| anyhow::anyhow!("Interval is too large"))?;

    Ok(duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_interval() {
        assert_eq!(parse_interval("30m").unwrap(), Duration::from_secs(1800));
        assert_eq!(parse_interval("2h").unwrap(), Duration::from_secs(7200));
        assert_eq!(parse_interval("1d").unwrap(), Duration::from_secs(86400));
        assert_eq!(parse_interval("90s").unwrap(), Duration::from_secs(90));
        assert!(parse_interval("abc").is_err());
    }

    #[test]
    fn test_parse_interval_rejects_zero() {
        let err = parse_interval("0s").unwrap_err().to_string();
        assert!(err.contains("greater than zero"));
    }

    #[test]
    fn test_parse_interval_rejects_too_large() {
        let err = parse_interval("18446744073709551615d")
            .unwrap_err()
            .to_string();
        assert!(err.contains("too large"));
    }

    #[test]
    fn test_parse_interval_rejects_malformed_unicode() {
        assert!(parse_interval("é").is_err());
        assert!(parse_interval("1日").is_err());
    }

    #[test]
    fn test_parse_cron() {
        let s = Schedule::parse("0 */6 * * *").unwrap();
        assert!(matches!(s, Schedule::Cron(_)));
    }

    #[test]
    fn test_parse_every() {
        let s = Schedule::parse("every 30m").unwrap();
        assert!(matches!(s, Schedule::Interval(_)));
    }

    #[test]
    fn test_next_after_interval() {
        let s = Schedule::parse("every 1h").unwrap();
        let now = Local::now();
        let next = s.next_after(now).unwrap();
        let diff = next - now;
        assert!((diff.num_seconds() - 3600).abs() < 2);
    }
}
