use super::*;

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires an isolated Vulkan device"]
fn image_depth_pipeline() -> anyhow::Result<()> {
    let shared = crate::shared::test_device();
    let renderer = vk::Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        4,
        4,
    )?;
    let source = BaseSource {
        name: "depth image",
        resolution: [4.0; 4],
        slot: 0,
        skinned: false,
        prelit: false,
        state: paper_scene::effects::material::ImageState {
            test: paper_scene::effects::material::DepthMode::Enabled,
            write: paper_scene::effects::material::DepthMode::Enabled,
            cull: paper_scene::effects::material::CullMode::Normal,
        },
    };
    let mut vertex = paper_scene::shader::translate(
        "attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}",
        paper_scene::shader::Stage::Vertex,
        &BTreeMap::new(),
    );
    let mut fragment = paper_scene::shader::translate(
        "void main(){gl_FragColor=vec4(1.0);}",
        paper_scene::shader::Stage::Fragment,
        &BTreeMap::new(),
    );
    paper_scene::shader::unify_uniforms(&mut vertex, &mut fragment);
    let pass = paper_scene::effects::EffectPass {
        blend: paper_scene::effects::PassBlend::Copy,
        property_source: None,
        user_uniforms: std::collections::BTreeSet::new(),
        name: "depth image".into(),
        vertex,
        geometry: None,
        fragment,
        hlsl: None,
        textures: Vec::new(),
        values: BTreeMap::new(),
        target: None,
        binds: Vec::new(),
    };
    let spec = source.pipeline(&renderer, &pass);
    assert!(spec.depth_attachment);
    assert_eq!((spec.depth.test, spec.depth.write, spec.depth.cull), (true, true, true));
    assert!(spec.depth.order == vk::DepthOrder::Greater);
    assert!(spec.geometry == vk::EffectGeometry::Quad);
    Ok(())
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires packed depth probes and isolated Vulkan"]
fn image_depth_pixels() -> anyhow::Result<()> {
    const WIDTH: usize = 1920;
    const HEIGHT: u32 = 1080;
    const CENTER: (usize, usize) = (960, 540);
    const PIXEL_BYTES: usize = 4;
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_IMAGE_DEPTH_PROBES").unwrap());
    let reference =
        std::path::PathBuf::from(std::env::var_os("SKWD_IMAGE_DEPTH_REFERENCE").unwrap());
    let shared = crate::shared::test_device();
    let mut allocations = Vec::new();
    for (variant, expected, depth) in [
        ("depth_nocull", [255, 0, 0], true),
        ("depth_off", [0, 0, 255], false),
        ("depth_on", [255, 0, 0], true),
        ("cull_only", [0, 0, 255], false),
    ] {
        let dir = root.join(variant);
        let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
        assert!(model.skipped.is_empty(), "{variant}: {:?}", model.skipped);
        let mut group = crate::app::scene::build_group(
            &shared,
            &mut model,
            true,
            &[(WIDTH as u32, HEIGHT)],
            paper_geom::FillMode::Fill,
        )?;
        assert_eq!(group.target.has_depth(), depth, "{variant}");
        allocations.push((variant, group.target.allocation_bytes));
        group.compose(1.0, 1.0 / 60.0)?;
        let (width, height, pixels) = group.read_canvas()?;
        assert_eq!((width, height), (WIDTH as u32, HEIGHT));
        let index = (CENTER.1 * WIDTH + CENTER.0) * PIXEL_BYTES;
        let actual: [u8; 3] = pixels[index..index + expected.len()].try_into().unwrap();
        let rendered = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        rendered.save(dir.join("paper.png"))?;
        assert_eq!(actual, expected, "{variant}");
        let native = image::open(reference.join(format!("{variant}.png")))?.into_rgb8();
        assert_eq!(actual, native.get_pixel(CENTER.0 as u32, CENTER.1 as u32).0);
        assert_eq!(rendered.dimensions(), native.dimensions());
        assert!(
            rendered
                .pixels()
                .zip(native.pixels())
                .all(|(actual, expected)| actual.0[..3] == expected.0),
            "{variant}"
        );
        group.destroy();
    }
    assert!(allocations[0].1 > allocations[1].1);
    assert_eq!(allocations[0].1, allocations[2].1);
    assert_eq!(allocations[1].1, allocations[3].1);
    std::fs::write(root.join("allocations.json"), serde_json::to_vec_pretty(&allocations)?)?;
    Ok(())
}

fn matrix_meta(source: &str) -> PassMeta {
    let mut vertex = paper_scene::shader::translate(
        source,
        paper_scene::shader::Stage::Vertex,
        &BTreeMap::new(),
    );
    let mut fragment = paper_scene::shader::translate(
        "void main(){gl_FragColor=vec4(1.0);}",
        paper_scene::shader::Stage::Fragment,
        &BTreeMap::new(),
    );
    paper_scene::shader::unify_uniforms(&mut vertex, &mut fragment);
    let pass = paper_scene::effects::EffectPass {
        blend: paper_scene::effects::PassBlend::Copy,
        property_source: None,
        user_uniforms: std::collections::BTreeSet::new(),
        name: "camera inverse".into(),
        vertex,
        geometry: None,
        fragment,
        hlsl: None,
        textures: Vec::new(),
        values: BTreeMap::new(),
        target: None,
        binds: Vec::new(),
    };
    PassMeta::of(&pass)
}

