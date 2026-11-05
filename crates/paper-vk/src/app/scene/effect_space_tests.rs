use super::effect_lifetime_tests::{model_files, texture_levels};
use super::*;

pub(super) const PASS_VERTEX: &[u8] = b"attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nuniform mat4 g_ModelViewProjectionMatrix;\nvarying vec2 v_TexCoord;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);v_TexCoord=a_TexCoord;}";

fn row(files: &[(&str, &[u8])]) -> Vec<[u8; 4]> {
    let shared = crate::shared::test_device();
    let mut model = model_files(files);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    let (width, _, rgba) = group.read_canvas().unwrap();
    group.destroy();
    let stride = width as usize * RGBA_CHANNELS;
    rgba[32 * stride..33 * stride]
        .chunks_exact(RGBA_CHANNELS)
        .map(|pixel| pixel.try_into().unwrap())
        .collect()
}

#[test]
#[ignore = "requires Vulkan"]
fn lit_offscreen_fills_layer() {
    let texture =
        texture_levels(&[paper_scene::tex::MipLevel { width: 1, height: 1, data: vec![255; 4] }]);
    let row = row(&[
        ("scene.json", br#"{"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},"objects":[{"id":1,"image":"models/lit.json","origin":"32 32 0","size":"32 32","effects":[{"file":"effects/copy.json"}]}]}"#),
        ("models/lit.json", br#"{"material":"materials/lit.json"}"#),
        ("materials/probe.tex", &texture),
        ("materials/lit.json", br#"{"passes":[{"shader":"lit","textures":["probe"]}]}"#),
        ("shaders/lit.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nuniform mat4 g_ModelMatrix;\nuniform mat4 g_ViewProjectionMatrix;\nvoid main(){gl_Position=mul(mul(vec4(a_Position,1.0),g_ModelMatrix),g_ViewProjectionMatrix);}"),
        ("shaders/lit.frag", b"void main(){gl_FragColor=vec4(1.0,0.0,0.0,1.0);}"),
        ("effects/copy.json", br#"{"passes":[{"material":"materials/copy.json"}]}"#),
        ("materials/copy.json", br#"{"passes":[{"shader":"copy"}]}"#),
        ("shaders/copy.vert", PASS_VERTEX),
        ("shaders/copy.frag", b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}"),
    ]);
    let lit: Vec<_> = (0..row.len()).filter(|&x| row[x][0] > 0).collect();
    assert_eq!((lit.first(), lit.last()), (Some(&16), Some(&47)));
}

#[test]
#[ignore = "requires Vulkan"]
fn solid_effect_full_uv() {
    let row = row(&[
        ("scene.json", br#"{"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},"objects":[{"id":1,"image":"models/flat.json","origin":"32 32 0","size":"64 64","effects":[{"file":"effects/uv.json"}]}]}"#),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/uv.json", br#"{"passes":[{"material":"materials/uv.json"}]}"#),
        ("materials/uv.json", br#"{"passes":[{"shader":"uv"}]}"#),
        ("shaders/uv.vert", PASS_VERTEX),
        ("shaders/uv.frag", b"varying vec2 v_TexCoord;\nvoid main(){gl_FragColor=vec4(v_TexCoord.x,0.0,0.0,1.0);}"),
    ]);
    assert_eq!((row[0][0], row[63][0]), (2, 253));
}

fn prelit_row(texture: &[u8], scene: &[u8]) -> Vec<[u8; 4]> {
    row(&[
        ("scene.json", scene),
        ("models/lit.json", br#"{"material":"materials/lit.json"}"#),
        ("materials/probe.tex", texture),
        ("materials/lit.json", br#"{"passes":[{"shader":"lit","textures":["probe"],"combos":{"LIGHTING":1}}]}"#),
        ("shaders/lit.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nuniform mat4 g_AltModelMatrix;\nvarying vec3 v_World;\nvoid main(){\n#if PRELIGHTING\nv_World=mul(vec4(a_Position,1.0),g_AltModelMatrix).xyz;\n#else\nv_World=vec3(0.0);\n#endif\ngl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/lit.frag", b"varying vec3 v_World;\nvoid main(){gl_FragColor=vec4(v_World.x/64.0,1.0,0.0,1.0);}"),
        ("effects/copy.json", br#"{"passes":[{"material":"materials/copy.json"}]}"#),
        ("materials/copy.json", br#"{"passes":[{"shader":"copy"}]}"#),
        ("shaders/copy.vert", PASS_VERTEX),
        ("shaders/copy.frag", b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}"),
    ])
}

fn square_texture(size: u32) -> Vec<u8> {
    texture_levels(&[paper_scene::tex::MipLevel {
        width: size,
        height: size,
        data: vec![255; (size * size) as usize * RGBA_CHANNELS],
    }])
}

const PRELIT_SCENE: &[u8] = br#"{"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},"objects":[{"id":1,"image":"models/lit.json","origin":"32 32 0","size":"32 32","effects":[{"file":"effects/copy.json"}]}]}"#;

#[test]
#[ignore = "requires Vulkan"]
fn prelit_world_position() {
    let row = prelit_row(&square_texture(16), PRELIT_SCENE);
    assert_eq!((row[16][0], row[32][0], row[47][0]), (98, 128, 157));
}

#[test]
#[ignore = "requires Vulkan"]
fn prelit_image_extent() {
    const IMAGE_FIELDS: usize = 34;
    let mut texture = square_texture(16);
    for (slot, field) in texture[IMAGE_FIELDS..IMAGE_FIELDS + 8].chunks_exact_mut(4).enumerate() {
        field.copy_from_slice(&[8_u32, 4][slot].to_le_bytes());
    }
    let row = prelit_row(&texture, PRELIT_SCENE);
    assert_eq!((row[16][0], row[47][0]), (113, 142));
}

#[test]
#[ignore = "requires Vulkan"]
fn prelit_needs_effects() {
    let scene = br#"{"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},"objects":[{"id":1,"image":"models/lit.json","origin":"32 32 0","size":"32 32"}]}"#;
    let row = prelit_row(&square_texture(16), scene);
    assert_eq!((row[32][0], row[32][1]), (0, 255));
}

const SKIN_VERTEX: &[u8] = b"#if SKINNING\nuniform mat4x3 g_Bones[BONECOUNT];\n#endif\nuniform mat4 g_ModelViewProjectionMatrix;\nuniform vec4 g_Texture0Rotation;\nuniform vec2 g_Texture0Translation;\nattribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nattribute uvec4 a_BlendIndices;\nattribute vec4 a_BlendWeights;\nvarying vec2 v_TexCoord;\nvoid main(){\nv_TexCoord=g_Texture0Translation+a_TexCoord.x*g_Texture0Rotation.xy+a_TexCoord.y*g_Texture0Rotation.zw;\n#if PRELIGHTING\ngl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);\n#else\ngl_Position=mul(vec4(mul(vec4(a_Position,1.0),g_Bones[a_BlendIndices.x]*a_BlendWeights.x+g_Bones[a_BlendIndices.y]*a_BlendWeights.y+g_Bones[a_BlendIndices.z]*a_BlendWeights.z+g_Bones[a_BlendIndices.w]*a_BlendWeights.w),1.0),g_ModelViewProjectionMatrix);\n#endif\n}";
const SKIN_FRAGMENT: &[u8] = b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){\n#if PRELIGHTING\ngl_FragColor=vec4(1.0,0.0,0.0,1.0);\n#else\n#if LIGHTING\ngl_FragColor=vec4(0.0,1.0,0.0,1.0);\n#else\ngl_FragColor=texSample2D(g_Texture0,v_TexCoord);\n#endif\n#endif\n}";
const SKIN_MODEL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../paper-scene/src/fixtures/a02/clock-step.mdl"
));
pub(super) const SKIN_SIZE: u32 = 256;

pub(super) fn skin_model(objects: &str) -> SceneModel {
    skin_model_with(objects, SKIN_FRAGMENT, &square_texture(16), "")
}

fn skin_model_with(objects: &str, fragment: &[u8], texture: &[u8], general: &str) -> SceneModel {
    let scene = format!(
        r#"{{"general":{{"orthogonalprojection":{{"width":4096,"height":4096}},"clearcolor":"0 0 0"{general}}},"objects":[{objects}]}}"#
    );
    let model = model_files(&[
        ("scene.json", scene.as_bytes()),
        ("models/skin.json", br#"{"material":"materials/skin.json","puppet":"models/skin.mdl"}"#),
        ("models/skin.mdl", SKIN_MODEL),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("materials/probe.tex", texture),
        ("materials/skin.json", br#"{"passes":[{"shader":"skin","textures":["probe"],"combos":{"LIGHTING":1}}]}"#),
        ("shaders/skin.vert", SKIN_VERTEX),
        ("shaders/skin.frag", fragment),
        ("effects/copy.json", br#"{"passes":[{"material":"materials/copy.json"}]}"#),
        ("materials/copy.json", br#"{"passes":[{"shader":"copy"}]}"#),
        ("shaders/copy.vert", PASS_VERTEX),
        ("shaders/copy.frag", b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}"),
    ]);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    model
}

fn canvas_bounds(group: &mut Group, channel: usize) -> [u32; 4] {
    let (width, height, rgba) = group.read_canvas().unwrap();
    let image = image::RgbaImage::from_raw(width, height, rgba).unwrap();
    let mut bounds = [width, height, 0, 0];
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel[channel] > 128 {
            bounds = [bounds[0].min(x), bounds[1].min(y), bounds[2].max(x), bounds[3].max(y)];
        }
    }
    assert!(bounds[0] < bounds[2] && bounds[1] < bounds[3], "no pixels: {bounds:?}");
    bounds.map(|edge| edge * SKIN_SIZE / width)
}

fn skin_bounds(objects: &str, channel: usize) -> [u32; 4] {
    let shared = crate::shared::test_device();
    let mut model = skin_model(objects);
    let mut group =
        build_group(&shared, &mut model, true, &[(SKIN_SIZE, SKIN_SIZE)], FillMode::Fill).unwrap();
    let bounds = canvas_bounds(&mut group, channel);
    group.destroy();
    bounds
}

const SKIN_OBJECT: &str =
    r#"{"id":1,"image":"models/skin.json","origin":"2048 2048 0","size":"64 64"}"#;
pub(super) const SKIN_WITH_EFFECT: &str = r#"{"id":1,"image":"models/skin.json","origin":"2048 2048 0","size":"64 64","effects":[{"file":"effects/copy.json"}]}"#;
const OTHER_EFFECT: &str = r#"{"id":2,"image":"models/flat.json","origin":"8 8 0","size":"4 4","color":"0 0 1","effects":[{"file":"effects/copy.json"}]}"#;

#[test]
#[ignore = "requires Vulkan"]
fn hosted_skin_draws() {
    let viewport = skin_bounds(SKIN_OBJECT, 1);
    let hosted = skin_bounds(&format!("{SKIN_OBJECT},{OTHER_EFFECT}"), 1);
    for (edge, expected) in hosted.iter().zip(viewport) {
        assert!(edge.abs_diff(expected) <= 2, "{hosted:?} {viewport:?}");
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn prelit_skin_feeds_effects() {
    let direct = skin_bounds(SKIN_OBJECT, 1);
    let baked = skin_bounds(SKIN_WITH_EFFECT, 0);
    for (edge, expected) in baked.iter().zip(direct) {
        assert!(edge.abs_diff(expected) <= 2, "{baked:?} {direct:?}");
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn hosted_skin_follows_parallax() {
    const CANVAS: f32 = 4096.0;
    let shared = crate::shared::test_device();
    let mut model = skin_model(&format!("{SKIN_OBJECT},{OTHER_EFFECT}"));
    model.mouse = paper_scene::mouse::Parallax {
        amount: 0.5,
        influence: 0.5,
        delay: 0.0,
        ..Default::default()
    };
    model.layers[0].mouse = paper_scene::mouse::LayerMouse { parallax: [1.0; 2], clock: None };
    let mut group =
        build_group(&shared, &mut model, true, &[(SKIN_SIZE, SKIN_SIZE)], FillMode::Fill).unwrap();
    group.compose(0.0, 1.0 / 30.0).unwrap();
    let before = canvas_bounds(&mut group, 1);
    let rest = group.quads[0].rect;
    group.mouse.update(1, [0.9, 0.1], [false; 3], (SKIN_SIZE, SKIN_SIZE), FillMode::Fill);
    group.compose(0.0, 1.0 / 30.0).unwrap();
    let after = canvas_bounds(&mut group, 1);
    let shift = [group.quads[0].rect[0] - rest[0], group.quads[0].rect[1] - rest[1]];
    group.destroy();
    assert!(shift[0].abs() > 64.0 && shift[1].abs() > 64.0, "{shift:?}");
    for (axis, edge) in [(0, 0), (1, 1), (0, 2), (1, 3)] {
        let moved = i64::from(after[edge]) - i64::from(before[edge]);
        let expected = (shift[axis] * SKIN_SIZE as f32 / CANVAS).round() as i64;
        assert!((moved - expected).abs() <= 2, "axis {axis}: {moved} vs {expected}");
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn prelit_samples_image_region() {
    const IMAGE_FIELDS: usize = 34;
    let mut data = vec![0_u8; 16 * 16 * RGBA_CHANNELS];
    for (index, pixel) in data.chunks_exact_mut(RGBA_CHANNELS).enumerate() {
        let inside = index % 16 < 8 && index / 16 < 8;
        pixel.copy_from_slice(if inside { &[255, 255, 255, 255] } else { &[255, 0, 0, 255] });
    }
    let mut texture = texture_levels(&[paper_scene::tex::MipLevel { width: 16, height: 16, data }]);
    for (slot, field) in texture[IMAGE_FIELDS..IMAGE_FIELDS + 8].chunks_exact_mut(4).enumerate() {
        field.copy_from_slice(&[8_u32, 8][slot].to_le_bytes());
    }
    let fragment = b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}";
    let shared = crate::shared::test_device();
    let mut model = skin_model_with(SKIN_WITH_EFFECT, fragment, &texture, "");
    let mut group =
        build_group(&shared, &mut model, true, &[(SKIN_SIZE, SKIN_SIZE)], FillMode::Fill).unwrap();
    let (width, height, rgba) = group.read_canvas().unwrap();
    group.destroy();
    let image = image::RgbaImage::from_raw(width, height, rgba).unwrap();
    let padding = image.pixels().filter(|pixel| pixel[0] > 128 && pixel[1] < 64).count();
    let content = image.pixels().filter(|pixel| pixel[0] > 128 && pixel[1] > 128).count();
    assert!(content > 0, "no image content");
    assert!(padding * 50 < content, "padding {padding} content {content}");
}

#[test]
#[ignore = "requires Vulkan"]
fn parallax_scene_leaves_viewport() {
    let shared = crate::shared::test_device();
    let mut model = skin_model_with(
        SKIN_OBJECT,
        SKIN_FRAGMENT,
        &square_texture(16),
        r#","cameraparallax":true,"cameraparallaxamount":0.5,"cameraparallaxmouseinfluence":0.5"#,
    );
    assert!(!model.skin_viewport);
    let group =
        build_group(&shared, &mut model, true, &[(SKIN_SIZE, SKIN_SIZE)], FillMode::Fill).unwrap();
    assert!(group.target.viewport.is_none());
    group.destroy();
    let mut plain = skin_model(SKIN_OBJECT);
    assert!(plain.skin_viewport);
    let group =
        build_group(&shared, &mut plain, true, &[(SKIN_SIZE, SKIN_SIZE)], FillMode::Fill).unwrap();
    assert!(group.target.viewport.is_some());
    group.destroy();
}
