use std::fmt;

use crate::date::Date;

/// A currency an amount can be entered in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Currency {
    /// US dollars, counted in cents.
    Usd,
    /// Gold, counted in ten-thousandths of a troy ounce.
    Gold,
    /// Bitcoin, counted in satoshis.
    Btc,
}

impl Currency {
    pub const ALL: [Self; 3] = [Self::Usd, Self::Gold, Self::Btc];

    /// How many decimal places one whole unit is divided into.
    pub const fn decimals(self) -> u32 {
        match self {
            Self::Usd => 2,
            Self::Gold => 4,
            Self::Btc => 8,
        }
    }

    /// Smallest units in one whole unit (a dollar, an ounce, a bitcoin).
    const fn scale(self) -> i64 {
        10i64.pow(self.decimals())
    }

    /// The code stored in the database. Never change these.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Usd => "USD",
            Self::Gold => "XAU",
            Self::Btc => "BTC",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.code() == code)
    }
}

/// An exact amount of money in one currency.
///
/// Held as a whole number of the currency's smallest unit, so sums never
/// drift from rounding and an amount is always exactly what was entered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Amount {
    currency: Currency,
    units: i64,
}

/// Why an amount could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseAmountError {
    Empty,
    /// Anything other than digits and at most one decimal point.
    Invalid,
    Negative,
    /// More decimal places than the currency can hold exactly.
    TooPrecise {
        max_decimals: u32,
    },
    TooLarge,
}

impl fmt::Display for ParseAmountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("enter an amount"),
            Self::Invalid => f.write_str("amounts may contain only digits and one decimal point"),
            Self::Negative => f.write_str("amounts cannot be negative"),
            Self::TooPrecise { max_decimals } => {
                write!(
                    f,
                    "this currency allows at most {max_decimals} decimal places"
                )
            }
            Self::TooLarge => f.write_str("that amount is too large"),
        }
    }
}

impl std::error::Error for ParseAmountError {}

impl Amount {
    pub const fn new(currency: Currency, units: i64) -> Self {
        Self { currency, units }
    }

    pub const fn zero(currency: Currency) -> Self {
        Self::new(currency, 0)
    }

    pub const fn currency(self) -> Currency {
        self.currency
    }

    /// The amount in the currency's smallest unit.
    pub const fn units(self) -> i64 {
        self.units
    }

    /// Reads a decimal amount such as `12.34` exactly, without rounding.
    pub fn parse(currency: Currency, text: &str) -> Result<Self, ParseAmountError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(ParseAmountError::Empty);
        }
        if text.starts_with('-') {
            return Err(ParseAmountError::Negative);
        }
        let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
        let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if (whole.is_empty() && frac.is_empty()) || !all_digits(whole) || !all_digits(frac) {
            return Err(ParseAmountError::Invalid);
        }
        let max_decimals = currency.decimals();
        let significant_frac = frac.trim_end_matches('0');
        if significant_frac.len() > max_decimals as usize {
            return Err(ParseAmountError::TooPrecise { max_decimals });
        }

        let mut units: i64 = 0;
        let digits = whole
            .bytes()
            .chain(significant_frac.bytes())
            .chain(std::iter::repeat_n(
                b'0',
                max_decimals as usize - significant_frac.len(),
            ));
        for digit in digits {
            units = units
                .checked_mul(10)
                .and_then(|u| u.checked_add(i64::from(digit - b'0')))
                .ok_or(ParseAmountError::TooLarge)?;
        }
        Ok(Self::new(currency, units))
    }

    /// The amount as a plain decimal with every decimal place shown, such
    /// as `12.30`, `1.2500`, or `0.00012345`. [`Amount::parse`] reads it
    /// back exactly.
    pub fn to_decimal_string(self) -> String {
        let scale = self.currency.scale().unsigned_abs();
        let magnitude = self.units.unsigned_abs();
        let sign = if self.units < 0 { "-" } else { "" };
        format!(
            "{sign}{}.{:0width$}",
            magnitude / scale,
            magnitude % scale,
            width = self.currency.decimals() as usize
        )
    }

    /// The sum, or `None` if the currencies differ or the total overflows.
    pub fn checked_add(self, other: Self) -> Option<Self> {
        if self.currency != other.currency {
            return None;
        }
        Some(Self::new(
            self.currency,
            self.units.checked_add(other.units)?,
        ))
    }
}

