use paper_control::{Pin, WIDGET_HEIGHTS};

const SNAP_LIMIT: u32 = 8;
const SNAP_TOLERANCE: f32 = 1e-3;
const THIRDS: [f32; 2] = [1.0 / 3.0, 2.0 / 3.0];
const ANCHORS: [f32; 3] = [0.0, 0.5, 1.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub position: [i32; 2],
    pub frame: [i32; 2],
    pub ink: f32,
    pub origin: [u32; 2],
    pub size: [usize; 2],
    pub shift: [f32; 2],
    pub bounds: [f32; 4],
}

struct Sizing {
    scale: f32,
    quantum: f32,
    grow: f32,
    sized: [f32; 2],
}

pub fn place(
    logical: [u32; 2],
    buffer: [u32; 2],
    frame: [f32; 2],
    pin: Pin,
    height: Option<f32>,
) -> Option<Place> {
    let Sizing { scale, quantum, grow, sized } = sizing(logical, buffer, frame, height)?;
    let output = logical.map(|side| side as f32);
    let position: [f32; 2] = std::array::from_fn(|axis| {
        let free = output[axis] - sized[axis];
        let low = (free.min(0.0) / quantum).floor() * quantum;
        let high = (free.max(0.0) / quantum).floor() * quantum;
        let wanted = pin.anchor[axis] * free + pin.offset[axis];
        ((wanted / quantum).round() * quantum).clamp(low, high)
    });
    let start = position.map(|value| value.max(0.0));
    let end: [f32; 2] =
        std::array::from_fn(|axis| (position[axis] + sized[axis]).min(output[axis]));
    if (0..2).any(|axis| end[axis] <= start[axis]) {
        return None;
    }
    let physical = |value: f32| (value * scale).round();
    let origin = start.map(physical);
    let size: [usize; 2] =
        std::array::from_fn(|axis| (physical(end[axis]) - origin[axis]) as usize);
    Some(Place {
        position: start.map(|value| value as i32),
        frame: std::array::from_fn(|axis| (end[axis] - start[axis]) as i32),
        ink: scale * grow,
        origin: origin.map(|value| value as u32),
        size,
        shift: std::array::from_fn(|axis| origin[axis] - physical(position[axis])),
        bounds: [position[0], position[1], sized[0], sized[1]],
    })
}

pub fn sized(
    logical: [u32; 2],
    buffer: [u32; 2],
    frame: [f32; 2],
    height: Option<f32>,
) -> Option<[f32; 2]> {
    sizing(logical, buffer, frame, height).map(|sizing| sizing.sized)
}

fn sizing(
    logical: [u32; 2],
    buffer: [u32; 2],
    frame: [f32; 2],
    height: Option<f32>,
) -> Option<Sizing> {
    if logical.contains(&0) || buffer.contains(&0) {
        return None;
    }
    let scale = buffer[0] as f32 / logical[0] as f32;
    let quantum = snap(scale);
    let whole = |side: f32| ((side / quantum) - SNAP_TOLERANCE).ceil() * quantum;
    let grow = height.map_or(1.0, |share| whole(share * logical[1] as f32) / frame[1]);
    let sized = frame.map(|side| whole(side * grow));
    Some(Sizing { scale, quantum, grow, sized })
}

pub fn pin(bounds: [f32; 4], output: [f32; 2]) -> (Pin, f32) {
    let [x, y, w, h] = bounds;
    let (corner, size) = ([x, y], [w, h]);
    let anchor: [f32; 2] =
        std::array::from_fn(|axis| third(corner[axis] + size[axis] / 2.0, output[axis]));
    let offset =
        std::array::from_fn(|axis| corner[axis] - anchor[axis] * (output[axis] - size[axis]));
    let height = (h / output[1]).clamp(*WIDGET_HEIGHTS.start(), *WIDGET_HEIGHTS.end());
    (Pin { anchor, offset }, height)
}

fn third(centre: f32, size: f32) -> f32 {
    let share = centre / size;
    let column = THIRDS.iter().take_while(|edge| share >= **edge).count();
    ANCHORS[column]
}

pub fn snap(scale: f32) -> f32 {
    (1..=SNAP_LIMIT)
        .map(|step| step as f32)
        .find(|step| ((step * scale).round() - step * scale).abs() < SNAP_TOLERANCE)
        .unwrap_or(1.0)
}

#[cfg(test)]
#[path = "place_tests.rs"]
mod tests;
