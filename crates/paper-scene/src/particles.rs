use crate::pkg::Package;
use serde_json::Value;
use std::collections::BTreeMap;

const MAX_PARTICLES: usize = 32_768;
pub const TRAIL_POINTS: usize = 12;
const MAX_PREWARM_STEPS: usize = 600;

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

fn control_points(doc: &Value) -> [ControlPoint; 8] {
    let mut points = [ControlPoint::default(); 8];
    for entry in doc.get("controlpoint").and_then(Value::as_array).into_iter().flatten() {
        if let Some(point) =
            entry.get("id").and_then(Value::as_u64).and_then(|id| points.get_mut(id as usize))
        {
            point.flags = num_of(entry.get("flags"), 0.0) as u32;
            point.offset = vec3_of(entry.get("offset"), [0.0; 3]);
        }
    }
    points
}

pub enum Emitter {
    Sphere {
        control_point: Option<usize>,
        origin: [f32; 3],
        directions: [f32; 3],
        sign: [f32; 3],
        min: f32,
        max: f32,
        rate: f32,
        instantaneous: u32,
    },
    Box {
        control_point: Option<usize>,
        origin: [f32; 3],
        extent: [f32; 3],
        rate: f32,
        instantaneous: u32,
    },
}

impl Emitter {
    fn control_point(&self) -> Option<usize> {
        match self {
            Self::Sphere { control_point, .. } | Self::Box { control_point, .. } => *control_point,
        }
    }

    fn rate(&self) -> f32 {
        match self {
            Self::Sphere { rate, .. } | Self::Box { rate, .. } => *rate,
        }
    }

    fn origin(&self) -> [f32; 3] {
        match self {
            Self::Sphere { origin, .. } | Self::Box { origin, .. } => *origin,
        }
    }

    fn instantaneous(&self) -> u32 {
        match self {
            Self::Sphere { instantaneous, .. } | Self::Box { instantaneous, .. } => *instantaneous,
        }
    }

    fn spawn(&self, rng: &mut Rng) -> [f32; 3] {
        match self {
            Self::Sphere { origin, directions, sign, min, max, .. } => {
                let theta = rng.range(0.0, std::f32::consts::TAU);
                let z = rng.range(-1.0, 1.0);
                let radial = (1.0 - z * z).max(0.0).sqrt();
                let depth = rng.unit().cbrt();
                let mut dir = [
                    radial * theta.cos() * depth * directions[0],
                    radial * theta.sin() * depth * directions[1],
                    z * depth * directions[2],
                ];
                let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
                let distance = min + (max - min) * len.min(1.0);
                if len > 1e-6 {
                    for (axis, value) in dir.iter_mut().enumerate() {
                        *value /= len;
                        if sign[axis] > 0.0 {
                            *value = value.abs();
                        } else if sign[axis] < 0.0 {
                            *value = -value.abs();
                        }
                    }
                }
                [
                    origin[0] + dir[0] * distance,
                    origin[1] + dir[1] * distance,
                    origin[2] + dir[2] * distance,
                ]
            }
            Self::Box { origin, extent, .. } => [
                origin[0] + rng.range(-extent[0], extent[0]),
                origin[1] + rng.range(-extent[1], extent[1]),
                origin[2] + rng.range(-extent[2], extent[2]),
            ],
        }
    }
}

pub enum Initializer {
    Lifetime { min: f32, max: f32, exponent: f32 },
    Size { min: f32, max: f32, exponent: f32 },
    Color { min: [f32; 3], max: [f32; 3], exponent: f32 },
    Velocity { min: [f32; 3], max: [f32; 3], exponent: f32 },
    Rotation { min: [f32; 3], max: [f32; 3] },
    Alpha { min: f32, max: f32, exponent: f32 },
    AngularVelocity { min: [f32; 3], max: [f32; 3] },
    TurbulentVelocity { min: f32, max: f32 },
}

pub struct Oscillator {
    pub frequency: (f32, f32),
    pub scale: (f32, f32),
    pub phase: (f32, f32),
    pub mask: [f32; 3],
}

