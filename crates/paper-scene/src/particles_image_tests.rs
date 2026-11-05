use super::*;
use crate::particles::{Emitter, Sim, parse_emitter};

fn source() -> ImageSource {
    Rc::new(RefCell::new(SampleSource {
        cells: vec![SampleCell { color: [0, 255, 0], bone: UNBOUND_BONE, position: [4, 8] }],
        state: SampleState::Ready,
        ..SampleSource::default()
    }))
}

fn system(flags: u32) -> crate::particles::ParticleSystem {
    let mut system = crate::particles::tests::system();
    system.emitters = vec![
        parse_emitter(&serde_json::json!({"name":"layerimage", "flags":flags, "rate":10})).unwrap(),
    ];
    system.initializers.clear();
    system.operators.clear();
    system
}

#[test]
fn preserves_raw_mask() {
    let dependencies = ImageDependency::parse(&serde_json::json!({"dependencies":[
        {"type":"emitterimage","id":38,"index":0,"mask":"Mask/NAME.tex"},
        {"type":"emitterimage","id":38,"index":1},
        {"type":"texture","id":38,"index":2}
    ]}));
    assert_eq!(
        dependencies,
        [
            ImageDependency { source: 38, ordinal: 0, mask: "Mask/NAME.tex".to_owned() },
            ImageDependency { source: 38, ordinal: 1, mask: String::new() },
        ]
    );
}

#[test]
fn unavailable_has_no_births() {
    let system = system(ImageFlags::COLOR.bits());
    let mut sim = Sim::new(1);
    sim.emit_particles(&system, 10, &mut |_| {});
    assert!(sim.particles.is_empty());
    let source = source();
    sim.bind_images(vec![Some(Rc::clone(&source))]);
    source.borrow_mut().cells.clear();
    source.borrow_mut().state = SampleState::Pending;
    let mut automatic = Sim::new(2);
    automatic.bind_images(vec![Some(Rc::clone(&source))]);
    automatic.step(&system, 0.1);
    assert!(automatic.particles.is_empty());
    source.borrow_mut().cells.push(SampleCell {
        color: [0, 255, 0],
        bone: UNBOUND_BONE,
        position: [4, 8],
    });
    source.borrow_mut().state = SampleState::Ready;
    automatic.step(&system, 0.1);
    assert!(!automatic.particles.is_empty());
    assert!(sim.particles.is_empty());
    source.borrow_mut().state = SampleState::Ready;
    source.borrow_mut().cells.clear();
    sim.emit_particles(&system, 10, &mut |_| {});
    assert!(sim.particles.is_empty());
}

#[test]
fn failed_manual_debits_rate() {
    let system = system(ImageFlags::COLOR.bits());
    let mut sim = Sim::new(1);
    sim.emit_particles(&system, 2, &mut |_| {});
    assert!(sim.particles.is_empty());
    assert_eq!(sim.emitter_timers[0].pending, -2.0);
    sim.bind_images(vec![Some(source())]);
    sim.step(&system, 0.1);
    assert!(sim.particles.is_empty());
    sim.step(&system, 0.1);
    assert!(sim.particles.is_empty());
    sim.step(&system, 0.1);
    assert_eq!(sim.particles.len(), 1);
}

#[test]
fn paused_manual_skips_debt() {
    let system = system(ImageFlags::COLOR.bits());
    let mut sim = Sim::new(1);
    sim.pause();
    sim.emit_particles(&system, 2, &mut |_| {});
    assert_eq!(sim.emitter_timers[0].pending, 0.0);
    assert!(!sim.is_playing());
    sim.bind_images(vec![Some(source())]);
    sim.play();
    sim.step(&system, 0.1);
    assert_eq!(sim.particles.len(), 1);
}

