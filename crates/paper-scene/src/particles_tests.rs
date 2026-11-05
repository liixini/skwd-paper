use super::*;

fn parse_initializer(entry: &Value) -> Option<Initializer> {
    super::parse_initializer(entry, ParticleDefaults::ORTHOGRAPHIC)
}

fn parse_operator(entry: &Value) -> Option<Operator> {
    super::parse_operator(entry, ParticleDefaults::ORTHOGRAPHIC)
}

#[test]
fn projection_selects_defaults() {
    let cases = [
        (serde_json::json!(null), false),
        (serde_json::json!({"auto":false}), false),
        (serde_json::json!({"auto":true}), true),
        (serde_json::json!({"auto":1}), false),
        (serde_json::json!({"auto":"true"}), false),
        (serde_json::json!({"width":100}), false),
        (serde_json::json!({"height":100}), false),
        (serde_json::json!({"width":0,"height":100}), false),
        (serde_json::json!({"width":100,"height":0}), false),
        (serde_json::json!({"width":100,"height":200}), true),
        (serde_json::json!({"width":-1,"height":200}), true),
        (serde_json::json!({"width":true,"height":200}), false),
    ];
    for (projection, ordinary) in cases {
        let scene = serde_json::json!({"general":{"orthogonalprojection":projection}});
        let defaults = ParticleDefaults::of(&scene);
        assert_eq!(defaults.cap_speed == 100.0, ordinary);
    }
    assert_eq!(ParticleDefaults::of(&serde_json::json!({})).cap_speed, 1.0);
}

#[test]
fn projection_converts_dimensions() {
    for (width, ordinary) in [
        (serde_json::json!(0.5), false),
        (serde_json::json!(-0.5), false),
        (serde_json::json!(1.9), true),
        (serde_json::json!(-1.9), true),
        (serde_json::json!(4_294_967_296_u64), false),
        (serde_json::json!(4_294_967_297_u64), true),
        (serde_json::json!(-4_294_967_296_i64), false),
    ] {
        let scene =
            serde_json::json!({"general":{"orthogonalprojection":{"width":width,"height":1}}});
        assert_eq!(ParticleDefaults::of(&scene).cap_speed == 100.0, ordinary);
    }
}

#[test]
fn projection_operator_defaults() {
    for (defaults, speed, distance, origin, radius) in [
        (ParticleDefaults::ORTHOGRAPHIC, 100.0, -150.0, [0.0, -200.0, 0.0], 50.0),
        (ParticleDefaults::OTHER, 1.0, 0.0, [0.0; 3], 1.0),
    ] {
        let Some(Operator::CapVelocity { max_speed, .. }) =
            super::parse_operator(&serde_json::json!({"name":"capvelocity"}), defaults)
        else {
            panic!("cap")
        };
        assert_eq!(max_speed, speed);
        let Some(Operator::CollisionPlane { distance: actual, .. }) =
            super::parse_operator(&serde_json::json!({"name":"collisionplane"}), defaults)
        else {
            panic!("plane")
        };
        assert_eq!(actual, distance);
        let Some(Operator::CollisionSphere { origin: actual, radius: size, .. }) =
            super::parse_operator(&serde_json::json!({"name":"collisionsphere"}), defaults)
        else {
            panic!("sphere")
        };
        assert_eq!((actual, size), (origin, radius));
    }
}

#[test]
fn authored_values_override_defaults() {
    for defaults in [ParticleDefaults::ORTHOGRAPHIC, ParticleDefaults::OTHER] {
        let Some(Operator::CapVelocity { max_speed, .. }) = super::parse_operator(
            &serde_json::json!({"name":"capvelocity","maxspeed":0}),
            defaults,
        ) else {
            panic!("cap")
        };
        assert_eq!(max_speed, 0.0);
        let Some(Operator::CollisionPlane { distance, .. }) = super::parse_operator(
            &serde_json::json!({"name":"collisionplane","distance":0}),
            defaults,
        ) else {
            panic!("plane")
        };
        assert_eq!(distance, 0.0);
        let Some(Operator::CollisionSphere { origin, radius, .. }) = super::parse_operator(
            &serde_json::json!({"name":"collisionsphere","origin":"1 2 3","radius":0}),
            defaults,
        ) else {
            panic!("sphere")
        };
        assert_eq!((origin, radius), ([1.0, 2.0, 3.0], 0.0));
    }
}

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
        sim.step_with_audio(&sys, 0.25, &bass, &bass, &mut |_| {});
    }
    assert_eq!(sim.particles.len(), 40);
    let age = sim.particles[0].age;
    sim.step_with_audio(&sys, 0.0, &bass, &bass, &mut |_| {});
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
    sim.step_with_audio(&sys, 0.25, &bass, &[], &mut |_| {});
    assert_eq!(sim.particles.len(), 10);
    assert!(sim.particles.iter().all(|particle| particle.pos[0] == -100.0));
}

#[test]
fn size_initializers_multiply() {
    for (sizes, scale, expected) in [
        (vec![], 1.0, 1.0),
        (vec![2.0], 1.0, 2.0),
        (vec![2.0, 3.0], 1.0, 6.0),
        (vec![], 2.0, 1.0),
        (vec![2.0, 3.0], 2.0, 24.0),
        (vec![2.0, 3.0], 0.5, 1.5),
    ] {
        let mut sys = system();
        sys.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
        sys.initializers.retain(|init| !matches!(init, Initializer::Size { .. }));
        sys.initializers.extend(sizes.into_iter().map(|size| Initializer::Size {
            min: size,
            max: size,
            exponent: 1.0,
        }));
        sys.size_scale = scale;
        let mut sim = Sim::new(7);
        sim.step(&sys, 0.0);
        assert_eq!(sim.particles.len(), 1);
        assert_eq!(sim.particles[0].size, expected);
        assert_eq!(sim.particles[0].base_size, expected);
        sim.step(&sys, 0.01);
        assert_eq!(sim.particles[0].size, expected);
    }
}

#[test]
fn scripted_size_births() {
    let mut sys = system();
    sys.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
    sys.authored_size_scale = 2.0;
    sys.size_scale = 0.5;

    let mut automatic = Sim::new(7);
    automatic.step(&sys, 0.0);
    assert_eq!(automatic.particles[0].size, 2.5);

    let mut manual = Sim::new(7);
    manual.stop();
    manual.emit_particles(&sys, 1, &mut |_| {});
    assert_eq!(manual.particles[0].size, 10.0);
}

pub(super) fn system() -> ParticleSystem {
    ParticleSystem {
        model_collisions: super::model_collision::Bindings::default(),
        image_dependencies: Vec::new(),
        children: Vec::new(),
        has_death_children: false,
        child_event: None,
        renderer: Renderer::Sprite,
        orientation: super::Orientation::Screen,
        renderer_flags: 0,
        extra_renderers: Vec::new(),
        extra_passes: Vec::new(),
        animation: Animation::Sequence,
        sequence_multiplier: 1.0,
        angular_motion: false,
        remapped_points: 0,
        trail: Trail {
            length: 0.0,
            min_length: 0.0,
            max_length: 100.0,
            segments: 4,
            fade_alpha: true,
            fade_size: false,
            uv_scroll: false,
            uv_scale: 1.0,
            subdivision: 0,
        },
        max_count: 64,
        start_time: 0.0,
        control_points: [ControlPoint::default(); 8],
        emitters: vec![Emitter::Box {
            audio: None,
            control_point: None,
            instantaneous: 0,
            timing: EmitterTiming::default(),
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
        spatial: None,
        world_matrix: None,
        scale: 1.0,
        scale3: [1.0; 3],
        base_color: [1.0; 3],
        alpha: 1.0,
        count_scale: 1.0,
        rate_scale: 1.0,
        size_scale: 1.0,
        authored_size_scale: 1.0,
        grab_slot: None,
        world: false,
        instance: super::InstanceFactors::default(),
    }
}

#[test]
fn zero_delta_initializes_particles_once_and_preserves_ribbon_history() {
    let mut continuous = Sim::new(7);
    continuous.step(&system(), 0.0);
    assert!(continuous.particles.is_empty());
    let mut sys = system();
    sys.renderer = Renderer::Ribbon;
    sys.emitters = vec![Emitter::Box {
        audio: None,
        control_point: None,
        instantaneous: 2,
        timing: EmitterTiming::default(),
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
fn new_birth_stays_zero() {
    let sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let sim = spawn_one(&sys);
    assert_eq!(sim.particles[0].age, 0.0);
}

#[test]
fn emitter_duration_stops_births() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"boxrandom","rate":20,"duration":0.1,"distancemax":"0 0 0"
        }))
        .unwrap(),
    ];
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.05);
    sim.step(&sys, 0.05);
    let count = sim.particles.len();
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), count);
}

#[test]
fn emitter_delay_waits_tick() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"boxrandom","rate":20,"delay":0.08,"duration":0.1
        }))
        .unwrap(),
    ];
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.05);
    sim.step(&sys, 0.05);
    assert!(sim.particles.is_empty());
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), 1);
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), 2);
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), 2);
}

#[test]
fn emitter_timer_outlasts_capacity() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"boxrandom","rate":20,"duration":0.1
        }))
        .unwrap(),
    ];
    sys.max_count = 1;
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.05);
    sim.step(&sys, 0.05);
    sim.particles.clear();
    sim.step(&sys, 0.05);
    assert!(sim.particles.is_empty());
}

#[test]
fn emitter_pause_preserves_duration() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"boxrandom","rate":20,"duration":0.1
        }))
        .unwrap(),
    ];
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.05);
    sim.emitting = false;
    sim.step(&sys, 0.05);
    sim.step(&sys, 0.05);
    sim.emitting = true;
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), 2);
}

#[test]
fn emitter_pause_preserves_instant() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"boxrandom","rate":0,"delay":0.05,"instantaneous":2
        }))
        .unwrap(),
    ];
    let mut sim = Sim::new(17);
    sim.emitting = false;
    sim.step(&sys, 0.05);
    sim.emitting = true;
    sim.step(&sys, 0.05);
    assert!(sim.particles.is_empty());
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), 2);
}

#[test]
fn playback_lifecycle() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles.len(), 1);
    assert!(sim.is_playing());

    sim.particles[0].vel = [10.0, 0.0, 0.0];
    sim.pause();
    sim.step(&sys, 0.1);
    assert!(!sim.is_playing());
    assert_eq!(sim.particles.len(), 1);
    assert!(sim.particles[0].pos[0] > 0.0);

    sim.stop();
    sim.step(&sys, 0.1);
    assert!(!sim.is_playing());
    assert!(sim.particles.is_empty());
    sim.play();
    sim.step(&sys, 0.01);
    assert!(sim.is_playing());
    assert_eq!(sim.particles.len(), 1);
}

#[test]
fn explicit_emission_after_stop() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
    sys.initializers[0] = Initializer::Lifetime { min: 0.2, max: 0.2, exponent: 1.0 };
    sys.max_count = 2;
    let mut sim = Sim::new(17);
    sim.stop();
    sim.emit_particles(&sys, 0, &mut |_| {});
    assert!(!sim.is_playing());
    assert!(sim.particles.is_empty());
    sim.emit_particles(&sys, 1, &mut |_| {});
    assert!(sim.is_playing());
    assert_eq!(sim.particles.len(), 1);
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles.len(), 1);
    sim.emit_particles(&sys, 2, &mut |_| {});
    assert_eq!(sim.particles.len(), 2);
    sim.step(&sys, 0.25);
    assert!(sim.particles.is_empty());
    assert!(!sim.is_playing());
    sim.emit_particles(&sys, 1, &mut |_| {});
    assert_eq!(sim.particles.len(), 1);
    assert!(sim.is_playing());
}

#[test]
fn manual_visits_emitters() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters =
        vec![point_emitter([-10.0, 0.0, 0.0], 0.0, 0), point_emitter([10.0, 0.0, 0.0], 0.0, 0)];
    let mut sim = Sim::new(17);
    sim.stop();
    sim.emit_particles(&sys, 1, &mut |_| {});
    assert_eq!(
        sim.particles.iter().map(|particle| particle.pos[0]).collect::<Vec<_>>(),
        [-10.0, 10.0]
    );
    assert!(sim.is_playing());
}

#[test]
fn manual_respects_capacity() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters =
        vec![point_emitter([-10.0, 0.0, 0.0], 0.0, 0), point_emitter([10.0, 0.0, 0.0], 0.0, 0)];
    sys.max_count = 1;
    let mut sim = Sim::new(17);
    sim.emit_particles(&sys, 1, &mut |_| {});
    assert_eq!(sim.particles.len(), 1);
    assert_eq!(sim.particles[0].pos[0], -10.0);
    assert_eq!(sim.emitter_timers[1].pending, 0.0);
}

#[test]
fn manual_debits_rate() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![point_emitter([0.0; 3], 4.0, 0)];
    let mut sim = Sim::new(17);
    sim.emit_particles(&sys, 1, &mut |_| {});
    assert_eq!(sim.emitter_timers[0].pending, -1.0);
    sim.step(&sys, 0.25);
    assert_eq!(sim.particles.len(), 1);
    sim.step(&sys, 0.25);
    assert_eq!(sim.particles.len(), 2);
}

#[test]
fn multi_automatic_rates() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters =
        vec![point_emitter([-10.0, 0.0, 0.0], 1.0, 0), point_emitter([10.0, 0.0, 0.0], 1.0, 0)];
    let mut sim = Sim::new(17);
    for _ in 0..8 {
        sim.step(&sys, 0.25);
    }
    assert_eq!(sim.particles.iter().filter(|particle| particle.pos[0] == -10.0).count(), 2);
    assert_eq!(sim.particles.iter().filter(|particle| particle.pos[0] == 10.0).count(), 2);
}

#[test]
fn count_preserves_bursts() {
    for count in [0.0, 0.25, 0.5, 1.0, 2.0, 3.0] {
        for emitters in [1, 2] {
            let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
            sys.count_scale = count;
            sys.emitters = (0..emitters).map(|_| point_emitter([0.0; 3], 0.0, 2)).collect();
            let mut automatic = Sim::new(17);
            automatic.step(&sys, 0.0);
            assert_eq!(automatic.particles.len(), emitters * 2);
            let mut manual = Sim::new(17);
            manual.emit_particles(&sys, 1, &mut |_| {});
            assert_eq!(manual.particles.len(), emitters * 3);
        }
    }
}

#[test]
fn count_scales_paired_rates() {
    for (count, expected) in [(0.5, 1), (1.0, 2), (2.0, 4)] {
        let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        sys.count_scale = count;
        sys.emitters =
            vec![point_emitter([-10.0, 0.0, 0.0], 1.0, 0), point_emitter([10.0, 0.0, 0.0], 1.0, 0)];
        let mut sim = Sim::new(17);
        for _ in 0..8 {
            sim.step(&sys, 0.25);
        }
        for x in [-10.0, 10.0] {
            assert_eq!(
                sim.particles.iter().filter(|particle| particle.pos[0] == x).count(),
                expected
            );
        }
    }
}

#[test]
fn count_mixed_accrual() {
    for (count, expected) in [(0.5, [2, 3]), (1.0, [5, 5]), (2.0, [10, 10])] {
        let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        sys.count_scale = count;
        sys.emitters =
            vec![point_emitter([-10.0, 0.0, 0.0], 1.0, 2), point_emitter([10.0, 0.0, 0.0], 1.0, 2)];
        for (manual, want) in expected.into_iter().enumerate() {
            let mut sim = Sim::new(17);
            sim.step(&sys, 0.125);
            sim.step(&sys, 0.125);
            sim.emit_particles(&sys, manual as u32, &mut |_| {});
            for _ in 0..39 {
                sim.step(&sys, 0.125);
            }
            for x in [-10.0, 10.0] {
                assert_eq!(
                    sim.particles.iter().filter(|particle| particle.pos[0] == x).count(),
                    want
                );
            }
        }
    }
}

#[test]
fn multi_manual_keeps_fraction() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters =
        vec![point_emitter([-10.0, 0.0, 0.0], 1.0, 0), point_emitter([10.0, 0.0, 0.0], 1.0, 0)];
    for (manual, steps, expected) in [(1, 7, 2), (4, 19, 5)] {
        let mut sim = Sim::new(17);
        sim.step(&sys, 0.25);
        assert_eq!(
            sim.emitter_timers.iter().map(|timer| timer.pending).collect::<Vec<_>>(),
            [0.25, 0.25]
        );
        sim.emit_particles(&sys, manual, &mut |_| {});
        for _ in 0..steps {
            sim.step(&sys, 0.25);
        }
        assert_eq!(
            sim.particles.iter().filter(|particle| particle.pos[0] == -10.0).count(),
            expected
        );
        assert_eq!(
            sim.particles.iter().filter(|particle| particle.pos[0] == 10.0).count(),
            expected
        );
    }
}

#[test]
fn stopped_birth_keeps_pause() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![point_emitter([0.0; 3], 4.0, 0)];
    let mut sim = Sim::new(17);
    sim.pause();
    sim.stop();
    sim.emit_particles(&sys, 1, &mut |_| {});
    assert_eq!(sim.particles.len(), 1);
    assert!(!sim.is_playing());
    assert_eq!(sim.emitter_timers[0].pending, 0.0);
    sim.play();
    assert!(sim.is_playing());
    sim.step(&sys, 0.25);
    assert_eq!(sim.particles.len(), 1);
}

#[test]
fn emitter_instant_debits_rate() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"boxrandom","rate":20,"duration":1,"instantaneous":2
        }))
        .unwrap(),
    ];
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), 3);
    sim.step(&sys, 0.05);
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), 3);
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), 4);
}

#[test]
fn emitter_debt_survives_capacity() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"boxrandom","rate":20,"duration":1,"instantaneous":2
        }))
        .unwrap(),
    ];
    sys.max_count = 1;
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), 1);
    sim.particles.clear();
    sim.step(&sys, 0.05);
    assert!(sim.particles.is_empty());
}

#[test]
fn emitter_selection_skips_delay() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = [
        serde_json::json!({
            "name":"boxrandom","rate":20,"delay":0.1,"origin":"100 0 0"
        }),
        serde_json::json!({
            "name":"boxrandom","rate":20,"origin":"-100 0 0"
        }),
    ]
    .iter()
    .map(|entry| parse_emitter(entry).unwrap())
    .collect();
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), 1);
    assert_eq!(sim.particles[0].pos[0], -100.0);
}

#[test]
fn emitter_negative_keeps_instant() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"boxrandom","rate":20,"duration":-1,"instantaneous":2
        }))
        .unwrap(),
    ];
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), 2);
    assert_eq!(sim.emitter_timers[0].pending, -1.0);
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), 2);
}

#[test]
fn emitter_defaults_are_unlimited() {
    let absent = parse_emitter(&serde_json::json!({"name":"boxrandom","rate":20})).unwrap();
    let explicit =
        parse_emitter(&serde_json::json!({"name":"boxrandom","rate":20,"duration":0})).unwrap();
    assert_eq!(absent.timing().duration, 0.0);
    assert_eq!(explicit.timing().duration, 0.0);
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![explicit];
    let mut sim = Sim::new(17);
    for _ in 0..4 {
        sim.step(&sys, 0.05);
    }
    assert!(sim.particles.len() >= 4);
}

#[test]
fn emitter_timers_parse_numbers() {
    let emitter = parse_emitter(&serde_json::json!({
        "name":"boxrandom","delay":true,"duration":0.5
    }))
    .unwrap();
    assert_eq!(emitter.timing().delay, 1.0);
    assert_eq!(emitter.timing().duration, 0.5);
    let invalid = parse_emitter(&serde_json::json!({
        "name":"boxrandom","delay":"1","duration":"0.5"
    }))
    .unwrap();
    assert_eq!(invalid.timing().delay, 0.0);
    assert_eq!(invalid.timing().duration, 0.0);
}

#[test]
fn periodic_fields_parse() {
    let doc = serde_json::json!({"emitter":[{
        "name":"boxrandom","flags":4,"rate":20,
        "minperiodicdelay":0.1,"maxperiodicdelay":0.1,
        "minperiodicduration":0.1,"maxperiodicduration":0.1,
        "maxtoemitperperiod":2
    }]});
    let mut skipped = Vec::new();
    let parsed = parse_steps(&doc, "emitter", "test", parse_emitter, &mut skipped);
    assert_eq!(parsed.len(), 1);
    assert!(skipped.is_empty());
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = parsed;
    let mut sim = Sim::new(17);
    let mut counts = Vec::new();
    for _ in 0..8 {
        sim.step(&sys, 0.05);
        counts.push(sim.particles.len());
    }
    assert_eq!(counts, [1, 2, 2, 2, 3, 4, 4, 4]);
}

#[test]
fn periodic_crosses_cycles() {
    let emitter = parse_emitter(&serde_json::json!({
        "name":"boxrandom","rate":20,"flags":4,
        "minperiodicdelay":0.1,"maxperiodicdelay":0.1,
        "minperiodicduration":0.1,"maxperiodicduration":0.1,
        "maxtoemitperperiod":2
    }))
    .unwrap();
    let mut timer = EmitterTimer::new(&emitter);
    let mut rng = Rng::new(17);
    assert_eq!(timer.births(&emitter, 0.55, 1.0, &mut rng, (&[], &[]), true), 6);
    assert_eq!(timer.births(&emitter, 0.05, 1.0, &mut rng, (&[], &[]), true), 0);
    assert_eq!(timer.births(&emitter, 0.1, 1.0, &mut rng, (&[], &[]), true), 2);
}

#[test]
fn periodic_restarts_after_stop() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"boxrandom","rate":20,"flags":4,
            "minperiodicdelay":0.1,"maxperiodicdelay":0.1,
            "minperiodicduration":0.1,"maxperiodicduration":0.1
        }))
        .unwrap(),
    ];
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), 1);
    sim.stop();
    sim.play();
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles.len(), 1);
}

#[test]
fn frame_caps_rate() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"boxrandom","flags":2,"rate":1000
        }))
        .unwrap(),
    ];
    let mut sim = Sim::new(17);
    for count in 1..=4 {
        sim.step(&sys, 0.05);
        assert_eq!(sim.particles.len(), count);
    }
    let unset = parse_emitter(&serde_json::json!({
        "name":"boxrandom","flags":1,"rate":1000
    }))
    .unwrap();
    assert!(!unset.timing().one_per_frame);
}

#[test]
fn position_initializers_replace_origin() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"boxrandom","origin":"100 100 100","instantaneous":1,"rate":0
        }))
        .unwrap(),
    ];
    for (entry, limit) in [
        (serde_json::json!({"name":"box","min":"1 2 3","max":"1 2 3"}), 0.0),
        (serde_json::json!({"name":"sphere","min":4,"max":4}), 4.0),
    ] {
        sys.initializers = vec![parse_initializer(&entry).unwrap()];
        let mut sim = Sim::new(17);
        sim.step(&sys, 0.05);
        let position = sim.particles[0].pos;
        if limit == 0.0 {
            assert_eq!(position, [1.0, 2.0, 3.0]);
        } else {
            let length = dot3(position, position).sqrt();
            assert!((length - limit).abs() < 1e-5);
        }
    }
}

#[test]
fn rope_kinds_track_history() {
    for (name, kind, history) in
        [("rope", Renderer::Rope, false), ("ropetrail", Renderer::Ribbon, true)]
    {
        let definition = serde_json::json!({
            "renderer":[{"name":name}],
            "emitter":[{"name":"boxrandom","instantaneous":2,"rate":0}],
            "initializer":[{"name":"lifetimerandom","min":1,"max":1}]
        })
        .to_string();
        let pkg =
            Package::parse(crate::tests::build_pkg(&[("particles/p.json", definition.as_bytes())]))
                .unwrap();
        let assets = crate::effects::Assets::discover(Some("/nonexistent"));
        let context = LoadContext {
            pkg: &pkg,
            assets: &assets,
            defaults: ParticleDefaults::ORTHOGRAPHIC,
            viewport: None,
            bounds_static: false,
            version: 0,
        };
        let system = load(&context, &serde_json::json!({}), "particles/p.json").0.unwrap();
        assert!(system.renderer == kind);
        let mut sim = Sim::new(17);
        sim.step(&system, 0.05);
        assert_eq!(sim.history.len() == 2, history);
    }
}

