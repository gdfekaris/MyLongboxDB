//! What the filter panel offers and how a row of it becomes a filter:
//! the fields, the conditions each allows, the values each condition
//! asks for, and reading those values. No widgets here, so all of it
//! can be tested directly.

use mylongbox_core::{
    Amount, Binding, BookCondition, Category, Choice, Compare, Currency, Date, DateCondition,
    DateField, Distribution, Filter, Flag, Grade, GradeType, IssueNumber, ListCondition, ListField,
    MagazineFormat, MoneyField, NumberField, PageQuality, Presence, Role, SeriesId,
    SignaturePlacement, TextCondition, TextField, Verification,
};

use crate::format;
use crate::i18n::gettext_noop;

/// The kinds of choice field, each with its own options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceKind {
    Category,
    MagazineFormat,
    Distribution,
    Binding,
    GradeType,
    PageQuality,
    BookCondition,
    DustJacketCondition,
    Verification,
    SignaturePlacement,
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

/// Every field the panel offers, the most used first. Labels are
/// translated where they are shown.
pub const FIELDS: &[Field] = &[
    field(gettext_noop("Series"), Kind::Text(TextField::SeriesName)),
    field(gettext_noop("Issue Number"), Kind::IssueNumber),
    field(gettext_noop("Story Title"), Kind::Text(TextField::Title)),
    field(gettext_noop("Publisher"), Kind::Text(TextField::Publisher)),
    field(gettext_noop("Imprint"), Kind::Text(TextField::Imprint)),
    field(gettext_noop("Creator"), Kind::Person),
    field(
        gettext_noop("Characters"),
        Kind::List(ListField::Characters),
    ),
    field(gettext_noop("Teams"), Kind::List(ListField::Teams)),
    field(gettext_noop("Story Arcs"), Kind::List(ListField::StoryArcs)),
    field(gettext_noop("Tags"), Kind::List(ListField::Tags)),
    field(gettext_noop("Category"), Kind::Choice(ChoiceKind::Category)),
    field(gettext_noop("Grade"), Kind::Grade),
    field(
        gettext_noop("Grade Type"),
        Kind::Choice(ChoiceKind::GradeType),
    ),
    field(
        gettext_noop("Page Quality"),
        Kind::Choice(ChoiceKind::PageQuality),
    ),
    field(gettext_noop("Key Issue"), Kind::Flag(Flag::KeyIssue)),
    field(
        gettext_noop("First Appearance"),
        Kind::Flag(Flag::FirstAppearance),
    ),
    field(gettext_noop("Cover Date"), Kind::Date(DateField::CoverDate)),
    field(
        gettext_noop("Release Date"),
        Kind::Date(DateField::ReleaseDate),
    ),
    field(
        gettext_noop("Current Value"),
        Kind::Money(MoneyField::CurrentValue),
    ),
    field(
        gettext_noop("Purchase Price"),
        Kind::Money(MoneyField::PurchasePrice),
    ),
    field(
        gettext_noop("Purchase Date"),
        Kind::Date(DateField::PurchaseDate),
    ),
    field(
        gettext_noop("Cover Price"),
        Kind::Money(MoneyField::CoverPrice),
    ),
    field(
        gettext_noop("Storage Box"),
        Kind::Text(TextField::StorageBox),
    ),
    field(
        gettext_noop("Box Position"),
        Kind::Number(NumberField::BoxPosition),
    ),
    field(gettext_noop("Variant"), Kind::Text(TextField::Variant)),
    field(
        gettext_noop("Printing"),
        Kind::Number(NumberField::Printing),
    ),
    field(
        gettext_noop("Distribution"),
        Kind::Choice(ChoiceKind::Distribution),
    ),
    field(
        gettext_noop("Cover Artists"),
        Kind::List(ListField::CoverArtists),
    ),
    field(gettext_noop("Signed"), Kind::Flag(Flag::Signed)),
    field(gettext_noop("Signed By"), Kind::List(ListField::SignedBy)),
    field(
        gettext_noop("Signature Verification"),
        Kind::Choice(ChoiceKind::Verification),
    ),
    field(
        gettext_noop("Signature Placement"),
        Kind::Choice(ChoiceKind::SignaturePlacement),
    ),
    field(gettext_noop("Sketch"), Kind::Flag(Flag::Sketch)),
    field(
        gettext_noop("Certificate Number"),
        Kind::Text(TextField::CertNumber),
    ),
    field(gettext_noop("Label"), Kind::Text(TextField::LabelType)),
    field(gettext_noop("Seller"), Kind::Text(TextField::Seller)),
    field(
        gettext_noop("Notes on This Copy"),
        Kind::Text(TextField::Notes),
    ),
    field(
        gettext_noop("Notes About This Issue"),
        Kind::Text(TextField::Synopsis),
    ),
    field(gettext_noop("Key Reason"), Kind::Text(TextField::KeyReason)),
    field(
        gettext_noop("Legacy Number"),
        Kind::Number(NumberField::LegacyNumber),
    ),
    field(
        gettext_noop("Page Count"),
        Kind::Number(NumberField::PageCount),
    ),
    field(gettext_noop("Volume"), Kind::Number(NumberField::Volume)),
    field(
        gettext_noop("Start Year"),
        Kind::Number(NumberField::StartYear),
    ),
    field(
        gettext_noop("Magazine Format"),
        Kind::Choice(ChoiceKind::MagazineFormat),
    ),
    field(
        gettext_noop("Black and White"),
        Kind::Flag(Flag::BlackAndWhite),
    ),
    field(gettext_noop("Binding"), Kind::Choice(ChoiceKind::Binding)),
    field(gettext_noop("ISBN"), Kind::Text(TextField::Isbn)),
    field(gettext_noop("Barcode"), Kind::Text(TextField::Barcode)),
    field(
        gettext_noop("Book Condition"),
        Kind::Choice(ChoiceKind::BookCondition),
    ),
    field(gettext_noop("Dust Jacket"), Kind::Flag(Flag::DustJacket)),
    field(
        gettext_noop("Dust Jacket Condition"),
        Kind::Choice(ChoiceKind::DustJacketCondition),
    ),
    field(gettext_noop("Slipcase"), Kind::Flag(Flag::Slipcase)),
    field(gettext_noop("Collects"), Kind::Collects),
    field(
        gettext_noop("Front Cover Photo"),
        Kind::Has(Presence::FrontPhoto),
    ),
    field(
        gettext_noop("Back Cover Photo"),
        Kind::Has(Presence::BackPhoto),
    ),
    field(
        gettext_noop("Signature Photo"),
        Kind::Has(Presence::SignaturePhoto),
    ),
    field(gettext_noop("Value Entry"), Kind::Has(Presence::Value)),
    field(
        gettext_noop("Grade or Condition"),
        Kind::Has(Presence::Grade),
    ),
    field(
        gettext_noop("Any Storage Box"),
        Kind::Has(Presence::StorageBox),
    ),
    field(
        gettext_noop("Any Purchase Price"),
        Kind::Has(Presence::PurchasePrice),
    ),
    field(
        gettext_noop("Any Purchase Date"),
        Kind::Has(Presence::PurchaseDate),
    ),
    field(
        gettext_noop("Any Cover Price"),
        Kind::Has(Presence::CoverPrice),
    ),
    field(
        gettext_noop("Any Cover Date"),
        Kind::Has(Presence::CoverDate),
    ),
    field(
        gettext_noop("Any Release Date"),
        Kind::Has(Presence::ReleaseDate),
    ),
    field(gettext_noop("Any Tags"), Kind::Has(Presence::Tags)),
    field(
        gettext_noop("Any Notes on This Copy"),
        Kind::Has(Presence::Notes),
    ),
    field(gettext_noop("Date Added"), Kind::Date(DateField::Added)),
];