#[test]
fn refresh_keeps_cached_births() {
    let system = system(ImageFlags::COLOR.bits());
    let source = source();
    let mut manual = Sim::new(1);
    manual.bind_images(vec![Some(Rc::clone(&source))]);
    source.borrow_mut().invalidate();
    source.borrow_mut().first_query_frame = Some(0);
    manual.set_scene_frame(1);
    manual.emit_particles(&system, 1, &mut |_| {});
    assert!(source.borrow().requested);
    assert_eq!(manual.particles.len(), 1);
    source.borrow_mut().state = SampleState::Pending;
    let mut automatic = Sim::new(2);
    automatic.bind_images(vec![Some(Rc::clone(&source))]);
    automatic.set_scene_frame(2);
    automatic.step(&system, 0.1);
    assert_eq!(automatic.particles.len(), 1);
    assert_eq!(automatic.particles[0].color, [0.0, 1.0, 0.0]);
}

#[test]
fn cold_queries_stage_bursts() {
    let system = system(ImageFlags::COLOR.bits());
    for (frames, expected) in
        [(&[0, 2, 4, 6][..], &[0, 0, 1, 1][..]), (&[0, 0, 2, 2, 4][..], &[0, 0, 0, 1, 1][..])]
    {
        let source = Rc::new(RefCell::new(SampleSource::default()));
        let mut captures = 0;
        let mut harvests = 0;
        let mut query = |source: &ImageSource| {
            let mut source = source.borrow_mut();
            match source.state {
                SampleState::Unready => {
                    captures += 1;
                    source.state = SampleState::Pending;
                    source.requested = false;
                }
                SampleState::Pending => {
                    harvests += 1;
                    source.cells.push(SampleCell {
                        color: [0, 255, 0],
                        bone: UNBOUND_BONE,
                        position: [4, 8],
                    });
                    source.state = SampleState::Ready;
                }
                SampleState::Ready | SampleState::Resized => unreachable!(),
            }
        };
        let observed: Vec<usize> = frames
            .iter()
            .map(|&frame| {
                let mut sim = Sim::new(frame);
                sim.bind_images(vec![Some(Rc::clone(&source))]);
                sim.set_scene_frame(frame);
                sim.step_with_audio(&system, 0.1, &[], &[], &mut query);
                sim.particles.len()
            })
            .collect();
        assert_eq!(observed.as_slice(), expected);
        assert_eq!((captures, harvests), (1, 1));
        assert_eq!(source.borrow().first_query_frame, Some(0));
    }
}

