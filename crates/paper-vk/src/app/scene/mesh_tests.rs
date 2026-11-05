use super::*;
use paper_scene::effects::{EffectPass, PassBlend};
use paper_scene::model::Camera;
use paper_scene::shader::{self, Stage};
use serde_json::json;

#[test]
fn split_mesh_matrices() {
    for (declaration, expected) in [
        ("uniform mat4 g_ModelViewProjectionMatrix;", [3.0; 16]),
        ("uniform mat4 g_ModelMatrix;\nuniform mat4 g_ViewProjectionMatrix;", [2.0; 16]),
    ] {
        let pass = mesh_pass(
            &format!("{declaration} void main() {{ gl_Position=vec4(0.0); }}"),
            "void main() { gl_FragColor=vec4(1.0); }",
        );
        let transform = MeshTransform {
            model: [2.0; 16],
            view_projection: [1.0; 16],
            projection: [3.0; 16],
            normal: [0.0; NORMAL_MATRIX_FLOATS],
            eye: [0.0; NORMAL_COMPONENTS],
            basis: None,
        };
        assert_eq!(mesh_projection(&PassMeta::of(&pass), transform).unwrap().1, expected);
    }
}

#[test]
fn mount_composition_order() {
    let pose = |origin, scale| paper_scene::model::MeshPose { origin, scale, angles: [0.0; 3] };
    let child = pose([1.0, 0.0, 0.0], [1.0; 3]);
    let parents = [pose([0.0, 1.0, 0.0], [1.0; 3]), pose([0.0; 3], [2.0; 3])];
    let world = mesh_model_matrix([0.0, 0.0, 5.0], [3.0; 3], [0.0, 0.0, 0.5]);
    let (model, _) = mesh_matrices(child, &parents, Some((1, &world)));
    let group = mat4_mul(
        &mesh_model_matrix([0.0, 1.0, 0.0], [1.0; 3], [0.0; 3]),
        &mesh_model_matrix([1.0, 0.0, 0.0], [1.0; 3], [0.0; 3]),
    );
    let expected =
        mat4_mul(&mesh_model_matrix([0.0; 3], [2.0; 3], [0.0; 3]), &mat4_mul(&world, &group));
    assert_eq!(model, expected);
    let (_, rotated) =
        mesh_matrices(pose([0.0; 3], [1.0; 3]), &[pose([0.0; 3], [1.0; 3])], Some((0, &world)));
    let turned =
        paper_scene::model::MeshPose { angles: [0.0, 0.0, 0.5], ..pose([0.0; 3], [1.0; 3]) };
    let (_, native) = mesh_matrices(pose([0.0; 3], [1.0; 3]), &[turned], None);
    for (actual, expected) in rotated.into_iter().zip(native) {
        assert!((actual - expected).abs() < 1e-6, "{rotated:?} {native:?}");
    }
}

#[test]
fn direct_clip_projection() {
    let pass = mesh_pass(
        "attribute vec3 a_Position; void main() { gl_Position=vec4(a_Position,1.0); }",
        "void main() { gl_FragColor=vec4(1.0); }",
    );
    let transform = MeshTransform {
        model: [2.0; 16],
        view_projection: [1.0; 16],
        projection: [3.0; 16],
        normal: [0.0; NORMAL_MATRIX_FLOATS],
        eye: [0.0; NORMAL_COMPONENTS],
        basis: None,
    };
    assert!(mesh_projection(&PassMeta::of(&pass), transform).is_none());
}

fn projected_bounds(model: &Mat4) -> [f32; 4] {
    let camera = Camera {
        eye: [0.0, 0.0, -4.0],
        center: [0.0; 3],
        up: [0.0, 1.0, 0.0],
        fov: 60.0,
        near: 0.1,
        far: 100.0,
    };
    let (view_projection, _, _) = authored_perspective(camera, (1280.0, 720.0), false);
    let transform = mat4_mul(&view_projection, model);
    let vertices = [[-0.6, -0.3, 0.0, 1.0], [0.9, -0.3, 0.0, 1.0], [-0.6, 0.5, 0.0, 1.0]];
    let projected = vertices.map(|vertex| {
        let clip: [f32; 4] = std::array::from_fn(|row| {
            (0..4).map(|column| transform[column * 4 + row] * vertex[column]).sum()
        });
        [640.0 * (clip[0] / clip[3] + 1.0), 360.0 * (clip[1] / clip[3] + 1.0)]
    });
    [
        projected.iter().map(|point| point[0]).fold(f32::INFINITY, f32::min),
        projected.iter().map(|point| point[1]).fold(f32::INFINITY, f32::min),
        projected.iter().map(|point| point[0]).fold(f32::NEG_INFINITY, f32::max),
        projected.iter().map(|point| point[1]).fold(f32::NEG_INFINITY, f32::max),
    ]
}

