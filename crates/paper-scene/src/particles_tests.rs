use super::*;

fn system() -> ParticleSystem {
    ParticleSystem {
        renderer: Renderer::Sprite,
        animation: Animation::Sequence,
        sequence_multiplier: 1.0,
        trail: Trail { length: 0.0, min_length: 0.0, max_length: 100.0, fade_alpha: true },
        max_count: 64,
        start_time: 0.0,
        control_points: [ControlPoint::default(); 8],
        emitters: vec![Emitter::Box {
            control_point: None,
            instantaneous: 0,
            origin: [0.0, 0.0, 0.0],
            extent: [10.0, 10.0, 0.0],
            rate: 60.0,
        }],
        initializers: vec![
            Initializer::Lifetime { exponent: 1.0, min: 1.0, max: 1.0 },
            Initializer::Size { exponent: 1.0, min: 5.0, max: 5.0 },
        ],
        operators: vec![Operator::AlphaFade { fade_in: 0.0, fade_out: 0.5 }],
        texture: None,
        additive: true,
        blend: super::ParticleBlend::Additive,
        perspective: false,
        pass: None,
        origin: (0.0, 0.0, 0.0),
        angle: 0.0,
        scale: 1.0,
        scale3: [1.0; 3],
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        rate_scale: 1.0,
        size_scale: 1.0,
        grab_slot: None,
        world: false,
    }
}

#[test]
fn emit_rate_and_capacity() {
    let sys = system();
    let mut sim = Sim::new(7);
    for _ in 0..30 {
        sim.step(&sys, 1.0 / 30.0);
    }
    assert!(!sim.particles.is_empty());
    assert!(sim.particles.len() <= Sim::capacity(&sys));
}

#[test]
fn particles_expire_after_lifetime() {
    let sys = system();
    let mut sim = Sim::new(11);
    for _ in 0..30 {
        sim.step(&sys, 1.0 / 30.0);
    }
    let peak = sim.particles.len();
    assert!(peak > 0);
    for _ in 0..120 {
        sim.step(&sys, 1.0 / 30.0);
    }
    assert!(sim.particles.iter().all(|particle| particle.age < particle.lifetime));
}

#[test]
fn alpha_fades_late() {
    let mut sys = system();
    sys.emitters = vec![Emitter::Box {
        control_point: None,
        instantaneous: 0,
        origin: [0.0, 0.0, 0.0],
        extent: [1.0, 1.0, 0.0],
        rate: 1.0,
    }];
    let mut sim = Sim::new(3);
    sim.step(&sys, 1.0);
    sim.particles.clear();
    sim.pending = 1.0;
    sim.step(&sys, 0.001);
    let early = sim.particles.first().map_or(0.0, |particle| particle.alpha);
    for _ in 0..90 {
        sim.step(&sys, 0.01);
    }
    let late = sim.particles.first().map_or(0.0, |particle| particle.alpha);
    assert!(late < early, "{early} -> {late}");
}

fn single(op: Operator) -> ParticleSystem {
    let mut sys = system();
    sys.emitters = vec![Emitter::Box {
        control_point: None,
        instantaneous: 0,
        origin: [0.0, 0.0, 0.0],
        extent: [0.0, 0.0, 0.0],
        rate: 1.0,
    }];
    sys.initializers = vec![
        Initializer::Lifetime { exponent: 1.0, min: 100.0, max: 100.0 },
        Initializer::Size { exponent: 1.0, min: 5.0, max: 5.0 },
    ];
    sys.operators = vec![op];
    sys
}

fn spawn_one(sys: &ParticleSystem) -> Sim {
    let mut sim = Sim::new(17);
    sim.pending = 1.0;
    sim.step(sys, 0.0001);
    assert_eq!(sim.particles.len(), 1);
    sim
}

#[test]
fn turbulence_displaces_particle() {
    let sys = single(Operator::Turbulence {
        scale: 0.01,
        speed: (500.0, 500.0),
        time_scale: 1.0,
        mask: [1.0, 1.0, 1.0],
    });
    let mut sim = spawn_one(&sys);
    let before = sim.particles[0].pos;
    for _ in 0..30 {
        sim.step(&sys, 1.0 / 30.0);
    }
    let after = sim.particles[0].pos;
    assert!((after[0] - before[0]).abs() + (after[1] - before[1]).abs() > 1.0);
}

