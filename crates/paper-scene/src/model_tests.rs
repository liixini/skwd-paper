use super::{
    Parallax, Properties, ancestor_chain, animation_id, resolve_transform, supported_layers,
};
use crate::tests::{build_pkg, build_tex};
use serde_json::{Value, json};

#[test]
fn effective_media_sources() {
    use super::MediaTexture::{Current, Previous};
    let thumbnail = |kind| match kind {
        Some(Current) => json!({"type":"system","name":"$mediaThumbnail"}),
        Some(Previous) => json!({"type":"system","name":"$mediaPreviousThumbnail"}),
        None => Value::Null,
    };
    for (instanced, base, instance, fallback, expected) in [
        (false, None, Some(Current), true, None),
        (true, None, Some(Current), true, Some(Current)),
        (false, Some(Current), Some(Previous), true, Some(Current)),
        (true, Some(Current), Some(Previous), true, Some(Previous)),
        (true, Some(Current), None, true, Some(Current)),
        (false, Some(Current), None, false, Some(Current)),
    ] {
        let scene = json!({"objects":[{"id":1,"image":"models/image.json","size":"4 4",
            "instance":{"usertextures":[thumbnail(instance)]}}]})
        .to_string();
        let model = json!({"material":"materials/image.json","instanced":instanced}).to_string();
        let mut pass = json!({"shader":"genericimage4","usertextures":[thumbnail(base)]});
        if fallback {
            pass["textures"] = json!([{"name":"fallback"}]);
        }
        let material = json!({"passes":[pass]}).to_string();
        let texture = build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
        let package = crate::pkg::Package::parse(build_pkg(&[
            ("scene.json", scene.as_bytes()),
            ("models/image.json", model.as_bytes()),
            ("materials/image.json", material.as_bytes()),
            ("materials/fallback.tex", &texture),
        ]))
        .unwrap();
        let model =
            super::load_with(&package, &crate::effects::Assets::discover(Some(""))).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert_eq!(model.layers.len(), 1);
        assert_eq!(model.layers[0].texture.system_texture, expected);
        if expected.is_some() {
            assert_eq!(model.layers[0].texture.pixels.base_rgba().unwrap(), [0; 4]);
        }
    }
}

fn mesh_package(passes: &Value) -> crate::pkg::Package {
    mesh_package_scene(
        passes,
        br#"{"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":1,"model":"models/mesh.mdl","angles":"0.3 0.4 0.5"}]}"#,
    )
}

const MESH_MATERIAL: &[u8] = b"materials/mesh.json";

fn triangle_mesh() -> Vec<u8> {
    let mut mesh = b"MDLV0004\0".to_vec();
    for value in [9_u32, 1, 1] {
        mesh.extend(value.to_le_bytes());
    }
    mesh.extend(MESH_MATERIAL);
    mesh.push(0);
    mesh.extend(0_u32.to_le_bytes());
    mesh.extend(60_u32.to_le_bytes());
    for vertex in
        [[-1.0_f32, -1.0, 0.0, 0.0, 1.0], [1.0, -1.0, 0.0, 1.0, 1.0], [-1.0, 1.0, 0.0, 0.0, 0.0]]
    {
        for value in vertex {
            mesh.extend(value.to_le_bytes());
        }
    }
    mesh.extend(6_u32.to_le_bytes());
    for index in [0_u16, 1, 2] {
        mesh.extend(index.to_le_bytes());
    }
    mesh
}

pub(crate) fn mesh_package_scene(passes: &Value, scene: &[u8]) -> crate::pkg::Package {
    shaded_mesh_package(passes, scene, b"void main(){gl_FragColor=vec4(1.0,0.0,0.0,1.0);}")
}

fn shaded_mesh_package(passes: &Value, scene: &[u8], fragment: &[u8]) -> crate::pkg::Package {
    let mesh = triangle_mesh();
    let material = json!({"passes": passes}).to_string();
    crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", scene),
        ("models/mesh.mdl", &mesh),
        ("materials/mesh.json", material.as_bytes()),
        ("shaders/mesh.vert", b"#include \"common.h\"\nattribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/mesh.frag", fragment),
    ])).unwrap()
}

#[test]
fn indexed_mesh_admitted() {
    let passes = json!([{"shader":"mesh","depthtest":"enabled","depthwrite":"enabled","cullmode":"nocull","blending":"normal"}]);
    let package = mesh_package(&passes);
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.meshes.len(), 1);
    assert_eq!(model.meshes[0].mesh.parts[0].indices, super::mesh::Indices::U16(vec![0, 1, 2]));
    assert_eq!(model.meshes[0].mesh.parts[0].vertices[0].position, [-1.0, -1.0, 0.0]);
    assert_eq!(model.meshes[0].pose.angles, [0.3, 0.4, 0.5]);
    assert!(model.meshes[0].parents.is_empty());
    assert!(model.camera.is_some());
}

#[test]
fn null_light_model() {
    let scene = br#"{"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":1,"model":"models/mesh.mdl"},{"id":2,"light":"point","model":null,"origin":"-1 2 3","color":"1 1 1","intensity":1.87,"radius":16.32}]}"#;
    let package = mesh_package_scene(&json!([{"shader":"mesh"}]), scene);
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.meshes.len(), 1);
    assert_eq!(model.meshes[0].materials[0].pass.values["g_LightsPosition"][..3], [-1.0, 2.0, 3.0]);
}

#[test]
#[ignore = "requires a packaged Arsenal control fixture"]
fn arsenal_mesh_materials_compile() {
    let path = std::path::PathBuf::from(std::env::var("SKWD_ARSENAL_SCENE_PKG").unwrap());
    let package = crate::pkg::Package::open(&path).unwrap();
    let model = super::load_from_dir(&package, path.parent().unwrap()).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.meshes.len(), 1);
    assert_eq!(model.meshes[0].mesh.parts.len(), 6);
    for material in &model.meshes[0].materials {
        let pass = &material.pass;
        assert!(pass.binds.is_empty());
        assert!(!pass.fragment.samplers.is_empty());
        crate::shader::compile(&pass.vertex.source, crate::shader::Stage::Vertex, &pass.name)
            .unwrap();
        crate::shader::compile(&pass.fragment.source, crate::shader::Stage::Fragment, &pass.name)
            .unwrap();
    }
}

#[test]
fn hidden_mesh_visibility() {
    let script = json!({"script":"export function update(value) { return value; }","value":false});
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    for (objects, meshes, visible) in [
        (json!([{"id":1,"model":"models/mesh.mdl","visible":false}]), 0, None),
        (json!([{"id":1,"model":"models/mesh.mdl","visible":script}]), 1, Some(false)),
        (
            json!([{"id":10,"visible":false},{"id":1,"parent":10,"model":"models/mesh.mdl","angles":{"script":"export function update(value) { return value; }","value":"0 0 0"}}]),
            1,
            Some(false),
        ),
        (
            json!([{"id":10,"visible":script.clone()},{"id":1,"parent":10,"model":"models/mesh.mdl"}]),
            1,
            Some(false),
        ),
        (
            json!([{"id":1,"model":"models/mesh.mdl","visible":script.clone()},{"id":2,"model":"models/mesh.mdl"}]),
            2,
            Some(false),
        ),
    ] {
        let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":objects});
        let package = mesh_package_scene(&json!([{"shader":"mesh"}]), scene.to_string().as_bytes());
        let model = super::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert_eq!(model.meshes.len(), meshes, "{objects}");
        assert_eq!(model.meshes.first().map(|mesh| mesh.visible), visible, "{objects}");
        assert!(model.meshes.iter().skip(1).all(|mesh| mesh.visible));
    }
}

