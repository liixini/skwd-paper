use super::*;

#[test]
fn emitter_audio_uses_selected_stereo_peak_then_smoothed_bounds_and_exponent() {
    let response = AudioResponse::parse(&serde_json::json!({"audioprocessingmode":3})).unwrap();
    let mut left = [0.0; 16];
    let mut right = [0.0; 16];
    left[7] = 4.0;
    right[7] = 4.0;
    assert_eq!(response.amount(&left, &right), 0.0);
    left[0] = 1.0;
    right[1] = 1.0;
    assert_eq!(response.amount(&left, &right), 0.0);
    left[0] = 0.9;
    right[0] = 0.9;
    assert!((response.amount(&left, &right) - 0.25).abs() < 1e-5);
    left[0] = 1.0;
    right[0] = 1.0;
    assert_eq!(response.amount(&left, &right), 1.0);
    assert!(AudioResponse::parse(&serde_json::json!({"audioprocessingmode":0})).is_none());

    for (mode, expected) in [(1, 0.15625), (2, 0.84375), (3, 0.5)] {
        let response = AudioResponse::parse(&serde_json::json!({
            "audioprocessingmode":mode,"audioprocessingexponent":1,
            "audioprocessingbounds":"0 1","audioprocessingfrequencystart":15,
            "audioprocessingfrequencyend":14
        }))
        .unwrap();
        left[14] = 0.25;
        right[14] = 0.75;
        assert_eq!(response.amount(&left, &right), expected);
    }
}

#[test]
fn equal_audio_bounds_activate_only_above_the_threshold() {
    let response = AudioResponse::parse(&serde_json::json!({
        "audioprocessingmode":3,"audioprocessingbounds":"0.8 0.8"
    }))
    .unwrap();
    for (level, expected) in [(0.7, 0.0), (0.8, 0.0), (0.9, 1.0)] {
        assert_eq!(response.amount(&[level; 16], &[level; 16]), expected);
    }
}

#[test]
fn audio_emitters_start_and_prewarm_silently_then_emit_only_with_matching_audio() {
    let mut sys = system();
    sys.start_time = 2.0;
    sys.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"boxrandom","rate":40,"audioprocessingmode":3
        }))
        .unwrap(),
    ];
    sys.initializers = vec![Initializer::Lifetime { min: 3.0, max: 3.0, exponent: 1.0 }];
    assert!(sys.needs_audio());
    let mut sim = Sim::new(7);
    sim.prewarm(&sys, 0.1);
    sim.step(&sys, 0.0);
    assert!(sim.particles.is_empty());
    let mut bass = [0.0; 16];
    bass[0] = 1.0;
    for _ in 0..4 {
        sim.step_with_audio(&sys, 0.25, &bass, &bass);
    }
    assert_eq!(sim.particles.len(), 40);
    let age = sim.particles[0].age;
    sim.step_with_audio(&sys, 0.0, &bass, &bass);
    assert_eq!(sim.particles.len(), 40);
    assert_eq!(sim.particles[0].age, age);
    for _ in 0..16 {
        sim.step(&sys, 0.25);
    }
    assert!(sim.particles.is_empty());

    sys.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"boxrandom","rate":40,"audioprocessingmode":0
        }))
        .unwrap(),
    ];
    let mut unbound = Sim::new(7);
    unbound.prewarm(&sys, 0.1);
    assert!(!sys.needs_audio());
    assert!(!unbound.particles.is_empty());
}

#[test]
fn inactive_audio_emitter_never_receives_another_emitters_particles() {
    let mut sys = system();
    sys.emitters = [
        serde_json::json!({"name":"boxrandom","rate":40,"origin":"-100 0 0","audioprocessingmode":1}),
        serde_json::json!({"name":"boxrandom","rate":40,"origin":"100 0 0","audioprocessingmode":2}),
    ].iter().map(|entry| parse_emitter(entry).unwrap()).collect();
    let mut bass = [0.0; 16];
    bass[0] = 1.0;
    let mut sim = Sim::new(7);
    sim.step_with_audio(&sys, 0.25, &bass, &[]);
    assert_eq!(sim.particles.len(), 10);
    assert!(sim.particles.iter().all(|particle| particle.pos[0] == -100.0));
}

