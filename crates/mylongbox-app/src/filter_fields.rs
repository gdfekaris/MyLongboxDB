//! What the filter panel offers and how a row of it becomes a filter:
//! the fields, the conditions each allows, the values each condition
//! asks for, and reading those values. No widgets here, so all of it
//! can be tested directly.

use mylongbox_core::{
    Amount, Binding, BookCondition, Category, Choice, Compare, Currency, Date, DateCondition,
    DateField, Distribution, Filter, Flag, Grade, GradeType, IssueNumber, ListCondition, ListField,
    MagazineFormat, MoneyField, NumberField, Presence, Role, SeriesId, TextCondition, TextField,
    Verification,
};

use crate::format;

/// The kinds of choice field, each with its own options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceKind {
    Category,
    MagazineFormat,
    Distribution,
    Binding,
    GradeType,
    BookCondition,
    DustJacketCondition,
    Verification,
}

/// What a field is, which decides its conditions and values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text(TextField),
    Person,
    List(ListField),
    IssueNumber,
    Grade,
    Number(NumberField),
    Money(MoneyField),
    Date(DateField),
    Choice(ChoiceKind),
    Flag(Flag),
    Has(Presence),
    Collects,
}

pub struct Field {
    pub label: &'static str,
    pub kind: Kind,
}

const fn field(label: &'static str, kind: Kind) -> Field {
    Field { label, kind }
}

/// Every field the panel offers, the most used first.
pub const FIELDS: &[Field] = &[
    field("Series", Kind::Text(TextField::SeriesName)),
    field("Issue Number", Kind::IssueNumber),
    field("Story Title", Kind::Text(TextField::Title)),
    field("Publisher", Kind::Text(TextField::Publisher)),
    field("Creator", Kind::Person),
    field("Characters", Kind::List(ListField::Characters)),
    field("Teams", Kind::List(ListField::Teams)),
    field("Story Arcs", Kind::List(ListField::StoryArcs)),
    field("Tags", Kind::List(ListField::Tags)),
    field("Category", Kind::Choice(ChoiceKind::Category)),
    field("Grade", Kind::Grade),
    field("Grade Type", Kind::Choice(ChoiceKind::GradeType)),
    field("Key Issue", Kind::Flag(Flag::KeyIssue)),
    field("First Appearance", Kind::Flag(Flag::FirstAppearance)),
    field("Cover Date", Kind::Date(DateField::CoverDate)),
    field("Release Date", Kind::Date(DateField::ReleaseDate)),
    field("Current Value", Kind::Money(MoneyField::CurrentValue)),
    field("Purchase Price", Kind::Money(MoneyField::PurchasePrice)),
    field("Purchase Date", Kind::Date(DateField::PurchaseDate)),
    field("Cover Price", Kind::Money(MoneyField::CoverPrice)),
    field("Storage Box", Kind::Text(TextField::StorageBox)),
    field("Box Position", Kind::Number(NumberField::BoxPosition)),
    field("Variant", Kind::Text(TextField::Variant)),
    field("Printing", Kind::Number(NumberField::Printing)),
    field("Distribution", Kind::Choice(ChoiceKind::Distribution)),
    field("Cover Artists", Kind::List(ListField::CoverArtists)),
    field("Signed", Kind::Flag(Flag::Signed)),
    field("Signed By", Kind::List(ListField::SignedBy)),
    field(
        "Signature Verification",
        Kind::Choice(ChoiceKind::Verification),
    ),
    field("Sketch", Kind::Flag(Flag::Sketch)),
    field("Certificate Number", Kind::Text(TextField::CertNumber)),
    field("Label", Kind::Text(TextField::LabelType)),
    field("Seller", Kind::Text(TextField::Seller)),
    field("Notes", Kind::Text(TextField::Notes)),
    field("Synopsis", Kind::Text(TextField::Synopsis)),
    field("Key Reason", Kind::Text(TextField::KeyReason)),
    field("Page Count", Kind::Number(NumberField::PageCount)),
    field("Volume", Kind::Number(NumberField::Volume)),
    field("Start Year", Kind::Number(NumberField::StartYear)),
    field("Magazine Format", Kind::Choice(ChoiceKind::MagazineFormat)),
    field("Black and White", Kind::Flag(Flag::BlackAndWhite)),
    field("Binding", Kind::Choice(ChoiceKind::Binding)),
    field("ISBN", Kind::Text(TextField::Isbn)),
    field("Barcode", Kind::Text(TextField::Barcode)),
    field("Book Condition", Kind::Choice(ChoiceKind::BookCondition)),
    field("Dust Jacket", Kind::Flag(Flag::DustJacket)),
    field(
        "Dust Jacket Condition",
        Kind::Choice(ChoiceKind::DustJacketCondition),
    ),
    field("Slipcase", Kind::Flag(Flag::Slipcase)),
    field("Collects", Kind::Collects),
    field("Front Cover Photo", Kind::Has(Presence::FrontPhoto)),
    field("Back Cover Photo", Kind::Has(Presence::BackPhoto)),
    field("Value Entry", Kind::Has(Presence::Value)),
    field("Grade or Condition", Kind::Has(Presence::Grade)),
    field("Any Storage Box", Kind::Has(Presence::StorageBox)),
    field("Any Purchase Price", Kind::Has(Presence::PurchasePrice)),
    field("Any Purchase Date", Kind::Has(Presence::PurchaseDate)),
    field("Any Cover Price", Kind::Has(Presence::CoverPrice)),
    field("Any Cover Date", Kind::Has(Presence::CoverDate)),
    field("Any Release Date", Kind::Has(Presence::ReleaseDate)),
    field("Any Tags", Kind::Has(Presence::Tags)),
    field("Any Notes", Kind::Has(Presence::Notes)),
    field("Date Added", Kind::Date(DateField::Added)),
];