pub enum Operator {
    Movement { gravity: [f32; 3], drag: f32 },
    AlphaFade { fade_in: f32, fade_out: f32 },
    SizeChange { start: f32, end: f32, start_time: f32, end_time: f32 },
    AlphaChange { start: f32, end: f32, start_time: f32, end_time: f32 },
    ColorChange { start: [f32; 3], end: [f32; 3], start_time: f32, end_time: f32 },
    OscillateAlpha(Oscillator),
    OscillatePosition(Oscillator),
    OscillateSize(Oscillator),
    AngularMovement { force: f32, drag: f32 },
    Turbulence { scale: f32, speed: (f32, f32), time_scale: f32, mask: [f32; 3] },
    ControlPointAttract { control_point: usize, origin: [f32; 3], scale: f32, threshold: f32 },
}

fn oscillator(entry: &Value, scale_fallback: f32) -> Oscillator {
    let frequency_min = num_of(entry.get("frequencymin"), 1.0);
    let scale_min = num_of(entry.get("scalemin"), scale_fallback);
    let phase_min = num_of(entry.get("phasemin"), 0.0);
    Oscillator {
        frequency: (frequency_min, num_of(entry.get("frequencymax"), frequency_min)),
        scale: (scale_min, num_of(entry.get("scalemax"), scale_min)),
        phase: (phase_min, num_of(entry.get("phasemax"), phase_min)),
        mask: vec3_of(entry.get("mask"), [1.0, 1.0, 1.0]),
    }
}

impl Oscillator {
    fn params(&self, seed: f32) -> (f32, f32, f32) {
        (
            self.frequency.0 + (self.frequency.1 - self.frequency.0) * seed,
            self.phase.0 + (self.phase.1 - self.phase.0) * seed,
            self.scale.0 + (self.scale.1 - self.scale.0) * seed,
        )
    }

    fn wave(&self, age: f32, seed: f32) -> f32 {
        let (frequency, phase, _) = self.params(seed);
        (age * frequency + phase).sin()
    }

    fn velocity(&self, age: f32, seed: f32) -> f32 {
        let (frequency, phase, amplitude) = self.params(seed);
        amplitude * frequency * (age * frequency + phase).cos()
    }

    fn amplitude(&self, seed: f32) -> f32 {
        self.params(seed).2
    }
}

