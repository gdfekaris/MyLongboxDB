//! The full-size view of a copy's cover photos, with a flip between
//! front and back and zooming to see condition up close.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib};
use mylongbox_core::Side;

use crate::covers;
use crate::i18n::gettext;

/// The furthest the photo can be enlarged: four photo pixels a screen
/// pixel is enough to judge spine ticks and corner wear.
const MAX_ZOOM: f64 = 4.0;
/// How much one step of zooming changes the size.
const STEP: f64 = 1.25;

/// The scale that shows a `width` × `height` photo whole in the view.
fn fit_scale(view: (f64, f64), photo: (f64, f64)) -> f64 {
    (view.0 / photo.0.max(1.0))
        .min(view.1 / photo.1.max(1.0))
        .max(f64::MIN_POSITIVE)
}

/// A zoom kept between showing the photo whole (or its real size, if
/// that is smaller) and [`MAX_ZOOM`].
fn clamp_zoom(zoom: f64, fit: f64) -> f64 {
    zoom.clamp(fit.min(1.0), MAX_ZOOM.max(fit))
}

/// The scroll position that keeps the point under `pointer` still when
/// the zoom changes from `old` to `new`.
fn anchored(scroll: f64, pointer: f64, old: f64, new: f64) -> f64 {
    ((scroll + pointer) * new / old - pointer).max(0.0)
}

/// What one of the viewer's buttons does.
type Action = fn(&Viewer);

struct Viewer {
    front: Option<gdk::Texture>,
    back: Option<gdk::Texture>,
    signature: Option<gdk::Texture>,
    side: Cell<Side>,
    /// The scale the photo is shown at, or `None` while fitted to the
    /// view, which follows the view's size.
    zoom: Cell<Option<f64>>,
    scrolled: gtk::ScrolledWindow,
    picture: gtk::Picture,
    /// Hold the picture to its zoomed size across and down; a picture
    /// otherwise asks for its photo's full size and would be stretched
    /// to it.
    across: adw::Clamp,
    down: adw::Clamp,
    missing: gtk::Box,
    zoom_label: gtk::Label,
}

impl Viewer {
    fn texture(&self) -> Option<&gdk::Texture> {
        match self.side.get() {
            Side::Front => self.front.as_ref(),
            Side::Back => self.back.as_ref(),
            Side::Signature => self.signature.as_ref(),
        }
    }

    fn photo_size(&self) -> Option<(f64, f64)> {
        self.texture()
            .map(|t| (f64::from(t.width()), f64::from(t.height())))
    }

    fn view_size(&self) -> (f64, f64) {
        (
            f64::from(self.scrolled.width()),
            f64::from(self.scrolled.height()),
        )
    }

    fn fit(&self) -> f64 {
        self.photo_size()
            .map_or(1.0, |photo| fit_scale(self.view_size(), photo))
    }

    /// Shows the current side, fitted to the view.
    fn show_side(&self) {
        let texture = self.texture().cloned();
        self.picture.set_paintable(texture.as_ref());
        self.picture
            .set_alternative_text(Some(&match self.side.get() {
                Side::Front => gettext("Front cover photo"),
                Side::Back => gettext("Back cover photo"),
                Side::Signature => gettext("Photo of the signature"),
            }));
        self.across.set_visible(texture.is_some());
        self.missing.set_visible(texture.is_none());
        self.missing_label().set_label(&match self.side.get() {
            Side::Front => gettext("No Front Cover Image"),
            Side::Back => gettext("No Back Cover Image"),
            Side::Signature => gettext("No Signature Photo"),
        });
        self.set_zoom(None, None);
    }

    fn missing_label(&self) -> gtk::Label {
        self.missing
            .last_child()
            .and_downcast::<gtk::Label>()
            .unwrap_or_default()
    }

