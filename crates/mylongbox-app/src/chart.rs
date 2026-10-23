//! A small line chart of a copy's value over time.

use adw::prelude::*;
use mylongbox_core::{Amount, Currency, Date, ValueEntry};

use crate::i18n::gettext;
use crate::style::{self, Style};

/// Space around the line, so dots at the edges are not cut off.
const PAD: f64 = 8.0;

/// One point per value in the currency of the newest value, placed by
/// date. Values in other currencies cannot be compared without exchange
/// rates, so they are left out.
pub fn points(values: &[(Date, Amount)]) -> Vec<(i64, i64)> {
    let Some(&(_, newest)) = values.last() else {
        return Vec::new();
    };
    let currency: Currency = newest.currency();
    let first = values[0].0;
    values
        .iter()
        .filter(|(_, v)| v.currency() == currency)
        .map(|&(date, v)| (first.days_until(date), v.units()))
        .collect()
}

/// Maps points into a `width` by `height` box with `PAD` around it,
/// higher values nearer the top.
pub fn scale(points: &[(i64, i64)], width: f64, height: f64) -> Vec<(f64, f64)> {
    let (min_x, max_x) = bounds(points.iter().map(|p| p.0));
    let (min_y, max_y) = bounds(points.iter().map(|p| p.1));
    let place = |value: i64, min: i64, max: i64, span: f64| {
        if max == min {
            span / 2.0
        } else {
            // Values in a chart are far below the range where i64 to f64
            // loses precision that a few pixels could show.
            #[allow(clippy::cast_precision_loss)]
            let t = (value - min) as f64 / (max - min) as f64;
            t * span
        }
    };
    let inner_w = (width - 2.0 * PAD).max(0.0);
    let inner_h = (height - 2.0 * PAD).max(0.0);
    points
        .iter()
        .map(|&(x, y)| {
            (
                PAD + place(x, min_x, max_x, inner_w),
                PAD + inner_h - place(y, min_y, max_y, inner_h),
            )
        })
        .collect()
}

fn bounds(values: impl Iterator<Item = i64>) -> (i64, i64) {
    values.fold((i64::MAX, i64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
}

/// Deco Pulp's touch: printed dots beneath the line, in the line's
/// color, and a gold tick under each value.
fn halftone_under(
    cr: &gtk::cairo::Context,
    spots: &[(f64, f64)],
    (width, height): (f64, f64),
    color: gtk::gdk::RGBA,
) {
    let (Some(first), Some(last)) = (spots.first(), spots.last()) else {
        return;
    };
    cr.save().ok();
    cr.move_to(first.0, height);
    for &(x, y) in spots {
        cr.line_to(x, y);
    }
    cr.line_to(last.0, height);
    cr.close_path();
    cr.clip();
    cr.set_source_rgba(
        f64::from(color.red()),
        f64::from(color.green()),
        f64::from(color.blue()),
        0.35,
    );
    const STEP: f64 = 5.0;
    let mut row = 0;
    let mut y = 0.0;
    while y < height {
        // Every other row sits half a step over, as a printing screen does.
        let mut x = if row % 2 == 0 { 0.0 } else { STEP / 2.0 };
        while x < width {
            cr.arc(x, y, 1.1, 0.0, std::f64::consts::TAU);
            x += STEP;
        }
        y += STEP;
        row += 1;
    }
    // Drawing errors only leave the chart plainer.
    let _ = cr.fill();
    cr.restore().ok();
    cr.save().ok();
    cr.set_source_rgb(0.788, 0.635, 0.153);
    cr.set_line_width(1.5);
    for &(x, _) in spots {
        cr.move_to(x, height - 5.0);
        cr.line_to(x, height);
    }
    let _ = cr.stroke();
    cr.restore().ok();
}

pub fn value_chart(values: &[ValueEntry]) -> gtk::DrawingArea {
    let dated: Vec<_> = values.iter().map(|v| (v.date, v.value)).collect();
    let points = points(&dated);
    let area = gtk::DrawingArea::builder()
        .content_height(90)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(12)
        // Takes the accent color, which the line is drawn in.
        .css_classes(["accent"])
        .build();
    area.update_property(&[gtk::accessible::Property::Label(&gettext(
        "Chart of this copy's value over time",
    ))]);
    area.set_draw_func(move |area, cr, width, height| {
        let spots = scale(&points, f64::from(width), f64::from(height));
        let color = area.color();
        cr.set_source_rgba(
            f64::from(color.red()),
            f64::from(color.green()),
            f64::from(color.blue()),
            0.8,
        );
        if style::current() == Style::Pulp {
            let size = (f64::from(width), f64::from(height));
            halftone_under(cr, &spots, size, color);
        }
        cr.set_line_width(2.0);
        for (i, &(x, y)) in spots.iter().enumerate() {
            if i == 0 {
                cr.move_to(x, y);
            } else {
                cr.line_to(x, y);
            }
        }
        // Drawing errors only leave the chart blank.
        let _ = cr.stroke();
        for &(x, y) in &spots {
            cr.arc(x, y, 3.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        }
    });
    area
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(date: &str, currency: Currency, units: i64) -> (Date, Amount) {
        (date.parse().unwrap(), Amount::new(currency, units))
    }

    #[test]
    fn points_follow_the_newest_currency() {
        let values = [
            entry("2020-01-01", Currency::Usd, 500),
            entry("2020-01-11", Currency::Gold, 3),
            entry("2020-01-31", Currency::Usd, 900),
        ];
        assert_eq!(points(&values), [(0, 500), (30, 900)]);
    }

    #[test]
    fn higher_values_sit_higher_and_flat_lines_sit_midway() {
        let spots = scale(&[(0, 100), (10, 200)], 116.0, 116.0);
        assert_eq!(spots, [(PAD, PAD + 100.0), (PAD + 100.0, PAD)]);
        let flat = scale(&[(0, 5), (10, 5)], 116.0, 116.0);
        assert_eq!(flat[0].1, PAD + 50.0);
    }
}