#[test]
fn attract_pulls_to_origin() {
    let sys = single(Operator::ControlPointAttract {
        control_point: 0,
        origin: [100.0, 0.0, 0.0],
        scale: 5000.0,
        threshold: 500.0,
    });
    let mut sim = spawn_one(&sys);
    for _ in 0..10 {
        sim.step(&sys, 1.0 / 30.0);
    }
    assert!(sim.particles[0].vel[0] > 0.0);
}

#[test]
fn color_change_over_life() {
    let mut sys = single(Operator::ColorChange {
        start: [1.0, 0.0, 0.0],
        end: [0.0, 0.0, 1.0],
        start_time: 0.0,
        end_time: 1.0,
    });
    sys.initializers = vec![
        Initializer::Lifetime { exponent: 1.0, min: 1.0, max: 1.0 },
        Initializer::Size { exponent: 1.0, min: 5.0, max: 5.0 },
    ];
    let mut sim = spawn_one(&sys);
    for _ in 0..15 {
        sim.step(&sys, 1.0 / 30.0);
    }
    let mid = sim.particles[0].color;
    assert!(mid[0] < 0.9 && mid[2] > 0.1, "{mid:?}");
}

#[test]
fn prewarm_is_bounded() {
    let mut sys = system();
    sys.start_time = 300.0;
    let mut sim = Sim::new(5);
    sim.prewarm(&sys, 1.0 / 30.0);
    assert!(sim.particles.len() <= Sim::capacity(&sys));
}