/// Reads a price in dollars and cents, such as `0.75`, as whole cents,
/// for the prices kept outside [`Currency`].
pub fn parse_cents(text: &str) -> Result<i64, ParseAmountError> {
    Amount::parse(Currency::Usd, text).map(Amount::units)
}

/// Whole cents as dollars and cents, such as `0.75`; [`parse_cents`]
/// reads it back.
pub fn cents_string(cents: i64) -> String {
    Amount::new(Currency::Usd, cents).to_decimal_string()
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.to_decimal_string();
        match self.currency {
            Currency::Usd => write!(f, "${n}"),
            Currency::Gold => write!(f, "{n} oz"),
            Currency::Btc => write!(f, "{n} BTC"),
        }
    }
}

/// The value of gold and bitcoin in US dollars on one date, as entered by
/// the user. The app cannot look prices up, so these are the only rates
/// it knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExchangeRate {
    date: Date,
    usd_cents_per_ounce: i64,
    usd_cents_per_bitcoin: i64,
}

/// Why an exchange rate was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateError {
    /// Both prices must be USD amounts above zero.
    NotPositiveUsd,
}

impl fmt::Display for RateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("exchange rates must be US dollar amounts above zero")
    }
}

impl std::error::Error for RateError {}

impl ExchangeRate {
    pub fn new(
        date: Date,
        usd_per_ounce: Amount,
        usd_per_bitcoin: Amount,
    ) -> Result<Self, RateError> {
        let valid = |a: Amount| a.currency() == Currency::Usd && a.units() > 0;
        if !valid(usd_per_ounce) || !valid(usd_per_bitcoin) {
            return Err(RateError::NotPositiveUsd);
        }
        Ok(Self {
            date,
            usd_cents_per_ounce: usd_per_ounce.units(),
            usd_cents_per_bitcoin: usd_per_bitcoin.units(),
        })
    }

    pub fn date(self) -> Date {
        self.date
    }

    pub fn usd_per_ounce(self) -> Amount {
        Amount::new(Currency::Usd, self.usd_cents_per_ounce)
    }

    pub fn usd_per_bitcoin(self) -> Amount {
        Amount::new(Currency::Usd, self.usd_cents_per_bitcoin)
    }

    /// US cents that one whole unit of `currency` is worth at this rate.
    fn cents_per_whole(self, currency: Currency) -> i64 {
        match currency {
            Currency::Usd => Currency::Usd.scale(),
            Currency::Gold => self.usd_cents_per_ounce,
            Currency::Btc => self.usd_cents_per_bitcoin,
        }
    }
}

/// Why an amount could not be shown in another currency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConversionError {
    /// No exchange rate exists on or before the amount's date.
    NoRate,
    /// The converted amount does not fit.
    Overflow,
}

/// How old the newest rate may get before the app reminds the user.
pub const STALE_AFTER_DAYS: i64 = 30;

/// The user's exchange-rate history.
#[derive(Clone, Debug, Default)]
pub struct RateTable {
    /// Sorted by date, at most one rate per date.
    rates: Vec<ExchangeRate>,
}

impl RateTable {
    /// Builds a table from rates in any order. If two share a date, the
    /// later one in `rates` wins.
    pub fn new(rates: impl IntoIterator<Item = ExchangeRate>) -> Self {
        let mut rates: Vec<_> = rates.into_iter().collect();
        rates.reverse();
        rates.sort_by_key(|r| r.date);
        rates.dedup_by_key(|r| r.date);
        Self { rates }
    }

    /// The newest rate on or before `date`.
    pub fn rate_on(&self, date: Date) -> Option<ExchangeRate> {
        let after = self.rates.partition_point(|r| r.date <= date);
        after.checked_sub(1).map(|i| self.rates[i])
    }

    pub fn latest(&self) -> Option<ExchangeRate> {
        self.rates.last().copied()
    }

    /// Shows `amount`, dated `on`, in `target` at the rate for that date,
    /// so a 1985 purchase shows what it was worth in 1985. Rounds half
    /// away from zero to the target's smallest unit.
    pub fn convert(
        &self,
        amount: Amount,
        on: Date,
        target: Currency,
    ) -> Result<Amount, ConversionError> {
        if amount.currency() == target {
            return Ok(amount);
        }
        let rate = self.rate_on(on).ok_or(ConversionError::NoRate)?;
        convert_at(amount, rate, target)
    }