fn flow(pos: [f32; 3], scale: f32, time: f32) -> (f32, f32) {
    let (x, y) = (pos[0] * scale, pos[1] * scale);
    let a = (x + time).sin() + (y * 1.3 - time * 0.7).cos();
    let b = (y + time * 1.1).sin() + (x * 0.9 + time * 0.6).cos();
    let len = a.hypot(b);
    if len < 1.0e-5 { (0.0, 0.0) } else { (a / len, b / len) }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Renderer {
    Sprite,
    Trail,
    Ribbon,
}

#[derive(Clone, Copy)]
pub struct Trail {
    pub length: f32,
    pub min_length: f32,
    pub max_length: f32,
    pub fade_alpha: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Animation {
    Sequence,
    RandomFrame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleBlend {
    Additive,
    Translucent,
    Normal,
}

pub struct ParticleSystem {
    pub renderer: Renderer,
    pub trail: Trail,
    pub animation: Animation,
    pub sequence_multiplier: f32,
    pub max_count: usize,
    pub start_time: f32,
    pub control_points: [ControlPoint; 8],
    pub emitters: Vec<Emitter>,
    pub initializers: Vec<Initializer>,
    pub operators: Vec<Operator>,
    pub texture: Option<crate::model::Texture>,
    pub additive: bool,
    pub blend: ParticleBlend,
    pub perspective: bool,
    pub pass: Option<crate::effects::EffectPass>,
    pub origin: (f32, f32, f32),
    pub angle: f32,
    pub scale: f32,
    pub scale3: [f32; 3],
    pub tint: [f32; 3],
    pub alpha: f32,
    pub rate_scale: f32,
    pub size_scale: f32,
    pub grab_slot: Option<usize>,
    pub world: bool,
}

impl ParticleSystem {
    #[must_use]
    pub fn follows_mouse(&self) -> bool {
        self.control_points.iter().any(|point| point.flags & 1 != 0)
    }

    #[must_use]
    pub fn draw_scale(&self) -> f32 {
        if self.world { 1.0 } else { self.scale }
    }

    #[must_use]
    pub fn draw_scale3(&self) -> [f32; 3] {
        if self.world { [1.0; 3] } else { self.scale3 }
    }
}

pub const SPRITE_FLOATS_PER_VERTEX: usize = 17;
pub const SPRITE_VERTICES: usize = 4;
pub const SPRITE_EXTENT: f32 = 0.5;
pub const SPRITE_INDICES: usize = 6;

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
    for particle in &sim.particles {
        if particle.alpha <= 0.002 || particle.size <= 0.01 {
            continue;
        }
        let life = if frames > 1 {
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
            }
        } else {
            (particle.age / particle.lifetime.max(1e-6)).clamp(0.0, 1.0)
        };
        for (u, v) in [(0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)] {
            out.extend_from_slice(&[
                particle.pos[0],
                particle.pos[1],
                particle.pos[2],
                u,
                v,
                particle.angle,
                particle.size * SPRITE_EXTENT,
                particle.color[0] * system.tint[0],
                particle.color[1] * system.tint[1],
                particle.color[2] * system.tint[2],
                particle.alpha,
                particle.vel[0],
                particle.vel[1],
                particle.vel[2],
                life,
                0.0,
                0.0,
            ]);
        }
        count += 1;
    }
    count
}

#[derive(Clone, Copy)]
pub struct Particle {
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub size: f32,
    pub base_size: f32,
    pub color: [f32; 3],
    pub alpha: f32,
    pub base_alpha: f32,
    pub angle: f32,
    pub angular: f32,
    pub age: f32,
    pub lifetime: f32,
    pub phase: f32,
}

fn parse_emitter(entry: &Value) -> Option<Emitter> {
    let name = entry.get("name").and_then(Value::as_str)?.to_ascii_lowercase();
    let origin = vec3_of(entry.get("origin"), [0.0, 0.0, 0.0]);
    let rate = num_of(entry.get("rate"), 10.0).clamp(0.0, 1_000_000.0);
    let instantaneous = num_of(entry.get("instantaneous"), 0.0).clamp(0.0, 100_000.0) as u32;
    match name.as_str() {
        "sphererandom" => Some(Emitter::Sphere {
            control_point: entry
                .get("controlpoint")
                .and_then(Value::as_u64)
                .filter(|id| *id < 8)
                .map(|id| id as usize),
            origin,
            directions: vec3_of(entry.get("directions"), [1.0, 1.0, 1.0]),
            sign: vec3_of(entry.get("sign"), [0.0, 0.0, 0.0]),
            min: num_of(entry.get("distancemin"), 0.0),
            max: num_of(entry.get("distancemax"), 0.0),
            rate,
            instantaneous,
        }),
        "boxrandom" => Some(Emitter::Box {
            control_point: entry
                .get("controlpoint")
                .and_then(Value::as_u64)
                .filter(|id| *id < 8)
                .map(|id| id as usize),
            origin,
            extent: vec3_of(entry.get("distancemax"), [0.0, 0.0, 0.0]),
            rate,
            instantaneous,
        }),
        _ => None,
    }
}

fn parse_initializer(entry: &Value) -> Option<Initializer> {
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
        "colorrandom" => Some(Initializer::Color {
            min: vec3_of(min, [255.0, 255.0, 255.0]),
            max: vec3_of(max, [255.0, 255.0, 255.0]),
            exponent,
        }),
        "velocityrandom" => Some(Initializer::Velocity {
            min: vec3_of(min, [0.0, 0.0, 0.0]),
            max: vec3_of(max, [0.0, 0.0, 0.0]),
            exponent,
        }),
        "rotationrandom" => Some(Initializer::Rotation {
            min: vec3_of(min, [0.0, 0.0, 0.0]),
            max: vec3_of(max, [0.0, 0.0, 0.0]),
        }),
        "angularvelocityrandom" => Some(Initializer::AngularVelocity {
            min: vec3_of(min, [0.0, 0.0, 0.0]),
            max: vec3_of(max, [0.0, 0.0, 0.0]),
        }),
        "turbulentvelocityrandom" => {
            Some(Initializer::TurbulentVelocity { min: num_of(min, 0.0), max: num_of(max, 0.0) })
        }
        _ => None,
    }
}