    /// Zooms to `zoom`, or fits the photo to the view for `None`,
    /// keeping the point under `pointer` still if given.
    fn set_zoom(&self, zoom: Option<f64>, pointer: Option<(f64, f64)>) {
        let Some((w, h)) = self.photo_size() else {
            self.zoom.set(None);
            self.zoom_label.set_label("");
            return;
        };
        let fit = self.fit();
        let old = self.zoom.get().unwrap_or(fit);
        let zoom = zoom.map(|z| clamp_zoom(z, fit)).filter(|z| *z > fit);
        self.zoom.set(zoom);
        // Fitted, the photo must not scroll, or it would be given its
        // full size; zoomed, it scrolls.
        let policy = match zoom {
            None => gtk::PolicyType::Never,
            Some(_) => gtk::PolicyType::Automatic,
        };
        self.scrolled.set_policy(policy, policy);
        match zoom {
            None => {
                self.picture.set_size_request(-1, -1);
                self.picture.set_content_fit(gtk::ContentFit::Contain);
                self.across.set_maximum_size(i32::MAX);
                self.down.set_maximum_size(i32::MAX);
            }
            Some(z) => {
                // At most four times a photo of at most 10 000 pixels a
                // side, far within i32.
                let (zw, zh) = ((w * z).round() as i32, (h * z).round() as i32);
                self.picture.set_size_request(zw, zh);
                self.picture.set_content_fit(gtk::ContentFit::Fill);
                for (clamp, size) in [(&self.across, zw), (&self.down, zh)] {
                    clamp.set_maximum_size(size);
                    clamp.set_tightening_threshold(size);
                }
            }
        }
        let new = zoom.unwrap_or(fit);
        self.show_zoom();
        // Scroll once the new size is laid out.
        let (hadj, vadj) = (self.scrolled.hadjustment(), self.scrolled.vadjustment());
        let view = self.view_size();
        let (px, py) = pointer.unwrap_or((view.0 / 2.0, view.1 / 2.0));
        let (sx, sy) = (hadj.value(), vadj.value());
        self.scrolled.add_tick_callback(move |_, _| {
            hadj.set_value(anchored(sx, px, old, new));
            vadj.set_value(anchored(sy, py, old, new));
            glib::ControlFlow::Break
        });
    }

    /// Shows the scale in use, which while fitted follows the view's
    /// size.
    fn show_zoom(&self) {
        let label = match (self.photo_size(), self.zoom.get()) {
            (None, _) => String::new(),
            (Some(_), Some(z)) => format!("{:.0}%", z * 100.0),
            (Some(_), None) => format!("{:.0}%", self.fit() * 100.0),
        };
        self.zoom_label.set_label(&label);
    }

    fn zoom_by(&self, factor: f64, pointer: Option<(f64, f64)>) {
        let current = self.zoom.get().unwrap_or_else(|| self.fit());
        self.set_zoom(Some(current * factor), pointer);
    }

    /// Fitted goes to actual size, anything else back to fitted.
    fn toggle_zoom(&self, pointer: Option<(f64, f64)>) {
        let zoom = match self.zoom.get() {
            None => Some(1.0_f64.max(self.fit() * STEP)),
            Some(_) => None,
        };
        self.set_zoom(zoom, pointer);
    }
}

/// A copy's photos: its covers, and a signature inside it if there is a
/// photo of one.
pub struct Photos {
    pub front: Option<gdk::Texture>,
    pub back: Option<gdk::Texture>,
    pub signature: Option<gdk::Texture>,
}

