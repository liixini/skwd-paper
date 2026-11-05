use super::*;
use crate::particles::{CollisionResponse, Sim, image::ImageDependency};

fn source() -> Source {
    Source {
        size: [40.0, 80.0, 20.0],
        center: [0.0; 3],
        matrix: model_matrix([0.0; 3], [1.0; 3], [0.0; 3]),
    }
}

fn capsule(source: Source) -> Capsule {
    Capsule::prepare(source.size, source.center, |point| source.point(point), |v| source.vector(v))
        .unwrap()
}

#[test]
fn fallback_capsule() {
    let capsule = capsule(source());
    assert_eq!(capsule.origin, [0.0, -40.0, 0.0]);
    assert_eq!(capsule.axis, [0.0, 1.0, 0.0]);
    assert_eq!(capsule.length, 80.0);
    assert_eq!(capsule.radius, 40.0);
    assert!(capsule.contact([34.0, 60.0, 0.0]).is_some());
    assert!(capsule.contact([36.0, 60.0, 0.0]).is_none());
}

#[test]
fn strict_tangent() {
    let capsule = capsule(source());
    for (x, hit) in [(39.0, true), (40.0, false), (41.0, false)] {
        assert_eq!(capsule.contact([x, 40.0, 0.0]).is_some(), hit);
    }
}

#[test]
fn scaled_rotated_capsule() {
    let mut source = source();
    source.matrix = model_matrix([0.0; 3], [0.5, 0.75, 1.5], [0.0; 3]);
    let scaled = capsule(source);
    assert_eq!(scaled.origin, [0.0, -30.0, 0.0]);
    assert_eq!(scaled.length, 60.0);
    assert_eq!(scaled.radius, 20.0);
    source.matrix = model_matrix([0.0; 3], [1.0; 3], [std::f32::consts::FRAC_PI_2, 0.0, 0.0]);
    let rotated = capsule(source);
    assert!(rotated.axis[1].abs() < 1e-6);
    assert!((rotated.axis[2] - 1.0).abs() < 1e-6);
    assert!(rotated.contact([34.0, 0.0, 60.0]).is_some());
}

#[test]
fn secondary_axis_tie() {
    let mut source = source();
    source.size = [20.0, 20.0, 40.0];
    source.matrix = model_matrix([0.0; 3], [2.0, 3.0, 1.0], [0.0; 3]);
    assert_eq!(capsule(source).radius, 60.0);
}

#[test]
fn mixed_type_binding() {
    let object = serde_json::json!({"dependencies":[
        {"type":"emitterimage","index":0,"id":38,"mask":"sample"},
        {"type":"collisionmodel","index":0,"id":24},
        {"type":"collisionmodel","index":1,"id":25}
    ]});
    let image = ImageDependency::parse(&object);
    assert_eq!(image[0].source, 38);
    assert_eq!(image[0].ordinal, 0);
    let mut system = crate::particles::tests::system();
    system.model_collisions = Bindings::parse(&object, 2);
    let mut skipped = Vec::new();
    bind_system(&mut system, &BTreeMap::from([(24, source())]), true, &mut skipped);
    assert!(system.model_collisions.sources[0].is_some());
    assert!(system.model_collisions.sources[1].is_none());
    assert_eq!(skipped.len(), 1);
}

#[test]
fn moving_particle_frame() {
    let mut system = crate::particles::tests::system();
    system.origin = (0.0, 0.0, 0.0);
    system.angle = 0.0;
    system.scale3 = [1.0; 3];
    system.world = false;
    system.model_collisions.sources = vec![Some(source())];
    let mut capsules = Vec::new();
    let mut previous = None;
    let mut failed = false;
    prepare(&system, &mut capsules, &mut previous, &mut failed);
    assert_eq!(capsules[0].unwrap().origin, [0.0, -40.0, 0.0]);
    system.origin = (10.0, 20.0, 30.0);
    prepare(&system, &mut capsules, &mut previous, &mut failed);
    assert_eq!(capsules[0].unwrap().origin, [-10.0, -60.0, -30.0]);
    system.world = true;
    prepare(&system, &mut capsules, &mut previous, &mut failed);
    assert_eq!(capsules[0].unwrap().origin, [0.0, -40.0, 0.0]);
}

