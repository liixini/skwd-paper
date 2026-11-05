use std::f32::consts::SQRT_2;

use paper_control::ClockStyle;

use crate::canvas::{Canvas, Ink};
use crate::lettering::Line;
use crate::scene::Scene;
use crate::shape;

struct Aperture {
    size: f32,
    spacing: f32,
    offset: f32,
    row: f32,
    bare: [f32; 2],
    idle: f32,
    inner_pad: f32,
    inner_width: f32,
    outer_pad: f32,
    outer_width: f32,
    outer_alpha: f32,
    corner: f32,
}

const APERTURE: Aperture = Aperture {
    size: 150.0,
    spacing: -6.0,
    offset: 0.22,
    row: 0.41,
    bare: [400.0, 280.0],
    idle: 452.0,
    inner_pad: 14.0,
    inner_width: 2.0,
    outer_pad: 48.0,
    outer_width: 1.0,
    outer_alpha: 0.4,
    corner: 12.0,
};

struct Greeter {
    size: f32,
    spacing: f32,
    accent_from: usize,
    date: Line,
    date_gap: f32,
    frame: [f32; 2],
}

const GREETER: Greeter = Greeter {
    size: 44.0,
    spacing: -1.5,
    accent_from: 3,
    date: Line::new(12.0).tracked(2.64),
    date_gap: 38.0,
    frame: [150.0, 70.0],
};

struct Control {
    clock: Line,
    date: Line,
    date_gap: f32,
    spine_gap: f32,
    spine_width: f32,
    frame: [f32; 2],
}

const CONTROL: Control = Control {
    clock: Line::new(34.0).aligned(0.5),
    date: Line::new(9.5).tracked(1.9).aligned(0.5),
    date_gap: 28.0,
    spine_gap: 74.0,
    spine_width: 1.4,
    frame: [260.0, 160.0],
};

struct Dial {
    reach: f32,
    margin: f32,
    inset: f32,
    thread: f32,
    pitch: f32,
    hatch: f32,
    hatch_keep: f32,
    knob: f32,
    bead_rim: f32,
    date: Line,
    month: Line,
    stagger: [f32; 2],
    outline: f32,
}

const DIAL: Dial = Dial {
    reach: 140.0,
    margin: 10.0,
    inset: 1.5,
    thread: 1.5,
    pitch: 7.0,
    hatch: 2.0,
    hatch_keep: 120.0 / 255.0,
    knob: 9.0,
    bead_rim: 1.5,
    date: Line::new(64.0).aligned(0.5),
    month: Line::new(40.0).aligned(0.5),
    stagger: [14.0, 30.0],
    outline: 2.0,
};

const OUTLINE_OFFSETS: [[f32; 2]; 8] = [
    [-1.0, -1.0],
    [0.0, -1.0],
    [1.0, -1.0],
    [-1.0, 0.0],
    [1.0, 0.0],
    [-1.0, 1.0],
    [0.0, 1.0],
    [1.0, 1.0],
];

pub fn frame(style: ClockStyle) -> [f32; 2] {
    match style {
        ClockStyle::Aperture => APERTURE.bare,
        ClockStyle::ApertureRing => [ring_side(); 2],
        ClockStyle::Greeter => GREETER.frame,
        ClockStyle::ControlCenter => CONTROL.frame,
        ClockStyle::DayDial => [2.0 * (DIAL.reach + DIAL.margin); 2],
    }
}

pub fn draw(style: ClockStyle, canvas: &mut Canvas, scene: &Scene<'_>) {
    match style {
        ClockStyle::Aperture => {
            let [width, height] = APERTURE.bare;
            aperture(canvas, scene, [width / 2.0, height / 2.0]);
        }
        ClockStyle::ApertureRing => ring(canvas, scene),
        ClockStyle::Greeter => greeter(canvas, scene),
        ClockStyle::ControlCenter => control(canvas, scene),
        ClockStyle::DayDial => dial(canvas, scene),
    }
}

fn ring_side() -> f32 {
    (APERTURE.idle + APERTURE.outer_pad) * SQRT_2 + APERTURE.corner * 2.0
}

fn aperture(canvas: &mut Canvas, scene: &Scene<'_>, centre: [f32; 2]) {
    let line = Line::new(APERTURE.size).tracked(APERTURE.spacing);
    let groups = [
        (scene.moment.hours(), -1.0, scene.tones.text),
        (scene.moment.minutes(), 1.0, scene.tones.accent),
    ];
    for (group, side, ink) in groups {
        let width = scene.lettering.width(&group, line);
        let x = centre[0] + side * APERTURE.size * APERTURE.offset - width / 2.0;
        let y = centre[1] + side * APERTURE.size * APERTURE.row;
        scene.lettering.draw(canvas, &group, [x, y], line, ink);
    }
}