#[test]
fn camera_inverse_uniform_bytes() {
    let meta = matrix_meta(
        "uniform mat4 g_ModelViewProjectionMatrix;\nuniform mat4 g_ModelViewProjectionMatrixInverse;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}",
    );
    let camera = paper_scene::model::Camera {
        eye: [1.0, 2.0, -5.0],
        center: [0.0; 3],
        up: [0.3, 1.0, 0.2],
        fov: 60.0,
        near: 0.1,
        far: 100.0,
    };
    let model =
        paper_scene::matrix::model_matrix([1.0, 0.5, 2.0], [1.5, 0.5, 1.0], [0.0, 0.0, 0.2]);
    for d3d_clip in [false, true] {
        let (view, ..) = authored_perspective(camera, (1280.0, 720.0), d3d_clip);
        let projection = mat4_mul(&view, &model);
        let mut values = BTreeMap::from([(FX_PROJECTION_INVERSE.into(), vec![42.0; 16])]);
        camera_uniforms(&meta, &mut values, &view, &projection, None).unwrap();
        values.insert(FX_MODEL_PROJECTION.into(), projection.to_vec());
        let clock = FrameClock { time: 0.0, dt: 0.0, daytime: 0.5 };
        let bytes =
            meta.uniform_bytes_with(clock, None, 1280, 720, (1280, 720), &[], &values, d3d_clip);
        let offset = meta.matrix_offset(FX_PROJECTION_INVERSE).unwrap();
        let inverse: Mat4 = std::array::from_fn(|index| {
            let start = offset + index * std::mem::size_of::<f32>();
            f32::from_le_bytes(bytes[start..start + std::mem::size_of::<f32>()].try_into().unwrap())
        });
        for (index, value) in mat4_mul(&projection, &inverse).into_iter().enumerate() {
            assert!((value - if index % 5 == 0 { 1.0 } else { 0.0 }).abs() < 0.00001);
        }
    }
}

#[test]
fn puppet_matrix_uniforms() {
    let meta = matrix_meta(
        "uniform mat4 g_ModelMatrix;\nuniform mat4 g_ViewProjectionMatrix;\nuniform mat4 g_ModelViewProjectionMatrix;\nuniform vec3 g_EyePosition;\nvoid main(){gl_Position=vec4(1.0);}",
    );
    let viewport = vk::SceneViewport::cover([3840.0, 2160.0], [1280, 720]).unwrap();
    let view = super::super::mesh::orthographic_view(viewport);
    let eye = super::super::orthographic_eye((3840.0, 2160.0));
    let point =
        [f32::from_bits(0x441a_a21f), f32::from_bits(0xc3a9_0dbc), f32::from_bits(0x4096_6c86)];
    for (origin, world_x, clip_x) in
        [(1920.0, 0x4611_f997, 0x4077_69cd), (1952.0, 0x4612_7997, 0x4078_7ade)]
    {
        let pose = LayerPose {
            geometry: Some(paper_scene::model::Geometry {
                spatial: None,
                depth_scale: 1.0,
                transform: paper_scene::affine::Affine {
                    a: 12.0,
                    b: 0.0,
                    c: 0.0,
                    d: 12.0,
                    x: origin,
                    y: 1080.0,
                },
                size: [728.0, 769.0],
                offset: [0.0; 2],
            }),
            rect: [origin, 1080.0, 8736.0, 9228.0],
            scale: [12.0; 2],
            angle: 0.0,
            depth: 0.0,
        };
        let mut values = pose_uniforms(pose, (3840.0, 2160.0), false);
        let model: Mat4 = values[FX_MODEL_MATRIX].as_slice().try_into().unwrap();
        let projection = mat4_mul(&view, &model);
        values.insert(
            super::super::FX_VIEW_PROJECTION.into(),
            scene_view_projection((3840.0, 2160.0)).to_vec(),
        );
        values.insert(FX_MODEL_PROJECTION.into(), projection.to_vec());
        camera_uniforms(&meta, &mut values, &view, &projection, Some(eye)).unwrap();
        let bytes = meta.uniform_bytes_with(
            FrameClock::default(),
            None,
            3840,
            2160,
            (1280, 720),
            &[],
            &values,
            true,
        );
        let read = |name: &str| -> Mat4 {
            let offset = meta.matrix_offset(name).unwrap();
            std::array::from_fn(|index| {
                let start = offset + index * size_of::<f32>();
                f32::from_ne_bytes(bytes[start..start + size_of::<f32>()].try_into().unwrap())
            })
        };
        let supplied_model = read(FX_MODEL_MATRIX);
        let supplied_view = read(super::super::FX_VIEW_PROJECTION);
        let supplied_projection = read(FX_MODEL_PROJECTION);
        assert_eq!(supplied_model, model);
        assert_eq!(supplied_view, view);
        assert_eq!(mat4_mul(&supplied_view, &supplied_model), supplied_projection);
        assert_eq!(values[super::super::mesh::EYE_POSITION], eye);
        let world = paper_scene::puppet::BonePose::Spatial(supplied_model).point(point);
        let clip = paper_scene::puppet::BonePose::Spatial(supplied_projection).point(point);
        assert_eq!(world.map(f32::to_bits), [world_x, 0xc53a_149a, 0x4096_6c86]);
        assert_eq!(clip.map(f32::to_bits), [clip_x, 0xc070_6e8d, 0x3f00_4d04]);
        assert_ne!(paper_scene::puppet::BonePose::Spatial(supplied_view).point(world), clip);
    }
}