#[test]
fn operator_responses() {
    for response in [
        CollisionResponse::Bounce(0.5),
        CollisionResponse::Slide,
        CollisionResponse::Stop,
        CollisionResponse::Delete,
    ] {
        let mut system = crate::particles::tests::system();
        system.emitters.clear();
        system.model_collisions.sources = vec![Some(source())];
        system.origin = (0.0, 0.0, 0.0);
        system.angle = 0.0;
        system.scale3 = [1.0; 3];
        system.world = false;
        system.operators = vec![super::super::Operator::CollisionModel {
            ordinal: 0,
            collision: Collision { response, stop_angular: false },
        }];
        let mut sim = Sim::new(1);
        sim.particles.push(Particle {
            id: 1,
            pos: [39.0, 40.0, 0.0],
            vel: [-4.0, 2.0, 0.0],
            size: 1.0,
            base_size: 1.0,
            color: [1.0; 3],
            base_color: [1.0; 3],
            alpha: 1.0,
            base_alpha: 1.0,
            angle: 0.0,
            angle_xy: [0.0; 2],
            angular: 3.0,
            angular_xy: [0.0; 2],
            age: 0.0,
            lifetime: 8.0,
            phase: 0.0,
            noise_seed: 0,
            slot: 0,
        });
        sim.step(&system, 0.0);
        if matches!(response, CollisionResponse::Delete) {
            assert!(sim.particles.is_empty());
            continue;
        }
        let particle = &sim.particles[0];
        assert!((particle.pos[0] - 40.0).abs() < 0.03);
        assert_eq!(particle.angular, 3.0);
        let velocity = match response {
            CollisionResponse::Bounce(_) => [2.0, 2.0, 0.0],
            CollisionResponse::Slide => [0.0, 2.0, 0.0],
            CollisionResponse::Stop => [0.0; 3],
            CollisionResponse::Delete => unreachable!(),
        };
        for (actual, expected) in particle.vel.iter().zip(velocity) {
            assert!((actual - expected).abs() < 0.01);
        }
    }
}

fn scene() -> Value {
    serde_json::json!({
        "camera":{"eye":"0 0 -400","center":"0 0 0","up":"0 1 0"},
        "general":{"fov":60,"nearz":0.1,"farz":1000},
        "objects":[
            {"id":24,"model":"models/mesh.mdl"},
            {"id":99,"origin":"0 0 0"},
            {"id":10,"particle":"particles/root.json","dependencies":[
                {"type":"emitterimage","id":38,"index":0,"mask":"sample"},
                {"type":"collisionmodel","id":24,"index":0}
            ]}
        ]
    })
}

fn definition() -> Value {
    serde_json::json!({
        "renderer":[{"name":"sprite"}],
        "emitter":[{"name":"boxrandom","rate":0}],
        "operator":[{"name":"collisionmodel","collisionbehavior":"stop"}]
    })
}

fn load_scene(
    scene: &Value,
    definition: &Value,
    child: Option<&Value>,
) -> crate::model::SceneModel {
    let scene = scene.to_string();
    let base = crate::model::tests::mesh_package_scene(
        &serde_json::json!([{"shader":"mesh"}]),
        scene.as_bytes(),
    );
    let mut mesh = base.find("models/mesh.mdl").unwrap().to_vec();
    assert_eq!(&mesh[..9], b"MDLV0004\0");
    mesh[4..8].copy_from_slice(b"0023");
    let bounds_at = 9 + 3 * size_of::<u32>() + b"materials/mesh.json\0".len() + size_of::<u32>();
    let mut bounds = Vec::new();
    for value in [-40.0_f32, -80.0, -20.0, 40.0, 80.0, 20.0] {
        bounds.extend(value.to_le_bytes());
    }
    bounds.extend(9_u32.to_le_bytes());
    drop(mesh.splice(bounds_at..bounds_at, bounds));
    mesh.extend([0; 7]);
    let definition = definition.to_string();
    let child = child.map(Value::to_string);
    let mut files: Vec<_> = base
        .entries()
        .iter()
        .filter(|entry| entry.path != "models/mesh.mdl")
        .map(|entry| (entry.path.as_str(), base.read(entry)))
        .collect();
    files.push(("models/mesh.mdl", &mesh));
    files.push(("particles/root.json", definition.as_bytes()));
    if let Some(child) = &child {
        files.push(("particles/child.json", child.as_bytes()));
    }
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&files)).unwrap();
    crate::model::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap()
}

