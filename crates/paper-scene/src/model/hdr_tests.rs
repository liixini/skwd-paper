use super::*;

#[test]
fn hdr_requires_authored_flag() {
    let props = Properties::new();
    assert!(!enabled(Some(&json!({"bloom": true})), &props));
    assert!(!enabled(Some(&json!({"hdr": true})), &props));
    assert!(!enabled(Some(&json!({"hdr": true, "bloom": true})), &props));
    assert!(enabled(
        Some(
            &json!({"hdr": true, "bloom": true, "orthogonalprojection": {"postprocessing": "ultra"}})
        ),
        &props
    ));
    assert!(enabled(
        Some(
            &json!({"hdr": true, "bloom": true, "orthogonalprojection": {"postprocessing": "displayhdr"}})
        ),
        &props
    ));
    assert!(bloom_entry(Some(&json!({"hdr": true})), &props, (1280.0, 720.0)).is_none());
}

#[test]
fn hdr_matches_native_pyramid() {
    let entry = bloom_entry(
        Some(&json!({"hdr": true, "bloom": true, "orthogonalprojection": {"postprocessing": "ultra"}})),
        &Properties::new(),
        (1280.0, 720.0),
    )
    .unwrap();
    let definition = definition(EFFECT_FILE, &entry).unwrap();
    let passes = definition["passes"].as_array().unwrap();
    assert_eq!(passes.len(), 16);
    assert_eq!(definition["fbos"][7]["scale"], 256);
    assert_eq!(passes[8]["material"], CUBIC_UPSAMPLE);
    assert_eq!(passes[9]["material"], CUBIC_UPSAMPLE);
    assert_eq!(passes[10]["material"], UPSAMPLE);
    assert_eq!(passes[14]["target"], LEVELS[0]);
    assert_eq!(
        entry["passes"][1]["constantshadervalues"][RENDER_VAR][0],
        f64::from(2.0f32 / 1280.0)
    );
    assert_eq!(
        entry["passes"][15]["constantshadervalues"][RENDER_VAR],
        json!([1.0, 0.0, 0.0, 0.0])
    );
}

#[test]
fn hdr_bounds_iterations() {
    for (size, authored, count) in [(16.0, 8, 4), (720.0, 0, 0), (720.0, 1, 0), (720.0, 3, 3)] {
        let entry = bloom_entry(
            Some(
                &json!({"hdr": true, "bloom": true, "bloomhdriterations": authored, "orthogonalprojection": {"postprocessing": "ultra"}}),
            ),
            &Properties::new(),
            (size, size),
        );
        if count == 0 {
            assert!(entry.is_none());
            continue;
        }
        let entry = entry.unwrap();
        let definition = definition(EFFECT_FILE, &entry).unwrap();
        assert_eq!(definition["fbos"].as_array().unwrap().len(), count);
        assert_eq!(definition["passes"].as_array().unwrap().len(), count * 2);
    }
}

#[test]
fn pyramid_uses_integer_extents() {
    use crate::effects::{Fbo, FboFormat, FboSize};
    let mut targets: Vec<_> = [32.0, 128.0, 256.0]
        .into_iter()
        .map(|scale| Fbo {
            name: String::new(),
            size: FboSize::Scale(scale),
            format: FboFormat::Rgba16f,
            repeat: None,
            clear: [0.0; 4],
            clear_on_setup: false,
            unique: false,
        })
        .collect();
    configure_targets(EFFECT_FILE, &mut targets);
    assert_eq!(
        targets.iter().map(|target| target.extent((1280, 720))).collect::<Vec<_>>(),
        [(40, 22), (10, 5), (5, 2)]
    );
}

#[test]
fn viewport_preserves_pyramid_plan() {
    use crate::effects::{Effect, EffectBind, EffectPass, EffectState, PassBlend};
    use crate::shader::{Stage, translate};
    for iterations in [3, 8] {
        let bloom = Bloom { canvas: [1280, 720], iterations, ..Bloom::default() };
        let (plan, overrides) = bloom.plan();
        let entry = json!({"passes": &overrides});
        let passes = plan
            .passes
            .into_iter()
            .zip(overrides)
            .map(|(pass, values)| EffectPass {
                blend: PassBlend::Copy,
                property_source: None,
                user_uniforms: std::collections::BTreeSet::new(),
                name: String::new(),
                vertex: translate("void main() {}", Stage::Vertex, &BTreeMap::new()),
                geometry: None,
                fragment: translate("void main() {}", Stage::Fragment, &BTreeMap::new()),
                hlsl: None,
                textures: Vec::new(),
                values: BTreeMap::from([(
                    RENDER_VAR.into(),
                    serde_json::from_value(values.constantshadervalues[RENDER_VAR].clone())
                        .unwrap(),
                )]),
                target: pass.target.map(str::to_owned),
                binds: pass
                    .bind
                    .into_iter()
                    .map(|binding| (binding.index, EffectBind::parse(binding.name, None)))
                    .collect(),
            })
            .collect();
        let mut effects = [Effect {
            state: EffectState::default(),
            name: String::new(),
            passes,
            fbos: Vec::new(),
            swaps: Vec::new(),
            clear_functions: std::collections::BTreeMap::new(),
        }];
        let original: Vec<_> = effects[0].passes.iter().map(|pass| pass.values.clone()).collect();
        for (index, pass) in effects[0].passes.iter_mut().enumerate() {
            let expected = std::mem::take(&mut pass.values);
            configure_pass("effects/custom.json", &entry, index, pass);
            assert!(pass.values.is_empty());
            configure_pass(EFFECT_FILE, &entry, index, pass);
            assert_eq!(pass.values, expected);
        }
        resize(&mut effects, bloom.canvas);
        assert_eq!(
            effects[0].passes.iter().map(|pass| pass.values.clone()).collect::<Vec<_>>(),
            original
        );
        for canvas in [[720, 1280], [640, 360], [1280, 720]] {
            resize(&mut effects, canvas);
            for (pass, values) in effects[0].passes.iter().zip(&original) {
                let expected = &values[RENDER_VAR];
                if pass.target.is_none() {
                    assert_eq!(&pass.values[RENDER_VAR], expected);
                } else {
                    for (index, value) in pass.values[RENDER_VAR].iter().enumerate() {
                        let axis = index % 2;
                        let expected =
                            expected[index] * bloom.canvas[axis] as f32 / canvas[axis] as f32;
                        assert!((value - expected).abs() <= f32::EPSILON);
                    }
                }
            }
        }
    }
}
