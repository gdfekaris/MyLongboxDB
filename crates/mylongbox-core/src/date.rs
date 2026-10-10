use std::fmt;
use std::str::FromStr;

/// A calendar date in the proleptic Gregorian calendar, years 1 to 9999.
///
/// Stored in the database as `YYYY-MM-DD` text, which sorts in date order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    // Field order matters: the derived ordering compares year, then
    // month, then day.
    year: u16,
    month: u8,
    day: u8,
}

/// Why a date was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateError {
    /// Not in `YYYY-MM-DD` form.
    Format,
    /// A year, month, or day that does not exist, such as 2023-02-29.
    OutOfRange,
}

impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Format => "dates must be written as YYYY-MM-DD",
            Self::OutOfRange => "that date does not exist",
        })
    }
}

impl std::error::Error for DateError {}

impl Date {
    pub const MIN: Self = Self {
        year: 1,
        month: 1,
        day: 1,
    };
    pub const MAX: Self = Self {
        year: 9999,
        month: 12,
        day: 31,
    };

    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, DateError> {
        if !(1..=9999).contains(&year)
            || !(1..=12).contains(&month)
            || day == 0
            || day > days_in_month(year, month)
        {
            return Err(DateError::OutOfRange);
        }
        Ok(Self { year, month, day })
    }

    pub fn year(self) -> u16 {
        self.year
    }

    pub fn month(self) -> u8 {
        self.month
    }

    pub fn day(self) -> u8 {
        self.day
    }

    /// The date `days` days later (or earlier, if negative), or `None` if
    /// that falls outside years 1 to 9999.
    pub fn add_days(self, days: i64) -> Option<Self> {
        from_day_number(self.day_number().checked_add(days)?)
    }

    /// Whole days from `self` to `later`; negative if `later` is earlier.
    pub fn days_until(self, later: Self) -> i64 {
        later.day_number() - self.day_number()
    }

    /// Days since 1970-01-01.
    fn day_number(self) -> i64 {
        // Howard Hinnant's days_from_civil, with March as the first month
        // of the year so the leap day falls at the end.
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let m = i64::from(self.month);
        let d = i64::from(self.day);
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }
}

fn from_day_number(days: i64) -> Option<Date> {
    // Checked first so the arithmetic below cannot overflow.
    if !(Date::MIN.day_number()..=Date::MAX.day_number()).contains(&days) {
        return None;
    }
    // Inverse of day_number (Hinnant's civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Date::new(
        u16::try_from(year).ok()?,
        u8::try_from(month).ok()?,
        u8::try_from(day).ok()?,
    )
    .ok()
}

fn is_leap_year(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl FromStr for Date {
    type Err = DateError;

    fn from_str(s: &str) -> Result<Self, DateError> {
        let b = s.as_bytes();
        let shape_ok = b.len() == 10
            && b[4] == b'-'
            && b[7] == b'-'
            && b.iter()
                .enumerate()
                .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
        if !shape_ok {
            return Err(DateError::Format);
        }
        let num = |range: std::ops::Range<usize>| -> u16 {
            b[range].iter().fold(0, |n, c| n * 10 + u16::from(c - b'0'))
        };
        let month = u8::try_from(num(5..7)).map_err(|_| DateError::OutOfRange)?;
        let day = u8::try_from(num(8..10)).map_err(|_| DateError::OutOfRange)?;
        Self::new(num(0..4), month, day)
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
    use proptest::prelude::*;

    fn date(s: &str) -> Date {
        s.parse().unwrap()
    }

    #[test]
    fn parses_and_displays() {
        assert_eq!(date("1985-07-04").to_string(), "1985-07-04");
        assert_eq!(date("0001-01-01"), Date::MIN);
        assert_eq!(date("9999-12-31"), Date::MAX);
    }

    #[test]
    fn rejects_malformed_text() {
        for s in [
            "",
            "1985-7-04",
            "1985/07/04",
            "85-07-04",
            "1985-07-04 ",
            "+985-07-04",
            "１９８５-07-04",
        ] {
            assert_eq!(s.parse::<Date>(), Err(DateError::Format), "{s:?}");
        }
    }

    #[test]
    fn rejects_dates_that_do_not_exist() {
        for s in [
            "0000-01-01",
            "1985-00-10",
            "1985-13-01",
            "1985-04-31",
            "2023-02-29",
            "1900-02-29",
            "1985-01-00",
        ] {
            assert_eq!(s.parse::<Date>(), Err(DateError::OutOfRange), "{s:?}");
        }
    }

    #[test]
    fn leap_years_follow_the_gregorian_rules() {
        assert!(date("2024-02-29").year() == 2024);
        assert!(date("2000-02-29").year() == 2000);
        assert!("2100-02-29".parse::<Date>().is_err());
    }

    #[test]
    fn known_day_counts() {
        assert_eq!(date("1970-01-01").day_number(), 0);
        assert_eq!(date("2000-03-01").day_number(), 11_017);
        assert_eq!(date("2026-01-01").days_until(date("2026-12-31")), 364);
        assert_eq!(date("2024-01-01").days_until(date("2024-12-31")), 365);
        assert_eq!(date("2026-03-01").add_days(-1), Some(date("2026-02-28")));
    }

    #[test]
    fn arithmetic_stays_within_range() {
        assert_eq!(Date::MAX.add_days(1), None);
        assert_eq!(Date::MIN.add_days(-1), None);
        assert_eq!(Date::MIN.add_days(i64::MAX), None);
    }

    fn any_date() -> impl Strategy<Value = Date> {
        (Date::MIN.day_number()..=Date::MAX.day_number()).prop_map(|n| from_day_number(n).unwrap())
    }

    proptest! {
        #[test]
        fn text_round_trips(d in any_date()) {
            prop_assert_eq!(d.to_string().parse::<Date>(), Ok(d));
        }

        #[test]
        fn day_numbers_round_trip(d in any_date()) {
            prop_assert_eq!(from_day_number(d.day_number()), Some(d));
        }

        #[test]
        fn adding_days_matches_days_until(d in any_date(), n in -100_000i64..100_000) {
            if let Some(later) = d.add_days(n) {
                prop_assert_eq!(d.days_until(later), n);
            }
        }

        #[test]
        fn ordering_matches_day_numbers_and_text(a in any_date(), b in any_date()) {
            prop_assert_eq!(a.cmp(&b), a.day_number().cmp(&b.day_number()));
            prop_assert_eq!(a.cmp(&b), a.to_string().cmp(&b.to_string()));
        }

        #[test]
        fn parsing_never_panics(chars in prop::collection::vec(
            prop_oneof![any::<char>(), prop::sample::select(vec!['0', '2', '9', '-'])],
            0..14,
        )) {
            let s: String = chars.into_iter().collect();
            let _ = s.parse::<Date>();
        }
    }
}