/// The values a condition asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Nothing more, as for "has none".
    Nothing,
    /// Text boxes, with these placeholders.
    Entries(Vec<&'static str>),
    /// Text boxes and a menu of these options.
    EntriesAndMenu(Vec<&'static str>, Vec<&'static str>),
    /// A menu of these options alone.
    Menu(Vec<&'static str>),
    /// Any number of these options, ticked.
    Checks(Vec<&'static str>),
}

const COMPARE: &[&str] = &["is", "is at least", "is at most", "is between"];
const CURRENCIES: [(&str, Currency); 3] = [
    ("USD", Currency::Usd),
    ("oz gold", Currency::Gold),
    ("BTC", Currency::Btc),
];

impl Kind {
    pub fn conditions(self) -> &'static [&'static str] {
        match self {
            Self::Text(_) => &["contains", "is", "is similar to"],
            Self::Person => &["is", "is, as"],
            Self::List(_) => &["include any of", "include all of", "exclude"],
            Self::IssueNumber | Self::Grade | Self::Number(_) | Self::Money(_) => COMPARE,
            Self::Date(_) => &["is before", "is after", "is between", "is in year"],
            Self::Choice(_) => &["is one of"],
            Self::Flag(_) => &["is"],
            Self::Has(_) => &["is present", "is missing"],
            Self::Collects => &["includes"],
        }
    }

    pub fn shape(self, condition: usize) -> Shape {
        let entries = |v: &[&'static str]| Shape::Entries(v.to_vec());
        // The last of the comparisons.
        let between = condition == 3;
        match self {
            Self::Text(_) => entries(&["Text"]),
            Self::Person if condition == 1 => Shape::EntriesAndMenu(
                vec!["Name"],
                Role::ALL.iter().map(|r| format::role(*r)).collect(),
            ),
            Self::Person => entries(&["Name"]),
            Self::List(_) => entries(&["Names, separated by commas"]),
            Self::IssueNumber | Self::Grade | Self::Number(_) if between => {
                entries(&["From", "To"])
            }
            Self::IssueNumber => entries(&["Number"]),
            Self::Grade => entries(&["Grade, e.g. 9.4"]),
            Self::Number(_) => entries(&["Number"]),
            Self::Money(_) => Shape::EntriesAndMenu(
                if between {
                    vec!["From", "To"]
                } else {
                    vec!["Amount"]
                },
                CURRENCIES.iter().map(|(name, _)| *name).collect(),
            ),
            // Before, after, between, in year.
            Self::Date(_) => match condition {
                2 => entries(&["From", "To"]),
                3 => entries(&["Year"]),
                _ => entries(&["YYYY-MM-DD, YYYY-MM, or YYYY"]),
            },
            Self::Choice(kind) => Shape::Checks(kind.labels()),
            Self::Flag(_) => Shape::Menu(vec!["Yes", "No"]),
            Self::Has(_) => Shape::Nothing,
            Self::Collects => entries(&["Series", "From number", "To number"]),
        }
    }
}

impl ChoiceKind {
    pub fn labels(self) -> Vec<&'static str> {
        match self {
            Self::Category => vec!["Comic", "Magazine", "Collected Edition"],
            Self::MagazineFormat => MagazineFormat::ALL
                .iter()
                .map(|f| format::magazine_format(*f))
                .collect(),
            Self::Distribution => Distribution::ALL
                .iter()
                .map(|d| format::distribution(*d))
                .collect(),
            Self::Binding => Binding::ALL.iter().map(|b| format::binding(*b)).collect(),
            Self::GradeType => vec!["Raw", "CGC", "CBCS", "PGX"],
            Self::BookCondition | Self::DustJacketCondition => BookCondition::ALL
                .iter()
                .map(|c| format::condition(*c))
                .collect(),
            Self::Verification => Verification::ALL
                .iter()
                .map(|v| format::verification(*v))
                .collect(),
        }
    }

    fn choice(self, picked: &[usize]) -> Choice {
        fn pick<T: Copy>(all: &[T], picked: &[usize]) -> Vec<T> {
            picked.iter().filter_map(|i| all.get(*i).copied()).collect()
        }
        match self {
            Self::Category => Choice::Category(pick(Category::ALL, picked)),
            Self::MagazineFormat => Choice::MagazineFormat(pick(MagazineFormat::ALL, picked)),
            Self::Distribution => Choice::Distribution(pick(Distribution::ALL, picked)),
            Self::Binding => Choice::Binding(pick(Binding::ALL, picked)),
            Self::GradeType => Choice::GradeType(pick(GradeType::ALL, picked)),
            Self::BookCondition => Choice::BookCondition(pick(BookCondition::ALL, picked)),
            Self::DustJacketCondition => {
                Choice::DustJacketCondition(pick(BookCondition::ALL, picked))
            }
            Self::Verification => Choice::Verification(pick(Verification::ALL, picked)),
        }
    }
}

/// What has been entered in a row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inputs {
    pub texts: Vec<String>,
    /// The option chosen in the row's menu.
    pub menu: usize,
    /// The options ticked.
    pub checks: Vec<usize>,
}

