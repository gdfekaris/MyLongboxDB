//! How catalog values read on screen.

use mylongbox_core::{
    Binding, BookCondition, CoverDate, Date, Distribution, Grade, GradeType, MagazineFormat, Role,
};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// e.g. `CGC 9.8`, or `9.4` for the owner's own estimate.
pub fn grade(grade: Option<Grade>, grade_type: Option<GradeType>) -> String {
    let Some(grade) = grade else {
        return String::new();
    };
    match grade_type {
        Some(GradeType::Cgc) => format!("CGC {grade}"),
        Some(GradeType::Cbcs) => format!("CBCS {grade}"),
        Some(GradeType::Pgx) => format!("PGX {grade}"),
        Some(GradeType::Raw) | None => grade.to_string(),
    }
}

pub fn condition(condition: BookCondition) -> &'static str {
    match condition {
        BookCondition::Sealed => "Sealed",
        BookCondition::New => "New",
        BookCondition::LikeNew => "Like New",
        BookCondition::VeryGood => "Very Good",
        BookCondition::Good => "Good",
        BookCondition::Fair => "Fair",
        BookCondition::Poor => "Poor",
    }
}

/// The short form printed on spines and in listings.
pub fn binding_badge(binding: Binding) -> &'static str {
    match binding {
        Binding::Hardcover => "HC",
        Binding::Softcover => "TPB",
        Binding::OversizedHardcover => "OHC",
    }
}

pub fn binding(binding: Binding) -> &'static str {
    match binding {
        Binding::Hardcover => "Hardcover",
        Binding::Softcover => "Softcover",
        Binding::OversizedHardcover => "Oversized hardcover",
    }
}

pub fn distribution(distribution: Distribution) -> &'static str {
    match distribution {
        Distribution::Direct => "Direct edition",
        Distribution::Newsstand => "Newsstand edition",
    }
}

pub fn magazine_format(format: MagazineFormat) -> &'static str {
    match format {
        MagazineFormat::Standard => "Standard size",
        MagazineFormat::Treasury => "Treasury size",
        MagazineFormat::Digest => "Digest size",
    }
}

/// Plural, as a heading over the people credited.
pub fn role(role: Role) -> &'static str {
    match role {
        Role::Writer => "Writers",
        Role::Penciller => "Pencillers",
        Role::Inker => "Inkers",
        Role::Colorist => "Colorists",
        Role::Letterer => "Letterers",
        Role::CoverArtist => "Cover Artists",
        Role::Editor => "Editors",
        Role::Article => "Articles",
        Role::Interview => "Interviews",
    }
}

/// e.g. `2nd printing`.
pub fn printing(printing: u32) -> String {
    let suffix = match (printing % 10, printing % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{printing}{suffix} printing")
}

/// e.g. `Mar 1975`, or `1975` when the month is unknown.
pub fn cover_date(date: Option<CoverDate>) -> String {
    match date {
        None => String::new(),
        Some(d) => match d.month() {
            Some(m) => format!("{} {}", MONTHS[usize::from(m - 1)], d.year()),
            None => d.year().to_string(),
        },
    }
}

/// Reads a cover date as people write it: `1975`, `1975-03`, `3/1975`,
/// `Mar 1975`, or `March 1975`. Empty text is no date; `Err` means the
/// text is not a date.
pub fn parse_cover_date(text: &str) -> Result<Option<CoverDate>, ()> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let year = |t: &str| -> Result<u16, ()> {
        if t.len() == 4 && t.bytes().all(|b| b.is_ascii_digit()) {
            t.parse().map_err(|_| ())
        } else {
            Err(())
        }
    };
    let month = |t: &str| -> Result<u8, ()> {
        if let Ok(m) = t.parse::<u8>() {
            return Ok(m);
        }
        let t = t.to_lowercase();
        let found = MONTHS
            .iter()
            .position(|m| t.len() >= 3 && m.to_lowercase().starts_with(&t[..3.min(t.len())]))
            .ok_or(())?;
        let full = [
            "january",
            "february",
            "march",
            "april",
            "may",
            "june",
            "july",
            "august",
            "september",
            "october",
            "november",
            "december",
        ];
        if !full[found].starts_with(t.trim_end_matches('.')) {
            return Err(());
        }
        u8::try_from(found + 1).map_err(|_| ())
    };
    let date = if let Some((y, m)) = text.split_once('-') {
        CoverDate::new(year(y)?, Some(month(m)?))
    } else if let Some((m, y)) = text.split_once('/') {
        CoverDate::new(year(y)?, Some(month(m)?))
    } else if let Some((m, y)) = text.split_once(' ') {
        CoverDate::new(year(y.trim())?, Some(month(m)?))
    } else {
        CoverDate::new(year(text)?, None)
    };
    date.map(Some).ok_or(())
}

/// e.g. `Mar 5, 1975`.
pub fn date(date: Date) -> String {
    format!(
        "{} {}, {}",
        MONTHS[usize::from(date.month() - 1)],
        date.day(),
        date.year()
    )
}

/// Joins the parts that are not empty with a middle dot.
pub fn joined<I, S>(parts: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    parts
        .into_iter()
        .filter(|p| !p.as_ref().is_empty())
        .map(|p| p.as_ref().to_owned())
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printings_get_ordinals() {
        let names = [1, 2, 3, 4, 11, 12, 13, 21, 22, 101, 111].map(printing);
        assert_eq!(
            names,
            [
                "1st printing",
                "2nd printing",
                "3rd printing",
                "4th printing",
                "11th printing",
                "12th printing",
                "13th printing",
                "21st printing",
                "22nd printing",
                "101st printing",
                "111th printing",
            ]
        );
    }

    #[test]
    fn grades_name_their_grader() {
        let g = Grade::parse("9.8");
        assert_eq!(grade(g, Some(GradeType::Cgc)), "CGC 9.8");
        assert_eq!(grade(g, Some(GradeType::Raw)), "9.8");
        assert_eq!(grade(g, None), "9.8");
        assert_eq!(grade(None, Some(GradeType::Cgc)), "");
    }

    #[test]
    fn dates_read_naturally() {
        assert_eq!(cover_date(CoverDate::new(1975, Some(3))), "Mar 1975");
        assert_eq!(cover_date(CoverDate::new(1975, None)), "1975");
        assert_eq!(cover_date(None), "");
        assert_eq!(date(Date::new(2024, 12, 5).unwrap()), "Dec 5, 2024");
    }

    #[test]
    fn cover_dates_parse_as_people_write_them() {
        let mar = CoverDate::new(1975, Some(3));
        for text in [
            "1975-03",
            "1975-3",
            "3/1975",
            "Mar 1975",
            "march 1975",
            "Mar. 1975",
        ] {
            assert_eq!(parse_cover_date(text), Ok(mar), "{text}");
        }
        assert_eq!(parse_cover_date("1975"), Ok(CoverDate::new(1975, None)));
        assert_eq!(parse_cover_date("  "), Ok(None));
        for bad in [
            "75",
            "13/1975",
            "Marz 1975",
            "Ma 1975",
            "1975-00",
            "spring",
            "1975 03 01",
        ] {
            assert_eq!(parse_cover_date(bad), Err(()), "{bad}");
        }
        // What the form shows reads back as the same date.
        assert_eq!(parse_cover_date(&cover_date(mar)), Ok(mar));
    }

    #[test]
    fn joined_skips_empty_parts() {
        assert_eq!(joined(["a", "", "b"]), "a · b");
        assert_eq!(joined(["", ""]), "");
    }
}