fn parse_operator(entry: &Value) -> Option<Operator> {
    let name = entry.get("name").and_then(Value::as_str)?.to_ascii_lowercase();
    match name.as_str() {
        "movement" => Some(Operator::Movement {
            gravity: vec3_of(entry.get("gravity"), [0.0, 0.0, 0.0]),
            drag: num_of(entry.get("drag"), 0.0),
        }),
        "alphafade" => Some(Operator::AlphaFade {
            fade_in: num_of(entry.get("fadeintime"), 0.1).max(0.0),
            fade_out: num_of(entry.get("fadeouttime"), 0.9).max(0.0),
        }),
        "sizechange" => Some(Operator::SizeChange {
            start: num_of(entry.get("startvalue"), 1.0),
            end: num_of(entry.get("endvalue"), 1.0),
            start_time: num_of(entry.get("starttime"), 0.0),
            end_time: num_of(entry.get("endtime"), 1.0),
        }),
        "alphachange" => Some(Operator::AlphaChange {
            start: num_of(entry.get("startvalue"), 0.0),
            end: num_of(entry.get("endvalue"), 1.0),
            start_time: num_of(entry.get("starttime"), 0.0),
            end_time: num_of(entry.get("endtime"), 1.0),
        }),
        "colorchange" => Some(Operator::ColorChange {
            start: vec3_of(entry.get("startvalue"), [1.0, 1.0, 1.0]),
            end: vec3_of(entry.get("endvalue"), [1.0, 1.0, 1.0]),
            start_time: num_of(entry.get("starttime"), 0.0),
            end_time: num_of(entry.get("endtime"), 1.0),
        }),
        "oscillatealpha" => Some(Operator::OscillateAlpha(oscillator(entry, 0.5))),
        "oscillateposition" => Some(Operator::OscillatePosition(oscillator(entry, 0.0))),
        "oscillatesize" => Some(Operator::OscillateSize(oscillator(entry, 1.0))),
        "angularmovement" => Some(Operator::AngularMovement {
            force: vec3_of(entry.get("force"), [0.0, 0.0, 0.0])[2],
            drag: num_of(entry.get("drag"), 0.0),
        }),
        "turbulence" => {
            let speed_min = num_of(entry.get("speedmin"), 0.0);
            Some(Operator::Turbulence {
                scale: num_of(entry.get("scale"), 0.01),
                speed: (speed_min, num_of(entry.get("speedmax"), speed_min)),
                time_scale: num_of(entry.get("timescale"), 1.0),
                mask: vec3_of(entry.get("mask"), [1.0, 1.0, 1.0]),
            })
        }
        "controlpointattract" => Some(Operator::ControlPointAttract {
            control_point: num_of(entry.get("controlpoint"), 0.0).clamp(0.0, 7.0) as usize,
            origin: vec3_of(entry.get("origin"), [0.0, 0.0, 0.0]),
            scale: num_of(entry.get("scale"), 0.0),
            threshold: num_of(entry.get("threshold"), 0.0).max(1.0),
        }),
        _ => None,
    }
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
        match self {
            Self::Pkg(pkg, assets) => crate::model::load_texture_named(pkg, name).or_else(|| {
                let trimmed = name.trim_end_matches(".tex");
                for candidate in [format!("materials/{trimmed}.tex"), format!("{trimmed}.tex")] {
                    if let Some(bytes) = assets.read_bytes(&candidate)
                        && let Some(texture) = crate::model::load_texture_bytes(&bytes)
                    {
                        return Some(texture);
                    }
                }
                None
            }),
            Self::Dir(root) => {
                let trimmed = name.trim_end_matches(".tex");
                for candidate in [format!("materials/{trimmed}.tex"), format!("{trimmed}.tex")] {
                    if let Some(bytes) = crate::effects::read_confined_bytes(
                        root,
                        &candidate,
                        crate::pkg::MAX_PACKAGE_ENTRY_BYTES,
                    ) && let Some(texture) = crate::model::load_texture_bytes(&bytes)
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
    renderer: Renderer,
    texture: Option<&crate::model::Texture>,
) -> Option<crate::effects::EffectPass> {
    let material = material?;
    let frames = texture.map_or(0, |texture| texture.frames.len());
    let mut combos = std::collections::BTreeMap::new();
    if let Some(texture) = texture {
        combos.insert("TEX0FORMAT".to_string(), texture.we_format());
    }
    combos.insert("THICKFORMAT".to_string(), 1i64);
    combos.insert("LIGHTING".to_string(), 0);
    combos.insert("SPRITESHEET".to_string(), i64::from(frames > 1));
    combos.insert("TRAILRENDERER".to_string(), i64::from(renderer == Renderer::Trail));
    crate::effects::particle_pass(pkg, assets, material, &combos)
}

pub fn load(
    pkg: &Package,
    assets: &crate::effects::Assets,
    object: &Value,
    path: &str,
) -> Option<ParticleSystem> {
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
    let entry = doc.get("renderer").and_then(Value::as_array).and_then(|list| list.first());
    let name = entry
        .and_then(|entry| entry.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("sprite")
        .to_ascii_lowercase();
    let renderer = match name.as_str() {
        "sprite" => Renderer::Sprite,
        "spritetrail" => Renderer::Trail,
        "ropetrail" | "rope" => Renderer::Ribbon,
        _ => return None,
    };
    let trail = Trail {
        length: num_of(entry.and_then(|entry| entry.get("length")), 0.05).max(0.0),
        min_length: num_of(entry.and_then(|entry| entry.get("minlength")), 0.0).max(0.0),
        max_length: num_of(entry.and_then(|entry| entry.get("maxlength")), 100.0).max(0.0),
        fade_alpha: entry
            .and_then(|entry| entry.get("fadealpha"))
            .is_none_or(|value| num_of(Some(value), 1.0) != 0.0),
    };
    let emitters: Vec<Emitter> = doc
        .get("emitter")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(parse_emitter).collect())
        .unwrap_or_default();
    if emitters.is_empty() {
        return None;
    }
    let mut initializers: Vec<Initializer> = doc
        .get("initializer")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(parse_initializer).collect())
        .unwrap_or_default();
    let operators = doc
        .get("operator")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(parse_operator).collect())
        .unwrap_or_default();
    let over = object.get("instanceoverride");
    let props = &assets.properties;
    let tint = over
        .and_then(|value| value.get("colorn"))
        .and_then(|value| crate::effects::bound_value(value, props))
        .map_or([1.0, 1.0, 1.0], |parts| {
            let pick = |index: usize| parts.get(index).copied().unwrap_or_else(|| parts[0]);
            if parts.is_empty() { [1.0, 1.0, 1.0] } else { [pick(0), pick(1), pick(2)] }
        });
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
            Initializer::Velocity { min, max, .. } => {
                for axis in 0..3 {
                    min[axis] *= speed_scale;
                    max[axis] *= speed_scale;
                }
            }
            Initializer::TurbulentVelocity { min, max } => {
                *min *= speed_scale;
                *max *= speed_scale;
            }
            _ => {}
        }
    }
    let origin = vec3_of(object.get("origin"), [0.0, 0.0, 0.0]);
    let scale3 = vec3_of(object.get("scale"), [1.0, 1.0, 1.0]);
    let scale = scale3[0];
    let animation = match doc.get("animationmode").and_then(Value::as_str) {
        Some("randomframe") => Animation::RandomFrame,
        _ => Animation::Sequence,
    };
    let material = material_json(&source, &doc);
    let blend = material_blend(material.as_ref());
    let texture = sprite_texture(&source, &doc);
    let pass = if renderer == Renderer::Ribbon {
        None
    } else {
        engine_pass(pkg, assets, material.as_ref(), renderer, texture.as_ref())
    };
    let grab_slot = pass.as_ref().and_then(|pass| {
        pass.binds
            .iter()
            .find(|(_, bind)| *bind == crate::effects::EffectBind::SceneSoFar)
            .map(|(slot, _)| *slot)
    });
    Some(ParticleSystem {
        renderer,
        trail,
        animation,
        sequence_multiplier: num_of(doc.get("sequencemultiplier"), 1.0).clamp(0.01, 100.0),
        max_count: (num_of(doc.get("maxcount"), 100.0) * count_scale).round().clamp(1.0, 100_000.0)
            as usize,
        start_time: num_of(doc.get("starttime"), 0.0).clamp(0.0, 300.0),
        control_points: control_points(&doc),
        emitters,
        initializers,
        operators,
        texture,
        additive: blend == ParticleBlend::Additive,
        blend,
        perspective: (num_of(doc.get("flags"), 0.0) as i64) & 4 != 0,
        world: (num_of(doc.get("flags"), 0.0) as i64) & 1 != 0,
        pass,
        origin: (origin[0], origin[1], origin[2]),
        angle: vec3_of(object.get("angles"), [0.0, 0.0, 0.0])[2],
        scale: if scale.abs() < f32::EPSILON { 1.0 } else { scale },
        scale3: [
            if scale3[0].abs() < f32::EPSILON { 1.0 } else { scale3[0] },
            if scale3[1].abs() < f32::EPSILON { 1.0 } else { scale3[1] },
            if scale3[2].abs() < f32::EPSILON { 1.0 } else { scale3[2] },
        ],
        tint,
        alpha: over.map_or(1.0, |value| num_bound(value.get("alpha"), props, 1.0)).clamp(0.0, 1.0),
        rate_scale: over
            .map_or(1.0, |value| num_bound(value.get("rate"), props, 1.0))
            .clamp(0.0, 10.0),
        size_scale: over
            .map_or(1.0, |value| num_bound(value.get("size"), props, 1.0))
            .clamp(0.0, 10.0),
        grab_slot,
    })
}

