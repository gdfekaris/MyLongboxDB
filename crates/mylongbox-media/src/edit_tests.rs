//! Tests of the editor's turns, straightening, crop, and cover finder.

use image::imageops;
use image::{Rgb, RgbImage};

use crate::edit::{
    Crop, Edit, MAX_STRAIGHTEN, MIN_CROP, Mapping, apply, find_cover, longest_run, preview, sample,
    turned_size,
};

/// A picture where every pixel differs, so any misplacement shows.
fn pattern(width: u32, height: u32) -> RgbImage {
    RgbImage::from_fn(width, height, |x, y| {
        Rgb([
            (x * 7 % 256) as u8,
            (y * 5 % 256) as u8,
            ((x + y) % 251) as u8,
        ])
    })
}

fn turned(n: u8) -> Edit {
    Edit {
        quarter_turns: n,
        ..Edit::default()
    }
}

#[test]
fn no_edit_keeps_every_pixel() {
    let image = pattern(48, 32);
    assert!(Edit::default().is_identity());
    assert_eq!(apply(&image, &Edit::default()), image);
}

#[test]
fn quarter_turns_match_exact_rotations() {
    let image = pattern(48, 32);
    assert_eq!(apply(&image, &turned(1)), imageops::rotate90(&image));
    assert_eq!(apply(&image, &turned(2)), imageops::rotate180(&image));
    assert_eq!(apply(&image, &turned(3)), imageops::rotate270(&image));
    assert_eq!(apply(&image, &turned(4)), image);
    assert_eq!(turned_size(48, 32, 1), (32, 48));
    assert_eq!(turned_size(48, 32, 2), (48, 32));
}

#[test]
fn crops_take_exactly_their_part() {
    let image = pattern(80, 40);
    let edit = Edit {
        crop: Crop {
            left: 0.25,
            top: 0.5,
            right: 0.75,
            bottom: 1.0,
        },
        ..Edit::default()
    };
    assert_eq!(
        apply(&image, &edit),
        imageops::crop_imm(&image, 20, 20, 40, 20).to_image()
    );
}

#[test]
fn turning_keeps_the_crop_on_the_same_part() {
    let image = pattern(80, 48);
    let mut edit = Edit {
        crop: Crop {
            left: 0.125,
            top: 0.25,
            right: 0.5,
            bottom: 0.875,
        },
        ..Edit::default()
    };
    let before = apply(&image, &edit);
    edit.turn_right();
    assert_eq!(apply(&image, &edit), imageops::rotate90(&before));
    edit.turn_right();
    assert_eq!(apply(&image, &edit), imageops::rotate180(&before));
    edit.turn_left();
    edit.turn_left();
    assert_eq!(apply(&image, &edit), before);
    edit.turn_left();
    assert_eq!(edit.quarter_turns, 3);
    assert_eq!(apply(&image, &edit), imageops::rotate270(&before));
}

#[test]
fn straightening_leaves_no_empty_corners() {
    let image = RgbImage::from_pixel(60, 90, Rgb([200, 120, 40]));
    for degrees in [-MAX_STRAIGHTEN, -7.5, 3.0, MAX_STRAIGHTEN] {
        let mut edit = Edit::default();
        edit.set_straighten(degrees);
        let out = apply(&image, &edit);
        assert_eq!(out.dimensions(), (60, 90));
        assert!(
            out.pixels().all(|p| *p == Rgb([200, 120, 40])),
            "{degrees}°"
        );
    }
}