#[test]
fn resize_defers_readback() {
    let system = system(ImageFlags::COLOR.bits());
    let source = source();
    source.borrow_mut().first_query_frame = Some(0);
    source.borrow_mut().resize();
    let mut sim = Sim::new(1);
    sim.bind_images(vec![Some(Rc::clone(&source))]);
    sim.set_scene_frame(2);
    let mut captures = 0;
    let mut harvests = 0;
    let mut query = |source: &ImageSource| {
        let mut source = source.borrow_mut();
        match source.state {
            SampleState::Unready => {
                captures += 1;
                source.state = SampleState::Pending;
                source.requested = false;
            }
            SampleState::Pending => {
                harvests += 1;
                source.cells[0].color = [255, 0, 0];
                source.state = SampleState::Ready;
            }
            SampleState::Ready | SampleState::Resized => unreachable!(),
        }
    };
    for _ in 0..3 {
        sim.emit_particles(&system, 1, &mut query);
    }
    assert_eq!((captures, harvests), (1, 1));
    assert_eq!(
        sim.particles.iter().map(|particle| particle.color).collect::<Vec<_>>(),
        [[0.0, 1.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]
    );
    assert_eq!(source.borrow().state, SampleState::Ready);
}

#[test]
fn manual_uses_source_color() {
    let system = system(ImageFlags::COLOR.bits());
    let mut sim = Sim::new(1);
    sim.bind_images(vec![Some(source())]);
    sim.emit_particles(&system, 1, &mut |_| {});
    assert_eq!(sim.particles[0].pos, [4.0, -8.0, 0.0]);
    assert_eq!(sim.particles[0].color, [0.0, 1.0, 0.0]);
    assert_eq!(sim.particles[0].base_color, [0.0, 1.0, 0.0]);
}

#[test]
fn image_multiplies_runtime_base() {
    let mut system = system(ImageFlags::COLOR.bits());
    system.base_color = [0.5, 0.25, 1.0];
    system.initializers.push(crate::particles::Initializer::Color {
        min: [255.0, 127.5, 255.0],
        max: [255.0, 127.5, 255.0],
        exponent: 1.0,
    });
    let mut sim = Sim::new(1);
    sim.bind_images(vec![Some(source())]);
    sim.emit_particles(&system, 1, &mut |_| {});
    assert_eq!(sim.particles[0].color, [0.0, 0.125, 0.0]);
    assert_eq!(sim.particles[0].base_color, [0.0, 0.125, 0.0]);
}

#[test]
fn offsets_before_transform() {
    let mut emitter = ImageEmitter::parse(
        &serde_json::json!({"flags":524_288,"offsetmin":"2 3 0","offsetmax":"2 3 0"}),
    );
    let source = source();
    source.borrow_mut().current.transform = Affine::new([10.0, 20.0], [2.0, 3.0], 0.0);
    let birth = emitter.birth(&source.borrow(), &mut Rng::new(1)).unwrap();
    assert_eq!(birth.position, [22.0, 5.0, 0.0]);
    assert_eq!(birth.color, [1.0; 3]);
    emitter.flags = ImageFlags::empty();
    assert_eq!(
        emitter.birth(&source.borrow(), &mut Rng::new(1)).unwrap().position,
        [18.0, -4.0, 0.0]
    );
}

#[test]
fn inherits_source_motion() {
    let emitter =
        ImageEmitter::parse(&serde_json::json!({"flags":262_144,"speedmin":2,"speedmax":2}));
    let source = source();
    source.borrow_mut().current.transform.x = 3.0;
    source.borrow_mut().dt = 0.5;
    assert_eq!(
        emitter.birth(&source.borrow(), &mut Rng::new(1)).unwrap().velocity,
        [12.0, 0.0, 0.0]
    );
}

#[test]
fn parent_motion_follows_space() {
    for world in [false, true] {
        let mut system = system(ImageFlags::COLOR.bits());
        system.emitters =
            vec![parse_emitter(&serde_json::json!({"name":"layerimage","rate":0})).unwrap()];
        system.world = world;
        system.origin = (10.0, 20.0, 0.0);
        system.angle = 0.35;
        system.scale = 2.0;
        system.scale3 = [2.0, 0.5, 1.0];
        let mut sim = Sim::new(1);
        sim.bind_images(vec![Some(source())]);
        sim.step(&system, 0.01);
        sim.emit_particles(&system, 1, &mut |_| {});
        let render = |system: &crate::particles::ParticleSystem, point| {
            let scale = system.draw_scale3();
            Affine::new([system.origin.0, system.origin.1], [scale[0], scale[1]], system.angle)
                .point(point)
        };
        let before = render(&system, sim.particles[0].pos);
        assert!((before[0] - 4.0).abs() < 0.001);
        assert!((before[1] + 8.0).abs() < 0.001);
        system.origin.0 += 200.0;
        system.origin.1 += 200.0;
        sim.step(&system, 0.01);
        let after = render(&system, sim.particles[0].pos);
        let shift = if world { 0.0 } else { 200.0 };
        assert!((after[0] - before[0] - shift).abs() < 0.001);
        assert!((after[1] - before[1] - shift).abs() < 0.001);
    }
}

#[test]
fn tiny_scale_keeps_birth() {
    let flags = ImageFlags::COLOR.bits() | ImageFlags::MOTION.bits();
    let mut system = system(flags);
    system.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"layerimage","flags":flags,"speedmin":1,"speedmax":1
        }))
        .unwrap(),
    ];
    system.scale = 1e-8;
    system.scale3 = [1e-8, 1e-8, 1.0];
    let source = source();
    source.borrow_mut().current.transform.x = 3.0;
    source.borrow_mut().dt = 1.0;
    let mut sim = Sim::new(1);
    sim.bind_images(vec![Some(source)]);
    sim.emit_particles(&system, 1, &mut |_| {});
    let particle = &sim.particles[0];
    assert!((particle.pos[0] * system.scale3[0] - 7.0).abs() < 0.001);
    assert!((particle.pos[1] * system.scale3[1] + 8.0).abs() < 0.001);
    assert!((particle.vel[0] * system.scale3[0] - 3.0).abs() < 0.001);
}