#[test]
fn load_operator_ordinals() {
    let mut definition = definition();
    definition["emitter"] = serde_json::json!([
        {"name":"boxrandom","rate":0}, {"name":"layerimage","rate":0}
    ]);
    definition["operator"] = serde_json::json!([
        {"name":"movement"},
        {"name":"collisionmodel","flags":3,"controlpoint":7},
        {"name":"alphafade"},
        {"name":"collisionmodel","collisionbehavior":"stop"}
    ]);
    let mut scene = scene();
    scene["objects"][2]["dependencies"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"type":"collisionmodel","id":24,"index":1}));
    let model = load_scene(&scene, &definition, None);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let system = &model.particles[0].system;
    let crate::particles::Emitter::Image { image, .. } = &system.emitters[1] else {
        panic!("image emitter");
    };
    assert_eq!(image.ordinal, 0);
    let operators: Vec<_> = system
        .operators
        .iter()
        .filter_map(|operator| {
            if let crate::particles::Operator::CollisionModel { ordinal, collision } = operator {
                Some((*ordinal, collision))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(operators.iter().map(|(ordinal, _)| *ordinal).collect::<Vec<_>>(), [0, 1]);
    assert!(matches!(operators[0].1.response, CollisionResponse::Bounce(0.5)));
    assert!(!operators[0].1.stop_angular);
    assert!(matches!(operators[1].1.response, CollisionResponse::Stop));
    assert!(system.model_collisions.sources.iter().all(Option::is_some));
}

#[test]
fn retains_mesh_bounds() {
    let mut scene = scene();
    scene["objects"][0]["scale"] = serde_json::json!("0.5 0.75 1.5");
    scene["objects"][0]["origin"] = serde_json::json!("10 20 30");
    let mut model = load_scene(&scene, &definition(), None);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(
        model.meshes[0].mesh.parts[0].bounds,
        Some(([-40.0, -80.0, -20.0], [40.0, 80.0, 20.0]))
    );
    drop(std::mem::take(&mut model.meshes));
    let system = &model.particles[0].system;
    let mut capsules = Vec::new();
    prepare(system, &mut capsules, &mut None, &mut false);
    let capsule = capsules[0].unwrap();
    assert_eq!(capsule.origin, [10.0, -10.0, 30.0]);
    assert_eq!(capsule.length, 60.0);
    assert_eq!(capsule.radius, 20.0);
}

#[test]
fn unsupported_root_skips() {
    for (field, value) in [
        ("angles", serde_json::json!("0.3 0 0")),
        ("angles", serde_json::json!("0 0.3 0")),
        ("parent", serde_json::json!(99)),
    ] {
        let mut scene = scene();
        scene["objects"][2][field] = value;
        let model = load_scene(&scene, &definition(), None);
        assert!(model.particles[0].system.model_collisions.sources[0].is_none());
        assert!(
            model
                .skipped
                .iter()
                .any(|reason| reason
                    == "collisionmodel: particle transform or high count unsupported")
        );
    }
    let mut definition = definition();
    definition["maxcount"] = serde_json::json!(10000);
    let model = load_scene(&scene(), &definition, None);
    assert!(model.particles[0].system.model_collisions.sources[0].is_none());
    assert!(model.skipped.iter().any(|reason| reason.contains("high count unsupported")));
}

#[test]
fn unsupported_source_skips() {
    for (field, value) in
        [("visible", serde_json::json!(false)), ("physics", serde_json::json!(true))]
    {
        let mut scene = scene();
        scene["objects"][0][field] = value;
        let model = load_scene(&scene, &definition(), None);
        assert!(model.particles[0].system.model_collisions.sources[0].is_none());
        assert!(
            model
                .skipped
                .iter()
                .any(|reason| reason == "collisionmodel: source 24 unsupported or unavailable")
        );
    }
    let mut scene = scene();
    scene["objects"][2]["dependencies"][1]["id"] = serde_json::json!(999);
    let model = load_scene(&scene, &definition(), None);
    assert!(model.particles[0].system.model_collisions.sources[0].is_none());
    assert!(
        model
            .skipped
            .iter()
            .any(|reason| reason == "collisionmodel: source 999 unsupported or unavailable")
    );
}

#[test]
fn child_inherits_bindings() {
    let child = definition();
    let mut root = definition();
    root["children"] = serde_json::json!([{
        "name":"particles/child.json", "dependencies":[
            {"type":"collisionmodel","id":777,"index":0},
            {"type":"emitterimage","id":888,"index":0}
        ]
    }]);
    let mut scene = scene();
    let model = load_scene(&scene, &root, Some(&child));
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let root = &model.particles[0].system;
    let child = &root.children[0];
    assert_eq!(child.model_collisions.dependencies, [(24, 0)]);
    assert_eq!(child.image_dependencies, root.image_dependencies);
    assert!(child.model_collisions.sources[0].is_some());
    assert!(matches!(
        child.operators[0],
        crate::particles::Operator::CollisionModel { ordinal: 0, .. }
    ));
    scene["objects"][2].as_object_mut().unwrap().remove("dependencies");
    let mut root = definition();
    root["children"] = serde_json::json!([{"name":"particles/child.json","dependencies":[
        {"type":"collisionmodel","id":777,"index":0}
    ]}]);
    let model = load_scene(&scene, &root, Some(&definition()));
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let child = &model.particles[0].system.children[0];
    assert!(child.model_collisions.dependencies.is_empty());
    assert!(child.model_collisions.sources[0].is_none());
}

#[test]
fn overflow_source_skips() {
    let mut scene = scene();
    scene["objects"][0]["scale"] = serde_json::json!("1e38 1 1");
    let model = load_scene(&scene, &definition(), None);
    assert_eq!(model.meshes.len(), 1);
    assert!(model.particles[0].system.model_collisions.sources[0].is_none());
    assert!(
        model
            .skipped
            .iter()
            .any(|reason| reason == "collisionmodel: source 24 unsupported or unavailable")
    );
}

#[test]
fn finite_bounds_overflow() {
    let scene = scene();
    let mut model = load_scene(&scene, &definition(), None);
    model.meshes[0].mesh.parts[0].bounds = Some(([2e38, -80.0, -20.0], [3e38, 80.0, 20.0]));
    assert!(Source::of(&model.meshes[0]).is_none());
    let objects = scene["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|object| (object["id"].as_u64().unwrap().to_string(), object))
        .collect();
    bind(&mut model.particles, &model.meshes, &objects, &mut model.skipped);
    assert!(model.particles[0].system.model_collisions.sources[0].is_none());
    assert!(
        model
            .skipped
            .iter()
            .any(|reason| reason == "collisionmodel: source 24 unsupported or unavailable")
    );
}

#[test]
fn composed_matrix_overflow() {
    let mut model = load_scene(&scene(), &definition(), None);
    model.meshes[0].pose.scale = [1e30; 3];
    model.meshes[0].parents.push(crate::model::MeshPose {
        origin: [0.0; 3],
        scale: [1e30; 3],
        angles: [0.0; 3],
    });
    assert!(Source::of(&model.meshes[0]).is_none());
}

#[test]
fn runtime_overflow_disables() {
    let mut system = crate::particles::tests::system();
    system.origin = (0.0, 0.0, 0.0);
    system.angle = 0.0;
    system.scale3 = [1.0; 3];
    system.world = false;
    system.model_collisions.sources = vec![Some(source())];
    let mut capsules = Vec::new();
    let mut previous = None;
    let mut failed = false;
    assert!(!prepare(&system, &mut capsules, &mut previous, &mut failed));
    assert!(capsules[0].is_some());
    system.scale3 = [f32::MIN_POSITIVE; 3];
    assert!(prepare(&system, &mut capsules, &mut previous, &mut failed));
    assert!(capsules[0].is_none());
    assert!(!prepare(&system, &mut capsules, &mut previous, &mut failed));
    system.scale3 = [2.0 * f32::MIN_POSITIVE; 3];
    assert!(!prepare(&system, &mut capsules, &mut previous, &mut failed));
    assert!(capsules[0].is_none());
    system.scale3 = [1.0; 3];
    assert!(!prepare(&system, &mut capsules, &mut previous, &mut failed));
    assert!(capsules[0].is_some());
    system.scale3 = [f32::MIN_POSITIVE; 3];
    assert!(prepare(&system, &mut capsules, &mut previous, &mut failed));
}

#[test]
fn child_transform_admission() {
    for (field, value) in [
        ("angles", serde_json::json!("0.3 0 0")),
        ("angles", serde_json::json!("0 0.3 0")),
        ("parent", serde_json::json!(99)),
    ] {
        let mut root = definition();
        root["children"] = serde_json::json!([{"name":"particles/child.json"}]);
        root["children"][0][field] = value;
        let model = load_scene(&scene(), &root, Some(&definition()));
        let root = &model.particles[0].system;
        assert!(root.model_collisions.sources[0].is_some());
        assert!(root.children[0].model_collisions.sources[0].is_none());
        assert!(
            model
                .skipped
                .iter()
                .any(|reason| reason
                    == "collisionmodel: particle transform or high count unsupported")
        );
    }
}

#[test]
fn centerline_preserves_nan() {
    let (normal, depth) = capsule(source()).contact([0.0, 0.0, 0.0]).unwrap();
    assert!(normal.iter().all(|value| value.is_nan()));
    assert!(depth.is_nan());
}

#[test]
fn event_binding_skips() {
    for event in ["eventfollow", "eventspawn", "eventdeath"] {
        let mut root = definition();
        root["children"] = serde_json::json!([{"name":"particles/child.json","type":event}]);
        let model = load_scene(&scene(), &root, Some(&definition()));
        let root = &model.particles[0].system;
        assert!(root.model_collisions.sources[0].is_some());
        assert!(root.children[0].model_collisions.sources[0].is_none());
        assert!(
            model
                .skipped
                .iter()
                .any(|reason| reason == "collisionmodel: event children unsupported")
        );
    }
}

#[test]
fn follower_collision_guard() {
    let mut root = definition();
    root["emitter"] = serde_json::json!([{
        "name":"boxrandom", "rate":0, "instantaneous":1, "origin":"200 0 0"
    }]);
    root["operator"] = serde_json::json!([]);
    root["children"] = serde_json::json!([{"name":"particles/child.json","type":"eventfollow"}]);
    let mut child = definition();
    child["emitter"] = serde_json::json!([{"name":"boxrandom","rate":0,"instantaneous":1}]);
    child["operator"] = serde_json::json!([{"name":"collisionmodel","collisionbehavior":"delete"}]);
    let mut model = load_scene(&scene(), &root, Some(&child));
    let mut root = model.particles.remove(0).system;
    let mut child = root.children.remove(0);
    assert!(!child.world);
    assert!(child.model_collisions.sources[0].is_none());
    let mut parent = Sim::new(1);
    parent.step(&root, 0.0);
    assert_eq!(parent.particles.len(), 1);
    assert_eq!(parent.particles[0].pos, [200.0, 0.0, 0.0]);
    let mut followers = crate::particles::Followers::default();
    let mut output = Sim::new(2);
    followers.step(&mut output, &child, (&root, &parent), 0.0, (&[], &[]), &mut |_| {});
    assert_eq!(output.particles.len(), 1);
    assert_eq!(output.particles[0].pos, [200.0, 0.0, 0.0]);
    child.model_collisions.sources[0] = Some(source());
    let mut followers = crate::particles::Followers::default();
    let mut output = Sim::new(2);
    followers.step(&mut output, &child, (&root, &parent), 0.0, (&[], &[]), &mut |_| {});
    assert!(output.particles.is_empty());
}