#[test]
fn native_rotation_bounds() {
    let model = mesh_model_matrix([0.0; 3], [1.0; 3], [0.3, 0.4, 0.5]);
    let bounds = projected_bounds(&model);
    for (actual, native) in bounds.into_iter().zip([494.0, 334.0, 735.0, 440.0]) {
        assert!((actual - native).abs() < 1.0);
    }
}

#[test]
fn native_parent_bounds() {
    for (child, parent_origin, parent_scale, parent_angles, expected) in [
        ([0.0; 3], [0.4, 0.2, 0.0], [1.0; 3], [0.0; 3], [438.0, 251.0, 671.0, 376.0]),
        ([0.4, 0.0, 0.0], [0.0; 3], [1.0; 3], [0.0, 0.0, 0.5], [441.0, 304.0, 704.0, 416.0]),
        ([0.4, 0.0, 0.0], [0.0; 3], [2.0, 1.0, 1.0], [0.0; 3], [236.0, 282.0, 702.0, 407.0]),
    ] {
        let parent = mesh_model_matrix(parent_origin, parent_scale, parent_angles);
        let local = mesh_model_matrix(child, [1.0; 3], [0.0; 3]);
        let model = mat4_mul(&parent, &local);
        let bounds = projected_bounds(&model);
        for (actual, native) in bounds.into_iter().zip(expected) {
            assert!((actual - native).abs() < 2.0, "{bounds:?} {expected:?}");
        }
    }
}

#[test]
fn native_normal_colors() {
    let normal = [std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2, 0.0];
    for (scale, angles, expected) in [
        ([2.0, 1.0, 1.0], [0.0; 3], [218.0, 218.0, 127.0]),
        ([1.0; 3], [0.3, 0.4, 0.5], [168.0, 248.0, 117.0]),
        ([-1.0, 1.0, 1.0], [0.0; 3], [37.0, 218.0, 127.0]),
    ] {
        let matrix = mesh_normal_matrix(&mesh_normal_model(scale, angles));
        let rgb: [f32; NORMAL_COMPONENTS] = std::array::from_fn(|row| {
            let value: f32 = (0..NORMAL_COMPONENTS)
                .map(|column| normal[column] * matrix[column * MATRIX_COLUMN_COMPONENTS + row])
                .sum();
            (value * 0.5 + 0.5) * 255.0
        });
        for (actual, native) in rgb.into_iter().zip(expected) {
            assert!((actual - native).abs() < 1.0);
        }
    }
}

#[test]
fn native_orthographic_bits() {
    for (output, y, translation) in [
        ([1280, 720], 0x3c63_8e39_u32, 0xbfe3_8e39_u32),
        ([720, 720], 0x3c00_0000, 0xbf80_0000),
        ([1279, 721], 0x3c63_0ffa, 0xbfe3_0ffa),
    ] {
        let view = vk::SceneViewport::cover([256.0, 256.0], output).unwrap();
        let projection = orthographic_view(view);
        assert_eq!(projection[5].to_bits(), y);
        assert_eq!(projection[13].to_bits(), translation);
        assert_eq!(projection[10].to_bits(), (1.0_f32 / 4000.0).to_bits());
        assert_eq!(projection[14], 0.5);
    }
}

#[test]
fn small_canvas_center() {
    let view = vk::SceneViewport::cover([1.0, 1.0], [1280, 720]).unwrap();
    let projection = orthographic_view(view);
    assert_eq!(projection[0] * 0.5 + projection[12], 0.0);
    assert_eq!(projection[5] * 0.5 + projection[13], 0.0);
    assert_eq!(projection[5], 2.0 / 0.5625);
    assert_eq!(view.authored_viewport(), [0.0, -280.0, 1280.0, 1280.0]);
}

fn flat_settings() -> Settings {
    Settings {
        projection: ProjectionMode::Orthographic,
        depth: true,
        camera: None,
        orientation: None,
        perspective_fov: 95.0,
        ambient: [0.0; 3],
        skylight: [0.0; 3],
    }
}

