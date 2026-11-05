use super::*;
use serde_json::json;

fn scene() -> Value {
    json!({"general":{"lightconfig":{"point":1}},"objects":[{
        "id":1,"light":"lpoint","origin":"64 128 64","color":"0.2 0.4 0.8",
        "intensity":2,"radius":512,"exponent":2,"castshadow":false,"castvolumetrics":false
    }]})
}

#[test]
fn point_uniforms() {
    let (config, skipped) = load(&scene(), &Properties::new());
    assert!(skipped.is_empty());
    assert_eq!(config.count, 1);
    assert_eq!(config.values[ORIGIN], [64.0, 128.0, 64.0, 2.0]);
    assert_eq!(config.values[COLOR], [0.4, 0.8, 1.6, 512.0]);
}

#[test]
fn volume_properties() {
    let mut scene = scene();
    let object = &mut scene["objects"][0];
    object["castvolumetrics"] = json!(true);
    object["density"] = json!({"user":"density","value":0.25});
    object["volumetricsexponent"] = json!(2.5);
    object["visible"] = json!({"user":"visible","value":true});
    let props = Properties::from([("density".into(), vec![0.75]), ("visible".into(), vec![0.0])]);
    let lights = lights(&scene, &props, &Attachments::new());
    assert_eq!(lights.len(), 1);
    assert!(!lights[0].visible);
    assert_eq!(lights[0].volume, Some(Volume { density: 0.75, exponent: 2.5 }));
    assert_eq!(lights[0].color, [0.4, 0.8, 1.6]);
}

#[test]
fn volume_parent_pose() {
    let mut scene = spot_scene();
    scene["objects"][0]["parent"] = json!(5);
    scene["objects"][0]["origin"] = json!("10 0 0");
    scene["objects"].as_array_mut().unwrap().push(json!({
        "id":5,"origin":"100 200 300","angles":"0 0 1.5707963267948966","visible":false
    }));
    let lights = lights(&scene, &Properties::new(), &Attachments::new());
    assert!(!lights[0].visible);
    for (actual, expected) in lights[0].origin.into_iter().zip([100.0, 210.0, 300.0]) {
        assert!((actual - expected).abs() < 0.001);
    }
    assert!(lights[0].forward[1] > 0.999);
}

fn spot_scene() -> Value {
    json!({"general":{"lightconfig":{"spot":1}},"objects":[{
        "id":1,"light":"lspot","origin":"960 540 250","color":"1 0.5 0.25",
        "intensity":2,"radius":500,"exponent":2,"innercone":30,"outercone":60
    }]})
}

#[test]
fn spot_uniforms() {
    let (config, skipped) = load(&spot_scene(), &Properties::new());
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(config.spots, 1);
    assert_eq!(config.count, 0);
    assert_eq!(config.values["g_LSpot_Color"], [2.0, 1.0, 0.5, 500.0]);
    assert_eq!(config.values["g_LSpot_Exponent"], [2.0, 0.0, 0.0, 0.0]);
    let origin = &config.values["g_LSpot_Origin"];
    assert_eq!(&origin[..3], [960.0, 540.0, 250.0]);
    assert!((origin[3] - 30.0_f32.to_radians().cos()).abs() < f32::EPSILON);
    let direction = &config.values["g_LSpot_Direction"];
    assert_eq!(&direction[..3], [1.0, 0.0, 0.0]);
    assert!((direction[3] - 0.5).abs() < f32::EPSILON);
}

#[test]
fn spot_rotation_and_parent() {
    for (angles, expected) in [
        ([0.0, std::f32::consts::FRAC_PI_2, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, 0.0, std::f32::consts::FRAC_PI_2], [0.0, 1.0, 0.0]),
    ] {
        let mut scene = spot_scene();
        scene["objects"][0]["angles"] = json!(angles);
        let values = uniforms(&scene, &Properties::new(), &Attachments::new(), &SunView::default());
        for (actual, expected) in values["g_LSpot_Direction"].iter().zip(expected) {
            assert!((actual - expected).abs() < f32::EPSILON);
        }
    }
    let mut scene = spot_scene();
    scene["objects"][0]["parent"] = json!(2);
    scene["objects"][0]["origin"] = json!("10 0 0");
    scene["objects"].as_array_mut().unwrap().push(json!({
        "id":2,"origin":"5 10 20","angles":[0,0,std::f32::consts::FRAC_PI_2]
    }));
    let values = uniforms(&scene, &Properties::new(), &Attachments::new(), &SunView::default());
    for (actual, expected) in values["g_LSpot_Origin"].iter().zip([5.0, 20.0, 20.0]) {
        assert!((actual - expected).abs() < 0.000_01);
    }
    scene["objects"][1]["visible"] = json!(false);
    assert!(
        uniforms(&scene, &Properties::new(), &Attachments::new(), &SunView::default())
            .values()
            .flatten()
            .all(|value| *value == 0.0)
    );
}