#[test]
fn authored_puppet_clock() {
    let mesh = paper_scene::puppet::parse(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../paper-scene/src/fixtures/a02/clock-step.mdl"
    )))
    .unwrap();
    let layers = vec![paper_scene::puppet::AnimationLayer {
        visible: true,
        id: 220,
        rate: 2.0,
        blend: 1.0,
        additive: false,
    }];
    let initial = paper_scene::puppet::authored(&mesh, &layers, Some(0.0)).unwrap();
    let expected = paper_scene::puppet::palette_layers(&mesh, &[(layers[0], 2.0)]).unwrap();
    let playback =
        paper_scene::puppet::playback::Playback::authored("182", &mesh, &layers).unwrap();
    let mut puppet = Puppet {
        id: "182".into(),
        model: Some(paper_scene::model::Puppet {
            cull: false,
            cpu: false,
            planar_owner: true,
            world: paper_scene::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]),
            mesh,
            layers,
            size: (1.0, 1.0),
            mask_textures: Vec::new(),
        }),
        sampled: Vec::new(),
        bone_revision: 0,
        clock: Some(Clock { playback, time: 0.0, dt: 0.0, spring: None, world: Vec::new() }),
        order: None,
        reordered: None,
    };
    let mut palette = paper_scene::effects::material::Palette {
        bones: initial.bones.clone(),
        morph: None,
        channels: Vec::new(),
        alpha: None,
        uniform: "g_Bones[1]".into(),
    };
    assert!(puppet.sample_authored(FrameClock::now(0.0, 0.0), &mut palette).unwrap());
    assert_eq!(palette.bones, initial.bones);
    assert!(puppet.sample_authored(FrameClock::now(1.0, 0.0), &mut palette).unwrap());
    assert_eq!(palette.bones, expected);
    assert_ne!(palette.bones, initial.bones);
    assert!(puppet.sample_authored(FrameClock::now(1.0, 0.0), &mut palette).unwrap());
    assert_eq!(palette.bones, expected);
}

#[test]
fn authored_order_rewrites() {
    let mesh = paper_scene::puppet::parse(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../paper-scene/src/fixtures/draw-order.mdl"
    )))
    .unwrap();
    let layers = vec![paper_scene::puppet::AnimationLayer {
        visible: true,
        id: mesh.animations[0].id,
        rate: 1.0,
        blend: 1.0,
        additive: false,
    }];
    let initial = paper_scene::puppet::authored(&mesh, &layers, Some(0.0)).unwrap();
    let playback = paper_scene::puppet::playback::Playback::authored("9", &mesh, &layers).unwrap();
    let order = paper_scene::puppet::draw_order::Sorter::of(&mesh);
    let mut puppet = Puppet {
        id: "9".into(),
        model: Some(paper_scene::model::Puppet {
            cull: false,
            cpu: false,
            planar_owner: true,
            world: paper_scene::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]),
            mesh,
            layers,
            size: (1.0, 1.0),
            mask_textures: Vec::new(),
        }),
        sampled: Vec::new(),
        bone_revision: 0,
        clock: Some(Clock { playback, time: 0.0, dt: 0.0, spring: None, world: Vec::new() }),
        order,
        reordered: None,
    };
    let mut palette = paper_scene::effects::material::Palette {
        bones: initial.bones.clone(),
        morph: None,
        channels: Vec::new(),
        alpha: None,
        uniform: "g_Bones[1]".into(),
    };
    assert!(puppet.sample_authored(FrameClock::now(0.0, 0.0), &mut palette).unwrap());
    let expected: Vec<u8> = [1_u16, 2, 0].iter().flat_map(|index| index.to_ne_bytes()).collect();
    assert_eq!(puppet.reordered.as_ref(), Some(&expected));
    assert!(puppet.sample_authored(FrameClock::now(0.5 / 30.0, 0.0), &mut palette).unwrap());
    assert_eq!(puppet.reordered, Some(expected));
}