fn ndc(view: &View, [x, y, z]: [f32; 3]) -> [f32; 2] {
    let clip = super::super::tests::transform(&view.projection, [x, y, z, 1.0]);
    [clip[0] / clip[3], clip[1] / clip[3]]
}

#[test]
fn perspective_mesh_depth() {
    let canvas = (1920.0, 1080.0);
    let flat = flat_settings().view(canvas, None, vk::ClipView::IDENTITY, false).unwrap();
    let deep = flat_settings().view(canvas, None, vk::ClipView::IDENTITY, true).unwrap();
    let eye = deep.eye[2];
    assert_eq!(deep.eye[..2], [960.0, 540.0]);
    assert!((eye - 494.8).abs() < 0.1, "{eye}");
    let near = ndc(&deep, [1060.0, 640.0, 0.0]);
    let level = ndc(&flat, [1060.0, 640.0, 0.0]);
    let far = ndc(&deep, [1060.0, 640.0, -eye]);
    for axis in 0..2 {
        assert!((near[axis] - level[axis]).abs() < 1e-5, "{near:?} {level:?}");
        assert!((far[axis] - level[axis] * 0.5).abs() < 1e-5, "{far:?} {level:?}");
    }
}

#[test]
fn perspective_follows_crop() {
    let wide = flat_settings().view((1920.0, 1080.0), None, vk::ClipView::IDENTITY, true).unwrap();
    let crop = vk::SceneViewport::cover([1920.0, 1920.0], [1920, 1080]);
    let square =
        flat_settings().view((1920.0, 1920.0), crop, vk::ClipView::IDENTITY, true).unwrap();
    let expected = ndc(&wide, [1060.0, 640.0, -500.0]);
    let actual = ndc(&square, [1060.0, 1060.0, -500.0]);
    for axis in 0..2 {
        assert!((actual[axis] - expected[axis]).abs() < 1e-5, "{actual:?} {expected:?}");
    }
}