#[test]
fn invalid_inverse_skips_birth() {
    for scale in [0.0, f32::NAN, f32::INFINITY, f32::MIN_POSITIVE] {
        let mut system = system(ImageFlags::COLOR.bits());
        system.scale3[0] = scale;
        let mut sim = Sim::new(1);
        sim.bind_images(vec![Some(source())]);
        sim.emit_particles(&system, 1, &mut |_| {});
        assert!(sim.particles.is_empty());
    }
    let flags = ImageFlags::COLOR.bits() | ImageFlags::MOTION.bits();
    let mut system = system(flags);
    system.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name":"layerimage","flags":flags,"speedmin":1,"speedmax":1
        }))
        .unwrap(),
    ];
    let source = source();
    source.borrow_mut().current.transform.x = 5.0;
    source.borrow_mut().dt = f32::MIN_POSITIVE;
    let mut sim = Sim::new(1);
    sim.bind_images(vec![Some(source)]);
    sim.emit_particles(&system, 1, &mut |_| {});
    assert!(sim.particles.is_empty());
}

#[test]
fn refresh_invalidates_shared_source() {
    let mut system = system(ImageFlags::COLOR.bits());
    let mut second =
        parse_emitter(&serde_json::json!({"name":"layerimage","flags":196_608,"rate":10})).unwrap();
    if let Emitter::Image { image, .. } = &mut second {
        image.ordinal = 1;
    }
    system.emitters.push(second);
    let source = source();
    let mut sim = Sim::new(1);
    sim.bind_images(vec![Some(Rc::clone(&source)), Some(Rc::clone(&source))]);
    for _ in 0..11 {
        sim.step(&system, 0.1);
    }
    assert_eq!(source.borrow().state, SampleState::Unready);
    source.borrow_mut().state = SampleState::Pending;
    source.borrow_mut().invalidate();
    assert_eq!(source.borrow().state, SampleState::Pending);
    assert_eq!(std::mem::size_of::<SampleCell>(), 8);
}

#[test]
fn zero_rate_refresh_waits() {
    let mut system = system(ImageFlags::COLOR.bits());
    let flags = ImageFlags::COLOR.bits() | ImageFlags::REFRESH.bits();
    system.emitters.push(
        parse_emitter(&serde_json::json!({"name":"layerimage","flags":flags,"rate":0})).unwrap(),
    );
    let source = source();
    let mut sim = Sim::new(1);
    sim.bind_images(vec![Some(Rc::clone(&source)), Some(Rc::clone(&source))]);
    for _ in 0..11 {
        sim.step(&system, 0.1);
    }
    assert_eq!(source.borrow().state, SampleState::Ready);
    assert_eq!(sim.image_refresh[1], 0.0);
    assert!(!sim.particles.is_empty());
}

#[test]
fn refresh_counts_births() {
    let mut system = system(ImageFlags::COLOR.bits() | ImageFlags::REFRESH.bits());
    if let Emitter::Image { rate, .. } = &mut system.emitters[0] {
        *rate = 1.0;
    }
    let mut sim = Sim::new(1);
    sim.bind_images(vec![Some(source())]);
    for _ in 0..9 {
        sim.step(&system, 0.1);
    }
    assert_eq!(sim.image_refresh[0], 0.0);
    sim.step(&system, 0.1);
    assert_eq!(sim.particles.len(), 1);
    assert!((sim.image_refresh[0] - 0.1).abs() < 0.001);
}