    /// Shows `amount` in `target` at the newest rate: what it is worth today.
    pub fn convert_at_latest(
        &self,
        amount: Amount,
        target: Currency,
    ) -> Result<Amount, ConversionError> {
        if amount.currency() == target {
            return Ok(amount);
        }
        let rate = self.latest().ok_or(ConversionError::NoRate)?;
        convert_at(amount, rate, target)
    }

    /// Whether to remind the user to enter a current rate: there is no
    /// rate at all, or the newest is more than [`STALE_AFTER_DAYS`] old.
    pub fn needs_update(&self, today: Date) -> bool {
        self.latest()
            .is_none_or(|r| r.date.days_until(today) > STALE_AFTER_DAYS)
    }
}

fn convert_at(
    amount: Amount,
    rate: ExchangeRate,
    target: Currency,
) -> Result<Amount, ConversionError> {
    let source = amount.currency();
    // target units = units * (cents per source unit) * (target scale)
    //                / ((source scale) * (cents per target unit)),
    // computed as one fraction so it is rounded exactly once.
    let numerator = i128::from(amount.units())
        .checked_mul(i128::from(rate.cents_per_whole(source)))
        .and_then(|n| n.checked_mul(i128::from(target.scale())))
        .ok_or(ConversionError::Overflow)?;
    let denominator = i128::from(source.scale()) * i128::from(rate.cents_per_whole(target));
    let units = div_round_half_away(numerator, denominator);
    let units = i64::try_from(units).map_err(|_| ConversionError::Overflow)?;
    Ok(Amount::new(target, units))
}

