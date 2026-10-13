//! How values inside saved filter rules are written as JSON: each in the
//! form a person would read and the app already parses, so a saved shelf
//! keeps working however the types behind it change.

use serde::de::{self, Deserializer};
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};

use crate::date::Date;
use crate::issue_number::IssueNumber;
use crate::model::Grade;
use crate::money::{Amount, Currency};

/// Writes a value as text and reads it back through `parse`.
macro_rules! as_text {
    ($type:ty, $what:literal, $write:expr, $parse:expr) => {
        impl Serialize for $type {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let write: fn(&$type) -> String = $write;
                s.serialize_str(&write(self))
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let text = String::deserialize(d)?;
                let parse: fn(&str) -> Option<$type> = $parse;
                parse(&text).ok_or_else(|| {
                    de::Error::custom(format!(concat!("invalid ", $what, ": {:?}"), text))
                })
            }
        }
    };
}

as_text!(Grade, "grade", |g| g.to_string(), Grade::parse);
as_text!(Date, "date", |d| d.to_string(), |t| t.parse().ok());
as_text!(
    IssueNumber,
    "issue number",
    |n| n.as_str().to_owned(),
    |t| IssueNumber::new(t).ok()
);
as_text!(
    Currency,
    "currency",
    |c| c.code().to_owned(),
    Currency::from_code
);

/// An amount as its currency and its count of the currency's smallest
/// unit, as stored everywhere else, so no float is ever involved.
impl Serialize for Amount {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut out = s.serialize_struct("Amount", 2)?;
        out.serialize_field("currency", &self.currency())?;
        out.serialize_field("units", &self.units())?;
        out.end()
    }
}

impl<'de> Deserialize<'de> for Amount {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Stored {
            currency: Currency,
            units: i64,
        }
        let stored = Stored::deserialize(d)?;
        if stored.units < 0 {
            return Err(de::Error::custom("amounts are never negative"));
        }
        Ok(Self::new(stored.currency, stored.units))
    }
}
