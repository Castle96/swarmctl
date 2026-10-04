//! Time helpers shared by commands.
//!
//! Two different duration grammars existed (`logs --since` accepted `30s`/`5m`
//! but not `2h30m` or `1w`; `events --since` accepted those but not a bare
//! number). One parser now handles both.

/// Parse a duration into seconds.
///
/// A unit is **required**: `30s`, `5m`, `2h`, `1d`, `1w`, or a compound form
/// like `1h30m`. Units may appear in any order and each may repeat.
///
/// A bare number is deliberately *not* a duration. `--since` must be able to
/// accept an absolute Unix timestamp, and `1754760000` is indistinguishable
/// from "1754760000 seconds ago". Following Docker, a value without a unit is
/// treated as an absolute timestamp; write `30s` for "the last 30 seconds".
///
/// Returns `None` for an empty string, a non-numeric component, or an unknown
/// unit, so callers can pass the value through as a timestamp.
pub fn parse_duration_secs(input: &str) -> Option<i64> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }

    let mut total: i64 = 0;
    let mut digits = String::new();
    let mut saw_unit = false;

    for c in s.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let n: i64 = digits.parse().ok()?;
        digits.clear();
        saw_unit = true;
        total = total.checked_add(n.checked_mul(unit_secs(c)?)?)?;
    }

    // Trailing digits after a unit ("5m3") are ambiguous: seconds? minutes and
    // seconds? Reject rather than silently dropping them.
    if saw_unit && !digits.is_empty() {
        return None;
    }

    saw_unit.then_some(total)
}

fn unit_secs(unit: char) -> Option<i64> {
    Some(match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        'w' => 604_800,
        // An unknown unit must fail rather than silently contribute zero, which
        // would turn `--since 10x` into "10 seconds ago".
        _ => return None,
    })
}

/// Convert a relative duration into an absolute Unix timestamp.
///
/// `--since` accepts either a relative duration (`1h`, `30m`) or an absolute
/// Unix timestamp, which is passed through unchanged.
pub fn resolve_since(input: &str, now: i64) -> String {
    match parse_duration_secs(input) {
        Some(seconds) => (now - seconds).to_string(),
        None => input.trim().to_string(),
    }
}

/// Current Unix time in seconds.
pub fn epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `HH:MM:SS` for the last `secs` seconds' wall-clock time.
pub fn hms(secs: i64) -> String {
    let mins = secs / 60;
    let hours = mins / 60;
    format!("{:02}:{:02}:{:02}", hours % 24, mins % 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_units() {
        assert_eq!(parse_duration_secs("30s"), Some(30));
        assert_eq!(parse_duration_secs("5m"), Some(300));
        assert_eq!(parse_duration_secs("2h"), Some(7200));
        assert_eq!(parse_duration_secs("1d"), Some(86_400));
        assert_eq!(parse_duration_secs("1w"), Some(604_800));
    }

    #[test]
    fn parses_compound_durations() {
        assert_eq!(parse_duration_secs("2h30m"), Some(9000));
        assert_eq!(parse_duration_secs("1h15m30s"), Some(4530));
        assert_eq!(parse_duration_secs("1d12h"), Some(129_600));
    }

    #[test]
    fn a_bare_number_is_a_timestamp_not_a_duration() {
        // Following Docker: without a unit the value is an absolute timestamp,
        // because `1754760000` cannot mean "1754760000 seconds ago".
        assert_eq!(parse_duration_secs("42"), None);
        assert_eq!(parse_duration_secs("  42  "), None);
        assert_eq!(parse_duration_secs("1754760000"), None);
    }

    #[test]
    fn tolerates_whitespace() {
        assert_eq!(parse_duration_secs("  10m "), Some(600));
    }

    #[test]
    fn rejects_unparseable_input() {
        for bad in ["", "   ", "abc", "10x", "m", "5m3", "--1h", "1y"] {
            assert_eq!(parse_duration_secs(bad), None, "should reject {bad:?}");
        }
    }

    #[test]
    fn rejects_overflow() {
        assert_eq!(parse_duration_secs("99999999999999999999w"), None);
    }

    #[test]
    fn resolve_since_subtracts_from_now() {
        let now = 1_000_000;
        assert_eq!(resolve_since("30s", now), "999970");
        assert_eq!(resolve_since("1h", now), "996400");
    }

    #[test]
    fn resolve_since_passes_timestamps_through() {
        let now = 1_000_000;
        assert_eq!(resolve_since("1754760000", now), "1754760000");
        assert_eq!(resolve_since("  1754760000 ", now), "1754760000");
    }

    #[test]
    fn hms_wraps_at_a_day() {
        assert_eq!(hms(0), "00:00:00");
        assert_eq!(hms(3661), "01:01:01");
        assert_eq!(hms(86_400 + 61), "00:01:01");
    }

    #[test]
    fn epoch_secs_is_plausible() {
        // Later than 2020-01-01 and before 2100.
        let now = epoch_secs();
        assert!(now > 1_577_836_800, "got {now}");
        assert!(now < 4_102_444_800, "got {now}");
    }
}