pub struct Sim {
    pub particles: Vec<Particle>,
    control_points: [[f32; 3]; 8],
    pointer: Option<[f32; 2]>,
    burst_done: bool,
    pub history: Vec<[[f32; 2]; TRAIL_POINTS]>,
    rng: Rng,
    pending: f32,
    time: f32,
    ribbon: bool,
}

impl Sim {
    #[must_use]
    pub fn new(seed: u32) -> Self {
        Self {
            particles: Vec::new(),
            history: Vec::new(),
            rng: Rng::new(seed),
            pending: 1.0,
            control_points: [[0.0; 3]; 8],
            pointer: None,
            burst_done: false,
            time: 0.0,
            ribbon: false,
        }
    }

    pub fn set_pointer(&mut self, pointer: [f32; 2]) {
        self.pointer = Some(pointer);
    }

    fn update_control_points(&mut self, system: &ParticleSystem) {
        let (sin, cos) = system.angle.sin_cos();
        let scale = system.draw_scale3();
        for (position, point) in self.control_points.iter_mut().zip(&system.control_points) {
            *position = point.offset;
            if point.flags & 1 != 0 {
                let pointer = self.pointer.unwrap_or([system.origin.0, system.origin.1]);
                position[0] += pointer[0];
                position[1] += pointer[1];
            }
            if point.flags & 3 != 0 {
                let x = position[0] - system.origin.0;
                let y = position[1] - system.origin.1;
                *position = [
                    (x * cos + y * sin) / scale[0],
                    (-x * sin + y * cos) / scale[1],
                    (position[2] - system.origin.2) / scale[2],
                ];
            }
        }
    }

