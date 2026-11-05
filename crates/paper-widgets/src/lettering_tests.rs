use super::*;

use crate::FONT;

fn lettering() -> Lettering {
    Lettering::open(FONT).unwrap()
}

#[test]
fn tracking_widens() {
    let lettering = lettering();
    let plain = lettering.width("2147", Line::new(40.0));
    let spaced = lettering.width("2147", Line::new(40.0).tracked(-6.0));
    assert!((plain - spaced - 18.0).abs() < 1e-3);
}

#[test]
fn baseline_centres_line() {
    let lettering = lettering();
    let mut canvas = Canvas::new(200, 100);
    lettering.draw(&mut canvas, "H", [10.0, 50.0], Line::new(60.0), Ink::solid([255; 3]));
    let metrics = paper_text::Font::parse(FONT, 60.0).unwrap().face().metrics(&[]).scale(60.0);
    let baseline = 50.0 + (metrics.ascent - metrics.descent) / 2.0;
    let bottom =
        (0..100).rev().find(|&y| (0..200).any(|x| canvas.pixels[y * 200 + x][3] > 128)).unwrap();
    assert!((bottom as f32 + 1.0 - baseline).abs() <= 1.0, "bottom {bottom} baseline {baseline}");
}

#[test]
fn rejects_garbage_font() {
    assert!(Lettering::open(vec![0u8; 64]).is_none());
}

#[test]
fn aligns_right() {
    let lettering = lettering();
    let mut canvas = Canvas::new(200, 60);
    let line = Line::new(30.0).aligned(1.0);
    let width = lettering.draw(&mut canvas, "47", [190.0, 30.0], line, Ink::solid([255; 3]));
    let right =
        (0..200).rev().find(|&x| (0..60).any(|y| canvas.pixels[y * 200 + x][3] > 0)).unwrap();
    assert!(width > 0.0 && (185..=191).contains(&right), "right {right}");
}

#[test]
fn sweep_drops_unused_sizes() {
    let lettering = lettering();
    let mut canvas = Canvas::new(200, 100);
    lettering.draw(&mut canvas, "47", [0.0, 50.0], Line::new(30.0), Ink::solid([255; 3]));
    lettering.sweep();
    assert_eq!(lettering.fonts.borrow().len(), 1);
    lettering.sweep();
    assert!(lettering.fonts.borrow().is_empty() && lettering.glyphs.borrow().is_empty());
}

#[test]
fn giant_glyphs_skip_cache() {
    let lettering = lettering();
    let mut canvas = Canvas::new(800, 800);
    lettering.draw(&mut canvas, "8", [0.0, 400.0], Line::new(700.0), Ink::solid([255; 3]));
    assert!(lettering.glyphs.borrow().is_empty());
    assert!(canvas.pixels.iter().any(|pixel| pixel[3] > 0));
}

#[test]
fn reads_font_file() {
    let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/RobotoCondensed-Bold.ttf"));
    assert_eq!(read_font(path).unwrap(), FONT);
    assert!(read_font(Path::new(env!("CARGO_MANIFEST_DIR"))).is_err());
}