#[test]
fn engine_sprite_stream_follows_the_thick_vertex_layout() {
    use super::{
        Animation, Particle, ParticleBlend, Renderer, SPRITE_FLOATS_PER_VERTEX, Sim, Trail,
        pack_sprites, sprite_indices,
    };
    let system = super::ParticleSystem {
        renderer: Renderer::Sprite,
        trail: Trail { length: 0.0, min_length: 0.0, max_length: 0.0, fade_alpha: false },
        animation: Animation::Sequence,
        sequence_multiplier: 1.0,
        max_count: 4,
        start_time: 0.0,
        control_points: [ControlPoint::default(); 8],
        emitters: Vec::new(),
        initializers: Vec::new(),
        operators: Vec::new(),
        texture: None,
        additive: true,
        blend: ParticleBlend::Additive,
        perspective: false,
        pass: None,
        origin: (0.0, 0.0, 0.0),
        angle: 0.0,
        scale: 1.0,
        scale3: [1.0; 3],
        tint: [1.0, 0.5, 0.25],
        alpha: 1.0,
        rate_scale: 1.0,
        size_scale: 1.0,
        grab_slot: None,
        world: false,
    };
    let mut sim = Sim::new(1);
    sim.particles.push(Particle {
        pos: [10.0, 20.0, 0.0],
        vel: [1.0, 2.0, 3.0],
        size: 40.0,
        base_size: 40.0,
        color: [1.0, 1.0, 1.0],
        alpha: 0.5,
        base_alpha: 0.5,
        angle: 0.3,
        angular: 0.0,
        age: 1.0,
        lifetime: 4.0,
        phase: 0.0,
    });
    sim.particles.push(Particle { alpha: 0.0, ..sim.particles[0] });
    let mut out = Vec::new();
    let count = pack_sprites(&system, &sim, 1, &mut out);
    assert_eq!(count, 1);
    assert_eq!(out.len(), 4 * SPRITE_FLOATS_PER_VERTEX);
    let v0 = &out[..SPRITE_FLOATS_PER_VERTEX];
    assert_eq!(&v0[0..3], &[10.0, 20.0, 0.0]);
    assert_eq!(&v0[3..7], &[0.0, 1.0, 0.3, 40.0 * super::SPRITE_EXTENT]);
    assert_eq!(super::SPRITE_EXTENT, 0.5);
    assert_eq!(&v0[7..11], &[1.0, 0.5, 0.25, 0.5]);
    assert_eq!(&v0[11..15], &[1.0, 2.0, 3.0, 0.25]);
    assert_eq!(&v0[15..17], &[0.0, 0.0]);
    let corners: Vec<[f32; 2]> = (0..4).map(|i| [out[i * 17 + 3], out[i * 17 + 4]]).collect();
    assert_eq!(corners, [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
    assert_eq!(sprite_indices(2), [0, 1, 2, 2, 3, 0, 4, 5, 6, 6, 7, 4]);
}

#[test]
fn instance_overrides_scale_count_lifetime_speed_and_bind_to_properties() {
    use crate::effects::Assets;
    use crate::pkg::Package;
    let particle = br#"{"renderer":[{"name":"sprite"}],"maxcount":100,
        "emitter":[{"name":"boxrandom","rate":2}],
        "initializer":[{"name":"lifetimerandom","min":1,"max":2},{"name":"velocityrandom","min":"1 2 0","max":"3 4 0"},{"name":"turbulentvelocityrandom","min":5,"max":6}]}"#;
    let object: serde_json::Value = serde_json::json!({
        "id": 1, "particle": "particles/p.json", "origin": "0 0 0",
        "instanceoverride": {"count": 0.5, "lifetime": 2.0, "speed": {"user": "spd", "value": 3.0}, "rate": {"user": "missing", "value": 0.25}, "colorn": {"user": "tint", "value": "1 1 1"}}
    });
    let files: Vec<(&str, &[u8])> = vec![("particles/p.json", particle)];
    let bytes = crate::tests::build_pkg(&files);
    let pkg = Package::parse(bytes).unwrap();
    let mut props = std::collections::BTreeMap::new();
    props.insert("spd".to_string(), vec![10.0]);
    props.insert("tint".to_string(), vec![0.5, 0.25, 1.0]);
    let assets = Assets::discover(Some("/nonexistent")).with_properties(props);
    let system = super::load(&pkg, &assets, &object, "particles/p.json").unwrap();
    assert_eq!(system.max_count, 50);
    assert_eq!(system.rate_scale, 0.25);
    assert_eq!(system.tint, [0.5, 0.25, 1.0]);
    let mut saw = 0;
    for init in &system.initializers {
        match init {
            super::Initializer::Lifetime { exponent: 1.0, min, max } => {
                assert_eq!((*min, *max), (2.0, 4.0));
                saw += 1;
            }
            super::Initializer::Velocity { exponent: 1.0, min, max } => {
                assert_eq!(min, &[10.0, 20.0, 0.0]);
                assert_eq!(max, &[30.0, 40.0, 0.0]);
                saw += 1;
            }
            super::Initializer::TurbulentVelocity { min, max } => {
                assert_eq!((*min, *max), (50.0, 60.0));
                saw += 1;
            }
            _ => {}
        }
    }
    assert_eq!(saw, 3);
}

#[test]
fn color_random_lerps_all_channels_with_one_parameter() {
    let mut sys = system();
    sys.initializers.push(Initializer::Color {
        exponent: 1.0,
        min: [255.0, 0.0, 0.0],
        max: [0.0, 0.0, 255.0],
    });
    let mut sim = Sim::new(3);
    for _ in 0..30 {
        sim.step(&sys, 1.0 / 30.0);
    }
    assert!(sim.particles.len() > 20);
    let mut distinct = std::collections::BTreeSet::new();
    for particle in &sim.particles {
        assert!(particle.color[1].abs() < 1e-6);
        assert!((particle.color[0] + particle.color[2] - 1.0).abs() < 1e-5, "{:?}", particle.color);
        distinct.insert((particle.color[0] * 1000.0) as i32);
    }
    assert!(distinct.len() > 5);
}

#[test]
fn sphere_emitter_renormalises_scaled_directions_and_honours_sign() {
    let mut sys = system();
    sys.emitters = vec![Emitter::Sphere {
        control_point: None,
        instantaneous: 0,
        origin: [0.0, 0.0, 0.0],
        directions: [1.0, 0.0, 0.0],
        sign: [0.0, 0.0, 0.0],
        min: 300.0,
        max: 300.0,
        rate: 60.0,
    }];
    let mut sim = Sim::new(5);
    for _ in 0..30 {
        sim.step(&sys, 1.0 / 30.0);
    }
    assert!(sim.particles.iter().all(|p| (p.pos[0].abs() - 300.0).abs() < 1e-3 && p.pos[1] == 0.0));
    assert!(
        sim.particles.iter().any(|p| p.pos[0] > 0.0)
            && sim.particles.iter().any(|p| p.pos[0] < 0.0)
    );
    sys.emitters = vec![Emitter::Sphere {
        control_point: None,
        instantaneous: 0,
        origin: [0.0, 0.0, 0.0],
        directions: [1.0, 1.0, 1.0],
        sign: [0.0, 0.0, -1.0],
        min: 300.0,
        max: 300.0,
        rate: 60.0,
    }];
    let mut sim = Sim::new(5);
    for _ in 0..30 {
        sim.step(&sys, 1.0 / 30.0);
    }
    assert!(sim.particles.iter().all(|p| p.pos[2] <= 0.0));
    assert!(sim.particles.iter().all(|p| {
        let r = (p.pos[0] * p.pos[0] + p.pos[1] * p.pos[1] + p.pos[2] * p.pos[2]).sqrt();
        (r - 300.0).abs() < 1e-2
    }));
}

#[test]
fn emitter_rate_and_capacity_cover_engine_downpours() {
    let entry: serde_json::Value = serde_json::json!({"name": "sphererandom", "rate": 25000, "sign": "10 10 10", "distancemax": 2000});
    let emitter = parse_emitter(&entry).unwrap();
    assert!((emitter.rate() - 25000.0).abs() < 1e-3);
    let Emitter::Sphere { sign, .. } = emitter else { panic!("sphere") };
    assert_eq!(sign, [10.0, 10.0, 10.0]);
    let mut sys = system();
    sys.max_count = 25000;
    assert_eq!(Sim::capacity(&sys), 25000);
}

#[test]
fn a_system_emits_its_first_particle_on_the_first_step_regardless_of_rate() {
    let mut sys = system();
    sys.emitters = vec![Emitter::Box {
        control_point: None,
        instantaneous: 0,
        origin: [0.0; 3],
        extent: [0.0; 3],
        rate: 0.001,
    }];
    sys.initializers = vec![Initializer::Lifetime { exponent: 1.0, min: 10.0, max: 10.0 }];
    let mut sim = Sim::new(9);
    sim.step(&sys, 1.0 / 30.0);
    assert_eq!(sim.particles.len(), 1);
    for _ in 0..30 {
        sim.step(&sys, 1.0 / 30.0);
    }
    assert_eq!(sim.particles.len(), 1);
}

#[test]
fn sphere_emitter_distance_follows_the_scaled_ball_length() {
    let mut sys = system();
    sys.max_count = 4000;
    sys.emitters = vec![Emitter::Sphere {
        control_point: None,
        instantaneous: 0,
        origin: [0.0; 3],
        directions: [1.0, 1.0, 0.0],
        sign: [1.0, 1.0, 1.0],
        min: 0.0,
        max: 1000.0,
        rate: 100_000.0,
    }];
    sys.initializers = vec![Initializer::Lifetime { exponent: 1.0, min: 100.0, max: 100.0 }];
    let mut sim = Sim::new(11);
    sim.step(&sys, 1.0 / 30.0);
    assert!(sim.particles.len() > 3000);
    let mut bins = [0usize; 5];
    let mut sum = 0.0f32;
    for particle in &sim.particles {
        let r = (particle.pos[0] * particle.pos[0] + particle.pos[1] * particle.pos[1]).sqrt();
        assert!(particle.pos[0] >= 0.0 && particle.pos[1] >= 0.0 && particle.pos[2].abs() < 1e-4);
        bins[((r / 200.0) as usize).min(4)] += 1;
        sum += r;
    }
    let n = sim.particles.len() as f32;
    let mean = sum / n;
    assert!((0.55..0.64).contains(&(mean / 1000.0)), "mean radius fraction {}", mean / 1000.0);
    assert!((bins[0] as f32 / n) < 0.1, "{bins:?}");
    assert!(bins[3] > bins[1] && bins[2] > bins[0], "{bins:?}");
}

#[test]
fn world_space_systems_keep_object_scale_off_positions_and_sizes_but_on_velocity() {
    use crate::effects::Assets;
    use crate::pkg::Package;
    let particle = br#"{"renderer":[{"name":"sprite"}],"maxcount":4,"flags":1,
        "emitter":[{"name":"boxrandom","rate":60}],
        "initializer":[{"name":"lifetimerandom","min":5,"max":5},{"name":"velocityrandom","min":"200 0 0","max":"200 0 0"}]}"#;
    let object: serde_json::Value = serde_json::json!({"id": 1, "particle": "particles/p.json", "origin": "600 540 0", "scale": "3 3 1"});
    let files: Vec<(&str, &[u8])> = vec![("particles/p.json", particle)];
    let bytes = crate::tests::build_pkg(&files);
    let pkg = Package::parse(bytes).unwrap();
    let assets = Assets::discover(Some("/nonexistent"));
    let system = super::load(&pkg, &assets, &object, "particles/p.json").unwrap();
    assert!(system.world);
    assert_eq!(system.draw_scale(), 1.0);
    assert_eq!(system.draw_scale3(), [1.0; 3]);
    assert_eq!(system.scale3, [3.0, 3.0, 1.0]);
    let mut sim = Sim::new(3);
    sim.step(&system, 1.0 / 30.0);
    assert!((sim.particles[0].vel[0] - 600.0).abs() < 1e-3, "{:?}", sim.particles[0].vel);
    let mut shell = system;
    shell.emitters = vec![Emitter::Sphere {
        control_point: None,
        instantaneous: 0,
        origin: [100.0, 0.0, 0.0],
        directions: [1.0, 0.0, 0.0],
        sign: [1.0, 0.0, 0.0],
        min: 300.0,
        max: 300.0,
        rate: 60.0,
    }];
    let mut sim = Sim::new(3);
    sim.step(&shell, 1.0 / 30.0);
    assert!(
        (sim.particles[0].pos[0] - 1000.0).abs() < 1e-2,
        "world spawn = origin + scaled offset: {:?}",
        sim.particles[0].pos
    );
    let local: serde_json::Value = serde_json::json!({"id": 1, "particle": "particles/l.json", "origin": "600 540 0", "scale": "3 3 1"});
    let lp = String::from_utf8(particle.to_vec()).unwrap().replace("\"flags\":1", "\"flags\":0");
    let files: Vec<(&str, &[u8])> = vec![("particles/l.json", lp.as_bytes())];
    let pkg = Package::parse(crate::tests::build_pkg(&files)).unwrap();
    let system = super::load(&pkg, &assets, &local, "particles/l.json").unwrap();
    assert!(!system.world);
    assert_eq!(system.draw_scale(), 3.0);
    let mut sim = Sim::new(3);
    sim.step(&system, 1.0 / 30.0);
    assert!((sim.particles[0].vel[0] - 200.0).abs() < 1e-3);
}

