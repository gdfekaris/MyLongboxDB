//! The photo editor: turn, straighten, and crop a cover photo before it
//! is saved. It works on a small preview; the same edit is then applied
//! to the full photo away from the window.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::cairo;
use mylongbox_media::{Crop, Edit, MAX_STRAIGHTEN, RgbImage, apply_edit, find_cover};

use crate::covers;

/// How close, in pixels, the pointer must be to grab an edge or corner.
const REACH: f64 = 14.0;

/// Which edges of the crop a drag moves; all four moves the whole crop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Grip {
    left: bool,
    top: bool,
    right: bool,
    bottom: bool,
}

impl Grip {
    const MOVE: Self = Self {
        left: true,
        top: true,
        right: true,
        bottom: true,
    };

    fn cursor(self) -> &'static str {
        match (self.left || self.right, self.top || self.bottom) {
            _ if self == Self::MOVE => "move",
            (true, true) if (self.left && self.top) || (self.right && self.bottom) => "nwse-resize",
            (true, true) => "nesw-resize",
            (true, false) => "ew-resize",
            _ => "ns-resize",
        }
    }
}

/// Where the picture sits in the editing area: its offset and scale.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    x: f64,
    y: f64,
    scale: f64,
    width: f64,
    height: f64,
}

impl Layout {
    /// The picture shown whole and centred, as `gtk::Picture` shows it.
    fn new(area: (f64, f64), frame: (u32, u32)) -> Self {
        let (fw, fh) = (f64::from(frame.0.max(1)), f64::from(frame.1.max(1)));
        let scale = (area.0 / fw).min(area.1 / fh).max(f64::MIN_POSITIVE);
        let (width, height) = (fw * scale, fh * scale);
        Self {
            x: (area.0 - width) / 2.0,
            y: (area.1 - height) / 2.0,
            scale,
            width,
            height,
        }
    }

    /// The crop's edges in the area's coordinates.
    fn edges(&self, crop: Crop) -> (f64, f64, f64, f64) {
        (
            self.x + f64::from(crop.left) * self.width,
            self.y + f64::from(crop.top) * self.height,
            self.x + f64::from(crop.right) * self.width,
            self.y + f64::from(crop.bottom) * self.height,
        )
    }

    /// What a press at `(x, y)` would grab: an edge or corner within
    /// reach, else the whole crop if inside it.
    fn grip(&self, crop: Crop, x: f64, y: f64) -> Option<Grip> {
        let (l, t, r, b) = self.edges(crop);
        let within = |v: f64, from: f64, to: f64| v >= from - REACH && v <= to + REACH;
        if !within(x, l, r) || !within(y, t, b) {
            return None;
        }
        let near = |v: f64, edge: f64| (v - edge).abs() <= REACH;
        let grip = Grip {
            left: near(x, l),
            top: near(y, t),
            right: near(x, r) && !near(x, l),
            bottom: near(y, b) && !near(y, t),
        };
        if grip.left || grip.top || grip.right || grip.bottom {
            Some(grip)
        } else if x > l && x < r && y > t && y < b {
            Some(Grip::MOVE)
        } else {
            None
        }
    }

    /// The crop after dragging `grip` by `(dx, dy)` area pixels from
    /// `start`.
    fn dragged(&self, start: Crop, grip: Grip, dx: f64, dy: f64) -> Crop {
        // Shares of the picture are small numbers; f32 holds them well.
        let sx = (dx / self.width) as f32;
        let sy = (dy / self.height) as f32;
        if grip == Grip::MOVE {
            let sx = sx.clamp(-start.left, 1.0 - start.right);
            let sy = sy.clamp(-start.top, 1.0 - start.bottom);
            return Crop {
                left: start.left + sx,
                top: start.top + sy,
                right: start.right + sx,
                bottom: start.bottom + sy,
            };
        }
        let mut crop = start;
        if grip.left {
            crop.left = (start.left + sx).min(start.right - mylongbox_media::MIN_CROP);
        }
        if grip.right {
            crop.right = (start.right + sx).max(start.left + mylongbox_media::MIN_CROP);
        }
        if grip.top {
            crop.top = (start.top + sy).min(start.bottom - mylongbox_media::MIN_CROP);
        }
        if grip.bottom {
            crop.bottom = (start.bottom + sy).max(start.top + mylongbox_media::MIN_CROP);
        }
        crop.clamped()
    }
}