#[test]
fn sprite_trail_limits() {
    let defaults = ParticleRender::parse(&serde_json::json!({"name":"spritetrail"})).unwrap();
    assert_eq!(defaults.trail.max_length, 10.0);
    let limits = ParticleRender::parse(&serde_json::json!({
        "name":"spritetrail","length":0.02,"minlength":12,"maxlength":2
    }))
    .unwrap();
    assert_eq!(
        (limits.trail.length, limits.trail.min_length, limits.trail.max_length),
        (0.02, 12.0, 2.0)
    );
    let rope = ParticleRender::parse(&serde_json::json!({"name":"ropetrail"})).unwrap();
    assert_eq!(rope.trail.max_length, 100.0);
}

#[test]
fn ribbon_shared_settings() {
    let mut first = ParticleRender::parse(&serde_json::json!({
        "name":"ropetrail", "length":0.2, "segments":8, "uvscale":2,
        "uvscrolling":true, "fadealpha":true
    }))
    .unwrap();
    let last = ParticleRender::parse(&serde_json::json!({
        "name":"ropetrail", "length":1.5, "segments":4, "uvscale":0, "fadesize":true
    }))
    .unwrap();
    let mut extras = [ParticleRender::SPRITE, last];
    share_trails(&mut first, &mut extras);
    for render in [first, extras[1]] {
        assert_eq!((render.trail.length, render.trail.segments), (1.5, 4));
        assert_eq!(render.trail.uv_factor(), 0.5);
        assert!(render.trail.uv_scroll);
    }
    assert!(first.trail.fade_alpha && !first.trail.fade_size);
    assert!(!extras[1].trail.fade_alpha && extras[1].trail.fade_size);
    assert!(extras[0].renderer == Renderer::Sprite);
}

#[test]
fn ribbon_uv_coordinates() {
    let mut sys = system();
    sys.renderer = Renderer::Ribbon;
    sys.trail.length = 1.0;
    sys.trail.uv_scale = 2.0;
    let mut sim = spawn_one(&sys);
    let mut packed = Vec::new();
    pack_rope_trail_points(&sim, sys.primary_render(), &mut packed);
    assert_eq!((packed[7], packed[11]), (1.0, 0.0));
    sys.trail.uv_scroll = true;
    pack_rope_trail_points(&sim, sys.primary_render(), &mut packed);
    assert_eq!((packed[7], packed[11]), (0.0, -1.0));
    sim.history[0].sample_count = u16::MAX;
    sim.step(&sys, 0.25);
    pack_rope_trail_points(&sim, sys.primary_render(), &mut packed);
    assert_eq!(packed[11], 0.0);
}

#[test]
fn ribbon_native_defaults() {
    let defaults = ParticleRender::parse(&serde_json::json!({"name":"ropetrail"})).unwrap();
    assert_eq!(
        (defaults.trail.length, defaults.trail.segments, defaults.trail.subdivision),
        (1.0, 4, 1)
    );
    assert!(!defaults.trail.fade_alpha);
    for (authored, expected) in [(-1, 1), (0, 1), (6, 6), (64, 32)] {
        let render = ParticleRender::parse(&serde_json::json!({
            "name":"ropetrail", "segments":authored,"length":0,"maxlength":7,"subdivision":64
        }))
        .unwrap();
        assert_eq!(render.trail.segments, expected);
        assert_eq!((render.trail.length, render.trail.subdivision), (0.001, 32));
        let mut render = render;
        share_trails(&mut render, &mut []);
        assert_eq!(render.trail.segments, expected.max(2));
    }
}

#[test]
fn renderer_records_parse() {
    let definition = br#"{"renderer":[
        {"name":"sprite"},{"name":"sprite"},{"name":"ropetrail","length":0.2,"fadealpha":false},
        {"name":"unknown"}],"emitter":[{"name":"boxrandom","instantaneous":1}]}"#;
    let pkg = Package::parse(crate::tests::build_pkg(&[("particles/p.json", definition)])).unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let context = LoadContext {
        pkg: &pkg,
        assets: &assets,
        defaults: ParticleDefaults::ORTHOGRAPHIC,
        viewport: None,
        bounds_static: false,
        version: 0,
    };
    let (system, skipped) = load(&context, &serde_json::json!({}), "particles/p.json");
    let system = system.unwrap();
    assert_eq!(system.extra_renderers.len(), 2);
    assert!(system.extra_renderers[0].renderer == Renderer::Sprite);
    assert!(system.extra_renderers[1].renderer == Renderer::Ribbon);
    assert_eq!(system.extra_renderers[1].trail.length, 0.2);
    assert!(!system.extra_renderers[1].trail.fade_alpha);
    assert_eq!(skipped.len(), 1);
}

#[test]
fn expiry_equality_survives() {
    let sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let mut sim = spawn_one(&sys);
    sim.particles[0].lifetime = 1.0;
    sim.particles[0].age = 0.75;
    sim.step(&sys, 0.25);
    assert_eq!(sim.particles.len(), 1);
    assert_eq!(sim.particles[0].age, 1.0);
    sim.step(&sys, 0.01);
    assert!(sim.particles.is_empty());
}

#[test]
fn delete_retains_slot() {
    let mut sys = single(
        parse_operator(&serde_json::json!({
            "name":"collisionplane","plane":"0 1 0","distance":1,
            "collisionbehavior":"delete"
        }))
        .unwrap(),
    );
    sys.max_count = 1;
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles.len(), 1);
    assert_eq!(sim.particles[0].age, sim.particles[0].lifetime);
    let previous_id = sim.particles[0].id;
    sim.pending = 1.0;
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles.len(), 1);
    assert_ne!(sim.particles[0].id, previous_id);
    assert_eq!(sim.particles[0].age, sim.particles[0].lifetime);
}

#[test]
fn delete_runs_later_operators() {
    let mut sys = single(
        parse_operator(&serde_json::json!({
            "name":"collisionplane","plane":"0 1 0","distance":1,
            "collisionbehavior":"delete"
        }))
        .unwrap(),
    );
    sys.initializers.push(Initializer::Velocity {
        min: [10.0, 0.0, 0.0],
        max: [10.0, 0.0, 0.0],
        exponent: 1.0,
    });
    sys.operators.push(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles.len(), 1);
    assert_eq!(sim.particles[0].age, sim.particles[0].lifetime);
    assert!((sim.particles[0].pos[0] - 0.1).abs() < 1e-5);
}

fn deletion_order(envelope: Operator) -> (Particle, Particle) {
    let collision = parse_operator(&serde_json::json!({
        "name":"collisionplane","plane":"0 1 0","distance":1,
        "collisionbehavior":"delete"
    }))
    .unwrap();
    let mut sys = single(collision);
    sys.operators.push(envelope);
    let mut after = Sim::new(17);
    after.pending = 1.0;
    after.step(&sys, 0.01);
    sys.operators.swap(0, 1);
    let mut before = Sim::new(17);
    before.pending = 1.0;
    before.step(&sys, 0.01);
    (after.particles[0], before.particles[0])
}

#[test]
fn delete_fade_order() {
    let (after, before) = deletion_order(Operator::AlphaFade { fade_in: 0.0, fade_out: 0.5 });
    assert_eq!(after.alpha, 0.0);
    assert_eq!(before.alpha, 1.0);
}

#[test]
fn delete_alpha_order() {
    let (after, before) = deletion_order(Operator::AlphaChange {
        start: 1.0,
        end: 0.0,
        start_time: 0.0,
        end_time: 1.0,
    });
    assert_eq!(after.alpha, 0.0);
    assert_eq!(before.alpha, 1.0);
}

#[test]
fn delete_size_order() {
    let (after, before) = deletion_order(Operator::SizeChange {
        start: 1.0,
        end: 2.0,
        start_time: 0.0,
        end_time: 1.0,
    });
    assert_eq!(after.size, 10.0);
    assert_eq!(before.size, 5.0);
}

#[test]
fn delete_color_order() {
    let (after, before) = deletion_order(Operator::ColorChange {
        start: [1.0, 0.0, 0.0],
        end: [0.0, 0.0, 1.0],
        start_time: 0.0,
        end_time: 1.0,
    });
    assert_eq!(after.color, [0.0, 0.0, 1.0]);
    assert_eq!(before.color, [1.0, 0.0, 0.0]);
}

#[test]
fn color_change_keeps_initializer() {
    let mut sys = single(Operator::ColorChange {
        start: [1.0, 1.0, 0.0],
        end: [1.0, 1.0, 0.0],
        start_time: 0.0,
        end_time: 1.0,
    });
    sys.initializers.push(Initializer::Color {
        min: [0.0, 255.0, 255.0],
        max: [0.0, 255.0, 255.0],
        exponent: 1.0,
    });
    let sim = spawn_one(&sys);
    assert_eq!(sim.particles[0].color, [0.0, 1.0, 0.0]);
}

#[test]
fn color_changes_multiply() {
    let mut sys = single(Operator::ColorChange {
        start: [0.5, 1.0, 1.0],
        end: [0.5, 1.0, 1.0],
        start_time: 0.0,
        end_time: 1.0,
    });
    sys.operators.push(Operator::ColorChange {
        start: [1.0, 0.5, 1.0],
        end: [1.0, 0.5, 1.0],
        start_time: 0.0,
        end_time: 1.0,
    });
    let sim = spawn_one(&sys);
    assert_eq!(sim.particles[0].color, [0.5, 0.5, 1.0]);
}

#[test]
fn color_resets_each_step() {
    let sys = single(Operator::ColorChange {
        start: [0.5; 3],
        end: [0.5; 3],
        start_time: 0.0,
        end_time: 1.0,
    });
    let mut sim = spawn_one(&sys);
    assert_eq!(sim.particles[0].color, [0.5; 3]);
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles[0].color, [0.5; 3]);
    sim.step(&sys, 0.05);
    assert_eq!(sim.particles[0].color, [0.5; 3]);
}

#[test]
fn ribbon_retirement_aligns() {
    let mut sys = single(
        parse_operator(&serde_json::json!({
            "name":"collisionplane","plane":"0 1 0","distance":1,
            "collisionbehavior":"delete"
        }))
        .unwrap(),
    );
    sys.renderer = Renderer::Ribbon;
    sys.trail.length = 10.0;
    sys.emitters = vec![point_emitter([0.0; 3], 0.0, 2)];
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles.len(), 2);
    assert_eq!(sim.history.len(), 2);
    let surviving_id = sim.particles[1].id;
    sim.particles[1].age = 0.0;
    sim.particles[1].pos = [20.0, 2.0, 0.0];
    sim.history[0].samples[0].pos = [11.0, 0.0, 0.0];
    sim.history[1].samples[0].pos = [22.0, 0.0, 0.0];
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles.len(), 1);
    assert_eq!(sim.particles[0].id, surviving_id);
    assert_eq!(sim.history.len(), 1);
    let points = sim.history[0].points().collect::<Vec<_>>();
    assert_eq!(points[0].pos, [20.0, 2.0, 0.0]);
    assert_eq!(points[1].pos, [22.0, 0.0, 0.0]);
}

#[test]
fn ribbon_shared_clock() {
    let mut sys = system();
    sys.renderer = Renderer::Ribbon;
    sys.trail.length = 1.0;
    sys.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.0);
    sim.step(&sys, 0.1);
    sim.emit_particles(&sys, 1, &mut |_| {});
    sim.step(&sys, 0.1);
    assert_ne!(sim.history[0].samples[0].time, sim.history[1].samples[0].time);
    sim.step(&sys, 0.1);
    assert_eq!(sim.history[0].samples[0].time, sim.history[1].samples[0].time);
    assert_eq!(sim.history[0].phase(), sim.history[1].phase());
}

#[test]
fn ribbon_last_length() {
    for (first, last, samples) in [(1.0, 0.2, 3), (0.2, 1.0, 2)] {
        let mut sys = system();
        sys.renderer = Renderer::Ribbon;
        sys.trail.length = first;
        let mut render = sys.primary_render();
        render.trail.length = last;
        sys.extra_renderers.push(render);
        sys.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
        let mut sim = Sim::new(17);
        sim.step(&sys, 0.0);
        sim.step(&sys, 0.1);
        assert_eq!(sim.history[0].samples.len(), samples);
    }
}

#[test]
fn ribbon_segment_history() {
    let mut particle = spawn_one(&system()).particles[0];
    particle.pos = [0.0; 3];
    for segments in [4, 6, 32] {
        let mut history = TrailHistory::new(&particle, 0.0, segments + 1);
        history.record(&particle, 0.0, 1.5);
        let mut particle = particle;
        for frame in 1..=61 {
            let time = frame as f32 / 30.0;
            particle.pos[0] = time * 100.0;
            history.record(&particle, time, 1.5);
        }
        let points = history.points().collect::<Vec<_>>();
        let span = points[0].pos[0] - points.last().unwrap().pos[0];
        let expected = match segments {
            4 => 123.33333,
            6 => 150.0,
            _ => 203.33333,
        };
        assert!((span - expected).abs() < 0.01, "{segments}: {span}");
        assert!(points.len() <= segments + 1);
    }
}

#[test]
fn ribbon_points_pack_history() {
    let mut sys = system();
    sys.renderer = Renderer::Ribbon;
    sys.trail.length = 1.0;
    sys.trail.fade_alpha = false;
    sys.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.0);
    sim.particles[0].pos[0] = 10.0;
    sim.history[0].record(&sim.particles[0], 0.1, sys.trail.length);
    let mut packed = Vec::new();
    assert_eq!(pack_rope_trail_points(&sim, sys.primary_render(), &mut packed), sys.trail.segments);
    assert_eq!(packed.len(), ROPE_POINT_FLOATS * sys.trail.segments);
    assert_eq!(packed[0], 10.0);
    assert_eq!(packed[4], 0.0);
    assert_eq!(packed[7], 2.0);
    assert_eq!(packed[19], 1.0);
    assert_eq!(packed[23], 1.0);
    assert_eq!(packed[ROPE_POINT_FLOATS], 0.0);
    assert_eq!(packed[ROPE_POINT_FLOATS + 4], 0.0);
    assert_eq!(packed[ROPE_POINT_FLOATS + 8], 10.0);
    assert_eq!(packed[ROPE_POINT_FLOATS + 12], 0.0);
}

#[test]
fn ribbon_current_attributes() {
    let mut sys = system();
    sys.renderer = Renderer::Ribbon;
    sys.trail.fade_alpha = true;
    let mut sim = spawn_one(&sys);
    sim.particles[0].size = 20.0;
    sim.particles[0].alpha = 0.6;
    sim.particles[0].color = [0.25, 0.5, 0.75];
    let mut packed = Vec::new();
    assert_eq!(pack_rope_trail_points(&sim, sys.primary_render(), &mut packed), sys.trail.segments);
    assert_eq!(packed[3], 10.0);
    assert_eq!(packed[15], 10.0);
    assert_eq!(&packed[16..20], &[0.25, 0.5, 0.75, 0.6]);
    assert_eq!(&packed[20..24], &[0.25, 0.5, 0.75, 0.6]);
    assert_eq!(std::mem::size_of::<TrailPoint>(), 16);
}

#[test]
fn alpha_fades_late() {
    let mut sys = system();
    sys.emitters = vec![Emitter::Box {
        audio: None,
        control_point: None,
        instantaneous: 0,
        timing: EmitterTiming::default(),
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
        instantaneous: 1,
        timing: EmitterTiming::default(),
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
fn cap_velocity_limits_speed() {
    let sys = single(
        parse_operator(&serde_json::json!({
            "name": "capvelocity", "maxspeed": 100
        }))
        .unwrap(),
    );
    let mut sim = spawn_one(&sys);
    sim.particles[0].vel = [300.0, 400.0, 0.0];
    sim.step(&sys, 0.01);
    for (actual, expected) in sim.particles[0].vel.into_iter().zip([60.0, 80.0, 0.0]) {
        assert!((actual - expected).abs() < 0.05);
    }
}

#[test]
fn collision_defaults_parse() {
    let Some(Operator::CollisionPlane { normal, distance, collision }) =
        parse_operator(&serde_json::json!({"name":"collisionplane","controlpoint":7}))
    else {
        panic!("missing plane");
    };
    assert_eq!(normal, [0.0, 1.0, 0.0]);
    assert_eq!(distance, -150.0);
    assert!(matches!(collision.response, CollisionResponse::Bounce(0.5)));
    assert!(!collision.stop_angular);
    let Some(Operator::CollisionSphere { origin, radius, collision, .. }) = parse_operator(
        &serde_json::json!({"name":"collisionsphere","collisionbehavior":"unknown"}),
    ) else {
        panic!("missing sphere");
    };
    assert_eq!(origin, [0.0, -200.0, 0.0]);
    assert_eq!(radius, 50.0);
    assert!(matches!(collision.response, CollisionResponse::Bounce(0.5)));
    assert!(!collision.stop_angular);
    assert!(parse_operator(&serde_json::json!({"name":"collisionplane","flags":1})).is_none());
    let Some(Operator::CollisionPlane { collision, .. }) =
        parse_operator(&serde_json::json!({"name":"collisionplane","flags":2,"controlpoint":7}))
    else {
        panic!("missing angular stop");
    };
    assert!(collision.stop_angular);
    assert!(parse_operator(&serde_json::json!({"name":"collisionplane","flags":4})).is_some());
    let Some(Operator::CollisionPlane { normal, .. }) =
        parse_operator(&serde_json::json!({"name":"collisionplane","plane":"0 0 0"}))
    else {
        panic!("missing zero plane");
    };
    assert!(normal.into_iter().all(f32::is_nan));
}

#[test]
fn plane_projects_and_responds() {
    for (behavior, velocity) in
        [("bounce", [3.0, 2.0, 0.0]), ("slide", [3.0, 0.0, 0.0]), ("stop", [0.0, 0.0, 0.0])]
    {
        let sys = single(
            parse_operator(&serde_json::json!({
                "name":"collisionplane","plane":"0 2 0","distance":-5,
                "collisionbehavior":behavior
            }))
            .unwrap(),
        );
        let mut sim = spawn_one(&sys);
        sim.particles[0].pos = [2.0, -6.0, 0.0];
        sim.particles[0].vel = [3.0, -4.0, 0.0];
        sim.step(&sys, 0.01);
        assert_eq!(sim.particles[0].pos, [2.0, -5.0, 0.0]);
        assert_eq!(sim.particles[0].vel, velocity);
    }
    let sys = single(
        parse_operator(&serde_json::json!({
            "name":"collisionplane","plane":"0 1 0","distance":-5
        }))
        .unwrap(),
    );
    let mut sim = spawn_one(&sys);
    sim.particles[0].pos[1] = -6.0;
    sim.particles[0].vel[1] = 4.0;
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles[0].vel[1], -2.0);
    sim.particles[0].pos[1] = -5.0;
    sim.particles[0].vel[1] = 4.0;
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles[0].vel[1], 4.0);
    for (bounce, expected) in [(0.0, 0.0), (1.0, 4.0), (2.0, 8.0)] {
        let sys = single(
            parse_operator(&serde_json::json!({
                "name":"collisionplane","plane":"0 1 0","distance":-5,
                "bouncefactor":bounce
            }))
            .unwrap(),
        );
        let mut sim = spawn_one(&sys);
        sim.particles[0].pos[1] = -6.0;
        sim.particles[0].vel[1] = -4.0;
        sim.step(&sys, 0.01);
        assert_eq!(sim.particles[0].vel[1], expected);
    }
}

#[test]
fn sphere_edges_project() {
    for (radius, position, expected) in [(5.0, 3.0, 5.0), (-5.0, 3.0, -5.0)] {
        let sys = single(
            parse_operator(&serde_json::json!({
                "name":"collisionsphere","origin":"0 0 0","radius":radius,
                "collisionbehavior":"stop"
            }))
            .unwrap(),
        );
        let mut sim = spawn_one(&sys);
        sim.particles[0].pos = [position, 0.0, 0.0];
        sim.particles[0].vel = [1.0, 2.0, 0.0];
        sim.step(&sys, 0.01);
        assert!((sim.particles[0].pos[0] - expected).abs() < 0.01);
        assert_eq!(sim.particles[0].vel, [0.0; 3]);
    }
    let sys = single(
        parse_operator(&serde_json::json!({
            "name":"collisionsphere","origin":"0 0 0","radius":5,
            "collisionbehavior":"stop"
        }))
        .unwrap(),
    );
    let mut sim = spawn_one(&sys);
    sim.particles[0].pos = [5.0, 0.0, 0.0];
    sim.particles[0].vel = [1.0, 0.0, 0.0];
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles[0].pos, [5.0, 0.0, 0.0]);
    assert_eq!(sim.particles[0].vel, [1.0, 0.0, 0.0]);
    sim.particles[0].pos = [0.0; 3];
    sim.step(&sys, 0.01);
    assert!(sim.particles[0].pos.iter().all(|coordinate| coordinate.is_nan()));
}

#[test]
#[ignore = "requires Intel RSQRT reference profile"]
fn sphere_nonaxis_responses() {
    assert_eq!(native_rsqrt(169.0).to_bits(), 0x3d9d_8000);
    check_sphere_responses(
        [6.0, 8.0, 24.0],
        [8.822_028, -44.903_96, -4.711_887_4],
        [12.548_018, -39.935_974, 10.192_074],
    );
}

#[test]
fn collision_delete_expires() {
    let sys = single(
        parse_operator(&serde_json::json!({
            "name":"collisionplane","plane":"0 1 0","distance":-5,
            "collisionbehavior":"delete"
        }))
        .unwrap(),
    );
    let mut sim = spawn_one(&sys);
    sim.particles[0].pos = [2.0, -6.0, 0.0];
    sim.particles[0].vel = [1.0, -2.0, 0.0];
    let mut particle = sim.particles[0];
    collision_response(
        &mut particle,
        [0.0, 1.0, 0.0],
        -1.0,
        Collision { response: CollisionResponse::Delete, stop_angular: false },
    );
    assert_eq!(particle.age, particle.lifetime);
    assert_eq!(particle.pos, [2.0, -6.0, 0.0]);
    assert_eq!(particle.vel, [1.0, -2.0, 0.0]);
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles.len(), 1);
    assert_eq!(sim.particles[0].age, sim.particles[0].lifetime);
    sim.step(&sys, 0.01);
    assert!(sim.particles.is_empty());
}

#[test]
fn collision_respects_order() {
    for reverse in [false, true] {
        let movement = Operator::Movement { gravity: [0.0; 3], drag: 0.0 };
        let collision = parse_operator(&serde_json::json!({
            "name":"collisionplane","plane":"0 1 0","distance":-5
        }))
        .unwrap();
        let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        sys.operators = if reverse { vec![collision, movement] } else { vec![movement, collision] };
        let mut sim = spawn_one(&sys);
        sim.particles[0].pos[1] = -6.0;
        sim.particles[0].vel[1] = -10.0;
        sim.step(&sys, 0.01);
        let expected = if reverse { -4.95 } else { -5.0 };
        assert!((sim.particles[0].pos[1] - expected).abs() < 1e-5);
    }
}

const BLEND_TOLERANCE: f32 = 1.5 / 4096.0 * (0.55 / 0.1 + 2.0);