#[test]
fn puppet_clock_lifecycle() {
    use paper_scene::puppet::playback::{Clip, Playback};
    use paper_scene::script::{Load, SceneScripts};
    use serde_json::json;
    let mesh = paper_scene::puppet::parse(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../paper-scene/src/fixtures/a02/clock-step.mdl"
    )))
    .unwrap();
    let mut model = paper_scene::model::Puppet {
        cull: false,
        cpu: false,
        planar_owner: true,
        world: paper_scene::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]),
        mesh,
        size: (1.0, 1.0),
        mask_textures: Vec::new(),
        layers: vec![paper_scene::puppet::AnimationLayer {
            visible: true,
            id: 220,
            rate: 0.0,
            blend: 1.0,
            additive: false,
        }],
    };
    let initial = paper_scene::puppet::authored(&model.mesh, &model.layers, None).unwrap();
    paper_scene::puppet::retain_animation(&mut model.mesh, model.layers[0].id).unwrap();
    let mut palette = paper_scene::effects::material::Palette {
        bones: initial.bones.clone(),
        morph: None,
        channels: Vec::new(),
        alpha: None,
        uniform: "g_Bones[1]".into(),
    };
    let mut puppet = Puppet {
        id: "182".into(),
        model: Some(model),
        sampled: Vec::new(),
        bone_revision: 0,
        clock: None,
        order: None,
        reordered: None,
    };
    let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":
        "export function init(){thisLayer.getAnimationLayer(0).setFrame(120);} export function update(){if(engine.runtime>=1)thisScene.destroyLayer(thisLayer);else thisLayer.getAnimationLayer(0).visible=engine.runtime>=0.5;return ''; }"}}]});
    let playback = Playback::new(vec![Clip {
        slot: 0,
        object: "182".into(),
        name: "Animación 1".into(),
        animation: 220,
        fps: 60.0,
        length: 1380.0,
        rate: 0.0,
    }]);
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &BTreeMap::new(),
        &json!({}),
        Load { puppets: Some(playback), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    puppet.id = "missing".into();
    assert!(puppet.sample(&host, &mut palette).is_err());
    puppet.id = "182".into();
    palette.bones = vec![initial.bones[0], initial.bones[0]].into();
    assert!(puppet.sample(&host, &mut palette).is_err());
    assert_eq!(palette.bones.len(), 2);
    palette.bones = initial.bones.clone();
    assert!(puppet.sample(&host, &mut palette).unwrap());
    assert_ne!(palette.bones, initial.bones);
    let held = palette.bones.clone();
    host.tick(0.25, 0.25, [0.5; 2]).unwrap();
    assert!(puppet.sample(&host, &mut palette).unwrap());
    let retained = puppet.model.as_ref().unwrap();
    assert_eq!(palette.bones, paper_scene::puppet::rest_palette(&retained.mesh).unwrap());
    assert_ne!(palette.bones, held);
    host.tick(0.5, 0.25, [0.5; 2]).unwrap();
    assert!(puppet.sample(&host, &mut palette).unwrap());
    assert_eq!(palette.bones, held);

    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["visible"], false);
    assert!(host.take_destroyed_images().is_empty());
    assert!(!puppet.sample(&host, &mut palette).unwrap());
    assert!(puppet.model.is_none());
    host.tick(2.0, 1.0, [0.5; 2]).unwrap();
    assert!(!puppet.sample(&host, &mut palette).unwrap());
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn layered_puppet_palette() {
    use paper_scene::puppet::playback::{Clip, Playback};
    use paper_scene::script::{Load, SceneScripts};
    use serde_json::json;
    let mesh = paper_scene::puppet::parse(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../paper-scene/src/fixtures/a02/clock-step.mdl"
    )))
    .unwrap();
    let selected: Vec<_> =
        mesh.animations.iter().take(2).map(|clip| (clip.id, clip.fps, clip.duration)).collect();
    assert_eq!(selected.len(), 2);
    let layers = selected
        .iter()
        .map(|(id, _, _)| paper_scene::puppet::AnimationLayer {
            visible: true,
            id: *id,
            rate: 0.0,
            blend: 0.5,
            additive: false,
        })
        .collect::<Vec<_>>();
    let initial = paper_scene::puppet::authored(&mesh, &layers, None).unwrap();
    let mut model = paper_scene::model::Puppet {
        cull: false,
        cpu: false,
        planar_owner: true,
        world: paper_scene::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]),
        mesh,
        size: (1.0, 1.0),
        mask_textures: Vec::new(),
        layers,
    };
    paper_scene::puppet::retain_animations(
        &mut model.mesh,
        &selected.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
    )
    .unwrap();
    let mut palette = paper_scene::effects::material::Palette {
        bones: initial.bones,
        morph: None,
        channels: Vec::new(),
        alpha: None,
        uniform: "g_Bones[1]".into(),
    };
    let mut puppet = Puppet {
        id: "182".into(),
        model: Some(model),
        sampled: Vec::new(),
        bone_revision: 0,
        clock: None,
        order: None,
        reordered: None,
    };
    let clips = selected
        .iter()
        .enumerate()
        .map(|(index, (id, fps, duration))| Clip {
            slot: 0,
            object: "182".into(),
            name: format!("clip{index}"),
            animation: *id,
            fps: *fps,
            length: *duration as f32,
            rate: 0.0,
        })
        .collect();
    let playback = Playback::new(clips);
    let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":
        "export function init(){thisLayer.getAnimationLayer(0).setFrame(36);thisLayer.getAnimationLayer(0).blend=0.5;thisLayer.getAnimationLayer(1).setFrame(120);thisLayer.getAnimationLayer(1).blend=0.25;}"}}]});
    let host = SceneScripts::load_with_storage(
        &mut scene,
        &std::collections::BTreeMap::new(),
        &json!({}),
        Load { puppets: Some(playback), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert!(puppet.sample(&host, &mut palette).unwrap());
    let samples = host
        .puppet_states("182")
        .into_iter()
        .map(|state| {
            let paper_scene::puppet::playback::Sample::Frame(seconds) = state.sample else {
                panic!("live layer")
            };
            (state.layer, seconds)
        })
        .collect::<Vec<_>>();
    let expected =
        paper_scene::puppet::palette_layers(&puppet.model.as_ref().unwrap().mesh, &samples)
            .unwrap();
    assert_eq!(palette.bones, expected);
    assert_eq!(samples[1].0.blend, 0.25);
}

#[test]
fn scripted_bone_palette() {
    use paper_scene::puppet::playback::{Clip, Playback};
    use paper_scene::script::{Load, SceneScripts};
    use serde_json::json;
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../paper-scene/src/fixtures/a02/clock-step.mdl"
    ));
    let mesh = paper_scene::puppet::parse(bytes).unwrap();
    let animation = &mesh.animations[0];
    let clip = Clip {
        slot: 0,
        object: "182".into(),
        name: "idle".into(),
        animation: animation.id,
        fps: animation.fps,
        length: animation.duration as f32,
        rate: 0.0,
    };
    let layer = paper_scene::puppet::AnimationLayer {
        visible: true,
        id: clip.animation,
        rate: 0.0,
        blend: 1.0,
        additive: false,
    };
    let initial = paper_scene::puppet::authored(&mesh, &[layer], None).unwrap();
    let mut model = paper_scene::model::Puppet {
        cull: false,
        cpu: false,
        planar_owner: true,
        world: paper_scene::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]),
        mesh,
        size: (1.0, 1.0),
        mask_textures: Vec::new(),
        layers: vec![layer],
    };
    paper_scene::puppet::retain_animation(&mut model.mesh, clip.animation).unwrap();
    let mut palette = paper_scene::effects::material::Palette {
        bones: initial.bones.clone(),
        morph: None,
        channels: Vec::new(),
        alpha: None,
        uniform: "g_Bones[1]".into(),
    };
    let mut puppet = Puppet {
        id: "182".into(),
        model: Some(model),
        sampled: Vec::new(),
        bone_revision: 0,
        clock: None,
        order: None,
        reordered: None,
    };
    let mut playback = Playback::new(vec![clip]);
    playback.install_rig(0, "182".into(), paper_scene::puppet::parse(bytes).unwrap());
    let script = "let first=true;export function update(){if(!first)return;first=false;const bone=thisLayer.getBoneTransform(0);bone.translation(bone.translation().add(new Vec3(4,0,0)));thisLayer.setBoneTransform(0,bone);}";
    let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":script}}]});
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &BTreeMap::new(),
        &json!({}),
        Load { puppets: Some(playback), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert!(puppet.sample(&host, &mut palette).unwrap());
    let Some(paper_scene::puppet::BonePose::Spatial(before)) = initial.bones[0] else {
        panic!("spatial bone")
    };
    let Some(paper_scene::puppet::BonePose::Spatial(after)) = palette.bones[0] else {
        panic!("spatial bone")
    };
    assert!((after[12] - before[12] - 4.0).abs() < 0.001);
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert!(puppet.sample(&host, &mut palette).unwrap());
    assert_eq!(palette.bones[0], Some(paper_scene::puppet::BonePose::Spatial(before)));
}

