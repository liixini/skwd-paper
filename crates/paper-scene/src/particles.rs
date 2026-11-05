use crate::matrix::{cross3, dot3};
use crate::model::ProjectionMode;
use crate::pkg::Package;
use serde_json::Value;
use std::cell::Cell;
use std::collections::BTreeMap;

#[path = "particles_audio.rs"]
mod audio;
pub use audio::AudioResponse;

#[path = "particles_children.rs"]
mod children;

#[path = "particles_image.rs"]
pub mod image;

#[path = "particles_sphere.rs"]
pub mod sphere;
pub use children::{ChildEvent, ChildEventKind, Followers};
use sphere::SphereShape;

#[path = "particles_sequence.rs"]
mod sequence;
use sequence::BetweenSequence;

#[path = "particles_noise.rs"]
mod noise;
use noise::PositionOffset;

#[path = "particles_event.rs"]
mod event;
use event::{EventInheritance, EventValues};

#[path = "particles_control.rs"]
mod control;
pub(crate) use control::SPATIAL_AXES;
use control::{InheritVelocity, MaintainBetween, MaintainDistance, ReduceMovement};

#[path = "particles_remap.rs"]
mod remap;
use remap::Remap;

#[path = "particles_vortex.rs"]
mod vortex;

#[path = "particles_boids.rs"]
mod boids;

#[path = "particles_collision.rs"]
mod collision;

#[path = "particles_dependency.rs"]
mod dependency;

#[path = "particles_model.rs"]
pub mod model_collision;

#[path = "particles_operators.rs"]
mod operators;

const REDUCE_MOVEMENT: &str = "reducemovementnearcontrolpoint";
const MAINTAIN_BETWEEN: &str = "maintaindistancebetweencontrolpoints";
const MAINTAIN_DISTANCE: &str = "maintaindistancetocontrolpoint";
const INHERIT_CONTROL_VELOCITY: &str = "inheritcontrolpointvelocity";

const MAX_PARTICLES: usize = 32_768;
const MAX_OBJECT_SYSTEMS: usize = 256;
const MAX_CHILD_DEPTH: usize = 8;
pub const MAX_INSTANCE_SIZE_SCALE: f32 = 10.0;
pub const MAX_INSTANCE_SPEED_SCALE: f32 = 100.0;
pub const MAX_INSTANCE_LIFETIME_SCALE: f32 = 100.0;
const SIZE_TO_NATIVE: f32 = 0.5;
pub const MAX_TRAIL_SEGMENTS: usize = 32;
const DEFAULT_TRAIL_SEGMENTS: usize = 4;
const MIN_TRAIL_SEGMENTS: usize = 2;
const MIN_TRAIL_LENGTH: f32 = 0.001;
const MAX_PREWARM_STEPS: usize = 600;
const MIN_PREWARM_STEP: f32 = 0.001;
const PREWARM_PARTICLE_THRESHOLD: usize = 500;
const PREWARM_SMALL_STEP: f32 = 0.05;
const PREWARM_LARGE_STEP: f32 = 0.2;
const DRAG_REFERENCE_DT: f32 = 0.025;
const DRAG_EXPONENT: f32 = 0.7;
const STEP_NAME: &str = "name";
const UNNAMED_STEP: &str = "unnamed";
const RENDERER_ORIENTATION: &str = "orientation";
const RENDERER_UPRIGHT: &str = "upright";
const RENDERER_FIXED: &str = "fixed";
const RENDERER_FLAGS: &str = "flags";
const RENDERER_LENGTH: &str = "length";
const RENDERER_MIN_LENGTH: &str = "minlength";
const RENDERER_MAX_LENGTH: &str = "maxlength";
const RENDERER_SEGMENTS: &str = "segments";
const RENDERER_FADE_ALPHA: &str = "fadealpha";
const RENDERER_FADE_SIZE: &str = "fadesize";
const RENDERER_UV_SCROLL: &str = "uvscrolling";
const RENDERER_UV_SCALE: &str = "uvscale";
const TRAIL_SCROLL_COMBO: &str = "TRAILSCROLLALPHA";
const TRAIL_ALPHA_COMBO: &str = "TRAILFADEALPHA";
const TRAIL_SIZE_COMBO: &str = "TRAILFADESIZE";
const RENDERER_SUBDIVISION: &str = "subdivision";
const MATERIAL_PASSES: &str = "passes";
const MATERIAL_SHADER: &str = "shader";
const SPRITE_SHADER: &str = "genericparticle";
const ROPE_SHADER: &str = "genericropeparticle";
const TRAIL_SUBDIVISION_COMBO: &str = "TRAILSUBDIVISION";
const DEFAULT_ROPE_SUBDIVISION: f32 = 3.0;
const DEFAULT_SPRITE_TRAIL_LIMIT: f32 = 10.0;
const CAP_VELOCITY: &str = "capvelocity";
const MAX_SPEED: &str = "maxspeed";
const COLLISION_PLANE: &str = "collisionplane";
const COLLISION_SPHERE: &str = "collisionsphere";
const COLLISION_BOUNDS: &str = "collisionbounds";
const COLLISION_QUAD: &str = "collisionquad";
const COLLISION_BOX: &str = "collisionbox";
const WORLD_SPACE_FLAG: i64 = 1;
const COLLISION_BEHAVIOR: &str = "collisionbehavior";
const COLLISION_SLIDE: &str = "slide";
const COLLISION_STOP: &str = "stop";
const COLLISION_DELETE: &str = "delete";
const COLLISION_FLAGS: &str = "flags";
const EMITTER_FIELD: &str = "emitter";
const COLLISION_CONTROL_POINT_FLAG: u32 = 1;
const COLLISION_ANGULAR_STOP_FLAG: u32 = 2;
const COLLISION_BOUNCE_FACTOR: &str = "bouncefactor";
const COLLISION_PLANE_NORMAL: &str = "plane";
const COLLISION_DISTANCE: &str = "distance";
const COLLISION_ORIGIN: &str = "origin";
const COLLISION_RADIUS: &str = "radius";
const COLLISION_DEFAULT_BOUNCE: f32 = 0.5;
const BOUNDS_PLANE_COUNT: usize = 4;
const BOUNDS_VECTOR_COMPONENTS: usize = 3;
const BOUNDS_PARENT: &str = "parent";
const BOUNDS_CHILDREN: &str = "children";
const BOUNDS_TYPE: &str = "type";
const BOUNDS_ANGLES: &str = "angles";
const BOUNDS_SCALE: &str = "scale";
const BOUNDS_EVENT_FOLLOW: &str = "eventfollow";
const PLANE_DEFAULT_NORMAL: [f32; 3] = [0.0, 1.0, 0.0];
const BLEND_IN_START: &str = "blendinstart";
const BLEND_IN_END: &str = "blendinend";
const BLEND_OUT_START: &str = "blendoutstart";
const BLEND_OUT_END: &str = "blendoutend";
const BLEND_MIN_SPAN: f32 = 0.0001;
const BLEND_ACTIVE_WINDOW: f32 = 0.01;
const HSV_COLOR_RANDOM: &str = "hsvcolorrandom";
const COLOR_LIST: &str = "colorlist";
const HUE_MIN: &str = "huemin";
const HUE_MAX: &str = "huemax";
const HUE_STEPS: &str = "huesteps";
const SATURATION_MIN: &str = "saturationmin";
const SATURATION_MAX: &str = "saturationmax";
const VALUE_MIN: &str = "valuemin";
const VALUE_MAX: &str = "valuemax";
const COLORS: &str = "colors";
const HUE_NOISE: &str = "huenoise";
const SATURATION_NOISE: &str = "saturationnoise";
const VALUE_NOISE: &str = "valuenoise";
const HUE_SEAM: f32 = 1.0 / 360.0;
const HSV_SECTOR_COUNT: f32 = 6.0;
const HSV_PAIR_SPAN: f32 = 2.0;
const HSV_BLUE_SECTOR: f32 = 4.0;
const HSV_GRAY_EPSILON: f32 = 1e-5;
const HSV_DEFAULT_STEPS: f32 = 6.0;
const HSV_RANDOM_BITS: u32 = 15;
const HSV_RANDOM_MAX: u32 = (1 << HSV_RANDOM_BITS) - 1;
const COLOR_CHANNELS: usize = 3;
const COLOR_RANDOM: &str = "colorrandom";
const COLOR_CHANGE: &str = "colorchange";
const INITIALIZER_FIELD: &str = "initializer";
const OPERATOR_FIELD: &str = "operator";
const MIN_FIELD: &str = "min";
const MAX_FIELD: &str = "max";
const START_VALUE_FIELD: &str = "startvalue";
const INSTANCE_OVERRIDE_FIELD: &str = "instanceoverride";
const COLORN_FIELD: &str = "colorn";
const LEGACY_COLOR_FIELD: &str = "color";
const DEFINITION_FLAGS_FIELD: &str = "flags";
const COLOR_VERSION: u32 = 5;
const COLOR_OVERRIDE_TOLERANCE: f32 = 0.003_529_411_7;
const COLOR_RECOLOR_DISABLED: i64 = 8;
const WHITE_HSV: [f32; 3] = [0.0, 0.0, 1.0];
const RED_HSV: [f32; 3] = [0.0, 1.0, 1.0];
const HSV_COMPONENTS: [[usize; 3]; 6] =
    [[1, 2, 0], [2, 1, 0], [0, 1, 2], [0, 2, 1], [2, 0, 1], [1, 0, 2]];

#[derive(Clone, Copy)]
pub struct ParticleDefaults {
    offset: PositionOffset,
    vortex: vortex::Defaults,
    boids: boids::Defaults,
    maintain_distance: f32,
    reduce_distance: [f32; 2],
    cap_speed: f32,
    plane_distance: f32,
    sphere_origin: [f32; 3],
    sphere_radius: f32,
}

impl ParticleDefaults {
    pub const ORTHOGRAPHIC: Self = Self {
        offset: PositionOffset::ORTHOGRAPHIC,
        vortex: vortex::Defaults::ORTHOGRAPHIC,
        boids: boids::Defaults::ORTHOGRAPHIC,
        maintain_distance: 200.0,
        reduce_distance: [100.0, 350.0],
        cap_speed: 100.0,
        plane_distance: -150.0,
        sphere_origin: [0.0, -200.0, 0.0],
        sphere_radius: 50.0,
    };
    pub const OTHER: Self = Self {
        offset: PositionOffset::OTHER,
        vortex: vortex::Defaults::OTHER,
        boids: boids::Defaults::OTHER,
        maintain_distance: 1.0,
        reduce_distance: [0.5, 1.0],
        cap_speed: 1.0,
        plane_distance: 0.0,
        sphere_origin: [0.0; 3],
        sphere_radius: 1.0,
    };

    #[must_use]
    pub fn of(scene: &Value) -> Self {
        Self::for_projection(ProjectionMode::of(scene))
    }

    pub fn for_projection(projection: ProjectionMode) -> Self {
        match projection {
            ProjectionMode::Orthographic | ProjectionMode::Automatic => Self::ORTHOGRAPHIC,
            ProjectionMode::Other => Self::OTHER,
        }
    }
}

#[derive(Clone, Copy)]
pub struct BoundsViewport {
    width: i32,
    height: i32,
}

impl BoundsViewport {
    pub fn of(scene: &Value) -> Option<Self> {
        let projection =
            scene.get(crate::model::GENERAL_FIELD)?.get(crate::model::PROJECTION_FIELD)?;
        if projection.get(crate::model::PROJECTION_AUTO).and_then(Value::as_bool) == Some(true) {
            return None;
        }
        let dimension = |key| {
            let value = projection.get(key)?;
            let number = value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
                .or_else(|| {
                    value
                        .as_f64()
                        .filter(|number| number.fract() == 0.0)
                        .map(|number| number as i64)
                })?;
            i32::try_from(number)
                .ok()
                .filter(|value| *value > 0 && (*value as f32) < i32::MAX as f32)
        };
        Some(Self {
            width: dimension(crate::model::PROJECTION_WIDTH)?,
            height: dimension(crate::model::PROJECTION_HEIGHT)?,
        })
    }
}

pub struct LoadContext<'a> {
    pub pkg: &'a Package,
    pub assets: &'a crate::effects::Assets,
    pub defaults: ParticleDefaults,
    pub viewport: Option<BoundsViewport>,
    pub bounds_static: bool,
    pub version: u32,
}

#[derive(Clone, Copy)]
pub struct Rng(u32);

impl Rng {
    #[must_use]
    pub fn new(seed: u32) -> Self {
        Self(seed | 1)
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / f32::from(1u16 << 8) / 65536.0
    }

    fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.unit()
    }

    fn range3(&mut self, min: [f32; 3], max: [f32; 3]) -> [f32; 3] {
        [self.range(min[0], max[0]), self.range(min[1], max[1]), self.range(min[2], max[2])]
    }

    fn biased(&mut self, exponent: f32) -> f32 {
        let unit = self.unit();
        if (exponent - 1.0).abs() < 1e-6 { unit } else { unit.powf(exponent.max(1e-4)) }
    }

    fn range_biased(&mut self, min: f32, max: f32, exponent: f32) -> f32 {
        min + (max - min) * self.biased(exponent)
    }

    fn range3_biased(&mut self, min: [f32; 3], max: [f32; 3], exponent: f32) -> [f32; 3] {
        [
            self.range_biased(min[0], max[0], exponent),
            self.range_biased(min[1], max[1], exponent),
            self.range_biased(min[2], max[2], exponent),
        ]
    }

    pub(crate) fn index(&mut self, max: u32) -> usize {
        let count = max + 1;
        let threshold = count.wrapping_neg() % count;
        loop {
            let product = u64::from(self.next_u32()) * u64::from(count);
            if (product as u32) >= threshold {
                return (product >> u32::BITS) as usize;
            }
        }
    }

    fn hue_index(&mut self, steps: u32) -> u32 {
        let sample = self.next_u32() >> (u32::BITS - HSV_RANDOM_BITS);
        hue_index(sample, steps)
    }
}

fn hue_index(sample: u32, steps: u32) -> u32 {
    (((sample as f32 / HSV_RANDOM_MAX as f32) * (steps + 1) as f32).trunc() as u32).min(steps)
}

fn vec3_of(value: Option<&Value>, fallback: [f32; 3]) -> [f32; 3] {
    let Some(text) = value.and_then(Value::as_str) else {
        if let Some(num) = value.and_then(Value::as_f64) {
            let single = num as f32;
            return [single, single, single];
        }
        return fallback;
    };
    let mut parts = text.split_whitespace().filter_map(|part| part.parse::<f32>().ok());
    [
        parts.next().unwrap_or(fallback[0]),
        parts.next().unwrap_or(fallback[1]),
        parts.next().unwrap_or(fallback[2]),
    ]
}

fn rgb_to_hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let value = r.max(g).max(b);
    let delta = value - r.min(g).min(b);
    if delta < HSV_GRAY_EPSILON || value <= 0.0 {
        return [0.0, 0.0, value];
    }
    let sector = if value == r {
        (g - b) / delta
    } else if value == g {
        HSV_PAIR_SPAN + (b - r) / delta
    } else {
        HSV_BLUE_SECTOR + (r - g) / delta
    };
    let hue = sector / HSV_SECTOR_COUNT;
    [if hue < 0.0 { hue + 1.0 } else { hue }, delta / value, value]
}

fn hsv_to_rgb([hue, saturation, value]: [f32; 3]) -> [f32; 3] {
    let chroma = value * saturation;
    let sector = (f64::from(hue * HSV_SECTOR_COUNT) % f64::from(HSV_SECTOR_COUNT)) as f32;
    let secondary = chroma * (1.0 - ((sector % HSV_PAIR_SPAN) - 1.0).abs());
    let offset = value - chroma;
    if !(0.0..HSV_SECTOR_COUNT).contains(&sector) {
        return [offset; COLOR_CHANNELS];
    }
    let components = [0.0, chroma, secondary];
    HSV_COMPONENTS[sector as usize].map(|index| components[index] + offset)
}

fn rgb_color(value: &Value) -> Option<[f32; 3]> {
    let mut parts = value.as_str()?.split_whitespace();
    let color =
        [parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?];
    parts.next().is_none().then_some(color)
}

fn rounded_color(color: [f32; 3]) -> [f32; 3] {
    color.map(|channel| format!("{channel:.5}").parse().unwrap_or(channel))
}

