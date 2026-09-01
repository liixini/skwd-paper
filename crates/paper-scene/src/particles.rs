use crate::pkg::Package;
use serde_json::Value;

const MAX_PARTICLES: usize = 4096;
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

fn num_of(value: Option<&Value>, fallback: f32) -> f32 {
    match value {
        Some(Value::Number(num)) => num.as_f64().unwrap_or(f64::from(fallback)) as f32,
        Some(Value::String(text)) => text.parse().unwrap_or(fallback),
        _ => fallback,
    }
}

pub enum Emitter {
    Sphere { origin: [f32; 3], directions: [f32; 3], min: f32, max: f32, rate: f32 },
    Box { origin: [f32; 3], extent: [f32; 3], rate: f32 },
}

impl Emitter {
    fn rate(&self) -> f32 {
        match self {
            Self::Sphere { rate, .. } | Self::Box { rate, .. } => *rate,
        }
    }

    fn spawn(&self, rng: &mut Rng) -> [f32; 3] {
        match self {
            Self::Sphere { origin, directions, min, max, .. } => {
                let theta = rng.range(0.0, std::f32::consts::TAU);
                let z = rng.range(-1.0, 1.0);
                let radial = (1.0 - z * z).max(0.0).sqrt();
                let distance = rng.range(*min, *max);
                [
                    origin[0] + radial * theta.cos() * distance * directions[0],
                    origin[1] + radial * theta.sin() * distance * directions[1],
                    origin[2] + z * distance * directions[2],
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
    Lifetime { min: f32, max: f32 },
    Size { min: f32, max: f32 },
    Color { min: [f32; 3], max: [f32; 3] },
    Velocity { min: [f32; 3], max: [f32; 3] },
    Rotation { min: [f32; 3], max: [f32; 3] },
    Alpha { min: f32, max: f32 },
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
    ControlPointAttract { origin: [f32; 3], scale: f32, threshold: f32 },
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
    fn wave(&self, time: f32, seed: f32) -> f32 {
        let frequency = self.frequency.0 + (self.frequency.1 - self.frequency.0) * seed;
        let phase = self.phase.0 + (self.phase.1 - self.phase.0) * seed;
        (time * frequency + phase * std::f32::consts::TAU).sin()
    }

    fn amplitude(&self, seed: f32) -> f32 {
        self.scale.0 + (self.scale.1 - self.scale.0) * seed
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

pub struct ParticleSystem {
    pub renderer: Renderer,
    pub trail: Trail,
    pub animation: Animation,
    pub sequence_multiplier: f32,
    pub max_count: usize,
    pub start_time: f32,
    pub emitters: Vec<Emitter>,
    pub initializers: Vec<Initializer>,
    pub operators: Vec<Operator>,
    pub texture: Option<crate::model::Texture>,
    pub additive: bool,
    pub origin: (f32, f32, f32),
    pub scale: f32,
    pub tint: [f32; 3],
    pub alpha: f32,
    pub rate_scale: f32,
    pub size_scale: f32,
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
    let rate = num_of(entry.get("rate"), 10.0).clamp(0.0, 4000.0);
    match name.as_str() {
        "sphererandom" => Some(Emitter::Sphere {
            origin,
            directions: vec3_of(entry.get("directions"), [1.0, 1.0, 1.0]),
            min: num_of(entry.get("distancemin"), 0.0),
            max: num_of(entry.get("distancemax"), 0.0),
            rate,
        }),
        "boxrandom" => Some(Emitter::Box {
            origin,
            extent: vec3_of(entry.get("distancemax"), [0.0, 0.0, 0.0]),
            rate,
        }),
        _ => None,
    }
}

fn parse_initializer(entry: &Value) -> Option<Initializer> {
    let name = entry.get("name").and_then(Value::as_str)?.to_ascii_lowercase();
    let min = entry.get("min");
    let max = entry.get("max");
    match name.as_str() {
        "lifetimerandom" => {
            Some(Initializer::Lifetime { min: num_of(min, 1.0), max: num_of(max, 1.0) })
        }
        "sizerandom" => Some(Initializer::Size { min: num_of(min, 10.0), max: num_of(max, 10.0) }),
        "alpharandom" => Some(Initializer::Alpha { min: num_of(min, 1.0), max: num_of(max, 1.0) }),
        "colorrandom" => Some(Initializer::Color {
            min: vec3_of(min, [255.0, 255.0, 255.0]),
            max: vec3_of(max, [255.0, 255.0, 255.0]),
        }),
        "velocityrandom" => Some(Initializer::Velocity {
            min: vec3_of(min, [0.0, 0.0, 0.0]),
            max: vec3_of(max, [0.0, 0.0, 0.0]),
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
                && let Some(mut texture) = source.texture(name)
            {
                crate::model::apply_particle_channels(&mut texture);
                return Some(texture);
            }
        }
    }
    None
}

fn is_additive(source: &Source, doc: &Value) -> bool {
    let Some(material) = doc.get("material").and_then(Value::as_str) else {
        return true;
    };
    let Some(json) = source.json(material) else {
        return true;
    };
    json.get("passes")
        .and_then(Value::as_array)
        .and_then(|passes| passes.first())
        .and_then(|pass| pass.get("blending"))
        .and_then(Value::as_str)
        .is_none_or(|mode| mode.eq_ignore_ascii_case("additive"))
}

fn material_refracts(source: &Source, doc: &Value) -> bool {
    let Some(material) = doc.get("material").and_then(Value::as_str) else {
        return false;
    };
    let Some(json) = source.json(material) else {
        return false;
    };
    json.get("passes")
        .and_then(Value::as_array)
        .and_then(|passes| passes.first())
        .and_then(|pass| pass.get("combos"))
        .and_then(|combos| combos.get("REFRACT"))
        .and_then(Value::as_i64)
        .is_some_and(|value| value != 0)
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
    let initializers = doc
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
    let tint = over
        .and_then(|value| value.get("colorn"))
        .map_or([1.0, 1.0, 1.0], |value| vec3_of(Some(value), [1.0, 1.0, 1.0]));
    let origin = vec3_of(object.get("origin"), [0.0, 0.0, 0.0]);
    let scale = vec3_of(object.get("scale"), [1.0, 1.0, 1.0])[0];
    let animation = match doc.get("animationmode").and_then(Value::as_str) {
        Some("randomframe") => Animation::RandomFrame,
        _ => Animation::Sequence,
    };
    Some(ParticleSystem {
        renderer,
        trail,
        animation,
        sequence_multiplier: num_of(doc.get("sequencemultiplier"), 1.0).clamp(0.01, 100.0),
        max_count: num_of(doc.get("maxcount"), 100.0).clamp(1.0, 100_000.0) as usize,
        start_time: num_of(doc.get("starttime"), 0.0).clamp(0.0, 300.0),
        emitters,
        initializers,
        operators,
        texture: sprite_texture(&source, &doc),
        additive: is_additive(&source, &doc),
        origin: (origin[0], origin[1], origin[2]),
        scale: if scale.abs() < f32::EPSILON { 1.0 } else { scale },
        tint,
        alpha: over.map_or(1.0, |value| num_of(value.get("alpha"), 1.0)).clamp(0.0, 1.0)
            * if material_refracts(&source, &doc) { 0.3 } else { 1.0 },
        rate_scale: over.map_or(1.0, |value| num_of(value.get("rate"), 1.0)).clamp(0.0, 10.0),
        size_scale: over.map_or(1.0, |value| num_of(value.get("size"), 1.0)).clamp(0.0, 10.0),
    })
}

pub struct Sim {
    pub particles: Vec<Particle>,
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
            pending: 0.0,
            time: 0.0,
            ribbon: false,
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
        particle.pos = system.emitters[index].spawn(&mut self.rng);
        for init in &system.initializers {
            match init {
                Initializer::Lifetime { min, max } => {
                    particle.lifetime = self.rng.range(*min, *max).max(0.01);
                }
                Initializer::Size { min, max } => {
                    particle.base_size = self.rng.range(*min, *max) * system.size_scale;
                    particle.size = particle.base_size;
                }
                Initializer::Alpha { min, max } => {
                    particle.base_alpha = self.rng.range(*min, *max).clamp(0.0, 1.0);
                    particle.alpha = particle.base_alpha;
                }
                Initializer::Color { min, max } => {
                    let color = self.rng.range3(*min, *max);
                    particle.color = [color[0] / 255.0, color[1] / 255.0, color[2] / 255.0];
                }
                Initializer::Velocity { min, max } => {
                    let vel = self.rng.range3(*min, *max);
                    particle.vel = [
                        particle.vel[0] + vel[0],
                        particle.vel[1] + vel[1],
                        particle.vel[2] + vel[2],
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
        self.ribbon = system.renderer == Renderer::Ribbon;
        self.time += dt;
        let capacity = Self::capacity(system);
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
                        let wave = osc.wave(time, seed);
                        let amount = osc.amplitude(seed);
                        particle.alpha *= (1.0 - amount * 0.5 * (1.0 - wave)).clamp(0.0, 1.0);
                    }
                    Operator::OscillateSize(osc) => {
                        let wave = osc.wave(time, seed);
                        let amount = osc.amplitude(seed);
                        particle.size *= (1.0 + amount * wave).max(0.0);
                    }
                    Operator::OscillatePosition(osc) => {
                        let wave = osc.wave(time, seed);
                        let amount = osc.amplitude(seed);
                        for (pos, mask) in particle.pos.iter_mut().zip(osc.mask.iter()) {
                            *pos += wave * amount * mask * dt;
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
                    Operator::ControlPointAttract { origin, scale, threshold } => {
                        let dx = origin[0] - particle.pos[0];
                        let dy = origin[1] - particle.pos[1];
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