/// The values a condition asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Nothing more, as for "has none".
    Nothing,
    /// Text boxes, with these placeholders, in English to look up.
    Entries(Vec<&'static str>),
    /// Text boxes and a menu of these options. Options, here and below,
    /// are already in the reader's language.
    EntriesAndMenu(Vec<&'static str>, Vec<&'static str>),
    /// A menu of these options alone.
    Menu(Vec<&'static str>),
    /// Any number of these options, ticked.
    Checks(Vec<&'static str>),
}

// Translators: conditions in a filter row, read as field, condition,
// value: "Grade is at least 9.4".
const COMPARE: &[&str] = &[
    gettext_noop("is"),
    gettext_noop("is at least"),
    gettext_noop("is at most"),
    gettext_noop("is between"),
];
const CURRENCIES: [(&str, Currency); 3] = [
    ("USD", Currency::Usd),
    // Translators: the currency menu beside an amount: gold counted in
    // troy ounces.
    (gettext_noop("oz gold"), Currency::Gold),
    ("BTC", Currency::Btc),
];

impl Kind {
    /// The conditions offered, untranslated; they are translated where
    /// they are shown.
    pub fn conditions(self) -> &'static [&'static str] {
        match self {
            // Translators: conditions in a filter row, read as field,
            // condition, value: "Series contains Thor".
            Self::Text(_) => {
                const {
                    &[
                        gettext_noop("contains"),
                        gettext_noop("is"),
                        gettext_noop("is similar to"),
                    ]
                }
            }
            // Translators: "Creator is, as" is followed by a name and a
            // role: "Creator is, as Kirby, Penciller".
            Self::Person => const { &[gettext_noop("is"), gettext_noop("is, as")] },
            // Translators: conditions for lists of names: "Characters
            // include any of Wolverine, Hulk".
            Self::List(_) => {
                const {
                    &[
                        gettext_noop("include any of"),
                        gettext_noop("include all of"),
                        gettext_noop("exclude"),
                    ]
                }
            }
            Self::IssueNumber | Self::Grade | Self::Number(_) | Self::Money(_) => COMPARE,
            Self::Date(_) => {
                const {
                    &[
                        gettext_noop("is before"),
                        gettext_noop("is after"),
                        gettext_noop("is between"),
                        gettext_noop("is in year"),
                    ]
                }
            }
            Self::Choice(_) => const { &[gettext_noop("is one of")] },
            Self::Flag(_) => const { &[gettext_noop("is")] },
            // Translators: whether a book has something at all, such as a
            // back cover photo.
            Self::Has(_) => const { &[gettext_noop("is present"), gettext_noop("is missing")] },
            // Translators: "Collects includes" is followed by a series and
            // a range of issue numbers.
            Self::Collects => const { &[gettext_noop("includes")] },
        }
    }

    pub fn shape(self, condition: usize) -> Shape {
        let entries = |v: &[&'static str]| Shape::Entries(v.to_vec());
        // The last of the comparisons.
        let between = condition == 3;
        match self {
            Self::Text(_) => entries(&[gettext_noop("Text")]),
            Self::Person if condition == 1 => Shape::EntriesAndMenu(
                vec![gettext_noop("Name")],
                Role::ALL.iter().map(|r| format::role(*r)).collect(),
            ),
            Self::Person => entries(&[gettext_noop("Name")]),
            Self::List(_) => entries(&[gettext_noop("Names, separated by commas")]),
            Self::IssueNumber | Self::Grade | Self::Number(_) if between => {
                entries(&[gettext_noop("From"), gettext_noop("To")])
            }
            Self::IssueNumber => entries(&[gettext_noop("Number")]),
            Self::Grade => entries(&[gettext_noop("Grade, e.g. 9.4")]),
            Self::Number(_) => entries(&[gettext_noop("Number")]),
            Self::Money(_) => Shape::EntriesAndMenu(
                if between {
                    vec![gettext_noop("From"), gettext_noop("To")]
                } else {
                    vec![gettext_noop("Amount")]
                },
                CURRENCIES
                    .iter()
                    .map(|(name, _)| format::label(name))
                    .collect(),
            ),
            // Before, after, between, in year.
            Self::Date(_) => match condition {
                2 => entries(&[gettext_noop("From"), gettext_noop("To")]),
                3 => entries(&[gettext_noop("Year")]),
                // Translators: how a date may be typed; the letters stand for
                // year, month, and day digits.
                _ => entries(&[gettext_noop("YYYY-MM-DD, YYYY-MM, or YYYY")]),
            },
            Self::Choice(kind) => Shape::Checks(kind.labels()),
            Self::Flag(_) => Shape::Menu(vec![
                format::label(gettext_noop("Yes")),
                format::label(gettext_noop("No")),
            ]),
            Self::Has(_) => Shape::Nothing,
            Self::Collects => entries(&[
                gettext_noop("Series"),
                gettext_noop("From number"),
                gettext_noop("To number"),
            ]),
        }
    }
}

impl ChoiceKind {
    pub fn labels(self) -> Vec<&'static str> {
        match self {
            Self::Category => vec![
                format::label(gettext_noop("Comic")),
                format::label(gettext_noop("Magazine")),
                format::label(gettext_noop("Collected Edition")),
            ],
            Self::MagazineFormat => MagazineFormat::ALL
                .iter()
                .map(|f| format::magazine_format(*f))
                .collect(),
            Self::Distribution => Distribution::ALL
                .iter()
                .map(|d| format::distribution(*d))
                .collect(),
            Self::Binding => Binding::ALL.iter().map(|b| format::binding(*b)).collect(),
            // Translators: an ungraded book, as opposed to one graded by
            // CGC, CBCS, or PGX.
            Self::GradeType => vec![format::label(gettext_noop("Raw")), "CGC", "CBCS", "PGX"],
            Self::PageQuality => PageQuality::ALL
                .iter()
                .map(|q| format::page_quality(*q))
                .collect(),
            Self::BookCondition | Self::DustJacketCondition => BookCondition::ALL
                .iter()
                .map(|c| format::condition(*c))
                .collect(),
            Self::Verification => Verification::ALL
                .iter()
                .map(|v| format::verification(*v))
                .collect(),
            Self::SignaturePlacement => SignaturePlacement::ALL
                .iter()
                .map(|p| format::signature_placement(*p))
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
            Self::PageQuality => Choice::PageQuality(pick(PageQuality::ALL, picked)),
            Self::BookCondition => Choice::BookCondition(pick(BookCondition::ALL, picked)),
            Self::DustJacketCondition => {
                Choice::DustJacketCondition(pick(BookCondition::ALL, picked))
            }
            Self::Verification => Choice::Verification(pick(Verification::ALL, picked)),
            Self::SignaturePlacement => {
                Choice::SignaturePlacement(pick(SignaturePlacement::ALL, picked))
            }
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

/// A filter as a row of the panel: the index of its field in [`FIELDS`],
/// its condition, and what to enter, so a saved shelf's rules can be
/// shown for editing. `series_label` names a series as the Collects
/// field lists it. `None` for a rule no field offers, or one whose series
/// is no longer listed.
pub fn unbuild(
    filter: &Filter,
    series_label: &dyn Fn(SeriesId) -> Option<String>,
) -> Option<(usize, usize, Inputs)> {
    fn compared<T>(c: &Compare<T>, write: impl Fn(&T) -> String) -> (usize, Vec<String>) {
        match c {
            Compare::Equal(a) => (0, vec![write(a)]),
            Compare::AtLeast(a) => (1, vec![write(a)]),
            Compare::AtMost(a) => (2, vec![write(a)]),
            Compare::Between(a, b) => (3, vec![write(a), write(b)]),
        }
    }
    fn picked<T: PartialEq>(all: &[T], chosen: &[T]) -> Vec<usize> {
        (0..all.len())
            .filter(|i| chosen.contains(&all[*i]))
            .collect()
    }
    let texts = |t: Vec<String>| Inputs {
        texts: t,
        ..Inputs::default()
    };
    let (kind, condition, inputs) = match filter {
        Filter::Text {
            field,
            condition,
            value,
        } => (
            Kind::Text(*field),
            match condition {
                TextCondition::Contains => 0,
                TextCondition::Is => 1,
                TextCondition::SimilarTo => 2,
            },
            texts(vec![value.clone()]),
        ),
        Filter::Person { name, role } => match role {
            None => (Kind::Person, 0, texts(vec![name.clone()])),
            Some(role) => (
                Kind::Person,
                1,
                Inputs {
                    texts: vec![name.clone()],
                    menu: Role::ALL.iter().position(|r| r == role)?,
                    checks: Vec::new(),
                },
            ),
        },
        Filter::List {
            field,
            condition,
            names,
        } => (
            Kind::List(*field),
            match condition {
                ListCondition::AnyOf => 0,
                ListCondition::AllOf => 1,
                ListCondition::NoneOf => 2,
            },
            texts(vec![names.join(", ")]),
        ),
        Filter::IssueNumber(c) => {
            let (condition, t) = compared(c, |n| n.as_str().to_owned());
            (Kind::IssueNumber, condition, texts(t))
        }
        Filter::Grade(c) => {
            let (condition, t) = compared(c, Grade::to_string);
            (Kind::Grade, condition, texts(t))
        }
        Filter::Number { field, compare } => {
            let (condition, t) = compared(compare, u32::to_string);
            (Kind::Number(*field), condition, texts(t))
        }
        Filter::Money { field, compare } => {
            let currency = match compare {
                Compare::Equal(a) | Compare::AtLeast(a) | Compare::AtMost(a) => a.currency(),
                Compare::Between(a, _) => a.currency(),
            };
            let (condition, t) = compared(compare, |a| a.to_decimal_string());
            (
                Kind::Money(*field),
                condition,
                Inputs {
                    texts: t,
                    menu: CURRENCIES.iter().position(|(_, c)| *c == currency)?,
                    checks: Vec::new(),
                },
            )
        }
        Filter::Date { field, condition } => {
            let (n, t) = match condition {
                DateCondition::Before(d) => (0, vec![d.to_string()]),
                DateCondition::After(d) => (1, vec![d.to_string()]),
                DateCondition::Between(a, b) => (2, vec![a.to_string(), b.to_string()]),
                DateCondition::InYear(y) => (3, vec![y.to_string()]),
            };
            (Kind::Date(*field), n, texts(t))
        }
        Filter::Choice(choice) => {
            let (kind, checks) = match choice {
                Choice::Category(v) => (ChoiceKind::Category, picked(Category::ALL, v)),
                Choice::MagazineFormat(v) => {
                    (ChoiceKind::MagazineFormat, picked(MagazineFormat::ALL, v))
                }
                Choice::Distribution(v) => (ChoiceKind::Distribution, picked(Distribution::ALL, v)),
                Choice::Binding(v) => (ChoiceKind::Binding, picked(Binding::ALL, v)),
                Choice::GradeType(v) => (ChoiceKind::GradeType, picked(GradeType::ALL, v)),
                Choice::PageQuality(v) => (ChoiceKind::PageQuality, picked(PageQuality::ALL, v)),
                Choice::BookCondition(v) => {
                    (ChoiceKind::BookCondition, picked(BookCondition::ALL, v))
                }
                Choice::DustJacketCondition(v) => (
                    ChoiceKind::DustJacketCondition,
                    picked(BookCondition::ALL, v),
                ),
                Choice::Verification(v) => (ChoiceKind::Verification, picked(Verification::ALL, v)),
                Choice::SignaturePlacement(v) => (
                    ChoiceKind::SignaturePlacement,
                    picked(SignaturePlacement::ALL, v),
                ),
            };
            (
                Kind::Choice(kind),
                0,
                Inputs {
                    checks,
                    ..Inputs::default()
                },
            )
        }
        Filter::Flag { flag, value } => (
            Kind::Flag(*flag),
            0,
            Inputs {
                menu: usize::from(!*value),
                ..Inputs::default()
            },
        ),
        Filter::Has { what, has } => (Kind::Has(*what), usize::from(!*has), Inputs::default()),
        Filter::Collects { series, from, to } => {
            let number = |n: &Option<IssueNumber>| {
                n.as_ref()
                    .map_or_else(String::new, |n| n.as_str().to_owned())
            };
            (
                Kind::Collects,
                0,
                texts(vec![series_label(*series)?, number(from), number(to)]),
            )
        }
    };
    let field = FIELDS.iter().position(|f| f.kind == kind)?;
    Some((field, condition, inputs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_rules_come_back_as_the_rows_that_make_them() {
        let d = |t: &str| t.parse::<Date>().unwrap();
        let db = mylongbox_core::Database::open_in_memory().unwrap();
        let ff = db
            .create_series(&mylongbox_core::SeriesFields::new(
                "Fantastic Four",
                Category::Comic,
            ))
            .unwrap();
        let label = |id: SeriesId| (id == ff).then(|| "Fantastic Four (1961)".to_owned());
        let find = |l: &str| (l == "Fantastic Four (1961)").then_some(ff);
        let rules = [
            Filter::Text {
                field: TextField::Notes,
                condition: TextCondition::Is,
                value: "spine roll".into(),
            },
            Filter::Person {
                name: "Jack Kirby".into(),
                role: None,
            },
            Filter::Person {
                name: "Jack Kirby".into(),
                role: Some(Role::Inker),
            },
            Filter::List {
                field: ListField::SignedBy,
                condition: ListCondition::NoneOf,
                names: vec!["Stan Lee".into(), "Steve Ditko".into()],
            },
            Filter::IssueNumber(Compare::Between(
                IssueNumber::new("Annual 3").unwrap(),
                IssueNumber::new("12").unwrap(),
            )),
            Filter::Grade(Compare::AtMost(Grade::parse("4.5").unwrap())),
            Filter::Number {
                field: NumberField::PageCount,
                compare: Compare::Equal(48),
            },
            Filter::Money {
                field: MoneyField::CoverPrice,
                compare: Compare::Between(
                    Amount::parse(Currency::Gold, "0.0125").unwrap(),
                    Amount::parse(Currency::Gold, "2").unwrap(),
                ),
            },
            Filter::Money {
                field: MoneyField::CurrentValue,
                compare: Compare::AtLeast(Amount::parse(Currency::Usd, "1250.5").unwrap()),
            },
            Filter::Date {
                field: DateField::Added,
                condition: DateCondition::Between(d("2026-01-01"), d("2026-10-12")),
            },
            Filter::Date {
                field: DateField::CoverDate,
                condition: DateCondition::After(d("1999-12-25")),
            },
            Filter::Date {
                field: DateField::ReleaseDate,
                condition: DateCondition::InYear(1988),
            },
            Filter::Choice(Choice::Verification(vec![
                Verification::CgcSignatureSeries,
                Verification::CbcsVerified,
            ])),
            Filter::Choice(Choice::DustJacketCondition(vec![BookCondition::ALL[0]])),
            Filter::Choice(Choice::PageQuality(vec![
                PageQuality::White,
                PageQuality::BrownBrittle,
            ])),
            Filter::Choice(Choice::SignaturePlacement(vec![
                SignaturePlacement::InsideFrontCover,
            ])),
            Filter::Has {
                what: Presence::SignaturePhoto,
                has: false,
            },
            Filter::Flag {
                flag: Flag::Sketch,
                value: false,
            },
            Filter::Has {
                what: Presence::Notes,
                has: true,
            },
            Filter::Collects {
                series: ff,
                from: IssueNumber::new("48").ok(),
                to: None,
            },
        ];
        for rule in rules {
            let (field, condition, inputs) = unbuild(&rule, &label).unwrap();
            assert_eq!(
                build(FIELDS[field].kind, condition, &inputs, &find),
                Some(rule.clone()),
                "{rule:?}"
            );
        }
        // A series no longer listed cannot be shown in a row.
        let gone = Filter::Collects {
            series: ff,
            from: None,
            to: None,
        };
        assert_eq!(unbuild(&gone, &|_| None), None);
    }

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
        assert_eq!(
            build(kind_of("Page Quality"), 0, &picked, &no_series),
            Some(Filter::Choice(Choice::PageQuality(vec![
                PageQuality::OffWhiteToWhite,
                PageQuality::OffWhite
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