fn authored_color(doc: &Value) -> Option<[f32; 3]> {
    let mut color = None;
    for entry in doc.get(INITIALIZER_FIELD).and_then(Value::as_array).into_iter().flatten() {
        let name = entry.get(STEP_NAME).and_then(Value::as_str).unwrap_or_default();
        let candidate = if name.eq_ignore_ascii_case(COLOR_RANDOM) {
            let min = vec3_of(entry.get(MIN_FIELD), [f32::from(u8::MAX); COLOR_CHANNELS]);
            let max = vec3_of(entry.get(MAX_FIELD), [f32::from(u8::MAX); COLOR_CHANNELS]);
            Some(std::array::from_fn(|axis| (min[axis] + max[axis]) / (2.0 * f32::from(u8::MAX))))
        } else if name.eq_ignore_ascii_case(HSV_COLOR_RANDOM) {
            let midpoint = |min, max, default_min, default_max| {
                ((num_of(entry.get(min), default_min) + num_of(entry.get(max), default_max)) * 0.5)
                    .clamp(0.0, 1.0)
            };
            Some(hsv_to_rgb([
                midpoint(HUE_MIN, HUE_MAX, 0.0, 1.0),
                midpoint(SATURATION_MIN, SATURATION_MAX, 0.5, 1.0),
                midpoint(VALUE_MIN, VALUE_MAX, 0.5, 1.0),
            ]))
        } else if name.eq_ignore_ascii_case(COLOR_LIST) {
            entry
                .get(COLORS)
                .and_then(Value::as_array)
                .and_then(|colors| colors.iter().find(|color| color.is_string()))
                .and_then(rgb_color)
        } else {
            None
        };
        if let Some(candidate) = candidate {
            color = Some(candidate);
        }
    }
    let color = color.or_else(|| {
        doc.get(OPERATOR_FIELD)
            .and_then(Value::as_array)?
            .iter()
            .find(|entry| {
                entry
                    .get(STEP_NAME)
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.eq_ignore_ascii_case(COLOR_CHANGE))
            })
            .map(|entry| vec3_of(entry.get(START_VALUE_FIELD), [1.0; COLOR_CHANNELS]))
    })?;
    Some(rounded_color(color))
}

fn recolor_endpoint(color: [f32; 3], original: [f32; 3], replacement: [f32; 3]) -> [f32; 3] {
    let [hue, saturation, value] = rgb_to_hsv(color);
    hsv_to_rgb([
        (hue + replacement[0] - original[0]).rem_euclid(1.0),
        (saturation + replacement[1] - original[1]).clamp(0.0, 1.0),
        (value + replacement[2] - original[2]).clamp(0.0, 1.0),
    ])
}

fn num_bound(value: Option<&Value>, props: &BTreeMap<String, Vec<f32>>, fallback: f32) -> f32 {
    value
        .and_then(|raw| crate::effects::bound_value(raw, props))
        .and_then(|parts| parts.first().copied())
        .unwrap_or(fallback)
}

fn num_of(value: Option<&Value>, fallback: f32) -> f32 {
    match value {
        Some(Value::Number(num)) => num.as_f64().unwrap_or(f64::from(fallback)) as f32,
        Some(Value::String(text)) => text.parse().unwrap_or(fallback),
        _ => fallback,
    }
}

#[derive(Clone, Copy, Default)]
pub struct ControlPoint {
    pub flags: u32,
    pub offset: [f32; 3],
}

fn control_points(doc: &Value) -> [ControlPoint; CONTROL_POINT_COUNT] {
    let mut points = [ControlPoint::default(); CONTROL_POINT_COUNT];
    for (point, entry) in points
        .iter_mut()
        .zip(doc.get("controlpoint").and_then(Value::as_array).into_iter().flatten())
    {
        point.flags = num_of(entry.get("flags"), 0.0) as u32;
        point.offset = vec3_of(entry.get("offset"), [0.0; 3]);
    }
    points
}

const EMITTER_ONE_PER_FRAME_FLAG: u32 = 1 << 1;
const PERIODIC_FALLBACK_SECONDS: f32 = 0.1;
const PERIODIC_DELAY_MIN: &str = "minperiodicdelay";
const PERIODIC_DELAY_MAX: &str = "maxperiodicdelay";
const PERIODIC_DURATION_MIN: &str = "minperiodicduration";
const PERIODIC_DURATION_MAX: &str = "maxperiodicduration";
const PERIODIC_MAX_EMIT: &str = "maxtoemitperperiod";
const BOX_POSITION: &str = "box";
const SPHERE_POSITION: &str = "sphere";
const CONTROL_POINT_FORCE: &str = "controlpointforce";
const CONTROL_POINT_FIELD: &str = "controlpoint";
const FORCE_OFFSET_FIELD: &str = "offset";
const FORCE_SCALE_FIELD: &str = "scale";
const FORCE_THRESHOLD_FIELD: &str = "threshold";
const FORCE_DEFAULT: f32 = 512.0;
const EMITTER_DELAY_KEY: &str = "delay";
const EMITTER_DURATION_KEY: &str = "duration";
const MAP_SEQUENCE_BOUNDS: &str = "bounds";
const MAP_SEQUENCE_LIMIT: &str = "limitbehavior";
const MAP_SEQUENCE_MIRROR: &str = "mirror";
const MAP_SEQUENCE_MIN_COUNT: f32 = 0.0001;
const MAP_SEQUENCE_DEFAULT_COUNT: f32 = 32.0;

#[derive(Clone, Copy, Default)]
pub struct EmitterTiming {
    pub delay: f32,
    pub duration: f32,
    pub one_per_frame: bool,
    pub periodic: Option<Periodic>,
}

#[derive(Clone, Copy)]
pub struct Periodic {
    delay: [f32; 2],
    duration: [f32; 2],
    max_per_period: usize,
}

pub enum Emitter {
    Image {
        image: image::ImageEmitter,
        rate: f32,
        instantaneous: u32,
        timing: EmitterTiming,
        audio: Option<AudioResponse>,
    },
    Sphere {
        control_point: Option<usize>,
        origin: [f32; 3],
        shape: SphereShape,
        rate: f32,
        instantaneous: u32,
        timing: EmitterTiming,
        audio: Option<AudioResponse>,
    },
    Box {
        control_point: Option<usize>,
        origin: [f32; 3],
        extent: [f32; 3],
        rate: f32,
        instantaneous: u32,
        timing: EmitterTiming,
        audio: Option<AudioResponse>,
    },
}

impl Emitter {
    fn control_point(&self) -> Option<usize> {
        match self {
            Self::Image { .. } => None,
            Self::Sphere { control_point, .. } | Self::Box { control_point, .. } => *control_point,
        }
    }

    fn rate(&self) -> f32 {
        match self {
            Self::Sphere { rate, .. } | Self::Box { rate, .. } | Self::Image { rate, .. } => *rate,
        }
    }

    fn audio(&self) -> Option<AudioResponse> {
        match self {
            Self::Sphere { audio, .. } | Self::Box { audio, .. } | Self::Image { audio, .. } => {
                *audio
            }
        }
    }

    fn active_rate(&self, left: &[f32], right: &[f32]) -> f32 {
        self.rate() * self.audio().map_or(1.0, |response| response.amount(left, right))
    }

    fn origin(&self) -> [f32; 3] {
        match self {
            Self::Image { .. } => [0.0; SPATIAL_AXES],
            Self::Sphere { origin, .. } | Self::Box { origin, .. } => *origin,
        }
    }

    fn instantaneous(&self) -> u32 {
        match self {
            Self::Sphere { instantaneous, .. }
            | Self::Box { instantaneous, .. }
            | Self::Image { instantaneous, .. } => *instantaneous,
        }
    }

    fn timing(&self) -> EmitterTiming {
        match self {
            Self::Sphere { timing, .. } | Self::Box { timing, .. } | Self::Image { timing, .. } => {
                *timing
            }
        }
    }

    fn spawn(&self, rng: &mut Rng) -> Option<[f32; 3]> {
        Some(match self {
            Self::Image { .. } => return None,
            Self::Sphere { origin, shape, .. } => {
                let offset = shape.offset(rng);
                std::array::from_fn(|axis| origin[axis] + offset[axis])
            }
            Self::Box { origin, extent, .. } => [
                origin[0] + rng.range(-extent[0], extent[0]),
                origin[1] + rng.range(-extent[1], extent[1]),
                origin[2] + rng.range(-extent[2], extent[2]),
            ],
        })
    }
}

pub enum Initializer {
    Lifetime {
        min: f32,
        max: f32,
        exponent: f32,
    },
    Size {
        min: f32,
        max: f32,
        exponent: f32,
    },
    Color {
        min: [f32; 3],
        max: [f32; 3],
        exponent: f32,
    },
    InheritVelocity(InheritVelocity),
    Remap(Remap),
    HsvColor(HsvColor),
    ColorList(ColorList),
    Velocity {
        min: [f32; 3],
        max: [f32; 3],
        exponent: f32,
    },
    BoxPosition {
        min: [f32; 3],
        max: [f32; 3],
    },
    SpherePosition {
        min: f32,
        max: f32,
    },
    Rotation {
        min: [f32; 3],
        max: [f32; 3],
    },
    Alpha {
        min: f32,
        max: f32,
        exponent: f32,
    },
    AngularVelocity {
        min: [f32; 3],
        max: [f32; 3],
    },
    TurbulentVelocity(Turbulent),
    BetweenSequence(BetweenSequence),
    PositionOffset(PositionOffset),
    InheritEvent(EventInheritance),
    MapSequence {
        control_point: usize,
        count: f32,
        bounds: [f32; 2],
        mirror: bool,
        axis: [f32; 3],
        min: [f32; 3],
        max: [f32; 3],
    },
}

pub struct HsvColor {
    hue_min: f32,
    hue_step: f32,
    hue_steps: u32,
    saturation: (f32, f32),
    value: (f32, f32),
}

pub struct ColorList {
    colors: Vec<[f32; 3]>,
    noise: [f32; 3],
}

#[derive(Clone, Copy)]
pub struct Turbulent {
    pub speed: (f32, f32),
    pub scale: f32,
    pub offset: f32,
    pub forward: [f32; 3],
    pub right: [f32; 3],
    pub time_scale: f32,
    pub phase: (f32, f32),
}

#[derive(Clone, Copy)]
pub struct Vortex {
    pub control_point: usize,
    pub infinite_axis: bool,
    pub axis: [f32; 3],
    pub offset: [f32; 3],
    pub distance: (f32, f32),
    pub speed: (f32, f32),
    pub audio: Option<AudioResponse>,
}

pub struct Oscillator {
    pub frequency: (f32, f32),
    pub scale: (f32, f32),
    pub phase: (f32, f32),
    pub mask: [f32; 3],
    pub blend: Option<OperatorBlend>,
}

pub enum Operator {
    Remap(Remap),
    InheritEvent {
        inherit: EventInheritance,
        blend: Option<OperatorBlend>,
    },
    Boids(boids::Boids),
    VortexV2(vortex::Vortex),
    MaintainDistance(MaintainDistance),
    MaintainBetween(MaintainBetween),
    ReduceMovement(ReduceMovement),
    Movement {
        gravity: [f32; 3],
        drag: f32,
    },
    CapVelocity {
        max_speed: f32,
        blend: Option<OperatorBlend>,
    },
    CollisionPlane {
        normal: [f32; 3],
        distance: f32,
        collision: Collision,
    },
    CollisionSphere {
        origin: [f32; 3],
        point: Option<usize>,
        radius: f32,
        collision: Collision,
    },
    CollisionBounds {
        planes: [BoundsPlane; BOUNDS_PLANE_COUNT],
        collision: Collision,
    },
    CollisionQuad(collision::Quad),
    CollisionBox,
    CollisionModel {
        ordinal: usize,
        collision: Collision,
    },
    AlphaFade {
        fade_in: f32,
        fade_out: f32,
    },
    SizeChange {
        start: f32,
        end: f32,
        start_time: f32,
        end_time: f32,
    },
    AlphaChange {
        start: f32,
        end: f32,
        start_time: f32,
        end_time: f32,
    },
    ColorChange {
        start: [f32; 3],
        end: [f32; 3],
        start_time: f32,
        end_time: f32,
    },
    OscillateAlpha(Oscillator),
    OscillatePosition(Oscillator),
    OscillateSize(Oscillator),
    AngularMovement {
        force: [f32; 3],
        drag: f32,
        blend: Option<OperatorBlend>,
    },
    Turbulence {
        scale: f32,
        speed: (f32, f32),
        phase_range: f32,
        time_scale: f32,
        mask: [f32; 3],
        blend: Option<OperatorBlend>,
        audio: Option<AudioResponse>,
    },
    ControlPointAttract {
        control_point: usize,
        scale: f32,
        threshold: f32,
        flags: u32,
        delete_threshold: f32,
        blend: Option<OperatorBlend>,
    },
    ControlPointForce {
        control_point: usize,
        offset: [f32; 3],
        scale: f32,
        threshold: f32,
        blend: Option<OperatorBlend>,
    },
    Vortex(Vortex),
}

impl Operator {
    fn audio(&self) -> Option<AudioResponse> {
        match self {
            Self::VortexV2(vortex) => vortex.audio,
            Self::Vortex(vortex) => vortex.audio,
            Self::Turbulence { audio, .. } => *audio,
            _ => None,
        }
    }

    fn gain(&self, left: &[f32], right: &[f32]) -> f32 {
        self.audio().map_or(1.0, |response| response.amount(left, right))
    }
}

#[derive(Clone, Copy)]
pub struct BoundsPlane {
    normal: [f32; 3],
    distance: f32,
}

#[derive(Clone, Copy, PartialEq)]
struct ParticleTransform {
    origin: [f32; 3],
    angle: f32,
    scale: [f32; 3],
}

impl ParticleTransform {
    fn for_system(system: &ParticleSystem) -> Self {
        Self {
            origin: [system.origin.0, system.origin.1, system.origin.2],
            angle: system.angle,
            scale: system.draw_scale3(),
        }
    }

    fn of(object: &Value) -> Option<Self> {
        let read = |key: &str, fallback: [f32; BOUNDS_VECTOR_COMPONENTS]| {
            let value = object.get(key);
            vec3_of(
                value,
                if value.is_some() { [f32::NAN; BOUNDS_VECTOR_COMPONENTS] } else { fallback },
            )
        };
        let origin = read(COLLISION_ORIGIN, [0.0; BOUNDS_VECTOR_COMPONENTS]);
        let angles = read(BOUNDS_ANGLES, [0.0; BOUNDS_VECTOR_COMPONENTS]);
        let scale = read(BOUNDS_SCALE, [1.0; BOUNDS_VECTOR_COMPONENTS]);
        let determinant = scale.iter().product::<f32>();
        if origin.iter().chain(angles.iter()).chain(scale.iter()).any(|value| !value.is_finite())
            || angles[0] != 0.0
            || angles[1] != 0.0
            || scale.iter().any(|value| *value <= f32::EPSILON)
            || !determinant.is_finite()
            || determinant <= 0.0
        {
            return None;
        }
        Some(Self { origin, angle: angles[2], scale })
    }

    fn is_identity(self) -> bool {
        self.origin == [0.0; BOUNDS_VECTOR_COMPONENTS]
            && self.angle == 0.0
            && self.scale == [1.0; BOUNDS_VECTOR_COMPONENTS]
    }

    fn matches(self, system: &ParticleSystem) -> bool {
        self.origin == [system.origin.0, system.origin.1, system.origin.2]
            && self.angle == system.angle
            && self.scale == system.scale3
    }

    fn inverse_vector(self, vector: [f32; 3]) -> [f32; 3] {
        let (sin, cos) = self.angle.sin_cos();
        [
            (cos * vector[0] + sin * vector[1]) / self.scale[0],
            (-sin * vector[0] + cos * vector[1]) / self.scale[1],
            vector[2] / self.scale[2],
        ]
    }

    fn inverse_point(self, point: [f32; 3]) -> [f32; 3] {
        self.inverse_vector(std::array::from_fn(|axis| point[axis] - self.origin[axis]))
    }

    fn planes(self, viewport: BoundsViewport, world: bool) -> [BoundsPlane; BOUNDS_PLANE_COUNT] {
        let width = viewport.width as f32;
        let height = viewport.height as f32;
        let geometry = [
            ([1.0, 0.0, 0.0], [0.0; BOUNDS_VECTOR_COMPONENTS]),
            ([0.0, 1.0, 0.0], [0.0; BOUNDS_VECTOR_COMPONENTS]),
            ([-1.0, 0.0, 0.0], [width, height, 0.0]),
            ([0.0, -1.0, 0.0], [width, height, 0.0]),
        ];
        geometry.map(|(normal, point)| {
            let normal = if world { normal } else { self.inverse_vector(normal) };
            let point = if world { point } else { self.inverse_point(point) };
            BoundsPlane { normal, distance: dot3(normal, point) }
        })
    }
}

#[derive(Clone, Copy)]
pub struct Collision {
    response: CollisionResponse,
    stop_angular: bool,
}

#[derive(Clone, Copy)]
enum CollisionResponse {
    Bounce(f32),
    Slide,
    Stop,
    Delete,
}

pub struct OperatorBlend {
    in_start: f32,
    in_rate: f32,
    out_end: f32,
    out_rate: f32,
}

impl OperatorBlend {
    fn parse(entry: &Value) -> Option<Self> {
        let mut in_start = num_of(entry.get(BLEND_IN_START), 0.0);
        let in_end = num_of(entry.get(BLEND_IN_END), 0.0);
        let out_start = num_of(entry.get(BLEND_OUT_START), 1.0);
        let mut out_end = num_of(entry.get(BLEND_OUT_END), 1.0);
        if in_end - BLEND_MIN_SPAN < in_start {
            in_start = in_end - BLEND_MIN_SPAN;
        }
        if out_end <= out_start + BLEND_MIN_SPAN {
            out_end = out_start + BLEND_MIN_SPAN;
        }
        let in_span = in_end - in_start;
        let out_span = out_end - out_start;
        let active = (in_end > BLEND_ACTIVE_WINDOW || out_start < 1.0 - BLEND_ACTIVE_WINDOW)
            && (out_start - in_end > BLEND_ACTIVE_WINDOW
                || in_span > BLEND_ACTIVE_WINDOW
                || out_span > BLEND_ACTIVE_WINDOW);
        active.then(|| Self {
            in_start,
            in_rate: reciprocal(in_span),
            out_end,
            out_rate: reciprocal(out_span),
        })
    }