    #[must_use]
    pub fn capacity(system: &ParticleSystem) -> usize {
        system.max_count.min(MAX_PARTICLES)
    }

    pub fn prewarm(&mut self, system: &ParticleSystem, step: f32) {
        let steps = ((system.start_time / step.max(0.001)) as usize).min(MAX_PREWARM_STEPS);
        for _ in 0..steps {
            self.step(system, step);
        }
    }

    fn emit(&mut self, system: &ParticleSystem) {
        let mut particle = Particle {
            pos: [0.0, 0.0, 0.0],
            vel: [0.0, 0.0, 0.0],
            size: 10.0,
            base_size: 10.0,
            color: [1.0, 1.0, 1.0],
            alpha: 1.0,
            base_alpha: 1.0,
            angle: 0.0,
            angular: 0.0,
            age: 0.0,
            lifetime: 1.0,
            phase: self.rng.unit() * std::f32::consts::TAU,
        };
        let index = (self.rng.next_u32() as usize) % system.emitters.len();
        let emitter = &system.emitters[index];
        particle.pos = emitter.spawn(&mut self.rng);
        if system.world {
            let origin = emitter.origin();
            for ((pos, base), scale) in
                particle.pos.iter_mut().zip(origin.iter()).zip(system.scale3.iter())
            {
                *pos = base + (*pos - base) * scale;
            }
        }
        let control = emitter
            .control_point()
            .or_else(|| (system.control_points[0].flags & 1 != 0).then_some(0));
        if let Some(position) = control.and_then(|index| self.control_points.get(index)) {
            for (value, offset) in particle.pos.iter_mut().zip(position) {
                *value += offset;
            }
        }
        for init in &system.initializers {
            match init {
                Initializer::Lifetime { min, max, exponent } => {
                    particle.lifetime = self.rng.range_biased(*min, *max, *exponent).max(0.01);
                }
                Initializer::Size { min, max, exponent } => {
                    particle.base_size =
                        self.rng.range_biased(*min, *max, *exponent) * system.size_scale;
                    particle.size = particle.base_size;
                }
                Initializer::Alpha { min, max, exponent } => {
                    particle.base_alpha =
                        self.rng.range_biased(*min, *max, *exponent).clamp(0.0, 1.0);
                    particle.alpha = particle.base_alpha;
                }
                Initializer::Color { min, max, exponent } => {
                    let t = self.rng.biased(*exponent);
                    particle.color = [
                        (min[0] + (max[0] - min[0]) * t) / 255.0,
                        (min[1] + (max[1] - min[1]) * t) / 255.0,
                        (min[2] + (max[2] - min[2]) * t) / 255.0,
                    ];
                }
                Initializer::Velocity { min, max, exponent } => {
                    let vel = self.rng.range3_biased(*min, *max, *exponent);
                    let k = if system.world { system.scale3 } else { [1.0; 3] };
                    particle.vel = [
                        particle.vel[0] + vel[0] * k[0],
                        particle.vel[1] + vel[1] * k[1],
                        particle.vel[2] + vel[2] * k[2],
                    ];
                }
                Initializer::TurbulentVelocity { min, max } => {
                    let speed = self.rng.range(*min, *max);
                    let theta = self.rng.range(0.0, std::f32::consts::TAU);
                    particle.vel[0] += theta.cos() * speed;
                    particle.vel[1] += theta.sin() * speed;
                }
                Initializer::Rotation { min, max } => {
                    particle.angle = self.rng.range3(*min, *max)[2];
                }
                Initializer::AngularVelocity { min, max } => {
                    particle.angular = self.rng.range3(*min, *max)[2];
                }
            }
        }
        if self.ribbon {
            let point = [particle.pos[0], particle.pos[1]];
            self.history.push([point; TRAIL_POINTS]);
        }
        self.particles.push(particle);
    }

