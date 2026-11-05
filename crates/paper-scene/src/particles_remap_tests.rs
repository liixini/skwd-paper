use super::super::tests::system;
use super::*;
use crate::particles::{Emitter, EmitterTiming, Operator, Sim};
use serde_json::{Value, json};

const CLOCK: FrameClock = FrameClock { time: 7.0, dt: 0.0, daytime: 0.25 };
const SYSTEM_TIME: f32 = 3.0;
const LAYER_TIME: f32 = 2.0;
const APPROXIMATION_ERROR: f32 = 2.0 * 1.5 / 4096.0;
const BLEND_WEIGHT_ERROR: f32 = 1.5 / 4096.0 * 0.55 / 0.1;

fn particle() -> Particle {
    Particle {
        id: 0,
        pos: [3.0, 4.0, 0.0],
        vel: [3.0, 0.0, 4.0],
        size: 8.0,
        base_size: 8.0,
        color: [0.5, 0.25, 1.0],
        base_color: [0.5, 0.25, 1.0],
        alpha: 0.5,
        base_alpha: 0.5,
        angle: 1.5,
        angle_xy: [0.0; 2],
        angular: 2.5,
        angular_xy: [0.0; 2],
        age: 1.0,
        lifetime: 4.0,
        phase: 0.0,
        noise_seed: 0,
        slot: 0,
    }
}

fn spawned_system(remap: &Value) -> ParticleSystem {
    let mut system = system();
    system.operators = vec![Operator::Remap(Remap::parse_operator(remap).unwrap())];
    system.remapped_points = written_points(&system.initializers, &system.operators);
    system.initializers.clear();
    system.emitters = vec![Emitter::Box {
        audio: None,
        control_point: None,
        instantaneous: 1,
        timing: EmitterTiming::default(),
        origin: [0.0; 3],
        extent: [0.0; 3],
        rate: 0.0,
    }];
    system
}