fn captured_mesh() -> SceneModel {
    super::super::tests::mesh_model(br#"{"general":{"orthogonalprojection":{"width":128,"height":128}},"objects":[{"id":1,"model":"models/mesh.mdl","origin":"64 64 10","scale":"20 20 20"},{"id":2,"image":"models/sample.json","origin":"20 64 0","size":"8 8"},{"id":3,"model":"models/mesh.mdl","origin":"64 64 -10","scale":"20 20 20"},{"id":4,"image":"models/sample.json","origin":"108 64 0","size":"8 8"}]}"#)
}

#[test]
fn bounded_capture_admission() {
    use paper_scene::effects::{EffectBind, Fbo, FboFormat, FboSize, Swap};
    for case in 0..11 {
        let mut model = captured_mesh();
        let layer = &mut model.layers[0];
        match case {
            0 => {}
            1 => layer.base_scene_input = false,
            2 => layer.passthrough = true,
            3 => layer.copy_background = true,
            4 => layer.kind = paper_scene::model::LayerKind::Viewport { hdr: false },
            5 => layer.effects[0].passes[0].target = Some("named".into()),
            6 => layer.effects[0].passes[0].binds.push((1, EffectBind::SceneHistory)),
            7 => layer.effects[0].fbos.push(Fbo {
                name: "named".into(),
                size: FboSize::Scale(1.0),
                format: FboFormat::Rgba8,
                repeat: None,
                clear: [0.0; 4],
                clear_on_setup: false,
                unique: false,
            }),
            8 => layer.effects[0].swaps.push(Swap {
                after_pass: None,
                source: "first".into(),
                target: "second".into(),
            }),
            9 => {
                let effect = captured_mesh().layers.remove(0).effects.remove(0);
                layer.effects.push(effect);
            }
            10 => {
                let pass = captured_mesh().layers.remove(0).effects.remove(0).passes.remove(0);
                layer.effects[0].passes.push(pass);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            viewport(&model, &[(128, 64)], paper_geom::FillMode::Fill).is_ok(),
            case == 0,
            "case {case}"
        );
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn mesh_capture_depth() {
    let device = crate::shared::test_device();
    let mut frames = Vec::new();
    for replay in [false, true] {
        let mut model = captured_mesh();
        model.meshes[0].scene_order = 0;
        model.layers[0].scene_order = 1;
        model.meshes[1].scene_order = 3;
        model.layers[1].scene_order = 4;
        for (order, color) in [(2, [0.0, 0.0, 1.0]), (5, [1.0, 1.0, 0.0])] {
            let mut layer = super::super::tests::test_layer(true, 0);
            layer.id = format!("overlay-{order}");
            layer.center = (55.0, 73.0);
            layer.size = (8.0, 8.0);
            layer.color = color;
            layer.scene_order = order;
            model.layers.push(layer);
        }
        let mut group = super::super::build_group(
            &device,
            &mut model,
            true,
            &[(128, 64)],
            paper_geom::FillMode::Fill,
        )
        .unwrap();
        group.replay_snapshots = replay;
        assert_eq!(group.scene_snapshot.as_ref().unwrap().viewport, group.target.viewport);
        group.frozen = false;
        group.compose(0.0, 0.016).unwrap();
        let (width, height, pixels) = group.read_canvas().unwrap();
        assert_eq!((width, height), (128, 64));
        for (x, y, color) in
            [(20, 32, [255, 0, 0]), (108, 32, [0, 0, 255]), (55, 41, [255, 255, 0])]
        {
            let offset = (y * width as usize + x) * 4;
            assert_eq!(&pixels[offset..offset + 3], &color, "pixel {x},{y}");
        }
        frames.push(pixels);
        group.destroy();
    }
    assert_eq!(frames[0], frames[1]);
}

fn framebuffer_pass(fragment: &str) -> EffectPass {
    let mut vertex = shader::translate(
        "attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){v_TexCoord=a_TexCoord;gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}",
        Stage::Vertex,
        &BTreeMap::new(),
    );
    let mut fragment = shader::translate(fragment, Stage::Fragment, &BTreeMap::new());
    shader::unify_uniforms(&mut vertex, &mut fragment);
    EffectPass {
        blend: PassBlend::Copy,
        property_source: None,
        user_uniforms: std::collections::BTreeSet::new(),
        name: "framebuffer".into(),
        vertex,
        geometry: None,
        fragment,
        hlsl: None,
        textures: Vec::new(),
        values: BTreeMap::new(),
        target: None,
        binds: Vec::new(),
    }
}

fn framebuffer_model() -> SceneModel {
    use paper_scene::effects::{Effect, EffectBind, EffectState};
    let mut model = super::super::tests::mesh_model(
        br#"{"general":{"orthogonalprojection":{"width":128,"height":128}},"objects":[]}"#,
    );
    let mut seed = super::super::tests::test_layer(true, 0);
    seed.id = "seed".into();
    seed.center = (64.0, 64.0);
    seed.size = (128.0, 128.0);
    seed.material = Some(paper_scene::effects::material::Material {
        state: paper_scene::effects::material::ImageState::default(),
        skin: None,
        prelit: None,
        pass: framebuffer_pass(
            "varying vec2 v_TexCoord;\nvoid main(){gl_FragColor=vec4(v_TexCoord,0.0,1.0);}",
        ),
        blend: PassBlend::Copy,
        texture_copy: false,
    });
    let mut blue = super::super::tests::test_layer(true, 0);
    blue.id = "blue".into();
    blue.center = (90.0, 50.0);
    blue.size = (8.0, 8.0);
    blue.color = [0.0, 0.0, 1.0];
    blue.scene_order = 1;
    let mut capture = super::super::tests::test_layer(true, 0);
    capture.id = "capture".into();
    capture.center = (108.0, 64.0);
    capture.size = (8.0, 8.0);
    capture.color = [1.0, 1.0, 0.75];
    capture.scene_order = 2;
    capture.base_scene_input = true;
    let mut base = framebuffer_pass(
        "uniform sampler2D g_Texture0;\nuniform vec4 g_Color4;\nuniform float g_Time;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.70703125,0.2890625)+vec2(g_Time*0.001))*g_Color4;}",
    );
    base.binds.push((0, EffectBind::SceneSoFar));
    let mut authored = framebuffer_pass(
        "varying vec2 v_TexCoord;\nuniform sampler2D g_Texture0;\nuniform vec3 g_Tone;\nvoid main(){vec4 pixel=texSample2D(g_Texture0,v_TexCoord);gl_FragColor=vec4(pixel.rgb*g_Tone,pixel.a);}",
    );
    authored.property_source = Some((0, 0));
    authored.binds.push((0, EffectBind::Previous));
    authored.values.insert("g_Tone".into(), vec![0.5; 3]);
    capture.effects = [base, authored]
        .into_iter()
        .map(|pass| Effect {
            state: EffectState::default(),
            name: "framebuffer".into(),
            passes: vec![pass],
            fbos: Vec::new(),
            swaps: Vec::new(),
            clear_functions: std::collections::BTreeMap::new(),
        })
        .collect();
    model.layers = vec![seed, blue, capture];
    model
}

#[test]
fn framebuffer_viewport_bounds() {
    for case in 0..8 {
        let mut model = framebuffer_model();
        let mut outputs = vec![(128, 64)];
        let mut mode = paper_geom::FillMode::Fill;
        match case {
            0 => {}
            1 => model.projection = ProjectionMode::Automatic,
            2 => model.projection = ProjectionMode::Other,
            3 => outputs.push((64, 128)),
            4 => mode = paper_geom::FillMode::Fit,
            5 => model.layers[2].copy_background = true,
            6 => {
                model.layers[2].effects[1].passes[0].binds =
                    vec![(0, paper_scene::effects::EffectBind::SceneUnderLayer)]
            }
            7 => model.layers[2].effects[1].passes[0].target = Some("named".into()),
            _ => unreachable!(),
        }
        assert_eq!(viewport(&model, &outputs, mode).is_ok(), case == 0, "case {case}");
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn framebuffer_cover_pixels() {
    let device = crate::shared::test_device();
    let mut model = framebuffer_model();
    assert_eq!(model.layers[2].effects[1].passes[0].property_source, Some((0, 0)));
    let mut group = super::super::build_group(
        &device,
        &mut model,
        true,
        &[(128, 64)],
        paper_geom::FillMode::Fill,
    )
    .unwrap();
    let expected = vk::SceneViewport::cover([128.0, 128.0], [128, 64]);
    assert_eq!(group.target.viewport, expected);
    assert_eq!(group.scene_snapshot.as_ref().unwrap().viewport, expected);
    group.compose(0.0, 0.016).unwrap();
    let (width, height, pixels) = group.read_canvas().unwrap();
    assert_eq!((width, height), (128, 64));
    for (x, y, expected) in [(90, 18, [0_u8, 0, 255]), (108, 32, [0, 0, 96])] {
        let offset = (y * width as usize + x) * 4;
        for (actual, expected) in pixels[offset..offset + 3].iter().zip(expected) {
            assert!(actual.abs_diff(expected) <= 1, "pixel {x},{y}: {actual} != {expected}");
        }
    }
    group.destroy();
}

#[test]
fn perspective_blend_gate() {
    use paper_scene::effects::{CompositeBuffer, EffectBind};
    for blend in [PassBlend::Copy, PassBlend::Alpha, PassBlend::Add] {
        let mut model = captured_mesh();
        assert!(!model.meshes.is_empty());
        model.layers[0].effects[0].passes[0].blend = blend;
        assert!(viewport(&model, &[(128, 128)], paper_geom::FillMode::Fill).is_ok());
        model.projection = ProjectionMode::Other;
        let source = model.layers[0].id.clone();
        model.layers[1].effects[0].passes[0].binds[0].1 =
            EffectBind::LayerComposite { layer: source, buffer: CompositeBuffer::A };
        let result = viewport(&model, &[(128, 128)], paper_geom::FillMode::Fill);
        if blend == PassBlend::Copy {
            assert!(result.unwrap().is_none());
        } else {
            assert_eq!(
                result.unwrap_err().to_string(),
                "base material blending requires explicit orthographic projection"
            );
        }
    }
}

fn mesh_pass(vertex: &str, fragment: &str) -> EffectPass {
    let mut vertex = shader::translate(vertex, Stage::Vertex, &BTreeMap::new());
    let mut fragment = shader::translate(fragment, Stage::Fragment, &BTreeMap::new());
    shader::unify_uniforms(&mut vertex, &mut fragment);
    EffectPass {
        blend: PassBlend::Copy,
        property_source: None,
        user_uniforms: std::collections::BTreeSet::new(),
        name: String::new(),
        vertex,
        geometry: None,
        fragment,
        hlsl: None,
        textures: Vec::new(),
        values: BTreeMap::new(),
        target: None,
        binds: Vec::new(),
    }
}

#[test]
fn authored_mesh_uniforms() {
    let camera = Camera {
        eye: [1.0, 2.0, -5.0],
        center: [0.0; 3],
        up: [0.3, 1.0, 0.2],
        fov: 60.0,
        near: 0.1,
        far: 100.0,
    };
    let pass = mesh_pass(
        "void main() { gl_Position=vec4(0.0); }",
        "uniform vec3 g_ViewUp;
         uniform vec3 g_ViewRight;
         uniform vec3 g_EyePosition;
         void main() { gl_FragColor=vec4(g_ViewUp+g_ViewRight+g_EyePosition,1.0); }",
    );
    let transform = MeshTransform {
        model: [0.0; 16],
        view_projection: [0.0; 16],
        projection: [0.0; 16],
        normal: [0.0; NORMAL_MATRIX_FLOATS],
        eye: camera.eye,
        basis: camera.basis(),
    };
    let mut values = BTreeMap::from([(FX_VIEW_UP.into(), vec![0.0; 3])]);
    transform.write_uniforms(&mut values);
    let bytes = PassMeta::of(&pass).uniform_bytes_with(
        FrameClock::default(),
        None,
        1280,
        720,
        (1280, 720),
        &[],
        &values,
        false,
    );
    for (name, divisor, expected) in [
        (FX_VIEW_UP, 2.0, [159.0, 240.0, 179.0]),
        (FX_VIEW_RIGHT, 2.0, [6.0, 166.0, 118.0]),
        (EYE_POSITION, 16.0, [143.0, 159.0, 48.0]),
    ] {
        let uniform = pass.fragment.uniforms.iter().find(|uniform| uniform.name == name).unwrap();
        for (channel, expected) in expected.into_iter().enumerate() {
            let offset = uniform.offset + channel * size_of::<f32>();
            let value =
                f32::from_ne_bytes(bytes[offset..offset + size_of::<f32>()].try_into().unwrap());
            let encoded = (0.5 + value / divisor) * 255.0;
            assert!((encoded - expected).abs() <= 1.0, "{name}");
        }
    }
}

#[test]
fn reflected_camera_uniforms() {
    let camera = Camera {
        eye: [1.0, 2.0, -5.0],
        center: [0.0; 3],
        up: [0.3, 1.0, 0.2],
        fov: 60.0,
        near: 0.1,
        far: 100.0,
    };
    let view_projection = authored_perspective(camera, (1280.0, 720.0), true).0;
    let model = mesh_model_matrix([-0.65, 1.2, 0.0], [0.4; 3], [0.0; 3]);
    let normal = MeshTransform {
        model,
        view_projection,
        projection: mat4_mul(&view_projection, &model),
        normal: [0.0; NORMAL_MATRIX_FLOATS],
        eye: camera.eye,
        basis: camera.basis(),
    };
    let reflected = normal.reflected();
    assert_eq!(reflected.model, normal.model);
    assert_eq!(reflected.eye, [1.0, -2.0, -5.0]);
    let basis = reflected.basis.unwrap();
    for (vector, expected) in [(basis.right, [6.0, 89.0, 118.0]), (basis.up, [159.0, 15.0, 179.0])]
    {
        for (value, expected) in vector.into_iter().zip(expected) {
            let encoded = (value * 0.5 + 0.5) * 255.0;
            assert!((encoded - expected).abs() <= 1.0);
        }
    }
    let restored = reflected.reflected();
    assert_eq!(restored.view_projection, normal.view_projection);
    assert_eq!(restored.projection, normal.projection);
}

#[test]
fn animated_reflection_textures() {
    let mut texture = paper_scene::model::solid_texture();
    assert!(texture.is_static());
    texture.image_count = 2;
    assert!(!texture.is_static());
    texture.image_count = 1;
    texture.frames = vec![
        paper_scene::model::SpriteFrame {
            uv: [0.0, 0.0, 1.0, 1.0],
            axes: [1.0, 0.0, 0.0, 1.0],
            rotated: false,
            time: 0.1,
            image: 0,
        };
        2
    ];
    assert!(!texture.is_static());
    texture.frames.clear();
    texture.pages.push(std::sync::Arc::new(paper_scene::tex::Pixels::rgba(1, 1, vec![255; 4])));
    assert!(!texture.is_static());
}

#[test]
fn reflection_vertex_inputs() {
    for vertex in [
        "uniform float g_Time;\nvoid main(){gl_Position=vec4(g_Time,0.0,0.0,1.0);}",
        "uniform vec2 g_PointerPosition;\nvoid main(){gl_Position=vec4(g_PointerPosition,0.0,1.0);}",
    ] {
        let mut pass = mesh_pass(vertex, "void main(){gl_FragColor=vec4(1.0);}");
        pass.fragment.uniforms.clear();
        assert!(pass.frame_dependent());
        pass.vertex.uniforms.clear();
        assert!(!pass.frame_dependent());
    }
}

#[test]
fn native_puppet_matrices() {
    let view = vk::SceneViewport::cover([3840.0, 2160.0], [1280, 720]).unwrap();
    let projection = orthographic_view(view);
    let point =
        [f32::from_bits(0x441a_a21f), f32::from_bits(0xc3a9_0dbc), f32::from_bits(0x4096_6c86)];
    for (origin, world_x, clip_x) in
        [(1920.0, 0x4611_f997, 0x4077_69cd), (1952.0, 0x4612_7997, 0x4078_7ade)]
    {
        let model = mesh_model_matrix([origin, 1080.0, 0.0], [12.0, 12.0, 1.0], [0.0; 3]);
        let world = paper_scene::puppet::BonePose::Spatial(model).point(point);
        assert_eq!(world.map(f32::to_bits), [world_x, 0xc53a_149a, 0x4096_6c86]);
        let combined = mat4_mul(&projection, &model);
        let clip = paper_scene::puppet::BonePose::Spatial(combined).point(point);
        assert_eq!(clip.map(f32::to_bits), [clip_x, 0xc070_6e8d, 0x3f00_4d04]);
        assert_ne!(paper_scene::puppet::BonePose::Spatial(projection).point(world), clip);
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn hidden_mesh_pixels() {
    let device = crate::shared::test_device();
    let mut samples = Vec::new();
    for visible in [true, false] {
        let mut model = captured_mesh();
        model.meshes[0].scene_order = 0;
        model.layers[0].scene_order = 1;
        model.meshes[1].scene_order = 3;
        model.layers[1].scene_order = 4;
        model.meshes[0].visible = visible;
        let mut group = super::super::build_group(
            &device,
            &mut model,
            true,
            &[(128, 64)],
            paper_geom::FillMode::Fill,
        )
        .unwrap();
        group.frozen = false;
        group.compose(0.0, 0.016).unwrap();
        let (width, _, pixels) = group.read_canvas().unwrap();
        let offset = (32 * width as usize + 20) * 4;
        samples.push(pixels[offset..offset + 3].to_vec());
        assert_eq!(group.meshes[0].visible, visible);
        group.destroy();
    }
    assert_eq!(samples[0], [255, 0, 0]);
    assert_ne!(samples[1], [255, 0, 0]);
}

#[test]
fn uncropped_mixed_fallback() {
    let mut model = captured_mesh();
    let effect = captured_mesh().layers.remove(0).effects.remove(0);
    model.layers[0].effects.push(effect);
    assert!(viewport(&model, &[(128, 64)], paper_geom::FillMode::Fill).is_err());
    assert!(viewport(&model, &[(128, 128)], paper_geom::FillMode::Fill).unwrap().is_none());
}

#[test]
#[ignore = "requires Vulkan"]
fn perspective_mesh_pixels() {
    let device = crate::shared::test_device();
    let mut canvases = Vec::new();
    for perspective in [false, true] {
        let scene = json!({"general":{"orthogonalprojection":{"width":128,"height":128}},"objects":[{"id":1,"model":"models/mesh.mdl","origin":"64 64 20","scale":"20 20 20","perspective":perspective}]});
        let mut model = super::super::tests::mesh_model(&serde_json::to_vec(&scene).unwrap());
        assert_eq!(model.meshes[0].perspective, perspective);
        let mut group = super::super::build_group(
            &device,
            &mut model,
            true,
            &[(128, 128)],
            paper_geom::FillMode::Fill,
        )
        .unwrap();
        group.compose(0.0, 0.016).unwrap();
        canvases.push(super::super::tests::lit_pixels(&group.read_canvas().unwrap().2));
        group.destroy();
    }
    assert!(canvases[1] > canvases[0], "{canvases:?}");
}

#[test]
#[ignore = "requires Vulkan"]
fn fallback_mesh_pixels() {
    let device = crate::shared::test_device();
    let mut samples = Vec::new();
    for fallback in [false, true] {
        let mut model = captured_mesh();
        if fallback {
            let effect = captured_mesh().layers.remove(0).effects.remove(0);
            model.layers[0].effects.push(effect);
        }
        let mut group = super::super::build_group(
            &device,
            &mut model,
            true,
            &[(128, 128)],
            paper_geom::FillMode::Fill,
        )
        .unwrap();
        assert_eq!(group.target.viewport.is_none(), fallback);
        group.frozen = false;
        group.compose(0.0, 0.016).unwrap();
        let (width, _, pixels) = group.read_canvas().unwrap();
        let row = &pixels[64 * width as usize * 4..65 * width as usize * 4];
        samples.push(row[32 * 4..].to_vec());
        group.destroy();
    }
    assert_eq!(samples[0], samples[1]);
    assert!(samples[0].chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]));
}

#[test]
#[ignore = "requires Vulkan"]
fn spring_owner_reaches_rig() {
    const STEP: f32 = 1.0 / 30.0;
    let bones = |moved: bool| {
        let package = super::super::effect_lifetime_tests::package_files(&[
            ("scene.json", br#"{"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100,"clearcolor":"0 0 0"},"objects":[{"id":1,"name":"rig","model":"models/mesh.mdl"}]}"#),
            ("models/mesh.mdl", include_bytes!("../../../../paper-scene/src/fixtures/spring.mdl")),
            ("materials/a.json", br#"{"passes":[{"shader":"skin","cullmode":"nocull"}]}"#),
            ("shaders/skin.vert", b"uniform mat4x3 g_Bones[BONECOUNT];\nuniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nattribute uvec4 a_BlendIndices;\nattribute vec4 a_BlendWeights;\nvoid main(){gl_Position=mul(vec4(mul(vec4(a_Position,1.0),g_Bones[a_BlendIndices.x])*a_BlendWeights.x,1.0),g_ModelViewProjectionMatrix);}"),
            ("shaders/skin.frag", b"void main(){gl_FragColor=vec4(1.0);}"),
        ]);
        let mut model = paper_scene::model::load_with(
            &package,
            &paper_scene::effects::Assets::discover(Some("/nonexistent")),
        )
        .unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let shared = crate::shared::test_device();
        let mut group = crate::app::scene::build_group(
            &shared,
            &mut model,
            true,
            &[(64, 64)],
            paper_geom::FillMode::Fill,
        )
        .unwrap();
        group.compose(0.0, 0.0).unwrap();
        if moved {
            group.meshes[0].pose(
                paper_scene::model::MeshPose {
                    origin: [3.0, 0.0, 0.0],
                    scale: [1.0; 3],
                    angles: [0.0, 0.9, 0.0],
                },
                &[],
            );
        }
        group.compose(STEP, STEP).unwrap();
        let skin = group.meshes[0].skin.as_ref().unwrap();
        let values = group.meshes[0].values[&skin.uniform].clone();
        group.destroy();
        values
    };
    assert_ne!(bones(true), bones(false));
}

#[test]
#[ignore = "requires Vulkan"]
fn keyframed_mesh_moves() {
    let origin = json!({"value":"0 0 0","animation":{
        "options":{"fps":10,"length":20,"mode":"single"},
        "c0":[{"frame":0,"value":0.0,"step":true},{"frame":10,"value":5.0,"step":true}],
        "c1":[{"frame":0,"value":0.0,"step":true}],
        "c2":[{"frame":0,"value":0.0,"step":true}]}});
    let scene = json!({
        "general":{},
        "camera":{"eye":"0 0 4","center":"0 0 0","up":"0 1 0"},
        "objects":[{"id":1,"model":"models/mesh.mdl","origin":origin}]
    });
    let mut model = super::super::tests::mesh_model(&serde_json::to_vec(&scene).unwrap());
    let device = crate::shared::test_device();
    let mut group = super::super::build_group(
        &device,
        &mut model,
        true,
        &[(32, 32)],
        paper_geom::FillMode::Fill,
    )
    .unwrap();
    group.compose(0.0, 0.0).unwrap();
    let first = group.meshes[0].transform.model[12];
    group.compose(2.0, 2.0).unwrap();
    let second = group.meshes[0].transform.model[12];
    group.destroy();
    assert_eq!([first, second], [0.0, 5.0]);
}