/// Opens the viewer on `side` of a copy's photos.
pub fn open(parent: &impl IsA<gtk::Widget>, title: &str, photos: Photos, side: Side) {
    let Photos {
        front,
        back,
        signature,
    } = photos;
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Contain)
        .can_shrink(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .vexpand(true)
        .build();
    let missing = covers::placeholder("", false);
    let clamp = |orientation| {
        adw::Clamp::builder()
            .orientation(orientation)
            .unit(adw::LengthUnit::Px)
            .maximum_size(i32::MAX)
            .build()
    };
    let down = clamp(gtk::Orientation::Vertical);
    down.set_child(Some(&picture));
    let across = clamp(gtk::Orientation::Horizontal);
    across.set_child(Some(&down));
    let stack = gtk::Box::builder().hexpand(true).vexpand(true).build();
    stack.append(&across);
    stack.append(&missing);
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&stack)
        .hexpand(true)
        .vexpand(true)
        .build();
    let zoom_label = gtk::Label::builder()
        .width_chars(5)
        .css_classes(["numeric", "dim-label"])
        .build();
    // The signature is offered only when there is a photo of it.
    let mut choices = vec![
        (Side::Front, gettext("_Front")),
        (Side::Back, gettext("_Back")),
    ];
    if signature.is_some() || side == Side::Signature {
        // Translators: a photo of a signature inside a comic, beside its
        // front and back covers.
        choices.push((Side::Signature, gettext("_Signature")));
    }
    let viewer = Rc::new(Viewer {
        front,
        back,
        signature,
        side: Cell::new(side),
        zoom: Cell::new(None),
        scrolled: scrolled.clone(),
        picture,
        across: across.clone(),
        down,
        missing,
        zoom_label: zoom_label.clone(),
    });

    let sides = adw::ToggleGroup::new();
    for (side, label) in choices {
        sides.add(
            adw::Toggle::builder()
                .name(side.code())
                .label(&label)
                .use_underline(true)
                .build(),
        );
    }
    sides.set_active_name(Some(side.code()));
    {
        let weak = Rc::downgrade(&viewer);
        sides.connect_active_name_notify(move |group| {
            if let Some(v) = weak.upgrade() {
                v.side.set(
                    group
                        .active_name()
                        .and_then(|name| Side::from_code(&name))
                        .unwrap_or(Side::Front),
                );
                v.show_side();
            }
        });
    }

    let button = |icon: &str, tip: &str| {
        gtk::Button::builder()
            .icon_name(icon)
            .tooltip_text(tip)
            .css_classes(["flat"])
            .build()
    };
    let out = button("zoom-out-symbolic", &gettext("Zoom Out"));
    let fit = button("zoom-fit-best-symbolic", &gettext("Fit to Window"));
    let into = button("zoom-in-symbolic", &gettext("Zoom In"));
    let actions: [(&gtk::Button, Action); 3] = [
        (&out, |v| v.zoom_by(1.0 / STEP, None)),
        (&fit, |v| v.set_zoom(None, None)),
        (&into, |v| v.zoom_by(STEP, None)),
    ];
    for (b, action) in actions {
        let weak = Rc::downgrade(&viewer);
        b.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                action(&v);
            }
        });
    }
    let zoom = gtk::Box::builder().spacing(2).build();
    zoom.append(&out);
    zoom.append(&zoom_label);
    zoom.append(&into);
    zoom.append(&fit);

    let header = adw::HeaderBar::builder().title_widget(&sides).build();
    header.pack_start(&zoom);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scrolled));

    // Ctrl and the scroll wheel zoom around the pointer; scrolling alone
    // pans as usual.
    let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    let pointer = Rc::new(Cell::new(None::<(f64, f64)>));
    {
        let weak = Rc::downgrade(&viewer);
        let pointer = pointer.clone();
        wheel.connect_scroll(move |c, _, dy| {
            let Some(v) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if !c
                .current_event_state()
                .contains(gdk::ModifierType::CONTROL_MASK)
            {
                return glib::Propagation::Proceed;
            }
            v.zoom_by(if dy < 0.0 { STEP } else { 1.0 / STEP }, pointer.get());
            glib::Propagation::Stop
        });
    }
    scrolled.add_controller(wheel);
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(move |_, x, y| pointer.set(Some((x, y))));
    scrolled.add_controller(motion);
    // Pinching zooms; dragging pans; a double click flips between
    // fitted and actual size.
    let pinch = gtk::GestureZoom::new();
    let pinch_start = Rc::new(Cell::new(1.0));
    {
        let (weak, start) = (Rc::downgrade(&viewer), pinch_start.clone());
        pinch.connect_begin(move |_, _| {
            if let Some(v) = weak.upgrade() {
                start.set(v.zoom.get().unwrap_or_else(|| v.fit()));
            }
        });
    }
    {
        let (weak, start) = (Rc::downgrade(&viewer), pinch_start);
        pinch.connect_scale_changed(move |g, scale| {
            if let Some(v) = weak.upgrade() {
                v.set_zoom(Some(start.get() * scale), g.bounding_box_center());
            }
        });
    }
    scrolled.add_controller(pinch);
    let pan = gtk::GestureDrag::new();
    let pan_start = Rc::new(Cell::new((0.0, 0.0)));
    {
        let (scrolled, start) = (scrolled.clone(), pan_start.clone());
        pan.connect_drag_begin(move |_, _, _| {
            start.set((
                scrolled.hadjustment().value(),
                scrolled.vadjustment().value(),
            ));
        });
    }
    {
        let (scrolled, start) = (scrolled.clone(), pan_start);
        pan.connect_drag_update(move |_, dx, dy| {
            let (x, y) = start.get();
            scrolled.hadjustment().set_value(x - dx);
            scrolled.vadjustment().set_value(y - dy);
        });
    }
    scrolled.add_controller(pan);
    let double = gtk::GestureClick::new();
    {
        let weak = Rc::downgrade(&viewer);
        double.connect_pressed(move |_, presses, x, y| {
            if presses == 2
                && let Some(v) = weak.upgrade()
            {
                v.toggle_zoom(Some((x, y)));
            }
        });
    }
    scrolled.add_controller(double);

    let keys = gtk::EventControllerKey::new();
    {
        let weak = Rc::downgrade(&viewer);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(v) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            match key {
                gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add => v.zoom_by(STEP, None),
                gdk::Key::minus | gdk::Key::KP_Subtract => v.zoom_by(1.0 / STEP, None),
                gdk::Key::_0 | gdk::Key::KP_0 => v.set_zoom(None, None),
                gdk::Key::_1 | gdk::Key::KP_1 => v.set_zoom(Some(1.0), None),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
    }

    let dialog = adw::Dialog::builder()
        .title(title)
        .content_width(1000)
        .content_height(820)
        .child(&toolbar)
        .build();
    dialog.add_controller(keys);
    viewer.show_side();
    // The view's size is known only once laid out, and changes with the
    // window; the fitted scale follows it.
    {
        let weak = Rc::downgrade(&viewer);
        scrolled.hadjustment().connect_page_size_notify(move |_| {
            if let Some(v) = weak.upgrade() {
                v.show_zoom();
            }
        });
    }
    // The dialog's widgets hold the viewer only weakly; keep it alive
    // while the dialog is.
    dialog.connect_closed(move |_| {
        let _ = &viewer;
    });
    dialog.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_shows_the_whole_photo() {
        assert_eq!(fit_scale((800.0, 600.0), (1600.0, 2400.0)), 0.25);
        assert_eq!(fit_scale((800.0, 600.0), (400.0, 150.0)), 2.0);
    }

    #[test]
    fn zoom_stays_between_fitted_and_four_times() {
        assert_eq!(clamp_zoom(0.01, 0.25), 0.25);
        assert_eq!(clamp_zoom(9.0, 0.25), MAX_ZOOM);
        assert_eq!(clamp_zoom(0.5, 0.25), 0.5);
        // A small photo may be shown at its real size, below the fit.
        assert_eq!(clamp_zoom(0.1, 2.0), 1.0);
        assert_eq!(clamp_zoom(9.0, 6.0), 6.0);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        // Doubling the size around a point 100 px into the view, scrolled
        // 50 px: that photo point was at 150 and is now at 300.
        assert_eq!(anchored(50.0, 100.0, 1.0, 2.0), 200.0);
        assert_eq!(anchored(200.0, 100.0, 2.0, 1.0), 50.0);
        assert_eq!(
            anchored(0.0, 100.0, 2.0, 1.0),
            0.0,
            "never before the start"
        );
    }
}
