//! The one timestamp this harness writes.
//!
//! A run file says when the run started, and a host record says when a sweep started and finished. That is the whole requirement: seconds, UTC, RFC 3339, and the same string on every platform.
//!
//! It is arithmetic rather than a dependency. A date library is a reasonable thing to depend on when a project needs calendars, time zones, parsing or formatting choices, and this project needs none of those, so the trade is a few dozen lines of civil calendar arithmetic against a dependency tree that has to be audited on every update for a field nobody reads programmatically.
//!
//! The conversion from a day count to a date is the standard one, and it is exact for every date this will ever be handed.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds in a day.
const DAY: i64 = 86_400;

/// Now, as it goes in a file.
///
/// A clock that is before the epoch, which is a machine with its date badly wrong, comes out as the epoch rather than as a panic. A sweep does not stop over a timestamp.
#[must_use]
pub fn now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(0));
    stamp(seconds)
}

/// A count of seconds since the epoch, written the way a run file writes it.
#[must_use]
pub fn stamp(seconds: i64) -> String {
    // Rust's remainder keeps the sign of the left side, and a negative time of day is not a time of day, so the day is floored rather than truncated.
    let days = seconds.div_euclid(DAY);
    let rest = seconds.rem_euclid(DAY);
    let (year, month, day) = civil(days);
    let (hour, minute, second) = (rest / 3600, rest % 3600 / 60, rest % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// A day count since the epoch, as a year, a month and a day.
///
/// Howard Hinnant's civil calendar algorithm, which shifts the year to start in March so that the leap day lands at the end of it and the month lengths become a straight line.
fn civil(days: i64) -> (i64, i64, i64) {
    // The epoch, moved to the first of March of year zero of a four hundred year cycle.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::{now, stamp};

    #[test]
    fn the_epoch_is_the_epoch() {
        assert_eq!(stamp(0), "1970-01-01T00:00:00Z");
    }

    // Dates picked for the cases the arithmetic can get wrong: a leap day, the day after one, the end of a century that is not a leap year, and the end of one that is.
    #[test]
    fn the_awkward_days_come_out_right() {
        assert_eq!(stamp(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(stamp(951_868_800), "2000-03-01T00:00:00Z");
        assert_eq!(stamp(4_107_456_000), "2100-02-28T00:00:00Z");
        assert_eq!(stamp(4_107_542_400), "2100-03-01T00:00:00Z");
        assert_eq!(stamp(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn the_time_of_day_is_the_time_of_day() {
        assert_eq!(stamp(1_756_944_000 + 3661), "2025-09-04T01:01:01Z");
        assert_eq!(stamp(1_756_944_000 + 86_399), "2025-09-04T23:59:59Z");
    }

    // A machine with its clock set before the epoch still writes a date rather than something that will not parse.
    #[test]
    fn a_time_before_the_epoch_is_still_a_date() {
        assert_eq!(stamp(-1), "1969-12-31T23:59:59Z");
        assert_eq!(stamp(-86_400), "1969-12-31T00:00:00Z");
    }

    #[test]
    fn now_is_the_shape_a_run_file_wants() {
        let text = now();
        assert_eq!(text.len(), 20, "{text}");
        assert!(text.ends_with('Z'), "{text}");
        // Written this century, and this is a test that will need looking at in seventy five years.
        assert!(text.starts_with("20"), "{text}");
    }
}