#[test]
fn oscillators_blend() {
    let entry = |name: &str| {
        serde_json::json!({"name":name, "frequencymin":0, "frequencymax":0, "scalemin":0.2,
            "scalemax":0.2, "blendinstart":0.5, "blendinend":0.6})
    };
    let base = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let mut particle = spawn_one(&base).particles[0];
    particle.age = 0.55;
    particle.lifetime = 1.0;
    particle.alpha = 1.0;
    particle.size = 1.0;
    let mut alpha = particle;
    let mut size = particle;
    let step = |particle: &mut Particle, name: &str| {
        let sys = single(parse_operator(&entry(name)).unwrap());
        let mut sim = Sim::new(1);
        sim.particles.push(*particle);
        let points = sim.control_motion(&sys, 0.0).map(std::cell::Cell::new);
        let written = std::cell::Cell::new(0);
        operators::Step {
            model_capsules: &[],
            system: &sys,
            event: None,
            control_points: &[[0.0; 3]; 8],
            control_motion: &points,
            written_points: &written,
            clock: crate::effects::FrameClock::default(),
            layer_time: 0.0,
            gains: &[1.0],
            time: 0.0,
            dt: 0.1,
            drag_scale: 1.0,
            substeps: 1,
            world_turn: None,
        }
        .apply(particle, 0, particle.pos, 0, 0);
    };
    step(&mut alpha, "oscillatealpha");
    step(&mut size, "oscillatesize");
    assert!((alpha.alpha - 0.6).abs() < BLEND_TOLERANCE, "{}", alpha.alpha);
    assert!((size.size - 0.6).abs() < BLEND_TOLERANCE, "{}", size.size);
}

#[test]
fn angular_blend_weight() {
    let mut angles = Vec::new();
    for blend in [false, true] {
        let mut entry = serde_json::json!({"name":"angularmovement","force":"0 0 10"});
        if blend {
            entry["blendinstart"] = serde_json::json!(0.5);
            entry["blendinend"] = serde_json::json!(0.6);
        }
        let mut sys = single(parse_operator(&entry).unwrap());
        sys.initializers = vec![Initializer::Lifetime { min: 1.0, max: 1.0, exponent: 1.0 }];
        let mut sim = spawn_one(&sys);
        sim.particles[0].age = 0.54;
        sim.particles[0].angular = 0.0;
        sim.particles[0].angle = 0.0;
        sim.step(&sys, 0.01);
        angles.push((sim.particles[0].angular, sim.particles[0].angle));
    }
    assert!((angles[1].0 / angles[0].0 - 0.5).abs() < BLEND_TOLERANCE, "{angles:?}");
    assert!((angles[1].1 / angles[0].1 - 0.25).abs() < BLEND_TOLERANCE, "{angles:?}");
}

#[test]
fn attract_blend_weight() {
    let mut pulls = Vec::new();
    for blend in [false, true] {
        let mut entry = serde_json::json!({"name":"controlpointattract","controlpoint":1,
            "scale":100,"threshold":500,"flags":0});
        if blend {
            entry["blendinstart"] = serde_json::json!(0.5);
            entry["blendinend"] = serde_json::json!(0.6);
        }
        let mut sys = single(parse_operator(&entry).unwrap());
        sys.control_points[1].offset = [100.0, 0.0, 0.0];
        sys.initializers = vec![Initializer::Lifetime { min: 1.0, max: 1.0, exponent: 1.0 }];
        let mut sim = spawn_one(&sys);
        sim.particles[0].age = 0.54;
        sim.particles[0].vel = [0.0; 3];
        sim.step(&sys, 0.01);
        pulls.push(sim.particles[0].vel[0]);
    }
    assert!(pulls[0] > 0.0);
    assert!((pulls[1] / pulls[0] - 0.5).abs() < BLEND_TOLERANCE, "{pulls:?}");
}

#[test]
fn inherit_blend_weight() {
    let mut particle =
        spawn_one(&single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 })).particles[0];
    particle.size = 2.0;
    let mut event = particle;
    event.size = 6.0;
    let event = EventValues::from(&event);
    for (inherit, weight, expected) in [
        (EventInheritance::SetSize, None, 6.0),
        (EventInheritance::SetSize, Some(0.5), 4.0),
        (EventInheritance::MultiplySize, None, 6.0),
        (EventInheritance::MultiplySize, Some(0.5), 4.0),
    ] {
        let mut target = particle;
        inherit.apply_operator(&mut target, event, weight);
        assert_eq!(target.size, expected, "{weight:?}");
    }
}

#[test]
fn hide_restarts_sim() {
    let sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let mut sim = spawn_one(&sys);
    sim.hide();
    assert!(sim.particles.is_empty());
    assert_eq!(sim.layer_time(), 0.0);
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles.len(), 1);
    sim.stop();
    sim.hide();
    sim.step(&sys, 0.01);
    assert!(sim.particles.is_empty());
}

#[test]
fn hide_keeps_stop() {
    let sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let mut sim = spawn_one(&sys);
    sim.stop();
    sim.hide();
    assert!(!sim.is_playing());
}

#[test]
fn hide_keeps_pause() {
    let sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let mut sim = spawn_one(&sys);
    sim.pause();
    sim.hide();
    sim.step(&sys, 0.01);
    assert!(sim.particles.is_empty());
    sim.play();
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles.len(), 1);
}

#[test]
fn hide_keeps_manual() {
    let sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let mut sim = Sim::new(17);
    sim.stop();
    sim.emit_particles(&sys, 2, &mut |_| {});
    assert_eq!(sim.particles.len(), 2);
    sim.hide();
    assert!(sim.particles.is_empty());
    sim.step(&sys, 0.5);
    assert!(sim.particles.is_empty());
    sim.emit_particles(&sys, 1, &mut |_| {});
    assert_eq!(sim.particles.len(), 1);
}

#[test]
fn turbulence_blend_weight() {
    let operator = |blend: bool| {
        let mut entry = serde_json::json!({"name":"turbulence","speedmin":100,"speedmax":100,
            "phasemax":0,"timescale":1,"scale":0.01,"mask":"1 1 0"});
        if blend {
            entry["blendinstart"] = serde_json::json!(0.5);
            entry["blendinend"] = serde_json::json!(0.6);
        }
        parse_operator(&entry).unwrap()
    };
    let mut gains = Vec::new();
    for blend in [false, true] {
        let mut sys = single(operator(blend));
        sys.initializers = vec![Initializer::Lifetime { min: 1.0, max: 1.0, exponent: 1.0 }];
        let mut sim = spawn_one(&sys);
        sim.particles[0].age = 0.54;
        sim.particles[0].vel = [0.0; 3];
        sim.step(&sys, 0.01);
        gains.push(sim.particles[0].vel[0]);
    }
    assert!(gains[0] != 0.0);
    assert!((gains[1] / gains[0] - 0.5).abs() < BLEND_TOLERANCE, "{gains:?}");
}

#[test]
fn point_sphere_parse() {
    for (entry, expected) in [
        (serde_json::json!({"name":"collisionsphere"}), None),
        (serde_json::json!({"name":"collisionsphere","flags":1}), Some(0)),
        (serde_json::json!({"name":"collisionsphere","flags":1,"controlpoint":9}), Some(7)),
    ] {
        let Some(Operator::CollisionSphere { point, .. }) = parse_operator(&entry) else {
            panic!("sphere")
        };
        assert_eq!(point, expected, "{entry}");
    }
}

#[test]
fn sphere_follows_point() {
    let mut sys = single(
        parse_operator(&serde_json::json!({
            "name":"collisionsphere","flags":1,"controlpoint":2,"radius":5,"origin":"900 0 0",
            "collisionbehavior":"stop"
        }))
        .unwrap(),
    );
    sys.control_points[2].offset = [100.0, 0.0, 0.0];
    let mut sim = spawn_one(&sys);
    sim.particles[0].pos = [103.0, 0.0, 0.0];
    sim.step(&sys, 0.01);
    assert!((sim.particles[0].pos[0] - 105.0).abs() < 0.01, "{}", sim.particles[0].pos[0]);
}

#[test]
fn collision_stops_angular_motion() {
    for sphere in [false, true] {
        let entry = if sphere {
            serde_json::json!({
                "name":"collisionsphere","origin":"0 0 0","radius":5,"flags":2
            })
        } else {
            serde_json::json!({
                "name":"collisionplane","plane":"0 1 0","distance":-5,"flags":2
            })
        };
        for reverse in [false, true] {
            let collision = parse_operator(&entry).unwrap();
            let angular = Operator::AngularMovement { force: [0.0; 3], drag: 0.0, blend: None };
            let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
            sys.emitters = vec![point_emitter([10.0, 0.0, 0.0], 1.0, 0)];
            sys.operators =
                if reverse { vec![angular, collision] } else { vec![collision, angular] };
            let mut sim = spawn_one(&sys);
            sim.particles[0].pos = if sphere { [3.0, 0.0, 0.0] } else { [0.0, -6.0, 0.0] };
            sim.particles[0].angle = 1.0;
            sim.particles[0].angular = 10.0;
            sim.step(&sys, 0.01);
            assert_eq!(sim.particles[0].angular, 0.0);
            let expected = if reverse { 1.1 } else { 1.0 };
            assert!((sim.particles[0].angle - expected).abs() < 1e-5);
            sim.particles[0].pos = if sphere { [5.0, 0.0, 0.0] } else { [0.0, -5.0, 0.0] };
            sim.particles[0].angular = 10.0;
            sim.step(&sys, 0.01);
            assert_eq!(sim.particles[0].angular, 10.0);
        }
    }
}

#[test]
fn cap_velocity_preserves_slow() {
    let sys = single(
        parse_operator(&serde_json::json!({
            "name": "capvelocity", "maxspeed": 100
        }))
        .unwrap(),
    );
    let mut sim = spawn_one(&sys);
    sim.particles[0].vel = [30.0, 40.0, 0.0];
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles[0].vel, [30.0, 40.0, 0.0]);
}

#[test]
fn cap_velocity_parses_defaults() {
    let op = parse_operator(&serde_json::json!({"name": "capvelocity"})).unwrap();
    let Operator::CapVelocity { max_speed, blend } = op else { panic!("capvelocity") };
    assert_eq!(max_speed, 100.0);
    assert!(blend.is_none());
}

#[test]
fn cap_velocity_blends_life() {
    let mut sys = single(
        parse_operator(&serde_json::json!({
            "name": "capvelocity", "maxspeed": 100,
            "blendinstart": 0.5, "blendinend": 0.6,
            "blendoutstart": 0.8, "blendoutend": 0.9
        }))
        .unwrap(),
    );
    sys.initializers = vec![Initializer::Lifetime { min: 1.0, max: 1.0, exponent: 1.0 }];
    let mut sim = spawn_one(&sys);
    #[cfg(target_arch = "x86_64")]
    let cases = [(0.24, 500.0), (0.54, 300.53348), (0.74, 99.99275), (0.84, 299.16614)];
    #[cfg(not(target_arch = "x86_64"))]
    let cases = [(0.24, 500.0), (0.54, 300.0), (0.74, 100.0), (0.84, 300.0)];
    for (age, expected) in cases {
        sim.particles[0].age = age;
        sim.particles[0].vel = [500.0, 0.0, 0.0];
        sim.step(&sys, 0.01);
        assert!((sim.particles[0].vel[0] - expected).abs() < 0.05);
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn cap_velocity_blend_boundary() {
    let mut sys = single(
        parse_operator(&serde_json::json!({
            "name": "capvelocity", "maxspeed": 100,
            "blendinstart": 0.5, "blendinend": 0.6
        }))
        .unwrap(),
    );
    sys.initializers = vec![Initializer::Lifetime { min: 1.0, max: 1.0, exponent: 1.0 }];
    let mut sim = spawn_one(&sys);
    sim.particles[0].age = 0.59;
    sim.particles[0].vel = [500.0, 0.0, 0.0];
    sim.step(&sys, 0.01);
    assert!((100.5..100.9).contains(&sim.particles[0].vel[0]));
}

#[test]
fn cap_velocity_saturates_nan() {
    let entries = [
        serde_json::json!({
            "name": "capvelocity", "maxspeed": 100,
            "blendoutstart": 0.5, "blendoutend": 1e40
        }),
        serde_json::json!({
            "name": "capvelocity", "maxspeed": 100,
            "blendinstart": 1e40, "blendinend": 1e40,
            "blendoutstart": 0.5
        }),
    ];
    for entry in entries {
        let mut sys = single(parse_operator(&entry).unwrap());
        sys.initializers = vec![Initializer::Lifetime { min: 1.0, max: 1.0, exponent: 1.0 }];
        let mut sim = spawn_one(&sys);
        sim.particles[0].age = 0.1;
        sim.particles[0].vel = [500.0, 0.0, 0.0];
        sim.step(&sys, 0.01);
        assert!((sim.particles[0].vel[0] - 100.0).abs() < 0.05);
    }
}

#[test]
fn cap_velocity_uses_depth() {
    let sys = single(Operator::CapVelocity { max_speed: 100.0, blend: None });
    let mut sim = spawn_one(&sys);
    sim.particles[0].vel = [300.0, 0.0, 400.0];
    sim.step(&sys, 0.01);
    for (actual, expected) in sim.particles[0].vel.into_iter().zip([60.0, 0.0, 80.0]) {
        assert!((actual - expected).abs() < 0.05);
    }
}

#[test]
fn cap_velocity_respects_order() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.operators.push(Operator::CapVelocity { max_speed: 100.0, blend: None });
    let mut sim = spawn_one(&sys);
    sim.particles[0].vel = [500.0, 0.0, 0.0];
    sim.step(&sys, 0.01);
    assert!((sim.particles[0].pos[0] - 5.0).abs() < 0.05);
    sys.operators.reverse();
    let mut sim = spawn_one(&sys);
    sim.particles[0].vel = [500.0, 0.0, 0.0];
    sim.step(&sys, 0.01);
    assert!((sim.particles[0].pos[0] - 1.0).abs() < 0.05);
}

#[test]
fn cap_velocity_zero_nan() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let mut sim = spawn_one(&sys);
    sys.operators = vec![Operator::CapVelocity { max_speed: 0.0, blend: None }];
    sim.particles[0].vel = [500.0, 0.0, 0.0];
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles[0].vel, [0.0; 3]);
    sim.step(&sys, 0.01);
    assert!(sim.particles[0].vel.iter().all(|vel| vel.is_nan()));
}