fn with_points(system: &ParticleSystem, run: impl FnOnce(&Frame<'_>)) {
    let mut sim = Sim::new(1);
    sim.update_control_points(system, 0.0);
    let points = sim.control_motion(system, 0.0).map(Cell::new);
    let written = Cell::new(0);
    run(&Frame {
        system,
        clock: CLOCK,
        system_time: SYSTEM_TIME,
        layer_time: LAYER_TIME,
        points: &points,
        written: &written,
        step: 0,
        substeps: 1,
        particle: 0,
    });
}

pub(in super::super) fn apply_plain(remap: &Remap, particle: &mut Particle) {
    with_points(&system(), |frame| remap.apply(particle, frame));
}

fn system_with_points(offsets: &[(usize, [f32; 3])]) -> ParticleSystem {
    let mut system = system();
    for &(index, offset) in offsets {
        system.control_points[index].offset = offset;
    }
    system
}

fn operated(entry: &Value, target: &mut Particle) {
    let remap = Remap::parse_operator(entry).unwrap();
    apply_plain(&remap, target);
}

fn close(actual: f32, expected: f32, label: &str) {
    close_within(actual, expected, 0.0, label);
}

fn close_within(actual: f32, expected: f32, extra: f32, label: &str) {
    let allowed = (1.0 + expected.abs()) * APPROXIMATION_ERROR + extra;
    assert!((actual - expected).abs() <= allowed, "{label}: {actual} vs {expected}");
}

fn close3(actual: [f32; 3], expected: [f32; 3], label: &str) {
    for axis in 0..SPATIAL_AXES {
        close(actual[axis], expected[axis], label);
    }
}

#[test]
fn wave_transforms() {
    for (transform, input, expected) in [
        ("sine", 0.0, 0.0),
        ("sine", 0.25, 0.5),
        ("sine", 0.5, 1.0),
        ("square", 0.1, 0.0),
        ("square", 0.3, 1.0),
        ("square", -0.1, 1.0),
        ("saw", 0.3, 0.6),
        ("saw", -0.1, 0.8),
        ("triangle", 0.25, 1.0),
        ("triangle", 0.5, 0.0),
        ("triangle", -0.25, 1.0),
        ("none", 0.3, 0.3),
    ] {
        let remap = Remap::parse_operator(&json!({
            "input":"rotation", "output":"opacity", "operation":"remap", "flags":0,
            "transformfunction":transform, "inputrangemin":-1, "inputrangemax":0
        }))
        .unwrap();
        let mut target = particle();
        target.angle = input - 1.0;
        apply_plain(&remap, &mut target);
        close(target.alpha, expected, transform);
    }
}

#[test]
fn negative_scale_waves() {
    for (transform, input, expected) in
        [("square", 0.3, 0.0), ("square", -0.3, 1.0), ("saw", 0.3, -0.6), ("saw", -0.3, 1.6)]
    {
        let remap = Remap::parse_operator(&json!({
            "input":"rotation", "output":"opacity", "operation":"remap", "flags":0,
            "transformfunction":transform, "transforminputscale":-2,
            "inputrangemin":-1, "inputrangemax":0
        }))
        .unwrap();
        let mut target = particle();
        target.angle = input - 1.0;
        apply_plain(&remap, &mut target);
        close(target.alpha, expected, transform);
    }
}

#[test]
fn measure_zero_length() {
    let (length, root) = measure([0.0; SPATIAL_AXES]);
    assert!(length.is_nan());
    assert_eq!(root, f32::INFINITY);
    assert_eq!(positive_inverse([0.0; SPATIAL_AXES]), 0.0);
    assert_eq!(unit([0.0; SPATIAL_AXES]), [0.0; SPATIAL_AXES]);
}

#[test]
fn scalar_inputs() {
    let target = particle();
    for (input, expected) in [
        ("lifetimefraction", 0.25),
        ("maxlifetime", 4.0),
        ("size", 4.0),
        ("opacity", 0.5),
        ("speed", 5.0),
        ("rotation", 1.5),
        ("angularspeed", 2.5),
        ("runtime", CLOCK.time),
        ("timeofday", CLOCK.daytime),
        ("particlesystemtime", CLOCK.time),
        ("layertime", LAYER_TIME),
    ] {
        let mut sample = target;
        operated(
            &json!({"input":input,"output":"maxlifetime","operation":"remap","flags":0,
                "outputrangemax":1}),
            &mut sample,
        );
        close(sample.lifetime, expected, input);
    }
}

#[test]
fn initial_time_differs() {
    let remap = Remap::parse_initial(&json!({
        "input":"particlesystemtime", "output":"maxlifetime", "operation":"remap", "flags":0
    }))
    .unwrap();
    let mut target = particle();
    apply_plain(&remap, &mut target);
    close(target.lifetime, SYSTEM_TIME, "initial");
}

#[test]
fn spawn_fraction_modes() {
    let entry = json!({"input":"lifetimefraction", "output":"maxlifetime", "operation":"remap",
        "flags":0});
    let remap = Remap::parse_initial(&entry).unwrap();
    let mut target = particle();
    target.noise_seed = 0.75_f32.to_bits();
    apply_plain(&remap, &mut target);
    close(target.lifetime, 0.0, "sequence");
    let mut random = system();
    random.animation = Animation::RandomFrame;
    with_points(&random, |frame| remap.apply(&mut target, frame));
    close(target.lifetime, 0.75, "randomframe");
}

#[test]
fn component_selection() {
    for (component, expected) in [
        ("x", [3.0; 3]),
        ("y", [0.0; 3]),
        ("z", [4.0; 3]),
        ("sum", [7.0; 3]),
        ("average", [7.0 / 3.0; 3]),
        ("max", [4.0; 3]),
        ("min", [0.0; 3]),
        ("all", [3.0, 0.0, 4.0]),
    ] {
        let mut target = particle();
        operated(
            &json!({"input":"velocity","inputcomponent":component,"output":"position",
                "operation":"remap","flags":0}),
            &mut target,
        );
        close3(target.pos, expected, component);
    }
}

#[test]
fn output_axis_only() {
    for (component, expected) in [
        ("all", [9.0, 9.0, 9.0]),
        ("x", [9.0, 4.0, 0.0]),
        ("y", [3.0, 9.0, 0.0]),
        ("z", [3.0, 4.0, 9.0]),
        ("sum", [3.0, 4.0, 0.0]),
    ] {
        let mut target = particle();
        operated(
            &json!({"input":"maxlifetime","output":"position","outputcomponent":component,
                "operation":"remap","outputrangemin":9,"outputrangemax":9}),
            &mut target,
        );
        close3(target.pos, expected, component);
    }
}

#[test]
fn scalar_operations() {
    for (operation, expected) in
        [("remap", 2.0), ("multiply", 1.0), ("add", 2.5), ("subtract", -1.5)]
    {
        let mut target = particle();
        operated(
            &json!({"input":"maxlifetime","output":"opacity","operation":operation,
                "outputrangemin":2,"outputrangemax":2}),
            &mut target,
        );
        close(target.alpha, expected, operation);
    }
}

#[test]
fn scalar_outputs() {
    let entry = |output: &str| {
        json!({"input":"maxlifetime","output":output,"operation":"remap",
            "outputrangemin":2,"outputrangemax":2})
    };
    let mut target = particle();
    operated(&entry("maxlifetime"), &mut target);
    close(target.lifetime, 2.0, "maxlifetime");
    operated(&entry("rotation"), &mut target);
    close(target.angle, 2.0, "rotation");
    operated(&entry("size"), &mut target);
    close(target.size, 4.0, "size");
    assert_eq!(target.base_size, 8.0);
    operated(&entry("color"), &mut target);
    close3(target.color, [2.0; 3], "color");
    operated(&entry("speed"), &mut target);
    close(dot3(target.vel, target.vel).sqrt(), 2.0, "speed");
    let before = target;
    for inert in ["lifetimefraction", "runtime", "timeofday", "layertime", "layerorigin"] {
        operated(&entry(inert), &mut target);
        assert_eq!(target.pos, before.pos, "{inert}");
        assert_eq!(target.alpha, before.alpha, "{inert}");
    }
}

#[test]
fn angular_needs_motion() {
    let remap = Remap::parse_operator(&json!({
        "input":"maxlifetime","output":"angularspeed","operation":"remap",
        "outputrangemin":2,"outputrangemax":2
    }))
    .unwrap();
    let mut target = particle();
    apply_plain(&remap, &mut target);
    assert_eq!(target.angular, 2.5);
    let mut moving = system();
    moving.angular_motion = true;
    with_points(&moving, |frame| remap.apply(&mut target, frame));
    assert_eq!(target.angular, 2.0);
}

#[test]
fn point_inputs() {
    let points = system_with_points(&[(1, [3.0, 0.0, 0.0]), (2, [0.0, 8.0, 0.0])]);
    for (input, expected) in [
        ("distancetocontrolpoint", [4.0; 3]),
        ("controlpoint", [3.0, 0.0, 0.0]),
        ("deltatocontrolpoint", [0.0, -4.0, 0.0]),
        ("directiontocontrolpoint", [0.0, -1.0, 0.0]),
    ] {
        let remap = Remap::parse_operator(&json!({
            "input":input, "inputcontrolpoint0":1, "output":"velocity", "operation":"remap",
            "flags":0, "inputrangemin":0, "inputrangemax":1
        }))
        .unwrap();
        let mut target = particle();
        with_points(&points, |frame| remap.apply(&mut target, frame));
        close3(target.vel, expected, input);
    }
}

#[test]
fn between_uses_outputs() {
    let points = system_with_points(&[(1, [0.0, 8.0, 0.0]), (3, [100.0, 0.0, 0.0])]);
    let remap = Remap::parse_operator(&json!({
        "input":"positionbetweentwocontrolpoints", "inputcontrolpoint0":3,
        "inputcontrolpoint1":3, "output":"maxlifetime", "operation":"remap", "flags":0
    }))
    .unwrap();
    let mut target = particle();
    with_points(&points, |frame| remap.apply(&mut target, frame));
    close(target.lifetime, 0.5, "between");
}

#[test]
fn layer_origin_input() {
    let mut placed = system();
    placed.origin = (11.0, 12.0, 13.0);
    let remap = Remap::parse_operator(&json!({
        "input":"layerorigin", "output":"velocity", "operation":"remap", "flags":0
    }))
    .unwrap();
    let mut target = particle();
    with_points(&placed, |frame| remap.apply(&mut target, frame));
    close3(target.vel, [11.0, 12.0, 13.0], "origin");
}

#[test]
fn coincident_points_nan() {
    let points = system_with_points(&[(1, [3.0, 4.0, 0.0]), (2, [3.0, 4.0, 0.0])]);
    for entry in [
        json!({"output":"distancetocontrolpoint","outputcontrolpoint0":1}),
        json!({"output":"positionbetweentwocontrolpoints","outputcontrolpoint0":1,
            "outputcontrolpoint1":2}),
    ] {
        let mut entry = entry;
        entry["input"] = json!("maxlifetime");
        entry["operation"] = json!("remap");
        let remap = Remap::parse_operator(&entry).unwrap();
        let mut target = particle();
        with_points(&points, |frame| remap.apply(&mut target, frame));
        assert!(target.pos.iter().all(|axis| axis.is_nan()), "{entry}");
    }
}

#[test]
fn point_outputs() {
    let points = system_with_points(&[(1, [3.0, 0.0, 0.0]), (2, [3.0, 8.0, 0.0])]);
    for (entry, expected) in [
        (
            json!({"output":"distancetocontrolpoint","outputcontrolpoint0":1,
                "outputrangemin":2,"outputrangemax":2}),
            [3.0, 2.0, 0.0],
        ),
        (
            json!({"output":"positionbetweentwocontrolpoints","outputcontrolpoint0":1,
                "outputcontrolpoint1":2,"outputrangemin":0.25,"outputrangemax":0.25}),
            [3.0, 2.0, 0.0],
        ),
        (
            json!({"output":"deltatocontrolpoint","outputcontrolpoint0":1,
                "outputrangemin":1,"outputrangemax":1}),
            [2.0, -1.0, -1.0],
        ),
        (
            json!({"output":"directiontocontrolpoint","outputcontrolpoint0":1,
                "outputcomponent":"x","outputrangemin":1,"outputrangemax":1}),
            [3.0 - 4.0 / 2.0_f32.sqrt(), 4.0 / 2.0_f32.sqrt(), 0.0],
        ),
    ] {
        let mut entry = entry;
        entry["input"] = json!("maxlifetime");
        entry["operation"] = json!("remap");
        let remap = Remap::parse_operator(&entry).unwrap();
        let mut target = particle();
        with_points(&points, |frame| remap.apply(&mut target, frame));
        close3(target.pos, expected, &entry.to_string());
    }
}

#[test]
fn blend_mixes_target() {
    let mut target = particle();
    target.age = 0.55;
    target.lifetime = 1.0;
    operated(
        &json!({"input":"maxlifetime","output":"opacity","operation":"remap",
            "blendinstart":0.5,"blendinend":0.6,"outputrangemin":1,"outputrangemax":1}),
        &mut target,
    );
    close_within(target.alpha, 0.75, 0.5 * BLEND_WEIGHT_ERROR, "blended");
    let mut speedy = particle();
    speedy.age = 0.55;
    speedy.lifetime = 1.0;
    operated(
        &json!({"input":"maxlifetime","output":"speed","operation":"remap",
            "blendinstart":0.5,"blendinend":0.6,"outputrangemin":1,"outputrangemax":1}),
        &mut speedy,
    );
    close_within(dot3(speedy.vel, speedy.vel).sqrt(), 3.0, 4.0 * BLEND_WEIGHT_ERROR, "speed");
}

#[test]
fn thunderbolt_flicker() {
    let entry = json!({"blendinend":0.6,"blendinstart":0.5,"flags":0,"id":25,
        "input":"particlesystemtime","name":"remapvalue","output":"opacity",
        "transformfunction":"sine","transforminputscale":6});
    for (age, expected) in [(0.4, 0.5), (0.55, 0.25), (0.8, 0.0)] {
        let mut target = particle();
        target.age = age;
        target.lifetime = 1.0;
        operated(&entry, &mut target);
        close_within(target.alpha, expected, 0.5 * BLEND_WEIGHT_ERROR, "flicker");
    }
}

#[test]
fn initial_default_input() {
    let remap = Remap::parse_initial(&json!({"operation":"remap", "output":"opacity",
        "inputrangemax":8, "flags":0}))
    .unwrap();
    let mut target = particle();
    apply_plain(&remap, &mut target);
    close(target.alpha, 0.5, "maxlifetime");
}

#[test]
fn operator_default_input() {
    let remap = Remap::parse_operator(&json!({"operation":"remap", "output":"opacity",
        "flags":0}))
    .unwrap();
    let mut target = particle();
    apply_plain(&remap, &mut target);
    close(target.alpha, 0.25, "lifetimefraction");
}

#[test]
fn octaves_like_asuint() {
    for (value, expected) in [
        (json!(null), Some(0)),
        (json!(true), Some(1)),
        (json!(3), Some(3)),
        (json!(3.0), Some(3)),
        (json!(1.5), Some(1)),
        (json!(-1), Some(u32::MAX)),
        (json!("3"), None),
    ] {
        assert_eq!(noise::octaves_of(&value), expected, "{value}");
    }
    for entry in [
        json!({"transformfunction":"none","transformoctaves":0}),
        json!({"transformfunction":"sine","transformoctaves":3.0}),
        json!({"transformfunction":"fbmnoise","transformoctaves":40}),
    ] {
        assert!(Remap::parse_operator(&entry).is_some(), "{entry}");
    }
}

#[test]
fn fractal_octave_floor() {
    let zero = noise::Fractal::new(0);
    let one = noise::Fractal::new(1);
    assert_eq!(zero.sample_plain(0.7), one.sample_plain(0.7));
}

#[test]
fn point_write_batches() {
    let system = system_with_points(&[(2, [10.0, 0.0, 0.0])]);
    let mut sim = Sim::new(1);
    sim.update_control_points(&system, 0.1);
    let moved = system_with_points(&[(2, [20.0, 0.0, 0.0])]);
    sim.update_control_points(&moved, 0.1);
    let points = sim.control_motion(&moved, 0.1).map(Cell::new);
    let written = Cell::new(0);
    let remap = Remap::parse_operator(&json!({"input":"maxlifetime", "output":"controlpoint",
        "outputcontrolpoint0":2, "operation":"add", "outputrangemin":1, "outputrangemax":1}))
    .unwrap();
    for index in 0..6 {
        let mut target = particle();
        remap.apply(
            &mut target,
            &Frame {
                system: &moved,
                clock: CLOCK,
                system_time: SYSTEM_TIME,
                layer_time: LAYER_TIME,
                points: &points,
                written: &written,
                step: 0,
                substeps: 2,
                particle: index,
            },
        );
    }
    assert_eq!(points[2].get().position, [22.0, 2.0, 2.0]);
    assert_eq!(written.get(), 1 << 2);
}

#[test]
fn thunderbolt_taper() {
    let points = system_with_points(&[(1, [3.0, 29.0, 0.0])]);
    let remap = Remap::parse_initial(&json!({"id":23,"input":"distancetocontrolpoint",
        "inputcontrolpoint0":1,"inputrangemax":50,"name":"remapinitialvalue",
        "operation":"multiply"}))
    .unwrap();
    let mut target = particle();
    with_points(&points, |frame| remap.apply(&mut target, frame));
    close(target.base_size, 8.0 * 0.5, "taper");
    assert_eq!(target.size, target.base_size);
    let mut near = particle();
    near.pos = [3.0, 29.0, 0.0];
    with_points(&points, |frame| remap.apply(&mut near, frame));
    assert_eq!(near.base_size, 0.0);
}

#[test]
fn written_point_persists() {
    let mut local = spawned_system(&json!({
        "input":"maxlifetime", "output":"controlpoint", "outputcontrolpoint0":2,
        "operation":"remap", "outputrangemin":"5 6 7", "outputrangemax":"5 6 7"
    }));
    let mut sim = Sim::new(3);
    sim.step(&local, 0.01);
    assert_eq!(sim.control_points[2], [5.0, 6.0, 7.0]);
    sim.particles.clear();
    sim.step(&local, 0.01);
    assert_eq!(sim.control_points[2], [5.0, 6.0, 7.0]);
    assert_eq!(local.remapped_points, 1 << 2);
    local.set_control_point(2, [40.0, 50.0, 60.0]);
    sim.particles.clear();
    sim.step(&local, 0.01);
    assert_eq!(sim.control_points[2], [5.0, 6.0, 7.0]);
}

#[test]
fn sim_passes_clock() {
    let timed = spawned_system(&json!({
        "input":"runtime", "output":"opacity", "operation":"remap", "flags":0
    }));
    let mut sim = Sim::new(3);
    sim.set_scene_clock(FrameClock { time: 0.5, dt: 0.0, daytime: 0.0 });
    sim.step(&timed, 0.01);
    close(sim.particles[0].alpha, 0.5, "alpha");
    assert_eq!(sim.layer_time, 0.01);
}

#[test]
fn plain_fractal_octaves() {
    let one = noise::Fractal::new(1);
    close(one.sample_plain(0.7), simplex_one(0.7), "one");
    let two = noise::Fractal::new(2);
    let expected = (simplex_one(0.7) + simplex_one(1.4) * 0.5) / 1.5;
    close(two.sample_plain(0.7), expected, "two");
}