#[test]
fn spot_defaults_and_validation() {
    let mut scene = spot_scene();
    for field in ["innercone", "outercone"] {
        scene["objects"][0].as_object_mut().unwrap().remove(field);
    }
    let values = uniforms(&scene, &Properties::new(), &Attachments::new(), &SunView::default());
    assert_eq!(values["g_LSpot_Origin"][3], 20.0_f32.to_radians().cos());
    assert_eq!(values["g_LSpot_Direction"][3], 30.0_f32.to_radians().cos());
    for (field, value) in [("innercone", json!(-1)), ("angles", json!("bad"))] {
        let mut scene = scene.clone();
        scene["objects"][0][field] = value;
        assert_eq!(load(&scene, &Properties::new()).1.len(), 1);
    }
}

#[test]
fn spot_effect_arrays() {
    let mut scene = spot_scene();
    scene["general"]["lightconfig"]["spot"] = json!(2);
    let model = effect_model(scene);
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let pass = &model.layers[0].effects[0].passes[0];
    let hlsl = &pass.hlsl.as_ref().unwrap().fragment;
    for name in ["g_LSpot_Origin", "g_LSpot_Color", "g_LSpot_Direction", "g_LSpot_Exponent"] {
        assert!(hlsl.contains(&format!("float4 {name}[2]")), "{name}");
    }
    let meta = crate::effects::PassMeta::of(pass);
    let values = BTreeMap::from([("g_LSpot_Color".into(), vec![3.0; 8])]);
    let bytes = meta.uniform_bytes_with(
        crate::effects::FrameClock::default(),
        None,
        256,
        256,
        (256, 256),
        &[],
        &values,
        false,
    );
    let uniform = pass
        .fragment
        .uniforms
        .iter()
        .find(|uniform| uniform.name.starts_with("g_LSpot_Color"))
        .unwrap();
    let expected: Vec<_> =
        values["g_LSpot_Color"].iter().flat_map(|value| value.to_ne_bytes()).collect();
    assert_eq!(&bytes[uniform.offset..uniform.offset + expected.len()], expected);
}

#[test]
fn disabled_configuration() {
    let mut scene = scene();
    scene["general"] = json!({});
    let (config, skipped) = load(&scene, &Properties::new());
    assert!(skipped.is_empty());
    assert_eq!(config.count, 0);
    assert!(config.values.is_empty());
}

#[test]
fn hidden_point_clears_slot() {
    let mut scene = scene();
    scene["objects"][0]["visible"] = json!(false);
    let values = uniforms(&scene, &Properties::new(), &Attachments::new(), &SunView::default());
    assert_eq!(values[ORIGIN], [0.0; 4]);
    assert_eq!(values[COLOR], [0.0; 4]);
}

#[test]
fn unsupported_lights() {
    let mut scene = scene();
    scene["objects"][0]["light"] = json!("tube");
    assert_eq!(load(&scene, &Properties::new()).1.len(), 1);
}

#[test]
fn point_defaults() {
    for (field, origin, color) in [
        (COLOR_FIELD, [64.0, 128.0, 64.0, 2.0], [0.0, 0.0, 0.0, 512.0]),
        (INTENSITY, [64.0, 128.0, 64.0, 2.0], [0.0, 0.0, 0.0, 512.0]),
        (RADIUS, [64.0, 128.0, 64.0, 2.0], [0.4, 0.8, 1.6, 1.0]),
        (EXPONENT, [64.0, 128.0, 64.0, 2.0], [0.4, 0.8, 1.6, 512.0]),
    ] {
        let mut scene = scene();
        scene["objects"][0].as_object_mut().unwrap().remove(field);
        let (config, skipped) = load(&scene, &Properties::new());
        assert!(skipped.is_empty());
        assert_eq!(config.values[ORIGIN], origin);
        assert_eq!(config.values[COLOR], color);
    }
}

#[test]
fn point_parent_and_update() {
    let mut scene = scene();
    scene["objects"][0]["parent"] = json!(2);
    scene["objects"].as_array_mut().unwrap().push(json!({"id":2,"origin":"32 20 5"}));
    let values = uniforms(&scene, &Properties::new(), &Attachments::new(), &SunView::default());
    assert_eq!(values[ORIGIN], [96.0, 148.0, 69.0, 2.0]);
    scene["objects"][0][INTENSITY] = json!(3);
    let values = uniforms(&scene, &Properties::new(), &Attachments::new(), &SunView::default());
    assert_eq!(values[COLOR], [0.6, 1.2, 2.4, 512.0]);
}