impl Inputs {
    fn text(&self, n: usize) -> &str {
        self.texts.get(n).map_or("", |t| t.trim())
    }
}

/// Which end of a period a partial date stands for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    First,
    Last,
}

/// A date typed in full, or as a month or a year standing for its first
/// or last day.
fn date_bound(text: &str, end: End) -> Option<Date> {
    let text = text.trim();
    if let Ok(date) = text.parse::<Date>() {
        return Some(date);
    }
    let mut parts = text.splitn(2, '-');
    let year: u16 = parts.next()?.parse().ok()?;
    let month: Option<u8> = match parts.next() {
        Some(m) => Some(m.parse().ok()?),
        None => None,
    };
    let (first_month, last_month) = month.map_or((1, 12), |m| (m, m));
    match end {
        End::First => Date::new(year, first_month, 1).ok(),
        End::Last => (28..=31)
            .rev()
            .find_map(|day| Date::new(year, last_month, day).ok()),
    }
}

/// An amount as typed, with its currency's sign and thousands commas
/// allowed.
fn amount(text: &str, currency: Currency) -> Option<Amount> {
    let cleaned: String = text
        .trim()
        .trim_start_matches('$')
        .chars()
        .filter(|c| *c != ',')
        .collect();
    Amount::parse(currency, &cleaned).ok()
}