    pub fn step(&mut self, system: &ParticleSystem, dt: f32) {
        let dt = dt.clamp(0.0, 0.25);
        self.update_control_points(system);
        self.ribbon = system.renderer == Renderer::Ribbon;
        self.time += dt;
        let capacity = Self::capacity(system);
        if !self.burst_done {
            self.burst_done = true;
            let burst: u32 = system.emitters.iter().map(Emitter::instantaneous).sum();
            if burst > 0 {
                self.pending = 0.0;
            }
            for _ in 0..burst {
                if self.particles.len() >= capacity {
                    break;
                }
                self.emit(system);
            }
        }
        let rate: f32 = system.emitters.iter().map(Emitter::rate).sum::<f32>() * system.rate_scale;
        self.pending += rate * dt;
        while self.pending >= 1.0 {
            self.pending -= 1.0;
            if self.particles.len() < capacity {
                self.emit(system);
            }
        }
        let time = self.time;
        for particle in &mut self.particles {
            particle.age += dt;
            let life = (particle.age / particle.lifetime).clamp(0.0, 1.0);
            let seed = particle.phase / std::f32::consts::TAU;
            particle.alpha = particle.base_alpha;
            particle.size = particle.base_size;
            for op in &system.operators {
                match op {
                    Operator::Movement { gravity, drag } => {
                        let damp = (1.0 - drag * dt).clamp(0.0, 1.0);
                        for ((vel, pos), gravity) in
                            particle.vel.iter_mut().zip(particle.pos.iter_mut()).zip(gravity.iter())
                        {
                            *vel = *vel * damp + gravity * dt;
                            *pos += *vel * dt;
                        }
                    }
                    Operator::AlphaFade { fade_in, fade_out } => {
                        if *fade_in > 0.0 && life < *fade_in {
                            particle.alpha *= life / *fade_in;
                        }
                        if *fade_out < 1.0 && life > *fade_out {
                            let span = (1.0 - *fade_out).max(f32::EPSILON);
                            particle.alpha *= ((1.0 - life) / span).clamp(0.0, 1.0);
                        }
                    }
                    Operator::SizeChange { start, end, start_time, end_time } => {
                        let span = (*end_time - *start_time).max(f32::EPSILON);
                        let t = ((life - *start_time) / span).clamp(0.0, 1.0);
                        particle.size *= start + (end - start) * t;
                    }
                    Operator::AlphaChange { start, end, start_time, end_time } => {
                        let span = (*end_time - *start_time).max(f32::EPSILON);
                        let t = ((life - *start_time) / span).clamp(0.0, 1.0);
                        particle.alpha *= start + (end - start) * t;
                    }
                    Operator::ColorChange { start, end, start_time, end_time } => {
                        let span = (*end_time - *start_time).max(f32::EPSILON);
                        let t = ((life - *start_time) / span).clamp(0.0, 1.0);
                        for ((channel, start), end) in
                            particle.color.iter_mut().zip(start.iter()).zip(end.iter())
                        {
                            *channel = start + (end - start) * t;
                        }
                    }
                    Operator::OscillateAlpha(osc) => {
                        let wave = osc.wave(life, seed);
                        let amount = osc.amplitude(seed);
                        particle.alpha *= (1.0 - amount * 0.5 * (1.0 - wave)).clamp(0.0, 1.0);
                    }
                    Operator::OscillateSize(osc) => {
                        let wave = osc.wave(particle.age, seed);
                        let amount = osc.amplitude(seed);
                        particle.size *= (1.0 + amount * wave).max(0.0);
                    }
                    Operator::OscillatePosition(osc) => {
                        let velocity = osc.velocity(particle.age - dt, seed);
                        let k = if system.world { system.scale3 } else { [1.0; 3] };
                        for ((pos, mask), k) in
                            particle.pos.iter_mut().zip(osc.mask.iter()).zip(k.iter())
                        {
                            *pos += velocity * dt * mask * k;
                        }
                    }
                    Operator::AngularMovement { force, drag } => {
                        particle.angular += force * dt;
                        particle.angular *= (1.0 - drag * dt).clamp(0.0, 1.0);
                    }
                    Operator::Turbulence { scale, speed, time_scale, mask } => {
                        let (dx, dy) = flow(particle.pos, *scale, time * *time_scale);
                        let strength = speed.0 + (speed.1 - speed.0) * seed;
                        particle.pos[0] += dx * strength * mask[0] * dt;
                        particle.pos[1] += dy * strength * mask[1] * dt;
                    }
                    Operator::ControlPointAttract { control_point, origin, scale, threshold } => {
                        let point =
                            self.control_points.get(*control_point).copied().unwrap_or([0.0; 3]);
                        let dx = point[0] + origin[0] - particle.pos[0];
                        let dy = point[1] + origin[1] - particle.pos[1];
                        let distance = dx.hypot(dy);
                        if distance > 1.0e-4 {
                            let falloff = (1.0 - distance / threshold).clamp(0.0, 1.0);
                            let pull = scale * falloff * dt / distance;
                            particle.vel[0] += dx * pull;
                            particle.vel[1] += dy * pull;
                        }
                    }
                }
            }
            particle.angle += particle.angular * dt;
            particle.alpha = (particle.alpha * system.alpha).clamp(0.0, 1.0);
        }
        if self.ribbon {
            for (trail, particle) in self.history.iter_mut().zip(self.particles.iter()) {
                trail.rotate_right(1);
                trail[0] = [particle.pos[0], particle.pos[1]];
            }
            let mut cursor = self.particles.iter();
            let mut history = std::mem::take(&mut self.history);
            history
                .retain(|_| cursor.next().is_some_and(|particle| particle.age < particle.lifetime));
            self.history = history;
        }
        self.particles.retain(|particle| particle.age < particle.lifetime);
    }
}

#[cfg(test)]
#[path = "particles_tests.rs"]
mod tests;
