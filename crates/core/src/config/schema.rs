//! Configuration schema validation and helpers

use std::time::Duration;

/// Parse a duration string like "30m", "1h", "2h30m"
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("Invalid duration: empty".to_string());
    }

    let mut total_seconds: u64 = 0;
    let mut current_num = String::new();
    let mut saw_unit = false;

    for c in s.chars() {
        if c.is_ascii_digit() {
            current_num.push(c);
        } else {
            if current_num.is_empty() {
                return Err(format!("Missing number before duration unit: {}", c));
            }

            let num: u64 = current_num
                .parse()
                .map_err(|_| format!("Invalid number in duration: {}", s))?;
            current_num.clear();
            saw_unit = true;

            let multiplier = match c {
                's' => 1,
                'm' => 60,
                'h' => 3600,
                'd' => 86400,
                _ => return Err(format!("Unknown duration unit: {}", c)),
            };

            let seconds = num
                .checked_mul(multiplier)
                .ok_or_else(|| format!("Duration too large: {}", s))?;
            total_seconds = total_seconds
                .checked_add(seconds)
                .ok_or_else(|| format!("Duration too large: {}", s))?;
        }
    }

    if !current_num.is_empty() {
        return Err(format!(
            "Missing duration unit after number: {}",
            current_num
        ));
    }

    if !saw_unit || total_seconds == 0 {
        return Err(format!("Invalid duration: {}", s));
    }

    Ok(Duration::from_secs(total_seconds))
}

/// Parse a time string like "09:00" or "22:30"
pub fn parse_time(s: &str) -> Result<(u8, u8), String> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid time format: {}. Expected HH:MM", s));
    }

    let hour: u8 = parts[0]
        .parse()
        .map_err(|_| format!("Invalid hour: {}", parts[0]))?;
    let minute: u8 = parts[1]
        .parse()
        .map_err(|_| format!("Invalid minute: {}", parts[1]))?;

    if hour > 23 {
        return Err(format!("Hour must be 0-23, got: {}", hour));
    }
    if minute > 59 {
        return Err(format!("Minute must be 0-59, got: {}", minute));
    }

    Ok((hour, minute))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_duration("1h30m").unwrap(), Duration::from_secs(5400));
        assert_eq!(parse_duration("1d").unwrap(), Duration::from_secs(86400));
    }

    #[test]
    fn test_parse_duration_rejects_trailing_number() {
        let err = parse_duration("1h30").unwrap_err();
        assert!(err.contains("Missing duration unit"));
    }

    #[test]
    fn test_parse_duration_rejects_zero_and_missing_number() {
        assert!(parse_duration("0s").is_err());
        assert!(parse_duration("m").is_err());
    }

    #[test]
    fn test_parse_duration_rejects_overflow() {
        let err = parse_duration("18446744073709551615d").unwrap_err();
        assert!(err.contains("too large") || err.contains("Invalid number"));

        let err = parse_duration("18446744073709551615s1s").unwrap_err();
        assert!(err.contains("too large"));
    }

    #[test]
    fn test_parse_time() {
        assert_eq!(parse_time("09:00").unwrap(), (9, 0));
        assert_eq!(parse_time("22:30").unwrap(), (22, 30));
        assert_eq!(parse_time("00:00").unwrap(), (0, 0));
        assert_eq!(parse_time("23:59").unwrap(), (23, 59));
    }
}
