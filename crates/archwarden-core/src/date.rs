//! A civil date, and the arithmetic to compare two of them.
//!
//! # Why there is no dependency here
//!
//! Everything this needs is three operations: parse `YYYY-MM-DD`, say which of
//! two dates is earlier, and count the days between them. ISO 8601 with
//! leading zeros already sorts correctly as text, and the conversion to and
//! from a day number is the well-known civil-from-days arithmetic — about
//! twenty lines with no table and no locale. A date library would bring a tree
//! of crates, a licence review and a timezone database to subtract two
//! numbers.
//!
//! # It is UTC, and that is a decision
//!
//! `today()` reads the system clock as UTC. A local date would make CI and a
//! laptop disagree about which day it is, which is the determinism decision 28
//! defended when it refused to read `git`. The cost is stated rather than
//! hidden: somebody in UTC-8 late in the evening is already on tomorrow's date
//! here, and `--as-of` is how they say otherwise. Issue #117.

use std::fmt;

/// A day, with no time and no zone.
///
/// Stored as the civil date rather than as a day number, so the value that
/// comes out is the value that went in and a round trip through text cannot
/// drift.
// No `Hash`: nothing keys a map by a date, and a derive nothing calls is a
// derive nothing tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    year: i32,
    month: u32,
    day: u32,
}

impl Date {
    /// The day the epoch starts, for a context that has no date to offer.
    pub const EPOCH: Self = Self {
        year: 1970,
        month: 1,
        day: 1,
    };

    /// The date `text` spells, if it spells one.
    ///
    /// ISO `YYYY-MM-DD` and nothing else. Not a lenient parser: `2026-1-5` and
    /// `01/12/2026` are refused rather than guessed at, because guessing which
    /// of two numbers is the month is how a deadline lands eleven months from
    /// where it was meant to. A value that is not a date is its own finding.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (year, rest) = text.split_once('-')?;
        let (month, day) = rest.split_once('-')?;
        if year.len() != 4 || month.len() != 2 || day.len() != 2 {
            return None;
        }

        let date = Self {
            year: year.parse().ok()?,
            month: month.parse().ok()?,
            day: day.parse().ok()?,
        };