#[test]
fn initializer_exponent_biases_toward_min_or_max_and_bursts_spawn_instantly() {
    let mut sys = system();
    sys.max_count = 2000;
    sys.emitters = vec![Emitter::Box {
        control_point: None,
        origin: [0.0; 3],
        extent: [0.0; 3],
        rate: 0.0,
        instantaneous: 1500,
    }];
    sys.initializers = vec![
        Initializer::Lifetime { min: 100.0, max: 100.0, exponent: 1.0 },
        Initializer::Size { min: 20.0, max: 200.0, exponent: 30.0 },
        Initializer::Alpha { min: 0.0, max: 1.0, exponent: 0.1 },
    ];
    let mut sim = Sim::new(4);
    sim.step(&sys, 1.0 / 30.0);
    assert_eq!(sim.particles.len(), 1500);
    let n = sim.particles.len() as f32;
    let mean_size = sim.particles.iter().map(|p| p.base_size).sum::<f32>() / n;
    let mean_alpha = sim.particles.iter().map(|p| p.base_alpha).sum::<f32>() / n;
    assert!(mean_size < 30.0, "exponent 30 biases toward min: {mean_size}");
    assert!(mean_alpha > 0.85, "exponent 0.1 biases toward max: {mean_alpha}");
    sim.step(&sys, 1.0 / 30.0);
    assert_eq!(sim.particles.len(), 1500, "burst happens once");
}