    fn weight(&self, life: f32) -> f32 {
        let incoming = simd_saturate((life - self.in_start) * self.in_rate);
        let outgoing = simd_saturate((self.out_end - life) * self.out_rate);
        incoming * outgoing
    }
}

fn drag_scale(frame_dt: f32) -> f32 {
    (DRAG_REFERENCE_DT / frame_dt.max(f32::EPSILON)).min(1.0).powf(DRAG_EXPONENT)
}

fn reciprocal(value: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use std::arch::x86_64::{_mm_cvtss_f32, _mm_rcp_ss, _mm_set_ss};
        _mm_cvtss_f32(_mm_rcp_ss(_mm_set_ss(value)))
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        value.recip()
    }
}

fn reciprocal_sqrt(value: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use std::arch::x86_64::{_mm_cvtss_f32, _mm_rsqrt_ss, _mm_set_ss};
        _mm_cvtss_f32(_mm_rsqrt_ss(_mm_set_ss(value)))
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        value.sqrt().recip()
    }
}

fn simd_min(first: f32, second: f32) -> f32 {
    if first < second { first } else { second }
}

fn simd_saturate(value: f32) -> f32 {
    let upper = simd_min(value, 1.0);
    if 0.0 > upper { 0.0 } else { upper }
}

fn oscillator(entry: &Value, frequency_max: f32, scale: (f32, f32)) -> Oscillator {
    let frequency_min = num_of(entry.get("frequencymin"), 1.0);
    let scale_min = num_of(entry.get("scalemin"), scale.0);
    let phase_min = num_of(entry.get("phasemin"), 0.0);
    Oscillator {
        frequency: (frequency_min, num_of(entry.get("frequencymax"), frequency_max)),
        scale: (scale_min, num_of(entry.get("scalemax"), scale.1)),
        phase: (phase_min, num_of(entry.get("phasemax"), std::f32::consts::TAU)),
        mask: vec3_of(entry.get("mask"), [1.0, 1.0, 0.0]),
        blend: OperatorBlend::parse(entry),
    }
}

impl Oscillator {
    fn weight(&self, particle: &Particle) -> f32 {
        self.blend
            .as_ref()
            .map_or(1.0, |blend| blend.weight(particle.age * reciprocal(particle.lifetime)))
    }

    fn blended(&self, particle: &Particle, modulation: f32) -> f32 {
        if self.blend.is_none() {
            return modulation;
        }
        (modulation - 1.0) * self.weight(particle) + 1.0
    }

    fn params(&self, seed: f32) -> (f32, f32, f32) {
        (
            self.frequency.0 + (self.frequency.1 - self.frequency.0) * seed,
            self.phase.0 + (self.phase.1 - self.phase.0) * seed,
            self.scale.0 + (self.scale.1 - self.scale.0) * seed,
        )
    }

    fn modulation(&self, age: f32, seed: f32) -> f32 {
        let (frequency, phase, _) = self.params(seed);
        self.scale.0
            + (self.scale.1 - self.scale.0)
                * seed
                * ((age + phase) * frequency).sin().mul_add(0.5, 0.5)
    }

    fn displacement(&self, age: f32, dt: f32, seed: f32) -> [f32; 3] {
        let (frequency, phase, amplitude) = self.params(seed);
        let delta = |offset: f32| {
            (((age + phase + offset) * frequency).sin()
                - ((age + phase + offset - dt) * frequency).sin())
                * amplitude
        };
        [delta(0.0), delta(seed * std::f32::consts::TAU), delta(0.0)]
    }
}

fn simplex([x, y, z]: [f32; 3]) -> f32 {
    let skew = (x + y + z) / 3.0;
    let cells = [floor(x + skew), floor(y + skew), floor(z + skew)];
    let unskew = (cells[0] + cells[1] + cells[2]) / 6.0;
    let p = [x - cells[0] + unskew, y - cells[1] + unskew, z - cells[2] + unskew];
    let first = [
        usize::from(p[0] >= p[1] && p[0] >= p[2]),
        usize::from(p[1] > p[0] && p[1] > p[2]),
        usize::from(p[2] > p[0] && p[2] > p[1]),
    ];
    let second = [
        usize::from(p[0] >= p[1] || p[0] >= p[2]),
        usize::from(p[1] > p[0] || p[1] >= p[2]),
        usize::from(p[2] > p[0] || p[2] > p[1]),
    ];
    let cell = cells.map(|value| (value as i64 & 255) as usize);
    let gradients = [
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
        [1.0, -1.0, 0.0],
        [-1.0, -1.0, 0.0],
        [1.0, 0.0, 1.0],
        [-1.0, 0.0, 1.0],
        [1.0, 0.0, -1.0],
        [-1.0, 0.0, -1.0],
        [0.0, 1.0, 1.0],
        [0.0, -1.0, 1.0],
        [0.0, 1.0, -1.0],
        [0.0, -1.0, -1.0],
    ];
    let mut sum = 0.0;
    for (corner, offset) in [[0; 3], first, second, [1; 3]].into_iter().enumerate() {
        let delta = std::array::from_fn::<_, 3, _>(|axis| {
            p[axis] - offset[axis] as f32 + corner as f32 / 6.0
        });
        let weight = (0.6 - dot3(delta, delta)).max(0.0);
        let hash = perm(
            cell[0]
                + offset[0]
                + perm(
                    cell[1] + offset[1] + perm(cell[2] + if corner == 3 { 0 } else { offset[2] }),
                ),
        ) % 12;
        sum += dot3(gradients[hash], delta) * weight.powi(4);
    }
    sum * 32.0
}

const PERLIN_PERM: [u8; 256] = [
    151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69,
    142, 8, 99, 37, 240, 21, 10, 23, 190, 6, 148, 247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219,
    203, 117, 35, 11, 32, 57, 177, 33, 88, 237, 149, 56, 87, 174, 20, 125, 136, 171, 168, 68, 175,
    74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122, 60, 211, 133, 230,
    220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54, 65, 25, 63, 161, 1, 216, 80, 73, 209, 76,
    132, 187, 208, 89, 18, 169, 200, 196, 135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173,
    186, 3, 64, 52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212, 207, 206,
    59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170, 213, 119, 248, 152, 2, 44, 154, 163,
    70, 221, 153, 101, 155, 167, 43, 172, 9, 129, 22, 39, 253, 19, 98, 108, 110, 79, 113, 224, 232,
    178, 185, 112, 104, 218, 246, 97, 228, 251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162,
    241, 81, 51, 145, 235, 249, 14, 239, 107, 49, 192, 214, 31, 181, 199, 106, 157, 184, 84, 204,
    176, 115, 121, 50, 45, 127, 4, 150, 254, 138, 236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141,
    128, 195, 78, 66, 215, 61, 156, 180,
];

fn perm(index: usize) -> usize {
    usize::from(PERLIN_PERM[index & 255])
}

const INTEGRAL_FLOATS: f32 = (1_u32 << (f32::MANTISSA_DIGITS - 1)) as f32;

fn floor(value: f32) -> f32 {
    if value == 0.0 || value.abs() >= INTEGRAL_FLOATS || value.is_nan() {
        return value;
    }
    let truncated = value as i32 as f32;
    if truncated > value { truncated - 1.0 } else { truncated }
}

fn simplex_one(value: f32) -> f32 {
    let cell = floor(value);
    let fraction = value - cell;
    let gradient = |offset: usize, distance: f32| {
        let hash = perm((cell as i64 & 255) as usize + offset);
        let magnitude = (hash & 7) as f32 + 1.0;
        let gradient = if hash & 8 == 0 { magnitude } else { -magnitude };
        let weight = 1.0 - distance * distance;
        gradient * distance * weight.powi(4)
    };
    (gradient(0, fraction) + gradient(1, fraction - 1.0)) * 0.395
}

fn normalize3(v: [f32; 3]) -> Option<[f32; 3]> {
    let len = dot3(v, v).sqrt();
    (len > 1e-4).then(|| [v[0] / len, v[1] / len, v[2] / len])
}

fn map_basis(axis: [f32; 3]) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let length = dot3(axis, axis).sqrt();
    let axis = if axis.iter().all(|value| *value == 0.0) {
        [0.0, 0.0, 1.0]
    } else {
        axis.map(|value| value / length)
    };
    if axis[0] == 0.0 && axis[1] == 0.0 {
        return (axis, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    }
    let tangent = cross3(axis, [0.0, 0.0, 1.0]);
    let tangent_length = dot3(tangent, tangent).sqrt();
    let tangent = tangent.map(|value| value / tangent_length);
    let normal = cross3(axis, tangent);
    let normal_length = dot3(normal, normal).sqrt();
    let normal = normal.map(|value| value / normal_length);
    (axis, tangent, normal)
}

fn rotate_axis(v: [f32; 3], axis: [f32; 3], angle: f32) -> [f32; 3] {
    let (sin, cos) = angle.sin_cos();
    let k = cross3(axis, v);
    let along = dot3(axis, v) * (1.0 - cos);
    [
        v[0] * cos + k[0] * sin + axis[0] * along,
        v[1] * cos + k[1] * sin + axis[1] * along,
        v[2] * cos + k[2] * sin + axis[2] * along,
    ]
}