#[test]
#[ignore = "requires authored puppet probe packages and Vulkan"]
fn authored_puppet_bone_pixels() -> anyhow::Result<()> {
    const BRIGHT_CHANNEL: u8 = 128;
    const MIN_BONE_SHIFT_PIXELS: u64 = 200;
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_PUPPET_PROBE_DIR").unwrap());
    let shared = crate::shared::test_device();
    let mut centers = Vec::new();
    let mut bone_x = Vec::new();
    for variant in ["baseline", "bone_shift"] {
        let dir = root.join(variant);
        let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
        let mut group = crate::app::scene::build_group(
            &shared,
            &mut model,
            true,
            &[(1920, 1080)],
            paper_geom::FillMode::Fill,
        )?;
        group.compose(1.0, 1.0 / 60.0)?;
        let Some(paper_scene::puppet::BonePose::Spatial(matrix)) = group.puppets[0].pose.bones[0]
        else {
            panic!("spatial puppet bone missing");
        };
        bone_x.push(matrix[12]);
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        if let Some(output) = std::env::var_os("SKWD_PUPPET_PROBE_OUTPUT") {
            image.save(std::path::PathBuf::from(output).join(format!("{variant}.png")))?;
        }
        let (sum, count) = image
            .enumerate_pixels()
            .filter(|(_, y, pixel)| {
                *y < height / 2 && pixel[0] > BRIGHT_CHANNEL && pixel[1] > BRIGHT_CHANNEL
            })
            .fold((0_u64, 0_u64), |(sum, count), (x, _, _)| (sum + u64::from(x), count + 1));
        assert!(count > 0, "{variant}: no upper paw");
        centers.push(sum / count);
        group.destroy();
    }
    assert!((bone_x[1] - bone_x[0] - 180.0).abs() < 0.001, "{bone_x:?}");
    assert!(centers[1] > centers[0] + MIN_BONE_SHIFT_PIXELS, "{centers:?}");
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn history_mips_are_private() -> anyhow::Result<()> {
    let mut model = crate::app::scene::tests::mesh_sampler_model(
        br#"{"general":{"orthogonalprojection":{"width":16,"height":16}},"objects":[{"id":1,"image":"models/sample.json","origin":"8 8 0","size":"16 16","instance":{"textures":["textures/white","_rt_MipMappedFrameBuffer"],"constantshadervalues":{"tint":"1 0 0"}}},{"id":2,"image":"models/sample.json","origin":"8 8 0","size":"16 16"}]}"#,
        br#"{"passes":[{"shader":"sample","textures":["textures/white","textures/white"],"blending":"additive","constantshadervalues":{"tint":"0 1 0","mipinfo":0.25}}]}"#,
        b"uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nuniform vec3 g_Tint; // {\"material\":\"tint\"}\nuniform float g_Texture1MipMapInfo; // {\"material\":\"mipinfo\"}\nvoid main(){gl_FragColor=vec4(g_Tint*g_Texture1MipMapInfo,1.0)+0.0*texSample2D(g_Texture0,vec2(0.5))+0.0*texSample2D(g_Texture1,vec2(0.5));}",
    );
    let shared = crate::shared::test_device();
    let mut group = crate::app::scene::build_group(
        &shared,
        &mut model,
        true,
        &[(16, 16)],
        paper_geom::FillMode::Fill,
    )?;
    let captured = group.read_canvas();
    group.destroy();
    let (width, height, rgba) = captured?;
    assert_eq!((width, height), (16, 16));
    let center = ((height / 2 * width + width / 2) * 4) as usize;
    assert_eq!(&rgba[center..center + 4], &[255, 64, 0, 255]);
    Ok(())
}

#[test]
#[ignore = "requires authored morph packages and Vulkan"]
fn authored_morph_pixels() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_MORPH_PROBE_DIR").unwrap());
    let shared = crate::shared::test_device();
    let mut captures = Vec::new();
    for variant in ["rest", "shape"] {
        let dir = root.join(variant);
        let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert_eq!(model.layers.len(), 1);
        assert!(model.layers[0].material.as_ref().is_some_and(|material| material.skin.is_some()));
        let mut group = crate::app::scene::build_group(
            &shared,
            &mut model,
            true,
            &[(1920, 1080)],
            paper_geom::FillMode::Fill,
        )?;
        group.compose(1.0, 1.0 / 60.0)?;
        let host = group.scripts.as_ref().unwrap().host.as_ref().unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        let weight = if variant == "shape" { 1.0 } else { 0.0 };
        assert_eq!(host.puppet_morph_weights("188"), Some(vec![weight]));
        let palette = group.materials[0].palette.as_ref().unwrap();
        assert_eq!(palette.morph.as_ref().unwrap().weights[1], weight);
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        assert_ne!(image.get_pixel(width * 3 / 4, height / 2), image.get_pixel(width - 1, 0));
        if let Some(reference) = std::env::var_os("SKWD_MORPH_REFERENCE_DIR") {
            let native =
                image::open(std::path::PathBuf::from(reference).join(format!("{variant}.png")))?
                    .into_rgba8();
            assert_eq!(image.dimensions(), native.dimensions());
            assert!(
                image
                    .as_raw()
                    .iter()
                    .zip(native.as_raw())
                    .all(|(paper, native)| paper.abs_diff(*native) <= 1)
            );
        }
        if let Some(output) = std::env::var_os("SKWD_MORPH_PROBE_OUTPUT") {
            image.save(std::path::PathBuf::from(output).join(format!("{variant}.png")))?;
        }
        captures.push(image);
        group.destroy();
    }
    assert!(captures[0] != captures[1]);
    Ok(())
}