#[test]
fn straightening_zooms_just_enough_to_fill_the_frame() {
    for (w, h) in [(60, 90), (90, 60), (80, 80)] {
        for turns in [0, 1] {
            for degrees in [-MAX_STRAIGHTEN, -4.0, 9.0, MAX_STRAIGHTEN] {
                let mut edit = Edit {
                    quarter_turns: turns,
                    ..Edit::default()
                };
                edit.set_straighten(degrees);
                let map = Mapping::new(w, h, &edit);
                let (fw, fh) = turned_size(w, h, turns);
                let (fw, fh) = (fw as f32, fh as f32);
                let (ow, oh) = (w as f32, h as f32);
                let corners =
                    [(0.0, 0.0), (fw, 0.0), (0.0, fh), (fw, fh)].map(|(x, y)| map.source(x, y));
                let what = format!("{w}x{h}, {turns} turns, {degrees}°");
                // Every corner shows part of the photo...
                for (x, y) in corners {
                    assert!(
                        (-1e-3..=ow + 1e-3).contains(&x) && (-1e-3..=oh + 1e-3).contains(&y),
                        "{what}: corner from ({x}, {y})"
                    );
                }
                // ...and at least one reaches its edge, so no more is cut
                // away than needed.
                let gap = corners
                    .iter()
                    .map(|(x, y)| x.min(ow - x).min(*y).min(oh - y))
                    .fold(f32::MAX, f32::min);
                assert!(gap < 0.01, "{what}: zoomed in {gap} too far");
            }
        }
    }
}

#[test]
fn colours_between_pixels_are_blended() {
    let image = RgbImage::from_fn(2, 2, |x, y| Rgb([(x * 200) as u8, (y * 100) as u8, 50]));
    assert_eq!(sample(&image, 0.0, 0.0), Rgb([0, 0, 50]));
    assert_eq!(sample(&image, 0.5, 0.0), Rgb([100, 0, 50]));
    assert_eq!(sample(&image, 0.0, 0.5), Rgb([0, 50, 50]));
    assert_eq!(sample(&image, 0.25, 0.75), Rgb([50, 75, 50]));
    // Points outside are held at the edge.
    assert_eq!(sample(&image, -3.0, 9.0), Rgb([0, 100, 50]));
}

#[test]
fn the_longest_run_wins() {
    let lines = [true, false, true, true, true, false, true, true];
    assert_eq!(longest_run(&lines), Some((2, 5)));
    assert_eq!(longest_run(&[false, true, true]), Some((1, 3)));
    assert_eq!(longest_run(&[false, false]), None);
}

#[test]
fn straightening_turns_clockwise() {
    // A dark upright line on white; turned clockwise, its top leans
    // right and its bottom left.
    let image = RgbImage::from_fn(101, 101, |x, _| {
        if x.abs_diff(50) <= 1 {
            Rgb([0, 0, 0])
        } else {
            Rgb([255, 255, 255])
        }
    });
    let mut edit = Edit::default();
    edit.set_straighten(10.0);
    let out = apply(&image, &edit);
    let darkest = |y: u32| (0..101).min_by_key(|x| out.get_pixel(*x, y).0[0]).unwrap();
    assert!(darkest(5) > 55, "top at {}", darkest(5));
    assert!(darkest(95) < 45, "bottom at {}", darkest(95));
    assert!(darkest(50).abs_diff(50) <= 1);
}

#[test]
fn straightening_stays_within_its_limits() {
    let mut edit = Edit::default();
    edit.set_straighten(40.0);
    assert_eq!(edit.straighten, MAX_STRAIGHTEN);
    edit.set_straighten(-40.0);
    assert_eq!(edit.straighten, -MAX_STRAIGHTEN);
    edit.set_straighten(f32::NAN);
    assert_eq!(edit.straighten, 0.0);
    // An out-of-range value set directly is limited when applied.
    let wild = Edit {
        straighten: 80.0,
        ..Edit::default()
    };
    assert_eq!(
        Mapping::new(40, 40, &wild).radians,
        MAX_STRAIGHTEN.to_radians()
    );
}