#[test]
fn full_capacity_stops_refresh() {
    let mut system = system(ImageFlags::COLOR.bits() | ImageFlags::REFRESH.bits());
    system.max_count = 1;
    if let Emitter::Image { rate, .. } = &mut system.emitters[0] {
        *rate = 100.0;
    }
    let mut sim = Sim::new(1);
    let source = source();
    sim.bind_images(vec![Some(Rc::clone(&source))]);
    for _ in 0..11 {
        sim.step(&system, 0.01);
    }
    assert_eq!(sim.particles.len(), 1);
    assert!((sim.image_refresh[0] - 0.01).abs() < 0.001);
    assert_eq!(source.borrow().state, SampleState::Ready);
}

#[test]
fn automatic_selects_rated_emitter() {
    let mut system = system(ImageFlags::COLOR.bits());
    let mut inactive = parse_emitter(&serde_json::json!({"name":"layerimage","rate":0})).unwrap();
    if let Emitter::Image { image, .. } = &mut inactive {
        image.ordinal = 1;
    }
    system.emitters.push(inactive);
    let green = source();
    let red = source();
    red.borrow_mut().cells[0].color = [255, 0, 0];
    let mut sim = Sim::new(1);
    sim.bind_images(vec![Some(green), Some(red)]);
    for _ in 0..5 {
        sim.step(&system, 0.1);
    }
    assert!(!sim.particles.is_empty());
    assert!(sim.particles.iter().all(|particle| particle.color == [0.0, 1.0, 0.0]));
}

#[test]
fn followers_share_image_source() {
    use crate::particles::{ChildEvent, ChildEventKind, Followers};
    let parent = crate::particles::tests::system();
    let mut parent_sim = Sim::new(1);
    parent_sim.emit_particles(&parent, 1, &mut |_| {});
    let mut child = system(ImageFlags::COLOR.bits());
    child.child_event =
        Some(ChildEvent { kind: ChildEventKind::Follow, limit: 1, probability: 1.0 });
    let mut followers = Followers::default();
    let mut output = Sim::new(2);
    let source = source();
    output.bind_images(vec![Some(Rc::clone(&source))]);
    followers.step(&mut output, &child, (&parent, &parent_sim), 0.1, (&[], &[]), &mut |_| {});
    assert_eq!(output.particles[0].color, [0.0, 1.0, 0.0]);
    source.borrow_mut().cells[0].color = [255, 0, 0];
    followers.step(&mut output, &child, (&parent, &parent_sim), 0.1, (&[], &[]), &mut |_| {});
    assert_eq!(output.particles.last().unwrap().color, [1.0, 0.0, 0.0]);
}

#[test]
fn load_binds_image_ordinal() {
    use crate::particles::{LoadContext, ParticleDefaults};
    let definition = br#"{"renderer":[{"name":"sprite"}],"emitter":[{"name":"boxrandom","rate":0},{"name":"layerimage","rate":10}]}"#;
    let bytes = crate::tests::build_pkg(&[("particles/source.json", definition)]);
    let pkg = crate::pkg::Package::parse(bytes).unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let context = LoadContext {
        pkg: &pkg,
        assets: &assets,
        defaults: ParticleDefaults::ORTHOGRAPHIC,
        version: 0,
        viewport: None,
        bounds_static: false,
    };
    let object = serde_json::json!({"dependencies":[{"type":"emitterimage","id":38,"index":0,"mask":"raw-mask"}]});
    let (system, skipped) = crate::particles::load(&context, &object, "particles/source.json");
    let system = system.unwrap();
    assert!(skipped.is_empty());
    assert!(matches!(system.emitters[0], Emitter::Box { .. }));
    let Emitter::Image { image, .. } = &system.emitters[1] else {
        panic!("image emitter");
    };
    assert_eq!(image.ordinal, 0);
    assert_eq!(
        system.image_dependencies,
        [ImageDependency { source: 38, ordinal: 0, mask: "raw-mask".to_owned() }]
    );
    let mut sim = Sim::new(1);
    sim.bind_images(vec![Some(source())]);
    sim.step(&system, 0.1);
    assert!(!sim.particles.is_empty());
    assert!(sim.particles.iter().all(|particle| particle.color == [0.0, 1.0, 0.0]));
}