fn system() -> ParticleSystem {
    ParticleSystem {
        children: Vec::new(),
        follow_limit: None,
        renderer: Renderer::Sprite,
        animation: Animation::Sequence,
        sequence_multiplier: 1.0,
        trail: Trail { length: 0.0, min_length: 0.0, max_length: 100.0, fade_alpha: true },
        max_count: 64,
        start_time: 0.0,
        control_points: [ControlPoint::default(); 8],
        emitters: vec![Emitter::Box {
            audio: None,
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
        count_scale: 1.0,
        rate_scale: 1.0,
        size_scale: 1.0,
        grab_slot: None,
        world: false,
    }
}

#[test]
fn zero_delta_initializes_particles_once_and_preserves_ribbon_history() {
    let mut continuous = Sim::new(7);
    continuous.step(&system(), 0.0);
    assert_eq!(continuous.particles.len(), 1);
    let mut sys = system();
    sys.renderer = Renderer::Ribbon;
    sys.emitters = vec![Emitter::Box {
        audio: None,
        control_point: None,
        instantaneous: 2,
        origin: [0.0; 3],
        extent: [0.0; 3],
        rate: 0.0,
    }];
    sys.operators = vec![Operator::Movement { gravity: [0.0; 3], drag: 0.0 }];
    let mut sim = Sim::new(7);
    sim.step(&sys, 0.0);
    assert_eq!(sim.particles.len(), 2);
    assert!(sim.particles.iter().all(|particle| particle.size == 5.0));
    for particle in &mut sim.particles {
        particle.vel = [10.0, 0.0, 0.0];
    }
    sim.step(&sys, 0.1);
    let history = sim.history.clone();
    for _ in 0..3 {
        sim.step(&sys, 0.0);
        assert_eq!(sim.history, history);
        assert_eq!(sim.particles.len(), 2);
        assert_eq!(sim.time, 0.1);
    }
    sim.step(&sys, 0.1);
    assert_ne!(sim.history, history);
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
        audio: None,
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
        audio: None,
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
fn turbulence_accelerates_without_directly_displacing_particle() {
    let sys = single(Operator::Turbulence {
        scale: 0.01,
        speed: (500.0, 500.0),
        phase_range: 0.0,
        time_scale: 1.0,
        mask: [1.0, 1.0, 1.0],
    });
    let mut sim = spawn_one(&sys);
    let before = sim.particles[0].pos;
    for _ in 0..30 {
        sim.step(&sys, 1.0 / 30.0);
    }
    let after = sim.particles[0].pos;
    assert_eq!(after, before);
    assert!(sim.particles[0].vel.iter().any(|value| value.abs() > 1.0));
}

#[test]
fn attract_pulls_to_control_point() {
    let mut sys = single(Operator::ControlPointAttract {
        control_point: 0,
        flags: 2,
        delete_threshold: 15.0,
        scale: 5000.0,
        threshold: 500.0,
    });
    sys.control_points[0].offset = [100.0, 0.0, 0.0];
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
        children: Vec::new(),
        follow_limit: None,
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
        count_scale: 1.0,
        rate_scale: 1.0,
        size_scale: 1.0,
        grab_slot: None,
        world: false,
    };
    let mut sim = Sim::new(1);
    sim.particles.push(Particle {
        id: 0,
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
        "initializer":[{"name":"lifetimerandom","min":1,"max":2},{"name":"velocityrandom","min":"1 2 0","max":"3 4 0"},{"name":"turbulentvelocityrandom","speedmin":5,"speedmax":6}]}"#;
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
            super::Initializer::TurbulentVelocity(turbulent) => {
                assert_eq!(turbulent.speed, (50.0, 60.0));
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
        audio: None,
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
        audio: None,
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
        audio: None,
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
        audio: None,
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
        audio: None,
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
        audio: None,
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
        audio: None,
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
        audio: None,
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
        flags: 2,
        delete_threshold: 15.0,
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

fn turbulent(scale: f32, offset: f32, phase: (f32, f32)) -> Turbulent {
    Turbulent {
        speed: (100.0, 100.0),
        scale,
        offset,
        forward: [0.0, 1.0, 0.0],
        right: [0.0, 0.0, 1.0],
        time_scale: 1.0,
        phase,
    }
}

fn point_emitter(origin: [f32; 3], rate: f32, instantaneous: u32) -> Emitter {
    Emitter::Box { audio: None, control_point: None, instantaneous, origin, extent: [0.0; 3], rate }
}

#[test]
fn turbulent_velocity_stays_inside_the_forward_cone_and_the_plane() {
    let mut sys = system();
    sys.initializers.push(Initializer::TurbulentVelocity(turbulent(0.5, 0.0, (0.0, 0.1))));
    let mut sim = Sim::new(9);
    for _ in 0..30 {
        sim.step(&sys, 1.0 / 30.0);
    }
    assert!(sim.particles.len() > 20);
    let limit = std::f32::consts::FRAC_PI_2.cos() - 1e-3;
    for particle in &sim.particles {
        let speed = dot3(particle.vel, particle.vel).sqrt();
        assert!((speed - 100.0).abs() < 0.01, "speed {speed}");
        assert_eq!(particle.vel[2], 0.0);
        assert!(particle.vel[1] / speed >= limit, "outside the cone: {:?}", particle.vel);
    }
}

#[test]
fn turbulent_directions_are_coherent_per_emission_and_drift_over_time() {
    let mut sys = system();
    sys.emitters = vec![point_emitter([0.0; 3], 600.0, 0)];
    sys.initializers.push(Initializer::TurbulentVelocity(turbulent(2.0, 0.0, (0.0, 0.0))));
    let mut sim = Sim::new(4);
    sim.step(&sys, 1.0 / 30.0);
    let first = sim.particles[0].vel;
    assert!(sim.particles.len() >= 10);
    assert!(
        sim.particles
            .iter()
            .all(|p| (p.vel[0] - first[0]).abs() < 1e-3 && (p.vel[1] - first[1]).abs() < 1e-3)
    );
    sim.particles.clear();
    for _ in 0..15 {
        sim.step(&sys, 1.0 / 30.0);
    }
    let later = sim.particles.last().unwrap().vel;
    assert!((later[0] - first[0]).abs() + (later[1] - first[1]).abs() > 1e-3, "{later:?}");
}

#[test]
fn turbulent_offset_and_scale_rotate_the_authored_forward_vector() {
    let straight = turbulent_direction(0.5, &turbulent(0.0, 0.0, (0.0, 0.0)));
    assert!((straight[0]).abs() < 1e-6 && (straight[1] - 1.0).abs() < 1e-6);
    let tilted = turbulent_direction(0.5, &turbulent(0.0, std::f32::consts::FRAC_PI_2, (0.0, 0.0)));
    assert!((tilted[0] + 1.0).abs() < 1e-5 && tilted[1].abs() < 1e-5, "{tilted:?}");
    let mut init = turbulent(0.0, 0.0, (0.0, 0.0));
    init.forward = [2.0, 0.0, 0.0];
    assert_eq!(turbulent_direction(0.5, &init), [2.0, 0.0, 0.0]);
}

#[test]
fn map_sequence_spreads_emissions_evenly_around_the_control_point() {
    let mut sys = system();
    sys.emitters = vec![point_emitter([0.0; 3], 60.0, 0)];
    sys.control_points[2].offset = [50.0, 20.0, 0.0];
    sys.initializers.push(Initializer::MapSequence {
        control_point: 2,
        count: 4.0,
        axis: [0.0, 0.0, 1.0],
        min: [100.0, 0.0, 0.0],
        max: [100.0, 0.0, 0.0],
    });
    let mut sim = Sim::new(2);
    sim.step(&sys, 3.0 / 60.0);
    assert_eq!(sim.particles.len(), 4);
    let expected = [[100.0, 0.0], [0.0, -100.0], [-100.0, 0.0], [0.0, 100.0]];
    for (particle, want) in sim.particles.iter().zip(expected) {
        assert!((particle.pos[0] - 50.0).abs() < 1e-3 && (particle.pos[1] - 20.0).abs() < 1e-3);
        assert!(
            (particle.vel[0] - want[0]).abs() < 1e-3 && (particle.vel[1] - want[1]).abs() < 1e-3,
            "{:?}",
            particle.vel
        );
    }
    sys.emitters = vec![point_emitter([30.0, 40.0, 0.0], 60.0, 0)];
    let mut sim = Sim::new(2);
    sim.step(&sys, 3.0 / 60.0);
    for (particle, want) in sim.particles.iter().zip(expected) {
        let along = [particle.pos[0] - 50.0, particle.pos[1] - 20.0];
        assert!((along[0] - want[0] * 0.5).abs() < 1e-3 && (along[1] - want[1] * 0.5).abs() < 1e-3);
    }
}

#[test]
fn vortex_spins_particles_around_the_axis_and_can_pull_them_inward() {
    let mut sys = system();
    sys.emitters = vec![point_emitter([100.0, 0.0, 0.0], 0.0, 1)];
    let vortex = Vortex {
        control_point: 0,
        infinite_axis: true,
        maintain_distance: false,
        ring: false,
        axis: [0.0, 0.0, 1.0],
        offset: [0.0; 3],
        distance: (0.0, 200.0),
        speed: (1000.0, 1000.0),
        center_force: 500.0,
        ring_radius: 300.0,
        ring_width: 50.0,
        ring_pull: (50.0, 10.0),
        audio: false,
    };
    sys.operators = vec![Operator::Vortex(vortex)];
    let mut sim = Sim::new(3);
    sim.step(&sys, 0.1);
    let spun = sim.particles[0].vel;
    assert!(spun[0].abs() < 1e-3 && (spun[1] + 100.0).abs() < 1e-3, "{spun:?}");
    sys.operators = vec![Operator::Vortex(Vortex { maintain_distance: true, ..vortex })];
    let mut sim = Sim::new(3);
    sim.step(&sys, 0.1);
    assert_eq!(sim.particles[0].vel, spun);
    sys.operators =
        vec![Operator::Vortex(Vortex { maintain_distance: true, distance: (0.0, 50.0), ..vortex })];
    let mut sim = Sim::new(3);
    sim.step(&sys, 0.1);
    let pulled = sim.particles[0].vel;
    assert!((pulled[0] + 50.0).abs() < 1e-3 && (pulled[1] + 100.0).abs() < 1e-3, "{pulled:?}");
    sys.operators = vec![Operator::Vortex(Vortex { ring: true, ..vortex })];
    let mut sim = Sim::new(3);
    sim.step(&sys, 0.1);
    assert_eq!(sim.particles[0].vel, [0.0; 3]);
    sys.operators = vec![Operator::Vortex(Vortex { ring: true, ring_radius: 100.0, ..vortex })];
    let mut sim = Sim::new(3);
    sim.step(&sys, 0.1);
    assert_eq!(sim.particles[0].vel, spun);
}

#[test]
fn once_animation_walks_the_sheet_a_single_time_and_holds_the_last_frame() {
    let mut sys = system();
    sys.animation = Animation::Once;
    let mut sim = Sim::new(5);
    sim.particles.push(Particle {
        id: 0,
        pos: [0.0; 3],
        vel: [0.0; 3],
        size: 10.0,
        base_size: 10.0,
        color: [1.0; 3],
        alpha: 1.0,
        base_alpha: 1.0,
        angle: 0.0,
        angular: 0.0,
        age: 0.0,
        lifetime: 1.0,
        phase: 0.0,
    });
    let mut out = Vec::new();
    pack_sprites(&sys, &sim, 8, &mut out);
    assert_eq!(out[14], 0.0);
    sim.particles[0].age = 0.5;
    pack_sprites(&sys, &sim, 8, &mut out);
    assert!((out[14] - 4.0 / 8.0).abs() < 1e-6);
    sim.particles[0].age = 1.0;
    pack_sprites(&sys, &sim, 8, &mut out);
    assert!((out[14] - 7.0 / 8.0).abs() < 1e-6);
    sys.sequence_multiplier = 2.0;
    sim.particles[0].age = 0.5;
    pack_sprites(&sys, &sim, 8, &mut out);
    assert!((out[14] - 7.0 / 8.0).abs() < 1e-6);
}

#[test]
fn turbulent_vortex_and_map_sequence_parsers_take_engine_defaults() {
    let init = parse_initializer(&serde_json::json!({
        "name": "turbulentvelocityrandom", "offset": -0.5, "scale": 0.1
    }))
    .unwrap();
    let Initializer::TurbulentVelocity(t) = init else { panic!("turbulent") };
    assert_eq!((t.speed, t.scale, t.offset), ((100.0, 250.0), 0.1, -0.5));
    assert_eq!(
        (t.forward, t.right, t.time_scale, t.phase),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 1.0, (0.0, 0.1))
    );
    let op = parse_operator(&serde_json::json!({
        "name": "vortex", "distanceinner": 0, "distanceouter": 32, "speedinner": 0,
        "speedouter": 2500, "flags": 3
    }))
    .unwrap();
    let Operator::Vortex(v) = op else { panic!("vortex") };
    assert_eq!(
        (v.distance, v.speed, v.infinite_axis, v.maintain_distance, v.ring),
        ((0.0, 32.0), (0.0, 2500.0), true, true, false)
    );
    assert_eq!(
        (v.center_force, v.ring_radius, v.ring_width, v.ring_pull),
        (1.0, 300.0, 50.0, (50.0, 10.0))
    );
    let map = parse_initializer(&serde_json::json!({
        "name": "mapsequencearoundcontrolpoint", "count": 3.02, "speedmin": "0 10 0",
        "speedmax": "0 100 0"
    }))
    .unwrap();
    let Initializer::MapSequence { count, min, max, .. } = map else { panic!("map") };
    assert_eq!((count, min, max), (3.02, [0.0, 10.0, 0.0], [0.0, 100.0, 0.0]));
}

fn overridden_system(
    overrides: &serde_json::Value,
    operators: &serde_json::Value,
) -> ParticleSystem {
    let definition = serde_json::json!({
        "renderer":[{"name":"sprite"}],"maxcount":1000,
        "emitter":[{"name":"boxrandom","rate":100,"distancemin":"0 0 0","distancemax":"0 0 0"}],
        "initializer":[{"name":"lifetimerandom","min":1.5,"max":1.5}],
        "operator":operators
    })
    .to_string();
    let bytes = crate::tests::build_pkg(&[("particles/p.json", definition.as_bytes())]);
    let pkg = crate::pkg::Package::parse(bytes).unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    super::load(
        &pkg,
        &assets,
        &serde_json::json!({"instanceoverride":overrides}),
        "particles/p.json",
    )
    .unwrap()
}

#[test]
fn count_multiplier_controls_steady_emission_and_zero_disables_it() {
    for (count, bounds) in [(0.18, 25..=28), (1.0, 143..=151), (0.0, 0..=0)] {
        let sys = overridden_system(&serde_json::json!({"count":count}), &serde_json::json!([]));
        let mut sim = Sim::new(7);
        for _ in 0..120 {
            sim.step(&sys, 1.0 / 15.0);
        }
        assert!(bounds.contains(&sim.particles.len()), "count={count}: {}", sim.particles.len());
    }
}

#[test]
fn rate_multiplier_advances_particle_age_and_motion_with_emission() {
    let mut sys = overridden_system(
        &serde_json::json!({"rate":0.25}),
        &serde_json::json!([
            {"name":"movement","gravity":"500 0 0","drag":5}
        ]),
    );
    let mut slow = Sim::new(7);
    let mut reference = Sim::new(7);
    for _ in 0..60 {
        slow.step(&sys, 1.0 / 60.0);
    }
    sys.rate_scale = 1.0;
    for _ in 0..60 {
        reference.step(&sys, 1.0 / 240.0);
    }
    assert_eq!(slow.particles.len(), reference.particles.len());
    assert_eq!(slow.particles[0].age, reference.particles[0].age);
    assert_eq!(slow.particles[0].pos, reference.particles[0].pos);
    assert!((slow.particles[0].age - 0.25).abs() < 0.001);
}

#[test]
fn speed_multiplier_scales_gravity_without_changing_age_or_drag() {
    let motion = serde_json::json!([{"name":"movement","gravity":"500 0 0","drag":5}]);
    let normal = overridden_system(&serde_json::json!({"speed":1.0}), &motion);
    let slow = overridden_system(&serde_json::json!({"speed":0.12}), &motion);
    let mut reference = Sim::new(7);
    let mut candidate = Sim::new(7);
    for _ in 0..15 {
        reference.step(&normal, 1.0 / 15.0);
        candidate.step(&slow, 1.0 / 15.0);
    }
    assert_eq!(candidate.particles.len(), reference.particles.len());
    assert_eq!(candidate.particles[0].age, reference.particles[0].age);
    assert!((candidate.particles[0].vel[0] / reference.particles[0].vel[0] - 0.12).abs() < 0.0001);
}

#[test]
fn speed_multiplier_scales_control_point_force() {
    let force = serde_json::json!([{"name":"controlpointattract","controlpoint":1,"scale":500,"threshold":10000}]);
    let mut normal = overridden_system(&serde_json::json!({"speed":1.0}), &force);
    normal.control_points[1].offset = [100.0, 0.0, 0.0];
    let mut slow = overridden_system(&serde_json::json!({"speed":0.12}), &force);
    slow.control_points[1].offset = [100.0, 0.0, 0.0];
    let mut reference = Sim::new(7);
    let mut candidate = Sim::new(7);
    reference.step(&normal, 1.0 / 15.0);
    candidate.step(&slow, 1.0 / 15.0);
    assert!(reference.particles[0].vel[0] > 0.0);
    assert!((candidate.particles[0].vel[0] / reference.particles[0].vel[0] - 0.12).abs() < 0.0001);
}

#[test]
fn movement_matches_proton_terminal_speeds_at_15_30_and_60_fps() {
    for (fps, expected) in [(15, 119.24), (30, 73.385), (60, 60.0)] {
        let mut sys = single(Operator::Movement { gravity: [300.0, 0.0, 0.0], drag: 5.0 });
        sys.initializers = vec![Initializer::Lifetime { min: 120.0, max: 120.0, exponent: 1.0 }];
        let mut sim = spawn_one(&sys);
        let dt = 1.0 / fps as f32;
        for _ in 0..fps * 6 {
            sim.step(&sys, dt);
        }
        let before = sim.particles[0].pos[0];
        for _ in 0..fps * 2 {
            sim.step(&sys, dt);
        }
        let speed = (sim.particles[0].pos[0] - before) / 2.0;
        assert!((speed - expected).abs() < 0.2, "{fps} FPS: {speed}");
    }
}

#[test]
fn movement_advances_before_drag_and_negative_drag_accelerates() {
    let mut sys = single(Operator::Movement { gravity: [30.0, 0.0, 0.0], drag: 5.0 });
    let mut sim = spawn_one(&sys);
    sim.particles[0].pos = [0.0; 3];
    sim.particles[0].vel = [60.0, 0.0, 0.0];
    sim.step(&sys, 0.02);
    assert!((sim.particles[0].pos[0] - 1.212).abs() < 1e-5);
    assert!((sim.particles[0].vel[0] - 54.54).abs() < 1e-5);
    sys.operators = vec![Operator::Movement { gravity: [0.0; 3], drag: -5.0 }];
    sim.step(&sys, 0.02);
    assert!((sim.particles[0].vel[0] - 59.994).abs() < 1e-4);
}

#[test]
fn alpha_and_size_oscillation_use_age_and_authored_scale_bounds() {
    for size in [false, true] {
        let oscillator = Oscillator {
            frequency: (2.0, 2.0),
            scale: (0.25, 0.25),
            phase: (1.0, 1.0),
            mask: [1.0; 3],
        };
        let op = if size {
            Operator::OscillateSize(oscillator)
        } else {
            Operator::OscillateAlpha(oscillator)
        };
        let sys = single(op);
        let mut sim = spawn_one(&sys);
        sim.particles[0].lifetime = 120.0;
        for _ in 0..180 {
            sim.step(&sys, 1.0 / 60.0);
            let p = &sim.particles[0];
            let ratio = if size { p.size / p.base_size } else { p.alpha / p.base_alpha };
            assert!((ratio - 0.25).abs() < 1e-6);
        }
    }
    let osc =
        Oscillator { frequency: (2.0, 2.0), scale: (0.2, 0.8), phase: (1.0, 1.0), mask: [1.0; 3] };
    let low = std::f32::consts::PI * 0.75 - 1.0;
    let high = std::f32::consts::PI * 1.25 - 1.0;
    assert!((osc.modulation(low, 0.5) - 0.2).abs() < 1e-6);
    assert!((osc.modulation(high, 0.5) - 0.5).abs() < 1e-6);
    assert!((osc.modulation(high + std::f32::consts::PI, 0.5) - 0.5).abs() < 1e-6);
}

#[test]
fn position_oscillation_integrates_exact_sine_differences_and_offsets_y_phase() {
    let osc = Oscillator {
        frequency: (7.0, 7.0),
        scale: (30.0, 30.0),
        phase: (0.4, 0.4),
        mask: [1.0; 3],
    };
    let seed = 0.25;
    let mut position = [0.0; 3];
    for frame in 1..=90 {
        let age = frame as f32 / 30.0;
        let delta = osc.displacement(age, 1.0 / 30.0, seed);
        for axis in 0..3 {
            position[axis] += delta[axis];
        }
    }
    let expected =
        |offset: f32| (((3.0 + 0.4 + offset) * 7.0).sin() - ((0.4 + offset) * 7.0).sin()) * 30.0;
    assert!((position[0] - expected(0.0)).abs() < 0.001);
    assert!((position[1] - expected(seed * std::f32::consts::TAU)).abs() < 0.001);
    assert_eq!(position[0], position[2]);
}

#[test]
fn turbulence_matches_proton_motion_at_15_fps() {
    for (y, samples) in [
        (50.0, [84.5, 88.5, 93.0, 97.0, 101.0, 107.0, 116.0]),
        (100.0, [99.0, 102.0, 102.0, 100.0, 96.0, 89.0, 82.5]),
    ] {
        let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        sys.max_count = 1;
        sys.emitters = vec![parse_emitter(&serde_json::json!({"name":"boxrandom", "origin":format!("0 {y} 0"), "distancemin":"0 0 0", "distancemax":"0 0 0", "rate":10})).unwrap()];
        sys.operators.push(parse_operator(&serde_json::json!({"name":"turbulence","scale":0.01,"speedmin":30,"speedmax":30,"timescale":0,"mask":"1 0 0"})).unwrap());
        let mut sim = Sim::new(17);
        let mut frame = 0;
        for (reference, expected) in
            [178.0_f32, 203.5, 228.5, 254.0, 278.0, 303.0, 328.5].into_iter().zip(samples)
        {
            let target = ((reference - 80.0) / 20.0 * 15.0).round() as usize;
            while frame < target {
                sim.step(&sys, 1.0 / 15.0);
                frame += 1;
            }
            let actual = 80.0 + sim.particles[0].pos[0] / 3.0;
            assert!((actual - expected).abs() < 1.5, "y={y} frame={frame}: {actual} != {expected}");
        }
    }
}

#[test]
fn simplex_matches_we_binary_reference_vectors() {
    for (point, expected) in [
        ([0.0, 0.5, 0.0], 0.050_442_874),
        ([0.0, 1.0, 0.0], 0.652_221_4),
        ([0.1, 0.5, 0.0], 0.185_624_85),
        ([0.5, 0.5, 0.0], 0.003_200_414_4),
        ([0.9, 1.0, 0.0], -0.570_397_73),
    ] {
        assert!(
            (simplex(point) - expected).abs() < 0.00001,
            "{point:?} {} != {expected}",
            simplex(point)
        );
    }
}

#[test]
fn child_definitions_load_recursively_with_bounds_and_instance_overrides() {
    let root = serde_json::json!({"emitter":[{"name":"boxrandom"}],"children":[
        {"name":"child.json","type":"eventfollow","maxcount":3,"scale":0.55},
        {"name":"missing.json"}
    ]})
    .to_string();
    let child =
        serde_json::json!({"emitter":[{"name":"boxrandom"}],"children":[{"name":"root.json"}]})
            .to_string();
    let pkg = Package::parse(crate::tests::build_pkg(&[
        ("root.json", root.as_bytes()),
        ("child.json", child.as_bytes()),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let system =
        load(&pkg, &assets, &serde_json::json!({"instanceoverride":{"count":0.2}}), "root.json")
            .unwrap();
    assert_eq!(system.children.len(), 1);
    let child = &system.children[0];
    assert_eq!(child.follow_limit, Some(3));
    assert_eq!(child.scale3, [0.55; 3]);
    assert_eq!(child.count_scale, 0.2);
    let mut current = &system;
    let mut depth = 1;
    while let Some(child) = current.children.first() {
        depth += 1;
        current = child;
    }
    assert_eq!(depth, 8);
}

#[test]
fn following_child_leaves_world_trails_and_stops_emitting_when_parent_expires() {
    let mut parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    parent.max_count = 1;
    parent.initializers.push(Initializer::Velocity {
        min: [60.0, 0.0, 0.0],
        max: [60.0, 0.0, 0.0],
        exponent: 1.0,
    });
    let mut child = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    child.world = true;
    child.follow_limit = Some(1);
    child.max_count = 20;
    child.emitters =
        vec![parse_emitter(&serde_json::json!({"name":"boxrandom","rate":10})).unwrap()];
    child.initializers = vec![Initializer::Lifetime { min: 2.0, max: 2.0, exponent: 1.0 }];
    let mut source = Sim::new(1);
    let mut output = Sim::new(2);
    let mut followers = Followers::default();
    for _ in 0..90 {
        source.step(&parent, 1.0 / 15.0);
        followers.step(&mut output, &child, (&parent, &source), 1.0 / 15.0, (&[], &[]));
    }
    assert!((18..=20).contains(&output.particles.len()));
    let head = source.particles[0].pos[0];
    let tail =
        output.particles.iter().map(|particle| particle.pos[0]).fold(f32::INFINITY, f32::min);
    assert!((head - tail - 120.0).abs() < 12.0, "{head} {tail}");
    source.particles.clear();
    for _ in 0..40 {
        followers.step(&mut output, &child, (&parent, &source), 1.0 / 15.0, (&[], &[]));
    }
    assert!(output.particles.is_empty());
}

#[test]
fn local_following_children_move_with_parent_and_respect_instance_limit() {
    let mut parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    parent.max_count = 3;
    parent.initializers.push(Initializer::Velocity {
        min: [60.0, 0.0, 0.0],
        max: [60.0, 0.0, 0.0],
        exponent: 1.0,
    });
    let mut child = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    child.follow_limit = Some(1);
    child.max_count = 1;
    let mut source = Sim::new(1);
    let mut output = Sim::new(2);
    let mut followers = Followers::default();
    for _ in 0..90 {
        source.step(&parent, 1.0 / 15.0);
        followers.step(&mut output, &child, (&parent, &source), 1.0 / 15.0, (&[], &[]));
    }
    assert_eq!(source.particles.len(), 3);
    assert_eq!(output.particles.len(), 1);
    assert_eq!(output.particles[0].pos, source.particles[0].pos);
}

#[test]
fn attraction_matches_proton_motion_in_two_and_three_dimensions() {
    for (z, samples) in [
        (0.0, [128.0, 140.0, 146.0, 144.5, 137.0, 123.0, 103.0]),
        (100.0, [113.5, 128.5, 139.0, 145.0, 146.0, 143.5, 136.5]),
    ] {
        let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        sys.max_count = 1;
        sys.control_points[1].offset = [100.0, 0.0, z];
        sys.operators.push(parse_operator(&serde_json::json!({"name":"controlpointattract","controlpoint":1,"scale":30,"threshold":500})).unwrap());
        let mut sim = Sim::new(1);
        let mut frame = 0;
        for (reference, expected) in
            [181.0_f32, 206.0, 233.0, 258.0, 282.0, 307.5, 333.0].into_iter().zip(samples)
        {
            let target = ((reference - 80.0) / 20.0 * 15.0).round() as usize;
            while frame < target {
                sim.step(&sys, 1.0 / 15.0);
                frame += 1;
            }
            let actual = 80.0 + sim.particles[0].pos[0] / 3.0;
            assert!((actual - expected).abs() < 2.0, "z={z} frame={frame}: {actual} != {expected}");
        }
    }
}

#[test]
fn attraction_ignores_unknown_origin_and_defaults_to_engine_radius() {
    let mut sys = single(
        parse_operator(&serde_json::json!({"name":"controlpointattract","origin":"100 0 0"}))
            .unwrap(),
    );
    let mut sim = Sim::new(1);
    sim.step(&sys, 1.0 / 15.0);
    assert_eq!(sim.particles[0].vel, [0.0; 3]);
    sys.control_points[0].offset = [0.0, 0.0, 100.0];
    sim.step(&sys, 1.0 / 15.0);
    assert!(sim.particles[0].vel[2] > 0.0);
    let points = control_points(&serde_json::json!({"controlpoint":[{"id":7,"offset":"100 0 0"}]}));
    assert_eq!(points[0].offset, [100.0, 0.0, 0.0]);
    assert_eq!(points[7].offset, [0.0; 3]);
}

#[test]
fn attraction_deletes_particles_that_cross_the_control_point() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.initializers.push(Initializer::Velocity {
        min: [600.0, 0.0, 0.0],
        max: [600.0, 0.0, 0.0],
        exponent: 1.0,
    });
    sys.control_points[1].offset = [5.0, 0.0, 0.0];
    sys.operators.push(parse_operator(&serde_json::json!({"name":"controlpointattract","controlpoint":1,"scale":0,"flags":1,"deletethreshold":1})).unwrap());
    let mut sim = Sim::new(1);
    sim.step(&sys, 1.0 / 60.0);
    assert!(sim.particles.is_empty());
}

#[test]
fn turbulent_velocity_matches_proton_direction_and_ignores_emission_position() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.max_count = 1;
    sys.initializers.push(
        parse_initializer(&serde_json::json!({
            "name":"turbulentvelocityrandom","speedmin":30,"speedmax":30,
            "forward":"1 0 0","right":"0 0 1","scale":0.5,
            "phasemin":50000,"phasemax":50000,"timescale":0.00001
        }))
        .unwrap(),
    );
    let mut velocities = Vec::new();
    for y in [0.0, 100.0] {
        sys.emitters = vec![point_emitter([0.0, y, 0.0], 10.0, 0)];
        let mut sim = Sim::new(1);
        sim.step(&sys, 1.0 / 15.0);
        let velocity = sim.particles[0].vel;
        assert!((velocity[0] - 23.05).abs() < 0.3, "{velocity:?}");
        assert!((velocity[1] - 18.87).abs() < 0.3, "{velocity:?}");
        velocities.push(velocity);
    }
    assert_eq!(velocities[0], velocities[1]);
}

#[test]
fn omitted_fade_and_change_settings_match_we_lifetime_envelopes() {
    for (name, life, expected) in [
        ("alphafade", 0.25, 0.5),
        ("alphafade", 0.5, 1.0),
        ("alphafade", 0.75, 0.5),
        ("alphachange", 0.25, 0.75),
        ("sizechange", 0.75, 0.25),
        ("colorchange", 0.75, 0.25),
    ] {
        let sys = single(parse_operator(&serde_json::json!({"name":name})).unwrap());
        let mut sim = spawn_one(&sys);
        sim.particles[0].age = life * 100.0 - 0.01;
        sim.step(&sys, 0.01);
        let p = &sim.particles[0];
        let value = match name {
            "sizechange" => p.size / p.base_size,
            "colorchange" => p.color[0],
            _ => p.alpha / p.base_alpha,
        };
        assert!((value - expected).abs() < 1e-5, "{name} at {life}: {value}");
    }
}

#[test]
fn overlapping_fade_preserves_we_simd_mask_combination() {
    let sys = single(Operator::AlphaFade { fade_in: 0.6, fade_out: 0.1 });
    let mut sim = spawn_one(&sys);
    sim.particles[0].base_alpha = 0.25;
    sim.particles[0].age = 25.0 - 0.01;
    sim.step(&sys, 0.01);
    assert!((sim.particles[0].alpha - 0.41666666).abs() < 1e-5);
    sim.particles[0].age = 70.0 - 0.01;
    sim.step(&sys, 0.01);
    assert!((sim.particles[0].alpha - 0.08333333).abs() < 1e-5);
}

#[test]
fn omitted_oscillator_ranges_use_independent_we_defaults() {
    for (name, frequency, scale) in [
        ("oscillatealpha", 10.0, (0.0, 1.0)),
        ("oscillatesize", 10.0, (0.8, 1.2)),
        ("oscillateposition", 5.0, (0.0, 10.0)),
    ] {
        let op = parse_operator(&serde_json::json!({"name":name,"frequencymin":2})).unwrap();
        let (Operator::OscillateAlpha(osc)
        | Operator::OscillateSize(osc)
        | Operator::OscillatePosition(osc)) = op
        else {
            unreachable!()
        };
        assert_eq!(osc.frequency, (2.0, frequency));
        assert_eq!(osc.scale, scale);
        assert_eq!(osc.phase, (0.0, std::f32::consts::TAU));
        assert_eq!(osc.mask, [1.0, 1.0, 0.0]);
    }
    let sys = single(parse_operator(&serde_json::json!({"name":"oscillatesize"})).unwrap());
    let mut sim = spawn_one(&sys);
    for _ in 0..600 {
        sim.step(&sys, 1.0 / 60.0);
        let p = &sim.particles[0];
        assert!((0.8..=1.2).contains(&(p.size / p.base_size)));
    }
}
