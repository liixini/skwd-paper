use super::{Particle, floor, num_of, perm, vec3_of};
use serde_json::Value;

pub(super) const POSITION_OFFSET: &str = "positionoffsetrandom";
const SCALE: &str = "scale";
const DISTANCE: &str = "distance";
const TIME_SCALE: &str = "timescale";
const DIRECTIONS: &str = "directions";
const SIGN: &str = "sign";
const OCTAVES: &str = "octaves";
const MAX_OCTAVES: u32 = 8;
const SKEW: f32 = 0.366_025_4;
const UNSKEW: f32 = 0.211_324_87;
const SIMPLEX_RADIUS: f32 = 0.5;
const OUTPUT_SCALE: f32 = 45.230_65;
const GRADIENT_AXIS_MASK: usize = 0x3c;
const GRADIENT_SIGN: usize = 1;
const GRADIENT_DOUBLE_SIGN: usize = 2;
const HASH_MASK: i64 = 255;

#[derive(Clone, Copy)]
pub struct PositionOffset {
    scale: f32,
    distance: f32,
    time_scale: f32,
    directions: [f32; 3],
    sign: [f32; 3],
    octaves: u32,
}

impl PositionOffset {
    pub(super) const ORTHOGRAPHIC: Self = Self {
        scale: 0.001,
        distance: 100.0,
        time_scale: 1.0,
        directions: [1.0, 1.0, 0.0],
        sign: [0.0; 3],
        octaves: 6,
    };
    pub(super) const OTHER: Self =
        Self { scale: 1.0, distance: 0.1, directions: [1.0; 3], ..Self::ORTHOGRAPHIC };

    pub(super) fn parse(entry: &Value, defaults: Self) -> Self {
        Self {
            scale: num_of(entry.get(SCALE), defaults.scale),
            distance: num_of(entry.get(DISTANCE), defaults.distance),
            time_scale: num_of(entry.get(TIME_SCALE), defaults.time_scale),
            directions: vec3_of(entry.get(DIRECTIONS), defaults.directions),
            sign: vec3_of(entry.get(SIGN), defaults.sign),
            octaves: (num_of(entry.get(OCTAVES), defaults.octaves as f32) as u32)
                .clamp(1, MAX_OCTAVES),
        }
    }

    pub(super) fn apply(&self, particle: &mut Particle, time: f32) {
        let time = time * self.time_scale;
        let points = [
            [particle.pos[0] * self.scale, time],
            [time, particle.pos[1] * self.scale],
            [particle.pos[2] * self.scale, -time],
        ];
        for (index, [x, y]) in points.into_iter().enumerate() {
            let mut noise = 0.0;
            let mut weight = 0.0;
            let mut frequency = 1.0;
            let mut amplitude = 1.0;
            for _ in 0..self.octaves {
                noise += simplex_two([x * frequency, y * frequency]) * amplitude;
                weight += amplitude;
                frequency *= 2.0;
                amplitude *= 0.5;
            }
            let offset = noise / weight * self.directions[index];
            let biased = offset * (1.0 - self.sign[index].abs()) + offset.abs() * self.sign[index];
            particle.pos[index] += biased * self.distance;
        }
    }
}

pub(super) fn simplex_two([x, y]: [f32; 2]) -> f32 {
    let skew = (x + y) * SKEW;
    let cell = [floor(x + skew), floor(y + skew)];
    let unskew = (cell[0] + cell[1]) * UNSKEW;
    let point = [x - (cell[0] - unskew), y - (cell[1] - unskew)];
    let first = [usize::from(point[0] > point[1]), usize::from(point[0] <= point[1])];
    let cell = cell.map(|value| (value as i64 & HASH_MASK) as usize);
    let mut sum = 0.0;
    for (corner, offset) in [[0; 2], first, [1; 2]].into_iter().enumerate() {
        let [x, y] =
            std::array::from_fn(|axis| point[axis] - offset[axis] as f32 + corner as f32 * UNSKEW);
        let weight = (SIMPLEX_RADIUS - x * x) - y * y;
        if weight < 0.0 {
            continue;
        }
        let hash = perm(cell[0] + offset[0] + perm(cell[1] + offset[1]));
        let (mut single, mut double) = if hash & GRADIENT_AXIS_MASK == 0 { (x, y) } else { (y, x) };
        if hash & GRADIENT_SIGN != 0 {
            single = -single;
        }
        double *= if hash & GRADIENT_DOUBLE_SIGN == 0 { 2.0 } else { -2.0 };
        sum += (single + double) * weight * weight * weight * weight;
    }
    sum * OUTPUT_SCALE
}