#[test]
fn dependency_layers_kept() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let mesh = triangle_mesh();
    let source = json!({"id":2,"image":"models/flat.json","size":"8 8","visible":false});
    let custom = json!({"id":2,"image":"models/custom.json","size":"8 8","visible":false});
    let texture = build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
    for (dependencies, source, gap) in [
        (json!([2]), Some(&source), None),
        (json!([]), Some(&source), Some("undeclared layer composite unsupported")),
        (json!([2]), None, Some("mesh layer composite 2 unavailable")),
        (json!([2]), Some(&custom), Some("mesh layer composite 2 provider unsupported")),
    ] {
        let mesh_object = json!({"id":1,"model":"models/mesh.mdl","dependencies":dependencies});
        let objects: Vec<&Value> = source.into_iter().chain([&mesh_object]).collect();
        let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":objects})
        .to_string();
        let package = crate::pkg::Package::parse(build_pkg(&[
            ("scene.json", scene.as_bytes()),
            ("models/mesh.mdl", &mesh),
            ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
            ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
            ("models/custom.json", br#"{"material":"materials/custom.json"}"#),
            ("materials/custom.json", br#"{"passes":[{"shader":"mesh","textures":["a"]}]}"#),
            ("materials/a.tex", &texture),
            ("materials/mesh.json", br#"{"passes":[{"shader":"mesh","textures":["_rt_imageLayerComposite_2_a"]}]}"#),
            ("shaders/mesh.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
            ("shaders/mesh.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5));}"),
        ]))
        .unwrap();
        let model = super::load_with(&package, &assets).unwrap();
        if let Some(gap) = gap {
            assert!(model.meshes.is_empty(), "{dependencies}");
            assert!(model.skipped.iter().any(|skip| skip.ends_with(gap)), "{:?}", model.skipped);
            continue;
        }
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert!(model.layers.iter().any(|layer| layer.id == "2"));
        let composite = crate::effects::EffectBind::LayerComposite {
            layer: "2".into(),
            buffer: crate::effects::CompositeBuffer::A,
        };
        assert_eq!(model.meshes[0].materials[0].pass.binds, [(0, composite)]);
        assert!(model.dependencies.contains("2"));
    }
}

#[test]
fn resolved_frame_reported() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let mesh = triangle_mesh();
    let reader = json!({"id":2,"image":"models/flat.json","size":"8 8","effects":[{"file":"effects/grab.json"}]});
    let refraction = json!({"id":3,"particle":"particles/grab.json","origin":"0 0 0"});
    let reads: &[u8] = b"uniform sampler2D g_Texture1;\nvoid main(){gl_FragColor=texSample2D(g_Texture1,vec2(0.5));}";
    let ignores: &[u8] = b"uniform sampler2D g_Texture1;\nvoid main(){gl_FragColor=vec4(1.0);}";
    for (objects, fragment, gap) in [
        (json!([{"id":1,"model":"models/mesh.mdl"}]), reads, false),
        (json!([reader, {"id":1,"model":"models/mesh.mdl"}]), reads, true),
        (json!([refraction, {"id":1,"model":"models/mesh.mdl"}]), reads, true),
        (json!([reader, {"id":1,"model":"models/mesh.mdl"}]), ignores, false),
    ] {
        let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":objects}).to_string();
        let package = crate::pkg::Package::parse(build_pkg(&[
            ("scene.json", scene.as_bytes()),
            ("models/mesh.mdl", &mesh),
            ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
            ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
            ("effects/grab.json", br#"{"passes":[{"material":"materials/grab.json"}]}"#),
            ("materials/grab.json", br#"{"passes":[{"shader":"grab","textures":[null,"_rt_FullFrameBuffer"]}]}"#),
            ("shaders/grab.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
            ("shaders/grab.frag", b"uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5))+texSample2D(g_Texture1,vec2(0.5));}"),
            ("particles/grab.json", br#"{"renderer":[{"name":"sprite"}],"material":"materials/grab.json","emitter":[{"name":"boxrandom","rate":1}]}"#),
            ("materials/mesh.json", br#"{"passes":[{"shader":"mesh","textures":[null,"_rt_FullFrameBuffer"]}]}"#),
            ("shaders/mesh.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
            ("shaders/mesh.frag", fragment),
        ]))
        .unwrap();
        let model = super::load_with(&package, &assets).unwrap();
        assert_eq!(model.meshes.is_empty(), gap, "{:?}", model.skipped);
        assert_eq!(model.skipped.iter().any(|skip| skip.ends_with(super::RESOLVED_FRAME_GAP)), gap);
    }
}

#[test]
fn hidden_mesh_source() {
    let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":1,"model":"models/mesh.mdl","visible":false},{"id":2,"source":"_rt_imageLayerComposite_1_a"}]});
    let package = mesh_package_scene(&json!([{"shader":"mesh"}]), scene.to_string().as_bytes());
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    assert!(model.meshes.is_empty());
    assert_eq!(model.skipped, ["object: hidden mesh source unsupported"]);
}

#[test]
fn chained_object_visibility() {
    let props = super::Properties::from([("shown".to_string(), vec![0.0])]);
    let objects = [
        json!({"id":1,"visible":{"user":"shown","value":true}}),
        json!({"id":2,"parent":1}),
        json!({"id":3,"visible":{"script":"","value":false}}),
        json!({"id":4,"visible":true}),
    ];
    let by_id: std::collections::HashMap<_, _> =
        objects.iter().map(|object| (object["id"].to_string(), object)).collect();
    let visible = objects.iter().map(|object| super::object_visible(object, &by_id, &props));
    assert_eq!(visible.collect::<Vec<_>>(), [false, false, false, true]);
}

#[test]
fn mesh_model_parent() {
    let scene = br#"{"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":10,"model":"models/mesh.mdl","origin":"0.4 0.2 0","scale":"2 1 1"},{"id":1,"parent":10,"model":"models/mesh.mdl","origin":"0.3 0 0"},{"id":2,"parent":3,"model":"models/mesh.mdl"},{"id":3,"image":"models/flat.json"}]}"#;
    let package = mesh_package_scene(&json!([{"shader":"mesh"}]), scene);
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    let child = model.meshes.iter().find(|mesh| mesh.id == "1").unwrap();
    assert_eq!(child.parents[0].origin, [0.4, 0.2, 0.0]);
    assert_eq!(child.parents[0].scale, [2.0, 1.0, 1.0]);
    assert!(model.skipped.iter().any(|skip| skip.ends_with("visual parent unsupported")));
    let hidden = br#"{"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":10,"model":"models/mesh.mdl","visible":false},{"id":1,"parent":10,"model":"models/mesh.mdl"}]}"#;
    let package = mesh_package_scene(&json!([{"shader":"mesh"}]), hidden);
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    assert!(model.meshes.iter().all(|mesh| mesh.id != "1"));
}

#[test]
fn mesh_parent_poses() {
    let scene = br#"{"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":10,"origin":"0.4 0.2 0","scale":"2 1 1"},{"id":1,"parent":10,"model":"models/mesh.mdl","origin":"0.3 0 0"}]}"#;
    let package = mesh_package_scene(&json!([{"shader":"mesh"}]), scene);
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.meshes[0].pose.origin, [0.3, 0.0, 0.0]);
    assert_eq!(model.meshes[0].parents[0].origin, [0.4, 0.2, 0.0]);
    assert_eq!(model.meshes[0].parents[0].scale, [2.0, 1.0, 1.0]);
}

#[test]
fn multipass_mesh_rejected() {
    let pass = json!({"shader":"mesh","depthtest":"enabled","depthwrite":"enabled","cullmode":"nocull","blending":"normal"});
    let package = mesh_package(&json!([pass, pass]));
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    assert!(model.meshes.is_empty());
    assert!(model.skipped[0].contains("multipass unsupported"));
}

#[test]
fn mesh_state_defaults() {
    use crate::effects::material::{CullMode, DepthMode};
    let package = mesh_package(&json!([{"shader":"mesh","blending":"normal"}]));
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let material = &model.meshes[0].materials[0];
    assert!(material.test == DepthMode::Enabled);
    assert!(material.write == DepthMode::Enabled);
    assert!(material.cull == CullMode::Normal);
}

#[test]
fn scene_zoom_resolved() {
    let props = Properties::from([("zoom".into(), vec![1.5])]);
    for (zoom, expected) in [
        (json!(null), 1.0),
        (json!(1.13), 1.13),
        (json!({"user":"zoom","value":1.2}), 1.5),
        (json!(0), 1.0),
        (json!(-2), 1.0),
        (json!("NaN"), 1.0),
    ] {
        let scene = json!({
            "general":{"orthogonalprojection":{"width":64,"height":64},"zoom":zoom},
            "objects":[{"id":1,"shape":"quad","origin":"32 32 0"}]
        });
        let source = scene.to_string();
        let pkg =
            crate::pkg::Package::parse(build_pkg(&[("scene.json", source.as_bytes())])).unwrap();
        let assets =
            crate::effects::Assets::discover(Some("/nonexistent")).with_properties(props.clone());
        assert_eq!(
            super::load_with(&pkg, &assets).unwrap().camera_view.scale(),
            expected,
            "{zoom}"
        );
    }
}

#[test]
fn missing_model_placeholder() {
    let scene = json!({
        "general":{"orthogonalprojection":{"width":640,"height":360}},
        "objects":[{"id":1,"image":"models/nothere.json","origin":"320 180 0","size":"300 300"},
            {"id":2,"image":"models/nothere.json","origin":"320 180 0"}]
    });
    let source = scene.to_string();
    let pkg = crate::pkg::Package::parse(build_pkg(&[("scene.json", source.as_bytes())])).unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let model = super::load_with(&pkg, &assets).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.layers[1].size, (1.0, 1.0));
    let layer = &model.layers[0];
    assert_eq!(
        (layer.size, layer.texture.width, layer.texture.nearest),
        ((300.0, 300.0), 40, true)
    );
    let rgba = layer.texture.pixels.base_rgba().unwrap();
    let pixel = |x: usize, y: usize| rgba[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4].to_vec();
    assert_eq!(pixel(5, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(6, 0), [255, 255, 0, 255]);
    assert_eq!(pixel(13, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(34, 34), [0, 0, 0, 255]);
    assert_eq!(pixel(39, 0), [255, 255, 0, 255]);
}

const IMAGE_VERTEX: &[u8] = b"attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);v_TexCoord=a_TexCoord;}";
const IMAGE_FRAGMENT: &[u8] = b"varying vec2 v_TexCoord;\nuniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}";

fn untextured_model(objects: &Value) -> super::SceneModel {
    let scene =
        json!({"general":{"orthogonalprojection":{"width":640,"height":360}},"objects":objects});
    let source = scene.to_string();
    let pkg = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", source.as_bytes()),
        ("models/empty.json", br#"{"autosize":true,"material":"materials/empty.json"}"#),
        ("models/sized.json", br#"{"material":"materials/empty.json"}"#),
        ("materials/empty.json", br#"{"passes":[{"shader":"image","depthtest":"enabled","cullmode":"normal","textures":[null]}]}"#),
        ("models/white.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("models/instance.json", br#"{"instanced":true,"solidlayer":true,"material":"materials/solid.json"}"#),
        ("materials/solid.json", br#"{"passes":[{"shader":"image","textures":["util/white"]}]}"#),
        ("shaders/image.vert", IMAGE_VERTEX),
        ("shaders/image.frag", IMAGE_FRAGMENT),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    super::load_with(&pkg, &assets).unwrap()
}

#[test]
fn null_texture_placeholder() {
    let model = untextured_model(&json!([
        {"id":1,"image":"models/empty.json","origin":"320 180 0","size":"300 300"},
        {"id":2,"image":"models/sized.json","origin":"320 180 0","size":"300 200"}
    ]));
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let sizes: Vec<_> =
        model.layers.iter().map(|layer| (layer.size, layer.texture.width)).collect();
    assert_eq!(sizes, [((6.0, 6.0), 40), ((300.0, 200.0), 40)]);
}

#[test]
fn instance_composite_bound() {
    let composite = json!({"id":3,"image":"models/instance.json","origin":"320 180 0","size":"300 200",
        "dependencies":[2],"instance":{"textures":["_rt_imageLayerComposite_2_a"]}});
    let own = json!({"id":4,"image":"models/instance.json","origin":"320 180 0","size":"300 200",
        "instance":{"textures":["_rt_imageLayerComposite_4_a"]}});
    let model = untextured_model(&json!([
        {"id":2,"image":"models/white.json","origin":"320 180 0","size":"40 30","visible":false},
        composite,
        own
    ]));
    let layer = model.layers.iter().find(|layer| layer.id == "3").expect("composite layer");
    let bind = crate::effects::EffectBind::LayerComposite {
        layer: "2".into(),
        buffer: crate::effects::CompositeBuffer::A,
    };
    assert_eq!(layer.effects[0].passes[0].binds, [(0, bind)]);
    assert!(!layer.base_scene_input);
    assert_eq!(layer.size, (300.0, 200.0));
    assert!(model.layers.iter().any(|layer| layer.id == "2"));
    assert_eq!(model.skipped.len(), 1, "{:?}", model.skipped);
    assert!(model.skipped[0].ends_with("no texture in models/instance.json"));
}

#[test]
fn referenced_compose_kept() {
    let consumer = json!({"id":2,"image":"models/util/composelayer.json","origin":"32 32 0","size":"64 64",
        "effects":[{"file":"effects/copy.json","passes":[{"textures":[null,"_rt_imageLayerComposite_1_a"]}]}]});
    for (objects, kept) in [
        (
            json!([{"id":1,"image":"models/util/composelayer.json","origin":"32 32 0","size":"64 64"}]),
            false,
        ),
        (
            json!([{"id":1,"image":"models/util/composelayer.json","origin":"32 32 0","size":"64 64"}, consumer]),
            true,
        ),
    ] {
        let scene =
            json!({"general":{"orthogonalprojection":{"width":64,"height":64}},"objects":objects});
        let source = scene.to_string();
        let pkg = crate::pkg::Package::parse(build_pkg(&[
            ("scene.json", source.as_bytes()),
            ("models/util/composelayer.json", br#"{"material":"materials/compose.json","passthrough":true}"#),
            ("materials/compose.json", br#"{"passes":[{"shader":"composelayer","blending":"translucent","textures":["_rt_FullFrameBuffer"]}]}"#),
            ("effects/copy.json", br#"{"passes":[{"material":"materials/copy.json"}]}"#),
            ("materials/copy.json", br#"{"passes":[{"shader":"passthrough"}]}"#),
            ("shaders/passthrough.vert", b"attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvoid main(){gl_Position=vec4(a_Position,1.0);v_TexCoord=a_TexCoord;}"),
            ("shaders/passthrough.frag", b"varying vec2 v_TexCoord;\nuniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}"),
        ]))
        .unwrap();
        let assets = crate::effects::Assets::discover(Some("/nonexistent"));
        let model = super::load_with(&pkg, &assets).unwrap();
        let layer = model.layers.iter().find(|layer| layer.id == "1");
        assert_eq!(
            layer.map(|layer| layer.effects.len()),
            kept.then_some(1),
            "{:?}",
            model.skipped
        );
        let transformed = layer.is_some_and(|layer| {
            layer.effects[0].passes[0].vertex.source.contains("#define TRANSFORM 1")
        });
        assert_eq!(transformed, kept);
    }
}

#[test]
fn project_layer_size() {
    for (autosize, project, size) in
        [(true, true, (64.0, 48.0)), (false, true, (16.0, 8.0)), (true, false, (16.0, 8.0))]
    {
        let model = json!({"material":"materials/compose.json","passthrough":true,
            "autosize":autosize,"projectlayer":project})
        .to_string();
        let scene = json!({"general":{"orthogonalprojection":{"width":64,"height":48}},
            "objects":[{"id":1,"image":"models/util/projectlayer.json","origin":"10 12 0","size":"16 8",
                "scale":"0.5 2 1","effects":[{"file":"effects/copy.json","passes":[{}]}]}]})
        .to_string();
        let pkg = crate::pkg::Package::parse(build_pkg(&[
            ("scene.json", scene.as_bytes()),
            ("models/util/projectlayer.json", model.as_bytes()),
            ("materials/compose.json", br#"{"passes":[{"shader":"composelayer","textures":["_rt_FullFrameBuffer"]}]}"#),
            ("effects/copy.json", br#"{"passes":[{"material":"materials/copy.json"}]}"#),
            ("materials/copy.json", br#"{"passes":[{"shader":"passthrough"}]}"#),
            ("shaders/passthrough.vert", b"attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvoid main(){gl_Position=vec4(a_Position,1.0);v_TexCoord=a_TexCoord;}"),
            ("shaders/passthrough.frag", b"varying vec2 v_TexCoord;\nuniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}"),
        ]))
        .unwrap();
        let assets = crate::effects::Assets::discover(Some("/nonexistent"));
        let model = super::load_with(&pkg, &assets).unwrap();
        let layer = &model.layers[0];
        assert_eq!(layer.composition_size, Some(size), "{:?}", model.skipped);
        assert_eq!(layer.size, (size.0 * 0.5, size.1 * 2.0));
        assert_eq!(layer.center, (10.0, 36.0));
    }
}

#[test]
fn quad_uses_canvas_height() {
    for (width, height) in [(256, 256), (512, 512), (512, 256), (256, 512)] {
        let scene = json!({
            "general":{"orthogonalprojection":{"width":width,"height":height}},
            "objects":[{"id":1,"shape":"quad","origin":"50 60 0","scale":"0.1 0.2 1"}]
        });
        let source = scene.to_string();
        let pkg =
            crate::pkg::Package::parse(build_pkg(&[("scene.json", source.as_bytes())])).unwrap();
        let assets = crate::effects::Assets::discover(Some("/nonexistent"));
        let model = super::load_with(&pkg, &assets).unwrap();
        assert!(model.skipped.is_empty());
        assert_eq!(model.layers.len(), 1);
        let layer = &model.layers[0];
        assert_eq!(layer.kind, super::LayerKind::Quad);
        assert_eq!(layer.size, (height as f32 * 0.1, height as f32 * 0.2));
        assert_eq!(layer.center, (50.0, height as f32 - 60.0));
        assert_eq!(layer.uv(), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(layer.texture.pixels.base_rgba(), Some(vec![0, 0, 0, 0]));
        let layout = super::script::Layout::new(layer, &scene, model.canvas, &assets.properties);
        assert_eq!(layout.size, [height as f32; 2]);
    }
}

#[test]
fn quad_preserves_effects() {
    let scene = br#"{"objects":[{"id":1,"shape":"quad","effects":[{"file":"effects/probe.json","passes":[{"combos":{"DIRECTDRAW":1}}]}]}]}"#;
    let package = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", scene),
        ("effects/probe.json", br#"{"passes":[{"material":"materials/probe.json"}]}"#),
        ("materials/probe.json", br#"{"passes":[{"shader":"probe"}]}"#),
        ("shaders/probe.vert", b"void main(){gl_Position=vec4(0.0);}"),
        ("shaders/probe.frag", b"void main(){gl_FragColor=vec4(1.0);}"),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let model = super::load_with(&package, &assets).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.layers[0].effects.len(), 1);
    assert_eq!(model.layers[0].effects[0].passes.len(), 1);
}

#[test]
fn text_reruns_scene_input() {
    let scene =
        br#"{"objects":[{"id":1,"shape":"quad","effects":[{"file":"effects/probe.json"}]}]}"#;
    let package = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", scene),
        ("effects/probe.json", br#"{"passes":[{"material":"materials/probe.json"}]}"#),
        ("materials/probe.json", br#"{"passes":[{"shader":"probe"}]}"#),
        ("shaders/probe.vert", b"void main(){gl_Position=vec4(0.0);}"),
        ("shaders/probe.frag", b"void main(){gl_FragColor=vec4(1.0);}"),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let mut layer = super::load_with(&package, &assets).unwrap().layers.remove(0);
    layer.kind = super::LayerKind::Text;
    assert!(!super::reflection_rerun(&layer));
    assert!(!super::reflection_gap(&layer));
    layer.effects[0].passes[0].binds.push((0, crate::effects::EffectBind::SceneSoFar));
    assert!(super::reflection_rerun(&layer));
}

#[test]
fn unknown_shapes_reported() {
    let package = crate::pkg::Package::parse(build_pkg(&[(
        "scene.json",
        br#"{"objects":[{"id":1,"name":"Sphere","shape":"sphere"}]}"#,
    )]))
    .unwrap();
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    assert!(model.layers.is_empty());
    assert_eq!(model.skipped, ["Sphere: unsupported shape"]);
}

#[test]
fn authored_camera_is_bounded() {
    let props = Properties::new();
    let scene = json!({
        "camera":{"eye":"0 0 0","center":"0 0 -1","up":"0 1 0"},
        "general":{"fov":50,"nearz":0.01,"farz":10000}
    });
    let camera = super::Camera::of(&scene, &props, super::ProjectionMode::of(&scene)).unwrap();
    assert_eq!(camera.eye, [0.0; 3]);
    assert_eq!(camera.center, [0.0, 0.0, -1.0]);
    assert_eq!((camera.fov, camera.near, camera.far), (50.0, 0.01, 10000.0));
    let mut low_fov = scene.clone();
    low_fov["general"]["fov"] = json!(0);
    assert_eq!(super::Camera::of(&low_fov, &props, super::ProjectionMode::Other).unwrap().fov, 0.1);
    let mut high_fov = scene.clone();
    high_fov["general"]["fov"] = json!(180);
    assert_eq!(
        super::Camera::of(&high_fov, &props, super::ProjectionMode::Other).unwrap().fov,
        179.9
    );

    let mut ordinary = scene.clone();
    ordinary["general"]["orthogonalprojection"] = json!({"width":1920,"height":1080});
    assert!(super::Camera::of(&ordinary, &props, super::ProjectionMode::of(&ordinary)).is_none());
    let mut zero = scene.clone();
    zero["general"]["orthogonalprojection"] = json!({"width":0,"height":0});
    assert!(super::Camera::of(&zero, &props, super::ProjectionMode::of(&zero)).is_some());
    zero["camera"]["up"] = json!("0 0 -1");
    assert!(super::Camera::of(&zero, &props, super::ProjectionMode::of(&zero)).is_none());
}

#[test]
fn camera_defaults_missing_fields() {
    let props = Properties::new();
    let mut scene = json!({
        "camera":{"eye":"0 0 0","center":"0 0 -1","up":"0 1 0"},
        "general":{}
    });
    for (general, expected) in [
        (json!({}), (50.0, 0.1, 10000.0)),
        (json!({"fov":60}), (60.0, 0.1, 10000.0)),
        (json!({"nearz":0.5}), (50.0, 0.5, 10000.0)),
        (json!({"farz":100}), (50.0, 0.1, 100.0)),
    ] {
        scene["general"] = general;
        let camera = super::Camera::of(&scene, &props, super::ProjectionMode::Other).unwrap();
        assert_eq!((camera.fov, camera.near, camera.far), expected);
    }
    for general in [
        json!({"nearz":0}),
        json!({"nearz":10000}),
        json!({"farz":0.1}),
        json!({"fov":"NaN"}),
        json!({"nearz":null}),
        json!({"farz":"invalid"}),
    ] {
        scene["general"] = general;
        assert!(super::Camera::of(&scene, &props, super::ProjectionMode::Other).is_none());
    }
}

#[test]
fn camera_requires_static_particles() {
    let particle =
        br#"{"renderer":[{"name":"sprite"}],"emitter":[{"name":"sphererandom","rate":100}]}"#;
    let scene = json!({
        "camera":{"eye":"0 0 0","center":"0 0 -1","up":"0 1 0"},
        "general":{"fov":50,"nearz":0.01,"farz":10000},
        "objects":[{"id":1,"particle":"particles/root.json","origin":"0 0 -5"}]
    });
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let load = |scene: &Value, definition: &[u8]| {
        let source = scene.to_string();
        let package = crate::pkg::Package::parse(build_pkg(&[
            ("scene.json", source.as_bytes()),
            ("particles/root.json", definition),
        ]))
        .unwrap();
        super::load_with(&package, &assets).unwrap()
    };
    assert!(load(&scene, particle).camera.is_some());
    let mut singular = scene.clone();
    singular["objects"][0]["scale"] = json!("0 1 1");
    assert!(load(&singular, particle).camera.is_none());
    singular["objects"][0]["scale"] = json!("1e-8 1 1");
    assert!(load(&singular, particle).camera.is_none());
    singular["objects"][0]["scale"] = json!(format!("{} 1 1", f32::EPSILON));
    let boundary = load(&singular, particle);
    assert!(boundary.camera.is_some());
    assert_eq!(boundary.particles[0].system.scale3[0], f32::EPSILON);
    for (field, value) in
        [("parent", json!(2)), ("angles", json!("1 0 0")), ("angles", json!("0 1 0"))]
    {
        let mut transformed = scene.clone();
        transformed["objects"][0][field] = value;
        assert!(load(&transformed, particle).camera.is_none());
    }
    let world = br#"{"flags":1,"renderer":[{"name":"sprite"}],"emitter":[{"name":"sphererandom","rate":100}]}"#;
    assert!(load(&scene, world).camera.is_none());
    for renderer in ["spritetrail", "rope"] {
        let definition = format!(
            r#"{{"renderer":[{{"name":"{renderer}"}}],"emitter":[{{"name":"sphererandom","rate":100}}]}}"#
        );
        assert!(load(&scene, definition.as_bytes()).camera.is_none());
    }
    for orientation in ["upright", "fixed"] {
        let definition = format!(
            r#"{{"renderer":[{{"name":"sprite","orientation":"{orientation}"}}],"emitter":[{{"name":"sphererandom","rate":100}}]}}"#
        );
        assert!(load(&scene, definition.as_bytes()).camera.is_none());
    }
    for flags in [1, 2] {
        let definition = format!(
            r#"{{"renderer":[{{"name":"sprite","flags":{flags}}}],"emitter":[{{"name":"sphererandom","rate":100}}]}}"#
        );
        assert!(load(&scene, definition.as_bytes()).camera.is_none());
    }
    let mut orthographic = scene.clone();
    orthographic["general"]["orthogonalprojection"] = json!({"width":1920,"height":1080});
    assert!(load(&orthographic, particle).camera.is_none());
    let mut empty = scene.clone();
    empty["objects"] = json!([]);
    empty["general"]["orthogonalprojection"] = json!({"width":0,"height":0});
    assert!(load(&empty, particle).camera.is_some());
    let flagged = br#"{"flags":4,"renderer":[{"name":"sprite"}],"emitter":[{"name":"sphererandom","rate":100}]}"#;
    assert!(load(&scene, flagged).camera.is_none());
    let pointed = br#"{"renderer":[{"name":"sprite"}],"emitter":[{"name":"sphererandom","rate":100}],"controlpoint":[{"id":0,"flags":1}]}"#;
    assert!(load(&scene, pointed).camera.is_none());
    let mut dynamic = scene.clone();
    dynamic["general"]["cameraparallax"] = json!(true);
    assert!(load(&dynamic, particle).camera.is_none());
    let mut shaken = scene.clone();
    shaken["general"]["camerashake"] = json!(true);
    assert!(load(&shaken, particle).camera.is_some());
    for field in ["sound", "model", "camera"] {
        let mut mixed = scene.clone();
        mixed["objects"][0][field] = json!("other");
        assert!(load(&mixed, particle).camera.is_none());
    }
    let mut extra = scene.clone();
    extra["objects"].as_array_mut().unwrap().push(json!({"id":2,"name":"camera object"}));
    assert!(load(&extra, particle).camera.is_none());
}

fn particle_model(definition: &str, child: Option<(&str, &str)>) -> super::SceneModel {
    let scene = br#"{"objects":[{"id":1,"name":"Sparks","particle":"particles/root.json"}]}"#;
    let mut files =
        vec![("scene.json", scene.as_slice()), ("particles/root.json", definition.as_bytes())];
    if let Some((path, definition)) = child {
        files.push((path, definition.as_bytes()));
    }
    let pkg = crate::pkg::Package::parse(build_pkg(&files)).unwrap();
    super::load_with(&pkg, &crate::effects::Assets::discover(Some("/nonexistent"))).unwrap()
}

#[test]
fn projection_defaults_reach_children() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("shaders")).unwrap();
    let preset = root.path().join("presets/preset_child/particles");
    std::fs::create_dir_all(&preset).unwrap();
    let child = br#"{"renderer":[{"name":"sprite"}],"emitter":[{"name":"boxrandom"}],"operator":[{"name":"collisionsphere"},{"name":"capvelocity","maxspeed":0}]}"#;
    std::fs::write(preset.join("preset_child.json"), child).unwrap();
    let parent = br#"{"renderer":[{"name":"sprite"}],"emitter":[{"name":"boxrandom"}],"operator":[{"name":"capvelocity"},{"name":"collisionplane"}],"children":[{"name":"particles/preset_child.json"}]}"#;
    for (projection, speed, distance, origin, radius) in [
        (json!({"auto":true}), 100.0, -150.0, [0.0, -200.0, 0.0], 50.0),
        (json!({"auto":false}), 1.0, 0.0, [0.0; 3], 1.0),
    ] {
        let scene = json!({"general":{"orthogonalprojection":projection},"objects":[
            {"id":1,"name":"Sparks","particle":"particles/root.json"}
        ]})
        .to_string();
        let package = crate::pkg::Package::parse(build_pkg(&[
            ("scene.json", scene.as_bytes()),
            ("particles/root.json", parent),
        ]))
        .unwrap();
        let assets = crate::effects::Assets::discover(Some(root.path().to_str().unwrap()));
        let model = super::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let system = &model.particles[0].system;
        let crate::particles::Operator::CapVelocity { max_speed, .. } = system.operators[0] else {
            panic!("cap")
        };
        assert_eq!(max_speed, speed);
        let crate::particles::Operator::CollisionPlane { distance: actual, .. } =
            system.operators[1]
        else {
            panic!("plane")
        };
        assert_eq!(actual, distance);
        let child = &system.children[0];
        let crate::particles::Operator::CollisionSphere { origin: actual, radius: size, .. } =
            child.operators[0]
        else {
            panic!("sphere")
        };
        assert_eq!((actual, size), (origin, radius));
        let crate::particles::Operator::CapVelocity { max_speed, .. } = child.operators[1] else {
            panic!("child cap")
        };
        assert_eq!(max_speed, 0.0);
    }
}

#[test]
fn unsupported_operator_reported() {
    let model = particle_model(
        r#"{"renderer":[{"name":"sprite"}],"emitter":[{"name":"boxrandom"}],"operator":[{"name":"movement"},{"name":"unknownmotion"}]}"#,
        None,
    );
    assert_eq!(model.particles.len(), 1);
    assert_eq!(model.particles[0].system.operators.len(), 1);
    assert_eq!(
        model.skipped,
        ["Sparks: particle particles/root.json: operator 1 (unknownmotion) unsupported"]
    );
}

#[test]
fn emitterless_parent_keeps_children() {
    let model = particle_model(
        r#"{"emitter":[],"renderer":[],"children":[{"name":"particles/child.json"}]}"#,
        Some(("particles/child.json", r#"{"emitter":[{"name":"boxrandom"}]}"#)),
    );
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let system = &model.particles[0].system;
    assert!(system.emitters.is_empty());
    assert!(system.texture.is_none());
    assert_eq!(system.children[0].emitters.len(), 1);
}

#[test]
fn emitterless_leaf_reported() {
    for definition in [r#"{"emitter":[]}"#, r#"{"renderer":[{"name":"sprite"}]}"#] {
        let model = particle_model(definition, None);
        assert!(model.particles.is_empty(), "{definition}");
        assert_eq!(
            model.skipped,
            ["Sparks: particle particles/root.json: no supported emitters"],
            "{definition}"
        );
    }
    let model = particle_model(
        r#"{"children":[{"name":"particles/child.json"}]}"#,
        Some(("particles/child.json", r#"{"emitter":[{"name":"boxrandom"}]}"#)),
    );
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
}

#[test]
fn rejected_steps_reported() {
    let model = particle_model(
        r#"{"renderer":[{"name":"unknownrenderer"},{"name":"sprite"}],"emitter":[{"name":"unknownburst"}],"initializer":[{"name":"unknownseed"}],"operator":[{"name":"unknownmotion"}]}"#,
        None,
    );
    assert!(model.particles.is_empty());
    assert_eq!(
        model.skipped,
        [
            "Sparks: particle particles/root.json: renderer 0 (unknownrenderer) unsupported",
            "Sparks: particle particles/root.json: emitter 0 (unknownburst) unsupported",
            "Sparks: particle particles/root.json: initializer 0 (unknownseed) unsupported",
            "Sparks: particle particles/root.json: operator 0 (unknownmotion) unsupported",
            "Sparks: particle particles/root.json: no supported emitters",
        ]
    );
}

#[test]
fn mixed_emitters_reported() {
    let model = particle_model(
        r#"{"renderer":[{"name":"sprite"},{"name":"unrendered"}],"emitter":[{"name":"boxrandom"},{"name":"unknownburst"}],"initializer":[{"name":"lifetimerandom"},{"name":"unknownseed"}]}"#,
        None,
    );
    assert_eq!(model.particles.len(), 1);
    assert_eq!(model.particles[0].system.emitters.len(), 1);
    assert_eq!(model.particles[0].system.initializers.len(), 1);
    assert_eq!(
        model.skipped,
        [
            "Sparks: particle particles/root.json: renderer 1 (unrendered) unsupported",
            "Sparks: particle particles/root.json: emitter 1 (unknownburst) unsupported",
            "Sparks: particle particles/root.json: initializer 1 (unknownseed) unsupported",
        ]
    );
}

#[test]
fn child_budget_reported() {
    let children = vec![json!({"name": "particles/child.json"}); 300];
    let root = json!({"renderer":[{"name":"sprite"}],"emitter":[{"name":"boxrandom"}],"children":children});
    let model = particle_model(
        &root.to_string(),
        Some((
            "particles/child.json",
            r#"{"renderer":[{"name":"sprite"}],"emitter":[{"name":"boxrandom"}]}"#,
        )),
    );
    assert_eq!(model.particles[0].system.children.len(), 255);
    assert_eq!(model.skipped.len(), 45);
    assert_eq!(
        model.skipped[0],
        "Sparks: particle particles/root.json: child 255 (particles/child.json) over the 256 system budget"
    );
}

#[test]
fn child_loss_reported() {
    let model = particle_model(
        r#"{"renderer":[{"name":"sprite"}],"emitter":[{"name":"boxrandom"}],"children":[{"name":"particles/child.json"},{"name":"particles/missing.json"}]}"#,
        Some((
            "particles/child.json",
            r#"{"renderer":[{"name":"unknownrenderer"}],"emitter":[{"name":"boxrandom"}]}"#,
        )),
    );
    assert_eq!(model.particles.len(), 1);
    assert!(model.particles[0].system.children.is_empty());
    assert_eq!(
        model.skipped,
        [
            "Sparks: particle particles/child.json: renderer 0 (unknownrenderer) unsupported",
            "Sparks: particle particles/root.json: child 0 (particles/child.json) unsupported",
            "Sparks: particle particles/root.json: child 1 (particles/missing.json) unsupported",
        ]
    );
}

#[test]
fn child_steps_reported() {
    let model = particle_model(
        r#"{"emitter":[{"name":"boxrandom"}],"children":[{"name":"particles/child.json"}]}"#,
        Some((
            "particles/child.json",
            r#"{"emitter":[{"name":"boxrandom"}],"operator":[{"name":"unknownmotion"}]}"#,
        )),
    );
    assert_eq!(model.particles[0].system.children.len(), 1);
    assert_eq!(
        model.skipped,
        ["Sparks: particle particles/child.json: operator 0 (unknownmotion) unsupported"]
    );
}

fn image_package() -> crate::pkg::Package {
    let fallback = build_tex("TEXB0002", 0, 0, &[255; 64], false, None);
    let scene = br#"{"general":{"orthogonalprojection":{"width":100,"height":100}},"objects":[{"id":1,"name":"left","image":"models/left.json","origin":"25 50 0","size":"20 20"},{"id":2,"name":"right","image":"models/right.json","origin":"75 50 0","size":"20 20"}]}"#;
    crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", scene),
        ("models/left.json", br#"{"material":"materials/left.json"}"#),
        ("models/right.json", br#"{"material":"materials/right.json"}"#),
        (
            "materials/left.json",
            br#"{"passes":[{"textures":["fallback"],"usertextures":["customleft"]}]}"#,
        ),
        (
            "materials/right.json",
            br#"{"passes":[{"textures":["fallback"],"usertextures":["customright"]}]}"#,
        ),
        ("materials/fallback.tex", &fallback),
    ]))
    .unwrap()
}

#[test]
fn created_layers_shared() {
    let star = build_tex("TEXB0002", 0, 0, &[255; 64], false, None);
    let create = "thisScene.createLayer({image:'models/star.json'});";
    let script = format!("export function init(){{{create}{create}{create}}}");
    let scene = json!({"general":{"orthogonalprojection":{"width":64,"height":64}},"objects":[{"id":1,"alpha":{"value":1,"script":script}}]}).to_string();
    let package = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", scene.as_bytes()),
        ("models/star.json", br#"{"material":"materials/star.json"}"#),
        ("materials/star.json", br#"{"passes":[{"textures":["star"]}]}"#),
        ("materials/star.tex", &star),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    assert_eq!(super::load_with(&package, &assets).unwrap().layers.len(), 3);
}

#[test]
fn textures_keyed_by_file() {
    let root = tempfile::tempdir().unwrap();
    for dir in ["shaders", "materials", "effects/a", "effects/b/materials"] {
        std::fs::create_dir_all(root.path().join(dir)).unwrap();
    }
    std::fs::write(root.path().join("effects/a/effect.json"), b"{}").unwrap();
    std::fs::write(root.path().join("effects/b/effect.json"), b"{}").unwrap();
    let shared = build_tex("TEXB0002", 0, 0, &[1; 64], false, None);
    let local = build_tex("TEXB0002", 0, 0, &[2; 64], false, None);
    std::fs::write(root.path().join("materials/noise.tex"), shared).unwrap();
    std::fs::write(root.path().join("effects/b/materials/noise.tex"), local).unwrap();
    let pkg = crate::pkg::Package::parse(build_pkg(&[("scene.json", b"{}")])).unwrap();
    let assets = crate::effects::Assets::discover(root.path().to_str()).with_texture_cache();
    let load = |assets: &crate::effects::Assets| {
        super::load_texture_or_asset(&pkg, assets, "noise").unwrap().pixels
    };
    let global = load(&assets);
    assert!(std::sync::Arc::ptr_eq(&global, &load(&assets.with_effect("effects/a/effect.json"))));
    assert_ne!(global, load(&assets.with_effect("effects/b/effect.json")));
}

#[test]
fn shared_texture_decoded_once() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let model = super::load_with(&image_package(), &assets).unwrap();
    let [left, right] = &model.layers[..] else { panic!("two layers") };
    assert!(std::sync::Arc::ptr_eq(&left.texture.pixels, &right.texture.pixels));
}

fn image_presets(root: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let base = root.join("1");
    let middle = root.join("2");
    let selected = root.join("3");
    for dir in [&base, &middle, &selected] {
        std::fs::create_dir(dir).unwrap();
    }
    std::fs::write(base.join("project.json"), r#"{"type":"scene","general":{"properties":{"customleft":{"type":"scenetexture","value":""},"customright":{"type":"scenetexture","value":""}}}}"#).unwrap();
    std::fs::write(
        middle.join("project.json"),
        r#"{"dependency":"1","preset":{"customleft":"files/über.jpeg"}}"#,
    )
    .unwrap();
    std::fs::write(
        selected.join("project.json"),
        r#"{"dependency":"2","preset":{"customright":"files/dance.gif"}}"#,
    )
    .unwrap();
    (middle, selected)
}

fn sample_jpeg(color: [u8; 3]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut bytes)
        .encode(&color, 1, 1, image::ExtendedColorType::Rgb8)
        .unwrap();
    bytes
}

fn sample_gif() -> Vec<u8> {
    gif_frames(2)
}

fn gif_frames(count: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    let frames = (0..count).map(|index| {
        let (ms, color) =
            if index % 2 == 0 { (30, [0, 250, 0, 255]) } else { (70, [0, 0, 250, 255]) };
        image::Frame::from_parts(
            image::RgbaImage::from_pixel(2, 2, image::Rgba(color)),
            0,
            0,
            image::Delay::from_numer_denom_ms(ms, 1),
        )
    });
    image::codecs::gif::GifEncoder::new(&mut bytes).encode_frames(frames).unwrap();
    bytes
}

#[test]
fn oversized_gif_dimensions_rejected() {
    let mut bytes = sample_gif();
    bytes[6..8].copy_from_slice(&(super::tex::MAX_TEXTURE_EDGE as u16 + 1).to_le_bytes());
    assert!(super::load_user_texture(&bytes).is_none());
}

#[test]
fn gif_frame_budget_rejected() {
    let mut bytes = gif_frames(5);
    bytes[6..8].copy_from_slice(&4096u16.to_le_bytes());
    bytes[8..10].copy_from_slice(&4096u16.to_le_bytes());
    assert!(super::load_user_texture(&bytes).is_none());
}

#[test]
fn preset_images_bind_materials() {
    let root = tempfile::tempdir().unwrap();
    let (middle, selected) = image_presets(root.path());
    std::fs::create_dir(middle.join("files")).unwrap();
    std::fs::create_dir(selected.join("files")).unwrap();
    std::fs::write(middle.join("files/über.jpeg"), sample_jpeg([240, 20, 10])).unwrap();
    std::fs::write(selected.join("files/dance.gif"), sample_gif()).unwrap();
    let model = super::load_from_dir(&image_package(), &selected).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let left = &model.layers[0].texture;
    assert_eq!((left.width, left.height), (1, 1));
    assert!(left.pixels.base_rgba().unwrap()[0] > 100);
    let right = &model.layers[1].texture;
    assert_eq!((right.width, right.height), (2, 2));
    assert_eq!((right.frames.len(), right.pages.len()), (2, 1));
    assert_eq!(right.frames.iter().map(|frame| frame.time).collect::<Vec<_>>(), [0.03, 0.07]);
    assert!(right.pixels.base_rgba().unwrap()[1] > 200);
    assert!(right.pages[0].base_rgba().unwrap()[2] > 200);
}

#[test]
fn selected_image_wins_chain() {
    let root = tempfile::tempdir().unwrap();
    let (middle, selected) = image_presets(root.path());
    std::fs::create_dir(middle.join("files")).unwrap();
    std::fs::create_dir(selected.join("files")).unwrap();
    std::fs::write(middle.join("files/über.jpeg"), sample_jpeg([240, 20, 10])).unwrap();
    std::fs::write(selected.join("files/über.jpeg"), sample_jpeg([10, 20, 240])).unwrap();
    std::fs::write(
        selected.join("project.json"),
        r#"{"dependency":"2","preset":{"customleft":"files/über.jpeg"}}"#,
    )
    .unwrap();
    let model = super::load_from_dir(&image_package(), &selected).unwrap();
    assert!(model.layers[0].texture.pixels.base_rgba().unwrap()[2] > 100);
}

#[test]
fn empty_image_uses_default() {
    let root = tempfile::tempdir().unwrap();
    let (_, selected) = image_presets(root.path());
    std::fs::write(
        selected.join("project.json"),
        r#"{"dependency":"2","preset":{"customleft":""}}"#,
    )
    .unwrap();
    let model = super::load_from_dir(&image_package(), &selected).unwrap();
    assert_eq!(model.layers[0].texture.width, 4);
}

#[test]
fn lenient_base_image_loads() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("files")).unwrap();
    std::fs::write(root.path().join("files/picture.jpeg"), sample_jpeg([240, 20, 10])).unwrap();
    std::fs::write(root.path().join("project.json"), r#"{"type":"scene","general":{"properties":{"customleft":{"type":"scenetexture","value":"files/picture.jpeg",},}},}"#).unwrap();
    let model = super::load_from_dir(&image_package(), root.path()).unwrap();
    assert_eq!(model.layers[0].texture.width, 1);
    assert!(model.layers[0].texture.pixels.base_rgba().unwrap()[0] > 100);
}

#[test]
fn escaped_preset_uses_default() {
    let root = tempfile::tempdir().unwrap();
    let (_, selected) = image_presets(root.path());
    std::fs::write(
        selected.join("project.json"),
        r#"{"dependency":"2","preset":{"customright":"../outside.gif"}}"#,
    )
    .unwrap();
    std::fs::write(root.path().join("outside.gif"), sample_gif()).unwrap();
    let model = super::load_from_dir(&image_package(), &selected).unwrap();
    let right = &model.layers[1].texture;
    assert_eq!((right.width, right.height), (4, 4));
    assert!(right.frames.is_empty());
}

#[cfg(unix)]
#[test]
fn linked_preset_uses_default() {
    let root = tempfile::tempdir().unwrap();
    let (_, selected) = image_presets(root.path());
    std::fs::create_dir(selected.join("files")).unwrap();
    std::fs::write(root.path().join("outside.gif"), sample_gif()).unwrap();
    std::os::unix::fs::symlink(root.path().join("outside.gif"), selected.join("files/dance.gif"))
        .unwrap();
    let model = super::load_from_dir(&image_package(), &selected).unwrap();
    assert_eq!(model.layers[1].texture.width, 4);
}

#[test]
fn oversized_preset_uses_default() {
    let root = tempfile::tempdir().unwrap();
    let (_, selected) = image_presets(root.path());
    std::fs::create_dir(selected.join("files")).unwrap();
    let file = std::fs::File::create(selected.join("files/dance.gif")).unwrap();
    file.set_len((super::MAX_USER_TEXTURE_FILE_BYTES + 1) as u64).unwrap();
    let model = super::load_from_dir(&image_package(), &selected).unwrap();
    assert_eq!(model.layers[1].texture.width, 4);
}

#[test]
fn preset_scene_load_uses_parent_then_preset_then_explicit_properties() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("1");
    let preset = root.path().join("2");
    std::fs::create_dir(&base).unwrap();
    std::fs::create_dir(&preset).unwrap();
    std::fs::write(
        base.join("project.json"),
        r#"{"type":"scene","general":{"properties":{"tint":{"type":"color","value":"1 0 0"}}}}"#,
    )
    .unwrap();
    std::fs::write(preset.join("project.json"), r#"{"dependency":"1","preset":{"tint":"0 1 0"}}"#)
        .unwrap();
    let scene =
        json!({"general":{"clearcolor":{"user":"tint","value":"1 1 1"}},"objects":[]}).to_string();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(b"PKGV0007");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&10u32.to_le_bytes());
    bytes.extend_from_slice(b"scene.json");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(scene.len() as u32).to_le_bytes());
    bytes.extend_from_slice(scene.as_bytes());
    let package = crate::pkg::Package::parse(bytes).unwrap();
    assert_eq!(super::load_from_dir(&package, &base).unwrap().clear, [1.0, 0.0, 0.0]);
    assert_eq!(super::load_from_dir(&package, &preset).unwrap().clear, [0.0, 1.0, 0.0]);
    let properties = Properties::from([("tint".into(), vec![0.0, 0.0, 1.0])]);
    assert_eq!(
        super::load_from_dir_with(&package, &preset, &properties).unwrap().clear,
        [0.0, 0.0, 1.0]
    );
}

fn scene(amount: f64) -> Value {
    json!({"general": {"cameraparallax": true, "cameraparallaxamount": amount}})
}

fn by_id(objects: &[Value]) -> std::collections::HashMap<String, &Value> {
    objects.iter().filter_map(|object| Some((object.get("id")?.to_string(), object))).collect()
}

#[test]
fn parallax_pushes_a_layer_away_from_the_canvas_centre_by_amount_times_depth() {
    let props = Properties::new();
    let canvas = (1000.0, 800.0);
    let parallax = Parallax::of(&scene(0.5), canvas, &props).expect("enabled");
    let object = json!({"id": 1, "origin": "700 500 0", "parallaxDepth": "2 0.5"});
    let objects = [object.clone()];
    let placed = resolve_transform(
        &object,
        &by_id(&objects),
        &props,
        Some(&parallax),
        &crate::model::Attachments::new(),
    );
    assert!((placed.origin.0 - (700.0 + (700.0 - 500.0) * 0.5 * 2.0)).abs() < 1e-3);
    assert!((placed.origin.1 - (500.0 + (500.0 - 400.0) * 0.5 * 0.5)).abs() < 1e-3);
}

#[test]
fn a_layer_at_the_focus_point_and_a_layer_without_depth_do_not_move() {
    let props = Properties::new();
    let canvas = (1000.0, 800.0);
    let parallax = Parallax::of(&scene(0.5), canvas, &props).expect("enabled");
    let centred = json!({"id": 1, "origin": "500 400 0", "parallaxDepth": "3 3"});
    let depthless = json!({"id": 2, "origin": "700 500 0"});
    for object in [&centred, &depthless] {
        let objects = [(*object).clone()];
        let base = resolve_transform(
            object,
            &by_id(&objects),
            &props,
            None,
            &crate::model::Attachments::new(),
        );
        let placed = resolve_transform(
            object,
            &by_id(&objects),
            &props,
            Some(&parallax),
            &crate::model::Attachments::new(),
        );
        assert!((placed.origin.0 - base.origin.0).abs() < 1e-3);
        assert!((placed.origin.1 - base.origin.1).abs() < 1e-3);
    }
}

#[test]
fn a_child_inherits_the_root_ancestors_offset_and_ignores_its_own_depth() {
    let props = Properties::new();
    let canvas = (1000.0, 800.0);
    let parallax = Parallax::of(&scene(0.5), canvas, &props).expect("enabled");
    let root = json!({"id": 1, "origin": "700 400 0", "parallaxDepth": "2 2"});
    let child = json!({"id": 2, "parent": 1, "origin": "10 0 0", "parallaxDepth": "50 50"});
    let objects = [root.clone(), child.clone()];
    let map = by_id(&objects);
    assert_eq!(ancestor_chain(&child, &map).len(), 2);
    let base = resolve_transform(&child, &map, &props, None, &crate::model::Attachments::new());
    let placed =
        resolve_transform(&child, &map, &props, Some(&parallax), &crate::model::Attachments::new());
    assert!((placed.origin.0 - base.origin.0 - (700.0 - 500.0) * 0.5 * 2.0).abs() < 1e-3);
    assert!((placed.origin.1 - base.origin.1).abs() < 1e-3);
}

#[test]
fn parallax_is_off_unless_the_scene_enables_it_and_defaults_to_half_amount() {
    let props = Properties::new();
    let canvas = (1000.0, 800.0);
    assert!(Parallax::of(&json!({"general": {}}), canvas, &props).is_none());
    assert!(Parallax::of(&json!({"general": {"cameraparallax": false}}), canvas, &props).is_none());
    let defaulted =
        Parallax::of(&json!({"general": {"cameraparallax": true}}), canvas, &props).expect("on");
    let object = json!({"id": 1, "origin": "600 400 0", "parallaxDepth": "1 1"});
    assert!((defaulted.offset(&object, &props).0 - 50.0).abs() < 1e-3);
}

fn unequal_tex() -> Vec<u8> {
    let mut bytes = b"TEXV0005\0TEXI0001\0".to_vec();
    for value in [0i32, 4, 4, 2, 4, 2, 0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(b"TEXB0001\0");
    bytes.extend(2i32.to_le_bytes());
    for (width, height, color) in [(4i32, 2i32, [255u8, 0, 0, 255]), (2, 2, [0, 255, 0, 255])] {
        for value in [1, width, height, width * height * 4] {
            bytes.extend(value.to_le_bytes());
        }
        for _ in 0..width * height {
            bytes.extend(color);
        }
    }
    bytes.extend(b"TEXS0003\0");
    for value in [2i32, 2, 2] {
        bytes.extend(value.to_le_bytes());
    }
    for image in [0i32, 1] {
        bytes.extend(image.to_le_bytes());
        let frame = if image == 0 {
            [0.1f32, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0]
        } else {
            [0.7f32, 2.0, 0.0, 0.0, 2.0, -2.0, 0.0]
        };
        for value in frame {
            bytes.extend(value.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn atlas_pages_preserved() {
    let bytes = unequal_tex();
    let texture = super::load_texture_bytes(&bytes).unwrap();
    assert_eq!(texture.pages.len(), 1);
    assert_eq!(texture.payload_bytes(), 48);
    let frames = texture.atlas_frames().unwrap();
    assert_eq!(frames[0].uv, [0.0, 0.0, 0.5, 1.0]);
    assert_eq!(frames[1].uv, [1.0, 0.0, 1.0, 1.0]);
    assert_eq!(frames[1].axes, [0.0, 1.0, -1.0, 0.0]);
    assert!(frames[1].rotated);
    assert_eq!(frames[1].image, 1);
    assert_eq!(texture.pages[0].base_rgba().unwrap(), [0, 255, 0, 255].repeat(4));
}

#[test]
fn text_atlas_charges_budget() {
    let mut texture = super::load_texture_bytes(&unequal_tex()).unwrap();
    let flat = texture.payload_bytes();
    let mut admitted = super::MAX_SCENE_TEXTURE_BYTES - flat;
    super::add_texture_bytes(&mut admitted, flat).unwrap();
    texture.text_batch = Some(crate::text::batch::Batch {
        canvas: [2, 3],
        pixels: crate::tex::Pixels::rgba(2, 3, vec![0; 24]),
        quads: Vec::new(),
    });
    assert_eq!(texture.payload_bytes(), flat + 24);
    let mut rejected = super::MAX_SCENE_TEXTURE_BYTES - flat;
    assert!(super::add_texture_bytes(&mut rejected, texture.payload_bytes()).is_err());
}

#[test]
fn sprite_info_at_init() {
    let scene = json!({"objects":[
        {"id":1,"name":"sprite","image":"models/sprite.json"},
        {"id":2,"name":"plain","image":"models/plain.json"},
        {"id":3,"name":"probe","alpha":{"value":1,"script":r"
export function init() {
    const sprite = thisScene.getLayer('sprite').getTextureAnimation();
    const plain = thisScene.getLayer('plain').getTextureAnimation();
    thisLayer.name = [sprite.frameCount, Math.abs(sprite.duration - 0.8) < 0.001, sprite.getFrame(),
        sprite.isPlaying(), plain === undefined].join(',');
}"}}
    ]});
    let source = scene.to_string();
    let texture = unequal_tex();
    let package = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", source.as_bytes()),
        ("models/sprite.json", br#"{"material":"materials/sprite.json"}"#),
        ("models/plain.json", br#"{"material":"materials/plain.json"}"#),
        ("materials/sprite.json", br#"{"passes":[{"shader":"genericimage2","combos":{"SPRITESHEET":1,"VERSION":2},"textures":["materials/textures/unequal"]}]}"#),
        ("materials/plain.json", br#"{"passes":[{"shader":"genericimage2","combos":{"VERSION":2},"textures":["materials/textures/unequal"]}]}"#),
        ("materials/textures/unequal.tex", &texture),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    assert_eq!(super::sprite_metadata(&scene, &package, &assets), [(0, vec![0.1, 0.7])]);
    let model = super::load_with(&package, &assets).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let scripts = model.scripts.unwrap();
    assert!(scripts.diagnostics.is_empty(), "{:?}", scripts.diagnostics);
    assert_eq!(scripts.scene["objects"][2]["name"], "2,true,0,true,true");
}

#[test]
fn hidden_script_layer_preserves_effect_source_alpha() {
    let mut scene = json!({"objects":[{"id":1,"visible":false},{"id":2,"parent":1,"color":"0.2 0.4 0.6","alpha":0.5}]});
    let layout = super::script::Layout {
        id: "2".into(),
        size: [64.0; 2],
        offset: [0.0; 2],
        text: false,
        hidden_without_fx: false,
        puppet: false,
        fullscreen: false,
    };
    let frame = super::script::frames(
        &scene,
        std::slice::from_ref(&layout),
        (64.0, 64.0),
        &Properties::new(),
        &crate::model::Attachments::new(),
    )
    .remove(0)
    .unwrap();
    assert_eq!(frame.tint, [0.2, 0.4, 0.6, 0.0]);
    assert_eq!(frame.source_tint, [0.2, 0.4, 0.6, 0.5]);
    scene["objects"][0]["visible"] = json!(true);
    let shown = super::script::frames(
        &scene,
        &[layout],
        (64.0, 64.0),
        &Properties::new(),
        &crate::model::Attachments::new(),
    )
    .remove(0)
    .unwrap();
    assert_eq!(shown.tint, frame.source_tint);
    assert_eq!(shown.source_tint, frame.source_tint);
}

#[test]
fn playback_parallax_ignores_saved_editor_camera_offset() {
    let mut scene = scene(1.27);
    scene["camera"] = json!({"eye":"20.05524 289.45834 1", "center":"20.05524 289.45834 0"});
    let props = Properties::new();
    let parallax = Parallax::of(&scene, (3840.0, 2160.0), &props).unwrap();
    let background = json!({"origin":"1920 1080 0", "parallaxDepth":"-1.71 -1.71"});
    assert_eq!(parallax.offset(&background, &props), (0.0, 0.0));
}

#[test]
fn text_defaults_to_unit_depth_for_placement_and_pointer_motion() {
    let props = Properties::new();
    let parallax = Parallax::of(&scene(0.5), (1000.0, 800.0), &props).unwrap();
    let root = json!({"id":1,"text":"Clock", "origin":"700 500 0"});
    let child =
        json!({"id":2,"parent":1,"image":"child.json","origin":"10 0 0","parallaxDepth":"0 0"});
    let objects = [root.clone(), child.clone()];
    let map = by_id(&objects);
    let transform =
        resolve_transform(&child, &map, &props, Some(&parallax), &crate::model::Attachments::new());
    assert_eq!(transform.origin, (810.0, 550.0, 0.0));
    let mouse = crate::mouse::Parallax { amount: 0.5, influence: 1.0, ..Default::default() };
    assert_eq!(
        super::layer_mouse(&child, &map, &props, mouse, transform, false).parallax,
        [1.0; 2]
    );
    let flat = json!({"text":"Clock", "origin":"700 500 0","parallaxDepth":"0 0"});
    assert_eq!(parallax.offset(&flat, &props), (0.0, 0.0));
}

#[test]
fn image_parallax_defaults_to_unit_depth_and_preserves_authored_depth() {
    let props = Properties::new();
    let parallax = Parallax::of(&scene(0.5), (1000.0, 800.0), &props).unwrap();
    let mouse = crate::mouse::Parallax { amount: 0.5, influence: 1.0, ..Default::default() };
    for (depth, expected) in
        [(None, [1.0, 1.0]), (Some("0 0"), [0.0, 0.0]), (Some("-2 0.5"), [-2.0, 0.5])]
    {
        let mut object = json!({"id":1,"image":"image.json","origin":"700 500 0"});
        if let Some(depth) = depth {
            object["parallaxDepth"] = json!(depth);
        }
        let objects = [object.clone()];
        let map = by_id(&objects);
        let transform = resolve_transform(
            &object,
            &map,
            &props,
            Some(&parallax),
            &crate::model::Attachments::new(),
        );
        assert_eq!(
            transform.origin,
            (700.0 + 100.0 * expected[0], 500.0 + 50.0 * expected[1], 0.0)
        );
        assert_eq!(
            super::layer_mouse(&object, &map, &props, mouse, transform, false).parallax,
            expected
        );
        assert_eq!(
            super::layer_mouse(
                &object,
                &map,
                &props,
                crate::mouse::Parallax::default(),
                transform,
                false
            )
            .parallax,
            [0.0; 2]
        );
    }
}

#[test]
fn non_image_parallax_defaults_are_preserved() {
    let props = Properties::new();
    for object in [json!({"text":"Clock"}), json!({"particle":"particle.json"})] {
        assert_eq!(super::parallax_depth(&object, &props), (1.0, 1.0));
    }
    for object in [json!({}), json!({"sound":"sound.json"})] {
        assert_eq!(super::parallax_depth(&object, &props), (0.0, 0.0));
    }
}

#[test]
fn child_inherits_omitted_image_parent_depth() {
    let props = Properties::new();
    let parallax = Parallax::of(&scene(0.5), (1000.0, 800.0), &props).unwrap();
    let root = json!({"id":1,"image":"parent.json","origin":"700 500 0"});
    let child =
        json!({"id":2,"parent":1,"image":"child.json","origin":"10 0 0","parallaxDepth":"0 0"});
    let objects = [root, child.clone()];
    let map = by_id(&objects);
    let transform =
        resolve_transform(&child, &map, &props, Some(&parallax), &crate::model::Attachments::new());
    assert_eq!(transform.origin, (810.0, 550.0, 0.0));
    let mouse = crate::mouse::Parallax { amount: 0.5, influence: 1.0, ..Default::default() };
    assert_eq!(
        super::layer_mouse(&child, &map, &props, mouse, transform, false).parallax,
        [1.0; 2]
    );
}

#[test]
fn shared_image_parser() {
    let package = image_package();
    let mut scene = package.scene_json().unwrap();
    scene["objects"].as_array_mut().unwrap().push(json!({
        "id":"@dynamic-2", "name":"child", "image":"models/right.json", "parent":1,
        "origin":"10 0 0", "size":"32 16", "color":"0 1 1"
    }));
    let resources = super::ImageResources::new(
        &package,
        &crate::effects::Assets::discover(Some("/nonexistent")),
    );
    drop(package);
    let image = resources.load(&scene, 2).unwrap().layer;
    assert_eq!(image.id, "@dynamic-2");
    assert_eq!(image.scene_order, 2);
    assert_eq!(image.center, (35.0, 50.0));
    assert_eq!(image.size, (32.0, 16.0));
    assert_eq!(image.color, [0.0, 1.0, 1.0]);
    assert_eq!(image.texture.payload_bytes(), 64);
}

#[test]
fn image_uv_flags() {
    let texture = build_tex("TEXB0002", 0, 0, &[255; 64], false, None);
    let scene = br#"{"general":{"orthogonalprojection":{"width":100,"height":100}},"objects":[{"id":1,"image":"models/image.json"}]}"#;
    for (field, padded) in [
        (None, true),
        (Some("nopadding"), false),
        (Some("fullscreen"), false),
        (Some("passthrough"), false),
        (Some("solidlayer"), false),
    ] {
        let mut model = json!({"material":"materials/image.json"});
        if let Some(field) = field {
            model[field] = json!(true);
        }
        let model = model.to_string();
        let package = crate::pkg::Package::parse(build_pkg(&[
            ("scene.json", scene),
            ("models/image.json", model.as_bytes()),
            ("materials/image.json", br#"{"passes":[{"textures":["image"]}]}"#),
            ("image.tex", &texture),
        ]))
        .unwrap();
        let assets = crate::effects::Assets::discover(Some("/nonexistent"));
        assert_eq!(super::model_uv_padding(&package, &assets, "models/image.json"), padded);
        let model = super::load_with(&package, &assets).unwrap();
        let resources = super::ImageResources::new(&package, &assets);
        let dynamic = resources.load(&package.scene_json().unwrap(), 0).unwrap().layer;
        assert_eq!(model.layers[0].pad_uv, padded, "{field:?}");
        assert_eq!(dynamic.pad_uv, padded, "{field:?}");
    }
}

#[test]
fn image_uv_extent() {
    let mut layer =
        super::load_with(&image_package(), &crate::effects::Assets::discover(Some("/nonexistent")))
            .unwrap()
            .layers
            .remove(0);
    layer.texture.width = 8;
    layer.texture.height = 16;
    layer.texture.img_width = 6;
    layer.texture.img_height = 10;
    assert!(
        layer
            .uv()
            .into_iter()
            .zip([0.01875, 0.009_375, 0.7125, 0.60625])
            .all(|(actual, expected)| (actual - expected).abs() < 1e-6)
    );
    layer.pad_uv = false;
    assert_eq!(layer.uv(), [0.0, 0.0, 0.75, 0.625]);
}

#[test]
fn dynamic_image_budget() {
    let package = image_package();
    let scene = package.scene_json().unwrap();
    let resources = super::ImageResources::new(
        &package,
        &crate::effects::Assets::discover(Some("/nonexistent")),
    );
    let image = resources.load(&scene, 0).unwrap().layer;
    assert_eq!(resources.bytes.get(), image.texture.payload_bytes());
    assert!(resources.load(&scene, 0).is_err());
    resources.release(0);
    assert_eq!(resources.bytes.get(), 0);
    resources.reserve(super::MAX_SCENE_TEXTURE_BYTES).unwrap();
    assert!(resources.load(&scene, 1).is_err());
    assert_eq!(resources.bytes.get(), super::MAX_SCENE_TEXTURE_BYTES);
}

#[test]
fn retained_initial_budget() {
    let package = image_package();
    let scene = package.scene_json().unwrap();
    let resources = super::ImageResources::new(
        &package,
        &crate::effects::Assets::discover(Some("/nonexistent")),
    );
    let image = resources.load(&scene, 0).unwrap().layer;
    resources.reserve(image.texture.payload_bytes()).unwrap();
    resources.release(0);
    assert_eq!(resources.bytes.get(), image.texture.payload_bytes());
}

#[test]
fn invalid_atlas_rejected() {
    let source = unequal_tex();
    let frames = source.windows(9).position(|bytes| bytes == b"TEXS0003\0").unwrap() + 21;
    for (offset, value) in
        [(0, 3i32.to_le_bytes()), (4, f32::NAN.to_le_bytes()), (16, 0.0f32.to_le_bytes())]
    {
        let mut bytes = source.clone();
        bytes[frames + offset..frames + offset + value.len()].copy_from_slice(&value);
        assert!(super::load_texture_bytes(&bytes).is_err());
    }
}

#[test]
fn image_size_defaults() {
    let texture = unequal_tex();
    for autosize in [None, Some(false), Some(true)] {
        for explicit in [false, true] {
            let mut object = json!({"id":1,"image":"model.json","scale":"3 4 1"});
            if explicit {
                object["size"] = json!("5 7");
            }
            let scene = json!({"objects":[object]}).to_string();
            let mut model = json!({"material":"material.json"});
            if let Some(flag) = autosize {
                model["autosize"] = json!(flag);
            }
            if autosize == Some(true) {
                model["width"] = json!(11);
                model["height"] = json!(13);
            }
            let model = model.to_string();
            let package = crate::pkg::Package::parse(build_pkg(&[
                ("scene.json", scene.as_bytes()),
                ("model.json", model.as_bytes()),
                (
                    "material.json",
                    br#"{"passes":[{"textures":["sprite"],"combos":{"SPRITESHEET":1}}]}"#,
                ),
                ("sprite.tex", &texture),
            ]))
            .unwrap();
            let model = super::load(&package).unwrap();
            let expected = if autosize == Some(true) {
                (6.0, 8.0)
            } else if explicit {
                (15.0, 28.0)
            } else {
                (3.0, 4.0)
            };
            assert_eq!(model.layers[0].size, expected);
            assert_eq!(
                (model.layers[0].texture.img_width, model.layers[0].texture.img_height),
                (2, 2)
            );
        }
    }
}

#[test]
fn image_autosize_sources() {
    let atlas = unequal_tex();
    let marker = atlas.windows(9).position(|bytes| bytes == b"TEXS0003\0").unwrap();
    let mut sprite = atlas.clone();
    sprite[marker + 13..marker + 17].copy_from_slice(&3i32.to_le_bytes());
    sprite[marker + 17..marker + 21].copy_from_slice(&1i32.to_le_bytes());
    let mut map = atlas[..marker].to_vec();
    map[22..26].copy_from_slice(&0i32.to_le_bytes());
    map[34..38].copy_from_slice(&3i32.to_le_bytes());
    map[38..42].copy_from_slice(&1i32.to_le_bytes());

    for (texture, material) in [
        (&sprite, br#"{"passes":[{"textures":["image"],"combos":{"SPRITESHEET":1}}]}"# as &[u8]),
        (&map, br#"{"passes":[{"textures":["image"]}]}"# as &[u8]),
    ] {
        let package = crate::pkg::Package::parse(build_pkg(&[
            ("scene.json", br#"{"objects":[{"id":1,"image":"model.json","scale":"3 4 1"}]}"#),
            ("model.json", br#"{"material":"material.json","autosize":true}"#),
            ("material.json", material),
            ("image.tex", texture),
        ]))
        .unwrap();
        let model = super::load(&package).unwrap();
        assert_eq!(model.layers[0].size, (9.0, 4.0));
        assert_eq!((model.layers[0].texture.img_width, model.layers[0].texture.img_height), (3, 1));
        assert_eq!((model.layers[0].texture.width, model.layers[0].texture.height), (4, 2));
    }
}

#[test]
fn hdr_preserves_layer_emission() {
    const ULTRA: &str = "ultra";
    const DISPLAY_HDR: &str = "displayhdr";
    for (hdr, bloom, postprocessing, active) in [
        (false, true, None, false),
        (true, false, None, false),
        (true, true, None, false),
        (true, true, Some(ULTRA), true),
        (true, true, Some(DISPLAY_HDR), true),
    ] {
        let scene = json!({
            "general":{"hdr":hdr,"bloom":bloom,"orthogonalprojection":{"postprocessing":postprocessing}},
            "objects":[{"id":1,"image":"model.json","color":"0.25 0.5 1","brightness":4}]
        });
        let source = scene.to_string();
        let texture = build_tex("TEXB0003", 0, 0, &[255; 64], false, None);
        let package = crate::pkg::Package::parse(build_pkg(&[
            ("scene.json", source.as_bytes()),
            ("model.json", br#"{"material":"material.json"}"#),
            ("material.json", br#"{"passes":[{"textures":["pixel"]}]}"#),
            ("pixel.tex", &texture),
        ]))
        .unwrap();
        let model = super::load(&package).unwrap();
        let expected = if active { [1.0, 2.0, 4.0] } else { [0.25, 0.5, 1.0] };
        assert_eq!(model.hdr, active);
        assert_eq!(model.layers[0].color, expected);
        let layout =
            super::script::Layout::new(&model.layers[0], &scene, model.canvas, &Properties::new());
        let frames = super::script::frames(
            &scene,
            &[layout],
            model.canvas,
            &Properties::new(),
            &super::Attachments::new(),
        );
        assert_eq!(&frames[0].as_ref().unwrap().tint[..3], &expected);
    }
}

#[test]
fn deep_parent_transform() {
    let mut objects: Vec<_> = (1..=64)
        .map(|id| {
            let mut object = json!({"id":id,"origin":"2 0 0"});
            if id > 1 {
                object["parent"] = json!(id - 1);
            }
            object
        })
        .collect();
    objects.push(json!({"id":65,"parent":64,"origin":"80 0 0"}));
    let by_id = objects.iter().map(|object| (object["id"].to_string(), object)).collect();
    let transform = resolve_transform(
        &objects[64],
        &by_id,
        &Properties::new(),
        None,
        &super::Attachments::new(),
    );
    assert_eq!(transform.origin, (208.0, 0.0, 0.0));
}

#[test]
fn parent_cycles_end() {
    let objects = [json!({"id":1,"parent":2}), json!({"id":2,"parent":1})];
    let by_id: std::collections::HashMap<_, _> =
        objects.iter().map(|object| (object["id"].to_string(), object)).collect();
    assert_eq!(ancestor_chain(&objects[0], &by_id).len(), 2);
}

#[test]
fn parent_basis_preserves_shear() {
    let diagonal = std::f32::consts::FRAC_1_SQRT_2;
    for scale in [2.0, -2.0, 0.0] {
        let objects = [
            json!({"id":1,"origin":"100 200 3","scale":format!("{scale} 1 1")}),
            json!({"id":2,"parent":1,"angles":format!("0 0 {}",std::f32::consts::FRAC_PI_4)}),
            json!({"id":3,"parent":2,"origin":"10 0 4"}),
        ];
        let map = by_id(&objects);
        let transform = resolve_transform(
            &objects[2],
            &map,
            &Properties::new(),
            None,
            &super::Attachments::new(),
        );
        for (actual, expected) in transform.affine.point([0.0, 10.0, 7.0]).into_iter().zip([
            100.0,
            200.0 + 20.0 * diagonal,
            7.0,
        ]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        assert!((transform.origin.0 - (100.0 + 10.0 * scale * diagonal)).abs() < 0.0001);
        assert_eq!(transform.origin.2, 7.0);
        assert_eq!(transform.affine.inverse().is_some(), scale != 0.0);
    }
}

#[test]
fn image_singular_alignment() {
    let package = image_package();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let resources = super::ImageResources::new(&package, &assets);
    let mut scene = package.scene_json().unwrap();
    scene["objects"][0]["scale"] = json!("0 2 1");
    scene["objects"][1]["parent"] = json!(1);
    scene["objects"][1]["alignment"] = json!("topleft");
    scene["objects"][1]["angles"] = json!(format!("0 0 {}", std::f32::consts::FRAC_PI_4));
    let layer = resources.load(&scene, 1).unwrap().layer;
    let layout = super::script::Layout::new(&layer, &scene, (100.0, 100.0), &Properties::new());
    assert_eq!(layout.size, [20.0, 20.0]);
    assert_eq!(layout.offset, [10.0, 10.0]);
    scene["objects"][0]["scale"] = json!("-3 2 1");
    let reloaded = super::ImageResources::new(&package, &assets).load(&scene, 1).unwrap().layer;
    let frames = super::script::frames(
        &scene,
        &[layout],
        (100.0, 100.0),
        &Properties::new(),
        &super::Attachments::new(),
    );
    assert_eq!(frames[0].as_ref().unwrap().geometry, reloaded.geometry);
}

#[test]
fn retains_hidden_emitter_source() {
    let scene = br#"{"objects":[{"id":38,"shape":"quad","visible":false},{"id":39,"shape":"quad","visible":false},{"id":19,"particle":"particles/source.json","dependencies":[{"type":"emitterimage","id":38,"index":0}]}]}"#;
    let particle = br#"{"renderer":[{"name":"sprite"}],"emitter":[{"name":"layerimage"}]}"#;
    let pkg = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", scene),
        ("particles/source.json", particle),
    ]))
    .unwrap();
    let model =
        super::load_with(&pkg, &crate::effects::Assets::discover(Some("/nonexistent"))).unwrap();
    assert_eq!(model.layers.len(), 1);
    assert_eq!(model.layers[0].id, "38");
    assert!(!model.layers[0].visible);
    assert_eq!(model.image_sources, std::collections::BTreeSet::from(["38".to_owned()]));
}

#[test]
fn child_inherits_root_sources() {
    let scene = br#"{"objects":[{"id":38,"shape":"quad","visible":false},{"id":39,"shape":"quad","visible":false},{"id":40,"shape":"quad","visible":false},{"id":19,"particle":"particles/root.json","dependencies":[{"type":"emitterimage","id":38,"index":0},{"type":"emitterimage","id":39,"index":1}]}]}"#;
    let root = br#"{"renderer":[{"name":"sprite"}],"emitter":[{"name":"boxrandom"}],"children":[{"name":"particles/child.json","dependencies":[{"type":"emitterimage","id":40,"index":0}]}]}"#;
    let child = br#"{"renderer":[{"name":"sprite"}],"emitter":[{"name":"boxrandom"},{"name":"layerimage"},{"name":"layerimage"}]}"#;
    let pkg = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", scene),
        ("particles/root.json", root),
        ("particles/child.json", child),
    ]))
    .unwrap();
    let model =
        super::load_with(&pkg, &crate::effects::Assets::discover(Some("/nonexistent"))).unwrap();
    assert_eq!(model.layers.len(), 2);
    assert_eq!(model.layers[0].id, "38");
    assert_eq!(model.layers[1].id, "39");
    assert_eq!(
        model.image_sources,
        std::collections::BTreeSet::from(["38".to_owned(), "39".to_owned()])
    );
    let child = &model.particles[0].system.children[0];
    assert_eq!(child.image_dependencies, model.particles[0].system.image_dependencies);
    let ordinals: Vec<_> = child
        .emitters
        .iter()
        .filter_map(|emitter| match emitter {
            crate::particles::Emitter::Image { image, .. } => Some(image.ordinal),
            _ => None,
        })
        .collect();
    assert_eq!(ordinals, [0, 1]);
}

#[test]
fn ignores_child_sources() {
    let scene = br#"{"objects":[{"id":38,"shape":"quad","visible":false},{"id":19,"particle":"particles/root.json"}]}"#;
    let root = br#"{"renderer":[{"name":"sprite"}],"emitter":[{"name":"boxrandom"}],"children":[{"name":"particles/child.json","dependencies":[{"type":"emitterimage","id":38,"index":0}]}]}"#;
    let child = br#"{"renderer":[{"name":"sprite"}],"emitter":[{"name":"layerimage"}]}"#;
    let pkg = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", scene),
        ("particles/root.json", root),
        ("particles/child.json", child),
    ]))
    .unwrap();
    let model =
        super::load_with(&pkg, &crate::effects::Assets::discover(Some("/nonexistent"))).unwrap();
    assert!(model.layers.is_empty());
    assert!(model.image_sources.is_empty());
    assert!(model.particles[0].system.children[0].image_dependencies.is_empty());
}

#[test]
fn orthographic_mesh_admitted() {
    let scene = br#"{"camera":{"eye":"64 32 100","center":"64 32 0","up":"0 1 0"},"general":{"orthogonalprojection":{"width":256,"height":256},"nearz":2,"farz":200},"objects":[{"id":1,"model":"models/mesh.mdl","origin":"128 128 0"}]}"#;
    let package = mesh_package_scene(&json!([{"shader":"mesh"}]), scene);
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.meshes.len(), 1);
    assert_eq!(model.projection, super::ProjectionMode::Orthographic);
    assert!(model.camera.is_none());
    assert_eq!(model.canvas, (256.0, 256.0));
}

#[test]
fn automatic_projection_context() {
    let scene = json!({"general":{"orthogonalprojection":{"auto":true}}});
    let projection = super::ProjectionMode::of(&scene);
    assert_eq!(projection, super::ProjectionMode::Automatic);
    assert!(super::Camera::of(&scene, &super::Properties::new(), projection).is_none());
}

#[test]
fn old_puppet_constraints() {
    let texture = build_tex("TEXB0002", 0, 0, &[255; 64], false, None);
    let package = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", br#"{"general":{"orthogonalprojection":{"width":100,"height":100}},"objects":[{"id":1,"name":"samurai","image":"models/samurai.json","size":"100 100"}]}"#),
        ("models/samurai.json", br#"{"material":"materials/samurai.json","puppet":"models/samurai.mdl"}"#),
        ("models/samurai.mdl", include_bytes!("fixtures/samurai.mdl")),
        ("materials/samurai.json", br#"{"passes":[{"shader":"genericimage4","textures":["fallback"]}]}"#),
        ("materials/fallback.tex", &texture),
    ])).unwrap();
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
}

#[test]
fn puppet_asset_catalog() {
    let bytes = include_bytes!("fixtures/a02/clock-step.mdl");
    let package = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", br#"{"objects":[]}"#),
        ("models/clock.json", br#"{"puppet":"models/clock.mdl"}"#),
        ("models/clock.mdl", bytes),
    ]))
    .unwrap();
    let mesh = super::model_puppet(&package, "models/clock.json").unwrap().unwrap();
    let first = &mesh.animations[0];
    let second = &mesh.animations[1];
    let scene = json!({"objects":[{"id":182,"image":"models/clock.json","animationlayers":[
        {"animation":first.id,"name":"idle","rate":0}
    ]}]});
    let mut playback = super::puppet_playback(&scene, &package, &Properties::new());
    let metadata = serde_json::to_value(playback.bone_descriptors()).unwrap();
    assert_eq!(metadata[0][0], 0);
    assert_eq!(metadata[0][1][0]["name"], mesh.bones[0].name);
    assert_eq!(metadata[0][1][0]["parent"], serde_json::Value::Null);
    let index = playback
        .create(0, &second.name, false, crate::puppet::playback::Transition::default())
        .unwrap();
    assert_eq!(
        playback.descriptors().into_iter().find(|entry| entry.clock == index).unwrap().name,
        second.name
    );
    assert_eq!(playback.states("182").len(), 2);
}

#[test]
fn puppet_bone_transform_script() {
    let package = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", br#"{"objects":[]}"#),
        ("models/clock.json", br#"{"puppet":"models/clock.mdl"}"#),
        ("models/clock.mdl", include_bytes!("fixtures/a02/clock-step.mdl")),
    ]))
    .unwrap();
    let mesh = super::model_puppet(&package, "models/clock.json").unwrap().unwrap();
    let animation = &mesh.animations[0];
    let base = crate::puppet::palette(
        &mesh,
        crate::puppet::AnimationLayer {
            visible: true,
            id: animation.id,
            rate: 0.0,
            blend: 1.0,
            additive: false,
        },
        0.0,
    )
    .unwrap();
    let script = r"
let count = 0;
export function update(value) {
    if (count++ > 0) return value;
    if (thisLayer.getBoneCount() !== 1) throw Error('bones');
    const world = thisLayer.getBoneTransform(0);
    const before = world.translation().x;
    const local = thisLayer.getLocalBoneTransform(0).translation().x;
    world.translation(world.translation().add(new Vec3(4, 0, 0)));
    thisLayer.setBoneTransform(0, world);
    if (Math.abs(thisLayer.getBoneTransform(0).translation().x - before - 4) > 0.001) throw Error('world');
    if (Math.abs(thisLayer.getLocalBoneTransform(0).translation().x - local) > 0.001) throw Error('local');
    return value;
}
";
    let mut scene = json!({"objects":[{"id":182,"image":"models/clock.json","animationlayers":[
        {"animation":animation.id,"name":"idle","rate":0}
    ],"origin":{"value":"0 0 0","script":script}}]});
    let puppets = super::puppet_playback(&scene, &package, &Properties::new());
    let mut host = crate::script::SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        crate::script::Load { puppets: Some(puppets), ..crate::script::Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let (_, palette) = host.puppet_bone_palette("182").unwrap();
    assert_eq!(palette.len(), 1);
    let crate::puppet::BonePose::Spatial(before) = base[0].unwrap() else { panic!("spatial bone") };
    assert!((palette[0][12] - before[12] - 4.0).abs() < 0.001);
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert!(host.puppet_bone_palette("182").is_none());
}

#[test]
#[ignore = "requires authored puppet probe packages"]
fn authored_puppet_bone_probe() {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_PUPPET_PROBE_DIR").unwrap());
    let palette = |variant: &str| {
        let dir = root.join(variant);
        let package = crate::pkg::Package::open(&dir.join("scene.pkg")).unwrap();
        let model = super::load_from_dir(&package, &dir).unwrap();
        let mut host = model.scripts.unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        host.tick(1.0, 1.0, [0.5; 2]).unwrap();
        host.puppet_bone_palette("30").unwrap().1
    };
    let baseline = palette("baseline");
    let shifted = palette("bone_shift");
    assert_eq!(baseline.len(), shifted.len());
    assert!((shifted[0][12] - baseline[0][12] - 180.0).abs() < 0.001);
}

#[test]
#[ignore = "requires authored puppet event packages"]
fn authored_puppet_event_probe() {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_PUPPET_EVENT_DIR").unwrap());
    let dir = root.join("event");
    let package = crate::pkg::Package::open(&dir.join("scene.pkg")).unwrap();
    let model = super::load_from_dir(&package, &dir).unwrap();
    let mut host = model.scripts.unwrap();
    host.tick(0.1, 0.1, [0.5; 2]).unwrap();
    assert!(!host.scene["objects"][0]["visible"].as_bool().unwrap());
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

const IMAGE_CAMERA_SCENE: &str = r#"{"camera":{"eye":"1 2 -5","center":"0 0 0","up":"0.3 1 0.2"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":1,"image":"models/image.json","origin":"0 0 0","size":"2 2"}]}"#;

fn image_camera_model(scene: &Value, extra: &[(&str, &[u8])]) -> super::SceneModel {
    let texture = build_tex("TEXB0002", 0, 0, &[255; 64], false, None);
    let passes = json!([{"shader":"mesh","depthtest":"enabled","depthwrite":"enabled","cullmode":"nocull","blending":"normal"}]);
    let package = mesh_package_scene(&passes, &serde_json::to_vec(scene).unwrap());
    let mut entries: Vec<_> =
        package.entries().iter().map(|entry| (entry.path.as_str(), package.read(entry))).collect();
    entries.extend([
        ("models/image.json", br#"{"material":"materials/image.json"}"#.as_slice()),
        ("materials/image.json", br#"{"passes":[{"textures":["image"]}]}"#.as_slice()),
        ("materials/image.tex", texture.as_slice()),
    ]);
    entries.extend_from_slice(extra);
    let package = crate::pkg::Package::parse(build_pkg(&entries)).unwrap();
    super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent"))).unwrap()
}

#[test]
fn retains_image_camera() {
    for (projection, retained) in [
        (Value::Null, true),
        (json!({"width":0,"height":0}), true),
        (json!({"width":1920,"height":1080}), false),
        (json!({"auto":true}), false),
    ] {
        let mut scene: Value = serde_json::from_str(IMAGE_CAMERA_SCENE).unwrap();
        scene["general"]["orthogonalprojection"] = projection;
        let model = image_camera_model(&scene, &[]);
        assert!(model.skipped.is_empty());
        assert_eq!(model.layers.len(), 1);
        assert_eq!(model.camera.is_some(), retained);
    }
}

#[test]
fn diagnoses_mixed_camera_images() {
    let mut scene: Value = serde_json::from_str(IMAGE_CAMERA_SCENE).unwrap();
    scene["objects"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":2,"model":"models/mesh.mdl","origin":"0 0 0"}));
    scene["general"]["cameraparallax"] = json!(true);
    let model = image_camera_model(&scene, &[]);
    assert_eq!(model.layers.len(), 1);
    assert!(model.camera.is_some());
    assert!(model.skipped.iter().any(|reason| reason == "authored camera image graph unsupported"));
}

#[test]
fn composed_camera_images() {
    let mut scene: Value = serde_json::from_str(IMAGE_CAMERA_SCENE).unwrap();
    scene["objects"][0]["image"] = json!("models/compose.json");
    scene["objects"][0]["size"] = json!("10 10");
    scene["objects"][0]["effects"] = json!([{"file":"effects/copy.json"}]);
    let model = image_camera_model(
        &scene,
        &[
            ("models/compose.json", br#"{"material":"materials/compose.json","passthrough":true}"#),
            (
                "materials/compose.json",
                br#"{"passes":[{"shader":"composelayer","depthtest":"enabled","textures":["_rt_FullFrameBuffer"]}]}"#,
            ),
            ("effects/copy.json", br#"{"passes":[{"material":"materials/copy.json"}]}"#),
            ("materials/copy.json", br#"{"passes":[{"shader":"copy"}]}"#),
            (
                "shaders/copy.vert",
                b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}",
            ),
            ("shaders/copy.frag", b"void main(){gl_FragColor=vec4(1.0);}"),
        ],
    );
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert!(model.camera.is_some());
    assert!(model.layers[0].passthrough);
    assert!(model.layers[0].material.is_none());
    assert!(model.layers[0].compose_depth);
}

const DEPTH_COMPOSE_FILES: &[(&str, &[u8])] = &[
    ("models/compose.json", br#"{"material":"materials/compose.json","passthrough":true}"#),
    (
        "materials/compose.json",
        br#"{"passes":[{"shader":"composelayer","depthtest":"enabled","textures":["_rt_FullFrameBuffer"]}]}"#,
    ),
    ("effects/grab.json", br#"{"passes":[{"material":"materials/grab.json"}]}"#),
    ("materials/grab.json", br#"{"passes":[{"shader":"grab","textures":[null,"_rt_FullFrameBuffer"]}]}"#),
    ("shaders/grab.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
    (
        "shaders/grab.frag",
        b"uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5))+texSample2D(g_Texture1,vec2(0.5,0.5));}",
    ),
    ("effects/copy.json", br#"{"passes":[{"material":"materials/copy.json"}]}"#),
    (
        "effects/target.json",
        br#"{"fbos":[{"name":"_rt_half","scale":2}],"passes":[{"material":"materials/copy.json","target":"_rt_half"}]}"#,
    ),
    ("materials/copy.json", br#"{"passes":[{"shader":"copy"}]}"#),
    ("shaders/copy.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
    ("shaders/copy.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5));}"),
];

#[test]
fn depth_composite_reported() {
    let grab = json!({"file":"effects/grab.json"});
    let copy = json!({"file":"effects/copy.json"});
    let hidden_grab = json!({"file":"effects/grab.json","visible":false});
    let target = json!({"file":"effects/target.json"});
    let cases = [
        (json!([grab]), 0, true),
        (json!([copy]), 0, false),
        (json!([copy, hidden_grab]), 0, false),
        (json!([copy]), 6, true),
        (json!([target]), 0, true),
    ];
    for (effects, blend, reported) in cases {
        let mut scene: Value = serde_json::from_str(IMAGE_CAMERA_SCENE).unwrap();
        scene["objects"][0]["image"] = json!("models/compose.json");
        scene["objects"][0]["size"] = json!("10 10");
        scene["objects"][0]["colorBlendMode"] = json!(blend);
        scene["objects"][0]["effects"] = effects.clone();
        scene["objects"][0]["alpha"] =
            json!({"value":1.0,"script":"export function update(value){return value;}"});
        let model = image_camera_model(&scene, DEPTH_COMPOSE_FILES);
        assert!(model.layers[0].compose_depth);
        let found = model
            .skipped
            .iter()
            .any(|reason| reason.ends_with("depth-tested effect composite unsupported"));
        assert_eq!(found, reported, "{effects} {blend} {:?}", model.skipped);
    }
}

fn rule_puppet() -> super::Puppet {
    let mesh = crate::puppet::parse(include_bytes!("fixtures/a02/clock-step.mdl")).unwrap();
    super::Puppet {
        cull: false,
        cpu: false,
        planar_owner: true,
        world: crate::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]),
        mesh,
        layers: Vec::new(),
        size: (1.0, 1.0),
        mask_textures: Vec::new(),
    }
}

fn plain(layer: &mut super::Layer) {
    layer.compose_depth = false;
    layer.passthrough = false;
    layer.copy_background = false;
    layer.composition_size = None;
}

#[test]
fn presentation_rules() {
    let mut scene: Value = serde_json::from_str(IMAGE_CAMERA_SCENE).unwrap();
    scene["objects"][0]["image"] = json!("models/compose.json");
    scene["objects"][0]["size"] = json!("10 10");
    scene["objects"][0]["effects"] = json!([{"file":"effects/copy.json"}]);
    let mut screened = scene["objects"][0].clone();
    screened["id"] = json!(2);
    screened["image"] = json!("models/screen.json");
    scene["objects"].as_array_mut().unwrap().push(screened);
    let mut files = DEPTH_COMPOSE_FILES.to_vec();
    files.extend_from_slice(&[
        ("models/screen.json", br#"{"material":"materials/screen.json"}"#.as_slice()),
        (
            "materials/screen.json",
            br#"{"passes":[{"shader":"copy","blending":"screen","depthtest":"enabled","textures":["image"]}]}"#,
        ),
    ]);
    let model = image_camera_model(&scene, &files);
    let screened = &model.layers[1];
    assert!(screened.composite_depth_test());
    assert!(!screened.presents_pass(&screened.effects[0].passes[0], false));
    assert!(screened.depth_composite_dropped());
    let cases: [(&str, fn(&mut super::Layer), bool); 9] = [
        ("compose", |_| {}, true),
        ("plain", plain, true),
        (
            "clock",
            |layer| layer.mouse.clock = Some(crate::mouse::Clock3d { origin: [0.0; 2] }),
            false,
        ),
        ("target", |layer| layer.effects[0].passes[0].target = Some("_rt_half".into()), false),
        (
            "under",
            |layer| {
                layer.effects[0].passes[0]
                    .binds
                    .push((1, crate::effects::EffectBind::SceneUnderLayer));
            },
            false,
        ),
        ("passthrough", |layer| layer.compose_depth = false, false),
        (
            "copy",
            |layer| {
                plain(layer);
                layer.copy_background = true;
            },
            false,
        ),
        (
            "composition",
            |layer| {
                plain(layer);
                layer.composition_size = Some((1.0, 1.0));
            },
            false,
        ),
        ("puppet", |layer| layer.puppet = Some(rule_puppet()), false),
    ];
    for (name, edit, presented) in cases {
        let mut model = image_camera_model(&scene, &files);
        let layer = &mut model.layers[0];
        edit(layer);
        let pass = &layer.effects[0].passes[0];
        assert_eq!(layer.presents_pass(pass, layer.puppet.is_some()), presented, "{name}");
        assert_eq!(
            layer.depth_composite_dropped(),
            layer.composite_depth_test() && !presented,
            "{name}"
        );
    }
}

#[test]
fn scene_input_diagnosed() {
    let mut scene: Value = serde_json::from_str(IMAGE_CAMERA_SCENE).unwrap();
    scene["objects"][0]["image"] = json!("models/grab.json");
    let model = image_camera_model(
        &scene,
        &[
            ("models/grab.json", br#"{"material":"materials/grab.json"}"#),
            (
                "materials/grab.json",
                br#"{"passes":[{"shader":"grab","textures":["_rt_FullFrameBuffer"]}]}"#,
            ),
            (
                "shaders/grab.vert",
                b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}",
            ),
            (
                "shaders/grab.frag",
                b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5));}",
            ),
        ],
    );
    assert!(model.layers[0].base_scene_input);
    assert!(model.skipped.iter().any(|reason| reason == "authored camera image graph unsupported"));
}

#[test]
fn shaken_camera_images() {
    let mut scene: Value = serde_json::from_str(IMAGE_CAMERA_SCENE).unwrap();
    scene["general"]["camerashake"] = json!(true);
    let model = image_camera_model(&scene, &[]);
    assert!(model.camera.is_some());
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
}

#[test]
fn camera_graph_layers() {
    let mut scene: Value = serde_json::from_str(IMAGE_CAMERA_SCENE).unwrap();
    let objects = scene["objects"].as_array_mut().unwrap();
    objects[0]["parent"] = json!(2);
    objects[0]["angles"] = json!("0 0.4 0");
    objects.push(json!({"id":2,"origin":"1 0 0","angles":"0.3 0 0"}));
    let model = image_camera_model(&scene, &[]);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert!(model.camera.is_some());
    let spatial = model.layers[0].geometry.unwrap().spatial.unwrap();
    let expected = crate::matrix::mat4_mul(
        &crate::matrix::model_matrix([1.0, 0.0, 0.0], [1.0; 3], [0.3, 0.0, 0.0]),
        &crate::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0, 0.4, 0.0]),
    );
    for (actual, expected) in spatial.iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-5, "{spatial:?}");
    }
}

#[test]
fn worldspace_tilt_kept() {
    let scene = br#"{"objects":[{"id":1,"name":"Sparks","angles":"0.5 0 0","particle":"particles/root.json"}]}"#;
    let definition =
        br#"{"flags":1,"renderer":[{"name":"sprite"}],"emitter":[{"name":"boxrandom"}]}"#;
    let pkg = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", scene.as_slice()),
        ("particles/root.json", definition.as_slice()),
    ]))
    .unwrap();
    let model =
        super::load_with(&pkg, &crate::effects::Assets::discover(Some("/nonexistent"))).unwrap();
    let system = &model.particles[0].system;
    assert!(system.spatial.is_none());
    assert!(system.world_matrix.is_some());
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
}

#[test]
fn scripts_read_authored() {
    let mut scene: Value = serde_json::from_str(IMAGE_CAMERA_SCENE).unwrap();
    let objects = scene["objects"].as_array_mut().unwrap();
    objects[0]["origin"] = json!({"value":"0 0 0","script":r"
export function update(value){
    value.x = thisScene.getCameraTransforms().eye.x;
    return value;
}"});
    objects.push(json!({"id":2,"camera":"default","origin":"7 0 0"}));
    let mut model = image_camera_model(&scene, &[]);
    assert_eq!(model.camera.unwrap().eye, [7.0, 0.0, 0.0]);
    let host = model.scripts.as_mut().unwrap();
    host.tick(0.1, 0.1, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let origin = super::vec3(host.scene["objects"][0].get("origin"), &super::Properties::new());
    assert_eq!(origin, Some((1.0, 0.0, 0.0)));
}

#[test]
fn zero_scale_untilted() {
    let scene = br#"{"objects":[{"id":1,"name":"Sparks","angles":"0.5 0 0","scale":"0 1 1","particle":"particles/root.json"}]}"#;
    let definition =
        br#"{"flags":1,"renderer":[{"name":"sprite"}],"emitter":[{"name":"boxrandom"}]}"#;
    let pkg = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", scene.as_slice()),
        ("particles/root.json", definition.as_slice()),
    ]))
    .unwrap();
    let model =
        super::load_with(&pkg, &crate::effects::Assets::discover(Some("/nonexistent"))).unwrap();
    let system = &model.particles[0].system;
    assert!(system.world_matrix.is_none());
    assert_eq!(system.scale3, [1.0; 3]);
}

#[test]
fn scripted_camera_images() {
    let mut scene: Value = serde_json::from_str(IMAGE_CAMERA_SCENE).unwrap();
    scene["objects"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":2,"model":"models/mesh.mdl","origin":"0 0 0"}));
    scene["objects"][0]["origin"] =
        json!({"value":"0 0 0","script":"export function update(value){return value;}"});
    let model = image_camera_model(&scene, &[]);
    assert_eq!(model.layers.len(), 1);
    assert_eq!(model.meshes.len(), 1);
    assert!(model.camera.is_some());
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
}

#[test]
fn unread_reflection_idle() {
    let passes = json!([{"shader":"mesh","blending":"normal","textures":["_rt_Reflection"]}]);
    let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[
        {"id":1,"model":"models/mesh.mdl"},
        {"id":2,"particle":"particles/missing.json","reflected":false}
    ]});
    for (fragment, sampled) in [
        (&b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=vec4(1.0);}"[..], false),
        (b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texture2D(g_Texture0,vec2(0.5,0.5));}", true),
    ] {
        let package = shaded_mesh_package(&passes, &serde_json::to_vec(&scene).unwrap(), fragment);
        let assets = crate::effects::Assets::discover(Some("/nonexistent"));
        let model = super::load_with(&package, &assets).unwrap();
        assert!(!model.meshes[0].reflected);
        assert_eq!(model.reflection, super::ReflectionUse { bound: true, sampled });
        assert!(model.unreflected.contains("2"));
    }
}

#[test]
fn combo_gated_read() {
    let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[
        {"id":1,"model":"models/mesh.mdl"}
    ]});
    let fragment = b"uniform sampler2D g_Texture0;\nvoid main(){\n#if REFLECTION\ngl_FragColor=texture2D(g_Texture0,vec2(0.5,0.5));\n#else\ngl_FragColor=vec4(1.0);\n#endif\n}";
    for combo in [0, 1] {
        let passes = json!([{"shader":"mesh","blending":"normal","textures":["_rt_Reflection"],"combos":{"REFLECTION":combo}}]);
        let package = shaded_mesh_package(&passes, &serde_json::to_vec(&scene).unwrap(), fragment);
        let assets = crate::effects::Assets::discover(Some("/nonexistent"));
        let model = super::load_with(&package, &assets).unwrap();
        assert_eq!(model.reflection.sampled, combo == 1);
    }
}

#[test]
fn reflection_membership() {
    let passes = json!([{"shader":"mesh","cullmode":"nocull","blending":"normal"}]);
    for (value, expected) in [
        (None, true),
        (Some(json!(true)), true),
        (Some(json!(false)), false),
        (Some(json!(0)), true),
        (Some(json!("false")), true),
    ] {
        let mut scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":1,"model":"models/mesh.mdl"}]});
        if let Some(value) = value {
            scene["objects"][0]["reflected"] = value;
        }
        let package = mesh_package_scene(&passes, &serde_json::to_vec(&scene).unwrap());
        let model =
            super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
                .unwrap();
        assert!(model.skipped.is_empty());
        assert_eq!(model.meshes[0].reflected, expected);
    }
}

#[test]
fn exact_animation_ids() {
    let props = Properties::new();
    for (raw, expected) in [
        (serde_json::json!(1), Some(1)),
        (serde_json::json!(211), Some(211)),
        (serde_json::json!(16_777_217), Some(16_777_217)),
        (serde_json::json!(u32::MAX), Some(u32::MAX)),
        (serde_json::json!({"value":16_777_217}), Some(16_777_217)),
        (serde_json::json!({"user":"absent","value":16_777_217}), Some(16_777_217)),
        (serde_json::json!(1.5), None),
        (serde_json::json!(-0.5), None),
        (serde_json::json!(4_294_967_296_u64), None),
        (serde_json::json!("NaN"), None),
        (serde_json::json!("inf"), None),
        (serde_json::json!("broken 1"), None),
        (serde_json::json!([1, 2]), None),
        (serde_json::json!(true), None),
        (serde_json::json!({"value":1.5}), None),
        (serde_json::json!({"user":{"name":"id","condition":"1"},"value":1}), None),
    ] {
        assert_eq!(animation_id(&raw, &props), expected, "{raw}");
    }
}

#[test]
fn selected_animation_id() {
    let props = Properties::new();
    for (raw, selected, accepted) in [
        (1.0, 1, true),
        (211.0, 211, true),
        (1.5, 1, false),
        (-0.5, 0, false),
        (16_777_217.0, 16_777_216, false),
        (16_777_217.0, 16_777_217, true),
    ] {
        let object = json!({"animationlayers":[{"animation":raw,"rate":0,"blend":1}]});
        let layers = [crate::puppet::AnimationLayer {
            visible: true,
            id: selected,
            rate: 0.0,
            blend: 1.0,
            additive: false,
        }];
        assert_eq!(supported_layers(&object, &props, &layers), accepted, "{raw}/{selected}");
    }
}

#[test]
fn keyframed_mesh_kept() {
    let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":1,"model":"models/mesh.mdl","origin":{"value":"0 0 0","animation":{"options":{"fps":10,"length":20,"mode":"single"},"c0":[{"frame":0,"value":0},{"frame":20,"value":1}]}}}]});
    let passes = json!([{"shader":"mesh","blending":"normal"}]);
    let package = mesh_package_scene(&passes, &serde_json::to_vec(&scene).unwrap());
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    assert!(model.scripts.is_none());
    assert!(!model.keyframes.is_empty());
    assert_eq!(model.meshes.len(), 1);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
}

#[test]
fn reflective_mesh_excluded() {
    let passes = json!([{"shader":"mesh","blending":"normal","textures":["_rt_Reflection"]}]);
    let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":1,"name":"mesh","model":"models/mesh.mdl"}]});
    let fragment = b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texture2D(g_Texture0,vec2(0.5,0.5));}";
    let package = shaded_mesh_package(&passes, &serde_json::to_vec(&scene).unwrap(), fragment);
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert!(!model.meshes[0].reflected);
    assert!(model.reflection.bound);
}

#[test]
fn moving_material_demoted() {
    let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[
        {"id":1,"name":"mesh","model":"models/mesh.mdl"},
        {"id":2,"image":"models/sample.json","size":"1 1","reflected":false},
        {"id":3,"model":"models/reader.mdl","reflected":false}
    ]});
    let mesh = triangle_mesh();
    let mut reader = mesh.clone();
    let material =
        reader.windows(MESH_MATERIAL.len()).position(|window| window == MESH_MATERIAL).unwrap();
    reader[material..material + MESH_MATERIAL.len()].copy_from_slice(b"materials/look.json");
    let scene = serde_json::to_vec(&scene).unwrap();
    let vertex: &[u8] = b"#include \"common.h\"\nattribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}";
    let package = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", &scene),
        ("models/mesh.mdl", &mesh),
        ("materials/mesh.json", br#"{"passes":[{"shader":"mesh","blending":"normal"}]}"#),
        ("shaders/mesh.vert", vertex),
        ("shaders/mesh.frag", b"uniform float g_Time;\nvoid main(){gl_FragColor=vec4(g_Time);}"),
        ("models/sample.json", br#"{"material":"materials/sample.json","instanced":true}"#),
        ("models/reader.mdl", &reader),
        ("materials/look.json", br#"{"passes":[{"shader":"sample","textures":["_rt_Reflection"],"blending":"normal"}]}"#),
        ("materials/sample.json", br#"{"passes":[{"shader":"sample","textures":["_rt_Reflection"],"blending":"normal"}]}"#),
        ("shaders/sample.vert", vertex),
        ("shaders/sample.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5));}"),
    ]))
    .unwrap();
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    assert!(!model.meshes[0].reflected);
    assert!(model.skipped.iter().any(|reason| reason.contains(super::REFLECTED_MOTION_GAP)));
}

#[test]
fn retained_model_budget() {
    let passes = json!([{"shader":"mesh","blending":"normal"}]);
    let package = mesh_package(&passes);
    let resources = super::ImageResources::new(
        &package,
        &crate::effects::Assets::discover(Some("/nonexistent")),
    );
    let data = serde_json::from_value(json!({"shapes":[{"vertexBuffer":vec![0.0;24],"vertexFormat":["position","normal","uv"],"material":"materials/mesh.json"}]})).unwrap();
    resources.create_model(1, data).unwrap();
    let scene = json!({"general":{"orthogonalprojection":{"width":128,"height":128}},"objects":[{"id":1,"model":1}]});
    let prepared = resources.load_model(&scene, 0).unwrap();
    let geometry = resources.model_bytes.borrow()[0].1;
    let layer = resources.model_layers.borrow()[&0];
    resources.reserve(super::MAX_SCENE_TEXTURE_BYTES - geometry - layer).unwrap();
    resources.destroy_model(1);
    assert!(resources.reserve(super::MAX_SCENE_TEXTURE_BYTES - layer).is_err());
    assert!(resources.reserve(super::MAX_SCENE_TEXTURE_BYTES - geometry).is_err());
    resources.release(0);
    assert!(resources.reserve(super::MAX_SCENE_TEXTURE_BYTES).is_err());
    drop(prepared);
    resources.reserve(super::MAX_SCENE_TEXTURE_BYTES).unwrap();
    assert!(!resources.has_models());
}

#[test]
fn replacement_budget_rollback() {
    let package = image_package();
    let scene = package.scene_json().unwrap();
    let resources = super::ImageResources::new(
        &package,
        &crate::effects::Assets::discover(Some("/nonexistent")),
    );
    let payload = resources.load(&scene, 0).unwrap().layer.texture.payload_bytes();
    resources.reserve(super::MAX_SCENE_TEXTURE_BYTES).unwrap();
    resources.initialize(super::MAX_SCENE_TEXTURE_BYTES, &[(0, payload)]).unwrap();
    assert!(resources.replace(0, payload + 1).is_err());
    assert_eq!(resources.bytes.get(), super::MAX_SCENE_TEXTURE_BYTES);
    assert_eq!(resources.images.borrow()[&0], payload);
    resources.replace(0, payload - 1).unwrap();
    assert_eq!(resources.bytes.get(), super::MAX_SCENE_TEXTURE_BYTES - 1);
    resources.release(0);
    assert_eq!(resources.bytes.get(), super::MAX_SCENE_TEXTURE_BYTES - payload);
}

#[test]
fn pending_resource_budget() {
    let package = image_package();
    let resources = super::ImageResources::new(
        &package,
        &crate::effects::Assets::discover(Some("/nonexistent")),
    );
    let payload =
        resources.load(&package.scene_json().unwrap(), 0).unwrap().layer.texture.payload_bytes();
    assert!(resources.initialize(super::MAX_SCENE_TEXTURE_BYTES, &[]).is_err());
    assert_eq!(resources.payload_bytes(), payload);
    assert_eq!(resources.images.borrow()[&0], payload);
    resources.initialize(0, &[]).unwrap();
    assert_eq!(resources.payload_bytes(), payload);
    resources.release(0);
    assert_eq!(resources.payload_bytes(), 0);
}

fn dynamic_text_package(scene: &Value) -> crate::pkg::Package {
    let path = std::env::var_os("SKWD_DYNAMIC_TEXT_PACKAGE").expect("pinned text package");
    let package = crate::pkg::Package::open(std::path::Path::new(&path)).unwrap();
    let font = package.find("fonts/Probe.ttf").expect("packaged probe font");
    crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", scene.to_string().as_bytes()),
        ("fonts/Probe.ttf", font),
        ("fonts/Invalid.ttf", b"invalid font"),
    ]))
    .unwrap()
}

#[test]
#[ignore = "requires pinned dynamic text package"]
fn dynamic_text_admission() {
    let mut scene = json!({"general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[{"id":1,"text":"A","font":"fonts/Probe.ttf","pointsize":8,"origin":"32 32 0"}]});
    let package = dynamic_text_package(&scene);
    let resources = super::ImageResources::new(
        &package,
        &crate::effects::Assets::discover(Some("/nonexistent")),
    );
    let prepared = resources.load(&scene, 0).unwrap();
    assert!(prepared.layer.is_text());
    assert!(prepared.layer.script_text.is_some());
    assert!(prepared.layer.texture.text_batch.is_some());
    assert_eq!(resources.bytes.get(), prepared.layer.texture.payload_bytes());
    resources.release(0);
    for field in [
        json!({"effects":[{"file":"missing","visible":false}]}),
        json!({"text":{"value":"A","script":"export function update(){return 'A';}"}}),
        json!({"font":"fonts/Invalid.ttf"}),
    ] {
        let mut candidate = scene.clone();
        candidate["objects"][0].as_object_mut().unwrap().extend(field.as_object().unwrap().clone());
        assert!(resources.load(&candidate, 0).is_err(), "{field}");
        assert_eq!(resources.bytes.get(), 0);
        assert!(resources.images.borrow().is_empty());
    }
    scene["objects"][0]["text"] = json!("");
    assert!(resources.load(&scene, 0).is_ok());
}

#[test]
#[ignore = "requires pinned dynamic text package"]
fn initial_text_retirement() {
    let scene = json!({"general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[{"id":1,"alpha":{"value":1,"script":
            "let layer; export function init(){layer=thisScene.createLayer({text:'A',font:'fonts/Probe.ttf',pointsize:8}); layer.text='AAAA';} export function update(value){if(engine.runtime>=1)thisScene.destroyLayer(layer);return value;}"}}]});
    let package = dynamic_text_package(&scene);
    let mut model =
        super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
            .unwrap();
    assert!(model.skipped.is_empty());
    assert_eq!(model.layers.len(), 1);
    assert_eq!(model.layers[0].script_text.as_ref().unwrap().shown, "AAAA");
    let payload = model.layers[0].texture.payload_bytes();
    let host = model.scripts.as_mut().unwrap();
    assert_eq!(host.image_resources().unwrap().payload_bytes(), payload);
    host.tick(1.0, 1.0, [0.0; 2]).unwrap();
    assert!(host.diagnostics.is_empty());
    assert_eq!(host.take_destroyed_images(), [1]);
    assert_eq!(host.image_resources().unwrap().payload_bytes(), 0);
}

#[test]
#[ignore = "requires pinned dynamic text package"]
fn rejected_text_style() {
    let mut scene = json!({"general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[{"id":1,"text":"A","font":"fonts/Probe.ttf","pointsize":8}]});
    let package = dynamic_text_package(&scene);
    let resources = super::ImageResources::new(
        &package,
        &crate::effects::Assets::discover(Some("/nonexistent")),
    );
    let mut layer = resources.load(&scene, 0).unwrap().layer;
    let text = layer.script_text.as_mut().unwrap();
    let before = text.render("A").unwrap().layout();
    let bounds = text.bounds;
    scene["objects"][0]["pointsize"] = json!(32);
    let candidate = &scene["objects"][0];
    let style = text.update_style(candidate, &Properties::new()).unwrap();
    let rendered = text.render("A").unwrap();
    assert_ne!(rendered.layout(), before);
    assert_ne!(rendered.bounds, bounds);
    assert_eq!(text.bounds, bounds);
    text.reject("A", Some(100));
    text.restore(style);
    assert_eq!(text.bounds, bounds);
    assert_eq!(text.render("A").unwrap().layout(), before);
    assert!(text.rejected("A", candidate, &Properties::new(), 100));
    assert!(!text.rejected("A", candidate, &Properties::new(), 99));
    assert!(!text.rejected("B", candidate, &Properties::new(), 100));
    text.accepted(rendered.bounds);
    assert_eq!(text.bounds, rendered.bounds);
    assert!(!text.has_rejection());
}

#[test]
#[ignore = "requires pinned dynamic text package"]
fn mixed_text_budget() {
    let scene = json!({"general":{"orthogonalprojection":{"width":64,"height":64}},
        "objects":[{"id":1,"text":{"value":"A","script":"export function update(value){return value;}"},"font":"fonts/Probe.ttf","pointsize":8},
        {"id":2,"text":"B","font":"fonts/Probe.ttf","pointsize":8}]});
    let package = dynamic_text_package(&scene);
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    let resources = model.scripts.as_ref().unwrap().image_resources().unwrap();
    let first = model.layers[0].texture.payload_bytes();
    let second = model.layers[1].texture.payload_bytes();
    assert_eq!(resources.images.borrow()[&0], first);
    assert_eq!(resources.images.borrow()[&1], second);
    resources.initialize(super::MAX_SCENE_TEXTURE_BYTES - 1, &[(0, first), (1, second)]).unwrap();
    let revision = resources.revision();
    resources.replace(0, first + 1).unwrap();
    assert_ne!(resources.revision(), revision);
    assert!(resources.replace(1, second + 1).is_err());
    assert_eq!(resources.images.borrow()[&1], second);
    resources.replace(0, first).unwrap();
    resources.replace(1, second + 1).unwrap();
    assert_eq!(resources.payload_bytes(), super::MAX_SCENE_TEXTURE_BYTES);
}

#[test]
fn mixed_text_reservation() {
    let package = image_package();
    let resources = super::ImageResources::new(
        &package,
        &crate::effects::Assets::discover(Some("/nonexistent")),
    );
    let source = package.scene_json().unwrap();
    let payload = resources.load(&source, 0).unwrap().layer.texture.payload_bytes();
    let revision = resources.revision();
    for media in [
        json!({"shape":"quad"}),
        json!({"particle":"missing.json"}),
        json!({"sound":"missing.wav"}),
    ] {
        let mut object = json!({"id":2,"text":"A","font":"fonts/missing.ttf"});
        object.as_object_mut().unwrap().extend(media.as_object().unwrap().clone());
        let scene = json!({"objects":[source["objects"][0].clone(),object]});
        let error = resources.load(&scene, 1).err().expect("mixed text rejected");
        assert!(error.to_string().contains("dynamic text competing media are unsupported"));
        assert_eq!(resources.bytes.get(), payload);
        assert_eq!(resources.images.borrow().len(), 1);
        assert_eq!(resources.images.borrow()[&0], payload);
        assert_eq!(resources.revision(), revision);
    }
}

fn native_sprite_package() -> crate::pkg::Package {
    let path = std::env::var_os("SKWD_DYNAMIC_IMAGE_PACKAGE").expect("pinned image package");
    crate::pkg::Package::open(std::path::Path::new(&path)).unwrap()
}

#[test]
#[ignore = "requires pinned dynamic image package"]
fn native_sprite_admission() {
    let package = native_sprite_package();
    let scene = package.scene_json().unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let resources = super::ImageResources::new(&package, &assets);
    let white = resources.load(&scene, 0).unwrap().layer;
    assert!(white.material.is_none());
    assert!(white.texture.frames.is_empty());
    resources.release(0);
    let image = resources.load(&scene, 2).unwrap().layer;
    assert!(image.material.is_none());
    assert!(image.spritesheet);
    assert!(image.texture.pages.is_empty());
    assert_eq!(image.texture.frames.len(), 2);
    assert!(image.texture.frames.iter().all(|frame| frame.image == 0 && frame.time == 1000.0));
    assert_eq!(
        resources.bytes.get(),
        image.texture.payload_bytes() + image.texture.frame_bytes().unwrap()
    );
    resources.release(2);
    assert_eq!(resources.bytes.get(), 0);
    resources.reserve(super::MAX_SCENE_TEXTURE_BYTES - image.texture.payload_bytes()).unwrap();
    let before = (resources.bytes.get(), resources.revision());
    assert!(resources.load(&scene, 2).is_err());
    assert_eq!((resources.bytes.get(), resources.revision()), before);
    assert!(resources.images.borrow().is_empty());
}

#[test]
#[ignore = "requires pinned dynamic image package"]
fn sprite_validation_wire() {
    let source = native_sprite_package();
    let script = r"
let phase=0;
function create(){const layer=thisScene.createLayer({image:'models/ordinary.json'});const sprite=layer.getTextureAnimation();return [sprite.frameCount,sprite.duration];}
export function init(){let rejected=false;try{thisScene.createLayer({image:'missing.json'});}catch(error){rejected=true;}thisLayer.name=JSON.stringify([create(),rejected,thisScene.getLayerCount()]);}
export function update(value){if(engine.runtime>=1 && phase===0){phase=1;thisLayer.name=JSON.stringify([create(),thisScene.getLayerCount()]);}return value;}
";
    let scene = json!({"general":{"orthogonalprojection":{"width":64,"height":64}},"objects":[{"id":1,"alpha":{"value":1,"script":script}}]});
    let text = scene.to_string();
    let mut files: Vec<_> = source
        .entries()
        .iter()
        .filter(|entry| entry.path != "scene.json")
        .map(|entry| (entry.path.as_str(), source.find(&entry.path).unwrap()))
        .collect();
    files.push(("scene.json", text.as_bytes()));
    let package = crate::pkg::Package::parse(build_pkg(&files)).unwrap();
    let mut model =
        super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
            .unwrap();
    let host = model.scripts.as_mut().unwrap();
    assert!(host.diagnostics.is_empty());
    assert_eq!(host.scene["objects"][0]["name"], "[[2,2000],true,2]");
    let resources = host.image_resources().unwrap();
    let initial = resources.payload_bytes();
    host.tick(0.5, 0.5, [0.0; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["name"], "[[2,2000],true,2]");
    assert_eq!(resources.payload_bytes(), initial);
    assert!(host.take_created_images().is_empty());
    host.tick(1.0, 0.5, [0.0; 2]).unwrap();
    assert!(host.diagnostics.is_empty());
    assert_eq!(host.scene["objects"][0]["name"], "[[2,2000],3]");
    assert_eq!(resources.payload_bytes(), initial * 2);
    let prepared = host.take_created_images();
    assert_eq!(prepared.len(), 1);
    let image = &prepared[0].1.layer;
    assert_eq!(initial, image.texture.payload_bytes() + image.texture.frame_bytes().unwrap());
}

#[test]
fn dynamic_pages_rejected() {
    let texture = unequal_tex();
    let package = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", br#"{"objects":[{"id":1,"image":"model.json"}]}"#),
        ("model.json", br#"{"material":"material.json"}"#),
        ("material.json", br#"{"passes":[{"shader":"genericimage2","combos":{"VERSION":2,"SPRITESHEET":1},"textures":["sprite"]}]}"#),
        ("sprite.tex", &texture),
    ])).unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let resources = super::ImageResources::new(&package, &assets);
    let before = (resources.bytes.get(), resources.revision());
    assert!(resources.load(&package.scene_json().unwrap(), 0).is_err());
    assert_eq!((resources.bytes.get(), resources.revision()), before);
    assert!(resources.images.borrow().is_empty());
}

#[test]
fn attachment_moves_descendants() {
    let objects = [
        json!({"id":4007,"origin":"100 0 0"}),
        json!({"id":9001,"parent":4007,"origin":"10 0 0"}),
        json!({"id":42,"parent":9001,"attachment":"Grip","origin":"3 0 0"}),
        json!({"id":43,"parent":42,"origin":"2 0 0"}),
    ];
    let by_id = objects.iter().map(|object| (object["id"].to_string(), object)).collect();
    let scene = json!({"objects":objects.clone()});
    let layout = super::script::Layout {
        id: "43".into(),
        size: [2.0, 2.0],
        offset: [0.0, 0.0],
        text: false,
        hidden_without_fx: false,
        puppet: false,
        fullscreen: false,
    };
    let props = Properties::new();
    for (x, expected) in [(5.0, 120.0), (15.0, 130.0)] {
        let mut attachments = super::Attachments::new();
        attachments.insert(
            ("9001", "Grip"),
            crate::puppet::AttachmentPose {
                affine: crate::affine::Affine::new([x, 0.0], [1.0, 1.0], 0.0),
                depth: 7.0,
            },
        );
        let child = resolve_transform(&objects[2], &by_id, &props, None, &attachments);
        let descendant = resolve_transform(&objects[3], &by_id, &props, None, &attachments);
        assert_eq!(child.origin, (expected - 2.0, 0.0, 7.0));
        assert_eq!(descendant.origin, (expected, 0.0, 7.0));
        let rendered = super::script::frames(
            &scene,
            std::slice::from_ref(&layout),
            (256.0, 256.0),
            &props,
            &attachments,
        );
        let rendered = rendered[0].as_ref().unwrap();
        assert_eq!(rendered.rect, [expected, 256.0, 2.0, 2.0]);
        assert_eq!(rendered.depth, 7.0);
    }
}

#[test]
fn parent_depth_scale() {
    let props = Properties::new();
    let parent = super::object_transform(&json!({"origin":"0 0 5","scale":"1 1 3"}), &props);
    let child = super::object_transform(&json!({"origin":"0 0 10","scale":"1 1 2"}), &props);
    let pose = super::compose(parent, child);
    assert_eq!(pose.depth_scale, 6.0);
    assert_eq!(pose.origin.2, 35.0);
}

#[test]
fn skeletal_animation_layers() {
    let mut object =
        serde_json::json!({"animationlayers": [{"animation": 1, "rate": 0, "blend": 1}]});
    assert_eq!(super::model_animation_layers(&object, &Properties::new()).unwrap().len(), 1);
    for rate in [0.5, 1.0, 2.0] {
        object["animationlayers"][0]["rate"] = serde_json::json!(rate);
        assert_eq!(
            super::model_animation_layers(&object, &Properties::new()).unwrap()[0].rate,
            rate
        );
    }
    object["animationlayers"][0]["rate"] = serde_json::json!(0);
    for bound in [
        serde_json::json!({"value": 1, "script": "return 0;"}),
        serde_json::json!({"value": 1, "animation": {}}),
    ] {
        object["animationlayers"][0]["blend"] = bound;
        assert!(super::model_animation_layers(&object, &Properties::new()).is_err());
    }
}

#[test]
fn spatial_geometry() {
    let angle = std::f32::consts::FRAC_PI_2;
    for (angles, expected) in
        [([angle, 0.0, 0.0], [16.0, 8.0, 27.0]), ([0.0, angle, 0.0], [22.0, 17.0, 24.0])]
    {
        let object = json!({"id":1,"origin":"10 20 30","scale":"2 3 4",
            "angles":format!("{} {} {}",angles[0],angles[1],angles[2])});
        let properties = Properties::new();
        let transform = super::object_transform(&object, &properties);
        let geometry =
            super::Geometry::new(transform, (100.0, 100.0), [2.0, 3.0], (1920.0, 1080.0));
        let matrix = geometry.spatial.unwrap();
        let point = [0, 1, 2].map(|row| {
            matrix[row] + 2.0 * matrix[4 + row] + 3.0 * matrix[8 + row] + matrix[12 + row]
        });
        for (actual, expected) in point.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.00001, "{point:?}");
        }
        let scene = json!({"objects":[object]});
        let layout = super::script::Layout {
            id: "1".into(),
            size: [100.0; 2],
            offset: [2.0, 3.0],
            text: false,
            hidden_without_fx: false,
            puppet: false,
            fullscreen: false,
        };
        let frames = super::script::frames(
            &scene,
            &[layout],
            (1920.0, 1080.0),
            &properties,
            &super::Attachments::new(),
        );
        assert_eq!(frames[0].as_ref().unwrap().geometry.unwrap().spatial, Some(matrix));
    }
    for z in [0.0, angle] {
        let object = json!({"angles":format!("0 0 {z}")});
        let transform = super::object_transform(&object, &Properties::new());
        assert!(
            super::Geometry::new(transform, (100.0, 100.0), [2.0, 3.0], (1920.0, 1080.0))
                .spatial
                .is_none()
        );
    }
}

#[test]
fn parented_spatial_scope() {
    let scene = json!({"objects":[
        {"id":1,"parent":2,"origin":"10 20 0"},
        {"id":2,"angles":"0.5 0 0","origin":"30 40 0"}
    ]});
    let layout = super::script::Layout {
        id: "1".into(),
        size: [100.0; 2],
        offset: [0.0; 2],
        text: false,
        hidden_without_fx: false,
        puppet: false,
        fullscreen: false,
    };
    let frames = super::script::frames(
        &scene,
        &[layout],
        (1920.0, 1080.0),
        &Properties::new(),
        &super::Attachments::new(),
    );
    let geometry = frames[0].as_ref().unwrap().geometry.unwrap();
    assert!(geometry.spatial.is_none());
    assert_eq!([geometry.transform.x, geometry.transform.y], [40.0, 1020.0]);
}

#[test]
fn vacant_dynamic_object() {
    let scene = json!({"objects":[{"id":"@dynamic-1","visible":false}]});
    let bytes = scene.to_string();
    let package =
        crate::pkg::Package::parse(build_pkg(&[("scene.json", bytes.as_bytes())])).unwrap();
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some(""))).unwrap();
    assert!(model.layers.is_empty());
    assert!(model.meshes.is_empty());
    assert!(model.particles.is_empty());
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let layout = super::script::Layout {
        id: "@dynamic-1".into(),
        size: [1.0; 2],
        offset: [0.0; 2],
        text: false,
        hidden_without_fx: false,
        puppet: false,
        fullscreen: false,
    };
    let frames = super::script::frames(
        &scene,
        &[layout],
        (64.0, 64.0),
        &Properties::new(),
        &super::Attachments::new(),
    );
    assert!(!frames[0].as_ref().unwrap().visible);
    assert_eq!(frames[0].as_ref().unwrap().tint[3], 0.0);
}

#[test]
fn authored_animation_visibility() {
    let props = Properties::new();
    let raw = json!({"animation":1,"rate":2,"blend":0.5,"visible":false});
    let layer = super::puppet_animation_layer(&raw, &props).unwrap();
    assert!(!layer.visible);
    assert_eq!(layer.blend, 0.5);
    assert_eq!(layer.rate, 2.0);
    for object in [json!({}), json!({"animationlayers":[]})] {
        assert!(supported_layers(&object, &props, &[]));
    }
    for object in [json!({"animationlayers":null}), json!({"animationlayers":[{}]})] {
        assert!(!supported_layers(&object, &props, &[]));
    }
}

#[test]
fn mesh_ready_rules() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let mut model = super::load_with(&image_package(), &assets).unwrap();
    let layer = &mut model.layers[0];
    assert!(layer.orthographic_mesh_ready());
    layer.copy_background = true;
    assert!(!layer.orthographic_mesh_ready());
    layer.copy_background = false;
    layer.kind = super::LayerKind::Viewport { hdr: false };
    assert!(!layer.orthographic_mesh_ready());
}

#[test]
fn streamed_sprite_texture() {
    use crate::tex::stream::{pack, tests};
    let sprite = pack(&tests::sprite(&tests::FRAMES, true), 0).unwrap().unwrap();
    let packed: usize = sprite.cells.iter().map(crate::tex::stream::Cell::packed_bytes).sum();
    let texture = super::streamed_texture(sprite).unwrap();
    assert_eq!((texture.width, texture.height), (2, 2));
    assert_eq!(texture.pixels.levels[0].data[..4], [0, 0, 0, 255]);
    assert_eq!(texture.stream.as_ref().map(|cells| cells.len()), Some(tests::FRAMES.len()));
    let frames = texture.atlas_frames().unwrap();
    assert_eq!(frames.iter().map(|frame| frame.image).collect::<Vec<_>>(), [0, 1, 2, 3]);
    assert!(frames.iter().all(|frame| frame.uv == [0.0, 0.0, 1.0, 1.0] && frame.time == 0.04));
    assert_eq!(texture.payload_bytes(), texture.pixels.bytes() + packed);
}

pub(crate) fn packaged_video(
    directory: &std::path::Path,
    tex: &[u8],
    extra: u64,
) -> crate::pkg::Package {
    use std::io::Write;
    let name = "materials/clip.tex";
    let mut header = Vec::new();
    crate::tests::push_len_str(&mut header, "PKGV0007");
    crate::tests::push_i32(&mut header, 1);
    crate::tests::push_len_str(&mut header, name);
    header.extend_from_slice(&0_u32.to_le_bytes());
    header.extend_from_slice(&u32::try_from(tex.len() as u64 + extra).unwrap().to_le_bytes());
    let path = directory.join("scene.pkg");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(&header).unwrap();
    file.write_all(tex).unwrap();
    file.set_len((header.len() + tex.len()) as u64 + extra).unwrap();
    crate::pkg::Package::open(&path).unwrap()
}

fn clip_texture(pkg: &crate::pkg::Package) -> super::Texture {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    super::load_texture_from(pkg, &assets, &["materials/clip.tex".into()], &[]).unwrap()
}

fn clip_payload() -> Vec<u8> {
    let mut payload = vec![93_u8; 256];
    payload[..12].copy_from_slice(b"\x00\x00\x00\x20ftypisom");
    payload
}

pub(crate) fn clip_tex(flags: u32, lz4: bool) -> Vec<u8> {
    let frames = (flags & crate::tex::FLAG_IS_GIF != 0).then_some("TEXS0003");
    crate::tests::build_tex("TEXB0003", 0, flags as i32, &clip_payload(), lz4, frames)
}

#[test]
fn packaged_video_range() {
    use std::os::unix::fs::FileExt;
    let directory = tempfile::tempdir().unwrap();
    let tex = clip_tex(crate::tex::FLAG_IS_VIDEO, false);
    let texture = clip_texture(&packaged_video(directory.path(), &tex, 0));
    let Some(super::Video::Range(range)) = &texture.video else { panic!("video range") };
    let mut read = vec![0; clip_payload().len()];
    range.file.read_exact_at(&mut read, range.offset).unwrap();
    assert_eq!((read, range.length), (clip_payload(), 256));
    assert_eq!(texture.payload_bytes(), 0);
}

#[test]
fn in_memory_video_bytes() {
    let tex = clip_tex(crate::tex::FLAG_IS_VIDEO, false);
    let memory = crate::pkg::Package::parse(build_pkg(&[("materials/clip.tex", &tex)])).unwrap();
    let Some(super::Video::Bytes(payload)) = clip_texture(&memory).video else {
        panic!("video bytes")
    };
    assert_eq!(*payload, *clip_payload());
}

#[test]
fn unclippable_video_bytes() {
    let directory = tempfile::tempdir().unwrap();
    for (flags, lz4) in [
        (crate::tex::FLAG_IS_VIDEO, true),
        (crate::tex::FLAG_IS_VIDEO | crate::tex::FLAG_IS_GIF, false),
    ] {
        let texture = clip_texture(&packaged_video(directory.path(), &clip_tex(flags, lz4), 0));
        let Some(super::Video::Bytes(payload)) = texture.video else { panic!("video bytes") };
        assert_eq!(*payload, *clip_payload());
    }
}

pub(crate) fn oversized_tex(length: u64) -> Vec<u8> {
    let mut tex =
        crate::tests::build_tex("TEXB0003", 0, crate::tex::FLAG_IS_VIDEO as i32, &[], false, None);
    let tail = tex.len() - 2 * size_of::<i32>();
    for (index, value) in [length, length].into_iter().enumerate() {
        let at = tail + index * size_of::<i32>();
        tex[at..at + size_of::<i32>()]
            .copy_from_slice(&u32::try_from(value).unwrap().to_le_bytes());
    }
    tex
}

#[test]
fn oversized_video_range() {
    let length = crate::tex::MAX_TEX_PAYLOAD_BYTES as u64 + 1;
    let tex = oversized_tex(length);
    let directory = tempfile::tempdir().unwrap();
    let pkg = packaged_video(directory.path(), &tex, length);
    assert!(crate::tex::parse_meta(pkg.find("materials/clip.tex").unwrap()).is_ok());
    let texture = clip_texture(&pkg);
    let Some(super::Video::Range(range)) = &texture.video else { panic!("video range") };
    assert_eq!(range.length, length);
}

#[test]
fn pose_errors_classified() {
    let props = crate::model::Properties::new();
    for (object, degenerate) in [
        (json!({"scale":"0 1 1"}), true),
        (json!({"origin":"nan 0 0"}), true),
        (json!({"origin":"left"}), false),
        (json!({"parent":"7"}), false),
    ] {
        let by_id = std::collections::HashMap::new();
        let Err(error) = super::mesh_pose(&object, &props, "m")
            .and_then(|_| super::mesh_parents(&object, &by_id, &props, "m"))
        else {
            panic!("pose accepted");
        };
        assert_eq!(matches!(error, super::PoseError::Degenerate(_)), degenerate, "{error}");
    }
}

#[test]
fn texture_variant_follows_property() {
    use crate::tests::{FULL_VARIANT, build_variant_tex};
    let bytes = build_variant_tex(
        &[
            (1, r#"{"condition":{"condition":"2","name":"Tunic"}}"#, FULL_VARIANT),
            (2, r#"{"condition":{"condition":"1","name":"tunic"}}"#, FULL_VARIANT),
        ],
        false,
    );
    for (value, expected) in [(0.0, None), (1.0, Some(2)), (2.0, Some(1))] {
        let assets = crate::effects::Assets::discover(Some("/nonexistent"))
            .with_properties(std::collections::BTreeMap::from([("tunic".into(), vec![value])]));
        assert_eq!(super::texture_variant(&bytes, &assets), expected);
        assert!(assets.property_dependencies().unwrap().contains("tunic"));
    }
    let patch = build_variant_tex(
        &[(1, r#"{"condition":{"condition":"1","name":"tunic"}}"#, [1, 1, 2, 2])],
        false,
    );
    let assets = crate::effects::Assets::discover(Some("/nonexistent"))
        .with_properties(std::collections::BTreeMap::from([("tunic".into(), vec![1.0])]));
    assert_eq!(super::texture_variant(&patch, &assets), None);
}

#[test]
fn mount_depths() {
    let props = crate::model::Properties::new();
    let objects = [
        json!({"id":10,"model":"models/rig.mdl"}),
        json!({"id":20,"parent":10,"attachment":"Grip"}),
        json!({"id":1,"parent":10,"attachment":"Grip"}),
        json!({"id":2,"parent":20}),
        json!({"id":3,"parent":20,"attachment":"Grip"}),
        json!({"id":4,"attachment":"Grip"}),
    ];
    let by_id: std::collections::HashMap<String, &Value> =
        objects.iter().map(|object| (object["id"].to_string(), object)).collect();
    let mount = |id: &str| {
        super::mesh_parents(by_id[id], &by_id, &props, "m")
            .map(|parents| parents.mount.map(|mount| (mount.owner, mount.depth)))
    };
    assert_eq!(mount("1").unwrap(), Some(("10".into(), 0)));
    assert_eq!(mount("2").unwrap(), Some(("10".into(), 1)));
    assert!(mount("3").is_err());
    assert!(mount("4").is_err());
}

#[test]
fn mesh_perspective_flag() {
    let props = crate::model::Properties::from([("deep".into(), vec![0.0])]);
    for (object, perspective) in [
        (json!({"perspective": true}), true),
        (json!({"perspective": {"user": "deep", "value": true}}), false),
        (json!({"perspective": false}), false),
        (json!({}), false),
    ] {
        assert_eq!(super::mesh_perspective(&object, &props), perspective, "{object}");
    }
}

#[test]
fn perspective_fov_default() {
    for (general, fov) in [
        (json!({}), 95.0),
        (json!({"perspectiveoverridefov": 21.0}), 21.0),
        (json!({"perspectiveoverridefov": 400.0}), 179.0),
    ] {
        let scene = json!({"general": general, "objects": [{"id": 1, "model": "models/mesh.mdl"}]});
        let package =
            mesh_package_scene(&json!([{"shader": "mesh"}]), scene.to_string().as_bytes());
        let model =
            super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
                .unwrap();
        assert_eq!(model.perspective_fov, fov, "{general}");
    }
}

#[test]
fn camera_object_drives_camera() {
    let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},
        "general":{"fov":60,"nearz":0.1,"farz":100},
        "objects":[{"id":1,"model":"models/mesh.mdl"},
            {"id":2,"camera":"default","origin":"1 2 3","angles":"0 0 0","fov":30.0}]});
    let package = mesh_package_scene(&json!([{"shader":"mesh"}]), scene.to_string().as_bytes());
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    let camera = model.camera.unwrap();
    assert_eq!(camera.eye, [1.0, 2.0, 3.0]);
    assert_eq!(camera.center, [1.0, 2.0, 2.0]);
    assert_eq!(camera.fov, 30.0);
}

#[test]
fn deferred_hidden_layers() {
    let script =
        "export function update(value) { thisScene.getLayer('hidden').origin; return value; }";
    let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},
        "general":{"fov":60,"nearz":0.1,"farz":100},
        "objects":[{"id":1,"name":"hidden","model":"models/mesh.mdl","visible":false},
            {"id":2,"model":"models/mesh.mdl","origin":{"value":"0 0 0","script":script}}]});
    let package = mesh_package_scene(&json!([{"shader":"mesh"}]), scene.to_string().as_bytes());
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    for (defer_hidden, meshes) in [(false, 2), (true, 1)] {
        let pass = super::LoadPass { defer_hidden, ..super::LoadPass::default() };
        let model =
            super::load_with_project(&package, &assets, &Value::Null, None, None, pass).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert_eq!(model.meshes.len(), meshes, "{defer_hidden}");
        let host = model.scripts.as_ref().unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    }
}

fn three_images(hidden: &[u8]) -> crate::pkg::Package {
    let script = "export function update(value) { return value; }";
    let scene = json!({"general":{"orthogonalprojection":{"width":8,"height":8}},"objects":[
        {"id":2,"image":"models/2.json","size":"4 4","visible":false},
        {"id":1,"image":"models/1.json","size":"4 4","alpha":{"value":1,"script":script}},
        {"id":3,"image":"models/3.json","size":"4 4","visible":false}]})
    .to_string();
    let visible = build_tex("TEXB0002", 0, 0, &[1; 64], false, None);
    let files: Vec<(String, Vec<u8>)> = (1..=3_u8)
        .flat_map(|id| {
            let tex = if id == 1 { visible.clone() } else { hidden.to_vec() };
            [
                (format!("models/{id}.json"), format!(r#"{{"material":"materials/{id}.json"}}"#)),
                (
                    format!("materials/{id}.json"),
                    format!(r#"{{"passes":[{{"textures":["{id}"]}}]}}"#),
                ),
            ]
            .map(|(path, text)| (path, text.into_bytes()))
            .into_iter()
            .chain([(format!("materials/{id}.tex"), tex)])
        })
        .chain([("scene.json".to_owned(), scene.into_bytes())])
        .collect();
    let entries: Vec<(&str, &[u8])> =
        files.iter().map(|(path, bytes)| (path.as_str(), bytes.as_slice())).collect();
    crate::pkg::Package::parse(build_pkg(&entries)).unwrap()
}

fn png_tex(payload: &[u8], edge: i32) -> Vec<u8> {
    const PNG_FORMAT: i32 = 13;
    let mut tex = build_tex("TEXB0003", 0, 0, payload, false, None);
    let container = b"TEXB0003\0";
    let start = tex.windows(container.len()).position(|window| window == container).unwrap();
    let header = start + container.len() + 4;
    for (offset, value) in [(0, PNG_FORMAT), (8, edge), (12, edge)] {
        tex[header + offset..header + offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    tex
}

fn budget_model(hidden: &[u8]) -> super::SceneModel {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let pass = super::LoadPass { texture_budget: 150, ..super::LoadPass::default() };
    super::load_with_project(&three_images(hidden), &assets, &Value::Null, None, None, pass)
        .unwrap()
}

#[test]
fn budget_defers_hidden() {
    let model = budget_model(&png_tex(&[0; 64], 4));
    let ids: Vec<_> = model.layers.iter().map(|layer| layer.id.as_str()).collect();
    assert_eq!(ids, ["1"]);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert!(model.scripts.is_some());
}

#[test]
fn budget_rolls_back() {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::new(4, 4).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let model = budget_model(&png_tex(png.get_ref(), 1));
    let ids: Vec<_> = model.layers.iter().map(|layer| layer.id.as_str()).collect();
    assert_eq!(ids, ["1"]);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
}

#[test]
fn budget_marker_survives_context() {
    let mut total = 0;
    let error = anyhow::Context::context(
        super::add_texture_bytes(&mut total, super::MAX_SCENE_TEXTURE_BYTES + 1),
        "layer",
    )
    .unwrap_err();
    assert!(error.downcast_ref::<super::TextureBudgetExceeded>().is_some());
}

#[test]
fn mesh_skin_selects_material() {
    let passes = json!([{"shader":"mesh","cullmode":"nocull","blending":"normal"}]);
    let alt =
        json!({"passes":[{"shader":"alt","cullmode":"nocull","blending":"normal"}]}).to_string();
    for (skin, expected) in [(None, Some("mesh")), (Some(1), Some("alt")), (Some(2), None)] {
        let mut object = json!({"id":1,"model":"models/mesh.mdl"});
        if let Some(skin) = skin {
            object["skin"] = json!(skin);
        }
        let scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[object]})
            .to_string();
        let base = mesh_package_scene(&passes, scene.as_bytes());
        let mut entries: Vec<(&str, Vec<u8>)> = base
            .entries()
            .iter()
            .map(|entry| (entry.path.as_str(), base.read(entry).to_vec()))
            .collect();
        let mesh = &mut entries.iter_mut().find(|(path, _)| *path == "models/mesh.mdl").unwrap().1;
        mesh[13..17].copy_from_slice(&2_u32.to_le_bytes());
        let tail = b"materials/mesh.json\0".len() + 21;
        mesh.splice(tail..tail, b"materials/alt.json\0".iter().copied());
        let vertex = base.entries().iter().find(|entry| entry.path == "shaders/mesh.vert").unwrap();
        entries.extend([
            ("materials/alt.json", alt.clone().into_bytes()),
            ("shaders/alt.vert", base.read(vertex).to_vec()),
            ("shaders/alt.frag", b"void main(){gl_FragColor=vec4(0.0,1.0,0.0,1.0);}".to_vec()),
        ]);
        let files: Vec<(&str, &[u8])> =
            entries.iter().map(|(path, bytes)| (*path, bytes.as_slice())).collect();
        let package = crate::pkg::Package::parse(build_pkg(&files)).unwrap();
        let model =
            super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
                .unwrap();
        let shader = model.meshes.first().map(|mesh| mesh.materials[0].pass.name.as_str());
        assert_eq!(shader, expected, "{:?}", model.skipped);
    }
}

#[test]
fn degenerate_static_mesh_reported() {
    let passes = json!([{"shader":"mesh","cullmode":"nocull","blending":"normal"}]);
    let scene = br#"{"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":1,"name":"flat","model":"models/mesh.mdl","scale":"0 0 0"}]}"#;
    let package = mesh_package_scene(&passes, scene);
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    assert!(model.meshes.is_empty());
    assert_eq!(model.skipped, ["flat: degenerate mesh pose unsupported"]);
}

#[test]
fn camera_frames_skip_puppets() {
    let scene = json!({
        "camera":{"eye":"0 0 10","center":"0 0 0","up":"0 1 0"},
        "general":{"fov":50,"nearz":0.1,"farz":100},
        "objects":[{"id":1,"parent":2,"origin":"0 0 0"},{"id":2,"angles":"0.5 0 0"}]
    });
    let tilted = [false, true].map(|puppet| {
        let layout = super::script::Layout {
            id: "1".into(),
            size: [100.0; 2],
            offset: [0.0; 2],
            text: false,
            hidden_without_fx: false,
            puppet,
            fullscreen: false,
        };
        let frames = super::script::frames(
            &scene,
            &[layout],
            (1920.0, 1080.0),
            &Properties::new(),
            &super::Attachments::new(),
        );
        frames[0].as_ref().unwrap().geometry.unwrap().spatial.is_some()
    });
    assert_eq!(tilted, [true, false]);
}

#[test]
fn camera_puppets_stay_flat() {
    let texture = build_tex("TEXB0002", 0, 0, &[255; 64], false, None);
    let scene = json!({
        "camera":{"eye":"0 0 -5","center":"0 0 0","up":"0 1 0"},
        "general":{"fov":60,"nearz":0.1,"farz":100},
        "objects":[{"id":1,"image":"models/clock.json","parent":2,"size":"2 2"},{"id":2,"angles":"0.5 0 0"}]
    })
    .to_string();
    let package = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", scene.as_bytes()),
        (
            "models/clock.json",
            br#"{"material":"materials/image.json","puppet":"models/clock.mdl"}"#,
        ),
        ("models/clock.mdl", include_bytes!("fixtures/a02/clock-step.mdl")),
        ("materials/image.json", br#"{"passes":[{"textures":["image"]}]}"#),
        ("materials/image.tex", texture.as_slice()),
    ]))
    .unwrap();
    let model = super::load_with(&package, &crate::effects::Assets::discover(Some("/nonexistent")))
        .unwrap();
    let layer = model.layers.iter().find(|layer| layer.id == "1").unwrap();
    assert!(layer.puppet.is_some());
    assert!(layer.geometry.unwrap().spatial.is_none());
}

#[test]
fn hidden_puppet_blend() {
    let texture = build_tex("TEXB0002", 0, 0, &[255; 64], false, None);
    let hidden = json!({"value":false,"animation":{}});
    let scripted = json!({"value":1.0,"script":"export function update(value) { return value; }"});
    for (visible, alpha, name) in [
        (json!(true), scripted.clone(), crate::effects::COLOR_BLEND_MESH_EFFECT),
        (hidden, scripted, crate::effects::COLOR_BLEND_MESH_EFFECT),
        (json!(true), json!(1.0), crate::effects::COLOR_BLEND_MESH_EFFECT),
        (json!(false), json!(1.0), crate::effects::COLOR_BLEND_EFFECT),
    ] {
        let scene = json!({"objects":[
            {"id":1,"image":"models/clock.json","size":"2 2","colorBlendMode":9,"visible":visible},
            {"id":2,"image":"models/clock.json","size":"2 2","alpha":alpha,"dependencies":[1]}
        ]})
        .to_string();
        let package = crate::pkg::Package::parse(build_pkg(&[
            ("scene.json", scene.as_bytes()),
            (
                "models/clock.json",
                br#"{"material":"materials/image.json","puppet":"models/clock.mdl"}"#,
            ),
            ("models/clock.mdl", include_bytes!("fixtures/a02/clock-step.mdl")),
            ("materials/image.json", br#"{"passes":[{"textures":["image"]}]}"#),
            ("materials/image.tex", texture.as_slice()),
            (
                "shaders/common_blending.h",
                b"vec3 ApplyBlending(const int mode, in vec3 A, in vec3 B, in float opacity) { return mix(A, B, opacity); }",
            ),
        ]))
        .unwrap();
        let assets = crate::effects::Assets::discover(Some("/nonexistent"));
        let model = super::load_with(&package, &assets).unwrap();
        let layer = model.layers.iter().find(|layer| layer.id == "1").unwrap();
        assert_eq!(layer.effects.last().unwrap().name, name);
    }
}

#[test]
fn compose_blend_chain() {
    for (mode, kept) in [(0, false), (31, false), (9, true)] {
        let scene = json!({"objects":[{"id":1,"image":"models/util/composelayer.json",
            "size":"64 64","colorBlendMode":mode}]})
        .to_string();
        let package = crate::pkg::Package::parse(build_pkg(&[
            ("scene.json", scene.as_bytes()),
            (
                "models/util/composelayer.json",
                br#"{"material":"materials/compose.json","passthrough":true}"#,
            ),
            ("materials/compose.json", br#"{"passes":[{"shader":"composelayer"}]}"#),
            (
                "shaders/common_blending.h",
                b"vec3 ApplyBlending(const int mode, in vec3 A, in vec3 B, in float opacity) { return mix(A, B, opacity); }",
            ),
        ]))
        .unwrap();
        let assets = crate::effects::Assets::discover(Some("/nonexistent"));
        let model = super::load_with(&package, &assets).unwrap();
        let names: Vec<_> = model
            .layers
            .iter()
            .flat_map(|layer| layer.effects.iter().map(|effect| effect.name.as_str()))
            .collect();
        let want: &[&str] = if kept { &[crate::effects::COLOR_BLEND_EFFECT] } else { &[] };
        assert_eq!(names, want, "{mode} {:?}", model.skipped);
    }
}

#[test]
fn additive_skips_chain() {
    let texture = build_tex("TEXB0002", 0, 0, &[255; 64], false, None);
    let lit = br#"{"passes":[{"shader":"lit","textures":["image"],"combos":{"LIGHTING":1}}]}"#;
    let plain = br#"{"passes":[{"textures":["image"]}]}"#;
    let normal = br#"{"passes":[{"shader":"lit","textures":["image"],"blending":"normal"}]}"#;
    let coverage =
        br#"{"passes":[{"shader":"lit","textures":["image"],"blending":"alphatocoverage"}]}"#;
    let sampler = json!({"id":2,"image":"models/flat.json","size":"2 2","dependencies":[1]});
    let cases: [(&str, &[u8], Option<&Value>, bool); 6] = [
        ("models/flat.json", plain, None, false),
        ("models/flat.json", lit, None, true),
        ("models/flat.json", normal, None, true),
        ("models/flat.json", coverage, None, true),
        ("models/flat.json", plain, Some(&sampler), false),
        ("models/clock.json", plain, None, false),
    ];
    for (image, material, other, kept) in cases {
        let first = json!({"id":1,"image":image,"size":"2 2","colorBlendMode":31});
        let objects: Vec<Value> = std::iter::once(first).chain(other.cloned()).collect();
        let scene = json!({ "objects": objects }).to_string();
        let package = crate::pkg::Package::parse(build_pkg(&[
            ("scene.json", scene.as_bytes()),
            ("models/flat.json", br#"{"material":"materials/image.json"}"#),
            (
                "models/clock.json",
                br#"{"material":"materials/image.json","puppet":"models/clock.mdl"}"#,
            ),
            ("models/clock.mdl", include_bytes!("fixtures/a02/clock-step.mdl")),
            ("materials/image.json", material),
            ("materials/image.tex", texture.as_slice()),
            (
                "shaders/lit.vert",
                b"attribute vec3 a_Position;\nvoid main() { gl_Position = vec4(a_Position, 1.0); }\n",
            ),
            ("shaders/lit.frag", b"void main() { gl_FragColor = vec4(1.0); }\n"),
        ]))
        .unwrap();
        let assets = crate::effects::Assets::discover(Some("/nonexistent"));
        let model = super::load_with(&package, &assets).unwrap();
        let layer = model.layers.iter().find(|layer| layer.id == "1").unwrap();
        assert!(layer.effects.is_empty(), "{image} {:?}", model.skipped);
        assert!(layer.additive());
        let blend = layer.material.as_ref().map(|material| material.blend);
        assert_eq!(blend, kept.then_some(crate::effects::PassBlend::Add));
    }
}

#[test]
fn folded_texture_shared() {
    let texture = build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
    let package =
        crate::pkg::Package::parse(build_pkg(&[("materials/Foo.tex", &texture)])).unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent")).with_texture_cache();
    let exact = super::load_texture_or_asset(&package, &assets, "Foo").unwrap();
    let folded = super::load_texture_or_asset(&package, &assets, "foo").unwrap();
    assert!(std::sync::Arc::ptr_eq(&exact.pixels, &folded.pixels));
}

#[test]
fn texture_names_keep_suffix() {
    assert_eq!(
        super::tex_candidates("logo.tex"),
        ["logo.tex.tex", "materials/logo.tex.tex", "logo.tex", "materials/logo.tex"]
    );
    assert_eq!(super::tex_candidates("logo"), ["logo.tex", "materials/logo.tex"]);
    assert_eq!(super::tex_candidates("materials/logo"), ["materials/logo.tex"]);
    assert_eq!(
        super::tex_candidates("logo.tex.tex"),
        ["logo.tex.tex.tex", "materials/logo.tex.tex.tex", "logo.tex", "materials/logo.tex"]
    );
}

#[test]
fn suffixed_texture_loads() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let texture = build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
    let package = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", br#"{"objects":[{"id":1,"image":"models/a.json","size":"4 4"}]}"#),
        ("models/a.json", br#"{"material":"materials/a.json"}"#),
        ("materials/a.json", br#"{"passes":[{"shader":"genericimage2","textures":["logo.tex"]}]}"#),
        ("materials/logo.tex.tex", &texture),
    ]))
    .unwrap();
    let model = super::load_with(&package, &assets).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.layers.len(), 1);
}

#[test]
fn absent_texture_named_missing() {
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let package = crate::pkg::Package::parse(build_pkg(&[
        ("scene.json", br#"{"objects":[{"id":1,"image":"models/a.json","size":"4 4"}]}"#),
        ("models/a.json", br#"{"material":"materials/a.json"}"#),
        ("materials/a.json", br#"{"passes":[{"shader":"genericimage2","textures":["gone"]}]}"#),
    ]))
    .unwrap();
    let model = super::load_with(&package, &assets).unwrap();
    assert!(model.skipped[0].contains("missing texture gone"), "{:?}", model.skipped);
}

#[test]
fn reservation_overrun_static() {
    let scene = json!({"objects":[{"id":1,"name":"caption","text":{"value":"authored","script":"export function init() { return 'scripted'; }"}}]});
    let source = scene.to_string();
    let package =
        crate::pkg::Package::parse(build_pkg(&[("scene.json", source.as_bytes())])).unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let scripted = super::load_with(&package, &assets).unwrap();
    assert!(scripted.scripts.is_some());
    RESERVATION_FAULT.set(true);
    let model = super::load_with(&package, &assets).unwrap();
    assert!(model.scripts.is_none());
    assert!(!RESERVATION_FAULT.get());
}

thread_local! {
    static RESERVATION_FAULT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(super) fn reservation_fault() -> anyhow::Result<()> {
    if RESERVATION_FAULT.replace(false) {
        anyhow::bail!("injected reservation overrun");
    }
    Ok(())
}