fn turbulent_direction(sample: f32, init: &Turbulent) -> [f32; 3] {
    let right = normalize3(init.right).unwrap_or([0.0, 0.0, 1.0]);
    let angle = simplex_one(sample) * std::f32::consts::PI * init.scale + init.offset;
    rotate_axis(init.forward, right, angle)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Renderer {
    Sprite,
    Trail,
    Rope,
    Ribbon,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Screen,
    Upright,
    Fixed,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Trail {
    pub length: f32,
    pub min_length: f32,
    pub max_length: f32,
    pub segments: usize,
    pub fade_alpha: bool,
    pub fade_size: bool,
    pub uv_scroll: bool,
    pub uv_scale: f32,
    pub subdivision: u32,
}

impl Trail {
    #[must_use]
    pub fn uv_factor(self) -> f32 {
        if self.uv_scale == 0.0 { 1.0 } else { self.uv_scale.recip() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrailPoint {
    pub pos: [f32; 3],
    time: f32,
}

impl TrailPoint {
    fn from_particle(particle: &Particle, time: f32) -> Self {
        Self { pos: particle.pos, time }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrailHistory {
    samples: std::collections::VecDeque<TrailPoint>,
    current: TrailPoint,
    initial: TrailPoint,
    capacity: usize,
    phase: f32,
    sample_count: u16,
}

impl TrailHistory {
    fn new(particle: &Particle, time: f32, capacity: usize) -> Self {
        let point = TrailPoint::from_particle(particle, time);
        let mut samples = std::collections::VecDeque::with_capacity(capacity);
        samples.push_back(point);
        Self { samples, current: point, initial: point, capacity, phase: 0.0, sample_count: 0 }
    }

    #[cfg(test)]
    fn record(&mut self, particle: &Particle, time: f32, length: f32) {
        let interval = length / (self.capacity - 1) as f32;
        let elapsed = self.phase * interval + time - self.current.time;
        let sample = self.samples.is_empty() || elapsed >= interval && time > self.current.time;
        self.record_frame(particle, time, if sample { 0.0 } else { elapsed / interval }, sample);
    }

    fn record_frame(&mut self, particle: &Particle, time: f32, phase: f32, sample: bool) {
        if self.samples.is_empty() && !sample {
            self.samples.push_front(self.current);
        }
        if sample {
            self.sample_count = self.sample_count.wrapping_add(1);
            self.samples.push_front(TrailPoint::from_particle(particle, time));
            self.samples.truncate(self.capacity - 1);
        }
        self.phase = phase.min(1.0);
        self.current = TrailPoint::from_particle(particle, time);
    }

    pub fn points(&self) -> impl Iterator<Item = &TrailPoint> + Clone {
        std::iter::once(&self.current).chain(self.samples.iter())
    }

    pub fn phase(&self) -> f32 {
        self.phase
    }

    fn reset(&mut self, particle: &Particle, time: f32) {
        self.current = TrailPoint::from_particle(particle, time);
        self.initial = self.current;
        self.samples.clear();
        self.samples.push_back(self.current);
        self.phase = 0.0;
        self.sample_count = 0;
    }

    fn transform(&mut self, mut point: impl FnMut([f32; 3]) -> [f32; 3]) {
        self.current.pos = point(self.current.pos);
        self.initial.pos = point(self.initial.pos);
        for sample in &mut self.samples {
            sample.pos = point(sample.pos);
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct ParticleRender {
    pub renderer: Renderer,
    pub orientation: Orientation,
    pub flags: i64,
    pub trail: Trail,
}

impl ParticleRender {
    const SPRITE: Self = Self {
        renderer: Renderer::Sprite,
        orientation: Orientation::Screen,
        flags: 0,
        trail: Trail {
            length: 0.05,
            min_length: 0.0,
            max_length: 100.0,
            segments: DEFAULT_TRAIL_SEGMENTS,
            fade_alpha: true,
            fade_size: false,
            uv_scroll: false,
            uv_scale: 1.0,
            subdivision: 3,
        },
    };

    fn parse(entry: &Value) -> Option<Self> {
        let name = entry.get(STEP_NAME).and_then(Value::as_str)?.to_ascii_lowercase();
        let renderer = match name.as_str() {
            "sprite" => Renderer::Sprite,
            "spritetrail" => Renderer::Trail,
            "rope" => Renderer::Rope,
            "ropetrail" => Renderer::Ribbon,
            _ => return None,
        };
        let orientation = match entry.get(RENDERER_ORIENTATION).and_then(Value::as_str) {
            Some(RENDERER_UPRIGHT) => Orientation::Upright,
            Some(RENDERER_FIXED) => Orientation::Fixed,
            _ => Orientation::Screen,
        };
        let max_length = if renderer == Renderer::Trail {
            DEFAULT_SPRITE_TRAIL_LIMIT
        } else {
            Self::SPRITE.trail.max_length
        };
        let ribbon = renderer == Renderer::Ribbon;
        let flag = |key, default| {
            entry.get(key).map_or(default, |value| {
                value.as_bool().unwrap_or_else(|| num_of(Some(value), 0.0) != 0.0)
            })
        };
        Some(Self {
            renderer,
            orientation,
            flags: num_of(entry.get(RENDERER_FLAGS), 0.0) as i64,
            trail: Trail {
                length: num_of(
                    entry.get(RENDERER_LENGTH),
                    if ribbon { 1.0 } else { Self::SPRITE.trail.length },
                )
                .max(if ribbon { MIN_TRAIL_LENGTH } else { 0.0 }),
                min_length: num_of(entry.get(RENDERER_MIN_LENGTH), Self::SPRITE.trail.min_length)
                    .max(0.0),
                max_length: num_of(entry.get(RENDERER_MAX_LENGTH), max_length).max(0.0),
                segments: (num_of(entry.get(RENDERER_SEGMENTS), DEFAULT_TRAIL_SEGMENTS as f32)
                    as usize)
                    .clamp(1, MAX_TRAIL_SEGMENTS),
                fade_alpha: flag(RENDERER_FADE_ALPHA, !ribbon),
                fade_size: flag(RENDERER_FADE_SIZE, false),
                uv_scroll: flag(RENDERER_UV_SCROLL, false),
                uv_scale: num_of(entry.get(RENDERER_UV_SCALE), 1.0),
                subdivision: num_of(
                    entry.get(RENDERER_SUBDIVISION),
                    if ribbon { 1.0 } else { DEFAULT_ROPE_SUBDIVISION },
                )
                .clamp(0.0, MAX_TRAIL_SEGMENTS as f32) as u32,
            },
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Animation {
    Sequence,
    RandomFrame,
    Once,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleBlend {
    Additive,
    Translucent,
    Normal,
}

pub const CONTROL_POINT_COUNT: usize = control::CONTROL_POINTS;

#[derive(Clone, Copy)]
struct InstanceFactors {
    color: [f32; 3],
    speed: f32,
    lifetime: f32,
}

impl Default for InstanceFactors {
    fn default() -> Self {
        Self { color: [1.0; 3], speed: 1.0, lifetime: 1.0 }
    }
}

pub struct ParticleSystem {
    pub model_collisions: model_collision::Bindings,
    pub image_dependencies: Vec<image::ImageDependency>,
    pub children: Vec<ParticleSystem>,
    pub has_death_children: bool,
    pub child_event: Option<ChildEvent>,
    pub renderer: Renderer,
    pub orientation: Orientation,
    pub renderer_flags: i64,
    pub trail: Trail,
    pub extra_renderers: Vec<ParticleRender>,
    pub animation: Animation,
    pub sequence_multiplier: f32,
    pub angular_motion: bool,
    pub remapped_points: u8,
    pub max_count: usize,
    pub start_time: f32,
    pub control_points: [ControlPoint; CONTROL_POINT_COUNT],
    pub emitters: Vec<Emitter>,
    pub initializers: Vec<Initializer>,
    pub operators: Vec<Operator>,
    pub texture: Option<crate::model::Texture>,
    pub additive: bool,
    pub blend: ParticleBlend,
    pub perspective: bool,
    pub pass: Option<crate::effects::EffectPass>,
    pub extra_passes: Vec<Option<crate::effects::EffectPass>>,
    pub origin: (f32, f32, f32),
    pub angle: f32,
    pub spatial: Option<crate::matrix::Mat4>,
    pub world_matrix: Option<crate::matrix::Mat4>,
    pub scale: f32,
    pub scale3: [f32; 3],
    pub base_color: [f32; 3],
    pub alpha: f32,
    pub count_scale: f32,
    pub rate_scale: f32,
    pub size_scale: f32,
    pub authored_size_scale: f32,
    pub grab_slot: Option<usize>,
    pub world: bool,
    instance: InstanceFactors,
}

impl ParticleSystem {
    pub fn set_color(&mut self, color: [f32; 3]) {
        if color.iter().all(|channel| channel.is_finite() && *channel >= 0.0) {
            self.instance.color = color;
        }
    }

    pub fn set_speed(&mut self, speed: f32) {
        if speed.is_finite() {
            self.instance.speed = speed.clamp(0.0, MAX_INSTANCE_SPEED_SCALE);
        }
    }

    pub fn set_lifetime(&mut self, lifetime: f32) {
        if lifetime.is_finite() {
            self.instance.lifetime = lifetime.clamp(0.0, MAX_INSTANCE_LIFETIME_SCALE);
        }
    }

    pub fn set_control_point(&mut self, index: usize, position: [f32; 3]) -> bool {
        let Some(point) = self.control_points.get_mut(index) else { return false };
        if !position.iter().all(|axis| axis.is_finite()) {
            return false;
        }
        point.offset = position;
        true
    }

    pub fn inherit_instance_factors(&mut self, parent: &Self) {
        self.instance = parent.instance;
    }

    #[must_use]
    pub fn primary_render(&self) -> ParticleRender {
        ParticleRender {
            renderer: self.renderer,
            orientation: self.orientation,
            flags: self.renderer_flags,
            trail: self.trail,
        }
    }

    #[must_use]
    pub fn ribbon_trail(&self) -> Trail {
        let trail = std::iter::once(self.primary_render())
            .chain(self.extra_renderers.iter().copied())
            .filter(|render| render.renderer == Renderer::Ribbon)
            .map(|render| render.trail)
            .next_back()
            .unwrap_or(self.trail);
        Trail { segments: trail.segments.clamp(MIN_TRAIL_SEGMENTS, MAX_TRAIL_SEGMENTS), ..trail }
    }

    #[must_use]
    pub fn prewarm_step(&self) -> f32 {
        if self.max_count < PREWARM_PARTICLE_THRESHOLD {
            PREWARM_SMALL_STEP
        } else {
            PREWARM_LARGE_STEP
        }
    }

    #[must_use]
    pub fn prewarm_steps(&self, step: f32) -> usize {
        let step = step.max(MIN_PREWARM_STEP);
        let mut elapsed = 0.0;
        let mut steps = 0;
        while elapsed < self.start_time && steps < MAX_PREWARM_STEPS {
            elapsed += step;
            steps += 1;
        }
        steps
    }

    #[must_use]
    pub fn needs_audio(&self) -> bool {
        self.emitters.iter().any(|emitter| emitter.audio().is_some())
            || self.operators.iter().any(|operator| operator.audio().is_some())
    }

    #[must_use]
    pub fn follows_mouse(&self) -> bool {
        self.control_points.iter().any(|point| point.flags & 1 != 0)
    }

    fn world_vector(&self, vector: [f32; 3]) -> [f32; 3] {
        let Some(matrix) = &self.world_matrix else {
            return std::array::from_fn(|axis| vector[axis] * self.scale3[axis]);
        };
        let world = std::array::from_fn(|row| {
            (0..SPATIAL_AXES)
                .map(|column| matrix[column * crate::matrix::MATRIX_SIDE + row] * vector[column])
                .sum::<f32>()
        });
        to_frame(world, self.angle.sin_cos())
    }

    fn control_offset(&self, index: usize, position: [f32; 3]) -> [f32; 3] {
        let relative = self.control_points[index].flags & control::ABSOLUTE_CONTROL_FLAGS == 0;
        if self.world && relative { self.world_vector(position) } else { position }
    }

    fn spawn_vector(&self, point: Option<usize>, vector: [f32; 3]) -> [f32; 3] {
        if !self.world {
            return vector;
        }
        let worldspace = point.is_some_and(|index| {
            index != 0 && self.control_points[index].flags & control::WORLDSPACE_CONTROL_FLAG != 0
        });
        if worldspace { to_frame(vector, self.angle.sin_cos()) } else { self.world_vector(vector) }
    }

    #[must_use]
    pub fn draw_scale(&self) -> f32 {
        if self.world { 1.0 } else { self.scale }
    }

    #[must_use]
    pub fn draw_scale3(&self) -> [f32; 3] {
        if self.world { [1.0; 3] } else { self.scale3 }
    }

    pub fn validate_bounds(&mut self, object: &Value) -> bool {
        if !self
            .operators
            .iter()
            .any(|operator| matches!(operator, Operator::CollisionBounds { .. }))
            || ParticleTransform::of(object).is_some_and(|model| model.matches(self))
        {
            return false;
        }
        self.operators.retain(|operator| !matches!(operator, Operator::CollisionBounds { .. }));
        true
    }
}

pub const SPRITE_FLOATS_PER_VERTEX: usize = 17;
pub const SPRITE_POINT_FLOATS: usize = 15;
pub const ROPE_POINT_FLOATS: usize = 24;
const SPLINE_POINTS: usize = 4;
pub const SPRITE_VERTICES: usize = 4;
pub const SPRITE_EXTENT: f32 = 0.5;
pub const SPRITE_INDICES: usize = 6;
pub const MIN_DRAW_ALPHA: f32 = 0.002;
pub const MIN_DRAW_SIZE: f32 = 0.01;
const MIN_PARTICLE_LIFETIME: f32 = 0.01;

#[must_use]
pub fn sprite_indices(capacity: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(capacity * SPRITE_INDICES);
    for particle in 0..capacity as u32 {
        let base = particle * SPRITE_VERTICES as u32;
        out.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 3, base]);
    }
    out
}

pub fn pack_sprites(
    system: &ParticleSystem,
    sim: &Sim,
    frames: usize,
    out: &mut Vec<f32>,
) -> usize {
    out.clear();
    let mut count = 0usize;
    for particle in sim.particles.iter().filter(|particle| particle.drawable()) {
        let life = sprite_life(system, particle, frames);
        let tail = sprite_tail(particle, life);
        for (u, v) in [(0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)] {
            out.extend_from_slice(&[particle.pos[0], particle.pos[1], particle.pos[2], u, v]);
            out.extend_from_slice(&tail);
            out.extend_from_slice(&particle.angle_xy);
        }
        count += 1;
    }
    count
}

fn sprite_tail(particle: &Particle, life: f32) -> [f32; 10] {
    [
        particle.angle,
        particle.size * SPRITE_EXTENT,
        particle.color[0],
        particle.color[1],
        particle.color[2],
        particle.alpha,
        particle.vel[0],
        particle.vel[1],
        particle.vel[2],
        life,
    ]
}

fn sprite_life(system: &ParticleSystem, particle: &Particle, frames: usize) -> f32 {
    if frames <= 1 {
        return (particle.age / particle.lifetime.max(1e-6)).clamp(0.0, 1.0);
    }
    match system.animation {
        Animation::RandomFrame => {
            let index = ((particle.phase / std::f32::consts::TAU) * frames as f32) as usize;
            index.min(frames - 1) as f32 / frames as f32
        }
        Animation::Sequence => {
            let pos = (particle.age / particle.lifetime.max(1e-6)).clamp(0.0, 1.0)
                * system.sequence_multiplier;
            (pos.fract() * (frames as f32 - 1e-4) / frames as f32).clamp(0.0, 0.9999)
        }
        Animation::Once => {
            let pos = (particle.age / particle.lifetime.max(1e-6)).clamp(0.0, 1.0)
                * frames as f32
                * system.sequence_multiplier;
            pos.min(frames as f32 - 1.0).floor() / frames as f32
        }
    }
}

pub fn pack_sprite_points(
    system: &ParticleSystem,
    sim: &Sim,
    frames: usize,
    out: &mut Vec<f32>,
) -> usize {
    out.clear();
    for particle in sim.particles.iter().filter(|particle| particle.drawable()) {
        out.extend_from_slice(&particle.pos);
        out.extend_from_slice(&particle.angle_xy);
        out.extend_from_slice(&sprite_tail(particle, sprite_life(system, particle, frames)));
    }
    out.len() / SPRITE_POINT_FLOATS
}

struct RopePoint {
    pos: [f32; 3],
    size: f32,
    color: [f32; 3],
    alpha: f32,
}

impl From<&Particle> for RopePoint {
    fn from(particle: &Particle) -> Self {
        Self {
            pos: particle.pos,
            size: particle.size,
            color: particle.color,
            alpha: particle.alpha,
        }
    }
}

fn push_rope_point(
    out: &mut Vec<f32>,
    [start, end, before, after]: [RopePoint; SPLINE_POINTS],
    [count, index]: [f32; 2],
) {
    out.extend_from_slice(&[
        start.pos[0],
        start.pos[1],
        start.pos[2],
        start.size * SPRITE_EXTENT,
        end.pos[0],
        end.pos[1],
        end.pos[2],
        count,
        before.pos[0],
        before.pos[1],
        before.pos[2],
        index,
        after.pos[0],
        after.pos[1],
        after.pos[2],
        end.size * SPRITE_EXTENT,
        end.color[0],
        end.color[1],
        end.color[2],
        end.alpha,
        start.color[0],
        start.color[1],
        start.color[2],
        start.alpha,
    ]);
}

pub fn pack_rope_points(sim: &Sim, out: &mut Vec<f32>) -> usize {
    out.clear();
    let count = sim.particles.len() as f32;
    for (index, segment) in spline_segments(sim.particles.iter()).enumerate() {
        if segment[1].visible() && segment.iter().all(|point| finite(point.pos)) {
            push_rope_point(out, segment.map(RopePoint::from), [count, index as f32]);
        }
    }
    out.len() / ROPE_POINT_FLOATS
}

pub fn pack_rope_trail_points(sim: &Sim, render: ParticleRender, out: &mut Vec<f32>) -> usize {
    out.clear();
    for (particle, trail) in sim.particles.iter().zip(&sim.history) {
        if !particle.visible() {
            continue;
        }
        let count = trail.points().count();
        let points = trail.points().chain(std::iter::repeat(&trail.initial)).take(trail.capacity);
        let vertex = |point: &TrailPoint| RopePoint { pos: point.pos, ..particle.into() };
        for (index, segment) in spline_segments(points).enumerate() {
            if !segment.iter().all(|point| finite(point.pos)) {
                continue;
            }
            let coordinates = if render.trail.uv_scroll {
                [index as f32, index as f32 - f32::from(trail.sample_count)]
            } else {
                [count.saturating_sub(1) as f32 * render.trail.uv_factor(), index as f32]
            };
            push_rope_point(out, segment.map(vertex), coordinates);
        }
    }
    out.len() / ROPE_POINT_FLOATS
}

#[derive(Clone, Copy)]
pub struct Particle {
    pub id: u64,
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub size: f32,
    pub base_size: f32,
    pub color: [f32; 3],
    pub base_color: [f32; 3],
    pub alpha: f32,
    pub base_alpha: f32,
    pub angle: f32,
    pub angle_xy: [f32; 2],
    pub angular: f32,
    pub angular_xy: [f32; 2],
    pub age: f32,
    pub lifetime: f32,
    pub phase: f32,
    pub noise_seed: u32,
    pub slot: u32,
}

impl Particle {
    #[must_use]
    pub fn visible(&self) -> bool {
        self.alpha > MIN_DRAW_ALPHA && self.size > MIN_DRAW_SIZE
    }

    #[must_use]
    pub fn drawable(&self) -> bool {
        self.visible()
            && self.angle.is_finite()
            && self.angle_xy.iter().all(|angle| angle.is_finite())
            && self.age.is_finite()
            && self.lifetime.is_finite()
            && finite(self.pos)
            && finite(self.vel)
            && self.color.iter().all(|channel| channel.is_finite())
    }
}

#[must_use]
pub fn finite(vector: [f32; 3]) -> bool {
    vector.iter().all(|component| component.is_finite())
}

pub fn spline_segments<'a, T: 'a>(
    points: impl Iterator<Item = &'a T> + Clone,
) -> impl Iterator<Item = [&'a T; SPLINE_POINTS]> {
    let ends = points.clone().skip(1);
    let afters = points.clone().skip(2).map(Some).chain(std::iter::repeat(None));
    let mut before = None;
    points.zip(ends).zip(afters).map(move |((start, end), after)| {
        let segment = [start, end, before.unwrap_or(start), after.unwrap_or(end)];
        before = Some(start);
        segment
    })
}

fn particle_alive(particle: &Particle) -> bool {
    particle.lifetime != 0.0
        && particle.lifetime.partial_cmp(&particle.age) != Some(std::cmp::Ordering::Less)
}

fn particle_life_fraction(particle: &Particle) -> f32 {
    (particle.age / particle.lifetime).clamp(0.0, 1.0)
}

fn to_frame(vector: [f32; 3], (sin, cos): (f32, f32)) -> [f32; 3] {
    [cos * vector[0] + sin * vector[1], cos * vector[1] - sin * vector[0], vector[2]]
}

#[must_use]
pub fn nonzero_scale(scale: [f32; 3]) -> [f32; 3] {
    scale.map(|axis| if axis.abs() < f32::EPSILON { 1.0 } else { axis })
}

fn collision_response(particle: &mut Particle, normal: [f32; 3], depth: f32, collision: Collision) {
    if collision.stop_angular {
        particle.angular = 0.0;
    }
    let velocity_scale = match collision.response {
        CollisionResponse::Bounce(bounce) => Some(1.0 + bounce),
        CollisionResponse::Slide => Some(1.0),
        CollisionResponse::Stop => None,
        CollisionResponse::Delete => {
            particle.age = particle.lifetime;
            return;
        }
    };
    for (position, normal) in particle.pos.iter_mut().zip(normal) {
        *position -= normal * depth;
    }
    if let Some(scale) = velocity_scale {
        let along = scale * dot3(particle.vel, normal);
        for (velocity, normal) in particle.vel.iter_mut().zip(normal) {
            *velocity -= normal * along;
        }
    } else {
        particle.vel.fill(0.0);
    }
}

fn emitter_time_of(value: Option<&Value>) -> f32 {
    match value {
        Some(Value::Number(number)) => number.as_f64().unwrap_or(0.0) as f32,
        Some(Value::Bool(value)) => f32::from(u8::from(*value)),
        _ => 0.0,
    }
}

fn parse_emitter(entry: &Value) -> Option<Emitter> {
    let flags = num_of(entry.get(COLLISION_FLAGS), 0.0) as u32;
    let name = entry.get("name").and_then(Value::as_str)?.to_ascii_lowercase();
    let origin = vec3_of(entry.get("origin"), [0.0, 0.0, 0.0]);
    let rate = num_of(entry.get("rate"), 10.0).clamp(0.0, 1_000_000.0);
    let instantaneous = num_of(entry.get("instantaneous"), 0.0).clamp(0.0, 100_000.0) as u32;
    let timing = EmitterTiming {
        delay: emitter_time_of(entry.get(EMITTER_DELAY_KEY)),
        duration: emitter_time_of(entry.get(EMITTER_DURATION_KEY)),
        one_per_frame: flags & EMITTER_ONE_PER_FRAME_FLAG != 0,
        periodic: parse_periodic(entry),
    };
    match name.as_str() {
        image::NAME => Some(Emitter::Image {
            image: image::ImageEmitter::parse(entry),
            rate,
            instantaneous,
            timing,
            audio: AudioResponse::parse(entry),
        }),
        sphere::NAME => Some(Emitter::Sphere {
            control_point: entry
                .get("controlpoint")
                .and_then(Value::as_u64)
                .filter(|id| *id < 8)
                .map(|id| id as usize)
                .or(Some(0)),
            origin,
            shape: SphereShape::parse(entry),
            rate,
            instantaneous,
            timing,
            audio: AudioResponse::parse(entry),
        }),
        "boxrandom" => Some(Emitter::Box {
            control_point: entry
                .get("controlpoint")
                .and_then(Value::as_u64)
                .filter(|id| *id < 8)
                .map(|id| id as usize)
                .or(Some(0)),
            origin,
            extent: vec3_of(entry.get("distancemax"), [0.0, 0.0, 0.0]),
            rate,
            instantaneous,
            timing,
            audio: AudioResponse::parse(entry),
        }),
        _ => None,
    }
}

fn parse_periodic(entry: &Value) -> Option<Periodic> {
    let pair = |min_key, max_key| {
        let min = num_of(entry.get(min_key), 0.0).max(0.0);
        let max = num_of(entry.get(max_key), 0.0).max(min);
        [min, max]
    };
    let delay = pair(PERIODIC_DELAY_MIN, PERIODIC_DELAY_MAX);
    let duration = pair(PERIODIC_DURATION_MIN, PERIODIC_DURATION_MAX);
    (delay[1] > 0.0 || duration[1] > 0.0).then_some(Periodic {
        delay,
        duration,
        max_per_period: num_of(entry.get(PERIODIC_MAX_EMIT), 0.0).max(0.0) as usize,
    })
}

fn parse_map_sequence(entry: &Value) -> Initializer {
    let bounds = vec3_of(entry.get(MAP_SEQUENCE_BOUNDS), [0.0, 1.0, 0.0]);
    Initializer::MapSequence {
        control_point: control::point_index(entry.get("controlpoint"), 0.0),
        count: num_of(entry.get("count"), MAP_SEQUENCE_DEFAULT_COUNT).max(MAP_SEQUENCE_MIN_COUNT),
        bounds: [bounds[0], bounds[1]],
        mirror: entry.get(MAP_SEQUENCE_LIMIT).and_then(Value::as_str) == Some(MAP_SEQUENCE_MIRROR),
        axis: vec3_of(entry.get("axis"), [0.0, 0.0, 1.0]),
        min: vec3_of(entry.get("speedmin"), [0.0; 3]),
        max: vec3_of(entry.get("speedmax"), [100.0; 3]),
    }
}

#[expect(clippy::too_many_lines, reason = "initializer decoding is one declarative match")]
fn parse_initializer(entry: &Value, defaults: ParticleDefaults) -> Option<Initializer> {
    let name = entry.get("name").and_then(Value::as_str)?.to_ascii_lowercase();
    let min = entry.get("min");
    let max = entry.get("max");
    let exponent = num_of(entry.get("exponent"), 1.0).clamp(1e-4, 1000.0);
    match name.as_str() {
        "lifetimerandom" => {
            Some(Initializer::Lifetime { min: num_of(min, 1.0), max: num_of(max, 1.0), exponent })
        }
        "sizerandom" => {
            Some(Initializer::Size { min: num_of(min, 10.0), max: num_of(max, 10.0), exponent })
        }
        "alpharandom" => {
            Some(Initializer::Alpha { min: num_of(min, 1.0), max: num_of(max, 1.0), exponent })
        }
        COLOR_RANDOM => Some(Initializer::Color {
            min: vec3_of(min, [255.0, 255.0, 255.0]),
            max: vec3_of(max, [255.0, 255.0, 255.0]),
            exponent,
        }),
        HSV_COLOR_RANDOM => {
            let hue_min = num_of(entry.get(HUE_MIN), 0.0);
            let raw_span = num_of(entry.get(HUE_MAX), 1.0) - hue_min;
            let hue_steps = (num_of(entry.get(HUE_STEPS), HSV_DEFAULT_STEPS) as i32).max(0) as u32;
            let hue_step = if hue_steps > 1 {
                let span = if raw_span == 0.0 { 1.0 } else { raw_span };
                let seam = u32::from((raw_span % 1.0).abs() < HUE_SEAM);
                span / (hue_steps - 1 + seam) as f32
            } else {
                0.0
            };
            Some(Initializer::HsvColor(HsvColor {
                hue_min,
                hue_step,
                hue_steps,
                saturation: (
                    num_of(entry.get(SATURATION_MIN), 0.5),
                    num_of(entry.get(SATURATION_MAX), 1.0),
                ),
                value: (num_of(entry.get(VALUE_MIN), 0.5), num_of(entry.get(VALUE_MAX), 1.0)),
            }))
        }
        COLOR_LIST => {
            let colors = match entry.get(COLORS) {
                None => vec![WHITE_HSV],
                Some(Value::Array(entries)) if entries.is_empty() => vec![RED_HSV],
                Some(Value::Array(entries)) => entries
                    .iter()
                    .map(rgb_color)
                    .map(|color| color.map(rgb_to_hsv))
                    .collect::<Option<Vec<_>>>()?,
                _ => return None,
            };
            Some(Initializer::ColorList(ColorList {
                colors,
                noise: [
                    num_of(entry.get(HUE_NOISE), 0.0),
                    num_of(entry.get(SATURATION_NOISE), 0.0),
                    num_of(entry.get(VALUE_NOISE), 0.0),
                ],
            }))
        }
        INHERIT_CONTROL_VELOCITY => InheritVelocity::parse(entry).map(Initializer::InheritVelocity),
        remap::REMAP_INITIAL => Remap::parse_initial(entry).map(Initializer::Remap),
        "velocityrandom" => Some(Initializer::Velocity {
            min: vec3_of(min, [0.0, 0.0, 0.0]),
            max: vec3_of(max, [0.0, 0.0, 0.0]),
            exponent,
        }),
        BOX_POSITION => Some(Initializer::BoxPosition {
            min: vec3_of(min, [0.0; SPATIAL_AXES]),
            max: vec3_of(max, [0.0; SPATIAL_AXES]),
        }),
        SPHERE_POSITION => {
            Some(Initializer::SpherePosition { min: num_of(min, 0.0), max: num_of(max, 0.0) })
        }
        "rotationrandom" => Some(Initializer::Rotation {
            min: vec3_of(min, [0.0, 0.0, 0.0]),
            max: vec3_of(max, [0.0, 0.0, std::f32::consts::TAU]),
        }),
        "angularvelocityrandom" => Some(Initializer::AngularVelocity {
            min: vec3_of(min, [0.0, 0.0, 0.0]),
            max: vec3_of(max, [0.0, 0.0, 0.0]),
        }),
        "turbulentvelocityrandom" => {
            let speed_min = num_of(entry.get("speedmin"), 100.0);
            let phase_min = num_of(entry.get("phasemin"), 0.0);
            Some(Initializer::TurbulentVelocity(Turbulent {
                speed: (speed_min, num_of(entry.get("speedmax"), 250.0)),
                scale: num_of(entry.get("scale"), 1.0),
                offset: num_of(entry.get("offset"), 0.0),
                forward: vec3_of(entry.get("forward"), [0.0, 1.0, 0.0]),
                right: vec3_of(entry.get("right"), [0.0, 0.0, 1.0]),
                time_scale: num_of(entry.get("timescale"), 1.0),
                phase: (phase_min, num_of(entry.get("phasemax"), 0.1)),
            }))
        }
        event::INHERIT_INITIAL => EventInheritance::parse(entry).map(Initializer::InheritEvent),
        noise::POSITION_OFFSET => {
            Some(Initializer::PositionOffset(PositionOffset::parse(entry, defaults.offset)))
        }
        sequence::BETWEEN_SEQUENCE => {
            Some(Initializer::BetweenSequence(BetweenSequence::parse(entry)))
        }
        "mapsequencearoundcontrolpoint" => Some(parse_map_sequence(entry)),
        _ => None,
    }
}

fn collision_behavior(entry: &Value) -> CollisionResponse {
    match entry.get(COLLISION_BEHAVIOR).and_then(Value::as_str) {
        Some(COLLISION_SLIDE) => CollisionResponse::Slide,
        Some(COLLISION_STOP) => CollisionResponse::Stop,
        Some(COLLISION_DELETE) => CollisionResponse::Delete,
        _ => CollisionResponse::Bounce(num_of(
            entry.get(COLLISION_BOUNCE_FACTOR),
            COLLISION_DEFAULT_BOUNCE,
        )),
    }
}

fn follows_point(entry: &Value) -> bool {
    num_of(entry.get(COLLISION_FLAGS), 0.0) as u32 & COLLISION_CONTROL_POINT_FLAG != 0
}

fn collision_of(entry: &Value) -> Collision {
    Collision {
        stop_angular: num_of(entry.get(COLLISION_FLAGS), 0.0) as u32 & COLLISION_ANGULAR_STOP_FLAG
            != 0,
        response: collision_behavior(entry),
    }
}

fn parse_collision(entry: &Value) -> Option<Collision> {
    (!follows_point(entry)).then(|| collision_of(entry))
}

fn parse_bounds_operator(
    entry: &Value,
    context: &LoadContext<'_>,
    object: &Value,
    doc: &Value,
    depth: usize,
) -> Option<Operator> {
    let viewport = context.viewport?;
    if !context.bounds_static
        || depth != 0
        || object.get(BOUNDS_PARENT).is_some()
        || object.get(BOUNDS_TYPE).and_then(Value::as_str) == Some(BOUNDS_EVENT_FOLLOW)
        || doc
            .get(BOUNDS_CHILDREN)
            .and_then(Value::as_array)
            .is_some_and(|children| !children.is_empty())
    {
        return None;
    }
    let model = ParticleTransform::of(object)?;
    let world = world_space(doc);
    if world && !model.is_identity() {
        return None;
    }
    let planes = model.planes(viewport, world);
    if planes.iter().any(|plane| {
        !plane.distance.is_finite() || plane.normal.iter().any(|component| !component.is_finite())
    }) {
        return None;
    }
    Some(Operator::CollisionBounds {
        planes,
        collision: Collision { response: collision_behavior(entry), stop_angular: false },
    })
}

fn world_space(doc: &Value) -> bool {
    (num_of(doc.get(COLLISION_FLAGS), 0.0) as i64) & WORLD_SPACE_FLAG != 0
}

fn parse_collision_operator(
    entry: &Value,
    name: &str,
    defaults: ParticleDefaults,
) -> Option<Operator> {
    match name {
        COLLISION_PLANE => {
            let collision = parse_collision(entry)?;
            let normal = vec3_of(entry.get(COLLISION_PLANE_NORMAL), PLANE_DEFAULT_NORMAL);
            let scale = 1.0 / dot3(normal, normal).sqrt();
            Some(Operator::CollisionPlane {
                normal: normal.map(|component| component * scale),
                distance: num_of(entry.get(COLLISION_DISTANCE), defaults.plane_distance),
                collision,
            })
        }
        COLLISION_SPHERE => Some(Operator::CollisionSphere {
            origin: vec3_of(entry.get(COLLISION_ORIGIN), defaults.sphere_origin),
            point: follows_point(entry)
                .then(|| control::point_index(entry.get(CONTROL_POINT_FIELD), 0.0)),
            radius: num_of(entry.get(COLLISION_RADIUS), defaults.sphere_radius),
            collision: collision_of(entry),
        }),
        _ => None,
    }
}

fn parse_operator(entry: &Value, defaults: ParticleDefaults) -> Option<Operator> {
    let name = entry.get("name").and_then(Value::as_str)?.to_ascii_lowercase();
    match name.as_str() {
        remap::REMAP_OPERATOR => Remap::parse_operator(entry).map(Operator::Remap),
        event::INHERIT_OPERATOR => EventInheritance::parse_operator(entry)
            .map(|inherit| Operator::InheritEvent { inherit, blend: OperatorBlend::parse(entry) }),
        boids::NAME => Some(Operator::Boids(boids::Boids::parse(entry, defaults.boids))),
        vortex::NAME => Some(Operator::VortexV2(vortex::Vortex::parse(entry, defaults.vortex))),
        "movement" => Some(Operator::Movement {
            gravity: vec3_of(entry.get("gravity"), [0.0, 0.0, 0.0]),
            drag: num_of(entry.get("drag"), 0.0),
        }),
        REDUCE_MOVEMENT => {
            Some(Operator::ReduceMovement(ReduceMovement::parse(entry, defaults.reduce_distance)))
        }
        MAINTAIN_BETWEEN => Some(Operator::MaintainBetween(MaintainBetween::parse(entry))),
        MAINTAIN_DISTANCE => Some(Operator::MaintainDistance(MaintainDistance::parse(
            entry,
            defaults.maintain_distance,
        ))),
        CAP_VELOCITY => Some(Operator::CapVelocity {
            max_speed: num_of(entry.get(MAX_SPEED), defaults.cap_speed),
            blend: OperatorBlend::parse(entry),
        }),
        COLLISION_PLANE | COLLISION_SPHERE => parse_collision_operator(entry, &name, defaults),
        COLLISION_QUAD => collision::Quad::parse(entry).map(Operator::CollisionQuad),
        COLLISION_BOX => Some(Operator::CollisionBox),
        "alphafade" => Some(Operator::AlphaFade {
            fade_in: num_of(entry.get("fadeintime"), 0.5).max(0.0),
            fade_out: num_of(entry.get("fadeouttime"), 0.5).max(0.0),
        }),
        "sizechange" => Some(Operator::SizeChange {
            start: num_of(entry.get("startvalue"), 1.0),
            end: num_of(entry.get("endvalue"), 0.0),
            start_time: num_of(entry.get("starttime"), 0.0),
            end_time: num_of(entry.get("endtime"), 1.0),
        }),
        "alphachange" => Some(Operator::AlphaChange {
            start: num_of(entry.get("startvalue"), 1.0),
            end: num_of(entry.get("endvalue"), 0.0),
            start_time: num_of(entry.get("starttime"), 0.0),
            end_time: num_of(entry.get("endtime"), 1.0),
        }),
        COLOR_CHANGE => Some(Operator::ColorChange {
            start: vec3_of(entry.get("startvalue"), [1.0, 1.0, 1.0]),
            end: vec3_of(entry.get("endvalue"), [0.0, 0.0, 0.0]),
            start_time: num_of(entry.get("starttime"), 0.0),
            end_time: num_of(entry.get("endtime"), 1.0),
        }),
        "oscillatealpha" => Some(Operator::OscillateAlpha(oscillator(entry, 10.0, (0.0, 1.0)))),
        "oscillateposition" => {
            Some(Operator::OscillatePosition(oscillator(entry, 5.0, (0.0, 10.0))))
        }
        "oscillatesize" => Some(Operator::OscillateSize(oscillator(entry, 10.0, (0.8, 1.2)))),
        "angularmovement" => Some(Operator::AngularMovement {
            force: vec3_of(entry.get("force"), [0.0, 0.0, 0.0]),
            drag: num_of(entry.get("drag"), 0.0),
            blend: OperatorBlend::parse(entry),
        }),
        "turbulence" => {
            let speed_min = num_of(entry.get("speedmin"), 500.0);
            Some(Operator::Turbulence {
                scale: num_of(entry.get("scale"), 0.01),
                speed: (speed_min, num_of(entry.get("speedmax"), 1000.0)),
                phase_range: num_of(entry.get("phasemax"), 0.0)
                    - num_of(entry.get("phasemin"), 0.0),
                time_scale: num_of(entry.get("timescale"), 20.0),
                mask: vec3_of(entry.get("mask"), [1.0, 1.0, 0.0]),
                blend: OperatorBlend::parse(entry),
                audio: AudioResponse::parse(entry),
            })
        }
        "controlpointattract" => Some(Operator::ControlPointAttract {
            control_point: num_of(entry.get("controlpoint"), 0.0).clamp(0.0, 7.0) as usize,
            scale: num_of(entry.get("scale"), 512.0),
            threshold: num_of(entry.get("threshold"), 512.0),
            flags: num_of(entry.get("flags"), 2.0) as u32,
            delete_threshold: num_of(entry.get("deletethreshold"), 15.0),
            blend: OperatorBlend::parse(entry),
        }),
        CONTROL_POINT_FORCE => Some(Operator::ControlPointForce {
            control_point: control::point_index(entry.get(CONTROL_POINT_FIELD), 0.0),
            offset: vec3_of(entry.get(FORCE_OFFSET_FIELD), [0.0; SPATIAL_AXES]),
            scale: num_of(entry.get(FORCE_SCALE_FIELD), FORCE_DEFAULT),
            threshold: num_of(entry.get(FORCE_THRESHOLD_FIELD), FORCE_DEFAULT),
            blend: OperatorBlend::parse(entry),
        }),
        "vortex" => {
            let flags = num_of(entry.get("flags"), 0.0) as u32;
            Some(Operator::Vortex(Vortex {
                control_point: num_of(entry.get("controlpoint"), 0.0).clamp(0.0, 7.0) as usize,
                infinite_axis: flags & 1 != 0,
                axis: vec3_of(entry.get("axis"), [0.0, 0.0, 1.0]),
                offset: vec3_of(entry.get("offset"), [0.0; 3]),
                distance: (
                    num_of(entry.get("distanceinner"), 500.0),
                    num_of(entry.get("distanceouter"), 650.0),
                ),
                speed: (
                    num_of(entry.get("speedinner"), 2500.0),
                    num_of(entry.get("speedouter"), 0.0),
                ),
                audio: AudioResponse::parse(entry),
            }))
        }
        _ => None,
    }
}

fn parse_steps<T>(
    doc: &Value,
    key: &str,
    path: &str,
    mut parse: impl FnMut(&Value) -> Option<T>,
    skipped: &mut Vec<String>,
) -> Vec<T> {
    let mut steps = Vec::new();
    for (index, entry) in doc.get(key).and_then(Value::as_array).into_iter().flatten().enumerate() {
        if let Some(step) = parse(entry) {
            steps.push(step);
        } else {
            let name = entry.get(STEP_NAME).and_then(Value::as_str).unwrap_or(UNNAMED_STEP);
            skipped.push(format!("particle {path}: {key} {index} ({name}) unsupported"));
        }
    }
    steps
}

pub enum Source<'a> {
    Pkg(&'a Package, &'a crate::effects::Assets),
    Dir(std::path::PathBuf),
}

impl Source<'_> {
    fn json(&self, relative: &str) -> Option<Value> {
        match self {
            Self::Pkg(pkg, assets) => pkg.find_json(relative).ok().flatten().or_else(|| {
                assets.read(relative).and_then(|text| crate::json::parse(text.as_bytes()).ok())
            }),
            Self::Dir(root) => {
                let bytes = crate::effects::read_confined_bytes(
                    root,
                    relative,
                    crate::json::MAX_JSON_BYTES,
                )?;
                crate::json::parse(&bytes).ok()
            }
        }
    }

    fn texture(&self, name: &str) -> Option<crate::model::Texture> {
        self.load(name).filter(|texture| texture.stream.is_none())
    }

    fn load(&self, name: &str) -> Option<crate::model::Texture> {
        match self {
            Self::Pkg(pkg, assets) => {
                let candidates = crate::model::tex_candidates(name);
                let mut preset = candidates.clone();
                preset.sort_by_key(|path| !path.starts_with(crate::model::MATERIALS_DIR));
                crate::model::load_texture_from(pkg, assets, &candidates, &preset)
            }
            Self::Dir(root) => {
                let mut candidates = crate::model::tex_candidates(name);
                candidates.sort_by_key(|path| !path.starts_with(crate::model::MATERIALS_DIR));
                for candidate in candidates {
                    if let Some(bytes) = crate::effects::read_confined_bytes(
                        root,
                        &candidate,
                        crate::pkg::MAX_PACKAGE_ENTRY_BYTES,
                    ) && let Ok(texture) = crate::model::load_texture_bytes(&bytes)
                    {
                        return Some(texture);
                    }
                }
                None
            }
        }
    }
}

fn sprite_texture(source: &Source, doc: &Value) -> Option<crate::model::Texture> {
    let material = doc.get("material").and_then(Value::as_str)?;
    let json = source.json(material)?;
    let passes = json.get("passes")?.as_array()?;
    for pass in passes {
        let textures = pass.get("textures").and_then(Value::as_array)?;
        for entry in textures {
            if let Some(name) = entry.as_str().filter(|text| !text.is_empty())
                && let Some(texture) = source.texture(name)
            {
                return Some(texture);
            }
        }
    }
    None
}

fn material_json(source: &Source, doc: &Value) -> Option<Value> {
    source.json(doc.get("material").and_then(Value::as_str)?)
}

fn material_blend(material: Option<&Value>) -> ParticleBlend {
    let mode = material
        .and_then(|json| json.get("passes"))
        .and_then(Value::as_array)
        .and_then(|passes| passes.first())
        .and_then(|pass| pass.get("blending"))
        .and_then(Value::as_str)
        .unwrap_or("additive");
    if mode.eq_ignore_ascii_case("translucent") {
        ParticleBlend::Translucent
    } else if mode.eq_ignore_ascii_case("normal") {
        ParticleBlend::Normal
    } else {
        ParticleBlend::Additive
    }
}

fn engine_pass(
    pkg: &Package,
    assets: &crate::effects::Assets,
    material: Option<&Value>,
    render: ParticleRender,
    texture: Option<&crate::model::Texture>,
) -> Option<crate::effects::EffectPass> {
    let mut material = material?.clone();
    if matches!(render.renderer, Renderer::Rope | Renderer::Ribbon)
        && let Some(pass) = material
            .get_mut(MATERIAL_PASSES)
            .and_then(Value::as_array_mut)
            .and_then(|passes| passes.first_mut())
        && pass.get(MATERIAL_SHADER).and_then(Value::as_str) == Some(SPRITE_SHADER)
    {
        pass[MATERIAL_SHADER] = Value::String(ROPE_SHADER.into());
    }
    let frames = texture.map_or(0, |texture| texture.frames.len());
    let mut combos = std::collections::BTreeMap::new();
    if let Some(texture) = texture {
        combos.insert("TEX0FORMAT".to_string(), texture.we_format());
    }
    combos.insert("THICKFORMAT".to_string(), 1i64);
    combos.insert("SPRITESHEET".to_string(), i64::from(frames > 1));
    combos.insert(
        "TRAILRENDERER".to_string(),
        i64::from(matches!(render.renderer, Renderer::Trail | Renderer::Ribbon)),
    );
    combos.insert(TRAIL_SUBDIVISION_COMBO.to_string(), i64::from(render.trail.subdivision));
    let scrolling = render.renderer == Renderer::Ribbon && render.trail.uv_scroll;
    combos.insert(TRAIL_SCROLL_COMBO.to_string(), i64::from(scrolling));
    combos.insert(TRAIL_ALPHA_COMBO.to_string(), i64::from(scrolling && render.trail.fade_alpha));
    combos.insert(TRAIL_SIZE_COMBO.to_string(), i64::from(scrolling && render.trail.fade_size));
    crate::effects::particle_pass(pkg, assets, &material, &combos)
}

fn share_trails(primary: &mut ParticleRender, extras: &mut [ParticleRender]) {
    let mut settings = primary.trail;
    settings.uv_scroll = false;
    settings.uv_scale = 1.0;
    for render in std::iter::once(*primary).chain(extras.iter().copied()) {
        if render.renderer == Renderer::Ribbon {
            settings.length = render.trail.length;
            settings.segments = render.trail.segments.max(MIN_TRAIL_SEGMENTS);
            settings.uv_scroll |= render.trail.uv_scroll;
            if render.trail.uv_scale != 0.0 {
                settings.uv_scale = render.trail.uv_scale;
            }
        }
    }
    for render in std::iter::once(primary).chain(extras.iter_mut()) {
        if render.renderer == Renderer::Ribbon {
            render.trail.length = settings.length;
            render.trail.segments = settings.segments;
            render.trail.uv_scroll = settings.uv_scroll;
            render.trail.uv_scale = settings.uv_scale;
        }
    }
}

pub fn load(
    context: &LoadContext<'_>,
    object: &Value,
    path: &str,
) -> (Option<ParticleSystem>, Vec<String>) {
    let mut skipped = Vec::new();
    let mut budget = MAX_OBJECT_SYSTEMS;
    let system = load_inner(context, object, path, 0, &mut budget, &mut skipped);
    (system, skipped)
}

fn load_inner(
    context: &LoadContext<'_>,
    object: &Value,
    path: &str,
    depth: usize,
    budget: &mut usize,
    skipped: &mut Vec<String>,
) -> Option<ParticleSystem> {
    let pkg = context.pkg;
    let assets = context.assets;
    if depth >= MAX_CHILD_DEPTH {
        return None;
    }
    *budget -= 1;
    let mut source = Source::Pkg(pkg, assets);
    let mut doc = source.json(path);
    if doc.is_none()
        && let Some(stem) = path.rsplit('/').next().and_then(|file| file.strip_suffix(".json"))
        && let Some(root) = assets.preset_root(stem)
    {
        source = Source::Dir(root);
        doc = source.json(path);
    }
    let doc = doc?;
    let renderers = doc.get("renderer").and_then(Value::as_array);
    let entry = renderers.and_then(|list| list.first());
    let primary = entry.map_or(Some(ParticleRender::SPRITE), ParticleRender::parse);
    let mut extra_renderers = Vec::new();
    if let Some(renderers) = renderers {
        for (index, entry) in renderers.iter().enumerate().skip(1) {
            if let Some(render) = ParticleRender::parse(entry) {
                extra_renderers.push(render);
            } else {
                let name = entry.get(STEP_NAME).and_then(Value::as_str).unwrap_or(UNNAMED_STEP);
                skipped.push(format!("particle {path}: renderer {index} ({name}) unsupported"));
            }
        }
    }
    if primary.is_none() {
        let name = entry
            .and_then(|entry| entry.get(STEP_NAME))
            .and_then(Value::as_str)
            .unwrap_or(UNNAMED_STEP);
        skipped.push(format!("particle {path}: renderer 0 ({name}) unsupported"));
    }
    let mut ordinal = 0;
    let emitters = parse_steps(
        &doc,
        EMITTER_FIELD,
        path,
        |entry| {
            let is_image = entry
                .get(STEP_NAME)
                .and_then(Value::as_str)
                .is_some_and(|name| name.eq_ignore_ascii_case(image::NAME));
            let mut emitter = parse_emitter(entry);
            if let Some(Emitter::Image { image, .. }) = &mut emitter {
                image.ordinal = ordinal;
            }
            ordinal += usize::from(is_image);
            emitter
        },
        skipped,
    );
    let mut initializers = parse_steps(
        &doc,
        INITIALIZER_FIELD,
        path,
        |entry| parse_initializer(entry, context.defaults),
        skipped,
    );
    let mut model_ordinal = 0;
    let mut operators = parse_steps(
        &doc,
        OPERATOR_FIELD,
        path,
        |entry| {
            if entry.get(STEP_NAME).and_then(Value::as_str) == Some(model_collision::NAME) {
                let ordinal = model_ordinal;
                model_ordinal += 1;
                Some(Operator::CollisionModel {
                    ordinal,
                    collision: Collision {
                        response: collision_behavior(entry),
                        stop_angular: false,
                    },
                })
            } else if entry.get(STEP_NAME).and_then(Value::as_str) == Some(COLLISION_BOUNDS) {
                parse_bounds_operator(entry, context, object, &doc, depth)
            } else {
                parse_operator(entry, context.defaults)
            }
        },
        skipped,
    );
    let authored =
        doc.get(EMITTER_FIELD).and_then(Value::as_array).is_some_and(|list| !list.is_empty());
    let parent =
        doc.get(BOUNDS_CHILDREN).and_then(Value::as_array).is_some_and(|list| !list.is_empty());
    if emitters.is_empty() && (authored || !parent) {
        skipped.push(format!("particle {path}: no supported emitters"));
        return None;
    }
    let mut primary = primary?;
    share_trails(&mut primary, &mut extra_renderers);
    let renderer = primary.renderer;
    let orientation = primary.orientation;
    let renderer_flags = primary.flags;
    let trail = primary.trail;
    let over = object.get(INSTANCE_OVERRIDE_FIELD);
    let props = &assets.properties;
    let normalized = over
        .and_then(|value| value.get(COLORN_FIELD))
        .and_then(|value| crate::effects::bound_value(value, props));
    let legacy = over
        .and_then(|value| value.get(LEGACY_COLOR_FIELD))
        .and_then(|value| crate::effects::bound_value(value, props))
        .map(|parts| {
            let pick = |index: usize| parts.get(index).copied().unwrap_or_else(|| parts[0]);
            if parts.is_empty() {
                [1.0; COLOR_CHANNELS]
            } else {
                rounded_color([pick(0), pick(1), pick(2)].map(|value| value / f32::from(u8::MAX)))
            }
        });
    let has_override = normalized.is_some() || legacy.is_some();
    let tint = normalized.map_or_else(
        || legacy.unwrap_or([1.0; COLOR_CHANNELS]),
        |parts| {
            let pick = |index: usize| parts.get(index).copied().unwrap_or_else(|| parts[0]);
            if parts.is_empty() { [1.0; COLOR_CHANNELS] } else { [pick(0), pick(1), pick(2)] }
        },
    );
    let summary = authored_color(&doc);
    let definition_flags = num_of(doc.get(DEFINITION_FLAGS_FIELD), 0.0) as i64;
    let changed = has_override
        && tint[0] >= 0.0
        && definition_flags & COLOR_RECOLOR_DISABLED == 0
        && tint.iter().all(|channel| channel.is_finite())
        && summary.is_none_or(|color| {
            color.iter().zip(tint).any(|(original, override_color)| {
                (original - override_color).abs() > COLOR_OVERRIDE_TOLERANCE
            })
        });
    if changed && let Some(color) = summary {
        let original = rgb_to_hsv(color);
        let replacement = rgb_to_hsv(tint);
        for initializer in &mut initializers {
            if let Initializer::Color { min, max, .. } = initializer {
                for endpoint in [min, max] {
                    let rgb = recolor_endpoint(
                        endpoint.map(|value| value / f32::from(u8::MAX)),
                        original,
                        replacement,
                    );
                    *endpoint = rgb.map(|value| value * f32::from(u8::MAX));
                }
            }
        }
    }
    let base_color = if changed && (context.version < COLOR_VERSION || summary.is_none()) {
        tint
    } else {
        [1.0; COLOR_CHANNELS]
    };
    let count_scale =
        over.map_or(1.0, |value| num_bound(value.get("count"), props, 1.0)).clamp(0.0, 10.0);
    let lifetime_scale =
        over.map_or(1.0, |value| num_bound(value.get("lifetime"), props, 1.0)).clamp(0.0, 100.0);
    let speed_scale =
        over.map_or(1.0, |value| num_bound(value.get("speed"), props, 1.0)).clamp(0.0, 100.0);
    for initializer in &mut initializers {
        match initializer {
            Initializer::Lifetime { min, max, .. } => {
                *min *= lifetime_scale;
                *max *= lifetime_scale;
            }
            Initializer::Velocity { min, max, .. } | Initializer::MapSequence { min, max, .. } => {
                for axis in 0..3 {
                    min[axis] *= speed_scale;
                    max[axis] *= speed_scale;
                }
            }
            Initializer::TurbulentVelocity(turbulent) => {
                turbulent.speed.0 *= speed_scale;
                turbulent.speed.1 *= speed_scale;
            }
            _ => {}
        }
    }
    for operator in &mut operators {
        match operator {
            Operator::Movement { gravity, .. } => {
                for axis in gravity {
                    *axis *= speed_scale;
                }
            }
            Operator::ControlPointAttract { scale, .. }
            | Operator::ControlPointForce { scale, .. } => *scale *= speed_scale,
            Operator::Vortex(vortex) => {
                vortex.speed.0 *= speed_scale;
                vortex.speed.1 *= speed_scale;
            }
            _ => {}
        }
    }
    let origin = vec3_of(object.get("origin"), [0.0, 0.0, 0.0]);
    let scale3 = vec3_of(object.get("scale"), [1.0, 1.0, 1.0]);
    let scale = scale3[0];
    let animation = match doc.get("animationmode").and_then(Value::as_str) {
        Some("randomframe") => Animation::RandomFrame,
        Some("once") => Animation::Once,
        _ => Animation::Sequence,
    };
    let material = material_json(&source, &doc);
    let blend = material_blend(material.as_ref());
    let texture = if emitters.is_empty() { None } else { sprite_texture(&source, &doc) };
    let pass = engine_pass(pkg, assets, material.as_ref(), primary, texture.as_ref());
    let extra_passes = extra_renderers
        .iter()
        .enumerate()
        .map(|(index, render)| {
            if *render == primary || extra_renderers[..index].contains(render) {
                None
            } else {
                engine_pass(pkg, assets, material.as_ref(), *render, texture.as_ref())
            }
        })
        .collect();
    let grab_slot = pass.as_ref().and_then(|pass| {
        pass.binds
            .iter()
            .find(|(_, bind)| *bind == crate::effects::EffectBind::SceneSoFar)
            .map(|(slot, _)| *slot)
    });
    let children: Vec<_> = doc
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, child)| {
            let Some(child_path) = child.get("name").and_then(Value::as_str) else {
                skipped.push(format!("particle {path}: child {index} unnamed"));
                return None;
            };
            if *budget == 0 {
                skipped.push(format!(
                    "particle {path}: child {index} ({child_path}) over the {MAX_OBJECT_SYSTEMS} system budget"
                ));
                return None;
            }
            let mut child = child.clone();
            if let Some(overrides) =
                child.get_mut(INSTANCE_OVERRIDE_FIELD).and_then(Value::as_object_mut)
            {
                overrides.retain(|key, _| key != COLORN_FIELD);
            }
            if let Some(overrides) = object.get(INSTANCE_OVERRIDE_FIELD) {
                child[INSTANCE_OVERRIDE_FIELD] = overrides.clone();
            }
            child[dependency::DEPENDENCIES] = object
                .get(dependency::DEPENDENCIES)
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new()));
            let system = load_inner(context, &child, child_path, depth + 1, budget, skipped);
            if system.is_none() {
                skipped.push(format!("particle {path}: child {index} ({child_path}) unsupported"));
            }
            system
        })
        .collect();
    let has_death_children = children
        .iter()
        .any(|child| child.child_event.is_some_and(|event| event.kind == ChildEventKind::Death));
    let size_scale = over
        .map_or(1.0, |value| num_bound(value.get("size"), props, 1.0))
        .clamp(0.0, MAX_INSTANCE_SIZE_SCALE);
    let angular_motion =
        initializers.iter().any(|step| matches!(step, Initializer::AngularVelocity { .. }))
            || operators.iter().any(|step| matches!(step, Operator::AngularMovement { .. }));
    Some(ParticleSystem {
        model_collisions: model_collision::Bindings::parse(object, model_ordinal),
        image_dependencies: image::ImageDependency::parse(object),
        children,
        has_death_children,
        child_event: ChildEvent::parse(object),
        renderer,
        orientation,
        renderer_flags,
        trail,
        extra_renderers,
        animation,
        sequence_multiplier: num_of(doc.get("sequencemultiplier"), 1.0).clamp(0.01, 100.0),
        angular_motion,
        remapped_points: remap::written_points(&initializers, &operators),
        max_count: num_of(doc.get("maxcount"), 100.0).round().clamp(1.0, 100_000.0) as usize,
        start_time: num_of(doc.get("starttime"), 0.0).clamp(0.0, 300.0),
        control_points: control_points(&doc),
        emitters,
        initializers,
        operators,
        texture,
        additive: blend == ParticleBlend::Additive,
        blend,
        perspective: (num_of(doc.get("flags"), 0.0) as i64) & 4 != 0,
        world: world_space(&doc),
        pass,
        extra_passes,
        origin: (origin[0], origin[1], origin[2]),
        angle: vec3_of(object.get("angles"), [0.0, 0.0, 0.0])[2],
        spatial: None,
        world_matrix: None,
        scale: if scale.abs() < f32::EPSILON { 1.0 } else { scale },
        scale3: nonzero_scale(scale3),
        base_color,
        alpha: over.map_or(1.0, |value| num_bound(value.get("alpha"), props, 1.0)).clamp(0.0, 1.0),
        count_scale,
        rate_scale: over
            .map_or(1.0, |value| num_bound(value.get("rate"), props, 1.0))
            .clamp(0.0, 10.0),
        size_scale,
        authored_size_scale: size_scale,
        grab_slot,
        instance: InstanceFactors::default(),
    })
}

struct EmitterTimer {
    delay: f32,
    duration: f32,
    pending: f32,
    instantaneous: u32,
    disabled: bool,
    phase_left: f32,
    phase_active: bool,
    period_emitted: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlaybackState {
    Playing,
    Stopped,
}

#[derive(Clone, Copy)]
enum SpawnOrigin {
    Automatic,
    Manual,
}

#[derive(Clone, Copy, Default)]
struct SequencePhase {
    phase: f32,
    step: f32,
}

impl SequencePhase {
    fn advance(&mut self, mirror: bool) {
        self.phase += self.step;
        if self.phase > 1.0 {
            if mirror {
                self.phase = 2.0 - self.phase;
                self.step = -self.step;
            } else {
                self.phase %= 1.0;
            }
        } else if self.phase < 0.0 {
            self.phase = -self.phase;
            self.step = -self.step;
        }
    }
}

impl EmitterTimer {
    fn new(emitter: &Emitter) -> Self {
        let timing = emitter.timing();
        Self {
            delay: timing.delay,
            duration: timing.duration,
            pending: 0.0,
            instantaneous: emitter.instantaneous(),
            disabled: false,
            phase_left: 0.0,
            phase_active: true,
            period_emitted: 0,
        }
    }

    fn births(
        &mut self,
        emitter: &Emitter,
        dt: f32,
        scale: f32,
        rng: &mut Rng,
        audio: (&[f32], &[f32]),
        can_emit: bool,
    ) -> usize {
        let timing = emitter.timing();
        let mut remaining = dt;
        let mut births = 0;
        let mut first = true;
        while remaining > 0.0 || (first && self.instantaneous > 0) {
            first = false;
            if let Some(periodic) = timing.periodic
                && self.phase_left <= 0.0
            {
                self.phase_left = periodic.phase(self.phase_active, rng);
            }
            let slice = timing.periodic.map_or(remaining, |_| remaining.min(self.phase_left));
            if self.phase_active && can_emit {
                let instant = self.instantaneous as usize;
                self.instantaneous = 0;
                self.pending += emitter.active_rate(audio.0, audio.1) * scale * slice;
                let due = if self.duration < 0.0 { 0 } else { self.pending.floor() as usize };
                self.pending -= due as f32 + instant as f32;
                let due =
                    if timing.one_per_frame { due.min(usize::from(births == 0)) } else { due };
                let allowance = timing.periodic.map_or(usize::MAX, |periodic| {
                    if periodic.max_per_period == 0 {
                        usize::MAX
                    } else {
                        periodic.max_per_period.saturating_sub(self.period_emitted)
                    }
                });
                let added = (due + instant).min(allowance);
                births += added;
                self.period_emitted += added;
            }
            remaining -= slice;
            if timing.periodic.is_some() {
                self.phase_left -= slice;
                if self.phase_left <= f32::EPSILON {
                    self.phase_active = !self.phase_active;
                    self.phase_left = 0.0;
                    if self.phase_active {
                        self.period_emitted = 0;
                    }
                }
            }
        }
        if self.duration > 0.0 {
            self.duration -= dt;
            self.disabled = self.duration <= 0.0;
        }
        if self.instantaneous == 0 && emitter.rate() <= 0.0 && emitter.audio().is_none() {
            self.disabled = true;
        }
        births
    }
}

impl Periodic {
    fn phase(self, active: bool, rng: &mut Rng) -> f32 {
        let [min, max] = if active { self.duration } else { self.delay };
        let phase = rng.range(min, max);
        if phase > 0.0 { phase } else { PERIODIC_FALLBACK_SECONDS }
    }
}

pub struct Sim {
    model_capsules: Vec<Option<model_collision::Capsule>>,
    model_pose: Option<(ParticleTransform, bool)>,
    model_invalid: bool,
    image_sources: Vec<Option<image::ImageSource>>,
    image_refresh: Vec<f32>,
    pub particles: Vec<Particle>,
    control_points: [[f32; 3]; 8],
    control_previous: Option<[[f32; 3]; 8]>,
    control_velocity: [[f32; 3]; 8],
    transform: Option<ParticleTransform>,
    pointer: Option<[f32; 2]>,
    burst_done: bool,
    pub history: Vec<TrailHistory>,
    rng: Rng,
    pending: f32,
    emitter_timers: Vec<EmitterTimer>,
    time: f32,
    ribbon: bool,
    history_remaining: f32,
    sequence_phases: Vec<SequencePhase>,
    next_id: u64,
    dead: usize,
    births: Vec<Particle>,
    birth_history: Vec<TrailHistory>,
    spare: Vec<Particle>,
    spare_history: Vec<TrailHistory>,
    slot_cursor: usize,
    next_slot: u32,
    prewarmed: std::collections::HashSet<u64>,
    prewarming: bool,
    emitting: bool,
    playback: PlaybackState,
    paused: bool,
    manual_only: bool,
    emission_offset: [f32; 3],
    event: Option<EventValues>,
    live_event: Option<EventValues>,
    retired: Vec<Particle>,
    step_id: u64,
    scene_frame: Option<u32>,
    clock: crate::effects::FrameClock,
    layer_time: f32,
    layer_visible: bool,
    parent_hidden: bool,
    boids: Option<Box<boids::Pool>>,
    gains: Vec<f32>,
}

impl Sim {
    #[must_use]
    pub fn new(seed: u32) -> Self {
        Self {
            model_capsules: Vec::new(),
            model_pose: None,
            model_invalid: false,
            image_sources: Vec::new(),
            image_refresh: Vec::new(),
            particles: Vec::new(),
            history: Vec::new(),
            rng: Rng::new(seed),
            pending: 0.0,
            emitter_timers: Vec::new(),
            control_points: [[0.0; 3]; 8],
            control_previous: None,
            control_velocity: [[0.0; 3]; control::CONTROL_POINTS],
            transform: None,
            pointer: None,
            burst_done: false,
            time: 0.0,
            ribbon: false,
            history_remaining: 0.0,
            sequence_phases: Vec::new(),
            next_id: 0,
            dead: 0,
            births: Vec::new(),
            birth_history: Vec::new(),
            spare: Vec::new(),
            spare_history: Vec::new(),
            slot_cursor: 0,
            next_slot: 0,
            prewarmed: std::collections::HashSet::new(),
            prewarming: false,
            emitting: true,
            playback: PlaybackState::Playing,
            paused: false,
            manual_only: false,
            emission_offset: [0.0; 3],
            event: None,
            live_event: None,
            retired: Vec::new(),
            step_id: 0,
            scene_frame: None,
            clock: crate::effects::FrameClock::default(),
            layer_time: 0.0,
            layer_visible: true,
            parent_hidden: false,
            boids: None,
            gains: Vec::new(),
        }
    }

    pub fn bind_images(&mut self, sources: Vec<Option<image::ImageSource>>) {
        self.image_sources = sources;
    }

    pub fn set_scene_frame(&mut self, frame: u32) {
        self.scene_frame = Some(frame);
    }

    pub fn hide(&mut self) {
        self.clear();
        self.burst_done = self.manual_only;
        if self.playback != PlaybackState::Stopped {
            self.emitting |= !self.manual_only;
        }
    }

    pub fn set_layer_visible(&mut self, visible: bool) {
        self.layer_visible = visible;
    }

    pub fn set_parent_hidden(&mut self, hidden: bool) {
        self.parent_hidden = hidden;
    }

    #[must_use]
    pub fn layer_time(&self) -> f32 {
        self.layer_time
    }

    pub fn set_scene_clock(&mut self, clock: crate::effects::FrameClock) {
        self.clock = clock;
    }

    pub fn set_pointer(&mut self, pointer: [f32; 2]) {
        self.pointer = Some(pointer);
    }

    pub fn play(&mut self) {
        if self.playback == PlaybackState::Stopped || (!self.emitting && self.particles.is_empty())
        {
            self.burst_done = false;
            self.pending = 0.0;
            self.emitter_timers.clear();
            self.manual_only = false;
            self.emitting = true;
        }
        self.playback = PlaybackState::Playing;
        self.paused = false;
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn stop(&mut self) {
        self.playback = PlaybackState::Stopped;
        self.emitting = false;
        self.manual_only = false;
        self.clear();
    }

    fn clear(&mut self) {
        self.particles.clear();
        self.history.clear();
        self.retired.clear();
        self.live_event = None;
        self.boids = None;
        self.burst_done = false;
        self.pending = 0.0;
        self.emitter_timers.clear();
        self.image_refresh.clear();
        self.sequence_phases.clear();
        self.time = 0.0;
        self.layer_time = 0.0;
    }

    #[must_use]
    pub fn is_playing(&self) -> bool {
        !self.paused && (self.emitting || !self.particles.is_empty())
    }

    pub fn emit_particles(
        &mut self,
        system: &ParticleSystem,
        count: u32,
        query: &mut dyn FnMut(&image::ImageSource),
    ) {
        if count == 0 || system.emitters.is_empty() {
            return;
        }
        if self.playback == PlaybackState::Stopped {
            self.manual_only = true;
            self.burst_done = true;
            self.playback = PlaybackState::Playing;
        }
        if self.emitter_timers.len() != system.emitters.len() {
            self.emitter_timers = system.emitters.iter().map(EmitterTimer::new).collect();
            if self.burst_done {
                for (timer, emitter) in self.emitter_timers.iter_mut().zip(&system.emitters) {
                    timer.instantaneous = 0;
                    timer.disabled = emitter.rate() <= 0.0 && emitter.audio().is_none();
                }
            }
            if self.emitter_timers.len() == 1 {
                self.emitter_timers[0].pending = self.pending;
            }
            self.pending = 0.0;
        }
        let mut permits = vec![None; system.emitters.len()];
        self.begin_births();
        for index in 0..system.emitters.len() {
            let available = Self::capacity(system).saturating_sub(self.population());
            if available == 0 {
                break;
            }
            let timer = &mut self.emitter_timers[index];
            let instant = if self.emitting && !self.paused && !timer.disabled && timer.delay <= 0.0
            {
                let instant = timer.instantaneous as usize;
                timer.instantaneous = 0;
                timer.pending -= (count as usize + instant) as f32;
                instant
            } else {
                0
            };
            for _ in 0..(count as usize + instant).min(available) {
                self.emit(system, Some(index), SpawnOrigin::Manual, &mut permits, query);
            }
        }
        self.settle_births();
    }

    fn begin_births(&mut self) {
        self.slot_cursor = 0;
        self.next_slot = 0;
    }

    fn population(&self) -> usize {
        self.particles.len() - self.dead + self.births.len()
    }

    fn free_slot(&mut self) -> u32 {
        while self.particles.get(self.slot_cursor).is_some_and(|particle| {
            particle.slot < self.next_slot
                || particle.slot == self.next_slot && particle_alive(particle)
        }) {
            if self.particles[self.slot_cursor].slot == self.next_slot {
                self.next_slot += 1;
            }
            self.slot_cursor += 1;
        }
        let slot = self.next_slot;
        self.next_slot += 1;
        slot
    }

    fn settle_births(&mut self) {
        let appended = self.dead == 0
            && self
                .births
                .first()
                .is_none_or(|born| self.particles.last().is_none_or(|last| last.slot < born.slot));
        if appended {
            self.particles.append(&mut self.births);
            self.history.append(&mut self.birth_history);
            return;
        }
        self.spare.clear();
        self.spare_history.clear();
        let mut history = self.history.drain(..);
        let mut birth_history = self.birth_history.drain(..);
        let mut births = self.births.drain(..).peekable();
        for particle in self.particles.drain(..) {
            let trail = history.next();
            if !particle_alive(&particle) {
                continue;
            }
            while let Some(born) = births.next_if(|born| born.slot < particle.slot) {
                self.spare.push(born);
                self.spare_history.extend(birth_history.next());
            }
            self.spare.push(particle);
            self.spare_history.extend(trail);
        }
        for born in births {
            self.spare.push(born);
            self.spare_history.extend(birth_history.next());
        }
        drop((history, birth_history));
        std::mem::swap(&mut self.particles, &mut self.spare);
        std::mem::swap(&mut self.history, &mut self.spare_history);
        self.dead = 0;
    }

    #[must_use]
    pub fn capacity(system: &ParticleSystem) -> usize {
        system.max_count.min(MAX_PARTICLES)
    }

    pub fn prewarm(&mut self, system: &ParticleSystem, step: f32) {
        let step = step.max(MIN_PREWARM_STEP);
        let steps = system.prewarm_steps(step);
        if steps > 0 {
            self.begin_prewarm();
        }
        for _ in 0..steps {
            self.step(system, step);
        }
        if steps > 0 {
            self.finish_prewarm();
        }
    }

    pub fn begin_prewarm(&mut self) {
        self.prewarming = true;
    }

    pub fn finish_prewarm(&mut self) {
        self.prewarming = false;
        self.history_remaining = 0.0;
        self.prewarmed.clear();
        self.prewarmed.extend(self.particles.iter().map(|particle| particle.id));
        self.retired.clear();
        for (history, particle) in self.history.iter_mut().zip(&self.particles) {
            history.reset(particle, self.time);
        }
    }

    fn has_emission(&self, system: &ParticleSystem) -> bool {
        if self.emitter_timers.is_empty() {
            !self.burst_done || system.emitters.iter().any(|emitter| emitter.rate() > 0.0)
        } else {
            self.emitter_timers.iter().zip(&system.emitters).any(|(timer, emitter)| {
                !timer.disabled && (timer.instantaneous > 0 || emitter.rate() > 0.0)
            })
        }
    }

    fn retire_existing(&mut self, system: &ParticleSystem, dt: f32) {
        for particle in &mut self.particles {
            particle.age += dt;
        }
        if system.has_death_children {
            self.retired.extend(
                self.particles.iter().filter(|particle| !particle_alive(particle)).copied(),
            );
        }
        self.dead = self.particles.iter().filter(|particle| !particle_alive(particle)).count();
    }

    fn emit_timed(
        &mut self,
        system: &ParticleSystem,
        dt: f32,
        capacity: usize,
        left: &[f32],
        right: &[f32],
        query: &mut dyn FnMut(&image::ImageSource),
    ) {
        if self.emitter_timers.len() != system.emitters.len() {
            self.emitter_timers = system.emitters.iter().map(EmitterTimer::new).collect();
        }
        self.image_refresh.resize(system.emitters.len(), 0.0);
        let mut permits = vec![None; system.emitters.len()];
        for index in 0..system.emitters.len() {
            let room = self.population() < capacity;
            let births = {
                let timer = &mut self.emitter_timers[index];
                if timer.disabled {
                    continue;
                }
                if timer.delay > 0.0 {
                    timer.delay -= dt;
                    continue;
                }
                timer.births(
                    &system.emitters[index],
                    dt,
                    system.count_scale,
                    &mut self.rng,
                    (left, right),
                    room,
                )
            };
            let births = births.min(capacity.saturating_sub(self.population()));
            if births > 0
                && let Emitter::Image { image, .. } = &system.emitters[index]
                && image.flags.contains(image::ImageFlags::REFRESH)
            {
                self.image_refresh[index] += dt;
                if self.image_refresh[index] > image::REFRESH_SECONDS {
                    self.image_refresh[index] %= image::REFRESH_SECONDS;
                    if let Some(source) =
                        self.image_sources.get(image.ordinal).and_then(Option::as_ref)
                    {
                        source.borrow_mut().invalidate();
                    }
                }
            }
            for _ in 0..births {
                self.emit(system, Some(index), SpawnOrigin::Automatic, &mut permits, query);
            }
        }
    }

    fn image_allowed(&self, ordinal: usize, query: &mut dyn FnMut(&image::ImageSource)) -> bool {
        let Some(source) = self.image_sources.get(ordinal).and_then(Option::as_ref) else {
            return false;
        };
        let frame = self.scene_frame.unwrap_or(self.step_id as u32);
        if !source.borrow_mut().query(frame) {
            return false;
        }
        if source.borrow().requested || source.borrow().state == image::SampleState::Pending {
            query(source);
        }
        !source.borrow().cells.is_empty()
    }

    fn emit(
        &mut self,
        system: &ParticleSystem,
        emitter_index: Option<usize>,
        origin: SpawnOrigin,
        permits: &mut [Option<bool>],
        query: &mut dyn FnMut(&image::ImageSource),
    ) {
        let random = self.rng.unit();
        let mut particle = Particle {
            id: self.next_id,
            pos: [0.0, 0.0, 0.0],
            vel: [0.0, 0.0, 0.0],
            size: 1.0,
            base_size: 1.0,
            color: system.base_color,
            base_color: system.base_color,
            alpha: 1.0,
            base_alpha: 1.0,
            angle: 0.0,
            angle_xy: [0.0; 2],
            angular: 0.0,
            angular_xy: [0.0; 2],
            age: 0.0,
            lifetime: 1.0,
            phase: random * std::f32::consts::TAU,
            noise_seed: random.to_bits(),
            slot: 0,
        };
        self.next_id = self.next_id.wrapping_add(1);
        let index =
            emitter_index.unwrap_or_else(|| (self.rng.next_u32() as usize) % system.emitters.len());
        let emitter = &system.emitters[index];
        let sampled = if let Emitter::Image { image, .. } = emitter {
            let allowed =
                *permits[index].get_or_insert_with(|| self.image_allowed(image.ordinal, query));
            if !allowed {
                return;
            }
            let Some(source) = self.image_sources.get(image.ordinal).and_then(Option::as_ref)
            else {
                return;
            };
            let source = source.borrow();
            let Some(birth) = image.birth(&source, &mut self.rng) else {
                return;
            };
            let transform = ParticleTransform::for_system(system);
            if transform.scale.iter().any(|scale| !scale.is_finite() || *scale == 0.0) {
                return;
            }
            let position = transform.inverse_point(birth.position);
            let velocity = transform.inverse_vector(birth.velocity);
            if position.iter().chain(&velocity).any(|value| !value.is_finite()) {
                return;
            }
            particle.pos = position;
            particle.vel = velocity;
            for (channel, sampled) in particle.color.iter_mut().zip(birth.color) {
                *channel *= sampled;
            }
            true
        } else {
            let Some(position) = emitter.spawn(&mut self.rng) else {
                return;
            };
            particle.pos = position;
            false
        };
        let point = emitter.control_point();
        particle.slot = self.free_slot();
        if system.world && !sampled {
            let origin = emitter.origin();
            let offset = system
                .spawn_vector(point, std::array::from_fn(|axis| particle.pos[axis] - origin[axis]));
            particle.pos = std::array::from_fn(|axis| origin[axis] + offset[axis]);
        }
        if let Emitter::Sphere { origin, shape, .. } = emitter {
            let offset = std::array::from_fn(|axis| particle.pos[axis] - origin[axis]);
            if let Some(velocity) =
                shape.launch(offset, &mut self.rng, |vector| system.spawn_vector(point, vector))
            {
                particle.vel = velocity;
            }
        }
        let control = point.or_else(|| (system.control_points[0].flags & 1 != 0).then_some(0));
        if let Some(index) = control.filter(|index| !sampled && *index < CONTROL_POINT_COUNT) {
            let offset = system.control_offset(index, self.control_points[index]);
            for (value, offset) in particle.pos.iter_mut().zip(offset) {
                *value += offset;
            }
        }
        if self.sequence_phases.len() != system.initializers.len() {
            self.sequence_phases = system
                .initializers
                .iter()
                .map(|init| match init {
                    Initializer::MapSequence { count, .. } => {
                        SequencePhase { phase: 0.0, step: 1.0 / *count }
                    }
                    Initializer::BetweenSequence(sequence) => {
                        SequencePhase { phase: 0.0, step: sequence.step }
                    }
                    _ => SequencePhase::default(),
                })
                .collect();
        }
        for (init_index, init) in system.initializers.iter().enumerate() {
            match init {
                Initializer::Lifetime { min, max, exponent } => {
                    particle.lifetime =
                        self.rng.range_biased(*min, *max, *exponent).max(MIN_PARTICLE_LIFETIME);
                }
                Initializer::Size { min, max, exponent } => {
                    let size_scale = match origin {
                        SpawnOrigin::Automatic => system.size_scale,
                        SpawnOrigin::Manual => system.authored_size_scale,
                    };
                    particle.base_size *= self.rng.range_biased(*min, *max, *exponent) * size_scale;
                    particle.size = particle.base_size;
                }
                Initializer::Alpha { min, max, exponent } => {
                    particle.base_alpha =
                        self.rng.range_biased(*min, *max, *exponent).clamp(0.0, 1.0);
                    particle.alpha = particle.base_alpha;
                }
                Initializer::Color { min, max, exponent } => {
                    let t = self.rng.biased(*exponent);
                    for axis in 0..COLOR_CHANNELS {
                        particle.color[axis] *=
                            (min[axis] + (max[axis] - min[axis]) * t) / f32::from(u8::MAX);
                    }
                }
                Initializer::HsvColor(hsv) => {
                    let hue = hsv.hue_min + self.rng.hue_index(hsv.hue_steps) as f32 * hsv.hue_step;
                    let saturation = self.rng.range(hsv.saturation.0, hsv.saturation.1);
                    let value = self.rng.range(hsv.value.0, hsv.value.1);
                    let color = hsv_to_rgb([hue, saturation, value]);
                    for (channel, sampled) in particle.color.iter_mut().zip(color) {
                        *channel *= sampled;
                    }
                }
                Initializer::ColorList(list) => {
                    let hsv = list.colors[self.rng.index(list.colors.len() as u32 - 1)];
                    let mut sampled = std::array::from_fn(|axis| {
                        let min = (hsv[axis] - list.noise[axis]).max(0.0);
                        let max = (hsv[axis] + list.noise[axis]).min(1.0);
                        self.rng.range(min, max)
                    });
                    sampled[0] -= sampled[0].floor();
                    sampled[1] = sampled[1].clamp(0.0, 1.0);
                    sampled[2] = sampled[2].clamp(0.0, 1.0);
                    let color = hsv_to_rgb(sampled);
                    for (channel, sampled) in particle.color.iter_mut().zip(color) {
                        *channel *= sampled;
                    }
                }
                Initializer::Remap(remap) => {
                    let slot = particle.slot as usize;
                    let points = self.control_motion(system, 0.0).map(Cell::new);
                    let written = Cell::new(0);
                    remap.apply(
                        &mut particle,
                        &remap::Frame {
                            system,
                            clock: self.clock,
                            system_time: self.time,
                            layer_time: self.layer_time,
                            points: &points,
                            written: &written,
                            step: 0,
                            substeps: 1,
                            particle: slot,
                        },
                    );
                    self.keep_written_points(system, written.get(), &points);
                }
                Initializer::InheritVelocity(inherit) => {
                    let factor = self.rng.range(inherit.min, inherit.max);
                    for (velocity, control) in
                        particle.vel.iter_mut().zip(self.control_velocity[inherit.point])
                    {
                        *velocity += control * factor;
                    }
                }
                Initializer::Velocity { min, max, exponent } => {
                    let vel = self.rng.range3_biased(*min, *max, *exponent);
                    let vel = system.spawn_vector(point, vel);
                    for (velocity, add) in particle.vel.iter_mut().zip(vel) {
                        *velocity += add;
                    }
                }
                Initializer::BoxPosition { min, max } => {
                    particle.pos = self.rng.range3(*min, *max);
                }
                Initializer::SpherePosition { min, max } => {
                    let radius = self.rng.unit().cbrt();
                    let z = self.rng.range(-1.0, 1.0);
                    let angle = self.rng.range(0.0, std::f32::consts::TAU);
                    let planar = (1.0 - z * z).sqrt();
                    let distance = min + (max - min) * radius;
                    particle.pos = [
                        planar * angle.cos() * distance,
                        planar * angle.sin() * distance,
                        z * distance,
                    ];
                }
                Initializer::TurbulentVelocity(turbulent) => {
                    let phase = self.rng.range(turbulent.phase.0, turbulent.phase.1);
                    let dir =
                        turbulent_direction((self.time + phase) * turbulent.time_scale, turbulent);
                    let speed = self.rng.range(turbulent.speed.0, turbulent.speed.1);
                    let push = dir.map(|value| value * speed);
                    let push = system.spawn_vector(point, push);
                    for (velocity, add) in particle.vel.iter_mut().zip(push) {
                        *velocity += add;
                    }
                }
                Initializer::MapSequence {
                    control_point, bounds, mirror, axis, min, max, ..
                } => {
                    let state = &mut self.sequence_phases[init_index];
                    let angle =
                        (bounds[0] + state.phase * (bounds[1] - bounds[0])) * std::f32::consts::TAU;
                    state.advance(*mirror);
                    let point = system.control_offset(
                        *control_point,
                        self.control_points.get(*control_point).copied().unwrap_or([0.0; 3]),
                    );
                    let frame = |vector| system.spawn_vector(Some(*control_point), vector);
                    let (axis, tangent_basis, normal_basis) = map_basis(*axis);
                    let (axis, tangent_basis, normal_basis) =
                        (frame(axis), frame(tangent_basis), frame(normal_basis));
                    let relative = std::array::from_fn(|index| particle.pos[index] - point[index]);
                    let axial = dot3(relative, axis);
                    let radial = std::array::from_fn(|index| relative[index] - axis[index] * axial);
                    let radius = dot3(radial, radial).sqrt();
                    let speed = self.rng.range3(*min, *max);
                    let (sin, cos) = angle.sin_cos();
                    for index in 0..3 {
                        let normal = sin * tangent_basis[index] + cos * normal_basis[index];
                        let tangent = cos * tangent_basis[index] - sin * normal_basis[index];
                        particle.pos[index] = point[index] + axis[index] * axial + normal * radius;
                        particle.vel[index] +=
                            tangent * speed[0] + normal * speed[1] + axis[index] * speed[2];
                    }
                }
                Initializer::InheritEvent(inherit) => {
                    if let Some(event) = self.event {
                        inherit.apply(&mut particle, event);
                    }
                }
                Initializer::PositionOffset(offset) => offset.apply(&mut particle, self.time),
                Initializer::BetweenSequence(sequence) => sequence.apply(
                    &mut particle,
                    &mut self.sequence_phases[init_index],
                    &self.control_points,
                    system.world,
                ),
                Initializer::Rotation { min, max } => {
                    let [x, y, z] = self.rng.range3(*min, *max);
                    particle.angle_xy = [x, y];
                    particle.angle = z;
                }
                Initializer::AngularVelocity { min, max } => {
                    let [x, y, z] = self.rng.range3(*min, *max);
                    particle.angular_xy = [x, y];
                    particle.angular = z;
                }
            }
        }
        for (pos, offset) in particle.pos.iter_mut().zip(self.emission_offset) {
            *pos += offset;
        }
        particle.base_color = particle.color;
        particle.lifetime =
            (particle.lifetime * system.instance.lifetime).max(MIN_PARTICLE_LIFETIME);
        for velocity in &mut particle.vel {
            *velocity *= system.instance.speed;
        }
        if system.instance.color != [1.0; COLOR_CHANNELS] {
            for (channel, factor) in particle.color.iter_mut().zip(system.instance.color) {
                *channel *= factor;
            }
        }
        if self.ribbon {
            let capacity = system.ribbon_trail().segments + 1;
            self.birth_history.push(TrailHistory::new(&particle, self.time, capacity));
        }
        self.births.push(particle);
    }

    pub fn step(&mut self, system: &ParticleSystem, dt: f32) {
        self.step_with_audio(system, dt, &[], &[], &mut |_| {});
    }

    pub fn step_with_audio(
        &mut self,
        system: &ParticleSystem,
        dt: f32,
        left: &[f32],
        right: &[f32],
        query: &mut dyn FnMut(&image::ImageSource),
    ) {
        if self.playback == PlaybackState::Stopped {
            return;
        }
        let frame_dt = dt.clamp(0.0, 0.25);
        let drag_scale = drag_scale(frame_dt);
        let substeps = if frame_dt >= 0.05 { 2 } else { 1 };
        let dt = if self.prewarming { frame_dt } else { frame_dt * system.rate_scale };
        if dt == 0.0
            && self.burst_done
            && (self.history_remaining != 0.0 || self.history.is_empty())
        {
            return;
        }
        self.retired.clear();
        self.step_id = self.step_id.wrapping_add(1);
        self.ribbon = system.renderer == Renderer::Ribbon
            || system.extra_renderers.iter().any(|render| render.renderer == Renderer::Ribbon);
        if dt > 0.0 {
            self.retire_existing(system, dt);
        }
        self.update_control_points(system, frame_dt);
        self.time += dt;
        if !self.prewarming && self.layer_visible {
            self.layer_time += dt;
        }
        self.begin_births();
        if !self.paused && !self.manual_only && !self.parent_hidden {
            let capacity = Self::capacity(system);
            if system.emitters.len() > 1
                || system.emitters.iter().any(|emitter| {
                    let timing = emitter.timing();
                    matches!(emitter, Emitter::Image { .. })
                        || timing.delay != 0.0
                        || timing.duration != 0.0
                        || timing.one_per_frame
                        || timing.periodic.is_some()
                })
                || !self.emitter_timers.is_empty()
            {
                self.burst_done = true;
                if self.emitting {
                    self.emit_timed(system, dt, capacity, left, right, query);
                }
            } else {
                if !self.burst_done {
                    self.burst_done = true;
                    self.pending *= system.count_scale;
                    let burst = system.emitters.iter().map(Emitter::instantaneous).sum::<u32>();
                    if burst > 0 || system.emitters.iter().all(|emitter| emitter.audio().is_some())
                    {
                        self.pending = 0.0;
                    }
                    let mut permits = vec![None; system.emitters.len()];
                    for _ in 0..burst {
                        if self.population() >= capacity {
                            break;
                        }
                        self.emit(system, None, SpawnOrigin::Automatic, &mut permits, query);
                    }
                }
                let rate: f32 = if self.emitting {
                    system.emitters.iter().map(|emitter| emitter.active_rate(left, right)).sum()
                } else {
                    0.0
                };
                self.pending += rate * system.count_scale * dt;
                let mut permits = vec![None; system.emitters.len()];
                while self.pending >= 1.0 {
                    self.pending -= 1.0;
                    if self.population() < capacity {
                        let audio_emitters =
                            system.emitters.iter().any(|emitter| emitter.audio().is_some());
                        let emitter_index = if audio_emitters {
                            let mut choice = self.rng.unit() * rate;
                            system.emitters.iter().position(|emitter| {
                                choice -= emitter.active_rate(left, right);
                                choice < 0.0
                            })
                        } else {
                            None
                        };
                        if !audio_emitters || emitter_index.is_some() {
                            self.emit(
                                system,
                                emitter_index,
                                SpawnOrigin::Automatic,
                                &mut permits,
                                query,
                            );
                        }
                    }
                }
            }
            if !self.has_emission(system) {
                self.emitting = false;
            }
        }
        self.settle_births();
        let control_motion = self.control_motion(system, frame_dt).map(Cell::new);
        let written_points = Cell::new(0);
        if model_collision::prepare(
            system,
            &mut self.model_capsules,
            &mut self.model_pose,
            &mut self.model_invalid,
        ) {
            tracing::warn!("particles: collisionmodel transform overflow unsupported");
        }
        self.gains.clear();
        self.gains.extend(system.operators.iter().map(|operator| operator.gain(left, right)));
        let context = operators::Step {
            model_capsules: &self.model_capsules,
            system,
            event: self.live_event,
            control_points: &self.control_points,
            control_motion: &control_motion,
            written_points: &written_points,
            clock: self.clock,
            layer_time: self.layer_time,
            gains: &self.gains,
            time: self.time,
            dt: dt / substeps as f32,
            drag_scale,
            substeps,
            world_turn: system.world.then(|| system.angle.sin_cos()),
        };
        if system.operators.iter().any(|operator| matches!(operator, Operator::Boids(_))) {
            let pool = self.boids.get_or_insert_with(|| Box::new(boids::Pool::new()));
            pool.sync(&self.particles, dt);
            pool.step(
                &context,
                self.scene_frame.unwrap_or(self.step_id as u32),
                &mut self.particles,
            );
        } else {
            for particle in &mut self.particles {
                let previous_position = particle.pos;
                let slot = particle.slot as usize;
                for step in 0..substeps {
                    particle.alpha = particle.base_alpha;
                    particle.size = particle.base_size;
                    particle.color = particle.base_color;
                    for operator in 0..system.operators.len() {
                        context.apply(particle, operator, previous_position, step, slot);
                    }
                }
                particle.alpha = (particle.alpha * system.alpha).clamp(0.0, 1.0);
            }
        }
        self.keep_written_points(system, written_points.get(), &control_motion);
        if system.instance.color != [1.0; COLOR_CHANNELS] {
            for particle in &mut self.particles {
                for (channel, factor) in particle.color.iter_mut().zip(system.instance.color) {
                    *channel *= factor;
                }
            }
        }
        if self.ribbon && !self.prewarming {
            let length = system.ribbon_trail().length;
            if let Some(history) = self.history.first() {
                let interval = length.max(MIN_TRAIL_LENGTH) / (history.capacity - 1) as f32;
                self.history_remaining -= dt;
                let sample = self.history_remaining <= 0.0;
                if sample {
                    self.history_remaining = interval;
                }
                let elapsed = interval - self.history_remaining;
                for (trail, particle) in self.history.iter_mut().zip(self.particles.iter()) {
                    trail.record_frame(particle, self.time, elapsed / interval, sample);
                }
            }
            if dt == 0.0 {
                let mut particles = self.particles.iter();
                self.history.retain(|_| {
                    particles.next().is_some_and(|particle| particle.age < particle.lifetime)
                });
            }
        }
        if dt == 0.0 {
            self.particles.retain(|particle| particle.age < particle.lifetime);
        }
    }
}

#[cfg(test)]
#[path = "particles_tests.rs"]
mod tests;