#[test]
#[ignore = "requires authored morph packages and Vulkan"]
fn authored_morph_animation() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_MORPH_PROBE_DIR").unwrap());
    let dir = root.join("animated");
    let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert!(model.scripts.is_none());
    let shared = crate::shared::test_device();
    let mut group = crate::app::scene::build_group(
        &shared,
        &mut model,
        true,
        &[(1920, 1080)],
        paper_geom::FillMode::Fill,
    )?;
    assert!(group.animated());
    assert!(!group.frozen);
    let weights = group.materials[0].palette.as_ref().unwrap().morph.as_ref().unwrap().weights;
    let before = group.read_canvas()?.2;
    group.compose(2.0, 2.0)?;
    assert_ne!(
        group.materials[0].palette.as_ref().unwrap().morph.as_ref().unwrap().weights,
        weights
    );
    let (width, height, after) = group.read_canvas()?;
    assert!(before != after);
    if let Some(output) = std::env::var_os("SKWD_MORPH_PROBE_OUTPUT") {
        image::RgbaImage::from_raw(width, height, after)
            .unwrap()
            .save(std::path::PathBuf::from(output).join("animated.png"))?;
    }
    group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires a CPU-routed morph scene and Vulkan"]
fn cpu_morph_scene() -> anyhow::Result<()> {
    let dir = std::path::PathBuf::from(std::env::var_os("SKWD_CPU_MORPH_SCENE").unwrap());
    let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
    let shared = crate::shared::test_device();
    let mut group = crate::app::scene::build_group(
        &shared,
        &mut model,
        true,
        &[(1920, 1080)],
        paper_geom::FillMode::Fill,
    )?;
    group.compose(1.0, 1.0 / 60.0)?;
    let puppet = group.puppets.iter().find(|puppet| puppet.puppet.mesh.morph.is_some()).unwrap();
    let skinned = paper_scene::puppet::skin_samples(&puppet.puppet.mesh, &puppet.samples);
    assert_ne!(puppet.pose.positions, skinned.positions);
    group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires authored morph modifier packages and Vulkan"]
fn authored_morph_modifiers() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_MORPH_MODIFIER_DIR").unwrap());
    let shared = crate::shared::test_device();
    let mut captures = Vec::new();
    for variant in ["off", "point", "axis", "bone"] {
        let dir = root.join(variant);
        let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let mut group = crate::app::scene::build_group(
            &shared,
            &mut model,
            true,
            &[(1920, 1080)],
            paper_geom::FillMode::Fill,
        )?;
        group.compose(1.0, 1.0 / 60.0)?;
        let host = group.scripts.as_ref().unwrap().host.as_ref().unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        let morph = group.materials[0].palette.as_ref().unwrap().morph.as_ref().unwrap();
        assert_eq!(morph.weights[1], 1.0);
        assert!(morph.modifiers.is_some());
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        image.save(root.join(format!("{variant}.png")))?;
        if let Some(reference) = std::env::var_os("SKWD_MORPH_MODIFIER_REFERENCE") {
            let native =
                image::open(std::path::PathBuf::from(reference).join(format!("{variant}.png")))?
                    .into_rgba8();
            assert_eq!(image.dimensions(), native.dimensions());
            assert!(
                image
                    .as_raw()
                    .iter()
                    .zip(native.as_raw())
                    .all(|(paper, native)| paper.abs_diff(*native) <= 1)
            );
        }
        captures.push(image);
        group.destroy();
    }
    assert!(captures.windows(2).all(|pair| pair[0] != pair[1]));
    Ok(())
}

#[test]
#[ignore = "requires authored channel packages and Vulkan"]
fn authored_texture_channel_pixels() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_PUPPET_CHANNEL_DIR").unwrap());
    let shared = crate::shared::test_device();
    let mut captures = Vec::new();
    for (variant, weight, rounding) in [("zero", 0.0, 1), ("unit", 1.0, 1), ("full", 2.0, 2)] {
        let dir = root.join(variant);
        let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let palette = &model.layers[0].material.as_ref().unwrap().skin.as_ref().unwrap().palette;
        assert_eq!(palette.channels[0], weight);
        let mut group = crate::app::scene::build_group(
            &shared,
            &mut model,
            true,
            &[(1920, 1080)],
            paper_geom::FillMode::Fill,
        )?;
        group.compose(1.0, 1.0 / 60.0)?;
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        image.save(root.join(format!("{variant}-paper.png")))?;
        if let Some(reference) = std::env::var_os("SKWD_CHANNEL_REFERENCE_DIR") {
            let native =
                image::open(std::path::PathBuf::from(reference).join(format!("{variant}.png")))?
                    .into_rgba8();
            assert_eq!(image.dimensions(), native.dimensions());
            assert!(
                image
                    .as_raw()
                    .iter()
                    .zip(native.as_raw())
                    .all(|(paper, native)| paper.abs_diff(*native) <= rounding)
            );
        }
        captures.push(image);
        group.destroy();
    }
    assert!(captures[0] != captures[1]);
    Ok(())
}

#[test]
#[ignore = "requires authored bone alpha packages, Vulkan and SKWD_PAPER_SCENE_MSAA=0"]
fn authored_bone_alpha_pixels() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_BONE_ALPHA_DIR").unwrap());
    let shared = crate::shared::test_device();
    let mut captures = Vec::new();
    for (variant, weight) in
        [("full", 1.0), ("mid", 0.721_917_3), ("faded", 0.0), ("additive", 0.402_487_78)]
    {
        let dir = root.join(variant);
        let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let mut group = crate::app::scene::build_group(
            &shared,
            &mut model,
            true,
            &[(1920, 1080)],
            paper_geom::FillMode::Fill,
        )?;
        group.compose(1.0, 1.0 / 60.0)?;
        let host = group.scripts.as_ref().unwrap().host.as_ref().unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        let palette = group.materials[0].palette.as_ref().unwrap();
        let alpha = palette.alpha.as_ref().unwrap();
        assert_eq!(alpha.weights.len(), 1);
        assert!((alpha.weights[0] - weight).abs() < 0.000_001, "{variant}: {:?}", alpha.weights);
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        image.save(root.join(format!("{variant}-paper.png")))?;
        if let Some(reference) = std::env::var_os("SKWD_BONE_ALPHA_REFERENCE") {
            let native =
                image::open(std::path::PathBuf::from(reference).join(format!("{variant}.png")))?
                    .into_rgba8();
            assert_eq!(image.dimensions(), native.dimensions());
            assert!(
                image
                    .as_raw()
                    .iter()
                    .zip(native.as_raw())
                    .all(|(paper, native)| paper.abs_diff(*native) <= 1),
                "{variant}"
            );
        }
        captures.push(image);
        group.destroy();
    }
    assert!(captures.windows(2).all(|pair| pair[0] != pair[1]));
    Ok(())
}

