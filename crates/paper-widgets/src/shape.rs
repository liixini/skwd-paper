use std::f32::consts::SQRT_2;

const SIDES: usize = 4;

use zeno::{Command, PathBuilder};

pub fn diamond(centre: [f32; 2], reach: f32) -> Vec<Command> {
    let [x, y] = centre;
    let mut path = Vec::new();
    path.move_to([x, y - reach])
        .line_to([x + reach, y])
        .line_to([x, y + reach])
        .line_to([x - reach, y])
        .close();
    path
}

pub fn corners(centre: [f32; 2], reach: f32) -> [[f32; 2]; SIDES] {
    let [x, y] = centre;
    [[x, y - reach], [x + reach, y], [x, y + reach], [x - reach, y]]
}

pub fn rim_at(centre: [f32; 2], reach: f32, fraction: f32) -> [f32; 2] {
    let corners = corners(centre, reach);
    let span = fraction.clamp(0.0, 1.0) * SIDES as f32;
    let edge = (span as usize).min(SIDES - 1);
    let along = span - edge as f32;
    let ([from_x, from_y], [to_x, to_y]) = (corners[edge], corners[(edge + 1) % SIDES]);
    [from_x + (to_x - from_x) * along, from_y + (to_y - from_y) * along]
}

pub fn sweep(centre: [f32; 2], reach: f32, fraction: f32) -> Vec<Command> {
    let corners = corners(centre, reach);
    let passed = ((fraction.clamp(0.0, 1.0) * SIDES as f32) as usize).min(SIDES - 1);
    let mut path = Vec::new();
    path.move_to(centre);
    for corner in &corners[..=passed] {
        path.line_to(*corner);
    }
    path.line_to(rim_at(centre, reach, fraction)).close();
    path
}

pub fn tilted(centre: [f32; 2], side: f32) -> Vec<Command> {
    diamond(centre, side / SQRT_2)
}

pub fn line(from: [f32; 2], to: [f32; 2]) -> Vec<Command> {
    let mut path = Vec::new();
    path.move_to(from).line_to(to);
    path
}

#[cfg(test)]
#[path = "shape_tests.rs"]
mod tests;
