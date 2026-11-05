use super::*;
use zeno::PathBuilder;

fn square(side: f32) -> Vec<Command> {
    let mut path = Vec::new();
    path.move_to([0.0, 0.0])
        .line_to([side, 0.0])
        .line_to([side, side])
        .line_to([0.0, side])
        .close();
    path
}

#[test]
fn fills_premultiplied() {
    let mut canvas = Canvas::new(4, 4);
    canvas.fill(&square(2.0), Ink::solid([200, 100, 0]).fade(0.5));
    assert_eq!(canvas.pixels[0], [100, 50, 0, 128]);
    assert_eq!(canvas.pixels[3], [0; 4]);
}

#[test]
fn clips_to_bounds() {
    let mut canvas = Canvas::new(2, 2);
    canvas.fill(&square(8.0), Ink::solid([255; 3]));
    assert!(canvas.pixels.iter().all(|pixel| *pixel == [255; 4]));
}

#[test]
fn occlusion_clears() {
    let mut canvas = Canvas::new(2, 1);
    canvas.fill(&square(4.0), Ink::solid([255; 3]));
    canvas.occlude(&[255, 0]);
    assert_eq!(canvas.pixels, vec![[0; 4], [255; 4]]);
}

#[test]
fn scale_widens_strokes() {
    let mut path = Vec::new();
    path.move_to([0.0, 4.0]).line_to([8.0, 4.0]);
    let mut canvas = Canvas::scaled([8.0, 8.0], 2.0);
    canvas.stroke(&path, 2.0, Ink::solid([255; 3]));
    let solid =
        (0..canvas.height).filter(|y| canvas.pixels[y * canvas.width + 8][3] == u8::MAX).count();
    assert_eq!([canvas.width, canvas.height], [16, 16]);
    assert_eq!(solid, 4);
}

#[test]
fn window_shifts_drawing() {
    let mut whole = Canvas::scaled([8.0, 8.0], 1.0);
    let mut part = Canvas::window([4, 4], 1.0, [4.0, 4.0]);
    let mut path = Vec::new();
    path.move_to([5.0, 5.0]).line_to([7.0, 5.0]).line_to([7.0, 7.0]).line_to([5.0, 7.0]).close();
    whole.fill(&path, Ink::solid([255; 3]));
    part.fill(&path, Ink::solid([255; 3]));
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(part.pixels[y * 4 + x], whole.pixels[(y + 4) * 8 + x + 4]);
        }
    }
}

#[test]
fn offscreen_shapes_skip() {
    let mut canvas = Canvas::window([4, 4], 1.0, [100.0, 100.0]);
    let mut path = Vec::new();
    path.move_to([0.0, 0.0]).line_to([10.0, 0.0]).line_to([10.0, 10.0]).close();
    canvas.fill(&path, Ink::solid([255; 3]));
    assert!(canvas.pixels.iter().all(|pixel| *pixel == CLEAR));
}