#[test]
fn turbulence_accelerates_without_directly_displacing_particle() {
    let sys = single(Operator::Turbulence {
        scale: 0.01,
        speed: (500.0, 500.0),
        phase_range: 0.0,
        time_scale: 1.0,
        mask: [1.0, 1.0, 1.0],
        blend: None,
        audio: None,
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

const AUDIO_OPERATORS: [&str; 2] = ["vortex", "turbulence"];

fn audio_velocity(name: &str, mode: u32, level: f32) -> [f32; 3] {
    let sys = single(
        parse_operator(&serde_json::json!({
            "name": name, "flags": 1, "mask": "1 1 0", "audioprocessingmode": mode,
            "audioprocessingbounds": "0 1", "audioprocessingexponent": 1,
            "audioprocessingfrequencyend": 15
        }))
        .unwrap(),
    );
    assert_eq!(sys.needs_audio(), mode != 0);
    let mut sim = spawn_one(&sys);
    sim.particles[0].pos = [100.0, 30.0, 0.0];
    sim.particles[0].vel = [0.0; 3];
    sim.step_with_audio(&sys, 0.01, &[level; 16], &[level; 16], &mut |_| {});
    sim.particles[0].vel
}

#[test]
fn audio_gain_scales_forces() {
    for name in AUDIO_OPERATORS {
        let plain = audio_velocity(name, 0, 0.0);
        assert!(plain.iter().any(|axis| axis.abs() > 1.0), "{name}");
        assert_eq!(audio_velocity(name, 3, 1.0), plain, "{name}");
        let half = audio_velocity(name, 3, 0.5);
        for (half, plain) in half.iter().zip(plain) {
            assert!((half - plain * 0.5).abs() < 1e-3, "{name}");
        }
    }
}

#[test]
fn silence_stops_audio_forces() {
    for name in AUDIO_OPERATORS {
        assert_eq!(audio_velocity(name, 3, 0.0), [0.0; 3], "{name}");
    }
}

fn vortex_velocity(entry: &serde_json::Value, pos: [f32; 3], dt: f32) -> [f32; 3] {
    let sys = single(parse_operator(entry).unwrap());
    let mut sim = spawn_one(&sys);
    sim.particles[0].pos = pos;
    sim.particles[0].vel = [0.0; 3];
    sim.step(&sys, dt);
    sim.particles[0].vel
}

#[test]
fn vortex_matches_engine() {
    let inverted = serde_json::json!({
        "name": "vortex", "axis": "0 0 1000", "distanceinner": 2000, "distanceouter": 0,
        "speedinner": -10000, "speedouter": 0
    });
    let tilted = serde_json::json!({
        "name": "vortex", "axis": "0 1000 1000", "distanceinner": 20000, "distanceouter": 0,
        "speedinner": 5000
    });
    let banded = serde_json::json!({
        "name": "vortex", "distanceinner": 400, "distanceouter": 700, "speedinner": 2500,
        "speedouter": 0
    });
    let cases = [
        (inverted, [500.0, 0.0, 0.0], 0.025, [0.0, 62.5, 0.0]),
        (tilted, [0.0, 300.0, 0.0], 0.025, [1.3258, 0.0, 0.0]),
        (banded, [100.0, 0.0, 0.0], 1.0 / 15.0, [0.0, -83.883, 0.0]),
    ];
    for (entry, pos, dt, expected) in cases {
        let vel = vortex_velocity(&entry, pos, dt);
        for (got, want) in vel.iter().zip(expected) {
            assert!((got - want).abs() <= 0.02 * want.abs().max(1.0), "{vel:?} {expected:?}");
        }
    }
}

#[test]
fn vortex_has_no_pull() {
    let entry = serde_json::json!({
        "name": "vortex", "flags": 7, "distanceinner": 0, "distanceouter": 50,
        "speedinner": 1000, "speedouter": 1000, "centerforce": 500, "ringradius": 100,
        "ringwidth": 10, "ringpulldistance": 5, "ringpullforce": 50
    });
    let spin = 1000.0 * 0.1 * drag_scale(0.1);
    for pos in [[100.0, 0.0, 0.0], [300.0, 0.0, 0.0]] {
        let vel = vortex_velocity(&entry, pos, 0.1);
        assert!(vel[0] == 0.0 && (vel[1] + spin).abs() < spin * 1e-3 && vel[2] == 0.0, "{vel:?}");
    }
}

#[test]
fn attract_pulls_to_control_point() {
    let mut sys = single(Operator::ControlPointAttract {
        control_point: 0,
        flags: 2,
        delete_threshold: 15.0,
        scale: 5000.0,
        threshold: 500.0,
        blend: None,
    });
    sys.control_points[0].offset = [100.0, 0.0, 0.0];
    let mut sim = spawn_one(&sys);
    for _ in 0..10 {
        sim.step(&sys, 1.0 / 30.0);
    }
    assert!(sim.particles[0].vel[0] > 0.0);
}

#[test]
fn force_pushes_from_offset() {
    let mut sys = single(
        parse_operator(&serde_json::json!({
            "name":"controlpointforce","controlpoint":0,
            "offset":"20 0 0","threshold":100,"scale":100
        }))
        .unwrap(),
    );
    let mut sim = spawn_one(&sys);
    sim.particles[0].pos = [10.0, 0.0, 0.0];
    sim.step(&sys, 0.01);
    assert!(sim.particles[0].vel[0] < 0.0);
    sys.operators = vec![
        parse_operator(&serde_json::json!({
            "name":"controlpointforce","controlpoint":0,
            "offset":"0 0 0","threshold":100,"scale":100
        }))
        .unwrap(),
    ];
    sim.particles[0].vel = [0.0; 3];
    sim.step(&sys, 0.01);
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
fn prewarm_native_schedule() {
    for (time, capacity, expected) in
        [(0.13, 1, 15.0), (0.51, 1, 55.0), (1.03, 1, 105.0), (1.49, 500, 160.0), (2.0, 500, 200.0)]
    {
        let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        sys.renderer = Renderer::Ribbon;
        sys.trail.length = 1.5;
        sys.start_time = time;
        sys.max_count = capacity;
        sys.rate_scale = 0.25;
        sys.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
        sys.initializers.push(Initializer::Velocity {
            exponent: 1.0,
            min: [100.0, 0.0, 0.0],
            max: [100.0, 0.0, 0.0],
        });
        let mut sim = Sim::new(17);
        sim.prewarm(&sys, sys.prewarm_step());
        assert!((sim.particles[0].pos[0] - expected).abs() < 0.001);
        assert!(sim.history[0].points().all(|point| point.pos == sim.particles[0].pos));
        assert_eq!(sim.history[0].sample_count, 0);
        sim.step(&sys, 0.0);
        assert_eq!(sim.history[0].sample_count, 1);
    }
}

#[test]
fn prewarm_suppresses_birth_children() {
    for kind in [ChildEventKind::Follow, ChildEventKind::Spawn] {
        let mut parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        parent.start_time = 0.5;
        parent.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
        let mut source = Sim::new(17);
        source.prewarm(&parent, parent.prewarm_step());
        let mut child = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        child.child_event = Some(child_event(kind));
        let mut followers = Followers::default();
        let mut output = Sim::new(18);
        followers.step(&mut output, &child, (&parent, &source), 0.01, (&[], &[]), &mut |_| {});
        assert!(output.particles.is_empty());
        source.emit_particles(&parent, 1, &mut |_| {});
        followers.step(&mut output, &child, (&parent, &source), 0.01, (&[], &[]), &mut |_| {});
        assert_eq!(output.particles.len(), 1);
    }
}

#[test]
fn nested_prewarm_births() {
    for kind in [ChildEventKind::Follow, ChildEventKind::Spawn] {
        let mut parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        parent.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
        let mut source = spawn_one(&parent);
        let mut child = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        child.start_time = 0.13;
        child.max_count = 10;
        child.child_event = Some(ChildEvent { limit: 2, ..child_event(ChildEventKind::Follow) });
        child.emitters = vec![point_emitter([0.0; 3], 10.0, 1)];
        let mut grandchild = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        grandchild.child_event = Some(ChildEvent { limit: 10, ..child_event(kind) });
        grandchild.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
        let mut children = Followers::default();
        let mut grandchildren = Followers::default();
        let mut middle = Sim::new(18);
        let mut output = Sim::new(19);
        for (dt, expected) in [(0.01, 0), (0.045, 1), (0.1, 3)] {
            if expected == 1 {
                source.emit_particles(&parent, 1, &mut |_| {});
            }
            children.step(&mut middle, &child, (&parent, &source), dt, (&[], &[]), &mut |_| {});
            grandchildren.step(
                &mut output,
                &grandchild,
                (&child, &middle),
                dt,
                (&[], &[]),
                &mut |_| {},
            );
            assert_eq!(output.particles.len(), expected);
        }
        assert_eq!(middle.prewarmed.len(), 4);
    }
}

#[test]
fn warmed_death_events() {
    let mut parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    parent.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
    let source = spawn_one(&parent);
    let mut child = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    child.start_time = 0.13;
    child.max_count = 1;
    child.has_death_children = true;
    child.child_event = Some(child_event(ChildEventKind::Follow));
    child.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
    child.initializers[0] = Initializer::Lifetime { min: 0.2, max: 0.2, exponent: 1.0 };
    let mut grandchild = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    grandchild.child_event = Some(child_event(ChildEventKind::Death));
    grandchild.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
    let mut children = Followers::default();
    let mut grandchildren = Followers::default();
    let mut middle = Sim::new(18);
    let mut output = Sim::new(19);
    for (dt, expected) in [(0.01, 0), (0.11, 1)] {
        children.step(&mut middle, &child, (&parent, &source), dt, (&[], &[]), &mut |_| {});
        grandchildren.step(
            &mut output,
            &grandchild,
            (&child, &middle),
            dt,
            (&[], &[]),
            &mut |_| {},
        );
        assert_eq!(output.particles.len(), expected);
    }
}

#[test]
fn child_prewarm_runs_once() {
    let parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let source = spawn_one(&parent);
    let mut child = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    child.start_time = 0.13;
    child.max_count = 1;
    child.initializers.push(Initializer::Velocity {
        exponent: 1.0,
        min: [100.0, 0.0, 0.0],
        max: [100.0, 0.0, 0.0],
    });
    child.child_event = Some(child_event(ChildEventKind::Follow));
    child.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
    let mut followers = Followers::default();
    let mut output = Sim::new(18);
    followers.step(&mut output, &child, (&parent, &source), 0.01, (&[], &[]), &mut |_| {});
    assert!((output.particles[0].pos[0] - 16.0).abs() < 0.001);
    followers.step(&mut output, &child, (&parent, &source), 0.01, (&[], &[]), &mut |_| {});
    assert!((output.particles[0].pos[0] - 17.0).abs() < 0.001);
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
        angular_motion: false,
        remapped_points: 0,
        model_collisions: super::model_collision::Bindings::default(),
        image_dependencies: Vec::new(),
        children: Vec::new(),
        has_death_children: false,
        child_event: None,
        renderer: Renderer::Sprite,
        orientation: super::Orientation::Screen,
        renderer_flags: 0,
        extra_renderers: Vec::new(),
        extra_passes: Vec::new(),
        trail: Trail {
            length: 0.0,
            min_length: 0.0,
            max_length: 0.0,
            segments: 4,
            fade_alpha: false,
            fade_size: false,
            uv_scroll: false,
            uv_scale: 1.0,
            subdivision: 0,
        },
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
        spatial: None,
        world_matrix: None,
        scale: 1.0,
        scale3: [1.0; 3],
        base_color: [1.0; 3],
        alpha: 1.0,
        count_scale: 1.0,
        rate_scale: 1.0,
        size_scale: 1.0,
        authored_size_scale: 1.0,
        grab_slot: None,
        world: false,
        instance: super::InstanceFactors::default(),
    };
    let mut sim = Sim::new(1);
    sim.particles.push(Particle {
        id: 0,
        pos: [10.0, 20.0, 0.0],
        vel: [1.0, 2.0, 3.0],
        size: 40.0,
        base_size: 40.0,
        color: [1.0, 1.0, 1.0],
        base_color: [1.0, 1.0, 1.0],
        alpha: 0.5,
        base_alpha: 0.5,
        angle: 0.3,
        angle_xy: [0.0; 2],
        angular: 0.0,
        angular_xy: [0.0; 2],
        age: 1.0,
        lifetime: 4.0,
        phase: 0.0,
        noise_seed: 0,
        slot: 0,
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
    assert_eq!(&v0[7..11], &[1.0, 1.0, 1.0, 0.5]);
    assert_eq!(&v0[11..15], &[1.0, 2.0, 3.0, 0.25]);
    assert_eq!(&v0[15..17], &[0.0, 0.0]);
    let corners: Vec<[f32; 2]> = (0..4).map(|i| [out[i * 17 + 3], out[i * 17 + 4]]).collect();
    assert_eq!(corners, [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
    assert_eq!(sprite_indices(2), [0, 1, 2, 2, 3, 0, 4, 5, 6, 6, 7, 4]);
}

#[test]
fn births_fill_gaps() {
    let sys = system();
    let mut sim = Sim::new(7);
    sim.emit_particles(&sys, 3, &mut |_| {});
    let last = sim.particles[2].id;
    sim.particles.remove(1);
    sim.emit_particles(&sys, 2, &mut |_| {});
    let slots: Vec<u32> = sim.particles.iter().map(|particle| particle.slot).collect();
    assert_eq!(slots, [0, 1, 2, 3]);
    assert_eq!(sim.particles[2].id, last);
}

#[test]
fn dead_slot_reused() {
    let sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let mut sim = spawn_one(&sys);
    sim.pending = 2.0;
    sim.step(&sys, 0.0001);
    let first = sim.particles[0].id;
    sim.particles[0].age = sim.particles[0].lifetime;
    sim.pending = 1.0;
    sim.step(&sys, 0.0001);
    let slots: Vec<u32> = sim.particles.iter().map(|particle| particle.slot).collect();
    assert_eq!(slots, [0, 1, 2]);
    assert_ne!(sim.particles[0].id, first);
}

#[test]
fn nan_skips_neighbors() {
    let mut sim = Sim::new(7);
    let sys = system();
    sim.emit_particles(&sys, 6, &mut |_| {});
    for (index, particle) in sim.particles.iter_mut().enumerate() {
        particle.pos = [index as f32 * 10.0, 0.0, 0.0];
    }
    sim.particles[2].pos[1] = f32::NAN;
    let mut packed = Vec::new();
    assert_eq!(pack_rope_points(&sim, &mut packed), 1);
    assert_eq!(&packed[..3], &[40.0, 0.0, 0.0]);
    sim.particles[0].alpha = f32::NAN;
    sim.particles[1].size = f32::NAN;
    sim.particles[3].angle = f32::NAN;
    assert_eq!(pack_sprite_points(&sys, &sim, 1, &mut packed), 2);
    assert_eq!(pack_sprites(&sys, &sim, 1, &mut packed), 2);
}

fn tilted_world() -> ParticleSystem {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.world = true;
    sys.world_matrix = Some(crate::matrix::model_matrix(
        [0.0; 3],
        [1.0; 3],
        [std::f32::consts::FRAC_PI_3, 0.0, 0.0],
    ));
    sys
}

fn upward(sys: &mut ParticleSystem) {
    sys.initializers.push(Initializer::Velocity {
        min: [0.0, 300.0, 0.0],
        max: [0.0, 300.0, 0.0],
        exponent: 1.0,
    });
}

fn aim_point(sys: &mut ParticleSystem, flags: u32, offset: [f32; 3]) {
    let Emitter::Box { control_point, .. } = &mut sys.emitters[0] else { panic!("box") };
    *control_point = Some(1);
    sys.control_points[1].flags = flags;
    sys.control_points[1].offset = offset;
}

#[test]
fn tilted_point_basis() {
    let mut sys = tilted_world();
    upward(&mut sys);
    aim_point(&mut sys, 0, [0.0, 100.0, 0.0]);
    let particle = spawn_one(&sys).particles[0];
    let half = 3f32.sqrt() / 2.0;
    for (actual, expected) in particle.pos.into_iter().zip([0.0, 50.0, 100.0 * half]) {
        assert!((actual - expected).abs() < 0.1, "{:?}", particle.pos);
    }
    assert!((particle.vel[1] - 150.0).abs() < 1e-3, "{:?}", particle.vel);
}

#[test]
fn worldspace_point_untilted() {
    let mut sys = tilted_world();
    upward(&mut sys);
    aim_point(&mut sys, 2, [0.0; 3]);
    let vel = spawn_one(&sys).particles[0].vel;
    assert!(vel[1] == 300.0 && vel[2] == 0.0, "{vel:?}");
}

#[test]
fn tilted_spin_world() {
    let mut sys = tilted_world();
    let angles = [std::f32::consts::FRAC_PI_3, 0.0, 0.7];
    sys.world_matrix = Some(crate::matrix::model_matrix([0.0; 3], [1.0; 3], angles));
    sys.angle = angles[2];
    upward(&mut sys);
    let vel = spawn_one(&sys).particles[0].vel;
    let (sin, cos) = sys.angle.sin_cos();
    let world = [cos * vel[0] - sin * vel[1], sin * vel[0] + cos * vel[1], vel[2]];
    let matrix = sys.world_matrix.unwrap();
    for (axis, actual) in world.into_iter().enumerate() {
        assert!((actual - 300.0 * matrix[4 + axis]).abs() < 1e-3, "{world:?}");
    }
}

#[test]
fn world_gravity_unturned() {
    let mut sys = single(Operator::Movement { gravity: [0.0, -100.0, 0.0], drag: 0.0 });
    sys.world = true;
    sys.angle = std::f32::consts::FRAC_PI_2;
    let mut sim = spawn_one(&sys);
    sim.particles[0].vel = [0.0; 3];
    sim.step(&sys, 0.1);
    let vel = sim.particles[0].vel;
    assert!(vel[0] < -1.0 && vel[1].abs() < 1e-3, "{vel:?}");
}

#[test]
fn nan_tilt_skipped() {
    let mut sim = Sim::new(7);
    let sys = system();
    sim.emit_particles(&sys, 2, &mut |_| {});
    sim.particles[1].angle_xy[1] = f32::NAN;
    let mut packed = Vec::new();
    assert_eq!(pack_sprites(&sys, &sim, 1, &mut packed), 1);
}

#[test]
fn tilted_world_velocity() {
    let mut sys = tilted_world();
    sys.initializers.push(Initializer::Velocity {
        min: [0.0, 300.0, 0.0],
        max: [0.0, 300.0, 0.0],
        exponent: 1.0,
    });
    let sim = spawn_one(&sys);
    let vel = sim.particles[0].vel;
    assert!((vel[1] - 150.0).abs() < 1e-3, "{vel:?}");
    assert!((vel[2] - 300.0 * 3f32.sqrt() / 2.0).abs() < 1e-3, "{vel:?}");
}

#[test]
fn tilted_world_spawn() {
    let mut sys = tilted_world();
    let Emitter::Box { extent, .. } = &mut sys.emitters[0] else { panic!("box") };
    *extent = [0.0, 100.0, 0.0];
    let sim = spawn_one(&sys);
    let pos = sim.particles[0].pos;
    assert!(pos[1].abs() > 1e-3, "{pos:?}");
    assert!((pos[2] / pos[1] - 3f32.sqrt()).abs() < 1e-3, "{pos:?}");
}

#[test]
fn rotation_packs_xy() {
    let mut sys =
        single(Operator::AngularMovement { force: [1.0, 2.0, 0.0], drag: 0.0, blend: None });
    sys.initializers.push(Initializer::Rotation { min: [0.5, 0.25, 0.0], max: [0.5, 0.25, 0.0] });
    let mut sim = spawn_one(&sys);
    let start = sim.particles[0].angle_xy;
    assert!((start[0] - 0.5).abs() < 1e-6 && (start[1] - 0.25).abs() < 1e-6, "{start:?}");
    sim.step(&sys, 0.1);
    let [x, y] = sim.particles[0].angle_xy;
    assert!(x > start[0] && y - start[1] > x - start[0], "{x} {y}");
    let mut packed = Vec::new();
    assert_eq!(pack_sprites(&sys, &sim, 1, &mut packed), 1);
    assert_eq!(packed[SPRITE_FLOATS_PER_VERTEX - 2..SPRITE_FLOATS_PER_VERTEX], [x, y]);
    assert_eq!(pack_sprite_points(&sys, &sim, 1, &mut packed), 1);
    assert_eq!(packed[3..5], [x, y]);
}

#[test]
fn nan_velocity_skipped() {
    let mut sim = Sim::new(7);
    let sys = system();
    sim.emit_particles(&sys, 2, &mut |_| {});
    sim.particles[0].vel[0] = f32::NAN;
    let mut packed = Vec::new();
    assert_eq!(pack_sprites(&sys, &sim, 1, &mut packed), 1);
}

#[test]
fn rope_points_use_neighbors() {
    let mut sim = Sim::new(7);
    let sys = system();
    sim.emit_particles(&sys, 4, &mut |_| {});
    for (index, particle) in sim.particles.iter_mut().enumerate() {
        particle.pos = [index as f32 * 10.0, 0.0, 0.0];
    }
    let mut packed = Vec::new();
    assert_eq!(pack_rope_points(&sim, &mut packed), 3);
    assert_eq!(packed.len(), 3 * ROPE_POINT_FLOATS);
    assert_eq!(&packed[ROPE_POINT_FLOATS..ROPE_POINT_FLOATS + 4], &[10.0, 0.0, 0.0, 2.5]);
    assert_eq!(&packed[ROPE_POINT_FLOATS + 8..ROPE_POINT_FLOATS + 11], &[0.0, 0.0, 0.0]);
    assert_eq!(&packed[ROPE_POINT_FLOATS + 12..ROPE_POINT_FLOATS + 15], &[30.0, 0.0, 0.0]);
}

#[test]
fn instance_overrides_bind_properties() {
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
    let context = LoadContext {
        pkg: &pkg,
        assets: &assets,
        defaults: ParticleDefaults::ORTHOGRAPHIC,
        viewport: None,
        bounds_static: false,
        version: 0,
    };
    let system = super::load(&context, &object, "particles/p.json").0.unwrap();
    assert_eq!(system.max_count, 100);
    assert_eq!(system.rate_scale, 0.25);
    assert_eq!(system.base_color, [0.5, 0.25, 1.0]);
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
fn color_initializers_multiply() {
    let cases = [
        (
            vec![
                serde_json::json!({"name":"colorrandom","min":"255 0 0","max":"255 0 0"}),
                serde_json::json!({"name":"hsvcolorrandom","huemin":0.166_666_67,"huemax":0.166_666_67,"huesteps":1,"saturationmin":1,"saturationmax":1,"valuemin":1,"valuemax":1}),
            ],
            [1.0, 0.0, 0.0],
        ),
        (
            vec![
                serde_json::json!({"name":"hsvcolorrandom","huemin":0.5,"huemax":0.5,"huesteps":1,"saturationmin":1,"saturationmax":1,"valuemin":1,"valuemax":1}),
                serde_json::json!({"name":"colorrandom","min":"255 255 0","max":"255 255 0"}),
            ],
            [0.0, 1.0, 0.0],
        ),
        (
            vec![
                serde_json::json!({"name":"colorrandom","min":"255 255 0","max":"255 255 0"}),
                serde_json::json!({"name":"colorlist","colors":["0 1 1"]}),
            ],
            [0.0, 1.0, 0.0],
        ),
    ];
    for (entries, expected) in cases {
        let mut sys = system();
        sys.initializers.extend(entries.iter().map(|entry| parse_initializer(entry).unwrap()));
        let color = spawn_one(&sys).particles[0].color;
        for axis in 0..3 {
            assert!((color[axis] - expected[axis]).abs() < 1e-5, "{color:?} != {expected:?}");
        }
    }
}

#[test]
fn hsv_defaults_and_steps() {
    assert_eq!(hue_index(0, 3), 0);
    assert_eq!(hue_index(HSV_RANDOM_MAX, 3), 3);
    assert_eq!(hue_index(HSV_RANDOM_MAX / 2, 3), 1);
    let Some(Initializer::HsvColor(default)) =
        parse_initializer(&serde_json::json!({"name":"hsvcolorrandom"}))
    else {
        panic!("missing HSV initializer");
    };
    assert_eq!((default.hue_min, default.hue_step, default.hue_steps), (0.0, 1.0 / 6.0, 6));
    assert_eq!((default.saturation, default.value), ((0.5, 1.0), (0.5, 1.0)));

    let mut sys = system();
    sys.initializers.push(
        parse_initializer(&serde_json::json!({
            "name":"hsvcolorrandom","huemin":0,"huemax":0.5,"huesteps":3,
            "saturationmin":1,"saturationmax":1,"valuemin":1,"valuemax":1
        }))
        .unwrap(),
    );
    let mut sim = Sim::new(17);
    for _ in 0..30 {
        sim.step(&sys, 1.0 / 30.0);
    }
    let mut seen = [false; 4];
    let expected: [[f32; 3]; 4] =
        [[1.0, 0.0, 0.0], [0.5, 1.0, 0.0], [0.0, 1.0, 1.0], [0.5, 0.0, 1.0]];
    for particle in &sim.particles {
        let index = expected.iter().position(|sample| {
            sample
                .iter()
                .zip(particle.color)
                .all(|(expected, actual)| (expected - actual).abs() < 1e-5)
        });
        seen[index.expect("unexpected hue sample")] = true;
    }
    assert!(seen.into_iter().all(|sample| sample));
}

#[test]
fn hsv_full_turn_wraps() {
    for (saturation, value, expected) in [(1.0, 1.0, [1.0, 0.0, 0.0]), (0.0, 0.5, [0.5, 0.5, 0.5])]
    {
        let mut sys = system();
        sys.initializers.push(
            parse_initializer(&serde_json::json!({
                "name":"hsvcolorrandom","huemin":1,"huemax":1,"huesteps":1,
                "saturationmin":saturation,"saturationmax":saturation,
                "valuemin":value,"valuemax":value
            }))
            .unwrap(),
        );
        let color = spawn_one(&sys).particles[0].color;
        for axis in 0..3 {
            assert!((color[axis] - expected[axis]).abs() < 1e-5, "{color:?} != {expected:?}");
        }
    }
}

#[test]
fn list_defaults_and_values() {
    for (entry, expected) in [
        (serde_json::json!({"name":"colorlist"}), [1.0, 1.0, 1.0]),
        (serde_json::json!({"name":"colorlist","colors":[]}), [1.0, 0.0, 0.0]),
        (serde_json::json!({"name":"colorlist","colors":["0 0 0.5"]}), [0.0, 0.0, 0.5]),
    ] {
        let mut sys = system();
        sys.initializers.push(parse_initializer(&entry).unwrap());
        let color = spawn_one(&sys).particles[0].color;
        for axis in 0..3 {
            assert!((color[axis] - expected[axis]).abs() < 1e-5, "{color:?} != {expected:?}");
        }
    }
}

#[test]
fn list_selects_all_colors() {
    let mut sys = system();
    sys.initializers.push(
        parse_initializer(&serde_json::json!({
            "name":"colorlist","colors":["1 0 0","0 1 0","0 0 1"]
        }))
        .unwrap(),
    );
    let palette: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut seen = [false; 3];
    for seed in [3, 17, 29] {
        let mut sim = Sim::new(seed);
        for _ in 0..30 {
            sim.step(&sys, 1.0 / 30.0);
        }
        assert!(sim.particles.len() > 20);
        for particle in &sim.particles {
            let index = palette.iter().position(|color| {
                color
                    .iter()
                    .zip(particle.color)
                    .all(|(expected, actual)| (expected - actual).abs() < 1e-5)
            });
            seen[index.expect("color outside list")] = true;
        }
    }
    assert!(seen.into_iter().all(|selected| selected));
}

#[test]
fn list_hue_noise_clips() {
    let mut sys = system();
    sys.initializers.push(
        parse_initializer(&serde_json::json!({
            "name":"colorlist","colors":["1 0 0"],"huenoise":0.1
        }))
        .unwrap(),
    );
    let mut sim = Sim::new(17);
    for _ in 0..30 {
        sim.step(&sys, 1.0 / 30.0);
    }
    assert!(sim.particles.len() > 20);
    assert!(sim.particles.iter().all(|particle| {
        (particle.color[0] - 1.0).abs() < 1e-5
            && particle.color[1] >= 0.0
            && particle.color[1] <= 0.6
            && particle.color[2].abs() < 1e-5
    }));
    assert!(sim.particles.iter().any(|particle| particle.color[1] > 0.3));
}

#[test]
fn sphere_emitter_renormalises_scaled_directions_and_honours_sign() {
    let mut sys = system();
    sys.emitters = vec![Emitter::Sphere {
        audio: None,
        control_point: None,
        instantaneous: 0,
        timing: EmitterTiming::default(),
        origin: [0.0, 0.0, 0.0],
        shape: SphereShape {
            directions: [1.0, 0.0, 0.0],
            sign: [0.0, 0.0, 0.0],
            distance: [300.0, 300.0],
            speed: [0.0; 2],
            polar_floor: -1.0,
        },
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
        timing: EmitterTiming::default(),
        origin: [0.0, 0.0, 0.0],
        shape: SphereShape {
            directions: [1.0, 1.0, 1.0],
            sign: [0.0, 0.0, -1.0],
            distance: [300.0, 300.0],
            speed: [0.0; 2],
            polar_floor: -1.0,
        },
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
    let Emitter::Sphere { shape, .. } = emitter else { panic!("sphere") };
    assert_eq!(shape.sign, [10.0, 10.0, 10.0]);
    let mut sys = system();
    sys.max_count = 25000;
    assert_eq!(Sim::capacity(&sys), 25000);
}

#[test]
fn slow_emission() {
    let mut sys = system();
    sys.emitters = vec![Emitter::Box {
        audio: None,
        control_point: None,
        instantaneous: 0,
        timing: EmitterTiming::default(),
        origin: [0.0; 3],
        extent: [0.0; 3],
        rate: 0.001,
    }];
    sys.initializers = vec![Initializer::Lifetime { exponent: 1.0, min: 10.0, max: 10.0 }];
    let mut sim = Sim::new(9);
    sim.step(&sys, 1.0 / 30.0);
    assert!(sim.particles.is_empty());
    for _ in 0..30 {
        sim.step(&sys, 1.0 / 30.0);
    }
    assert!(sim.particles.is_empty());
}

#[test]
fn sphere_emitter_distance_follows_the_scaled_ball_length() {
    let mut sys = system();
    sys.max_count = 4000;
    sys.emitters = vec![Emitter::Sphere {
        audio: None,
        control_point: None,
        instantaneous: 0,
        timing: EmitterTiming::default(),
        origin: [0.0; 3],
        shape: SphereShape {
            directions: [1.0, 1.0, 0.0],
            sign: [1.0, 1.0, 1.0],
            distance: [0.0, 1000.0],
            speed: [0.0; 2],
            polar_floor: -1.0,
        },
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
    let context = LoadContext {
        pkg: &pkg,
        assets: &assets,
        defaults: ParticleDefaults::ORTHOGRAPHIC,
        viewport: None,
        bounds_static: false,
        version: 0,
    };
    let system = super::load(&context, &object, "particles/p.json").0.unwrap();
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
        timing: EmitterTiming::default(),
        origin: [100.0, 0.0, 0.0],
        shape: SphereShape {
            directions: [1.0, 0.0, 0.0],
            sign: [1.0, 0.0, 0.0],
            distance: [300.0, 300.0],
            speed: [0.0; 2],
            polar_floor: -1.0,
        },
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
    let context = LoadContext {
        pkg: &pkg,
        assets: &assets,
        defaults: ParticleDefaults::ORTHOGRAPHIC,
        viewport: None,
        bounds_static: false,
        version: 0,
    };
    let system = super::load(&context, &local, "particles/l.json").0.unwrap();
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
        timing: EmitterTiming::default(),
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
        instantaneous: 1,
        timing: EmitterTiming::default(),
    }];
    sys.initializers = vec![Initializer::Lifetime { min: 40.0, max: 40.0, exponent: 1.0 }];
    let osc = |frequency: f32, phase: f32| {
        vec![Operator::OscillatePosition(Oscillator {
            frequency: (frequency, frequency),
            scale: (300.0, 300.0),
            phase: (phase, phase),
            mask: [1.0, 0.0, 0.0],
            blend: None,
        })]
    };
    sys.operators = osc(1.0, 0.0);
    let mut sim = Sim::new(5);
    let dt = 1.0 / 100.0;
    let mut peak = 0.0f32;
    for step in 1..=628 {
        sim.step(&sys, dt);
        let expected = 300.0 * ((step - 1) as f32 * dt).sin();
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
    assert!((x - 300.0).abs() < 6.0, "world oscillation unscaled: {x}");
}

#[test]
fn cursor_control_point_spawns_a_trail_without_moving_existing_particles() {
    let doc = br#"{"controlpoint":[{"id":0,"flags":1,"offset":"4 6 0"},{"id":80,"flags":1}],"emitter":[{"name":"boxrandom","rate":10,"instantaneous":1}],"initializer":[{"name":"lifetimerandom","min":10,"max":10}]}"#;
    let pkg = crate::pkg::Package::parse(crate::tests::build_pkg(&[("p.json", doc)])).unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let object = serde_json::json!({"origin":"100 200 0","scale":"2 3 1"});
    let context = LoadContext {
        pkg: &pkg,
        assets: &assets,
        defaults: ParticleDefaults::ORTHOGRAPHIC,
        viewport: None,
        bounds_static: false,
        version: 0,
    };
    let sys = load(&context, &object, "p.json").0.unwrap();
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
        instantaneous: 1,
        timing: EmitterTiming::default(),
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
        blend: None,
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
    Emitter::Box {
        audio: None,
        control_point: None,
        instantaneous,
        timing: EmitterTiming::default(),
        origin,
        extent: [0.0; 3],
        rate,
    }
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
        bounds: [0.0, 1.0],
        mirror: false,
        axis: [0.0, 0.0, 1.0],
        min: [100.0, 0.0, 0.0],
        max: [100.0, 0.0, 0.0],
    });
    let mut sim = Sim::new(2);
    sim.step(&sys, 4.0 / 60.0);
    assert_eq!(sim.particles.len(), 4);
    let expected = [[100.0, 0.0], [0.0, -100.0], [-100.0, 0.0], [0.0, 100.0]];
    let radius = 50.0f32.hypot(20.0);
    for (particle, want) in sim.particles.iter().zip(expected) {
        let relative = [particle.pos[0] - 50.0, particle.pos[1] - 20.0];
        assert!((relative[0] - radius * -want[1] / 100.0).abs() < 1e-3);
        assert!((relative[1] - radius * want[0] / 100.0).abs() < 1e-3);
        assert!(
            (particle.vel[0] - want[0]).abs() < 1e-3 && (particle.vel[1] - want[1]).abs() < 1e-3,
            "{:?}",
            particle.vel
        );
    }
    sys.emitters = vec![point_emitter([30.0, 40.0, 0.0], 60.0, 0)];
    let mut sim = Sim::new(2);
    sim.step(&sys, 3.0 / 60.0);
    let radius = 20.0f32.hypot(20.0);
    for (particle, want) in sim.particles.iter().zip(expected) {
        let along = [particle.pos[0] - 50.0, particle.pos[1] - 20.0];
        assert!((along[0] - radius * -want[1] / 100.0).abs() < 1e-3);
        assert!((along[1] - radius * want[0] / 100.0).abs() < 1e-3);
    }
}

#[test]
fn map_sequence_follows_pointer() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.operators.clear();
    sys.origin = (1280.0, 720.0, 0.0);
    sys.control_points[0].flags = 1;
    sys.emitters = vec![point_emitter([1.0, 0.0, 0.0], 0.0, 1)];
    sys.initializers.push(Initializer::MapSequence {
        control_point: 0,
        count: 4.0,
        bounds: [0.0, 1.0],
        mirror: false,
        axis: [0.0, 0.0, 1.0],
        min: [0.0; 3],
        max: [0.0; 3],
    });
    for (pointer, center) in [([640.0, 720.0], -640.0), ([1920.0, 720.0], 640.0)] {
        let mut sim = Sim::new(17);
        sim.set_pointer(pointer);
        sim.step(&sys, 0.01);
        assert_eq!(sim.particles.len(), 1);
        let particle = sim.particles[0];
        assert!((particle.pos[0] - center).abs() < 1e-4);
        assert!((particle.pos[1] - 1.0).abs() < 1e-4);
        assert_eq!(particle.vel, [0.0; 3]);
    }
}

#[test]
fn map_preserves_axial_velocity() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.operators.clear();
    sys.emitters = vec![point_emitter([53.0, 24.0, 12.0], 0.0, 1)];
    sys.control_points[2].offset = [50.0, 20.0, 0.0];
    sys.initializers.push(Initializer::Velocity {
        min: [5.0, 7.0, 11.0],
        max: [5.0, 7.0, 11.0],
        exponent: 1.0,
    });
    sys.initializers.push(Initializer::MapSequence {
        control_point: 2,
        count: 4.0,
        bounds: [0.0, 1.0],
        mirror: false,
        axis: [0.0, 0.0, 1.0],
        min: [10.0, 20.0, 30.0],
        max: [10.0, 20.0, 30.0],
    });
    let sim = spawn_one(&sys);
    assert_eq!(sim.particles[0].pos, [50.0, 25.0, 12.0]);
    assert_eq!(sim.particles[0].vel, [15.0, 27.0, 41.0]);
}

#[test]
fn map_sequence_rescales_world() {
    let mut sys = system();
    sys.scale3 = [0.5; 3];
    sys.emitters = vec![point_emitter([100.0, 0.0, 0.0], 60.0, 0)];
    sys.initializers.push(Initializer::MapSequence {
        control_point: 0,
        count: 4.0,
        bounds: [0.0, 1.0],
        mirror: false,
        axis: [0.0, 0.0, 1.0],
        min: [0.0, 10.0, 0.0],
        max: [0.0, 10.0, 0.0],
    });
    for (world, radius, speed) in [(false, 100.0, 10.0), (true, 50.0, 5.0)] {
        sys.world = world;
        let mut sim = Sim::new(2);
        sim.step(&sys, 4.0 / 60.0);
        assert_eq!(sim.particles.len(), 4);
        for particle in &sim.particles {
            assert!(
                (particle.pos[0].hypot(particle.pos[1]) - radius).abs() < 1e-3,
                "{:?}",
                particle.pos
            );
            assert!(
                (particle.vel[0].hypot(particle.vel[1]) - speed).abs() < 1e-4,
                "{:?}",
                particle.vel
            );
        }
    }
}

#[test]
fn sphere_launch_ignores_scale() {
    for scale in [[0.127; 3], [0.5, 1.0, 1.0]] {
        let mut sys = system();
        sys.world = true;
        sys.scale3 = scale;
        sys.emitters = vec![
            parse_emitter(&serde_json::json!({"name": "sphererandom", "rate": 60, "distancemin": 300, "distancemax": 300, "speedmin": 40, "speedmax": 40})).unwrap(),
        ];
        let mut sim = Sim::new(4);
        sim.step(&sys, 4.0 / 60.0);
        assert!(!sim.particles.is_empty());
        for particle in &sim.particles {
            let ([x, y, z], [vx, vy, vz]) = (particle.pos, particle.vel);
            assert!((vx.hypot(vy) - 40.0).abs() < 1e-3, "{:?}", particle.vel);
            let ellipse = (x / (300.0 * scale[0])).hypot(y / (300.0 * scale[1]));
            assert!((ellipse - 1.0).abs() < 1e-4, "{:?}", particle.pos);
            assert!((x * vy - y * vx).abs() < 1e-3 * x.hypot(y) * 40.0, "{:?}", particle.pos);
            assert_eq!([z, vz], [0.0; 2]);
        }
    }
}

#[test]
fn map_sequence_phase_bounds() {
    let mut repeat = SequencePhase { phase: 0.75, step: 0.25 };
    repeat.advance(false);
    assert_eq!(repeat.phase, 1.0);
    repeat.advance(false);
    assert_eq!(repeat.phase, 0.25);
    let mut mirror = SequencePhase { phase: 1.0, step: 0.25 };
    mirror.advance(true);
    assert_eq!(mirror.phase, 0.75);
    assert_eq!(mirror.step, -0.25);
    mirror.phase = 0.0;
    mirror.advance(true);
    assert_eq!(mirror.phase, 0.25);
    assert_eq!(mirror.step, 0.25);
}

#[test]
fn map_fractional_phase_wraps() {
    let mut phase = SequencePhase { phase: 0.0, step: 1.0 / 3.02 };
    for expected in [0.331_125_83, 0.662_251_65, 0.993_377_45, 0.324_503_3] {
        phase.advance(false);
        assert_eq!(phase.phase, expected);
    }
}

#[test]
fn map_states_stay_independent() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.operators.clear();
    sys.emitters = vec![point_emitter([1.0, 0.0, 0.0], 0.0, 1)];
    for count in [2.0, 4.0] {
        sys.initializers.push(Initializer::MapSequence {
            control_point: 0,
            count,
            bounds: [0.0, 1.0],
            mirror: false,
            axis: [0.0, 0.0, 1.0],
            min: [0.0; 3],
            max: [0.0; 3],
        });
    }
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.01);
    assert_eq!(sim.sequence_phases[2].phase, 0.5);
    assert_eq!(sim.sequence_phases[3].phase, 0.25);
    sim.pending = 1.0;
    sim.step(&sys, 0.01);
    assert_eq!(sim.sequence_phases[2].phase, 1.0);
    assert_eq!(sim.sequence_phases[3].phase, 0.5);
}

#[test]
fn map_bounds_turn_position() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.operators.clear();
    sys.emitters = vec![point_emitter([1.0, 0.0, 0.0], 0.0, 1)];
    sys.initializers.push(Initializer::MapSequence {
        control_point: 0,
        count: 4.0,
        bounds: [0.25, 0.75],
        mirror: false,
        axis: [0.0, 0.0, 1.0],
        min: [0.0; 3],
        max: [0.0; 3],
    });
    let sim = spawn_one(&sys);
    assert!((sim.particles[0].pos[0] - 1.0).abs() < 1e-4);
    assert!(sim.particles[0].pos[1].abs() < 1e-4);
}

#[test]
fn map_sequence_vertical_basis() {
    let (axis, tangent, normal) = map_basis([0.0, 0.0, -1.0]);
    assert_eq!(axis, [0.0, 0.0, -1.0]);
    assert_eq!(tangent, [1.0, 0.0, 0.0]);
    assert_eq!(normal, [0.0, 1.0, 0.0]);
    assert_eq!(map_basis([0.0; 3]).0, [0.0, 0.0, 1.0]);
}

#[test]
fn map_tiny_axis_survives() {
    let (axis, _, _) = map_basis([1e-30, 0.0, 0.0]);
    assert!(axis[0].is_infinite());
    assert!(axis[1].is_nan());
}

#[test]
fn map_control_point_index() {
    for (entry, expected) in [
        (serde_json::json!({"name": "mapsequencearoundcontrolpoint"}), 0),
        (
            serde_json::json!({"name": "mapsequencearoundcontrolpoint", "controlpoint": "invalid"}),
            0,
        ),
        (serde_json::json!({"name": "mapsequencearoundcontrolpoint", "controlpoint": -1}), 7),
        (serde_json::json!({"name": "mapsequencearoundcontrolpoint", "controlpoint": 7}), 7),
    ] {
        let parsed = parse_initializer(&entry).unwrap();
        let Initializer::MapSequence { control_point, .. } = parsed else { panic!("map") };
        assert_eq!(control_point, expected);
    }
}

#[test]
fn map_sequence_parser_defaults() {
    let parsed = parse_initializer(&serde_json::json!({
        "name":"mapsequencearoundcontrolpoint"
    }))
    .unwrap();
    let Initializer::MapSequence { count, bounds, mirror, .. } = parsed else { panic!("map") };
    assert_eq!(count, 32.0);
    assert_eq!(bounds, [0.0, 1.0]);
    assert!(!mirror);
    let parsed = parse_initializer(&serde_json::json!({
        "name":"mapsequencearoundcontrolpoint", "count":3.02,
        "bounds":"0.25 0.75", "limitbehavior":"mirror"
    }))
    .unwrap();
    let Initializer::MapSequence { count, bounds, mirror, .. } = parsed else { panic!("map") };
    assert_eq!(count, 3.02);
    assert_eq!(bounds, [0.25, 0.75]);
    assert!(mirror);
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
        base_color: [1.0; 3],
        alpha: 1.0,
        base_alpha: 1.0,
        angle: 0.0,
        angle_xy: [0.0; 2],
        angular: 0.0,
        angular_xy: [0.0; 2],
        age: 0.0,
        lifetime: 1.0,
        phase: 0.0,
        noise_seed: 0,
        slot: 0,
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
    assert_eq!((v.distance, v.speed, v.infinite_axis), ((0.0, 32.0), (0.0, 2500.0), true));
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
    let context = LoadContext {
        pkg: &pkg,
        assets: &assets,
        defaults: ParticleDefaults::ORTHOGRAPHIC,
        viewport: None,
        bounds_static: false,
        version: 0,
    };
    super::load(&context, &serde_json::json!({"instanceoverride":overrides}), "particles/p.json")
        .0
        .unwrap()
}

fn tint_system(definition: &Value, overrides: &Value, version: u32) -> ParticleSystem {
    let mut definition = definition.clone();
    definition["renderer"] = serde_json::json!([{"name":"sprite"}]);
    definition["emitter"] = serde_json::json!([{"name":"boxrandom","instantaneous":1}]);
    let text = definition.to_string();
    let pkg = crate::pkg::Package::parse(crate::tests::build_pkg(&[(
        "particles/tint.json",
        text.as_bytes(),
    )]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let context = LoadContext {
        pkg: &pkg,
        assets: &assets,
        defaults: ParticleDefaults::ORTHOGRAPHIC,
        viewport: None,
        bounds_static: false,
        version,
    };
    super::load(&context, &serde_json::json!({"instanceoverride":overrides}), "particles/tint.json")
        .0
        .unwrap()
}

fn assert_color(actual: [f32; 3], expected: [f32; 3]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
    }
}

#[test]
fn color_override_follows_version() {
    let definition = serde_json::json!({
        "initializer":[{"name":"colorrandom","min":"255 255 255","max":"255 255 255"}]
    });
    let overrides = serde_json::json!({"colorn":"1 0.5019608 1"});
    for (version, green) in [(0, 0.501_960_8_f32.powi(2)), (5, 0.501_960_8)] {
        let system = tint_system(&definition, &overrides, version);
        let sim = spawn_one(&system);
        assert_color(sim.particles[0].color, [1.0, green, 1.0]);
    }
}

#[test]
fn uncolored_override_both_versions() {
    let overrides = serde_json::json!({"colorn":"1 0.5019608 1"});
    for version in [0, 5] {
        let system = tint_system(&serde_json::json!({}), &overrides, version);
        assert_color(spawn_one(&system).particles[0].color, [1.0, 0.501_960_8, 1.0]);
    }
}

#[test]
fn legacy_color_is_normalized() {
    let system =
        tint_system(&serde_json::json!({}), &serde_json::json!({"color":"255 128 255"}), 5);
    assert_color(system.base_color, [1.0, 0.501_96, 1.0]);
    assert_color(spawn_one(&system).particles[0].color, [1.0, 0.501_96, 1.0]);
}

#[test]
fn last_color_initializer_wins() {
    let definition = serde_json::json!({
        "initializer":[
            {"name":"colorrandom","min":"255 255 255","max":"255 255 255"},
            {"name":"colorrandom","min":"0 255 255","max":"0 255 255"}
        ],
        "operator":[{"name":"colorchange","startvalue":"1 0 0"}]
    });
    assert_color(super::authored_color(&definition).unwrap(), [0.0, 1.0, 1.0]);
    let fallback = serde_json::json!({
        "operator":[{"name":"colorchange","startvalue":"0.25 0.5 1"}]
    });
    assert_color(super::authored_color(&fallback).unwrap(), [0.25, 0.5, 1.0]);
    assert!(
        super::authored_color(&serde_json::json!({"initializer":[
            {"name":"colorlist","colors":[]}
        ]}))
        .is_none()
    );
}

#[test]
fn color_override_respects_guards() {
    let plain = serde_json::json!({
        "initializer":[{"name":"colorrandom","min":"0 255 255","max":"0 255 255"}]
    });
    let untouched = tint_system(&plain, &serde_json::json!({}), 0);
    assert_color(spawn_one(&untouched).particles[0].color, [0.0, 1.0, 1.0]);
    let equal = tint_system(&plain, &serde_json::json!({"colorn":"0 1 1"}), 0);
    assert_color(spawn_one(&equal).particles[0].color, [0.0, 1.0, 1.0]);
    let colored = serde_json::json!({
        "flags":8,
        "initializer":[{"name":"colorrandom","min":"255 255 255","max":"255 255 255"}]
    });
    let system = tint_system(&colored, &serde_json::json!({"colorn":"1 0.5 1"}), 0);
    assert_color(spawn_one(&system).particles[0].color, [1.0; 3]);
    let mut enabled = colored;
    enabled["flags"] = serde_json::json!(0);
    let system = tint_system(&enabled, &serde_json::json!({"colorn":"-1 0.5 1"}), 0);
    assert_color(spawn_one(&system).particles[0].color, [1.0; 3]);
}

#[test]
fn child_inherits_root_override() {
    let root = serde_json::json!({
        "renderer":[{"name":"sprite"}],
        "emitter":[{"name":"boxrandom","instantaneous":1}],
        "children":[{
            "name":"particles/child.json",
            "type":"eventfollow",
            "instanceoverride":{"count":0.75,"colorn":"0 1 0"}
        }]
    })
    .to_string();
    let child = serde_json::json!({
        "renderer":[{"name":"sprite"}],
        "emitter":[{"name":"boxrandom","instantaneous":1}],
        "initializer":[{"name":"colorrandom","min":"0 255 255","max":"0 255 255"}]
    })
    .to_string();
    let pkg = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("particles/root.json", root.as_bytes()),
        ("particles/child.json", child.as_bytes()),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let context = LoadContext {
        pkg: &pkg,
        assets: &assets,
        defaults: ParticleDefaults::ORTHOGRAPHIC,
        viewport: None,
        bounds_static: false,
        version: 5,
    };
    let root = super::load(
        &context,
        &serde_json::json!({"instanceoverride":{"colorn":"1 0.5019608 1"}}),
        "particles/root.json",
    )
    .0
    .unwrap();
    let child = &root.children[0];
    assert_color(child.base_color, [1.0; 3]);
    assert_color(spawn_one(child).particles[0].color, [1.0, 0.501_960_8, 1.0]);

    let root = super::load(&context, &serde_json::json!({}), "particles/root.json").0.unwrap();
    let child = &root.children[0];
    assert_color(child.base_color, [1.0; 3]);
    assert_color(spawn_one(child).particles[0].color, [0.0, 1.0, 1.0]);
    assert_eq!(child.count_scale, 0.75);
}

#[test]
fn count_multiplier_controls_steady_emission_and_zero_disables_it() {
    for (count, bounds) in [(0.18, 25..=28), (1.0, 150..=157), (0.0, 0..=0)] {
        let sys = overridden_system(&serde_json::json!({"count":count}), &serde_json::json!([]));
        let mut sim = Sim::new(7);
        for _ in 0..120 {
            sim.step(&sys, 1.0 / 15.0);
        }
        assert!(bounds.contains(&sim.particles.len()), "count={count}: {}", sim.particles.len());
    }
}

#[test]
fn count_preserves_capacity() {
    let definition = br#"{"maxcount":4,"emitter":[
        {"name":"boxrandom","origin":"-10 0 0","instantaneous":4},
        {"name":"boxrandom","origin":"10 0 0","instantaneous":4}]}"#;
    let pkg =
        crate::pkg::Package::parse(crate::tests::build_pkg(&[("particles/p.json", definition)]))
            .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let context = LoadContext {
        pkg: &pkg,
        assets: &assets,
        defaults: ParticleDefaults::ORTHOGRAPHIC,
        viewport: None,
        bounds_static: false,
        version: 0,
    };
    for count in [
        serde_json::json!(0.0),
        serde_json::json!(0.5),
        serde_json::json!(1.0),
        serde_json::json!(2.0),
        serde_json::json!({"value":0.5}),
        serde_json::json!({"value":2.0}),
    ] {
        let sys = super::load(
            &context,
            &serde_json::json!({"instanceoverride":{"count":count}}),
            "particles/p.json",
        )
        .0
        .unwrap();
        assert_eq!(sys.max_count, 4);
        let mut sim = Sim::new(17);
        sim.step(&sys, 0.0);
        assert_eq!(sim.particles.len(), 4);
        assert!(sim.particles.iter().all(|particle| particle.pos[0] == -10.0));
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
    assert!((slow.particles[0].age - (0.25 - 3.0 / 240.0)).abs() < 0.001);
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
fn instance_birth_factors() {
    let mut sys = system();
    sys.initializers = vec![
        Initializer::Lifetime { min: 4.0, max: 4.0, exponent: 1.0 },
        Initializer::Velocity { min: [2.0, 0.0, 0.0], max: [2.0, 0.0, 0.0], exponent: 1.0 },
        Initializer::Color { min: [128.0, 255.0, 64.0], max: [128.0, 255.0, 64.0], exponent: 1.0 },
    ];
    sys.set_color([0.5, 0.25, 1.0]);
    sys.set_speed(2.0);
    sys.set_lifetime(0.5);
    let mut sim = Sim::new(7);
    sim.emit_particles(&sys, 1, &mut |_| {});
    assert_eq!(sim.particles[0].vel, [4.0, 0.0, 0.0]);
    assert_eq!(sim.particles[0].lifetime, 2.0);
    assert!((sim.particles[0].color[0] - 64.0 / 255.0).abs() < 0.0001);
    assert_eq!(sim.particles[0].color[1], 0.25);
    sys.set_color([1.0; 3]);
    sys.set_speed(0.0);
    sys.set_speed(3.0);
    sys.set_lifetime(1.0);
    sim.emit_particles(&sys, 1, &mut |_| {});
    assert_eq!(sim.particles[1].vel, [6.0, 0.0, 0.0]);
    assert_eq!(sim.particles[1].lifetime, 4.0);
    assert_eq!(sim.particles[1].color[1], 1.0);
}

#[test]
fn zero_instance_lifetime() {
    let mut sys = system();
    sys.initializers = vec![Initializer::Lifetime { min: 4.0, max: 4.0, exponent: 1.0 }];
    sys.set_lifetime(0.0);
    let mut sim = Sim::new(7);
    sim.emit_particles(&sys, 1, &mut |_| {});
    assert_eq!(sim.particles[0].lifetime, 0.01);
    assert_eq!(particle_life_fraction(&sim.particles[0]), 0.0);
}

#[test]
fn instance_force_and_color() {
    let mut sys = single(Operator::Movement { gravity: [10.0, 0.0, 0.0], drag: 0.0 });
    let mut sim = spawn_one(&sys);
    sim.particles[0].vel = [0.0; 3];
    sys.set_speed(0.0);
    sim.step(&sys, 0.1);
    assert_eq!(sim.particles[0].vel[0], 0.0);
    sys.set_speed(2.0);
    sys.set_color([0.0, 1.0, 0.0]);
    sim.step(&sys, 0.1);
    assert_eq!(sim.particles[0].vel[0], 2.0);
    assert_eq!(sim.particles[0].color, [0.0, 1.0, 0.0]);
    sys.set_color([1.0; 3]);
    sim.step(&sys, 0.1);
    assert_eq!(sim.particles[0].color, [1.0; 3]);
}

#[test]
fn child_inherits_instance_factors() {
    let mut parent = system();
    parent.set_color([0.25, 0.5, 1.0]);
    parent.set_speed(2.0);
    parent.set_lifetime(0.5);
    let mut child = system();
    child.initializers = vec![
        Initializer::Lifetime { min: 4.0, max: 4.0, exponent: 1.0 },
        Initializer::Velocity { min: [3.0, 0.0, 0.0], max: [3.0, 0.0, 0.0], exponent: 1.0 },
        Initializer::Color { min: [255.0; 3], max: [255.0; 3], exponent: 1.0 },
    ];
    child.inherit_instance_factors(&parent);
    let mut sim = Sim::new(1);
    sim.emit_particles(&child, 1, &mut |_| {});
    assert_eq!(sim.particles[0].color, [0.25, 0.5, 1.0]);
    assert_eq!(sim.particles[0].vel, [6.0, 0.0, 0.0]);
    assert_eq!(sim.particles[0].lifetime, 2.0);
}

#[test]
fn instance_control_point() {
    let mut sys = single(Operator::ControlPointAttract {
        control_point: 1,
        flags: 0,
        delete_threshold: 0.0,
        scale: 100.0,
        threshold: 100.0,
        blend: None,
    });
    assert!(!sys.set_control_point(CONTROL_POINT_COUNT, [1.0; 3]));
    assert!(!sys.set_control_point(1, [f32::NAN, 0.0, 0.0]));
    assert!(sys.set_control_point(1, [20.0, 0.0, 0.0]));
    let mut sim = spawn_one(&sys);
    sim.step(&sys, 0.01);
    assert!(sim.particles[0].vel[0] > 0.0);
    assert!(sys.set_control_point(1, [-20.0, 0.0, 0.0]));
    sim.particles[0].vel = [0.0; 3];
    sim.step(&sys, 0.01);
    assert!(sim.particles[0].vel[0] < 0.0);
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
            blend: None,
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
    let osc = Oscillator {
        frequency: (2.0, 2.0),
        scale: (0.2, 0.8),
        phase: (1.0, 1.0),
        mask: [1.0; 3],
        blend: None,
    };
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
        blend: None,
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
    let context = LoadContext {
        pkg: &pkg,
        assets: &assets,
        defaults: ParticleDefaults::ORTHOGRAPHIC,
        viewport: None,
        bounds_static: false,
        version: 0,
    };
    let system =
        load(&context, &serde_json::json!({"instanceoverride":{"count":0.2}}), "root.json")
            .0
            .unwrap();
    assert_eq!(system.children.len(), 1);
    let child = &system.children[0];
    assert_eq!(child.child_event.unwrap().limit, 3);
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
    child.child_event = Some(child_event(ChildEventKind::Follow));
    child.max_count = 20;
    child.emitters =
        vec![parse_emitter(&serde_json::json!({"name":"boxrandom","rate":10})).unwrap()];
    child.initializers = vec![Initializer::Lifetime { min: 2.0, max: 2.0, exponent: 1.0 }];
    let mut source = Sim::new(1);
    let mut output = Sim::new(2);
    let mut followers = Followers::default();
    for _ in 0..90 {
        source.step(&parent, 1.0 / 15.0);
        followers.step(
            &mut output,
            &child,
            (&parent, &source),
            1.0 / 15.0,
            (&[], &[]),
            &mut |_| {},
        );
    }
    assert!((18..=20).contains(&output.particles.len()));
    let head = source.particles[0].pos[0];
    let tail =
        output.particles.iter().map(|particle| particle.pos[0]).fold(f32::INFINITY, f32::min);
    assert!((head - tail - 120.0).abs() < 12.0, "{head} {tail}");
    source.particles.clear();
    for _ in 0..40 {
        followers.step(
            &mut output,
            &child,
            (&parent, &source),
            1.0 / 15.0,
            (&[], &[]),
            &mut |_| {},
        );
    }
    assert!(output.particles.is_empty());
}

#[test]
fn local_trail_padding() {
    let mut parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    parent.emitters = vec![point_emitter([40.0, 20.0, 30.0], 0.0, 1)];
    let mut source = spawn_one(&parent);
    let mut child = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    child.world = false;
    child.renderer = Renderer::Ribbon;
    child.child_event = Some(child_event(ChildEventKind::Follow));
    child.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
    let mut followers = Followers::default();
    let mut output = Sim::new(18);
    let mut packed = Vec::new();
    for position in [[40.0, 20.0, 30.0], [70.0, 50.0, -15.0]] {
        source.particles[0].pos = position;
        followers.step(&mut output, &child, (&parent, &source), 0.01, (&[], &[]), &mut |_| {});
        pack_rope_trail_points(&output, child.primary_render(), &mut packed);
        assert_eq!(packed.len(), child.trail.segments * ROPE_POINT_FLOATS);
        for segment in packed.chunks_exact(ROPE_POINT_FLOATS) {
            for offset in [0, 4, 8, 12] {
                assert_eq!(&segment[offset..offset + 3], &position);
            }
        }
    }
}

#[test]
fn world_trail_padding() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.world = true;
    sys.renderer = Renderer::Ribbon;
    sys.emitters = vec![point_emitter([20.0, 30.0, 40.0], 0.0, 1)];
    let mut sim = spawn_one(&sys);
    sys.origin = (5.0, 7.0, 9.0);
    sim.step(&sys, 0.01);
    let mut packed = Vec::new();
    pack_rope_trail_points(&sim, sys.primary_render(), &mut packed);
    assert_eq!(packed.len(), sys.trail.segments * ROPE_POINT_FLOATS);
    for segment in packed.chunks_exact(ROPE_POINT_FLOATS) {
        for offset in [0, 4, 8, 12] {
            assert_eq!(&segment[offset..offset + 3], &[15.0, 23.0, 31.0]);
        }
    }
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
    child.child_event = Some(child_event(ChildEventKind::Follow));
    child.max_count = 1;
    let mut source = Sim::new(1);
    let mut output = Sim::new(2);
    let mut followers = Followers::default();
    for _ in 0..90 {
        source.step(&parent, 1.0 / 15.0);
        followers.step(
            &mut output,
            &child,
            (&parent, &source),
            1.0 / 15.0,
            (&[], &[]),
            &mut |_| {},
        );
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
    assert_eq!(sim.particles.len(), 1);
    assert_eq!(sim.particles[0].age, sim.particles[0].lifetime);
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
        sys.emitters = vec![point_emitter([0.0, y, 0.0], 10.0, 1)];
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
    assert!((sim.particles[0].alpha - 0.416_666_66).abs() < 1e-5);
    sim.particles[0].age = 70.0 - 0.01;
    sim.step(&sys, 0.01);
    assert!((sim.particles[0].alpha - 0.083_333_33).abs() < 1e-5);
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

#[test]
fn bounds_viewport_inputs() {
    for (projection, expected) in [
        (serde_json::json!({"width":640,"height":360}), true),
        (serde_json::json!({"width":640.0,"height":360.0}), true),
        (serde_json::json!({"width":640.5,"height":360}), false),
        (serde_json::json!({"width":640,"height":360,"auto":true}), false),
        (serde_json::json!({"width":640,"height":-360}), false),
        (serde_json::json!({"width":0,"height":360}), false),
        (serde_json::json!({"width":i32::MAX,"height":360}), false),
        (serde_json::json!({"width":2_147_483_520,"height":360}), true),
        (serde_json::json!({"auto":true}), false),
    ] {
        let scene = serde_json::json!({"general":{"orthogonalprojection":projection}});
        assert_eq!(BoundsViewport::of(&scene).is_some(), expected);
    }
}

#[test]
fn bounds_inverse_coefficients() {
    let model = ParticleTransform { origin: [100.0, 0.0, 0.0], angle: 0.0, scale: [2.0, 3.0, 1.0] };
    let planes = model.planes(BoundsViewport { width: 640, height: 360 }, false);
    assert_eq!(planes[0].normal, [0.5, 0.0, 0.0]);
    assert_eq!(planes[0].distance, -25.0);
    let mut sys = single(Operator::CollisionBounds {
        planes,
        collision: Collision { response: CollisionResponse::Stop, stop_angular: false },
    });
    sys.origin = (100.0, 0.0, 0.0);
    sys.scale3 = [2.0, 3.0, 1.0];
    let mut sim = spawn_one(&sys);
    sim.particles[0].pos = [-60.0, 100.0, 0.0];
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles[0].pos, [-57.5, 100.0, 0.0]);
}

#[test]
fn bounds_rotated_corner() {
    let model = ParticleTransform {
        origin: [0.0; 3],
        angle: std::f32::consts::FRAC_PI_4,
        scale: [2.0, 1.0, 1.0],
    };
    let planes = model.planes(BoundsViewport { width: 640, height: 360 }, false);
    assert!((planes[2].distance + 265.0).abs() < 0.0001);
    assert!((planes[3].distance - 15.0).abs() < 0.0001);
}

#[test]
fn bounds_resolved_transform() {
    let object = serde_json::json!({"origin":"5 0 0"});
    let model = ParticleTransform::of(&object).unwrap();
    let mut sys = single(Operator::CollisionBounds {
        planes: model.planes(BoundsViewport { width: 640, height: 360 }, false),
        collision: Collision { response: CollisionResponse::Stop, stop_angular: false },
    });
    sys.origin = (5.0, 0.0, 0.0);
    assert!(!sys.validate_bounds(&object));
    sys.operators.push(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.origin.0 = 6.0;
    assert!(sys.validate_bounds(&object));
    assert_eq!(sys.operators.len(), 1);
    assert!(matches!(sys.operators[0], Operator::Movement { .. }));
}

#[test]
fn bounds_corner_priority() {
    let model = ParticleTransform { origin: [0.0; 3], angle: 0.0, scale: [1.0; 3] };
    let planes = model.planes(BoundsViewport { width: 640, height: 360 }, false);
    let sys = single(Operator::CollisionBounds {
        planes,
        collision: Collision { response: CollisionResponse::Stop, stop_angular: false },
    });
    let mut sim = spawn_one(&sys);
    sim.particles[0].pos = [-10.0, 370.0, 0.0];
    sim.particles[0].vel = [20.0, -30.0, 0.0];
    sim.particles[0].angular = 10.0;
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles[0].pos, [-10.0, 360.0, 0.0]);
    assert_eq!(sim.particles[0].vel, [0.0; 3]);
    assert_eq!(sim.particles[0].angular, 10.0);
    sim.particles[0].pos = [0.0, 360.0, 0.0];
    sim.particles[0].vel = [20.0, -30.0, 0.0];
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles[0].vel, [20.0, -30.0, 0.0]);
}

#[test]
fn bounds_four_edges() {
    let model = ParticleTransform { origin: [0.0; 3], angle: 0.0, scale: [1.0; 3] };
    let sys = single(Operator::CollisionBounds {
        planes: model.planes(BoundsViewport { width: 640, height: 360 }, false),
        collision: Collision { response: CollisionResponse::Stop, stop_angular: false },
    });
    for (before, after) in [
        ([-1.0, 90.0, 0.0], [0.0, 90.0, 0.0]),
        ([220.0, -1.0, 0.0], [220.0, 0.0, 0.0]),
        ([641.0, 270.0, 0.0], [640.0, 270.0, 0.0]),
        ([420.0, 361.0, 0.0], [420.0, 360.0, 0.0]),
    ] {
        let mut sim = spawn_one(&sys);
        sim.particles[0].pos = before;
        sim.step(&sys, 0.01);
        assert_eq!(sim.particles[0].pos, after);
    }
}

#[test]
fn box_preserves_motion() {
    for flags in 0..=3 {
        for behavior in ["stop", "bounce", "delete"] {
            let operator = parse_operator(&serde_json::json!({
                "name":"collisionbox", "origin":"0 -100 0", "size":"80 40 20",
                "angles":"0 0 45", "controlpoint":7, "flags":flags,
                "collisionbehavior":behavior, "bouncefactor":2
            }))
            .unwrap();
            let mut candidate = single(operator);
            candidate.operators.push(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
            let control = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
            let mut actual = spawn_one(&candidate);
            let mut expected = spawn_one(&control);
            for sim in [&mut actual, &mut expected] {
                sim.emitting = false;
                sim.particles[0].pos = [-80.0, -100.0, 0.0];
                sim.particles[0].vel = [80.0, 0.0, 0.0];
                sim.particles[0].angular = 10.0;
            }
            for _ in 0..60 {
                actual.step(&candidate, 1.0 / 30.0);
                expected.step(&control, 1.0 / 30.0);
            }
            assert_eq!(actual.particles.len(), 1);
            let actual = &actual.particles[0];
            let expected = &expected.particles[0];
            assert_eq!(actual.pos, expected.pos);
            assert_eq!(actual.vel, expected.vel);
            assert_eq!(actual.angular, expected.angular);
            assert_eq!(actual.lifetime, expected.lifetime);
        }
    }
}

#[test]
fn quad_extent() {
    for (size, x, expected_y, expected_velocity) in [
        ("100 100", 30.0, -100.0, [0.0; 3]),
        ("20 100", 30.0, -110.0, [0.0, -40.0, 0.0]),
        ("100 100", 60.0, -110.0, [0.0, -40.0, 0.0]),
    ] {
        let op = parse_operator(&serde_json::json!({
            "name":"collisionquad", "origin":"0 -100 0", "size":size,
            "collisionbehavior":"stop", "flags":0
        }))
        .unwrap();
        let sys = single(op);
        let mut sim = spawn_one(&sys);
        sim.particles[0].pos = [x, -110.0, 0.0];
        sim.particles[0].vel = [0.0, -40.0, 0.0];
        sim.step(&sys, 0.01);
        assert_eq!(sim.particles[0].pos, [x, expected_y, 0.0]);
        assert_eq!(sim.particles[0].vel, expected_velocity);
    }
}

#[test]
fn quad_requires_supported_geometry() {
    for entry in [
        serde_json::json!({"name":"collisionquad","origin":"0 -100 0"}),
        serde_json::json!({"name":"collisionquad","size":"100 100"}),
        serde_json::json!({"name":"collisionquad","origin":"0 -100 0","size":"0 100"}),
        serde_json::json!({"name":"collisionquad","origin":"0 -100 0","size":"100 100 100"}),
        serde_json::json!({"name":"collisionquad","origin":"0 -100 0","size":"100 100","flags":1}),
    ] {
        assert!(parse_operator(&entry).is_none(), "{entry}");
    }
}

#[test]
fn bounds_flags_ignore() {
    let bytes = crate::tests::build_pkg(&[]);
    let pkg = crate::pkg::Package::parse(bytes).unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let context = LoadContext {
        pkg: &pkg,
        assets: &assets,
        defaults: ParticleDefaults::ORTHOGRAPHIC,
        viewport: Some(BoundsViewport { width: 640, height: 360 }),
        bounds_static: true,
        version: 0,
    };
    let entry = serde_json::json!({
        "name":"collisionbounds","flags":3,"controlpoint":7,"collisionbehavior":"stop"
    });
    let object = serde_json::json!({"origin":"0 0 0","scale":"1 1 1"});
    let doc = serde_json::json!({"flags":0});
    let Some(Operator::CollisionBounds { collision, .. }) =
        parse_bounds_operator(&entry, &context, &object, &doc, 0)
    else {
        panic!("bounds")
    };
    assert!(!collision.stop_angular);
    assert!(matches!(collision.response, CollisionResponse::Stop));
    assert!(matches!(
        collision_behavior(&serde_json::json!({"collisionbehavior":"unknown"})),
        CollisionResponse::Bounce(0.5)
    ));
    assert!(matches!(
        collision_behavior(&serde_json::json!({"collisionbehavior":"slide"})),
        CollisionResponse::Slide
    ));
    assert!(matches!(
        collision_behavior(&serde_json::json!({"collisionbehavior":"delete"})),
        CollisionResponse::Delete
    ));
}

#[test]
fn bounds_delete_defers() {
    let model = ParticleTransform { origin: [0.0; 3], angle: 0.0, scale: [1.0; 3] };
    let sys = single(Operator::CollisionBounds {
        planes: model.planes(BoundsViewport { width: 640, height: 360 }, false),
        collision: Collision { response: CollisionResponse::Delete, stop_angular: false },
    });
    let mut sim = spawn_one(&sys);
    sim.particles[0].pos = [-1.0, 20.0, 0.0];
    sim.particles[0].vel = [2.0, 3.0, 0.0];
    sim.step(&sys, 0.01);
    assert_eq!(sim.particles.len(), 1);
    assert_eq!(sim.particles[0].pos, [-1.0, 20.0, 0.0]);
    assert_eq!(sim.particles[0].vel, [2.0, 3.0, 0.0]);
    assert_eq!(sim.particles[0].age, sim.particles[0].lifetime);
    sim.step(&sys, 0.01);
    assert!(sim.particles.is_empty());
}

#[test]
fn bounds_static_gate() {
    let bytes = crate::tests::build_pkg(&[]);
    let pkg = crate::pkg::Package::parse(bytes).unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let mut context = LoadContext {
        pkg: &pkg,
        assets: &assets,
        defaults: ParticleDefaults::ORTHOGRAPHIC,
        viewport: Some(BoundsViewport { width: 640, height: 360 }),
        bounds_static: true,
        version: 0,
    };
    let entry = serde_json::json!({"name":"collisionbounds"});
    let object = serde_json::json!({"origin":"0 0 0","scale":"1 1 1"});
    let doc = serde_json::json!({"flags":0});
    assert!(parse_bounds_operator(&entry, &context, &object, &doc, 0).is_some());
    assert!(parse_bounds_operator(&entry, &context, &object, &doc, 1).is_none());
    for unsupported in [
        serde_json::json!({"parent":1}),
        serde_json::json!({"angles":"1 0 0"}),
        serde_json::json!({"scale":"0 1 1"}),
        serde_json::json!({"origin":"bad"}),
    ] {
        assert!(parse_bounds_operator(&entry, &context, &unsupported, &doc, 0).is_none());
    }
    let huge_scale = serde_json::json!({"scale":"1e20 1e20 1e20"});
    assert!(ParticleTransform::of(&huge_scale).is_none());
    assert!(parse_bounds_operator(&entry, &context, &huge_scale, &doc, 0).is_none());
    let huge_origin = serde_json::json!({
        "origin":format!("{} 0 0", f32::MAX),
        "scale":"0.00000023841858 1 1"
    });
    assert!(ParticleTransform::of(&huge_origin).is_some());
    assert!(parse_bounds_operator(&entry, &context, &huge_origin, &doc, 0).is_none());
    assert!(
        parse_bounds_operator(&entry, &context, &object, &serde_json::json!({"flags":1}), 0)
            .is_some()
    );
    assert!(
        parse_bounds_operator(
            &entry,
            &context,
            &serde_json::json!({"origin":"1 0 0"}),
            &serde_json::json!({"flags":1}),
            0
        )
        .is_none()
    );
    assert!(
        parse_bounds_operator(&entry, &context, &object, &serde_json::json!({"children":[{}]}), 0)
            .is_none()
    );
    context.bounds_static = false;
    assert!(parse_bounds_operator(&entry, &context, &object, &doc, 0).is_none());
    context.bounds_static = true;
    context.viewport = None;
    assert!(parse_bounds_operator(&entry, &context, &object, &doc, 0).is_none());
}

#[test]
fn between_sequence_endpoints() {
    for (limit, expected) in
        [("repeat", [10.0, 60.0, 110.0, 10.0, 60.0]), ("mirror", [10.0, 60.0, 110.0, 60.0, 10.0])]
    {
        let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        sys.operators.clear();
        sys.emitters = vec![point_emitter([0.0, 7.0, 9.0], 0.0, 5)];
        sys.control_points[0].offset = [10.0, 0.0, 0.0];
        sys.control_points[1].offset = [110.0, 0.0, 0.0];
        sys.initializers.push(
            parse_initializer(&serde_json::json!({
                "name":"mapsequencebetweencontrolpoints", "count":3, "limitbehavior":limit
            }))
            .unwrap(),
        );
        let mut sim = Sim::new(17);
        sim.step(&sys, 0.0);
        assert_eq!(sim.particles.len(), 5);
        for (particle, x) in sim.particles.iter().zip(expected) {
            assert_eq!(particle.pos, [x, 7.0, 9.0]);
        }
    }
}

#[test]
fn between_sequence_flags() {
    for (flags, radial, velocity, size, arc) in [
        (0, 4.0, 8.0, 5.0, 0.0),
        (1, 0.0, 8.0, 5.0, 0.0),
        (2, 4.0, 0.0, 5.0, 0.0),
        (4, 4.0, 8.0, 2.5, 0.0),
        (8, 4.0, 8.0, 5.0, 30.0),
        (15, 0.0, 0.0, 2.5, 30.0),
    ] {
        let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        sys.operators.clear();
        sys.emitters = vec![point_emitter([0.0, 4.0, 0.0], 0.0, 3)];
        sys.control_points[1].offset = [100.0, 0.0, 0.0];
        sys.initializers.push(Initializer::Velocity {
            min: [8.0; 3],
            max: [8.0; 3],
            exponent: 1.0,
        });
        sys.initializers.push(
            parse_initializer(&serde_json::json!({
                "name":"mapsequencebetweencontrolpoints", "count":3, "flags":flags,
                "sizereductionamount":0.5
            }))
            .unwrap(),
        );
        let mut sim = Sim::new(17);
        sim.step(&sys, 0.0);
        for idx in [0, 2] {
            let particle = sim.particles[idx];
            assert_eq!(particle.pos[1], radial);
            assert_eq!(particle.vel, [velocity; 3]);
            assert_eq!(particle.base_size, size);
        }
        assert_eq!(sim.particles[1].pos, [50.0, 4.0 + arc, 0.0]);
        assert_eq!(sim.particles[1].vel, [8.0; 3]);
        assert_eq!(sim.particles[1].base_size, 5.0);
    }
}

#[test]
fn between_bounds_preserve_taper() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.operators.clear();
    sys.emitters = vec![point_emitter([0.0; 3], 0.0, 3)];
    sys.control_points[1].offset = [100.0, 0.0, 0.0];
    sys.initializers.push(
        parse_initializer(&serde_json::json!({
            "name":"mapsequencebetweencontrolpoints", "count":3, "flags":8,
            "bounds":"0.25 0.75", "arcamount":0.2, "arcdirection":"0 0 2"
        }))
        .unwrap(),
    );
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.0);
    let positions: Vec<_> = sim.particles.iter().map(|particle| particle.pos).collect();
    assert_eq!(positions, [[25.0, 0.0, 0.0], [50.0, 0.0, 40.0], [75.0, 0.0, 0.0]]);
}

#[test]
fn between_sequence_world_space() {
    for (world, expected) in [(false, [20.0, 24.0, 0.0]), (true, [20.0, 4.0, 0.0])] {
        let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        sys.operators.clear();
        sys.world = world;
        sys.emitters = vec![point_emitter([0.0, 4.0, 0.0], 0.0, 1)];
        sys.control_points[0].offset = [20.0, 20.0, 0.0];
        sys.control_points[1].offset = [120.0, 20.0, 0.0];
        sys.initializers.push(
            parse_initializer(&serde_json::json!({
                "name":"mapsequencebetweencontrolpoints"
            }))
            .unwrap(),
        );
        let mut sim = Sim::new(17);
        sim.step(&sys, 0.0);
        assert_eq!(sim.particles[0].pos, expected);
    }
}

#[test]
fn between_fractional_count_restarts() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.operators.clear();
    sys.emitters = vec![point_emitter([0.0; 3], 0.0, 5)];
    sys.control_points[7].offset = [100.0, 0.0, 0.0];
    sys.initializers.push(
        parse_initializer(&serde_json::json!({
            "name":"mapsequencebetweencontrolpoints", "count":3.5, "controlpointend":-1
        }))
        .unwrap(),
    );
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.0);
    let positions: Vec<_> = sim.particles.iter().map(|particle| particle.pos[0]).collect();
    assert_eq!(positions, [0.0, 40.0, 80.0, 0.0, 40.0]);
}

#[test]
fn coincident_points_remain_finite() {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.operators.clear();
    sys.emitters = vec![point_emitter([5.0, 7.0, 9.0], 0.0, 3)];
    sys.initializers.push(
        parse_initializer(&serde_json::json!({
            "name":"mapsequencebetweencontrolpoints", "count":1, "flags":0
        }))
        .unwrap(),
    );
    let mut sim = Sim::new(17);
    sim.step(&sys, 0.0);
    for particle in sim.particles {
        assert_eq!(particle.pos, [5.0, 7.0, 9.0]);
    }
}

#[test]
fn simplex_two_native_vectors() {
    for (point, expected) in [
        ([0.0, 0.0], 0.0),
        ([0.1, 0.2], -0.017_088_167),
        ([0.25, 0.0], 0.871_226_3),
        ([0.0, 0.25], -0.411_206_28),
        ([-0.5, 0.75], 0.897_257_45),
        ([3.5, -1.25], -0.234_811_72),
        ([10.0, 10.0], 0.217_569_26),
        ([100.1, 200.2], 0.176_962_48),
    ] {
        assert!((noise::simplex_two(point) - expected).abs() < 1e-6, "{point:?}");
    }
}

fn offset_system(entry: &Value, defaults: ParticleDefaults) -> ParticleSystem {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.operators.clear();
    sys.emitters = vec![point_emitter([0.25; 3], 0.0, 1)];
    sys.initializers.push(super::parse_initializer(entry, defaults).unwrap());
    sys
}

#[test]
fn offset_signed_directions() {
    for (sign, expected) in [
        ("0 0 0", [0.871_226_3, -0.411_206_28, 0.871_226_3]),
        ("1 1 1", [0.871_226_3, 0.411_206_28, 0.871_226_3]),
        ("-1 -1 -1", [-0.871_226_3, -0.411_206_28, -0.871_226_3]),
    ] {
        let sys = offset_system(
            &serde_json::json!({
                "name":"positionoffsetrandom", "scale":1, "distance":1,
                "timescale":0, "directions":"1 1 1", "sign":sign, "octaves":1
            }),
            ParticleDefaults::ORTHOGRAPHIC,
        );
        let sim = spawn_one(&sys);
        for (position, expected) in sim.particles[0].pos.into_iter().zip(expected) {
            assert!((position - 0.25 - expected).abs() < 1e-6);
        }
        assert_eq!(sim.particles[0].vel, [0.0; 3]);
    }
}

#[test]
fn offset_projection_defaults() {
    for (defaults, explicit) in [
        (
            ParticleDefaults::ORTHOGRAPHIC,
            serde_json::json!({"scale":0.001,"distance":100,"directions":"1 1 0"}),
        ),
        (
            ParticleDefaults::OTHER,
            serde_json::json!({"scale":1,"distance":0.1,"directions":"1 1 1"}),
        ),
    ] {
        let mut entry = explicit;
        entry["name"] = serde_json::json!("positionoffsetrandom");
        let expected = spawn_one(&offset_system(&entry, defaults)).particles[0].pos;
        let sys = offset_system(&serde_json::json!({"name":"positionoffsetrandom"}), defaults);
        assert_eq!(spawn_one(&sys).particles[0].pos, expected);
    }
}

#[test]
fn offset_octaves_clamp() {
    for (requested, expected) in [(0, 1), (9, 8)] {
        let mut entry = serde_json::json!({"name":"positionoffsetrandom","octaves":requested});
        let actual = spawn_one(&offset_system(&entry, ParticleDefaults::OTHER)).particles[0].pos;
        entry["octaves"] = serde_json::json!(expected);
        let expected = spawn_one(&offset_system(&entry, ParticleDefaults::OTHER)).particles[0].pos;
        assert_eq!(actual, expected);
    }
}

fn event_child(input: &str) -> ParticleSystem {
    let mut sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    sys.operators.clear();
    sys.child_event = Some(child_event(ChildEventKind::Follow));
    sys.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
    sys.initializers.extend([
        Initializer::Alpha { min: 0.5, max: 0.5, exponent: 1.0 },
        Initializer::Color {
            min: [255.0, 127.5, 63.75],
            max: [255.0, 127.5, 63.75],
            exponent: 1.0,
        },
        Initializer::Velocity { min: [5.0, 7.0, 11.0], max: [5.0, 7.0, 11.0], exponent: 1.0 },
        parse_initializer(
            &serde_json::json!({"name":"inheritinitialvaluefromevent","input":input}),
        )
        .unwrap(),
    ]);
    sys
}

#[test]
fn event_inheritance_modes() {
    let parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let mut source = spawn_one(&parent);
    source.particles[0].color = [0.5, 0.25, 1.0];
    source.particles[0].alpha = 0.25;
    source.particles[0].size = 7.0;
    source.particles[0].vel = [2.0, 3.0, 4.0];
    for (input, color, alpha, size, velocity) in [
        ("setcolor", [0.5, 0.25, 1.0], 0.5, 5.0, [5.0, 7.0, 11.0]),
        ("multiplycolor", [0.5, 0.125, 0.25], 0.5, 5.0, [5.0, 7.0, 11.0]),
        ("setopacity", [1.0, 0.5, 0.25], 0.25, 5.0, [5.0, 7.0, 11.0]),
        ("multiplyopacity", [1.0, 0.5, 0.25], 0.125, 5.0, [5.0, 7.0, 11.0]),
        ("setcoloropacity", [0.5, 0.25, 1.0], 0.25, 5.0, [5.0, 7.0, 11.0]),
        ("multiplycoloropacity", [0.5, 0.125, 0.25], 0.125, 5.0, [5.0, 7.0, 11.0]),
        ("setsize", [1.0, 0.5, 0.25], 0.5, 7.0, [5.0, 7.0, 11.0]),
        ("multiplysize", [1.0, 0.5, 0.25], 0.5, 17.5, [5.0, 7.0, 11.0]),
        ("setvelocity", [1.0, 0.5, 0.25], 0.5, 5.0, [2.0, 3.0, 4.0]),
        ("addvelocity", [1.0, 0.5, 0.25], 0.5, 5.0, [7.0, 10.0, 15.0]),
    ] {
        let child = event_child(input);
        let mut followers = Followers::default();
        let mut output = Sim::new(3);
        followers.step(&mut output, &child, (&parent, &source), 0.0, (&[], &[]), &mut |_| {});
        let particle = output.particles[0];
        assert_eq!(particle.color, color, "{input}");
        assert_eq!(particle.alpha, alpha, "{input}");
        assert_eq!(particle.size, size, "{input}");
        assert_eq!(particle.vel, velocity, "{input}");
        assert_eq!(particle.base_color, color, "{input}");
    }
}

#[test]
fn event_size_radius() {
    let parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let mut source = spawn_one(&parent);
    for (size, expected) in [(64.0, 128.0), (32.0, 64.0)] {
        source.particles[0].size = size;
        let mut child = event_child("multiplysize");
        child.initializers = vec![
            Initializer::Size { min: 4.0, max: 4.0, exponent: 1.0 },
            Initializer::InheritEvent(EventInheritance::MultiplySize),
        ];
        let mut followers = Followers::default();
        let mut output = Sim::new(3);
        followers.step(&mut output, &child, (&parent, &source), 0.0, (&[], &[]), &mut |_| {});
        assert_eq!(output.particles[0].size, expected);
        assert_eq!(output.particles[0].base_size, expected);
    }
}

#[test]
fn event_live_parent() {
    let parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    for kind in [ChildEventKind::Follow, ChildEventKind::Spawn] {
        let mut source = spawn_one(&parent);
        source.particles[0].size = 8.0;
        let mut child = event_child("setcolor");
        child.initializers.pop();
        child.child_event = Some(child_event(kind));
        child.operators = vec![
            parse_operator(&serde_json::json!({
                "name":"inheritvaluefromevent", "input":"setsize"
            }))
            .unwrap(),
        ];
        let mut followers = Followers::default();
        let mut output = Sim::new(3);
        followers.step(&mut output, &child, (&parent, &source), 0.01, (&[], &[]), &mut |_| {});
        assert_eq!(output.particles[0].size, 8.0);
        source.particles[0].size = 4.0;
        followers.step(&mut output, &child, (&parent, &source), 0.01, (&[], &[]), &mut |_| {});
        assert_eq!(output.particles[0].size, 4.0);
        assert_eq!(output.particles[0].base_size, 5.0);
        source.particles[0].age = source.particles[0].lifetime;
        followers.step(&mut output, &child, (&parent, &source), 0.01, (&[], &[]), &mut |_| {});
        assert_eq!(output.particles[0].size, 5.0);
    }
}

#[test]
fn event_radius() {
    let parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let mut source = spawn_one(&parent);
    source.particles[0].size = 8.0;
    let mut child = event_child("setcolor");
    child.initializers.pop();
    child.operators = vec![
        parse_operator(&serde_json::json!({
            "name":"inheritvaluefromevent", "input":"multiplysize"
        }))
        .unwrap(),
    ];
    let mut followers = Followers::default();
    let mut output = Sim::new(3);
    followers.step(&mut output, &child, (&parent, &source), 0.01, (&[], &[]), &mut |_| {});
    assert_eq!(output.particles[0].size, 20.0);
    source.particles[0].size = 4.0;
    followers.step(&mut output, &child, (&parent, &source), 0.01, (&[], &[]), &mut |_| {});
    assert_eq!(output.particles[0].size, 10.0);
    assert_eq!(output.particles[0].base_size, 5.0);
}

#[test]
fn event_operator_limits() {
    for input in ["setcolor", "multiplyopacity", "setvelocity", "addrotation"] {
        assert!(
            parse_operator(&serde_json::json!({
                "name":"inheritvaluefromevent", "input":input
            }))
            .is_none()
        );
    }
}

#[test]
fn event_requires_parent() {
    let child = event_child("setcoloropacity");
    let particle = spawn_one(&child).particles[0];
    assert_eq!(particle.color, [1.0, 0.5, 0.25]);
    assert_eq!(particle.alpha, 0.5);
    for input in ["setrotation", "addrotation", "setangularvelocity", "addangularvelocity"] {
        assert!(
            parse_initializer(
                &serde_json::json!({"name":"inheritinitialvaluefromevent","input":input})
            )
            .is_none()
        );
    }
}

fn child_event(kind: ChildEventKind) -> ChildEvent {
    ChildEvent { kind, limit: 1, probability: 1.0 }
}

#[test]
fn retirement_storage() {
    let mut system = system();
    system.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
    system.initializers = vec![Initializer::Lifetime { min: 0.1, max: 0.1, exponent: 1.0 }];
    for enabled in [false, true] {
        system.has_death_children = enabled;
        let mut sim = spawn_one(&system);
        sim.step(&system, 0.2);
        assert_eq!(sim.retired.len(), usize::from(enabled));
        sim.step(&system, 0.2);
        assert!(sim.retired.is_empty());
    }
}

#[test]
fn parsed_death_producer() {
    let pkg = Package::parse(crate::tests::build_pkg(&[
        ("parent.json", br#"{"emitter":[{"name":"boxrandom"}],"children":[{"name":"child.json","type":"eventdeath"}]}"#),
        ("child.json", br#"{"emitter":[{"name":"boxrandom"}]}"#),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let context = LoadContext {
        pkg: &pkg,
        assets: &assets,
        defaults: ParticleDefaults::ORTHOGRAPHIC,
        viewport: None,
        bounds_static: false,
        version: 0,
    };
    let mut parent = load(&context, &serde_json::json!({}), "parent.json").0.unwrap();
    let children = std::mem::take(&mut parent.children);
    assert!(parent.has_death_children);
    assert!(!children[0].has_death_children);
}

#[test]
fn flattened_death_snapshot() {
    let mut parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    parent.emitters = vec![point_emitter([12.0, 24.0, 0.0], 0.0, 1)];
    parent.initializers = vec![Initializer::Lifetime { min: 0.1, max: 0.1, exponent: 1.0 }];
    let mut child = event_child("setcoloropacity");
    child.child_event = Some(child_event(ChildEventKind::Death));
    child.initializers.push(Initializer::Lifetime { min: 0.5, max: 0.5, exponent: 1.0 });
    parent.has_death_children = true;
    parent.children.push(child);
    let children = std::mem::take(&mut parent.children);
    let child = &children[0];
    let mut source = spawn_one(&parent);
    source.particles[0].color = [0.25, 0.5, 1.0];
    source.particles[0].alpha = 0.3;
    let mut followers = Followers::default();
    let mut output = Sim::new(3);
    followers.step(&mut output, child, (&parent, &source), 0.0, (&[], &[]), &mut |_| {});
    assert!(output.particles.is_empty());
    source.step(&parent, 0.2);
    assert!(source.particles.is_empty());
    followers.step(&mut output, child, (&parent, &source), 0.0, (&[], &[]), &mut |_| {});
    assert_eq!(output.particles.len(), 1);
    assert_eq!(output.particles[0].color, [0.25, 0.5, 1.0]);
    assert_eq!(output.particles[0].alpha, 0.3);
    assert_eq!(output.particles[0].pos, [12.0, 24.0, 0.0]);
    for _ in 0..4 {
        followers.step(&mut output, child, (&parent, &source), 0.2, (&[], &[]), &mut |_| {});
    }
    assert!(output.particles.is_empty());
}

#[test]
fn spawn_once() {
    let parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let mut source = spawn_one(&parent);
    source.particles[0].pos = [12.0, 24.0, 0.0];
    let mut child = event_child("setsize");
    child.child_event = Some(child_event(ChildEventKind::Spawn));
    child.initializers.push(Initializer::Lifetime { min: 0.1, max: 0.1, exponent: 1.0 });
    child.initializers.push(Initializer::Velocity { min: [0.0; 3], max: [0.0; 3], exponent: 1.0 });
    let mut followers = Followers::default();
    let mut output = Sim::new(3);
    followers.step(&mut output, &child, (&parent, &source), 0.0, (&[], &[]), &mut |_| {});
    assert_eq!(output.particles[0].pos, [12.0, 24.0, 0.0]);
    source.particles[0].pos = [36.0, 48.0, 0.0];
    followers.step(&mut output, &child, (&parent, &source), 0.05, (&[], &[]), &mut |_| {});
    assert_eq!(output.particles[0].pos, [12.0, 24.0, 0.0]);
    for _ in 0..4 {
        followers.step(&mut output, &child, (&parent, &source), 0.2, (&[], &[]), &mut |_| {});
    }
    assert!(output.particles.is_empty());
}

#[test]
fn probability_once() {
    let parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let source = spawn_one(&parent);
    let mut child = event_child("setsize");
    child.child_event.as_mut().unwrap().probability = 0.0;
    let mut followers = Followers::default();
    let mut output = Sim::new(3);
    followers.step(&mut output, &child, (&parent, &source), 0.0, (&[], &[]), &mut |_| {});
    child.child_event.as_mut().unwrap().probability = 1.0;
    followers.step(&mut output, &child, (&parent, &source), 0.0, (&[], &[]), &mut |_| {});
    assert!(output.particles.is_empty());
}

#[test]
fn child_event_types() {
    for (name, kind) in [
        ("eventfollow", ChildEventKind::Follow),
        ("eventspawn", ChildEventKind::Spawn),
        ("eventdeath", ChildEventKind::Death),
    ] {
        let event = ChildEvent::parse(&serde_json::json!({"type":name})).unwrap();
        assert_eq!(event.kind, kind);
        assert_eq!(event.limit, 10);
        assert_eq!(event.probability, 1.0);
    }
    assert!(ChildEvent::parse(&serde_json::json!({"type":"eventbirth"})).is_none());
}

#[test]
fn nested_death_snapshot() {
    let mut parent = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    parent.emitters = vec![point_emitter([12.0, 24.0, 0.0], 0.0, 1)];
    parent.initializers = vec![Initializer::Lifetime { min: 0.1, max: 0.1, exponent: 1.0 }];
    let mut child = event_child("setcolor");
    child.child_event = Some(child_event(ChildEventKind::Death));
    child.emitters = vec![point_emitter([5.0, 0.0, 0.0], 0.0, 1)];
    child.initializers.push(Initializer::Lifetime { min: 0.1, max: 0.1, exponent: 1.0 });
    child.initializers.push(Initializer::Velocity { min: [0.0; 3], max: [0.0; 3], exponent: 1.0 });
    let mut grandchild = event_child("setcolor");
    grandchild.child_event = Some(child_event(ChildEventKind::Death));
    child.has_death_children = true;
    child.children.push(grandchild);
    parent.has_death_children = true;
    parent.children.push(child);
    let child = &parent.children[0];
    let grandchild = &child.children[0];
    let mut source = spawn_one(&parent);
    source.particles[0].color = [0.25, 0.5, 1.0];
    let mut children = Followers::default();
    let mut grandchildren = Followers::default();
    let mut middle = Sim::new(3);
    let mut output = Sim::new(4);
    source.step(&parent, 0.2);
    children.step(&mut middle, child, (&parent, &source), 0.0, (&[], &[]), &mut |_| {});
    assert_eq!(middle.particles[0].pos, [17.0, 24.0, 0.0]);
    source.step(&parent, 0.2);
    children.step(&mut middle, child, (&parent, &source), 0.2, (&[], &[]), &mut |_| {});
    assert!(middle.particles.is_empty());
    grandchildren.step(&mut output, grandchild, (child, &middle), 0.0, (&[], &[]), &mut |_| {});
    assert_eq!(output.particles.len(), 1);
    assert_eq!(output.particles[0].pos, [17.0, 24.0, 0.0]);
    assert_eq!(output.particles[0].color, [0.25, 0.5, 1.0]);
}

#[test]
fn inherit_control_velocity() {
    for (world, scale, expected) in [(false, 1.0, 0.0), (false, 2.0, 0.0), (true, 2.0, 10.0)] {
        let mut system = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        system.world = world;
        system.scale3 = [scale; 3];
        system.scale = scale;
        system.emitters = vec![point_emitter([0.0; 3], 10.0, 0)];
        system.initializers.push(
            parse_initializer(&serde_json::json!({
                "name":"inheritcontrolpointvelocity", "controlpoint":1, "min":1, "max":1
            }))
            .unwrap(),
        );
        let mut sim = spawn_one(&system);
        system.origin.0 += 1.0;
        sim.step(&system, 0.1);
        assert_eq!(sim.particles.last().unwrap().vel, [expected, 0.0, 0.0]);
    }
}

#[test]
fn inherited_velocity_adds() {
    let mut system = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    system.rate_scale = 2.0;
    system.emitters = vec![point_emitter([0.0; 3], 10.0, 0)];
    system.initializers.extend([
        Initializer::Velocity { min: [2.0; 3], max: [2.0; 3], exponent: 1.0 },
        parse_initializer(&serde_json::json!({
            "name":"inheritcontrolpointvelocity", "controlpoint":1, "min":0.5, "max":0.5
        }))
        .unwrap(),
    ]);
    let mut sim = spawn_one(&system);
    assert_eq!(sim.particles[0].vel, [2.0; 3]);
    system.control_points[1].offset = [1.0, 2.0, 3.0];
    sim.step(&system, 0.1);
    assert_eq!(sim.particles.last().unwrap().vel, [7.0, 12.0, 17.0]);
}

#[test]
fn world_origin_motion() {
    let mut system = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    system.world = true;
    system.initializers.push(Initializer::Velocity {
        min: [30.0, 0.0, 0.0],
        max: [30.0, 0.0, 0.0],
        exponent: 1.0,
    });
    system.emitters = vec![point_emitter([0.0; 3], 0.0, 1)];
    let mut sim = Sim::new(3);
    sim.step(&system, 0.0);
    for _ in 0..20 {
        system.origin.0 += 3.0;
        sim.step(&system, 0.1);
    }
    assert!(sim.particles[0].pos[0].abs() < 0.001);
    assert_eq!(sim.particles[0].vel, [30.0, 0.0, 0.0]);
}

#[test]
fn control_velocity_defaults() {
    let Initializer::InheritVelocity(inherit) = parse_initializer(&serde_json::json!({
        "name":"inheritcontrolpointvelocity"
    }))
    .unwrap() else {
        panic!()
    };
    assert_eq!((inherit.point, inherit.min, inherit.max), (0, 0.1, 0.2));
    assert!(
        parse_initializer(&serde_json::json!({
            "name":"inheritcontrolpointvelocity", "controlpoint":8
        }))
        .is_none()
    );
}

#[test]
fn maintain_distance_rows() {
    for (start, expected, blend) in [
        ([200.0, 0.0, 0.0], [100.0, 0.0, 0.0], false),
        ([150.0, 200.0, 0.0], [60.0, 80.0, 0.0], false),
        ([200.0, 0.0, 0.0], [200.0, 0.0, 0.0], true),
    ] {
        let mut system = system();
        system.emitters = vec![point_emitter(start, 0.0, 1)];
        system.operators = vec![
            parse_operator(&serde_json::json!({
                "name":"maintaindistancetocontrolpoint", "distance":100,
                "blendinstart":if blend {0.8} else {0.0},
                "blendinend":if blend {0.9} else {0.0}
            }))
            .unwrap(),
        ];
        let sim = spawn_one(&system);
        for (actual, expected) in sim.particles[0].pos.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.05);
        }
    }
}

#[test]
fn maintain_point_translation() {
    let mut system = system();
    system.emitters = vec![point_emitter([200.0, 0.0, 0.0], 0.0, 1)];
    system.operators = vec![
        parse_operator(&serde_json::json!({
            "name":"maintaindistancetocontrolpoint", "distance":100,
            "blendinstart":0.8,"blendinend":0.9
        }))
        .unwrap(),
    ];
    let mut sim = spawn_one(&system);
    system.control_points[0].offset = [10.0, 20.0, 0.0];
    sim.step(&system, 0.02);
    assert_eq!(sim.particles[0].pos, [210.0, 20.0, 0.0]);
}

#[test]
fn maintain_variable_strength() {
    let mut system = system();
    system.emitters = vec![point_emitter([200.0, 0.0, 0.0], 0.0, 1)];
    system.operators.clear();
    let mut sim = spawn_one(&system);
    system.operators.push(
        parse_operator(&serde_json::json!({
            "name":"maintaindistancetocontrolpoint", "distance":100, "variablestrength":10
        }))
        .unwrap(),
    );
    sim.step(&system, 0.02);
    assert!((sim.particles[0].pos[0] - 180.0).abs() < 0.05);
}

#[test]
fn maintain_projection_defaults() {
    for (defaults, expected) in
        [(ParticleDefaults::ORTHOGRAPHIC, 200.0), (ParticleDefaults::OTHER, 1.0)]
    {
        let mut system = system();
        system.emitters = vec![point_emitter([400.0, 0.0, 0.0], 0.0, 1)];
        system.operators = vec![
            super::parse_operator(
                &serde_json::json!({
                    "name":"maintaindistancetocontrolpoint","controlpoint":-1
                }),
                defaults,
            )
            .unwrap(),
        ];
        system.control_points[7].offset = [10.0, 0.0, 0.0];
        let sim = spawn_one(&system);
        assert!((sim.particles[0].pos[0] - 10.0 - expected).abs() < 0.05);
    }
}

#[test]
fn maintained_segment_moves() {
    for blend in [false, true] {
        let mut system = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        system.emitters = vec![point_emitter([25.0, 30.0, 0.0], 0.0, 1)];
        system.control_points[1].offset = [100.0, 0.0, 0.0];
        let mut sim = spawn_one(&system);
        system.operators = vec![
            parse_operator(&serde_json::json!({
                "name":"maintaindistancebetweencontrolpoints",
                "blendinstart":if blend {0.8} else {0.0},
                "blendinend":if blend {0.9} else {0.0}
            }))
            .unwrap(),
        ];
        system.control_points[0].offset = [10.0, 0.0, 0.0];
        system.control_points[1].offset = [210.0, 0.0, 0.0];
        sim.step(&system, 0.02);
        let expected = if blend { 25.0 } else { 60.0 };
        assert!((sim.particles[0].pos[0] - expected).abs() < 0.05);
        assert_eq!(sim.particles[0].pos[1], 30.0);
    }
}

#[test]
fn maintained_segment_endpoints() {
    for (position, expected) in [(-100.0, 0.0), (200.0, 100.0)] {
        let mut system = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        system.emitters = vec![point_emitter([position, 30.0, 0.0], 0.0, 1)];
        system.control_points[1].offset = [100.0, 0.0, 0.0];
        system.operators = vec![
            parse_operator(&serde_json::json!({
                "name":"maintaindistancebetweencontrolpoints"
            }))
            .unwrap(),
        ];
        let sim = spawn_one(&system);
        assert!((sim.particles[0].pos[0] - expected).abs() < 0.05);
        assert_eq!(sim.particles[0].pos[1], 30.0);
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
#[ignore = "requires Intel RSQRT reference profile"]
fn maintained_segment_drifts() {
    assert_eq!(native_rsqrt(10000.0).to_bits(), 0x3c23_d000);
    let position = segment_drift();
    assert!((90.0..91.0).contains(&position[0]));
    assert_eq!(position[1], 30.0);
}

#[test]
fn degenerate_segment_stays() {
    let mut system = system();
    system.emitters = vec![point_emitter([25.0, 30.0, 0.0], 0.0, 1)];
    system.operators = vec![
        parse_operator(&serde_json::json!({
            "name":"maintaindistancebetweencontrolpoints"
        }))
        .unwrap(),
    ];
    assert_eq!(spawn_one(&system).particles[0].pos, [25.0, 30.0, 0.0]);
}

#[test]
fn reduction_distance_curve() {
    for (position, expected) in [(50.0, 8.0), (150.0, 9.0), (250.0, 10.0), (0.0, 10.0)] {
        let mut system = system();
        system.emitters = vec![point_emitter([position, 0.0, 0.0], 0.0, 1)];
        system.initializers.push(Initializer::Velocity {
            min: [10.0; 3],
            max: [10.0; 3],
            exponent: 1.0,
        });
        system.operators.clear();
        let mut sim = spawn_one(&system);
        system.operators.push(
            parse_operator(&serde_json::json!({
                "name":"reducemovementnearcontrolpoint", "distanceinner":100,"distanceouter":200,
                "reductioninner":10,"reductionouter":0
            }))
            .unwrap(),
        );
        sim.step(&system, 0.02);
        for velocity in sim.particles[0].vel {
            assert!((velocity - expected).abs() < 0.01);
        }
        assert_eq!(sim.particles[0].pos, [position, 0.0, 0.0]);
    }
}

#[test]
fn reduction_equal_endpoints() {
    let mut system = system();
    system.emitters = vec![point_emitter([102.0, 0.0, 0.0], 0.0, 1)];
    system.initializers.push(Initializer::Velocity {
        min: [10.0; 3],
        max: [10.0; 3],
        exponent: 1.0,
    });
    system.operators.clear();
    let mut sim = spawn_one(&system);
    system.operators.push(
        parse_operator(&serde_json::json!({
            "name":"reducemovementnearcontrolpoint", "distanceinner":100,"distanceouter":100,
            "reductioninner":2,"reductionouter":2
        }))
        .unwrap(),
    );
    sim.step(&system, 0.02);
    for velocity in sim.particles[0].vel {
        assert!((velocity - 9.4).abs() < 0.001);
    }
}

#[test]
fn reduction_blend_gates_clamp() {
    let mut system = system();
    system.emitters = vec![point_emitter([50.0, 0.0, 0.0], 0.0, 1)];
    system.initializers.push(Initializer::Velocity {
        min: [10.0; 3],
        max: [10.0; 3],
        exponent: 1.0,
    });
    system.operators.clear();
    let mut sim = spawn_one(&system);
    system.operators.push(
        parse_operator(&serde_json::json!({
            "name":"reducemovementnearcontrolpoint", "reductioninner":10000,
            "blendinstart":0.0,"blendinend":0.04
        }))
        .unwrap(),
    );
    sim.step(&system, 0.02);
    for velocity in sim.particles[0].vel {
        assert!((velocity - 5.0).abs() < 0.03);
    }
}

#[test]
fn remap_size_units() {
    for (operation, maximum, expected) in [
        ("remap", 64.0, 64.0),
        ("multiply", 1.0, 32.0),
        ("add", 32.0, 96.0),
        ("subtract", 32.0, 32.0),
    ] {
        let mut system = system();
        system.operators.clear();
        system.initializers = vec![
            Initializer::Lifetime { min: 100.0, max: 100.0, exponent: 1.0 },
            Initializer::Size { min: 64.0, max: 64.0, exponent: 1.0 },
            parse_initializer(&serde_json::json!({
                "name":"remapinitialvalue", "operation":operation,
                "inputrangemax":200, "outputrangemax":maximum
            }))
            .unwrap(),
        ];
        let sim = spawn_one(&system);
        assert_eq!(sim.particles[0].base_size, expected);
        assert_eq!(sim.particles[0].size, expected);
    }
}

#[test]
fn remap_reads_prior_initializers() {
    let mut system = system();
    system.operators.clear();
    system.initializers = vec![
        Initializer::Alpha { min: 0.25, max: 0.25, exponent: 1.0 },
        parse_initializer(&serde_json::json!({
            "name":"remapinitialvalue", "input":"opacity", "output":"maxlifetime",
            "operation":"remap", "outputrangemax":8
        }))
        .unwrap(),
        parse_initializer(&serde_json::json!({
            "name":"remapinitialvalue", "output":"opacity", "operation":"remap",
            "inputrangemax":4
        }))
        .unwrap(),
    ];
    let sim = spawn_one(&system);
    assert_eq!(sim.particles[0].lifetime, 2.0);
    assert_eq!(sim.particles[0].base_alpha, 0.5);
    assert_eq!(sim.particles[0].alpha, 0.5);
}

#[test]
fn remap_clamp_flags() {
    for (flags, expected) in [(0, 8.0), (1, 4.0), (2, 2.0), (3, 2.0)] {
        let mut system = system();
        system.operators.clear();
        system.initializers = vec![
            Initializer::Lifetime { min: 2.0, max: 2.0, exponent: 1.0 },
            parse_initializer(&serde_json::json!({
                "name":"remapinitialvalue", "operation":"remap",
                "outputrangemax":2,"flags":flags
            }))
            .unwrap(),
        ];
        assert_eq!(spawn_one(&system).particles[0].size, expected);
    }
}

#[test]
fn remap_zero_span() {
    let mut system = system();
    system.operators.clear();
    let remap = remap::Remap::parse_initial(&serde_json::json!({
        "input":"size","operation":"remap","inputrangemin":0,"inputrangemax":0,"flags":0
    }))
    .unwrap();
    let mut particle = spawn_one(&system).particles.remove(0);
    particle.size = f32::EPSILON;
    remap::tests::apply_plain(&remap, &mut particle);
    assert_eq!(particle.size, 1.0);
}

#[test]
fn remap_rejects_unknown_names() {
    for value in [
        serde_json::json!({"input":"unknown"}),
        serde_json::json!({"output":"unknown"}),
        serde_json::json!({"operation":"unknown"}),
        serde_json::json!({"inputcomponent":"unknown"}),
        serde_json::json!({"outputcomponent":"unknown"}),
        serde_json::json!({"transformfunction":"unknown"}),
        serde_json::json!({"transformoctaves":"3"}),
    ] {
        assert!(remap::Remap::parse_initial(&value).is_none(), "{value}");
        assert!(remap::Remap::parse_operator(&value).is_none(), "{value}");
    }
}

#[test]
fn vortex_swirl_direction() {
    for (axis, expected) in [("0 0 2", [0.0, -0.2, 0.0]), ("0 2 0", [0.0, 0.0, 0.2])] {
        let mut system = system();
        system.emitters = vec![point_emitter([100.0, 0.0, 0.0], 0.0, 1)];
        system.operators.clear();
        let mut sim = spawn_one(&system);
        system.operators.push(
            parse_operator(&serde_json::json!({
                "name":"vortex_v2", "axis":axis, "speedinner":10, "speedouter":10, "centerforce":0
            }))
            .unwrap(),
        );
        sim.step(&system, 0.02);
        for (actual, expected) in sim.particles[0].vel.iter().zip(expected) {
            assert!((actual - expected).abs() < 0.002);
        }
        assert_eq!(sim.particles[0].pos, [100.0, 0.0, 0.0]);
    }
}

#[test]
fn vortex_ring_band() {
    for (position, sign) in [(50.0, 1.0), (150.0, -1.0), (100.0, 0.0)] {
        let mut system = system();
        system.emitters = vec![point_emitter([position, 0.0, 0.0], 0.0, 1)];
        system.operators.clear();
        let mut sim = spawn_one(&system);
        system.operators.push(
            parse_operator(&serde_json::json!({
                "name":"vortex_v2", "flags":4, "speedinner":0, "speedouter":0, "centerforce":0,
                "ringradius":100,"ringwidth":10,"ringpulldistance":100,"ringpullforce":2
            }))
            .unwrap(),
        );
        sim.step(&system, 0.01);
        let speed = sim.particles[0].vel[0];
        if sign == 0.0 {
            assert_eq!(speed, 0.0);
        } else {
            assert!(speed * sign > 0.0);
        }
    }
}

#[test]
fn vortex_blend_and_audio() {
    let mut system = system();
    system.emitters = vec![point_emitter([100.0, 0.0, 0.0], 0.0, 1)];
    system.operators.clear();
    let mut sim = spawn_one(&system);
    system.operators.push(
        parse_operator(&serde_json::json!({
            "name":"vortex_v2", "speedinner":10,"speedouter":10,"centerforce":0,
            "blendinstart":0.0,"blendinend":0.04,
            "audioprocessingmode":1,"audioprocessingbounds":"0 1","audioprocessingexponent":1
        }))
        .unwrap(),
    );
    assert!(system.needs_audio());
    sim.step_with_audio(&system, 0.02, &[1.0], &[], &mut |_| {});
    assert!((sim.particles[0].vel[1] + 0.1).abs() < 0.002);
    sim.step_with_audio(&system, 0.02, &[0.0], &[], &mut |_| {});
    assert!((sim.particles[0].vel[1] + 0.1).abs() < 0.002);
}

#[test]
#[cfg(target_arch = "x86_64")]
#[ignore = "requires Intel RSQRT reference profile"]
fn vortex_center_correction() {
    assert_eq!(native_rsqrt(10000.0).to_bits(), 0x3c23_d000);
    assert_eq!(native_rsqrt(100.2_f32 * 100.2).to_bits(), 0x3c23_7800);
    check_center_correction(-2.191_43);
}

#[test]
fn remap_size_input_radius() {
    let mut system = system();
    system.operators.clear();
    system.initializers = vec![
        Initializer::Size { min: 64.0, max: 64.0, exponent: 1.0 },
        parse_initializer(&serde_json::json!({
            "name":"remapinitialvalue","input":"size","output":"opacity",
            "operation":"remap","inputrangemax":64
        }))
        .unwrap(),
    ];
    assert_eq!(spawn_one(&system).particles[0].alpha, 0.5);
}

#[test]
fn remap_operator_size_operations() {
    for (operation, value, expected) in [
        ("remap", 32.0, 64.0),
        ("multiply", 0.5, 32.0),
        ("add", 16.0, 96.0),
        ("subtract", 16.0, 32.0),
    ] {
        let op = parse_operator(&serde_json::json!({
            "name":"remapvalue", "input":"lifetimefraction", "output":"size",
            "operation":operation, "inputrangemin":0, "inputrangemax":1,
            "outputrangemin":value, "outputrangemax":value,
            "flags":1, "transformfunction":"none"
        }))
        .unwrap();
        let mut sys = single(op);
        sys.initializers[1] = Initializer::Size { min: 64.0, max: 64.0, exponent: 1.0 };
        let mut sim = spawn_one(&sys);
        assert_eq!(sim.particles[0].size, expected, "{operation}");
        assert_eq!(sim.particles[0].base_size, 64.0);
        sim.step(&sys, 0.01);
        assert_eq!(sim.particles[0].size, expected, "{operation}");
    }
}

#[test]
fn remap_operator_lifetime_fraction() {
    let remap = remap::Remap::parse_operator(&serde_json::json!({
        "input":"lifetimefraction", "output":"size", "operation":"remap",
        "inputrangemin":0, "inputrangemax":1,
        "outputrangemin":0, "outputrangemax":64
    }))
    .unwrap();
    let sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    let mut particle = spawn_one(&sys).particles[0];
    particle.age = 50.0;
    remap::tests::apply_plain(&remap, &mut particle);
    assert!((particle.size - 64.0).abs() < 0.05, "{}", particle.size);
}

#[test]
fn fractional_first_emission() {
    let mut system = system();
    system.emitters = vec![point_emitter([100.0, 0.0, 0.0], 1000.0, 0)];
    system.operators.clear();
    let mut sim = Sim::new(1);
    for (dt, count) in [(0.0, 0), (0.000_328, 0), (0.0006, 0), (0.0001, 1), (0.001, 2)] {
        sim.step(&system, dt);
        assert_eq!(sim.particles.len(), count);
    }
}

#[test]
fn boids_operator_order() {
    for before in [false, true] {
        let mut system = system();
        system.emitters = vec![point_emitter([10.0, 0.0, 0.0], 0.0, 1)];
        system.operators.clear();
        let mut sim = spawn_one(&system);
        assert!(sim.boids.is_none());
        let boids = parse_operator(&serde_json::json!({
            "name":"boids","flags":0,"separationfactor":0,
            "alignmentfactor":0,"cohesionfactor":1
        }))
        .unwrap();
        let movement = Operator::Movement { gravity: [0.0; 3], drag: 0.0 };
        system.operators = if before { vec![boids, movement] } else { vec![movement, boids] };
        sim.step(&system, 0.02);
        let particle = sim.particles[0];
        assert!((particle.vel[0] + 0.2).abs() < 0.00001);
        assert!((particle.pos[0] - if before { 9.996 } else { 10.0 }).abs() < 0.00001);
        assert!(sim.boids.is_some());
    }
}

#[test]
fn vortex_corrected_step() {
    let mut system = system();
    system.emitters = vec![point_emitter([100.0, 0.0, 0.0], 0.0, 1)];
    system.operators.clear();
    let mut sim = spawn_one(&system);
    system.operators.push(
        parse_operator(&serde_json::json!({
            "name":"vortex_v2","speedinner":20,"speedouter":20,"centerforce":0
        }))
        .unwrap(),
    );
    sim.step(&system, 0.1);
    assert!((sim.particles[0].vel[1] + 0.757_858).abs() < 0.001);
    assert_eq!(sim.particles[0].pos, [100.0, 0.0, 0.0]);
}

#[test]
fn boids_corrected_step() {
    let mut system = system();
    system.emitters = vec![point_emitter([100.0, 0.0, 0.0], 0.0, 1)];
    system.operators.clear();
    let mut sim = spawn_one(&system);
    sim.particles[0].vel = [10.0, 0.0, 0.0];
    system.operators.push(
        parse_operator(&serde_json::json!({
            "name":"boids","flags":0,"neighborthreshold":200,
            "separationfactor":0,"alignmentfactor":1,"cohesionfactor":0
        }))
        .unwrap(),
    );
    sim.step(&system, 0.1);
    assert!((sim.particles[0].vel[0] - 9.62466).abs() < 0.00001);
}

#[test]
fn boids_uses_scene_frame() {
    let mut system = system();
    system.max_count = 200;
    system.emitters = vec![point_emitter([10.0, 0.0, 0.0], 0.0, 200)];
    system.operators.clear();
    let mut sim = Sim::new(1);
    sim.step(&system, 0.0);
    for (index, particle) in sim.particles.iter_mut().enumerate() {
        particle.pos[0] = index as f32 + 1.0;
    }
    system.operators.push(
        parse_operator(&serde_json::json!({
            "name":"boids","flags":0,"neighborthreshold":1000,
            "separationfactor":0,"alignmentfactor":0,"cohesionfactor":1
        }))
        .unwrap(),
    );
    sim.set_scene_frame(1);
    sim.step(&system, 0.02);
    assert_eq!(sim.step_id, 2);
    assert_eq!(sim.particles[0].vel, [0.0; 3]);
    assert!(sim.particles[4].vel[0] > 0.0);
}

#[test]
fn rope_selects_geometry_shader() {
    let pkg = Package::parse(crate::tests::build_pkg(&[
        (
            "shaders/genericropeparticle.vert",
            b"attribute vec4 a_PositionVec4;\nvarying vec4 v_Color;\nvoid main(){gl_Position=a_PositionVec4;v_Color=vec4(1.0);}\n",
        ),
        (
            "shaders/genericropeparticle.geom",
            b"in vec4 v_Color;\nin vec4 gl_Position;\nout vec4 v_Color;\nout vec4 gl_Position;\n[maxvertexcount(3)]\nvoid main() {\nPS_INPUT vertex;vertex.v_Color=IN[0].v_Color;vertex.gl_Position=IN[0].gl_Position;OUT.Append(vertex);OUT.Append(vertex);OUT.Append(vertex);}\n",
        ),
        (
            "shaders/genericropeparticle.frag",
            b"varying vec4 v_Color;\nvoid main(){gl_FragColor=v_Color;}\n",
        ),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let material = serde_json::json!({"passes":[{"shader":"genericparticle"}]});
    let render = ParticleRender { renderer: Renderer::Rope, ..ParticleRender::SPRITE };
    let pass = super::engine_pass(&pkg, &assets, Some(&material), render, None).unwrap();
    assert!(pass.geometry.is_some());
    assert_eq!(pass.name, "genericropeparticle");
}

#[test]
fn particle_lighting_tracks_material() {
    let pkg = Package::parse(crate::tests::build_pkg(&[
        (
            "shaders/genericparticle.vert",
            b"attribute vec3 a_Position;\nvoid main() { gl_Position = vec4(a_Position, 1.0); }\n",
        ),
        (
            "shaders/genericparticle.frag",
            b"#if LIGHTING\nvoid main() { gl_FragColor = vec4(0.25); }\n#else\nvoid main() { gl_FragColor = vec4(0.75); }\n#endif\n",
        ),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    for (material, lit) in [
        (
            serde_json::json!({"passes": [{"shader": "genericparticle", "combos": {"LIGHTING": 1}}]}),
            true,
        ),
        (
            serde_json::json!({"passes": [{"shader": "genericparticle", "combos": {"LIGHTING": 0}}]}),
            false,
        ),
        (serde_json::json!({"passes": [{"shader": "genericparticle"}]}), false),
    ] {
        let pass = super::engine_pass(&pkg, &assets, Some(&material), ParticleRender::SPRITE, None)
            .unwrap();
        assert_eq!(pass.fragment.source.contains("vec4(0.25)"), lit);
        assert_eq!(pass.fragment.source.contains("vec4(0.75)"), !lit);
    }
}

#[test]
fn sphere_native_responses() {
    let inverse = native_rsqrt(169.0);
    let normal = [3.0, 4.0, 12.0].map(|value| value * inverse);
    let position = [3.0, 4.0, 12.0].map(|value| value - value * inverse * (169.0 * inverse - 26.0));
    let velocity = |response| {
        let along = response * ((20.0 * normal[0] - 30.0 * normal[1]) + 40.0 * normal[2]);
        std::array::from_fn(|axis| [20.0, -30.0, 40.0][axis] - normal[axis] * along)
    };
    check_sphere_responses(position, velocity(1.5), velocity(1.0));
}

#[test]
fn maintained_segment_native() {
    let position = segment_drift();
    let inverse = native_rsqrt(10_000.0);
    let mut expected = 100.0_f32;
    for _ in 0..301 {
        let projection = expected * inverse * 100.0;
        let mapped = (projection * inverse).clamp(0.0, 1.0) * inverse * 10_000.0;
        expected += mapped * (100.0 * inverse) - projection * inverse * 100.0;
    }
    assert_eq!(position[0], expected);
    assert_eq!(position[1], 30.0);
}

#[test]
fn vortex_native_correction() {
    let predicted = 100.0_f32 + 10.0 * 0.02;
    let ratio = native_rsqrt(predicted * predicted) * native_rsqrt(10_000.0) * 10_000.0;
    let corrected = 10.0 + predicted * ((ratio - 1.0) * 50.0);
    check_center_correction(corrected);
}

fn native_rsqrt(value: f32) -> f32 {
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

fn check_sphere_responses(position: [f32; 3], bounce: [f32; 3], slide: [f32; 3]) {
    for (behavior, expected_velocity) in
        [("bounce", Some(bounce)), ("slide", Some(slide)), ("delete", None)]
    {
        let mut sys = single(
            parse_operator(&serde_json::json!({
                "name":"collisionsphere","origin":"0 0 0","radius":26,
                "bouncefactor":0.5,"collisionbehavior":behavior
            }))
            .unwrap(),
        );
        sys.emitters = vec![point_emitter([30.0, 0.0, 0.0], 1.0, 0)];
        let mut sim = spawn_one(&sys);
        sim.particles[0].pos = [3.0, 4.0, 12.0];
        sim.particles[0].vel = [20.0, -30.0, 40.0];
        sim.step(&sys, 0.01);
        if let Some(expected) = expected_velocity {
            let particle = &sim.particles[0];
            for (actual, expected) in particle.pos.into_iter().zip(position) {
                assert!((actual - expected).abs() < 1e-4);
            }
            for (actual, expected) in particle.vel.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 1e-4);
            }
        } else {
            assert_eq!(sim.particles.len(), 1);
            assert_eq!(sim.particles[0].age, sim.particles[0].lifetime);
            sim.step(&sys, 0.01);
            assert!(sim.particles.is_empty());
        }
    }
}

fn segment_drift() -> [f32; 3] {
    let mut system = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
    system.emitters = vec![point_emitter([100.0, 30.0, 0.0], 0.0, 1)];
    system.control_points[1].offset = [100.0, 0.0, 0.0];
    system.operators = vec![
        parse_operator(&serde_json::json!({
            "name":"maintaindistancebetweencontrolpoints"
        }))
        .unwrap(),
    ];
    let mut sim = spawn_one(&system);
    for _ in 0..300 {
        sim.step(&system, 1.0 / 30.0);
    }
    sim.particles[0].pos
}

fn check_center_correction(corrected: f32) {
    for (flags, expected) in [(0, 10.0), (2, corrected)] {
        let mut system = system();
        system.emitters = vec![point_emitter([100.0, 0.0, 0.0], 0.0, 1)];
        system.operators.clear();
        system.initializers.push(Initializer::Velocity {
            min: [10.0, 0.0, 0.0],
            max: [10.0, 0.0, 0.0],
            exponent: 1.0,
        });
        let mut sim = spawn_one(&system);
        system.operators.push(
            parse_operator(&serde_json::json!({
                "name":"vortex_v2", "speedinner":0,"speedouter":0,"centerforce":1,"flags":flags
            }))
            .unwrap(),
        );
        sim.step(&system, 0.02);
        assert!((sim.particles[0].vel[0] - expected).abs() < 0.000_01);
    }
}

#[test]
fn remap_noise() {
    const ENCODING: f32 = 16_777_216.0;
    let cases = [
        ("simplexnoise", 0.00, 10.0, 3, 0x3f02_a21e, [8_388_608, 8_388_608, 8_388_608]),
        ("simplexnoise", 0.25, 10.0, 3, 0x3d4e_a9c0, [3_146_234, 12_774_174, 9_764_101]),
        ("simplexnoise", 0.50, 10.0, 3, 0x3f45_df51, [2_635_955, 6_036_280, 14_170_827]),
        ("fbmnoise", 0.25, 8.0, 3, 0x3e2a_5e2c, [9_357_984, 7_552_322, 8_573_901]),
        ("fbmnoise", 0.25, 8.0, 1, 0x3ec6_e958, [11_394_995, 1_108_429, 11_256_234]),
        ("fbmnoise", 0.25, 8.0, 5, 0x3f7e_e647, [4_956_213, 5_146_255, 13_042_890]),
    ];
    for (transform, input, scale, octaves, seed, expected) in cases {
        let mut entry = serde_json::json!({
            "input":"maxlifetime", "output":"velocity", "operation":"remap",
            "inputrangemin":1000.0-input, "inputrangemax":1001.0-input,
            "transformfunction":transform, "transforminputscale":scale
        });
        if octaves != 3 {
            entry["transformoctaves"] = serde_json::json!(octaves);
        }
        let remap = remap::Remap::parse_operator(&entry).unwrap();
        let sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        let mut particle = spawn_one(&sys).particles[0];
        particle.lifetime = 1000.0;
        particle.noise_seed = seed;
        remap::tests::apply_plain(&remap, &mut particle);
        for (actual, encoded) in particle.vel.into_iter().zip(expected) {
            assert!(
                (actual - encoded as f32 / ENCODING).abs() <= 1.0 / ENCODING,
                "{transform} {input} {seed}: {actual} vs {encoded}"
            );
        }
    }
}

#[test]
fn remap_noise_speed() {
    for (factor, flags, velocity, expected) in [
        (0.25, 0, [3.0, 4.0, 0.0], [0.749_816_9, 0.999_755_86, 0.0]),
        (2.0, 0, [3.0, 4.0, 0.0], [5.998_535, 7.998_047, 0.0]),
        (-2.0, 0, [3.0, 4.0, 0.0], [-5.998_535, -7.998_047, 0.0]),
        (2.0, 3, [3.0, 4.0, 0.0], [2.999_267_6, 3.999_023_4, 0.0]),
        (-2.0, 3, [3.0, 4.0, 0.0], [0.0; 3]),
        (2.0, 0, [0.0; 3], [0.0; 3]),
    ] {
        let remap = remap::Remap::parse_operator(&serde_json::json!({
            "output":"speed", "transformfunction":"fbmnoise", "flags":flags,
            "outputrangemin":factor, "outputrangemax":factor
        }))
        .unwrap();
        let sys = single(Operator::Movement { gravity: [0.0; 3], drag: 0.0 });
        let mut particle = spawn_one(&sys).particles[0];
        particle.vel = velocity;
        remap::tests::apply_plain(&remap, &mut particle);
        assert_eq!(particle.vel, expected, "{factor} {flags}");
    }
}

#[test]
fn floor_matches_std() {
    let edges = [
        0.0,
        -0.0,
        0.5,
        -0.5,
        -1.0,
        -1.5,
        8_388_607.5,
        -8_388_607.5,
        8_388_608.0,
        -8_388_608.0,
        2_147_483_648.0,
        -2_147_483_904.0,
        f32::MAX,
        f32::MIN,
        f32::MIN_POSITIVE,
        -f32::MIN_POSITIVE,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];
    let strided = (0..=u32::MAX).step_by(997).map(f32::from_bits);
    for value in edges.into_iter().chain(strided) {
        let floored = super::floor(value);
        if value.is_nan() {
            assert!(floored.is_nan());
        } else {
            assert_eq!(floored.to_bits(), value.floor().to_bits(), "{value}");
        }
    }
}