#[test]
fn oscillate_position_integrates_a_radians_per_second_wave_on_particle_age_with_no_spawn_offset() {
    let mut sys = system();
    sys.max_count = 1;
    sys.emitters = vec![Emitter::Box {
        control_point: None,
        origin: [0.0; 3],
        extent: [0.0; 3],
        rate: 1.0,
        instantaneous: 0,
    }];
    sys.initializers = vec![Initializer::Lifetime { min: 40.0, max: 40.0, exponent: 1.0 }];
    let osc = |frequency: f32, phase: f32| {
        vec![Operator::OscillatePosition(Oscillator {
            frequency: (frequency, frequency),
            scale: (300.0, 300.0),
            phase: (phase, phase),
            mask: [1.0, 0.0, 0.0],
        })]
    };
    sys.operators = osc(1.0, 0.0);
    let mut sim = Sim::new(5);
    let dt = 1.0 / 100.0;
    let mut peak = 0.0f32;
    for step in 1..=628 {
        sim.step(&sys, dt);
        let expected = 300.0 * (step as f32 * dt).sin();
        assert!(
            (sim.particles[0].pos[0] - expected).abs() < 6.0,
            "step {step}: {}",
            sim.particles[0].pos[0]
        );
        peak = peak.max(sim.particles[0].pos[0]);
    }
    assert!(
        (peak - 300.0).abs() < 6.0,
        "amplitude = scale * mask, period = tau / frequency: {peak}"
    );
    assert!(sim.particles[0].pos[1].abs() < 1e-3);
    sys.operators = osc(0.0001, std::f32::consts::FRAC_PI_2);
    let mut sim = Sim::new(5);
    for _ in 0..100 {
        sim.step(&sys, dt);
    }
    assert!(sim.particles[0].pos[0].abs() < 1.0, "no offset at spawn: {}", sim.particles[0].pos[0]);
    sys.operators = osc(1.0, 0.0);
    sys.world = true;
    sys.scale3 = [2.0, 1.0, 3.0];
    let mut sim = Sim::new(5);
    for _ in 0..157 {
        sim.step(&sys, dt);
    }
    let x = sim.particles[0].pos[0];
    assert!((x - 600.0).abs() < 12.0, "world-space systems scale the movement per axis: {x}");
}