#[test]
fn bounds_sampling_grid() {
    assert_eq!(SampleGrid::new([7680, 4320]).unwrap().target, [960, 540]);
    assert_eq!(SampleGrid::new([8, 65535]).unwrap().target, [2, 540]);
    assert_eq!(SampleGrid::new([1, 1]).unwrap().target, [2, 2]);
    assert!(SampleGrid::new([0, 1]).is_none());
    assert_eq!(SampleGrid::new([7680, 4320]).unwrap().bytes(), 2_073_600);
}

#[test]
fn quantizes_eligible_cells() {
    let grid = SampleGrid::new([9, 9]).unwrap();
    let pixels = [1, 2, 3, 126, 4, 5, 6, 127, 7, 8, 9, 128, 10, 11, 12, 255];
    let mut cells = Vec::new();
    grid.read(&pixels, &mut cells).unwrap();
    assert_eq!(
        cells,
        [
            SampleCell { color: [7, 8, 9], bone: 255, position: [-1, 2] },
            SampleCell { color: [4, 5, 6], bone: 255, position: [2, -1] },
            SampleCell { color: [10, 11, 12], bone: 255, position: [2, 2] },
        ]
    );
    assert!(grid.read(&pixels[..12], &mut cells).is_err());
}

#[test]
fn cold_burst_demands_once() {
    let mut system = system(ImageFlags::COLOR.bits());
    system.emitters = vec![
        parse_emitter(&serde_json::json!({
            "name": "layerimage", "rate": 0, "instantaneous": 10,
        }))
        .unwrap(),
    ];
    let source = Rc::new(RefCell::new(SampleSource::default()));
    let mut sim = Sim::new(1);
    sim.bind_images(vec![Some(Rc::clone(&source))]);
    assert!(!source.borrow().requested);
    sim.step(&system, 0.1);
    assert!(!source.borrow().requested);
    assert_eq!(source.borrow().first_query_frame, Some(1));
    assert!(sim.particles.is_empty());
    {
        let mut source = source.borrow_mut();
        source.requested = false;
        source.state = SampleState::Ready;
        source.cells.push(SampleCell { color: [255; 3], bone: UNBOUND_BONE, position: [0; 2] });
    }
    sim.step(&system, 0.1);
    assert!(!source.borrow().requested);
    assert!(sim.particles.is_empty());
}

#[test]
fn zero_rate_waits() {
    let mut system = system(ImageFlags::COLOR.bits());
    system.emitters =
        vec![parse_emitter(&serde_json::json!({"name": "layerimage", "rate": 0})).unwrap()];
    let source = Rc::new(RefCell::new(SampleSource::default()));
    let mut sim = Sim::new(1);
    sim.bind_images(vec![Some(Rc::clone(&source))]);
    sim.step(&system, 1.0);
    assert!(!source.borrow().requested);
    sim.emit_particles(&system, 1, &mut |_| {});
    assert!(!source.borrow().requested);
    sim.set_scene_frame(2);
    sim.emit_particles(&system, 1, &mut |_| {});
    assert!(source.borrow().requested);
}

#[test]
fn spatial_palette_sample() {
    let matrix = [0.0, 0.0, 2.0, 0.0, 0.0, 3.0, 0.0, 0.0, -4.0, 0.0, 0.0, 0.0, 5.0, 6.0, 7.0, 1.0];
    let pose = SamplePose {
        transform: Affine::new([10.0, 20.0], [1.0, 1.0], 0.0),
        bones: vec![Some(crate::puppet::BonePose::Spatial(matrix)), None].into(),
    };
    assert_eq!(pose.point([1.0, 2.0, 3.0], 0), Some([3.0, 32.0, 9.0]));
    assert_eq!(pose.point([1.0, 2.0, 3.0], UNBOUND_BONE), Some([11.0, 22.0, 3.0]));
    assert_eq!(pose.point([1.0, 2.0, 3.0], 1), None);
    assert_eq!(pose.point([1.0, 2.0, 3.0], 2), None);
}