#[test]
#[ignore = "requires authored multi-bone alpha packages, Vulkan and SKWD_PAPER_SCENE_MSAA=0"]
fn weighted_bone_alpha_pixels() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_BONE_ALPHA_DIR").unwrap());
    let shared = crate::shared::test_device();
    let mut captures = Vec::new();
    for variant in ["opaque", "weighted"] {
        let dir = root.join(variant);
        let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
        let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let palette = &model.layers[0].material.as_ref().unwrap().skin.as_ref().unwrap().palette;
        let weights = &palette.alpha.as_ref().unwrap().weights;
        assert_eq!(weights.len(), 3);
        for (index, weight) in weights.iter().enumerate() {
            let expected = if variant == "opaque" { 1.0 } else { index as f32 / 3.0 };
            assert!((weight - expected).abs() < 0.000_001);
        }
        let mut group = crate::app::scene::build_group(
            &shared,
            &mut model,
            true,
            &[(1920, 1080)],
            paper_geom::FillMode::Fill,
        )?;
        group.compose(1.0, 1.0 / 60.0)?;
        let (width, height, pixels) = group.read_canvas()?;
        let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        image.save(root.join(format!("{variant}-paper.png")))?;
        if let Some(reference) = std::env::var_os("SKWD_BONE_ALPHA_REFERENCE") {
            let native =
                image::open(std::path::PathBuf::from(reference).join(format!("{variant}.png")))?
                    .into_rgba8();
            assert_eq!(image.dimensions(), native.dimensions());
            assert!(
                image
                    .as_raw()
                    .iter()
                    .zip(native.as_raw())
                    .all(|(paper, native)| paper.abs_diff(*native) <= 1),
                "{variant}"
            );
        }
        captures.push(image);
        group.destroy();
    }
    assert!(captures[0] != captures[1]);
    Ok(())
}

#[test]
#[ignore = "requires authored puppet composition packages and Vulkan"]
fn puppet_composition_pixels() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_PUPPET_COMPOSITION").unwrap());
    let shared = crate::shared::test_device();
    for variant in [
        "tint",
        "pair",
        "transform",
        "parent",
        "effect",
        "effect-mid",
        "mask",
        "mask-shift",
        "tone",
    ] {
        let (image, _) = composition_frame(&shared, &root.join(variant))?;
        image.save(root.join(format!("{variant}-paper.png")))?;
        let native = image::open(root.join(format!("{variant}-native/at12s.png")))?.into_rgba8();
        assert_eq!(image.dimensions(), native.dimensions());
        assert!(
            image
                .as_raw()
                .iter()
                .zip(native.as_raw())
                .all(|(paper, native)| paper.abs_diff(*native) <= 1),
            "{variant}"
        );
    }
    Ok(())
}

fn composition_frame(
    shared: &crate::shared::SharedDevice,
    dir: &std::path::Path,
) -> anyhow::Result<(image::RgbaImage, ash::vk::SampleCountFlags)> {
    let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, dir)?;
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let count = model.layers.len();
    let mut group = crate::app::scene::build_group(
        shared,
        &mut model,
        true,
        &[(1920, 1080)],
        paper_geom::FillMode::Fill,
    )?;
    assert_eq!(group.materials.len(), count);
    group.compose(1.0, 1.0 / 60.0)?;
    let host = group.scripts.as_ref().unwrap().host.as_ref().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let (width, height, pixels) = group.read_canvas()?;
    let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
    let samples = group.target.samples;
    group.destroy();
    Ok((image, samples))
}

#[test]
#[ignore = "requires authored puppet composition packages and Vulkan"]
fn additive_puppet_pixels() -> anyhow::Result<()> {
    const PUPPETS: [u32; 4] = [300, 740, 1180, 1620];
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_PUPPET_COMPOSITION").unwrap());
    let shared = crate::shared::test_device();
    let dir = root.join("additive");
    let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let mut group = crate::app::scene::build_group(
        &shared,
        &mut model,
        true,
        &[(1920, 1080)],
        paper_geom::FillMode::Fill,
    )?;
    assert_eq!(group.puppets.len(), PUPPETS.len());
    group.compose(1.0, 1.0 / 60.0)?;
    let (width, height, pixels) = group.read_canvas()?;
    group.destroy();
    let image = image::RgbaImage::from_raw(width, height, pixels).unwrap();
    let native = image::open(root.join("additive-native/at12s.png"))?.into_rgba8();
    let row = image.height() / 2;
    for x in PUPPETS {
        assert_eq!(image.get_pixel(x, row), native.get_pixel(x, row), "{x}");
    }
    Ok(())
}

#[test]
#[ignore = "requires authored puppet composition packages and default MSAA Vulkan"]
fn puppet_effect_msaa() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_PUPPET_COMPOSITION").unwrap());
    let shared = crate::shared::test_device();
    for variant in ["effect", "effect-mid", "mask", "mask-shift", "tone"] {
        let (image, samples) = composition_frame(&shared, &root.join(variant))?;
        assert!(samples == ash::vk::SampleCountFlags::TYPE_1);
        let native = image::open(root.join(format!("{variant}-native/at12s.png")))?.into_rgba8();
        let center = (image.width() / 2, image.height() / 2);
        assert_eq!(
            image.get_pixel(center.0, center.1),
            native.get_pixel(center.0, center.1),
            "{variant}"
        );
        image.save(root.join(format!("{variant}-msaa.png")))?;
    }
    Ok(())
}