#[test]
fn cursor_control_point_spawns_a_trail_without_moving_existing_particles() {
    let doc = br#"{"controlpoint":[{"id":0,"flags":1,"offset":"4 6 0"},{"id":80,"flags":1}],"emitter":[{"name":"boxrandom","rate":10}],"initializer":[{"name":"lifetimerandom","min":10,"max":10}]}"#;
    let pkg = crate::pkg::Package::parse(crate::tests::build_pkg(&[("p.json", doc)])).unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let object = serde_json::json!({"origin":"100 200 0","scale":"2 3 1"});
    let sys = load(&pkg, &assets, &object, "p.json").unwrap();
    assert!(sys.follows_mouse());
    let mut sim = Sim::new(1);
    sim.set_pointer([120.0, 230.0]);
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles[0].pos, [12.0, 12.0, 0.0]);
    sim.set_pointer([320.0, 530.0]);
    sim.step(&sys, 0.1);
    assert_eq!(sim.particles[0].pos, [12.0, 12.0, 0.0]);
    assert_eq!(sim.particles[1].pos, [112.0, 112.0, 0.0]);
}

#[test]
fn explicit_control_points_respect_rotation_world_space_and_static_offsets() {
    let mut sys = system();
    sys.origin = (100.0, 200.0, 0.0);
    sys.scale3 = [2.0, 3.0, 1.0];
    sys.angle = std::f32::consts::FRAC_PI_2;
    sys.control_points[1] = ControlPoint { flags: 1, offset: [0.0; 3] };
    sys.control_points[2] = ControlPoint { flags: 2, offset: [120.0, 230.0, 0.0] };
    sys.control_points[3] = ControlPoint { flags: 0, offset: [4.0, 5.0, 0.0] };
    sys.emitters = vec![Emitter::Box {
        control_point: Some(1),
        origin: [0.0; 3],
        extent: [0.0; 3],
        rate: 0.0,
        instantaneous: 0,
    }];
    let mut sim = Sim::new(2);
    sim.set_pointer([120.0, 230.0]);
    sim.step(&sys, 0.01);
    assert!((sim.particles[0].pos[0] - 15.0).abs() < 1e-4);
    assert!((sim.particles[0].pos[1] + 20.0 / 3.0).abs() < 1e-4);
    assert_eq!(sim.control_points[1], sim.control_points[2]);
    assert_eq!(sim.control_points[3], [4.0, 5.0, 0.0]);
    sys.world = true;
    sim.step(&sys, 0.01);
    assert!((sim.control_points[1][0] - 30.0).abs() < 1e-4);
    assert!((sim.control_points[1][1] + 20.0).abs() < 1e-4);
}

#[test]
fn attraction_tracks_its_selected_mouse_control_point() {
    let mut sys = single(Operator::ControlPointAttract {
        control_point: 1,
        origin: [0.0; 3],
        scale: 100.0,
        threshold: 1000.0,
    });
    sys.control_points[1] = ControlPoint { flags: 1, offset: [0.0; 3] };
    let mut sim = Sim::new(3);
    sim.set_pointer([100.0, 0.0]);
    sim.step(&sys, 0.01);
    assert!(sim.particles[0].vel[0] > 0.0);
    sim.particles[0].vel = [0.0; 3];
    sim.set_pointer([-100.0, 0.0]);
    sim.step(&sys, 0.01);
    assert!(sim.particles[0].vel[0] < 0.0);
}