/// Names from a list box, split at commas or semicolons.
fn names(text: &str) -> Vec<String> {
    text.split([',', ';'])
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A comparison from one or two values, or `None` if they do not read.
fn compare<T>(
    condition: usize,
    inputs: &Inputs,
    read: impl Fn(&str) -> Option<T>,
) -> Option<Compare<T>> {
    let first = read(inputs.text(0));
    Some(match condition {
        0 => Compare::Equal(first?),
        1 => Compare::AtLeast(first?),
        2 => Compare::AtMost(first?),
        _ => Compare::Between(first?, read(inputs.text(1))?),
    })
}

/// The filter a row stands for, or `None` while it is incomplete or its
/// values do not read, so a half-typed row changes nothing. `series`
/// finds a series by the label it is listed under.
pub fn build(
    kind: Kind,
    condition: usize,
    inputs: &Inputs,
    series: &dyn Fn(&str) -> Option<SeriesId>,
) -> Option<Filter> {
    let nonempty = |n: usize| Some(inputs.text(n)).filter(|t| !t.is_empty());
    Some(match kind {
        Kind::Text(field) => Filter::Text {
            field,
            condition: match condition {
                0 => TextCondition::Contains,
                1 => TextCondition::Is,
                _ => TextCondition::SimilarTo,
            },
            value: nonempty(0)?.to_owned(),
        },
        Kind::Person => Filter::Person {
            name: nonempty(0)?.to_owned(),
            role: if condition == 1 {
                Some(*Role::ALL.get(inputs.menu)?)
            } else {
                None
            },
        },
        Kind::List(field) => {
            let names = names(inputs.text(0));
            if names.is_empty() {
                return None;
            }
            Filter::List {
                field,
                condition: match condition {
                    0 => ListCondition::AnyOf,
                    1 => ListCondition::AllOf,
                    _ => ListCondition::NoneOf,
                },
                names,
            }
        }
        Kind::IssueNumber => {
            Filter::IssueNumber(compare(condition, inputs, |t| IssueNumber::new(t).ok())?)
        }
        Kind::Grade => Filter::Grade(compare(condition, inputs, Grade::parse)?),
        Kind::Number(field) => Filter::Number {
            field,
            compare: compare(condition, inputs, |t| t.parse().ok())?,
        },
        Kind::Money(field) => {
            let currency = CURRENCIES.get(inputs.menu)?.1;
            Filter::Money {
                field,
                compare: compare(condition, inputs, |t| amount(t, currency))?,
            }
        }
        Kind::Date(field) => Filter::Date {
            field,
            condition: match condition {
                0 => DateCondition::Before(date_bound(inputs.text(0), End::First)?),
                1 => DateCondition::After(date_bound(inputs.text(0), End::Last)?),
                2 => DateCondition::Between(
                    date_bound(inputs.text(0), End::First)?,
                    date_bound(inputs.text(1), End::Last)?,
                ),
                _ => DateCondition::InYear(inputs.text(0).parse().ok().filter(|y| *y > 0)?),
            },
        },
        Kind::Choice(kind) => {
            if inputs.checks.is_empty() {
                return None;
            }
            Filter::Choice(kind.choice(&inputs.checks))
        }
        Kind::Flag(flag) => Filter::Flag {
            flag,
            value: inputs.menu == 0,
        },
        Kind::Has(what) => Filter::Has {
            what,
            has: condition == 0,
        },
        Kind::Collects => {
            let number = |n: usize| -> Option<Option<IssueNumber>> {
                match nonempty(n) {
                    None => Some(None),
                    Some(t) => IssueNumber::new(t).ok().map(Some),
                }
            };
            Filter::Collects {
                series: series(nonempty(0)?)?,
                from: number(1)?,
                to: number(2)?,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(texts: &[&str]) -> Inputs {
        Inputs {
            texts: texts.iter().map(|t| (*t).to_owned()).collect(),
            ..Inputs::default()
        }
    }

    fn no_series(_: &str) -> Option<SeriesId> {
        None
    }

    fn kind_of(label: &str) -> Kind {
        FIELDS.iter().find(|f| f.label == label).unwrap().kind
    }

    #[test]
    fn dates_ask_for_two_values_only_between() {
        let cover = kind_of("Cover Date");
        let count = |c| match cover.shape(c) {
            Shape::Entries(v) => v.len(),
            _ => 0,
        };
        assert_eq!([count(0), count(1), count(2), count(3)], [1, 1, 2, 1]);
        let grade = kind_of("Grade");
        assert!(matches!(grade.shape(3), Shape::Entries(v) if v.len() == 2));
    }

    #[test]
    fn every_field_has_conditions_and_a_shape_for_each() {
        let mut labels = std::collections::HashSet::new();
        for f in FIELDS {
            assert!(labels.insert(f.label), "{} twice", f.label);
            assert!(!f.kind.conditions().is_empty());
            for c in 0..f.kind.conditions().len() {
                let _ = f.kind.shape(c);
            }
        }
    }

    #[test]
    fn half_typed_rows_change_nothing() {
        for f in FIELDS {
            for c in 0..f.kind.conditions().len() {
                let built = build(f.kind, c, &Inputs::default(), &no_series);
                // Only rows that need no typing are complete when empty.
                let complete = matches!(f.kind, Kind::Has(_) | Kind::Flag(_));
                assert_eq!(built.is_some(), complete, "{} {c}", f.label);
            }
        }
        let grade = kind_of("Grade");
        assert_eq!(build(grade, 0, &typed(&["nine"]), &no_series), None);
        assert_eq!(build(grade, 3, &typed(&["9.0"]), &no_series), None);
    }

    #[test]
    fn rows_read_their_values() {
        let s = |t: &str| t.to_owned();
        assert_eq!(
            build(
                kind_of("Series"),
                2,
                &typed(&[" Amazng Spider-Man "]),
                &no_series
            ),
            Some(Filter::Text {
                field: TextField::SeriesName,
                condition: TextCondition::SimilarTo,
                value: s("Amazng Spider-Man"),
            })
        );
        let penciller = Role::ALL
            .iter()
            .position(|r| *r == Role::Penciller)
            .unwrap();
        assert_eq!(
            build(
                kind_of("Creator"),
                1,
                &Inputs {
                    texts: vec![s("Jack Kirby")],
                    menu: penciller,
                    checks: vec![],
                },
                &no_series
            ),
            Some(Filter::Person {
                name: s("Jack Kirby"),
                role: Some(Role::Penciller),
            })
        );
        assert_eq!(
            build(
                kind_of("Characters"),
                1,
                &typed(&["Wolverine, Hulk; "]),
                &no_series
            ),
            Some(Filter::List {
                field: ListField::Characters,
                condition: ListCondition::AllOf,
                names: vec![s("Wolverine"), s("Hulk")],
            })
        );
        assert_eq!(
            build(kind_of("Grade"), 3, &typed(&["9.0", "9.8"]), &no_series),
            Some(Filter::Grade(Compare::Between(
                Grade::parse("9.0").unwrap(),
                Grade::parse("9.8").unwrap()
            )))
        );
        assert_eq!(
            build(
                kind_of("Current Value"),
                1,
                &typed(&["$1,250.50"]),
                &no_series
            ),
            Some(Filter::Money {
                field: MoneyField::CurrentValue,
                compare: Compare::AtLeast(Amount::parse(Currency::Usd, "1250.50").unwrap()),
            })
        );
        let gold = Inputs {
            texts: vec![s("0.5")],
            menu: 1,
            checks: vec![],
        };
        assert_eq!(
            build(kind_of("Purchase Price"), 2, &gold, &no_series),
            Some(Filter::Money {
                field: MoneyField::PurchasePrice,
                compare: Compare::AtMost(Amount::parse(Currency::Gold, "0.5").unwrap()),
            })
        );
        let picked = Inputs {
            checks: vec![1, 2],
            ..Inputs::default()
        };
        assert_eq!(
            build(kind_of("Grade Type"), 0, &picked, &no_series),
            Some(Filter::Choice(Choice::GradeType(vec![
                GradeType::Cgc,
                GradeType::Cbcs
            ])))
        );
        let no = Inputs {
            menu: 1,
            ..Inputs::default()
        };
        assert_eq!(
            build(kind_of("Key Issue"), 0, &no, &no_series),
            Some(Filter::Flag {
                flag: Flag::KeyIssue,
                value: false,
            })
        );
        assert_eq!(
            build(
                kind_of("Back Cover Photo"),
                1,
                &Inputs::default(),
                &no_series
            ),
            Some(Filter::Has {
                what: Presence::BackPhoto,
                has: false,
            })
        );
    }

    #[test]
    fn partial_dates_stand_for_their_first_or_last_day() {
        let d = |t: &str| t.parse::<Date>().unwrap();
        let cover = kind_of("Cover Date");
        assert_eq!(
            build(cover, 2, &typed(&["1975", "1985"]), &no_series),
            Some(Filter::Date {
                field: DateField::CoverDate,
                condition: DateCondition::Between(d("1975-01-01"), d("1985-12-31")),
            })
        );
        assert_eq!(
            build(cover, 0, &typed(&["1975-06"]), &no_series),
            Some(Filter::Date {
                field: DateField::CoverDate,
                condition: DateCondition::Before(d("1975-06-01")),
            })
        );
        assert_eq!(
            build(cover, 1, &typed(&["2024-02"]), &no_series),
            Some(Filter::Date {
                field: DateField::CoverDate,
                condition: DateCondition::After(d("2024-02-29")),
            })
        );
        assert_eq!(
            build(cover, 1, &typed(&["1999-12-25"]), &no_series),
            Some(Filter::Date {
                field: DateField::CoverDate,
                condition: DateCondition::After(d("1999-12-25")),
            })
        );
        assert_eq!(
            build(cover, 3, &typed(&["1974"]), &no_series),
            Some(Filter::Date {
                field: DateField::CoverDate,
                condition: DateCondition::InYear(1974),
            })
        );
        for bad in ["", "June", "1975-13", "0", "1975-06-31"] {
            assert_eq!(build(cover, 0, &typed(&[bad]), &no_series), None, "{bad}");
        }
        for bad in ["0", "nineteen", "-4", ""] {
            assert_eq!(build(cover, 3, &typed(&[bad]), &no_series), None, "{bad}");
        }
    }

    #[test]
    fn collects_rows_name_a_listed_series() {
        let db = mylongbox_core::Database::open_in_memory().unwrap();
        let ff = db
            .create_series(&mylongbox_core::SeriesFields::new(
                "Fantastic Four",
                Category::Comic,
            ))
            .unwrap();
        let collects = kind_of("Collects");
        let known = |label: &str| (label == "Fantastic Four (1961)").then_some(ff);
        assert_eq!(
            build(
                collects,
                0,
                &typed(&["Fantastic Four (1961)", "48", ""]),
                &known
            ),
            Some(Filter::Collects {
                series: ff,
                from: IssueNumber::new("48").ok(),
                to: None,
            })
        );
        assert_eq!(
            build(collects, 0, &typed(&["Nothing", "", ""]), &known),
            None
        );
        assert_eq!(
            build(
                collects,
                0,
                &typed(&["Fantastic Four (1961)", "x y z ~", ""]),
                &known
            )
            .is_some(),
            IssueNumber::new("x y z ~").is_ok()
        );
    }
}