        // A day number that round-trips is a day that exists, which catches
        // 2026-02-30 and 2025-02-29 without a table of month lengths.
        (date.month >= 1
            && date.month <= 12
            && date.day >= 1
            && Self::from_days(date.days()) == date)
            .then_some(date)
    }

    /// Today, in UTC.
    ///
    /// Nothing but the clock read: the arithmetic is `from_unix_seconds`,
    /// which is testable exactly. One function doing both would leave a test
    /// that can only assert a *range* around today — and a range wide enough
    /// to hold every plausible today is wide enough to hold a wrong answer.
    #[must_use]
    pub fn today() -> Self {
        Self::from_unix_seconds(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        )
    }

    /// The day a Unix timestamp falls in, in UTC.
    fn from_unix_seconds(seconds: u64) -> Self {
        Self::from_days(i64::try_from(seconds / 86_400).unwrap_or(0))
    }

    /// How many days separate this date from `other`, positive when later.
    #[must_use]
    pub fn days_since(self, other: Self) -> i64 {
        self.days() - other.days()
    }

    /// Days since 1970-01-01, by Howard Hinnant's `days_from_civil`.
    fn days(self) -> i64 {
        let year = i64::from(self.year) - i64::from(self.month <= 2);
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let month = i64::from(self.month);
        let day_of_year =
            (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    /// The inverse, by `civil_from_days`.
    fn from_days(days: i64) -> Self {
        let days = days + 719_468;
        let era = days.div_euclid(146_097);
        let day_of_era = days - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let m = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * m + 2) / 5 + 1;
        let month = m + if m < 10 { 3 } else { -9 };

        Self {
            year: i32::try_from(year + i64::from(month <= 2)).unwrap_or(0),
            month: u32::try_from(month).unwrap_or(1),
            day: u32::try_from(day).unwrap_or(1),
        }
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> Date {
        // `expect` rather than a formatted panic: the closure a `panic!` needs
        // would be a function in this file that never runs, and a derive or a
        // closure nothing calls is one nothing tests.
        Date::parse(text).expect("a valid ISO date")
    }

    /// The format, and the round trip: what comes out is what went in.
    #[test]
    fn an_iso_date_parses_and_prints_itself_back() {
        for text in ["2026-12-01", "1970-01-01", "2000-02-29", "2400-02-29"] {
            assert_eq!(date(text).to_string(), text);
        }
    }

    /// Not a lenient parser. Guessing which of two numbers is the month is how
    /// a deadline lands eleven months from where it was meant to.
    #[test]
    fn anything_that_is_not_an_iso_date_is_refused() {
        for text in [
            "2026-1-5",
            // Each length checked on its own: the guard asks three questions,
            // and a test that only ever gets two of them wrong at once cannot
            // tell one from another.
            "26-01-05",
            "2026-1-05",
            "2026-01-5",
            "02026-01-05",
            "01/12/2026",
            "1 December 2026",
            "2026-12",
            "2026-12-01T00:00:00Z",
            "",
            "next tuesday",
            "2026-13-01",
            "2026-00-01",
            "2026-12-00",
            "2026-02-30",
            "2025-02-29",
        ] {
            assert!(Date::parse(text).is_none(), "{text} should be refused");
        }
    }

    /// A leap day that exists is accepted and one that does not is not, which
    /// is the round trip doing the work instead of a table of month lengths.
    #[test]
    fn the_leap_rule_is_the_real_one() {
        assert!(Date::parse("2024-02-29").is_some(), "divisible by 4");
        assert!(Date::parse("2000-02-29").is_some(), "and by 400");
        assert!(Date::parse("1900-02-29").is_none(), "but not by 100 alone");
    }

    /// The whole arithmetic this feature needs: how many days have passed.
    #[test]
    fn the_days_between_two_dates_are_counted() {
        assert_eq!(date("2027-01-15").days_since(date("2026-12-01")), 45);
        assert_eq!(date("2026-12-01").days_since(date("2027-01-15")), -45);
        assert_eq!(date("2026-12-01").days_since(date("2026-12-01")), 0);
        // Across a leap day, which is where a naive count is wrong by one.
        assert_eq!(date("2024-03-01").days_since(date("2024-02-28")), 2);
        assert_eq!(date("2023-03-01").days_since(date("2023-02-28")), 1);
    }

    /// Ordering is what a deadline asks, and it agrees with the text order —
    /// which is why no library is needed to sort these.
    #[test]
    fn dates_order_the_way_their_text_does() {
        let mut dates = [date("2026-12-01"), date("2025-01-31"), date("2026-01-31")];
        dates.sort_unstable();

        assert_eq!(
            dates.map(|d| d.to_string()),
            ["2025-01-31", "2026-01-31", "2026-12-01"]
        );
        assert!(date("2026-12-01") > date("2026-11-30"));
    }

    /// The clock arithmetic, pinned exactly. A test of `today()` alone can
    /// only assert a *range*, and a range wide enough to hold every plausible
    /// today is wide enough to hold a wrong answer: dividing by the wrong
    /// thing lands inside it at some hours of the day and not others.
    #[test]
    fn a_timestamp_lands_on_the_day_that_contains_it() {
        // Midnight on 2026-08-19 UTC, and the seconds either side of the day.
        assert_eq!(Date::from_unix_seconds(1_787_097_600), date("2026-08-19"));
        assert_eq!(Date::from_unix_seconds(1_787_097_599), date("2026-08-18"));
        assert_eq!(Date::from_unix_seconds(1_787_183_999), date("2026-08-19"));
        assert_eq!(Date::from_unix_seconds(1_787_184_000), date("2026-08-20"));

        assert_eq!(Date::from_unix_seconds(0), Date::EPOCH);
        assert_eq!(
            Date::from_unix_seconds(86_399),
            Date::EPOCH,
            "still the same day"
        );
    }

    /// And `today()` is that arithmetic applied to the clock, which is all
    /// that is left to say about it.
    #[test]
    fn today_is_a_real_date_no_earlier_than_the_day_this_was_written() {
        let today = Date::today();

        assert!(today >= date("2026-08-19"), "{today}");
        assert_eq!(
            Date::parse(&today.to_string()),
            Some(today),
            "and it round-trips like any other"
        );
    }

    /// The epoch is the value a context with no date to offer carries, and it
    /// is a real date so nothing has to handle a placeholder.
    #[test]
    fn the_epoch_is_an_ordinary_date() {
        assert_eq!(Date::EPOCH.to_string(), "1970-01-01");
        assert_eq!(Date::EPOCH.days_since(Date::EPOCH), 0);
        assert_eq!(date("1970-01-02").days_since(Date::EPOCH), 1);
        assert_eq!(date("1969-12-31").days_since(Date::EPOCH), -1);
    }
}