#[test]
#[ignore = "requires native puppet depth-scale fixtures and Vulkan"]
fn puppet_depth_scale() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_PUPPET_DEPTH_SCALE").unwrap());
    let shared = crate::shared::test_device();
    for variant in ["unit", "scaled", "parent"] {
        let (image, _) = composition_frame(&shared, &root.join(variant))?;
        let native = image::open(root.join(format!("{variant}-native/at12s.png")))?.into_rgba8();
        image.save(root.join(format!("{variant}-paper.png")))?;
        assert_eq!(image.dimensions(), native.dimensions());
        assert!(
            image.as_raw().iter().zip(native.as_raw()).all(|(a, b)| a.abs_diff(*b) <= 1),
            "{variant}"
        );
    }
    Ok(())
}

#[test]
fn authored_spring_clock() {
    let mut mesh = paper_scene::puppet::parse(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../paper-scene/src/fixtures/a02/clock-step.mdl"
    )))
    .unwrap();
    mesh.springs.push(paper_scene::puppet::spring::Definition {
        bone: 0,
        translation: Some(paper_scene::puppet::spring::Translation {
            stiffness: 8.0,
            friction: 2.0,
            inertia: 0.5,
            maximum: 0.0,
        }),
        rotation: None,
        gravity: None,
    });
    let initial = paper_scene::puppet::authored(&mesh, &[], Some(0.0)).unwrap();
    let clock = Clock {
        playback: paper_scene::puppet::playback::Playback::authored("spring", &mesh, &[]).unwrap(),
        time: 0.0,
        dt: 0.0,
        spring: Some(Box::new(paper_scene::puppet::spring::Simulation::new(
            mesh.bones.len(),
            &mesh.springs,
        ))),
        world: Vec::new(),
    };
    let mut puppet = Puppet {
        id: "spring".into(),
        model: Some(paper_scene::model::Puppet {
            cull: false,
            cpu: false,
            planar_owner: true,
            world: paper_scene::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]),
            mesh,
            layers: Vec::new(),
            size: (1.0, 1.0),
            mask_textures: Vec::new(),
        }),
        sampled: Vec::new(),
        bone_revision: 0,
        clock: Some(clock),
        order: None,
        reordered: None,
    };
    let mut palette = paper_scene::effects::material::Palette {
        bones: initial.bones,
        morph: None,
        channels: Vec::new(),
        alpha: None,
        uniform: "g_Bones[1]".into(),
    };
    assert!(puppet.sample_authored(FrameClock::now(0.0, 0.125), &mut palette).unwrap());
    let origin = palette.bones[0].unwrap().point([0.0; 3]);
    let storage = puppet.clock.as_ref().unwrap().world.as_ptr();
    puppet.model.as_mut().unwrap().world[12] = 8.0;
    assert!(puppet.sample_authored(FrameClock::now(1.0, 0.125), &mut palette).unwrap());
    assert_eq!(palette.bones[0].unwrap().point([0.0; 3])[0] - origin[0], -3.5);
    assert_eq!(puppet.clock.as_ref().unwrap().world.as_ptr(), storage);
    assert!(puppet.sampled.is_empty());
}

#[test]
fn clipless_puppet_host() {
    use paper_scene::puppet::playback::Playback;
    use paper_scene::script::{Load, SceneScripts};
    use serde_json::json;
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../paper-scene/src/fixtures/a02/clock-step.mdl"
    ));
    let mesh = paper_scene::puppet::parse(bytes).unwrap();
    let initial = paper_scene::puppet::authored(&mesh, &[], None).unwrap();
    let mut playback = Playback::default();
    playback.install_rig(0, "182".into(), paper_scene::puppet::parse(bytes).unwrap());
    let mut scene = json!({"objects":[{"id":182},{"id":183,"text":{"value":"","script":"export function update(){return 'alive';}"}}]});
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &BTreeMap::new(),
        &json!({}),
        Load { puppets: Some(playback), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    let mut puppet = Puppet {
        id: "182".into(),
        model: Some(paper_scene::model::Puppet {
            cull: false,
            cpu: false,
            planar_owner: true,
            world: paper_scene::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]),
            mesh,
            layers: Vec::new(),
            size: (1.0, 1.0),
            mask_textures: Vec::new(),
        }),
        sampled: Vec::new(),
        bone_revision: 0,
        clock: None,
        order: None,
        reordered: None,
    };
    let mut palette = paper_scene::effects::material::Palette {
        bones: initial.bones.clone(),
        morph: None,
        channels: Vec::new(),
        alpha: None,
        uniform: "g_Bones[1]".into(),
    };
    for time in [0.0, 1.0] {
        host.tick(time, 0.0, [0.5; 2]).unwrap();
        puppet.sample(&host, &mut palette).unwrap();
        assert_eq!(palette.bones, initial.bones);
        assert_eq!(host.scene["objects"][1]["text"], "alive");
    }
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn prelit_rebakes_live_inputs() {
    use super::Input;
    let bytes = [1_u8, 2];
    let still = [Input::Texture(0)];
    assert!(!prelit::stale(Some(&bytes), &bytes, false, &still));
    assert!(prelit::stale(Some(&bytes), &[3], false, &still));
    assert!(prelit::stale(None, &bytes, false, &still));
    assert!(prelit::stale(Some(&bytes), &bytes, true, &still));
    for input in [Input::ShadowAtlas, Input::LightCookie, Input::Reflection, Input::History] {
        assert!(prelit::stale(Some(&bytes), &bytes, false, &[Input::Texture(0), input]));
    }
}