/// What one of the editor's buttons does.
type Action = fn(&Editor);

struct Editor {
    preview: RgbImage,
    edit: Cell<Edit>,
    /// The size of the turned preview, which the crop is a share of.
    frame: Cell<(u32, u32)>,
    picture: gtk::Picture,
    area: gtk::DrawingArea,
    toasts: adw::ToastOverlay,
    straighten: gtk::Scale,
    drag: Cell<Option<(Grip, Crop)>>,
}

impl Editor {
    /// Redraws the preview turned and straightened, uncropped; the crop
    /// is drawn over it.
    fn refresh(&self) {
        let whole = Edit {
            crop: Crop::FULL,
            ..self.edit.get()
        };
        let frame = apply_edit(&self.preview, &whole);
        self.frame.set(frame.dimensions());
        self.picture
            .set_paintable(covers::texture_of(&frame).as_ref());
        self.area.queue_draw();
    }

    fn layout(&self) -> Layout {
        Layout::new(
            (f64::from(self.area.width()), f64::from(self.area.height())),
            self.frame.get(),
        )
    }

    fn change(&self, f: impl FnOnce(&mut Edit)) {
        let mut edit = self.edit.get();
        f(&mut edit);
        self.edit.set(edit);
        self.refresh();
    }

    /// Crops to the cover if one stands out from the background.
    fn fit_to_cover(&self) {
        let whole = Edit {
            crop: Crop::FULL,
            ..self.edit.get()
        };
        match find_cover(&apply_edit(&self.preview, &whole)) {
            Some(crop) => {
                self.change(|e| e.crop = crop);
            }
            None => self
                .toasts
                .add_toast(adw::Toast::new("No cover stands out from the background")),
        }
    }

    fn draw(&self, cr: &cairo::Context) {
        let layout = self.layout();
        let (l, t, r, b) = layout.edges(self.edit.get().crop);
        // Dim what the crop leaves out.
        cr.set_fill_rule(cairo::FillRule::EvenOdd);
        cr.rectangle(layout.x, layout.y, layout.width, layout.height);
        cr.rectangle(l, t, r - l, b - t);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.55);
        let _ = cr.fill();
        // Thirds, to help line the cover up while straightening.
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.35);
        cr.set_line_width(1.0);
        for n in [1.0, 2.0] {
            let x = l + (r - l) * n / 3.0;
            let y = t + (b - t) * n / 3.0;
            cr.move_to(x, t);
            cr.line_to(x, b);
            cr.move_to(l, y);
            cr.line_to(r, y);
        }
        let _ = cr.stroke();
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.set_line_width(2.0);
        cr.rectangle(l, t, r - l, b - t);
        let _ = cr.stroke();
        for (x, y) in [(l, t), (r, t), (l, b), (r, b)] {
            cr.rectangle(x - 5.0, y - 5.0, 10.0, 10.0);
        }
        let _ = cr.fill();
    }
}