/// `n / d` rounded half away from zero; `d` must be positive.
fn div_round_half_away(n: i128, d: i128) -> i128 {
    let q = n / d;
    let r = n % d;
    if r.unsigned_abs() * 2 >= d.unsigned_abs() {
        q + n.signum()
    } else {
        q
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn usd(text: &str) -> Amount {
        Amount::parse(Currency::Usd, text).unwrap()
    }

    #[test]
    fn cents_read_and_write_as_dollars_and_cents() {
        assert_eq!(parse_cents("0.75"), Ok(75));
        assert_eq!(parse_cents(" 1.5 "), Ok(150));
        assert_eq!(parse_cents("2"), Ok(200));
        assert!(parse_cents("0.755").is_err());
        assert!(parse_cents("-1").is_err());
        assert_eq!(cents_string(75), "0.75");
        assert_eq!(cents_string(1200), "12.00");
    }

    fn date(s: &str) -> Date {
        s.parse().unwrap()
    }

    fn rate(on: &str, per_ounce: &str, per_bitcoin: &str) -> ExchangeRate {
        ExchangeRate::new(date(on), usd(per_ounce), usd(per_bitcoin)).unwrap()
    }

    #[test]
    fn parses_exactly() {
        assert_eq!(usd("12.34").units(), 1234);
        assert_eq!(usd("12").units(), 1200);
        assert_eq!(usd("12.").units(), 1200);
        assert_eq!(usd(".5").units(), 50);
        assert_eq!(usd(" 0.10 ").units(), 10);
        assert_eq!(
            Amount::parse(Currency::Gold, "1.25").unwrap().units(),
            12_500
        );
        assert_eq!(
            Amount::parse(Currency::Btc, "0.00012345").unwrap().units(),
            12_345
        );
        assert_eq!(
            Amount::parse(Currency::Btc, "21000000").unwrap().units(),
            2_100_000_000_000_000
        );
    }

    #[test]
    fn trailing_zeros_beyond_the_precision_are_harmless() {
        assert_eq!(usd("1.2300000").units(), 123);
    }

    #[test]
    fn refuses_to_round() {
        assert_eq!(
            Amount::parse(Currency::Usd, "1.234"),
            Err(ParseAmountError::TooPrecise { max_decimals: 2 })
        );
        assert_eq!(
            Amount::parse(Currency::Btc, "0.000000001"),
            Err(ParseAmountError::TooPrecise { max_decimals: 8 })
        );
    }

    #[test]
    fn rejects_bad_input() {
        use ParseAmountError::*;
        for (text, error) in [
            ("", Empty),
            ("   ", Empty),
            (".", Invalid),
            ("1.2.3", Invalid),
            ("$5", Invalid),
            ("1,000", Invalid),
            ("1e3", Invalid),
            ("+5", Invalid),
            ("٣", Invalid),
            ("-5", Negative),
            ("92233720368547758.08", TooLarge),
        ] {
            assert_eq!(Amount::parse(Currency::Usd, text), Err(error), "{text:?}");
        }
    }

    #[test]
    fn displays_every_decimal_place() {
        assert_eq!(usd("12.3").to_decimal_string(), "12.30");
        assert_eq!(
            Amount::new(Currency::Gold, 12_500).to_decimal_string(),
            "1.2500"
        );
        assert_eq!(
            Amount::new(Currency::Btc, 1).to_decimal_string(),
            "0.00000001"
        );
        assert_eq!(Amount::new(Currency::Usd, -5).to_decimal_string(), "-0.05");
        assert_eq!(usd("12.3").to_string(), "$12.30");
        assert_eq!(Amount::new(Currency::Gold, 12_500).to_string(), "1.2500 oz");
        assert_eq!(Amount::new(Currency::Btc, 1).to_string(), "0.00000001 BTC");
    }

    #[test]
    fn currency_codes_round_trip() {
        for c in Currency::ALL {
            assert_eq!(Currency::from_code(c.code()), Some(c));
        }
        assert_eq!(Currency::from_code("usd"), None);
    }

    #[test]
    fn adds_only_matching_currencies() {
        assert_eq!(usd("1.50").checked_add(usd("2.25")), Some(usd("3.75")));
        assert_eq!(usd("1").checked_add(Amount::new(Currency::Btc, 1)), None);
        assert_eq!(
            Amount::new(Currency::Usd, i64::MAX).checked_add(usd("0.01")),
            None
        );
    }

    #[test]
    fn rates_must_be_positive_dollars() {
        let d = date("2026-01-01");
        assert!(ExchangeRate::new(d, usd("0"), usd("1")).is_err());
        assert!(ExchangeRate::new(d, Amount::new(Currency::Usd, -100), usd("1")).is_err());
        assert!(ExchangeRate::new(d, usd("1"), Amount::new(Currency::Usd, -1)).is_err());
        assert!(ExchangeRate::new(d, usd("1"), Amount::new(Currency::Btc, 5)).is_err());
    }

    #[test]
    fn converts_at_the_rate_on_or_before_the_date() {
        let table = RateTable::new([
            rate("1985-01-01", "300.00", "0.01"),
            rate("2026-01-01", "4000.00", "100000.00"),
        ]);
        let paid = usd("1.50");
        // In 1985, $1.50 bought 0.005 oz of gold at $300/oz.
        assert_eq!(
            table.convert(paid, date("1985-06-15"), Currency::Gold),
            Ok(Amount::new(Currency::Gold, 50))
        );
        // The same dollars on the later rate's date.
        assert_eq!(
            table.convert(paid, date("2026-01-01"), Currency::Gold),
            Ok(Amount::new(Currency::Gold, 4))
        );
        assert_eq!(
            table.convert(paid, date("1984-12-31"), Currency::Gold),
            Err(ConversionError::NoRate)
        );
        assert_eq!(
            table.convert(paid, date("1900-01-01"), Currency::Usd),
            Ok(paid)
        );
    }

    #[test]
    fn converts_between_gold_and_bitcoin_through_dollars() {
        let table = RateTable::new([rate("2026-01-01", "4000.00", "100000.00")]);
        let one_ounce = Amount::parse(Currency::Gold, "1").unwrap();
        // 1 oz = $4,000 = 0.04 BTC.
        assert_eq!(
            table.convert_at_latest(one_ounce, Currency::Btc),
            Ok(Amount::parse(Currency::Btc, "0.04").unwrap())
        );
    }

    #[test]
    fn rounds_half_away_from_zero() {
        let table = RateTable::new([rate("2026-01-01", "3.00", "1.00")]);
        // $0.01 at $3/oz is 0.00333... oz, which rounds to 0.0033.
        assert_eq!(
            table.convert_at_latest(usd("0.01"), Currency::Gold),
            Ok(Amount::new(Currency::Gold, 33))
        );
        // 0.0050 oz at $1/oz is half a cent, which rounds up to $0.01.
        let table = RateTable::new([rate("2026-01-01", "1.00", "1.00")]);
        assert_eq!(
            table.convert_at_latest(Amount::new(Currency::Gold, 50), Currency::Usd),
            Ok(usd("0.01"))
        );
        assert_eq!(div_round_half_away(-5, 10), -1);
        assert_eq!(div_round_half_away(-4, 10), 0);
    }

    #[test]
    fn later_duplicate_dates_win() {
        let table = RateTable::new([
            rate("2026-01-01", "1.00", "1.00"),
            rate("2026-01-01", "2.00", "2.00"),
        ]);
        assert_eq!(table.latest().unwrap().usd_per_ounce(), usd("2.00"));
        assert_eq!(table.rates.len(), 1);
    }

    #[test]
    fn reminds_when_rates_are_missing_or_old() {
        assert!(RateTable::default().needs_update(date("2026-10-10")));
        let table = RateTable::new([rate("2026-09-10", "1.00", "1.00")]);
        assert!(!table.needs_update(date("2026-10-10")));
        assert!(table.needs_update(date("2026-10-11")));
    }

    fn any_currency() -> impl Strategy<Value = Currency> {
        prop::sample::select(Currency::ALL.to_vec())
    }

    fn any_date() -> impl Strategy<Value = Date> {
        (0i64..3_000_000).prop_map(|n| Date::MIN.add_days(n).unwrap())
    }

    fn any_rate() -> impl Strategy<Value = ExchangeRate> {
        (any_date(), 1i64..=i64::MAX, 1i64..=i64::MAX).prop_map(|(d, gold, btc)| {
            ExchangeRate::new(
                d,
                Amount::new(Currency::Usd, gold),
                Amount::new(Currency::Usd, btc),
            )
            .unwrap()
        })
    }

    proptest! {
        #[test]
        fn decimal_text_round_trips(c in any_currency(), units in 0i64..=i64::MAX) {
            let amount = Amount::new(c, units);
            prop_assert_eq!(Amount::parse(c, &amount.to_decimal_string()), Ok(amount));
        }

        #[test]
        fn parsing_never_panics(c in any_currency(), chars in prop::collection::vec(
            prop_oneof![any::<char>(), prop::sample::select(vec!['0', '9', '.', '-', ' '])],
            0..40,
        )) {
            let text: String = chars.into_iter().collect();
            let _ = Amount::parse(c, &text);
        }

        #[test]
        fn conversion_never_panics(
            units in any::<i64>(),
            from in any_currency(),
            to in any_currency(),
            r in any_rate(),
        ) {
            let table = RateTable::new([r]);
            match table.convert_at_latest(Amount::new(from, units), to) {
                Ok(a) => prop_assert_eq!(a.currency(), to),
                Err(e) => prop_assert_eq!(e, ConversionError::Overflow),
            }
        }

        #[test]
        fn conversion_preserves_order(
            a in 0i64..1_000_000_000_000,
            b in 0i64..1_000_000_000_000,
            from in any_currency(),
            to in any_currency(),
            gold in 1i64..1_000_000_000,
            btc in 1i64..1_000_000_000_000,
        ) {
            let r = ExchangeRate::new(
                Date::MIN,
                Amount::new(Currency::Usd, gold),
                Amount::new(Currency::Usd, btc),
            ).unwrap();
            let table = RateTable::new([r]);
            let x = table.convert_at_latest(Amount::new(from, a), to).unwrap();
            let y = table.convert_at_latest(Amount::new(from, b), to).unwrap();
            if a <= b {
                prop_assert!(x.units() <= y.units());
            }
        }

        #[test]
        fn rate_lookup_never_uses_a_later_rate(
            rates in prop::collection::vec(any_rate(), 0..20),
            on in any_date(),
        ) {
            let table = RateTable::new(rates.clone());
            match table.rate_on(on) {
                Some(r) => {
                    prop_assert!(r.date() <= on);
                    prop_assert!(!rates.iter().any(|x| x.date() > r.date() && x.date() <= on));
                }
                None => prop_assert!(rates.iter().all(|x| x.date() > on)),
            }
        }
    }
}