fn ring(canvas: &mut Canvas, scene: &Scene<'_>) {
    let half = ring_side() / 2.0;
    let centre = [half, half];
    let accent = scene.tones.accent;
    canvas.stroke(
        &shape::tilted(centre, APERTURE.idle + APERTURE.inner_pad),
        APERTURE.inner_width,
        accent,
    );
    let outer = APERTURE.idle + APERTURE.outer_pad;
    canvas.stroke(
        &shape::tilted(centre, outer),
        APERTURE.outer_width,
        accent.fade(APERTURE.outer_alpha),
    );
    let reach = outer / SQRT_2;
    for [dx, dy] in [[0.0, -reach], [reach, 0.0], [0.0, reach], [-reach, 0.0]] {
        canvas.fill(&shape::tilted([centre[0] + dx, centre[1] + dy], APERTURE.corner), accent);
    }
    aperture(canvas, scene, centre);
}

fn greeter(canvas: &mut Canvas, scene: &Scene<'_>) {
    let clock = format!("{}:{}", scene.moment.hours(), scene.moment.minutes());
    let line = Line::new(GREETER.size);
    let middle = GREETER.size / 2.0;
    let mut x = 0.0;
    for (index, ch) in clock.chars().enumerate() {
        let ink = if index >= GREETER.accent_from { scene.tones.accent } else { scene.tones.text };
        x +=
            scene.lettering.draw(canvas, &ch.to_string(), [x, middle], line, ink) + GREETER.spacing;
    }
    scene.lettering.draw(
        canvas,
        &scene.short_date(),
        [0.0, middle + GREETER.date_gap],
        GREETER.date,
        scene.tones.muted,
    );
}

fn control(canvas: &mut Canvas, scene: &Scene<'_>) {
    let x = CONTROL.frame[0] / 2.0;
    let top = CONTROL.clock.size / 2.0;
    let clock = format!("{}:{}", scene.moment.hours(), scene.moment.minutes());
    scene.lettering.draw(canvas, &clock, [x, top], CONTROL.clock, scene.tones.text);
    scene.lettering.draw(
        canvas,
        &scene.long_date(),
        [x, top + CONTROL.date_gap],
        CONTROL.date,
        scene.tones.text,
    );
    let spine = shape::line([x, top + CONTROL.spine_gap], [x, CONTROL.frame[1]]);
    canvas.stroke(&spine, CONTROL.spine_width, scene.tones.accent);
}

fn dial(canvas: &mut Canvas, scene: &Scene<'_>) {
    let middle = DIAL.reach + DIAL.margin;
    let centre = [middle, middle];
    let tones = scene.tones;
    let share = scene.moment.share();
    canvas.stroke(&shape::diamond(centre, DIAL.reach), DIAL.thread, tones.accent);
    canvas.fill(&shape::diamond(centre, DIAL.reach - DIAL.inset), tones.shade);
    let swept = shape::sweep(centre, DIAL.reach, share);
    let mut cut = canvas.coverage(&swept, None);
    let lines = canvas.coverage(&hatch(centre), Some(DIAL.hatch));
    for (level, line) in cut.iter_mut().zip(lines) {
        *level = (u16::from(*level) * u16::from(line) / u16::from(u8::MAX)) as u8;
    }
    canvas.cover_all(&cut, Ink::solid(tones.shade.mix(tones.accent, DIAL.hatch_keep)));
    let knob = shape::rim_at(centre, DIAL.reach, share);
    canvas.fill(&shape::diamond(knob, DIAL.knob), tones.accent);
    canvas.fill(&shape::diamond(knob, DIAL.knob - DIAL.bead_rim), tones.text);
    let [shift_x, shift_y] = DIAL.stagger;
    let day = scene.moment.day.to_string();
    let month = &scene.labels.months[scene.moment.month];
    outlined(canvas, scene, &day, [middle - shift_x, middle - shift_y], DIAL.date, tones.text);
    outlined(canvas, scene, month, [middle + shift_x, middle + shift_y], DIAL.month, tones.accent);
}

fn hatch(centre: [f32; 2]) -> Vec<zeno::Command> {
    let reach = DIAL.reach;
    let step = DIAL.pitch * SQRT_2;
    let [low_x, low_y] = [centre[0] - reach, centre[1] - reach];
    let [high_x, high_y] = [centre[0] + reach, centre[1] + reach];
    let mut path = Vec::new();
    let mut sum = ((low_x + low_y) / step).floor() * step;
    while sum < high_x + high_y {
        path.extend(shape::line([sum - low_y, low_y], [sum - high_y, high_y]));
        sum += step;
    }
    path
}

fn outlined(
    canvas: &mut Canvas,
    scene: &Scene<'_>,
    text: &str,
    at: [f32; 2],
    line: Line,
    ink: Ink,
) {
    for [dx, dy] in OUTLINE_OFFSETS {
        let shifted = [at[0] + dx * DIAL.outline, at[1] + dy * DIAL.outline];
        scene.lettering.draw(canvas, text, shifted, line, scene.tones.shade);
    }
    scene.lettering.draw(canvas, text, at, line, ink);
}

#[cfg(test)]
#[path = "clock_tests.rs"]
mod tests;
