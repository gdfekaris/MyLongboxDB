//! The Gaps section: the numbers missing from each run, with a choice
//! of whether issues in collected editions fill them.

use std::rc::Rc;

use adw::prelude::*;
use mylongbox_core::{Category, SeriesGaps, SeriesId};

use crate::form::series_label;

/// The most ranges listed for one series before the rest are counted.
const LISTED: usize = 30;

/// What the page's controls do.
pub struct Hooks {
    pub count_collected: Rc<dyn Fn(bool)>,
    /// Opens a series' books.
    pub open: Rc<dyn Fn(SeriesId, String)>,
}

/// The missing numbers as a reader would write them: `#3–4, #9`, with a
/// count of any beyond the first [`LISTED`] ranges.
pub fn missing_text(missing: &[(u32, u32)]) -> String {
    let mut parts: Vec<String> = missing
        .iter()
        .take(LISTED)
        .map(|(a, b)| {
            if a == b {
                format!("#{a}")
            } else {
                format!("#{a}\u{2013}{b}")
            }
        })
        .collect();
    let rest: u32 = missing.iter().skip(LISTED).map(|(a, b)| b - a + 1).sum();
    if rest > 0 {
        parts.push(format!("and {rest} more"));
    }
    parts.join(", ")
}

/// How much of the run is owned, e.g. `Own 14 of 20, #1–20`.
fn summary(g: &SeriesGaps) -> String {
    let unit = match g.category {
        Category::CollectedEdition => "volumes",
        Category::Comic | Category::Magazine => "issues",
    };
    format!(
        "Own {} of {} {unit}, #{}\u{2013}{}",
        g.owned,
        g.last - g.first + 1,
        g.first,
        g.last
    )
}

pub fn page(gaps: &[SeriesGaps], count_collected: bool, hooks: &Hooks) -> adw::PreferencesPage {
    let options = adw::PreferencesGroup::new();
    let collected = adw::SwitchRow::builder()
        .title("Count Issues in Collected Editions")
        .subtitle("An issue reprinted in a book you own fills its gap")
        .active(count_collected)
        .build();
    {
        let hook = hooks.count_collected.clone();
        collected.connect_active_notify(move |row| hook(row.is_active()));
    }
    options.add(&collected);

    let runs = adw::PreferencesGroup::builder()
        .title("Missing Numbers")
        .description(
            "Runs go from the lowest number you own to the highest, or to the \
             series\u{2019} known issue count when it is set.",
        )
        .build();
    if gaps.is_empty() {
        let none = adw::ActionRow::builder()
            .title("No gaps: every run you own is complete")
            .build();
        runs.add(&none);
    }
    for g in gaps {
        let label = series_label(&g.name, g.start_year);
        let row = adw::ActionRow::new();
        row.set_use_markup(false);
        row.set_title(&label);
        row.set_subtitle(&format!(
            "Missing {}\n{}",
            missing_text(&g.missing),
            summary(g)
        ));
        row.set_subtitle_lines(4);
        row.set_activatable(true);
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        let (hook, series) = (hooks.open.clone(), g.series);
        row.connect_activated(move |_| hook(series, label.clone()));
        runs.add(&row);
    }

    let page = adw::PreferencesPage::new();
    page.add(&options);
    page.add(&runs);
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_numbers_read_naturally() {
        assert_eq!(missing_text(&[(3, 3)]), "#3");
        assert_eq!(missing_text(&[(3, 4), (9, 9)]), "#3\u{2013}4, #9");
        let many: Vec<(u32, u32)> = (0..40).map(|n| (n * 3, n * 3 + 1)).collect();
        let text = missing_text(&many);
        assert!(text.starts_with("#0\u{2013}1, #3\u{2013}4"));
        assert!(text.ends_with(", and 20 more"), "{text}");
        assert_eq!(text.matches('#').count(), LISTED);
    }

    #[test]
    fn runs_are_summed_up() {
        let g = SeriesGaps {
            series: mylongbox_core::Database::open_in_memory()
                .unwrap()
                .create_series(&mylongbox_core::SeriesFields::new("X", Category::Comic))
                .unwrap(),
            name: "X".into(),
            start_year: None,
            category: Category::CollectedEdition,
            owned: 3,
            first: 1,
            last: 5,
            missing: vec![(2, 3)],
        };
        assert_eq!(summary(&g), "Own 3 of 5 volumes, #1\u{2013}5");
    }
}