/// Opens the editor on `preview`, starting from `proposal` if a cover
/// was found. `done` gets the edit when the owner saves; closing the
/// editor otherwise drops the photo.
pub fn open(
    parent: &impl IsA<gtk::Widget>,
    title: &str,
    preview: RgbImage,
    proposal: Option<Crop>,
    done: impl Fn(Edit) + 'static,
) {
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Contain)
        .can_shrink(true)
        .hexpand(true)
        .vexpand(true)
        .build();
    let area = gtk::DrawingArea::builder()
        .hexpand(true)
        .vexpand(true)
        .build();
    let canvas = gtk::Overlay::builder()
        .child(&picture)
        .margin_top(18)
        .margin_bottom(12)
        .margin_start(18)
        .margin_end(18)
        .build();
    canvas.add_overlay(&area);

    let straighten = gtk::Scale::with_range(
        gtk::Orientation::Horizontal,
        f64::from(-MAX_STRAIGHTEN),
        f64::from(MAX_STRAIGHTEN),
        0.1,
    );
    straighten.set_value(0.0);
    straighten.set_digits(1);
    straighten.set_format_value_func(|_, degrees| format!("{degrees:.1}\u{b0}"));
    straighten.set_draw_value(true);
    straighten.set_hexpand(true);
    straighten.add_mark(0.0, gtk::PositionType::Bottom, None);
    straighten.update_property(&[gtk::accessible::Property::Label("Straighten")]);

    let editor = Rc::new(Editor {
        preview,
        edit: Cell::new(Edit {
            crop: proposal.unwrap_or(Crop::FULL),
            ..Edit::default()
        }),
        frame: Cell::new((1, 1)),
        picture,
        area: area.clone(),
        toasts: adw::ToastOverlay::new(),
        straighten: straighten.clone(),
        drag: Cell::new(None),
    });
    editor.refresh();

    {
        let weak = Rc::downgrade(&editor);
        area.set_draw_func(move |_, cr, _, _| {
            if let Some(editor) = weak.upgrade() {
                editor.draw(cr);
            }
        });
    }
    let drag = gtk::GestureDrag::new();
    {
        let weak = Rc::downgrade(&editor);
        drag.connect_drag_begin(move |_, x, y| {
            if let Some(e) = weak.upgrade() {
                let crop = e.edit.get().crop;
                e.drag.set(e.layout().grip(crop, x, y).map(|g| (g, crop)));
            }
        });
    }
    {
        let weak = Rc::downgrade(&editor);
        drag.connect_drag_update(move |_, dx, dy| {
            if let Some(e) = weak.upgrade()
                && let Some((grip, start)) = e.drag.get()
            {
                let crop = e.layout().dragged(start, grip, dx, dy);
                let mut edit = e.edit.get();
                edit.crop = crop;
                e.edit.set(edit);
                e.area.queue_draw();
            }
        });
    }
    {
        let weak = Rc::downgrade(&editor);
        drag.connect_drag_end(move |_, _, _| {
            if let Some(e) = weak.upgrade() {
                e.drag.set(None);
            }
        });
    }
    area.add_controller(drag);
    let motion = gtk::EventControllerMotion::new();
    {
        let weak = Rc::downgrade(&editor);
        motion.connect_motion(move |_, x, y| {
            if let Some(e) = weak.upgrade()
                && e.drag.get().is_none()
            {
                let grip = e.layout().grip(e.edit.get().crop, x, y);
                e.area.set_cursor_from_name(grip.map(Grip::cursor));
            }
        });
    }
    area.add_controller(motion);
    {
        let weak = Rc::downgrade(&editor);
        straighten.connect_value_changed(move |scale| {
            if let Some(e) = weak.upgrade() {
                // The slider's range matches the limit, so this is safe
                // to narrow.
                e.change(|edit| edit.set_straighten(scale.value() as f32));
            }
        });
    }

    let button = |icon: Option<&str>, label: Option<&str>, tip: &str| {
        let b = gtk::Button::builder().tooltip_text(tip).build();
        if let Some(icon) = icon {
            b.set_icon_name(icon);
        }
        if let Some(label) = label {
            b.set_label(label);
            b.set_use_underline(true);
        }
        b
    };
    let left = button(Some("object-rotate-left-symbolic"), None, "Turn Left");
    let right = button(Some("object-rotate-right-symbolic"), None, "Turn Right");
    let fit = button(
        None,
        Some("_Fit to Cover"),
        "Crop to the cover, found by its edges",
    );
    let reset = button(None, Some("_Reset"), "Undo All Changes");
    let actions: [(&gtk::Button, Action); 4] = [
        (&left, |e| e.change(Edit::turn_left)),
        (&right, |e| e.change(Edit::turn_right)),
        (&fit, Editor::fit_to_cover),
        (&reset, |e| {
            e.change(|edit| *edit = Edit::default());
            e.straighten.set_value(0.0);
        }),
    ];
    for (b, action) in actions {
        let weak = Rc::downgrade(&editor);
        b.connect_clicked(move |_| {
            if let Some(e) = weak.upgrade() {
                action(&e);
            }
        });
    }
    let straighten_label = gtk::Label::builder()
        .label("Straighten")
        .css_classes(["dim-label"])
        .build();
    let controls = gtk::Box::builder()
        .spacing(12)
        .margin_start(18)
        .margin_end(18)
        .margin_bottom(18)
        .build();
    let turns = gtk::Box::builder().css_classes(["linked"]).build();
    turns.append(&left);
    turns.append(&right);
    controls.append(&turns);
    controls.append(&straighten_label);
    controls.append(&straighten);
    controls.append(&fit);
    controls.append(&reset);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    content.append(&canvas);
    content.append(&controls);
    editor.toasts.set_child(Some(&content));

    let cancel = gtk::Button::with_mnemonic("_Cancel");
    let save = gtk::Button::builder()
        .label("_Save")
        .use_underline(true)
        .css_classes(["suggested-action"])
        .build();
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    header.pack_start(&cancel);
    header.pack_end(&save);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&editor.toasts));

    let dialog = adw::Dialog::builder()
        .title(title)
        .content_width(960)
        .content_height(760)
        .child(&toolbar)
        .default_widget(&save)
        .build();
    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| {
            dialog.close();
        });
    }
    {
        let dialog = dialog.clone();
        let editor = editor.clone();
        let done = RefCell::new(Some(done));
        save.connect_clicked(move |_| {
            if let Some(done) = done.borrow_mut().take() {
                done(editor.edit.get());
            }
            dialog.close();
        });
    }
    dialog.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        // A 100 × 200 picture in a 400 × 300 area: scaled by 1.5 and
        // centred across.
        Layout::new((400.0, 300.0), (100, 200))
    }

    #[test]
    fn the_picture_is_shown_whole_and_centred() {
        let l = layout();
        assert_eq!(
            l,
            Layout {
                x: 125.0,
                y: 0.0,
                scale: 1.5,
                width: 150.0,
                height: 300.0
            }
        );
        assert_eq!(l.edges(Crop::FULL), (125.0, 0.0, 275.0, 300.0));
    }

    #[test]
    fn presses_grab_corners_edges_or_the_middle() {
        let l = layout();
        let crop = Crop::FULL;
        let at = |x, y| l.grip(crop, x, y);
        assert_eq!(
            at(126.0, 2.0),
            Some(Grip {
                left: true,
                top: true,
                right: false,
                bottom: false
            })
        );
        assert_eq!(at(274.0, 299.0).map(Grip::cursor), Some("nwse-resize"));
        assert_eq!(at(274.0, 1.0).map(Grip::cursor), Some("nesw-resize"));
        assert_eq!(at(130.0, 150.0).map(Grip::cursor), Some("ew-resize"));
        assert_eq!(at(200.0, 296.0).map(Grip::cursor), Some("ns-resize"));
        assert_eq!(at(200.0, 150.0), Some(Grip::MOVE));
        assert_eq!(at(20.0, 150.0), None);
    }

    #[test]
    fn dragging_moves_edges_within_bounds() {
        let l = layout();
        let left = Grip {
            left: true,
            top: false,
            right: false,
            bottom: false,
        };
        // 15 area pixels is a tenth of the 150-pixel-wide picture.
        let crop = l.dragged(Crop::FULL, left, 15.0, 40.0);
        assert!((crop.left - 0.1).abs() < 1e-6);
        assert_eq!((crop.top, crop.right, crop.bottom), (0.0, 1.0, 1.0));
        // An edge cannot cross the other.
        let crop = l.dragged(Crop::FULL, left, 1000.0, 0.0);
        assert!(crop.right - crop.left >= mylongbox_media::MIN_CROP - 1e-6);
        // Moving keeps the size and stays inside.
        let half = Crop {
            left: 0.25,
            top: 0.25,
            right: 0.75,
            bottom: 0.75,
        };
        let moved = l.dragged(half, Grip::MOVE, 1000.0, -1000.0);
        assert_eq!(
            moved,
            Crop {
                left: 0.5,
                top: 0.0,
                right: 1.0,
                bottom: 0.5
            }
        );
    }
}
