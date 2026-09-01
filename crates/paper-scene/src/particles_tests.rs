use super::*;

fn system() -> ParticleSystem {
    ParticleSystem {
        renderer: Renderer::Sprite,
        animation: Animation::Sequence,
        sequence_multiplier: 1.0,
        trail: Trail { length: 0.0, min_length: 0.0, max_length: 100.0, fade_alpha: true },
        max_count: 64,
        start_time: 0.0,
        emitters: vec![Emitter::Box {
            origin: [0.0, 0.0, 0.0],
            extent: [10.0, 10.0, 0.0],
            rate: 60.0,
        }],
        initializers: vec![
            Initializer::Lifetime { min: 1.0, max: 1.0 },
            Initializer::Size { min: 5.0, max: 5.0 },
        ],
        operators: vec![Operator::AlphaFade { fade_in: 0.0, fade_out: 0.5 }],
        texture: None,
        additive: true,
        origin: (0.0, 0.0, 0.0),
        scale: 1.0,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        rate_scale: 1.0,
        size_scale: 1.0,
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
    sys.emitters =
        vec![Emitter::Box { origin: [0.0, 0.0, 0.0], extent: [1.0, 1.0, 0.0], rate: 1.0 }];
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
    sys.emitters =
        vec![Emitter::Box { origin: [0.0, 0.0, 0.0], extent: [0.0, 0.0, 0.0], rate: 1.0 }];
    sys.initializers = vec![
        Initializer::Lifetime { min: 100.0, max: 100.0 },
        Initializer::Size { min: 5.0, max: 5.0 },
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
        Initializer::Lifetime { min: 1.0, max: 1.0 },
        Initializer::Size { min: 5.0, max: 5.0 },
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