const GRID_X: u32 = 501_125_321;
const GRID_Y: u32 = 1_136_930_381;
const HASH_MULTIPLIER: u32 = 668_265_261;
const HASH_SHIFT: u32 = 15;
const AXIS_SWAP: u32 = 4;
const GRADIENT_WEIGHT: f32 = 2.414_213_7;
const SEEDED_SCALE: f32 = 38.283_688;
const LAST_CORNER: f32 = -0.577_350_26;
const FRACTAL_GAIN: f32 = 0.5;
const FRACTAL_LACUNARITY: f32 = 2.0;
const FRACTAL_OCTAVE_LIMIT: u32 = 32;

pub(super) fn octaves_of(value: &Value) -> Option<u32> {
    match value {
        Value::Null => Some(0),
        Value::Bool(flag) => Some(u32::from(*flag)),
        Value::Number(number) => Some(number.as_u64().map_or_else(
            || number.as_f64().map_or(0, |real| real as i64 as u32),
            |whole| whole as u32,
        )),
        Value::String(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

pub(super) struct Fractal {
    octaves: u32,
    normalization: f32,
}

impl Fractal {
    pub(super) fn new(octaves: u32) -> Self {
        let octaves = octaves.clamp(1, FRACTAL_OCTAVE_LIMIT);
        let mut amplitude = 1.0;
        let mut sum = amplitude;
        for _ in 1..octaves {
            amplitude *= FRACTAL_GAIN;
            sum += amplitude;
        }
        Self { octaves, normalization: 1.0 / sum }
    }

    pub(super) fn sample_plain(&self, mut x: f32) -> f32 {
        let mut amplitude = self.normalization;
        let mut sum = 0.0;
        for _ in 0..self.octaves {
            sum += super::simplex_one(x) * amplitude;
            x *= FRACTAL_LACUNARITY;
            amplitude *= FRACTAL_GAIN;
        }
        sum
    }

    pub(super) fn sample(&self, seed: u32, mut x: f32) -> f32 {
        let mut amplitude = self.normalization;
        let mut sum = seeded_simplex(seed, x) * amplitude;
        for octave in 1..self.octaves {
            x *= FRACTAL_LACUNARITY;
            amplitude *= FRACTAL_GAIN;
            sum += seeded_simplex(seed.wrapping_add(octave), x) * amplitude;
        }
        sum
    }
}

pub(super) fn seeded_simplex(seed: u32, x: f32) -> f32 {
    let skew = x * SKEW;
    let cell_x = floor(x + skew);
    let cell_y = floor(skew);
    let unskew = (cell_x + cell_y) * UNSKEW;
    let x = x - (cell_x - unskew);
    let y = -(cell_y - unskew);
    let first_x = i32::from(x > y);
    let first_y = 1 - first_x;
    let cell_x = cell_x as i32;
    let cell_y = cell_y as i32;
    let first = seeded_corner(seed, cell_x, cell_y, x, y);
    let middle = seeded_corner(
        seed,
        cell_x.wrapping_add(first_x),
        cell_y.wrapping_add(first_y),
        (x - first_x as f32) + UNSKEW,
        (y - first_y as f32) + UNSKEW,
    );
    let last = seeded_corner(
        seed,
        cell_x.wrapping_add(1),
        cell_y.wrapping_add(1),
        x + LAST_CORNER,
        y + LAST_CORNER,
    );
    (first + middle + last) * SEEDED_SCALE
}

fn seeded_corner(seed: u32, cell_x: i32, cell_y: i32, x: f32, y: f32) -> f32 {
    let hash = (seed ^ (cell_x as u32).wrapping_mul(GRID_X) ^ (cell_y as u32).wrapping_mul(GRID_Y))
        .wrapping_mul(HASH_MULTIPLIER);
    let hash = hash ^ ((hash as i32) >> HASH_SHIFT) as u32;
    let mut gradient_x = if hash & GRADIENT_SIGN as u32 == 0 { x } else { -x };
    let mut gradient_y = if hash & GRADIENT_DOUBLE_SIGN as u32 == 0 { y } else { -y };
    if hash & AXIS_SWAP != 0 {
        std::mem::swap(&mut gradient_x, &mut gradient_y);
    }
    let gradient = gradient_x * GRADIENT_WEIGHT + gradient_y;
    let weight = (-x * x + -y * y + SIMPLEX_RADIUS).max(0.0);
    let squared = weight * weight;
    gradient * squared * squared
}