fn effect_model(scene: Value) -> super::super::SceneModel {
    let fragment = b"#include \"common.h\"\nvec3 ComputePBRLightShadow(vec3 n, vec3 l, vec3 v, vec3 a, vec3 c, float r, float e, vec3 s, vec3 f, float rough, float metal, float shadow) { return c; }\n#require LightingV1\nvoid main() { gl_FragColor = vec4(PerformLighting_V1(CAST3(0.0),CAST3(1.0),CAST3(1.0),CAST3(1.0),CAST3(1.0),CAST3(1.0),0.5,0.0),1.0); }\n";
    effect_model_with_fragment(scene, fragment)
}

fn effect_model_with_fragment(mut scene: Value, fragment: &[u8]) -> super::super::SceneModel {
    scene["objects"].as_array_mut().unwrap().push(json!({
        "id":2,"shape":"quad","effects":[{"file":"effects/light.json"}]
    }));
    let scene = scene.to_string();
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", scene.as_bytes()),
        ("effects/light.json", br#"{"passes":[{"material":"materials/light.json"}]}"#),
        ("materials/light.json", br#"{"passes":[{"shader":"light","combos":{"LIGHTING":1}}]}"#),
        ("shaders/light.vert", b"void main() { gl_Position=vec4(0.0); }\n"),
        ("shaders/light.frag", fragment),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    super::super::load_with(&package, &assets).unwrap()
}

#[test]
fn lighting_effect_uniform_bytes() {
    let model = effect_model(scene());
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let pass = &model.layers[0].effects[0].passes[0];
    assert!(pass.hlsl.as_ref().unwrap().fragment.contains("float4 g_LPoint_Origin[1]"));
    let meta = crate::effects::PassMeta::of(pass);
    let bytes = meta.uniform_bytes(
        crate::effects::FrameClock::now(0.0, 0.0),
        None,
        256,
        256,
        (256, 256),
        &[],
    );
    for (name, expected) in
        [(ORIGIN, [64.0_f32, 128.0, 64.0, 2.0]), (COLOR, [0.4, 0.8, 1.6, 512.0])]
    {
        let uniform = pass.fragment.uniforms.iter().find(|u| u.name.starts_with(name)).unwrap();
        let expected: Vec<_> = expected.into_iter().flat_map(f32::to_ne_bytes).collect();
        assert_eq!(&bytes[uniform.offset..uniform.offset + expected.len()], expected);
    }
}

#[test]
fn ordered_visible_points() {
    let mut scene = scene();
    let mut second = scene["objects"][0].clone();
    second["id"] = json!(3);
    second["color"] = json!("0 0 1");
    scene["objects"].as_array_mut().unwrap().push(second);
    scene["general"]["lightconfig"]["point"] = json!(2);
    let (config, skipped) = load(&scene, &Properties::new());
    assert!(skipped.is_empty());
    assert_eq!(config.values[COLOR], [0.4, 0.8, 1.6, 512.0, 0.0, 0.0, 2.0, 512.0]);
    scene["objects"][0]["visible"] = json!(false);
    let (config, skipped) = load(&scene, &Properties::new());
    assert!(skipped.is_empty());
    assert_eq!(config.values[COLOR], [0.0, 0.0, 2.0, 512.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(&config.values[ORIGIN][4..], [0.0; 4]);
}

#[test]
fn point_array_packing() {
    let mut scene = scene();
    scene["general"]["lightconfig"]["point"] = json!(2);
    let model = effect_model(scene);
    assert!(model.skipped.is_empty());
    let pass = &model.layers[0].effects[0].passes[0];
    assert!(pass.hlsl.as_ref().unwrap().fragment.contains("float4 g_LPoint_Origin[2]"));
    let meta = crate::effects::PassMeta::of(pass);
    let values = std::collections::BTreeMap::from([(
        COLOR.to_owned(),
        vec![1.0, 0.0, 0.0, 512.0, 0.0, 0.0, 1.0, 256.0],
    )]);
    let bytes = meta.uniform_bytes_with(
        crate::effects::FrameClock::default(),
        None,
        256,
        256,
        (256, 256),
        &[],
        &values,
        false,
    );
    let uniform =
        pass.fragment.uniforms.iter().find(|uniform| uniform.name.starts_with(COLOR)).unwrap();
    assert_eq!(uniform.count, 2);
    let expected: Vec<_> = values[COLOR].iter().flat_map(|value| value.to_ne_bytes()).collect();
    assert_eq!(&bytes[uniform.offset..uniform.offset + expected.len()], expected);
}

#[test]
fn legacy_mesh_points() {
    let scene = json!({"objects":[
        {"id":1,"model":"models/pistols/pistols.mdl"},
        {"id":2,"light":"point","origin":"-1.110 2.460 0.335",
         "color":"1 1 1","intensity":1.87,"radius":16.32},
        {"id":3,"light":"point","origin":"-1.105 4.002 0.347",
         "color":"0.890 0.690 0.086","intensity":0.18,"radius":22.85}
    ]});
    let (config, skipped) = load(&scene, &Properties::new());
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(&config.values[LEGACY_POSITION][..6], [-1.110, 2.460, 0.335, -1.105, 4.002, 0.347]);
    assert_eq!(&config.values[LEGACY_POSITION][6..], [0.0; 6]);
    let color = &config.values[LEGACY_COLOR_RADIUS];
    assert_eq!(&color[..4], [1.87, 1.87, 1.87, 16.32]);
    for (actual, expected) in
        color[4..8].iter().zip([0.890 * 0.18, 0.690 * 0.18, 0.086 * 0.18, 22.85])
    {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
    assert_eq!(&color[8..], [0.0; 8]);
}

#[test]
fn legacy_point_arrays() {
    let mut scene = scene();
    scene["objects"][0]["light"] = json!("point");
    scene["objects"][0]["model"] = json!("models/test.mdl");
    let (config, skipped) = load(&scene, &Properties::new());
    assert!(skipped.is_empty());
    let fragment = b"uniform vec3 g_LightsPosition[4];\nuniform vec4 g_LightsColorRadius[4];\nvoid main() { gl_FragColor=vec4(g_LightsPosition[0]+g_LightsColorRadius[0].xyz,1.0); }\n";
    let model = effect_model_with_fragment(scene, fragment);
    let pass = &model.layers[0].effects[0].passes[0];
    let bytes = crate::effects::PassMeta::of(pass).uniform_bytes_with(
        crate::effects::FrameClock::default(),
        None,
        256,
        256,
        (256, 256),
        &[],
        &config.values,
        false,
    );
    for (name, expected) in
        [(LEGACY_POSITION, [64.0, 128.0, 64.0, 0.0]), (LEGACY_COLOR_RADIUS, [0.4, 0.8, 1.6, 512.0])]
    {
        let uniform = pass.fragment.uniforms.iter().find(|u| u.name.starts_with(name)).unwrap();
        assert_eq!(uniform.count, LEGACY_CAPACITY);
        for (actual, expected) in bytes[uniform.offset..uniform.offset + 16]
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
            .zip(expected)
        {
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn point_shadow_tiles() {
    let mut scene = scene();
    scene["objects"][0]["castshadow"] = json!(true);
    let lights = lights(&scene, &Properties::new(), &Attachments::new());
    let layout = ShadowLayout::new(&lights).unwrap();
    assert_eq!(layout.extent(), [256, 256]);
    assert_eq!(layout.tile(&lights[0], 0), Some([0, 0, 128, 85]));
    assert_eq!(layout.tile(&lights[0], 5), Some([128, 170, 128, 85]));
    assert_eq!(layout.tile(&lights[0], 6), None);
}

#[test]
fn reverse_shadow_projection() {
    for mut scene in [scene(), spot_scene()] {
        scene["objects"][0]["radius"] = json!(600);
        for source in [0.0, 0.05, 10.0] {
            scene["objects"][0]["lightsourcesize"] = json!(source);
            let lights = lights(&scene, &Properties::new(), &Attachments::new());
            let projection = lights[0].shadow_parameters();
            assert!((projection[0] - 1.0 / 599.0).abs() < 0.000001);
            assert!((projection[1] - 600.0 / 599.0).abs() < 0.000001);
        }
    }
}

#[test]
fn legacy_premultiplied_layout() {
    let light = |id: u32, color: &str, intensity: f32, radius: f32| {
        json!({"id":id,"light":"point","origin":"10 20 30","color":color,
               "intensity":intensity,"radius":radius})
    };
    let scene = json!({"objects":[
        light(1, "1 0.5 0.25", 2.0, 10.0),
        light(2, "0 1 0", 1.0, 3.0),
        light(3, "0 0 1", 0.5, 4.0),
        light(4, "0.5 0.25 1", 4.0, 2.0)
    ]});
    let (config, skipped) = load(&scene, &Properties::new());
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(
        config.values[LEGACY_PREMULTIPLIED],
        [200.0, 100.0, 50.0, 8.0, 0.0, 9.0, 0.0, 4.0, 0.0, 0.0, 8.0, 16.0]
    );
}

#[test]
fn legacy_arrays_absent() {
    let values = uniforms(&scene(), &Properties::new(), &Attachments::new(), &SunView::default());
    assert!(!values.keys().any(|name| name.starts_with(LEGACY_UNIFORM_PREFIX)));
}