#[test]
fn crops_are_kept_sensible() {
    let flipped = Crop {
        left: 0.8,
        top: 0.9,
        right: 0.2,
        bottom: 0.1,
    }
    .clamped();
    assert_eq!(
        flipped,
        Crop {
            left: 0.2,
            top: 0.1,
            right: 0.8,
            bottom: 0.9
        }
    );
    let outside = Crop {
        left: -1.0,
        top: -0.5,
        right: 3.0,
        bottom: 1.5,
    }
    .clamped();
    assert_eq!(outside, Crop::FULL);
    let tiny = Crop {
        left: 0.5,
        top: 0.98,
        right: 0.5,
        bottom: 0.99,
    }
    .clamped();
    assert!((tiny.right - tiny.left - MIN_CROP).abs() < 1e-6);
    assert!((tiny.bottom - tiny.top - MIN_CROP).abs() < 1e-6);
    assert!(tiny.bottom <= 1.0);
    let broken = Crop {
        left: f32::NAN,
        top: f32::INFINITY,
        right: f32::NAN,
        bottom: f32::NEG_INFINITY,
    }
    .clamped();
    assert_eq!(broken, Crop::FULL, "a broken crop falls back to everything");
    assert_eq!(Crop::FULL.pixels(30, 20), (0, 0, 30, 20));
    assert_eq!(tiny.pixels(1, 1), (0, 0, 1, 1));
    let sliver = Crop {
        left: 0.1,
        top: 0.1,
        right: 0.15,
        bottom: 0.15,
    };
    assert_eq!(sliver.pixels(1, 1), (0, 0, 1, 1), "never less than a pixel");
}

#[test]
fn previews_are_small_copies() {
    assert_eq!(
        preview(&pattern(3000, 2000), 1200).dimensions(),
        (1200, 800)
    );
    assert_eq!(
        preview(&pattern(2000, 3000), 1200).dimensions(),
        (800, 1200)
    );
    assert_eq!(preview(&pattern(300, 200), 1200).dimensions(), (300, 200));
}

/// A busy cover at the given shares of a `width` × `height` photo on a
/// plain, slightly uneven background.
fn photographed(width: u32, height: u32, cover: Crop, background: [u8; 3]) -> RgbImage {
    RgbImage::from_fn(width, height, |x, y| {
        let (fx, fy) = (x as f32 / width as f32, y as f32 / height as f32);
        if fx >= cover.left && fx < cover.right && fy >= cover.top && fy < cover.bottom {
            Rgb([
                (x * 13 % 200) as u8 + 30,
                (y * 7 % 180) as u8,
                ((x / 9 + y / 11) % 2 * 200) as u8,
            ])
        } else {
            let grain = ((x * 31 + y * 17) % 9) as u8;
            Rgb(background.map(|c| c.saturating_add(grain)))
        }
    })
}

fn near(a: Crop, b: Crop) -> bool {
    [
        (a.left, b.left),
        (a.top, b.top),
        (a.right, b.right),
        (a.bottom, b.bottom),
    ]
    .iter()
    .all(|(x, y)| (x - y).abs() < 0.02)
}

#[test]
fn the_cover_is_found_on_a_plain_background() {
    let cover = Crop {
        left: 0.2,
        top: 0.1,
        right: 0.8,
        bottom: 0.9,
    };
    for background in [[120, 110, 100], [20, 20, 25], [235, 235, 230]] {
        let found = find_cover(&photographed(600, 800, cover, background)).unwrap();
        assert!(near(found, cover), "{background:?}: {found:?}");
    }
    let off_centre = Crop {
        left: 0.05,
        top: 0.3,
        right: 0.6,
        bottom: 0.95,
    };
    let found = find_cover(&photographed(900, 700, off_centre, [90, 60, 40])).unwrap();
    assert!(near(found, off_centre), "{found:?}");
    // A cover taking up only a quarter of the photo's width.
    let narrow = Crop {
        left: 0.4,
        top: 0.1,
        right: 0.65,
        bottom: 0.9,
    };
    let found = find_cover(&photographed(800, 600, narrow, [120, 110, 100])).unwrap();
    assert!(near(found, narrow), "{found:?}");
}

#[test]
fn nothing_is_proposed_without_a_clear_cover() {
    // Plain, filled to the edges, or too small to judge.
    assert_eq!(
        find_cover(&RgbImage::from_pixel(400, 600, Rgb([128, 128, 128]))),
        None
    );
    assert_eq!(find_cover(&pattern(400, 600)), None);
    assert_eq!(find_cover(&pattern(10, 10)), None);
    // A cover that fills almost the whole photo leaves nothing to crop.
    let almost = Crop {
        left: 0.01,
        top: 0.01,
        right: 0.99,
        bottom: 0.99,
    };
    assert_eq!(
        find_cover(&photographed(400, 600, almost, [128, 128, 128])),
        None
    );
}
