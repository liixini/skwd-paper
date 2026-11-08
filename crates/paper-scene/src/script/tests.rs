use super::runtime::HEAP_LIMIT;
use super::{ParticleOp, SceneScripts, ScriptCommand, SoundOp, SpriteOp};
use crate::model::Properties;
use serde_json::json;

fn stalling_host(scene: &mut serde_json::Value) -> SceneScripts {
    let limits = super::Limits {
        load: std::time::Duration::from_secs(1),
        compose: std::time::Duration::from_millis(100),
    };
    let load = super::Load { limits, ..super::Load::default() };
    SceneScripts::load_with_storage(scene, &Properties::new(), &json!({}), load).unwrap().unwrap()
}

fn image_host(scene: &mut serde_json::Value) -> SceneScripts {
    let source = scene.to_string();
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", source.as_bytes()),
        ("models/white.json", br#"{"material":"materials/white.json"}"#),
        ("materials/white.json", br#"{"passes":[{"shader":"flat"}]}"#),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some(""));
    let images = crate::model::ImageResources::new(&package, &assets);
    SceneScripts::load_with_storage(
        scene,
        &Properties::new(),
        &json!({}),
        super::Load { images: Some(images), ..super::Load::default() },
    )
    .unwrap()
    .unwrap()
}

#[test]
fn created_image_identity() {
    let script = r"
const asset = engine.registerAsset('models/white.json',true);
export function init() {
    const custom = {};
    custom.self = custom;
    thisScene.getLayer('template').custom = custom;
    Object.defineProperty(thisLayer.getEffect(0), 'custom', {value:custom,enumerable:true});
    thisLayer.instance = {};
    Object.defineProperty(thisLayer.instance, 'custom', {value:custom,enumerable:true});
    thisScene.getLayer('template').origin = new Vec3(6,5,0);
    const template = thisScene.getInitialLayerConfig('template');
    delete template.id;
    template.name = 'created';
    template.visible = true;
    template.origin = new Vec3(10,20,0);
    template.size = '30 40';
    const layer = thisScene.createLayer(template);
    const proto = Object.getPrototypeOf(layer);
    layer.color = new Vec3(0,1,1);
    layer.origin = new Vec3(12,20,0);
    const idHidden = layer.id === undefined;
    const typeHidden = layer.type === undefined;
    layer.custom = 731;
    layer.id = 'custom';
    thisLayer.text = JSON.stringify([
        asset, thisScene.getLayerCount(), thisScene.getLayer('created') === layer,
        thisScene.enumerateLayers()[2] === layer, thisScene.getLayerIndex(layer),
        thisScene.getInitialLayerConfig(layer).origin, layer.origin.x,
        idHidden, typeHidden, layer.id, thisScene.getLayer('created') === layer,
        typeof layer.constructor, layer.constructor.name,
        proto !== Object.prototype, Object.prototype.isPrototypeOf(layer),
        Object.prototype.hasOwnProperty.call(proto,'constructor'),
        typeof layer.hasOwnProperty, typeof layer.toString, layer.custom
    ]);
}

";
    let mut scene = json!({"objects":[
            {"id":1,"name":"template","image":"models/white.json","visible":false,"origin":"5 5 0","size":"10 10"},
    {"id":2,"name":"observer","effects":[{"name":"unused"}],"instance":{"native":true},"text":{"value":"","script":script}}
        ]});
    let mut host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"].as_array().unwrap().len(), 3);
    assert_eq!(scene["objects"][2]["origin"], "12 20 0");
    assert_eq!(scene["objects"][2]["color"], "0 1 1");
    assert_eq!(scene["objects"][0]["origin"], "6 5 0");
    assert_eq!(scene["objects"][2]["id"], "@dynamic-2");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(scene["objects"][1]["text"].as_str().unwrap())
            .unwrap(),
        json!([
            "models/white.json",
            3,
            true,
            true,
            2,
            "10 20 0",
            12,
            true,
            true,
            "custom",
            true,
            "function",
            "",
            true,
            true,
            true,
            "function",
            "function",
            731
        ])
    );
    assert_eq!(host.take_created_images().len(), 1);
    assert_eq!(host.created_layer_slots(), [2]);
    assert!(host.take_created_images().is_empty());
}

#[test]
fn registered_asset_layer() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":r"
const asset = engine.registerAsset('models/white.json');
export function init() {
    const layer = thisScene.createLayer(asset);
    if (!layer || thisScene.getLayerCount() !== 2) throw Error('asset layer');
    return thisScene.getInitialLayerConfig(layer).image;
}"}}]});
    let mut host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "models/white.json");
    assert_eq!(host.scene["objects"][1]["image"], "models/white.json");
    assert_eq!(host.take_created_images().len(), 1);
}

#[test]
fn registered_particle_asset() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":r"
const handle = engine.registerAsset('particles/p.json');
let particle;
export function init() {
    particle = thisScene.createLayer(handle);
    particle.emitParticles(2);
    particle.instance.alpha = 0.5;
    particle.instance.count = 2;
    particle.instance.rate = 0.25;
    if (thisScene.getLayerCount() !== 2) throw Error('particle layer');
    return thisScene.getInitialLayerConfig(particle).particle;
}
export function update() {
    if (engine.runtime >= 2) thisScene.destroyLayer(particle);
}"}}]});
    let source = scene.to_string();
    let texture = crate::tests::build_tex("TEXB0002", 0, 0, &[255; 64], false, None);
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", source.as_bytes()),
        ("particles/p.json", br#"{"renderer":[{"name":"sprite"}],"material":"materials/p.json","emitter":[{"name":"boxrandom","rate":1}]}"#),
        ("materials/p.json", br#"{"passes":[{"shader":"genericparticle","textures":["mask"]}]}"#),
        ("materials/mask.tex", &texture),
    ])).unwrap();
    let assets = crate::effects::Assets::discover(Some(""));
    let images = crate::model::ImageResources::new(&package, &assets);
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        super::Load { images: Some(images), ..super::Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["text"], "particles/p.json");
    let created = host.take_created_particles();
    assert_eq!(created.len(), 1);
    let (slot, particle) = created.into_iter().next().unwrap();
    assert_eq!(slot, 1);
    assert_eq!(particle.id, "@dynamic-1");
    assert!(particle.system.texture.is_some());
    assert!(host.take_created_particles().is_empty());
    let commands = host.take_commands();
    assert!(commands.iter().any(|command| matches!(command,
        ScriptCommand::Particle { id, op: ParticleOp::Emit(2) } if id == "@dynamic-1")));
    for expected in [ParticleOp::Alpha(0.5), ParticleOp::Count(2.0), ParticleOp::Rate(0.25)] {
        assert!(commands.iter().any(|command| matches!(command,
            ScriptCommand::Particle { id, op } if id == "@dynamic-1" && *op == expected)));
    }
    host.tick(2.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.take_destroyed_images(), vec![1]);
    assert!(host.take_created_particles().is_empty());
}

#[test]
fn image_effect_handle_shape() {
    let mut scene = json!({"objects":[{"id":1,"name":"image","image":"models/white.json","effects":[{"id":1000,"name":"test","visible":true}],"instance":{"native":true},"visible":{"value":true,"script":r"
export function init() {
    for (const key of ['effects', 'image', 'instance']) {
        if (thisLayer[key] !== undefined || Object.hasOwn(thisLayer, key) || key in thisLayer) throw Error('config exposed');
    }
    const effect = thisLayer.getEffect(0);
    if (!effect || effect !== thisLayer.getEffect(0) || effect !== thisLayer.getEffect('test')) throw Error('effect identity');
    if (effect.name !== 'test' || effect.visible !== true || !Object.hasOwn(effect, 'name') || !Object.hasOwn(effect, 'visible')) throw Error('effect fields');
    if (effect.id !== undefined || Object.hasOwn(effect, 'id') || 'id' in effect) throw Error('effect id');
    if (thisLayer.getEffectCount() !== 1) throw Error('effect count');
    for (const key of [1, -1, 'missing', '0', '1000']) if (thisLayer.getEffect(key) !== null) throw Error('missing effect');
    const config = thisScene.getInitialLayerConfig(thisLayer);
    if (config.image !== 'models/white.json' || config.effects[0].id !== 1000 || !config.instance.native) throw Error('initial config');
    return true;
}"}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["image"], "models/white.json");
    assert_eq!(scene["objects"][0]["effects"][0]["name"], "test");
    assert_eq!(scene["objects"][0]["effects"][0]["id"], 1000);
    assert_eq!(scene["objects"][0]["instance"]["native"], true);
}

#[test]
fn array_extensions_stay_custom() {
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json","effects":[{"name":"test","params":[1]}],"visible":{"value":true,"script":r"
export function init() {
    const values = thisLayer.getEffect(0).params;
    const cycle = {}; cycle.self = cycle;
    const call = () => 7;
    values.push(cycle, call, 1n);
    if (values.length !== 4 || values[1] !== cycle || values[2] !== call || values[3] !== 1n) throw Error('array identity');
    if (values[1].self !== cycle || values[2]() !== 7 || Object.getOwnPropertyDescriptor(values, '3').value !== 1n) throw Error('array values');
    values[0] = 2;
    delete values[0];
    values[0] = 3;
    return true;
}"}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["effects"][0]["params"], json!([3]));
}

#[test]
fn nested_replacement_keeps_native() {
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json","effects":[{"name":"test","nested":{"value":1,"items":[1]}}],"visible":{"value":true,"script":r"
export function init() {
    const effect = thisLayer.getEffect(0);
    effect.nested = {value:2,items:[1]};
    const nested = effect.nested;
    const cycle = {}; cycle.self = cycle;
    nested.custom = cycle;
    nested.big = 1n;
    nested.call = () => 7;
    Object.defineProperty(nested, 'bomb', {enumerable:true, get(){throw Error('getter');}});
    nested.items.push(cycle, 1n);
    if (nested.custom !== cycle || nested.big !== 1n || nested.call() !== 7 || nested.items[1] !== cycle || nested.items[2] !== 1n) throw Error('custom identity');
    nested.value = 3;
    nested.items[0] = 4;
    try { effect.nested = cycle; throw Error('accepted cycle'); } catch (error) { if (error.message === 'accepted cycle') throw error; }
    if (effect.nested !== nested) throw Error('native changed');
    return true;
}"}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["effects"][0]["nested"], json!({"value":3,"items":[4]}));
}

#[test]
fn nested_replacements_keep_order() {
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json","effects":[{"name":"test","nested":{"value":0}}],"visible":{"value":true,"script":r"
export function init() {
    const effect = thisLayer.getEffect(0);
    effect.nested = {value:1};
    effect.nested.value = 2;
    effect.nested = {value:3};
    return true;
}"}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["effects"][0]["nested"]["value"], 3);
}

#[test]
fn native_descriptors_drain() {
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json","alpha":1,"effects":[
        {"name":"data","visible":true},{"name":"getter","visible":true}
    ],"visible":{"value":true,"script":r"
export function init() {
    const data = thisLayer.getEffect(0);
    const getter = thisLayer.getEffect(1);
    Object.defineProperty(data, 'visible', {value:false,writable:true,configurable:true});
    Object.defineProperty(thisLayer, 'alpha', {value:0,writable:true,configurable:true});
    Object.defineProperty(getter, 'visible', {get(){return false},configurable:true});
    const cycle = {}; cycle.self = cycle;
    Object.defineProperty(data, 'custom', {value:cycle,writable:true,configurable:true});
    Object.defineProperty(data, 'big', {value:1n,configurable:true});
    if (data.visible !== false || thisLayer.alpha !== 0 || getter.visible !== false) throw Error('descriptor read');
    if (Object.getOwnPropertyDescriptor(data, 'visible').value !== false) throw Error('data descriptor');
    if (typeof Object.getOwnPropertyDescriptor(getter, 'visible').get !== 'function') throw Error('getter descriptor');
    if (data.custom !== cycle || data.big !== 1n) throw Error('custom descriptor');
    return true;
}"}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["effects"][0]["visible"], false);
    assert_eq!(scene["objects"][0]["effects"][1]["visible"], true);
    assert_eq!(scene["objects"][0]["alpha"], 0);
    assert!(scene["objects"][0]["effects"][0].get("custom").is_none());
    assert!(scene["objects"][0]["effects"][0].get("big").is_none());
}

#[test]
fn readonly_writes_stay_local() {
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json","alpha":1,"origin":"1 2 3","effects":[
        {"name":"locked","visible":true},{"name":"setter","visible":true},{"name":"direct","visible":true}
    ],"visible":{"value":true,"script":r"
export function init() {
    const locked = thisLayer.getEffect(0);
    const setter = thisLayer.getEffect(1);
    const direct = thisLayer.getEffect(2);
    Object.defineProperty(locked, 'visible', {value:false,writable:false,configurable:true});
    Object.defineProperty(thisLayer, 'alpha', {value:1,writable:false,configurable:true});
    const origin = thisLayer.origin;
    Object.defineProperty(origin, 'x', {value:1,writable:false,configurable:true});
    let rejected = 0;
    try { locked.visible = true; } catch (error) { rejected++; }
    try { thisLayer.alpha = 0; } catch (error) { rejected++; }
    try { origin.x = 9; } catch (error) { rejected++; }
    if (rejected !== 3 || locked.visible !== false || thisLayer.alpha !== 1 || thisLayer.origin.x !== 1) throw Error('readonly write');
    let shadow = true;
    Object.defineProperty(setter, 'visible', {get(){return shadow},set(value){shadow=value},configurable:true});
    setter.visible = false;
    if (setter.visible !== false) throw Error('accessor write');
    direct.visible = false;
    return true;
}

export function update(value) {
    if (engine.runtime >= 1) thisLayer.origin.y = 7;
    return value;
}"}}]});
    let mut host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.take_effect_resets(), [0]);
    assert!(host.take_effect_resets().is_empty());
    assert_eq!(scene["objects"][0]["effects"][0]["visible"], false);
    assert_eq!(scene["objects"][0]["effects"][1]["visible"], true);
    assert_eq!(scene["objects"][0]["effects"][2]["visible"], false);
    assert_eq!(scene["objects"][0]["alpha"], 1);
    assert_eq!(scene["objects"][0]["origin"], "1 2 3");
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["origin"], "1 2 3");
}

#[test]
fn effect_visibility_round_trip() {
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json","effects":[{"name":"copy","visible":true}],"visible":{"value":true,"script":r"
export function init() {
    const effect = thisLayer.getEffect(0);
    effect.visible = false;
    effect.visible = false;
    effect.visible = true;
    effect.visible = true;
    return true;
}"}}]});
    let mut host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["effects"][0]["visible"], true);
    assert_eq!(host.take_effect_resets(), [0]);
    assert!(host.take_effect_resets().is_empty());
}

#[test]
fn effect_visibility_same_value() {
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json","effects":[{"name":"copy","visible":true}],"visible":{"value":true,"script":r"
export function init() {
    const effect = thisLayer.getEffect(0);
    effect.visible = true;
    effect.visible = true;
    return true;
}"}}]});
    let mut host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["effects"][0]["visible"], true);
    assert!(host.take_effect_resets().is_empty());
}

#[test]
fn native_fields_copy_values() {
    let script = r"
export function init() {
    const initial = thisScene.getInitialLayerConfig(thisLayer);
    const rounded = Math.fround(0.2);
    if (thisLayer.color.x !== rounded || thisLayer.origin.x !== rounded || thisLayer.alpha !== rounded) throw Error('authored precision');
    if (initial.color !== '0.2 0.2 0.2' || initial.origin !== '0.2 0.2 0.2' || initial.alpha !== 0.2) throw Error('authoring changed');
    const source = new Vec3(0.2);
    const clone = source.copy();
    thisLayer.custom = source;
    thisLayer.color = new Vec3(0);
    thisLayer.origin = new Vec3(0);
    thisLayer.alpha = 0;
    thisLayer.color = source;
    thisLayer.origin = clone;
    thisLayer.alpha = 0.2;
    const color = thisLayer.color;
    const origin = thisLayer.origin;
    if (color === thisLayer.color || origin === thisLayer.origin || color === source || origin === clone) throw Error('native alias');
    if (source.x !== 0.2 || clone.x !== 0.2 || thisLayer.custom !== source) throw Error('source changed');
    source.x = 0.75;
    clone.x = 0.75;
    if (color.x !== rounded || origin.x !== rounded) throw Error('copy changed');
    color.x = 0.9;
    origin.x = 0.9;
    thisLayer.color.x = 0.8;
    thisLayer.origin.x = 0.8;
    if (thisLayer.color.x !== rounded || thisLayer.origin.x !== rounded || thisLayer.alpha !== rounded) throw Error('native copy changed');
    return true;
}";
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json",
        "color":"0.2 0.2 0.2","origin":"0.2 0.2 0.2","alpha":0.2,
        "visible":{"value":true,"script":script}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["alpha"], f64::from(0.2_f32));
    for key in ["color", "origin"] {
        assert_eq!(
            scene["objects"][0][key],
            "0.20000000298023224 0.20000000298023224 0.20000000298023224"
        );
    }
    assert!(scene["objects"][0].get("custom").is_none());
}

#[test]
fn kind_scalars_round() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"image","image":"models/white.json","brightness":1.0},
        {"id":2,"name":"sound","sound":"sounds/quiet.wav","volume":1.0},
        {"id":3,"text":{"value":"","script":r"
export function init() {
    const image = thisScene.getLayer('image');
    const sound = thisScene.getLayer('sound');
    const value = 0.123456789;
    for (const [layer, key] of [[image, 'brightness'], [sound, 'volume']]) {
        layer[key] = value;
        if (layer[key] !== Math.fround(value) || Object.getOwnPropertyDescriptor(layer, key).value !== Math.fround(value)) throw Error('assignment precision');
        Object.defineProperty(layer, key, {value:0.8123456789,writable:true,configurable:true});
        if (layer[key] !== Math.fround(0.8123456789) || Object.getOwnPropertyDescriptor(layer, key).value !== Math.fround(0.8123456789)) throw Error('descriptor precision');
    }
    return 'ready';
}"}}
    ]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let rounded = f64::from(0.812_345_7_f32);
    assert_eq!(scene["objects"][0]["brightness"], rounded);
    assert_eq!(scene["objects"][1]["volume"], rounded);
    assert_eq!(scene["objects"][2]["text"], "ready");
}

#[test]
fn numeric_fields_match_native() {
    let cases: [(&str, &[&str], &[&str]); 4] = [
        ("image", &["brightness", "alpha"], &["colorBlendMode", "sortorder"]),
        (
            "text",
            &[
                "blursize",
                "outlinethickness",
                "dropshadowopacity",
                "maxwidth",
                "dropshadowsize",
                "pointsize",
                "backgroundbrightness",
                "brightness",
                "alpha",
            ],
            &["maxrows", "colorBlendMode", "sortorder"],
        ),
        ("sound", &["mindistance", "maxtime", "mintime", "attenuation", "volume"], &["sortorder"]),
        ("particle", &[], &["sortorder"]),
    ];
    for (kind, floats, integers) in cases {
        let mut target = json!({"id":1,"name":"target"});
        target[kind] = json!("fixture");
        for key in floats.iter().chain(integers) {
            target[*key] = json!(0.312_345_678_9);
        }
        let mut scene = json!({"objects":[target,{"id":2,"probe":{"floats":floats,"integers":integers},"text":{"value":"","script":r"
export function init() {
    const target = thisScene.getLayer('target');
    const probe = thisScene.getInitialLayerConfig(thisLayer).probe;
    for (const [keys, convert] of [[probe.floats, Math.fround], [probe.integers, Math.trunc]]) {
        for (const key of keys) {
            if (target[key] !== convert(0.3123456789) || Object.getOwnPropertyDescriptor(target,key).value !== target[key]) throw Error('initial number ' + key);
            target[key] = 0.8123456789;
            if (target[key] !== convert(0.8123456789) || Object.getOwnPropertyDescriptor(target,key).value !== target[key]) throw Error('assigned number ' + key);
            Object.defineProperty(target,key,{value:2.8123456789,enumerable:true,configurable:true,writable:true});
            if (target[key] !== convert(2.8123456789) || Object.getOwnPropertyDescriptor(target,key).value !== target[key]) throw Error('defined number ' + key);
            if (thisScene.getInitialLayerConfig(target)[key] !== 0.3123456789) throw Error('authored number ' + key);
        }
    }
    return 'ready';
}"}}]});
        let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        assert!(host.diagnostics.is_empty(), "{kind}: {:?}", host.diagnostics);
        for key in floats {
            assert_eq!(scene["objects"][0][*key], 2.812_345_743_179_321_3);
        }
        for key in integers {
            assert_eq!(scene["objects"][0][*key], 2);
        }
        assert_eq!(scene["objects"][1]["text"], "ready");
    }
}

#[test]
fn numeric_defaults_stay_local() {
    for (kind, defaults) in [
        ("image", json!({"colorBlendMode":0,"sortorder":0})),
        (
            "text",
            json!({"blursize":6,"outlinethickness":4,"dropshadowopacity":1,"dropshadowsize":6,"colorBlendMode":0,"sortorder":0}),
        ),
        ("sound", json!({"mindistance":1,"maxtime":5,"mintime":1,"attenuation":1,"sortorder":0})),
        ("particle", json!({"sortorder":0})),
    ] {
        let mut target = json!({"id":1,"name":"target"});
        target[kind] = json!("fixture");
        let mut scene = json!({"objects":[target.clone(),{"id":2,"probe":defaults,"text":{"value":"","script":r"
export function init() {
    const target = thisScene.getLayer('target');
    const defaults = thisScene.getInitialLayerConfig(thisLayer).probe;
    const initial = thisScene.getInitialLayerConfig(target);
    for (const [key,value] of Object.entries(defaults)) {
        if (target[key] !== value || Object.getOwnPropertyDescriptor(target,key).value !== value) throw Error('numeric default ' + key);
        if (Object.hasOwn(initial,key)) throw Error('default authored ' + key);
    }
    return 'ready';
}"}}]});
        let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        assert!(host.diagnostics.is_empty(), "{kind}: {:?}", host.diagnostics);
        assert_eq!(scene["objects"][0], target);
        assert_eq!(scene["objects"][1]["text"], "ready");
    }
}

#[test]
fn visual_size_copies() {
    for kind in ["image", "text"] {
        let mut target = json!({"id":1,"name":"target","size":"10 20"});
        target[kind] = json!("fixture");
        let mut scene = json!({"objects":[
            target,
            {"id":2,"text":{"value":"","script":r"
export function init() {
    const target = thisScene.getLayer('target');
    const source = new Vec2(0.123456789, 0.8123456789);
    target.size = source;
    const first = target.size;
    if (!(first instanceof Vec2) || first === target.size || first === source) throw Error('size copy');
    if (first.x !== Math.fround(source.x) || first.y !== Math.fround(source.y)) throw Error('size precision');
    first.x = 1;
    source.y = 1;
    if (target.size.x !== Math.fround(0.123456789) || target.size.y !== Math.fround(0.8123456789)) throw Error('size alias');
    return 'ready';
}"}}
        ]});
        let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(scene["objects"][1]["text"], "ready");
        assert_eq!(scene["objects"][0]["size"], "0.12345679104328156 0.8123456835746765");
    }
}

#[test]
fn native_descriptors_round_values() {
    let script = r"
export function init() {
    Object.defineProperty(thisLayer, 'alpha', {value:0.2,writable:true,configurable:true});
    if (thisLayer.alpha !== Math.fround(0.2) || Object.getOwnPropertyDescriptor(thisLayer, 'alpha').value !== Math.fround(0.2)) throw Error('descriptor precision');
    const source = new Vec3(0.2);
    Object.defineProperty(thisLayer, 'color', {get(){return source},configurable:true});
    if (thisLayer.color !== source || thisLayer.color.x !== 0.2) throw Error('getter shadow');
    Object.defineProperty(thisLayer, 'origin', {writable:false});
    let rejected = false;
    try {thisLayer.origin = source;} catch (error) {rejected = true;}
    if (!rejected || thisLayer.origin.x !== Math.fround(0.2)) throw Error('readonly origin');
    Object.freeze(thisLayer);
    if (thisLayer.origin !== thisLayer.origin || thisLayer.origin.x !== Math.fround(0.2)) throw Error('frozen invariant');
}";
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json",
        "color":"1 1 1","origin":"0.2 0.2 0.2","alpha":1,
        "visible":{"value":true,"script":script}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["alpha"], f64::from(0.2_f32));
    assert_eq!(scene["objects"][0]["color"], "1 1 1");
    assert_eq!(scene["objects"][0]["origin"], "0.2 0.2 0.2");
}

#[test]
fn native_fixed_descriptors() {
    let script = r"
export function init() {
    const source = new Vec3(0.2);
    Object.defineProperty(thisLayer, 'color', Object.freeze({value:source,writable:false,configurable:false}));
    if (thisLayer.color === source || thisLayer.color.x !== Math.fround(0.2)) throw Error('fixed vector');
    if (Object.getOwnPropertyDescriptor(thisLayer, 'color').value === source) throw Error('fixed descriptor');
    if (!Reflect.defineProperty(thisLayer, 'alpha', {value:0.2,writable:false,configurable:false})) throw Error('fixed alpha');
    if (thisLayer.alpha !== Math.fround(0.2) || Object.getOwnPropertyDescriptor(thisLayer, 'alpha').value !== Math.fround(0.2)) throw Error('alpha precision');
    if (Reflect.defineProperty(thisLayer, 'alpha', {value:0.3}) || thisLayer.alpha !== Math.fround(0.2)) throw Error('readonly alpha');
    const ordinary = {};
    Object.defineProperty(ordinary, 'color', {value:source});
    const sound = thisScene.getLayer('sound');
    Object.defineProperty(sound, 'color', {value:source});
    if (ordinary.color !== source || sound.color !== source || source.x !== 0.2) throw Error('custom identity');
    const bulk = thisScene.getLayer('bulk');
    Object.defineProperties(bulk, Object.freeze({color:Object.freeze({value:source,writable:false,configurable:false}),alpha:Object.freeze({value:0.2,writable:false,configurable:false})}));
    if (bulk.color === source || bulk.color.x !== Math.fround(0.2) || bulk.alpha !== Math.fround(0.2)) throw Error('fixed bulk');
    Object.freeze(thisLayer);
    if (thisLayer.color.x !== Math.fround(0.2)) throw Error('frozen layer');
}";
    let mut scene = json!({"objects":[
        {"id":1,"image":"models/white.json","color":"1 1 1","alpha":1,"visible":{"value":true,"script":script}},
        {"id":2,"name":"bulk","image":"models/white.json","color":"1 1 1","alpha":1},
        {"id":3,"name":"sound","sound":"test.mp3"}
    ]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    for slot in [0, 1] {
        assert_eq!(
            scene["objects"][slot]["color"],
            "0.20000000298023224 0.20000000298023224 0.20000000298023224"
        );
        assert_eq!(scene["objects"][slot]["alpha"], f64::from(0.2_f32));
    }
    assert!(scene["objects"][2].get("color").is_none());
}

#[test]
fn descriptor_validation_precedes_conversion() {
    let script = r"
export function init() {
    const order = [];
    const source = new Vec3(0.2);
    Object.defineProperty(source, 'x', {get(){order.push('x');throw Error('premature conversion');}});
    for (const define of [Object.defineProperty, Reflect.defineProperty]) {
        let invalid = false;
        try { define(thisLayer, 'color', {value:source,get(){return source;}}); } catch (error) { invalid = error instanceof TypeError; }
        if (!invalid || order.length) throw Error('mixed descriptor');
    }
    let stopped = false;
    const descriptors = {
        get color(){if(this !== descriptors) throw Error('bulk receiver');order.push('color');return {value:source};},
        get alpha(){order.push('alpha');throw Error('stop');}
    };
    try {Object.defineProperties(thisLayer, descriptors);} catch (error) {stopped = error.message === 'stop';}
    if (!stopped || order.join(',') !== 'color,alpha' || thisLayer.color.x !== 1) throw Error('bulk validation');
}";
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json","color":"1 1 1",
        "visible":{"value":true,"script":script}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["color"], "1 1 1");
}

#[test]
fn descriptor_conversion_follows_commits() {
    let script = r"
export function init() {
    const source = new Vec3(0.2);
    let sawCommit = false;
    Object.defineProperty(source, 'x', {get(){sawCommit = thisLayer.alpha === Math.fround(0.3);throw Error('stop');}});
    let stopped = false;
    try {Object.defineProperties(thisLayer, {alpha:{value:0.3},color:{value:source}});} catch (error) {stopped = error.message === 'stop';}
    if (!stopped || !sawCommit || thisLayer.alpha !== Math.fround(0.3) || thisLayer.color.x !== 1) throw Error('conversion order');
    const alpha = Object.getOwnPropertyDescriptor(thisLayer, 'alpha');
    if (!alpha.writable || !alpha.configurable || !alpha.enumerable) throw Error('omitted attributes');
    Object.defineProperty(thisLayer, 'custom', {value:1,writable:true,configurable:true,enumerable:false});
    Object.defineProperties(thisLayer, {custom:{value:2}});
    const custom = Object.getOwnPropertyDescriptor(thisLayer, 'custom');
    if (custom.value !== 2 || !custom.writable || !custom.configurable || custom.enumerable) throw Error('custom attributes');
}";
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json","color":"1 1 1","alpha":1,
        "visible":{"value":true,"script":script}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["alpha"], f64::from(0.3_f32));
    assert_eq!(scene["objects"][0]["color"], "1 1 1");
    assert!(scene["objects"][0].get("custom").is_none());
}

#[test]
fn descriptor_builtins_keep_contracts() {
    let script = r"
export function init() {
    const order = [];
    const source = new Vec3(0.2);
    Object.defineProperty(source, 'x', {get(){order.push('x');return 0.2;}});
    const descriptor = {
        get value(){if(this !== descriptor) throw Error('value receiver');order.push('value');return source;},
        get writable(){if(this !== descriptor) throw Error('writable receiver');order.push('writable');return true;}
    };
    let coercions = 0;
    const key = {[Symbol.toPrimitive](){coercions++;return 'origin';}};
    Object.defineProperty(thisLayer, key, descriptor);
    if (coercions !== 1 || order.join(',') !== 'value,writable,x' || thisLayer.origin.x !== Math.fround(0.2)) throw Error('descriptor order');
    for (const [owner, name, arity] of [[Object,'defineProperty',3],[Reflect,'defineProperty',3],[Object,'defineProperties',2]]) {
        const method = owner[name];
        const flags = Object.getOwnPropertyDescriptor(owner, name);
        if (method.name !== name || method.length !== arity || Object.hasOwn(method, 'prototype')) throw Error('method metadata');
        if (!flags.writable || !flags.configurable || flags.enumerable) throw Error('method attributes');
        let rejected = false;
        try {new method();} catch (error) {rejected = error instanceof TypeError;}
        if (!rejected) throw Error('method constructor');
    }
    for (const define of [Object.defineProperty, Reflect.defineProperty]) {
        let rejected = false;
        try {define(null, key, {value:source});} catch (error) {rejected = error instanceof TypeError;}
        if (!rejected || coercions !== 1) throw Error('invalid target');
        rejected = false;
        try {define(thisLayer, 'alpha', null);} catch (error) {rejected = error instanceof TypeError;}
        if (!rejected) throw Error('invalid descriptor');
    }
}";
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json","origin":"1 1 1",
        "visible":{"value":true,"script":script}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(
        scene["objects"][0]["origin"],
        "0.20000000298023224 0.20000000298023224 0.20000000298023224"
    );
}

#[test]
fn native_field_callbacks_round() {
    let script = r"
export function init(value) {
    if (!(value instanceof Vec3) || value.x !== Math.fround(0.2)) throw Error('input precision');
    return new Vec3(0.3);
}
export function update(value) {
    if (value.x !== Math.fround(0.3)) throw Error('return precision');
    value.x = 0.4;
    if (thisLayer.color.x !== Math.fround(0.3)) throw Error('callback alias');
}";
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json",
        "color":{"value":"0.2 0.2 0.2","script":script}}]});
    let mut host = image_host(&mut scene);
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(
        host.scene["objects"][0]["color"],
        "0.30000001192092896 0.30000001192092896 0.30000001192092896"
    );
}

#[test]
fn stale_vectors_stay_local() {
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json","origin":"1 2 3","color":"1 1 1","visible":{"value":true,"script":r"
let oldOrigin;
let oldColor;
let stale = false;
let live = false;
export function init() {
    oldOrigin = thisLayer.origin;
    oldColor = thisLayer.color;
    thisLayer.origin = new Vec3(4,5,6);
    thisLayer.color = new Vec3(0.2,0.3,0.4);
    if (oldOrigin === thisLayer.origin || oldColor === thisLayer.color) throw Error('vector not replaced');
    return true;
}
export function update(value) {
    if (engine.runtime >= 1 && !stale) {
        stale = true;
        oldOrigin.x = 9;
        oldColor.z = 0.9;
        if (oldOrigin.x !== 9 || oldColor.z !== 0.9) throw Error('old alias did not mutate');
        if (thisLayer.origin.x !== 4 || thisLayer.color.z !== Math.fround(0.4)) throw Error('old alias changed current');
    }
    if (engine.runtime >= 2 && !live) {
        live = true;
        thisLayer.origin.y = 7;
        thisLayer.color.x = 0.8;
    }
    return value;
}"}}]});
    let mut host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["origin"], "4 5 6");
    assert_eq!(
        host.scene["objects"][0]["color"],
        "0.20000000298023224 0.30000001192092896 0.4000000059604645"
    );
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["origin"], "4 5 6");
    assert_eq!(
        host.scene["objects"][0]["color"],
        "0.20000000298023224 0.30000001192092896 0.4000000059604645"
    );
    host.tick(2.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["origin"], "4 5 6");
    assert_eq!(
        host.scene["objects"][0]["color"],
        "0.20000000298023224 0.30000001192092896 0.4000000059604645"
    );
}

#[test]
fn created_image_lifecycle() {
    let script = r"
let layer;
export function init() { thisLayer.text = String(thisScene.getLayer('late') === null); }
export function update() {
    if (engine.runtime >= 1 && !layer) {
        layer = thisScene.createLayer({name:'late',image:'models/white.json',origin:'10 20 0',size:'30 40',color:'0 1 1'});
        layer.custom = 731;
        thisLayer.text = JSON.stringify([thisScene.getLayerCount(),thisScene.getLayer('late')===layer,thisScene.getLayerIndex(layer)]);
    } else if (engine.runtime >= 2 && layer && thisScene.getLayer('late') === layer) {
        const result = thisScene.destroyLayer(layer);
        thisLayer.text = JSON.stringify([result,thisScene.getLayerCount(),thisScene.getLayer('late')===null,layer.name,layer.visible]);
    } else if (engine.runtime >= 3) {
        thisLayer.text = JSON.stringify([layer.name,layer.visible,layer.custom,thisScene.getLayer('late')===null]);
    }
    return thisLayer.text;
}
";
    let mut scene = json!({"objects":[
        {"id":1,"name":"observer","text":{"value":"","script":script}}
    ]});
    let mut host = image_host(&mut scene);
    assert!(host.take_created_images().is_empty());
    assert_eq!(scene["objects"][0]["text"], "true");
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"].as_array().unwrap().len(), 2);
    assert_eq!(host.take_created_images().len(), 1);
    assert_eq!(host.created_layer_slots(), [1]);
    assert_eq!(host.scene["objects"][0]["text"], "[2,true,1]");
    host.tick(2.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "[true,1,true,\"late\",true]");
    assert_eq!(host.scene["objects"][1]["visible"], false);
    assert_eq!(host.take_destroyed_images(), vec![1]);
    host.tick(3.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "[null,null,731,true]");
}

#[test]
fn invalid_image_rejected() {
    let script = r"
export function init() {
    let failed = 0;
    for (const config of [
        {name:'sound',sound:'sound.wav'},
        {name:'scripted',image:'models/white.json',color:{value:'0 1 1',script:'export function update(){}'}}
    ]) {
        try { thisScene.createLayer(config); } catch (_) { failed++; }
    }
    thisLayer.text = JSON.stringify([failed,thisScene.getLayerCount(),thisScene.getLayer('sound')]);
}
";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host = image_host(&mut scene);
    assert_eq!(scene["objects"].as_array().unwrap().len(), 1);
    assert_eq!(scene["objects"][0]["text"], "[2,1,null]");
    assert!(host.take_created_images().is_empty());
}

#[test]
fn slow_init_completes() {
    let script = r"
export function init() {
    const start = Date.now();
    while (Date.now() - start < 150) {}
    thisLayer.text = 'done';
}
";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "done");
}

#[test]
fn created_child_follows_parent() {
    let script = r"
export function init() {
    thisScene.createLayer({image:'models/white.json', parent:2, origin:'10 0 0', size:'10 10'});
}
";
    let mut scene = json!({"objects":[
        {"id":1,"text":{"value":"","script":script}},
        {"id":2,"image":"models/white.json","origin":"100 50 0","scale":"2 2 1","size":"10 10"}
    ]});
    let mut host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let created = host.take_created_images();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].1.layer.center, (120.0, 1030.0));
}

#[test]
fn missing_image_placeholder() {
    let script = r"
export function init() {
    const layer = thisScene.createLayer('models/missing.json');
    thisLayer.text = JSON.stringify([layer !== null, thisScene.getLayerCount()]);
}
";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host = image_host(&mut scene);
    assert_eq!(host.scene["objects"][0]["text"], "[true,2]");
    let created = host.take_created_images();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].1.layer.texture.width, 40);
}

#[test]
fn keyframe_creation_rejected() {
    let script = r"
export function init() {
    let rejected = false;
    try {
        thisScene.createLayer({name:'animated',image:'models/white.json',alpha:{value:1,animation:{options:{fps:30,length:30},c0:[0,1]}}});
    } catch (_) { rejected = true; }
    const count = thisScene.getLayerCount();
    const valid = thisScene.createLayer({name:'valid',image:'models/white.json'});
    thisLayer.text = JSON.stringify([rejected,count,thisScene.getLayerCount(),thisScene.getLayer('animated')===null,thisScene.getLayer('valid')===valid]);
}
";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host = image_host(&mut scene);
    assert_eq!(scene["objects"].as_array().unwrap().len(), 2);
    assert_eq!(scene["objects"][0]["text"], "[true,1,2,true,true]");
    assert_eq!(host.take_created_images().len(), 1);
    assert_eq!(host.created_layer_slots(), [1]);
}

#[test]
fn created_sort_accepted() {
    let script = r"
export function update() {
    if (engine.runtime < 1 || shared.done) return;
    shared.done = true;
    const layer = thisScene.createLayer({name:'sorted',image:'models/white.json'});
    thisScene.sortLayer(layer,0);
}
";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host = image_host(&mut scene);
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.take_commands(), vec![ScriptCommand::Sort { order: vec![1, 0] }]);
    assert_eq!(host.created_layer_slots(), [1]);
}

#[test]
fn created_tiny_lifecycle() {
    let script = r"
export function init() {
    const layer = thisScene.createLayer({name:'brief',image:'models/white.json'});
    thisLayer.text = JSON.stringify([thisScene.destroyLayer(layer),thisScene.getLayerCount(),thisScene.getLayer('brief')]);
}
";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host = image_host(&mut scene);
    assert_eq!(scene["objects"].as_array().unwrap().len(), 2);
    assert_eq!(scene["objects"][0]["text"], "[true,1,null]");
    assert_eq!(scene["objects"][1]["visible"], false);
    assert_eq!(host.take_created_images().len(), 1);
    assert_eq!(host.take_destroyed_images(), vec![1]);
}

#[test]
fn no_scripts_creates_no_runtime_and_preserves_scene() {
    let mut scene = json!({"objects":[{"id":1,"alpha":0.5}]});
    let original = scene.clone();
    assert!(
        SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
            .unwrap()
            .is_none()
    );
    assert_eq!(scene, original);
}

#[test]
fn hot_properties_preserve_script_state_and_reset_bound_defaults() {
    let project = json!({"general":{"properties":{"format":{"type":"bool","value":true},"opacity":{"type":"slider","value":1.0},"tint":{"type":"color","value":"1 1 1"}}}});
    let mut scene = json!({"objects":[{"id":1,"alpha":{"user":"opacity","value":1.0},"color":{"user":"tint","value":"1 1 1"},"text":{"value":"", "scriptproperties":{"clock":{"user":"format","value":true}},"script":"let frames=0; const props=createScriptProperties().addCheckbox({name:'clock',value:true}).finish(); export function update(){return (props.clock?'12':'24')+':'+(++frames);}"}}]});
    let defaults = crate::effects::parse_properties(&project);
    let mut host = SceneScripts::load(&mut scene, &defaults, &project).unwrap().unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "");
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "12:1");
    let changed = Properties::from([
        ("format".into(), vec![0.0]),
        ("opacity".into(), vec![0.25]),
        ("tint".into(), vec![1.0, 0.0, 0.0]),
    ]);
    assert!(host.update_properties(&changed).unwrap());
    assert!(host.tick(1.0, 0.016, [0.5; 2]).unwrap());
    assert_eq!(host.scene["objects"][0]["text"], "24:2");
    assert_eq!(host.scene["objects"][0]["alpha"], 0.25);
    assert_eq!(host.scene["objects"][0]["color"], "1 0 0");
    assert_eq!(host.properties["opacity"], vec![0.25]);
    assert!(host.update_properties(&Properties::new()).unwrap());
    host.tick(2.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "12:3");
    assert_eq!(host.scene["objects"][0]["alpha"], 1.0);
    assert_eq!(host.scene["objects"][0]["color"], "1 1 1");
}

#[test]
fn hot_properties_call_the_existing_user_property_listener() {
    let project = json!({"general":{"properties":{"opacity":{"type":"slider","value":1.0},"stable":{"type":"bool","value":true}}}});
    let mut scene = json!({"objects":[{"id":1,"alpha":{"user":"opacity","value":1.0},"text":{"value":"", "script":"let calls=0; export function applyUserProperties(props){thisLayer.text=String(++calls)+':'+props.opacity+':'+Object.keys(props).sort().join(',')+':'+engine.userProperties.stable;}"}}]});
    let mut host =
        SceneScripts::load(&mut scene, &crate::effects::parse_properties(&project), &project)
            .unwrap()
            .unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "1:1:opacity,stable:true");
    assert!(host.update_properties(&Properties::from([("opacity".into(), vec![0.5])])).unwrap());
    assert!(host.tick(1.0, 0.016, [0.5; 2]).unwrap());
    assert_eq!(host.scene["objects"][0]["text"], "2:0.5:opacity:true");
}

#[test]
fn deferred_compositions_reload_before_changing_their_visibility_or_ancestry() {
    let project = json!({"general":{"properties":{
        "location":{"type":"slider","value":1},
        "visible":{"type":"bool","value":false},
        "opacity":{"type":"slider","value":1}
    }}});
    let mut scene = json!({"objects":[
        {"id":1,"visible":{"user":{"name":"location","condition":"2"},"value":true}},
        {"id":"composition","parent":1,"visible":{"user":"visible","value":false}},
        {"id":3,"alpha":{"user":"opacity","value":1},"text":{"value":"", "script":"let ticks=0; export function update(){return String(++ticks);}"}}
    ]});
    let mut host =
        SceneScripts::load(&mut scene, &crate::effects::parse_properties(&project), &project)
            .unwrap()
            .unwrap();
    host.restrict_hidden_layer_updates(&["composition".into()]);
    let original = host.scene.clone();
    for (name, value) in [("location", 2.0), ("visible", 1.0)] {
        assert!(!host.update_properties(&Properties::from([(name.into(), vec![value])])).unwrap());
        assert_eq!(host.scene, original);
        assert_eq!(host.properties, crate::effects::parse_properties(&project));
    }
    assert!(host.update_properties(&Properties::from([("opacity".into(), vec![0.5])])).unwrap());
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][2]["alpha"], 0.5);
    assert_eq!(host.scene["objects"][2]["text"], "1");
}

#[test]
fn deferred_compositions_reject_callbacks_before_they_can_reveal_a_layer() {
    let project = json!({"general":{"properties":{"opacity":{"type":"slider","value":1}}}});
    let mut scene = json!({"objects":[
        {"id":1,"name":"composition","visible":false},
        {"id":2,"alpha":{"user":"opacity","value":1},"text":{"value":"", "script":"let calls=0; export function applyUserProperties(props){thisLayer.text=String(++calls); thisScene.getLayer('composition').visible=props.opacity<0.5;}"}}
    ]});
    let mut host =
        SceneScripts::load(&mut scene, &crate::effects::parse_properties(&project), &project)
            .unwrap()
            .unwrap();
    host.restrict_hidden_layer_updates(&["1".into()]);
    let original = host.scene.clone();
    assert!(!host.update_properties(&Properties::from([("opacity".into(), vec![0.25])])).unwrap());
    assert_eq!(host.scene, original);
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["visible"], false);
    assert_eq!(host.scene["objects"][1]["text"], "1");
    assert_eq!(host.properties["opacity"], [1.0]);
}

#[test]
fn first_update_clock() {
    let script = r"
let count = 0;
export function init() { return 0.25; }
export function update() {
    if (engine.frametime === 0) throw Error('zero delta');
    return ++count;
}
";
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":0,"script":script}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert_eq!(scene["objects"][0]["alpha"], 0.25);
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["alpha"], 1);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn modules_keep_independent_state_and_init_return_values() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":0.0,"script":"let n=0; export function init(value){return 0.25;} export function update(value){n++; return value+0.1;}"}},{"id":2,"alpha":{"value":0.0,"script":"let n=0; export function update(value){n++; return n;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert!((host.scene["objects"][0]["alpha"].as_f64().unwrap() - 0.35).abs() < 1e-6);
    assert_eq!(host.scene["objects"][1]["alpha"], 1);
}

#[test]
fn vector_returns_and_cross_layer_writes_reach_scene() {
    let mut scene = json!({"objects":[{"id":1,"name":"first","origin":{"value":"10 20 0","script":"import * as WEMath from 'WEMath'; export function update(value){thisScene.getLayer('other').visible=false; return value.add(new Vec3(1,2,0));}"}},{"id":2,"name":"other","visible":true}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["origin"], "11 22 0");
    assert_eq!(host.scene["objects"][1]["visible"], false);
}

#[test]
fn math_converts_and_interpolates() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":r"
import * as WEMath from 'WEMath';
export function update() {
    return [
        typeof WEMath.deg2rad === 'number',
        typeof WEMath.rad2deg === 'number',
        Math.abs(180 * WEMath.deg2rad - Math.PI) < 0.00001,
        Math.abs(Math.PI * WEMath.rad2deg - 180) < 0.0001,
        WEMath.mix(2, 10, 0.25) === 4,
        WEMath.smoothStep(0, 10, 5) === 0.5,
        WEMath.smoothstep(0, 10, 5) === 0.5,
        WEMath.radians(180) === Math.PI,
        Math.abs(WEMath.degrees(Math.PI) - 180) < 0.0001
    ].join(',');
}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "true,true,true,true,true,true,true,true,true");
}

#[test]
fn vector_module_converts_angles() {
    let cases: [(f64, f64, f64, f64); 6] = [
        (0.0, 1.0, 0.0, 0.0),
        (90.0, 0.0, 1.0, 90.0),
        (-90.0, 0.0, -1.0, -90.0),
        (450.0, 0.0, 1.0, 90.0),
        (180.0, -1.0, 0.0, 180.0),
        (-135.0, -1.0, -1.0, -135.0),
    ];
    for (angle, x, y, expected_angle) in cases {
        let source = format!(
            r"
import * as WEVector from 'WEVector';
export function update() {{
    const direction = WEVector.angleVector2({angle});
    thisLayer.origin = new Vec3(direction.x, direction.y, 0);
    return String(WEVector.vectorAngle2(new Vec2({x}, {y})));
}}"
        );
        let mut scene =
            json!({"objects":[{"id":1,"origin":"0 0 0","text":{"value":"","script":source}}]});
        let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
            .unwrap()
            .unwrap();
        host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        let origin = crate::effects::json_numbers(&host.scene["objects"][0]["origin"]).unwrap();
        assert!((f64::from(origin[0]) - angle.to_radians().cos()).abs() < 0.00001);
        assert!((f64::from(origin[1]) - angle.to_radians().sin()).abs() < 0.00001);
        let actual_angle =
            host.scene["objects"][0]["text"].as_str().unwrap().parse::<f64>().unwrap();
        assert!((actual_angle - expected_angle).abs() < 0.0001);
    }

    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":r"
import * as WEVector from 'WEVector';
export function update() {
    return [WEVector.vectorAngle2(new Vec2(0, 0)),
        WEVector.lerp(new Vec2(0, 2), new Vec2(2, 4), 0.5).equals(new Vec2(1, 3))].join(',');
}
"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "0,true");
}

#[test]
fn numeric_layer_uses_index() {
    let mut scene = json!({"objects":[
        {"id":10,"name":"first","color":"1 0 0"},
        {"id":20,"name":"second","color":"0 1 0","alpha":{"value":1,"script":"export function init(){ thisScene.getLayer(0).color = new Vec3(0,0,1); }"}}
    ]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["color"], "0 0 1");
    assert_eq!(scene["objects"][1]["color"], "0 1 0");
}

#[test]
fn id_lookup_converts_strings() {
    let cases = [
        ("'10'", "thisScene.getLayerByIndex(1)"),
        ("'100'", "thisScene.getLayerByIndex(2)"),
        ("'163'", "thisLayer"),
        ("'010'", "thisScene.getLayerByIndex(1)"),
        ("'+00010tail'", "thisScene.getLayerByIndex(1)"),
        ("' 10.0'", "thisScene.getLayerByIndex(1)"),
        ("'10\\0junk'", "thisScene.getLayerByIndex(1)"),
        ("'\\t\\n10junk'", "thisScene.getLayerByIndex(1)"),
        ("'-18446744073709551606'", "thisScene.getLayerByIndex(1)"),
        ("'4294967306'", "null"),
        ("'18446744073709551626'", "null"),
        ("'-18446744073709551626'", "null"),
        ("'0'", "null"),
        ("''", "null"),
        ("'garbage'", "null"),
        ("'0xa'", "null"),
        ("'++10'", "null"),
        ("'1e1'", "null"),
        ("'\\u00a010'", "null"),
        ("'victim'", "null"),
        ("'0'.repeat(100000)+'10'", "thisScene.getLayerByIndex(1)"),
        ("'9'.repeat(100000)", "null"),
        ("10", "null"),
        ("undefined", "null"),
        ("null", "null"),
        ("true", "null"),
        ("new String('10')", "null"),
        ("thisLayer", "null"),
    ];
    for (expr, expected) in cases {
        let script = format!(
            "export function init(){{if(thisScene.getLayerByID({expr}) !== {expected}) throw Error('lookup');if(thisScene.getLayerByID() !== undefined) throw Error('missing');}}"
        );
        let mut scene = json!({"objects":[
            {"id":0,"name":"zero"},
            {"id":10,"name":"victim"},
            {"id":100,"name":"other"},
            {"id":163,"name":"observer","text":{"value":"","script":script}}
        ]});
        let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        assert!(host.diagnostics.is_empty(), "{expr}: {:?}", host.diagnostics);
    }
}

#[test]
fn id_lookup_keeps_origin() {
    let mut scene = json!({"objects":[
        {"id":10,"name":"victim"},
        {"id":20,"name":"observer","text":{"value":"","script":r"
export function init(){
    const victim=thisScene.getLayerByID('10');
    victim.id=99;
    return JSON.stringify([thisScene.getLayerByID('10')===victim,thisScene.getLayerByID('99')===null,thisScene.getLayer(0)===victim]);
}"}}
    ]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][1]["text"], "[true,true,true]");
}

#[test]
fn id_lookup_tracks_lifecycle() {
    let mut scene = json!({"objects":[
        {"id":10,"name":"victim","visible":{"value":true,"script":r"
export function destroy(){
    thisScene.getLayer('observer').text='hook:'+JSON.stringify([shared.pending,thisScene.getLayerByID('10')===null,thisScene.getLayerByID('20')===thisScene.getLayer('observer')]);
}"}},
        {"id":20,"name":"observer","text":{"value":"","script":r"
let ticks=0;
export function init(){
    if(thisScene.getLayerByID('10')!==thisScene.getLayer('victim')) throw Error('alive');
}
export function update(){
    if(engine.runtime===0)return 'alive';
    if(ticks++===0){
        thisScene.destroyLayer(thisScene.getLayerByID('10'));
        shared.pending=thisScene.getLayerByID('10')===null;
        return 'pending';
    }
    return 'retired:'+JSON.stringify([thisScene.getLayerByID('10')===null,thisScene.getLayerByID('20')===thisLayer]);
}"}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert_eq!(scene["objects"][1]["text"], "");
    assert!(host.tick(1.0, 1.0, [0.5; 2]).unwrap());
    assert_eq!(host.scene["objects"][1]["text"], "hook:[true,true,true]");
    assert!(host.tick(2.0, 1.0, [0.5; 2]).unwrap());
    assert_eq!(host.scene["objects"][1]["text"], "retired:[true,true]");
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn observed_layer_references() {
    let cases = [
        ("0", Some(0)),
        ("1", Some(1)),
        ("0.9", Some(0)),
        ("1.9", Some(1)),
        ("-0.5", Some(0)),
        ("-1", None),
        ("999", None),
        ("NaN", Some(0)),
        ("Infinity", Some(0)),
        ("4294967296", Some(0)),
        ("4294967297", Some(1)),
        ("'0'", None),
        ("'10'", Some(0)),
        ("'20'", Some(1)),
        ("'probe'", Some(0)),
        ("'index-target'", Some(1)),
        ("null", None),
        ("undefined", None),
        ("thisLayer", Some(1)),
        ("{}", None),
        ("{id:10}", None),
        ("{name:'probe'}", None),
        ("[]", None),
        ("[0]", None),
        ("true", None),
        ("false", None),
        ("new Number(1)", None),
        ("new String('probe')", None),
        ("thisScene.getLayer('probe')", Some(0)),
    ];
    for (expr, index) in cases {
        let expected = if expr == "'0'" {
            "null".to_owned()
        } else {
            index.map_or_else(
                || "undefined".to_owned(),
                |index| format!("thisScene.getLayerByIndex({index})"),
            )
        };
        let script = format!(
            "export function init(){{if(thisScene.getLayer({expr}) !== {expected}) throw Error('lookup');}}"
        );
        let mut scene = json!({"objects":[
            {"id":10,"name":"probe"},
            {"id":20,"name":"index-target","alpha":{"value":1,"script":script}}
        ]});
        let host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
            .unwrap()
            .unwrap();
        assert!(host.diagnostics.is_empty(), "{expr}: {:?}", host.diagnostics);
    }
}

#[test]
fn named_board_finishes_init() {
    let mut objects: Vec<_> = (0..600)
        .map(|index| json!({"id":index + 1,"name":format!("byte-{index}"),"color":"1 1 1"}))
        .collect();
    objects[0]["alpha"] = json!({"value":1,"script":r"
export function init() {
    const original=globalThis.Map;
    globalThis.Map=function(){throw Error('replaced Map');};
    try {
        for (let i=0;i<600;i++) thisScene.getLayer('byte-'+i).color=new Vec3(0,0,0);
    } finally {globalThis.Map=original;}
}
"});
    let mut scene = json!({"objects":objects});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    for layer in host.scene["objects"].as_array().unwrap() {
        assert_eq!(layer["color"], "0 0 0");
    }
}

#[test]
fn named_layers_follow_mutations() {
    let script = r"
function check(key, expected) {
    if (thisScene.getLayer(key)!==expected) throw Error('lookup '+key);
}
export function init() {
    const first=thisScene.getLayer(0), second=thisScene.getLayer(1);
    check('20',first);
    check('second',second);
    first.name='renamed';
    check('20',second);
    check('renamed',first);
    Object.defineProperty(first,'name',{value:'second'});
    check('second',first);
    thisScene.sortLayer(second,0);
    check('second',second);
    let reads=0;
    Object.defineProperty(second,'name',{get(){reads++;return 'getter-'+reads;},configurable:true});
    check('getter-1',second);
    check('getter-2',second);
    if(reads!==2) throw Error('getter count');
    delete second.name;
    const proto={name:'inherited'};
    Object.setPrototypeOf(second,proto);
    check('inherited',second);
    proto.name='changed';
    check('changed',second);
    Object.defineProperty(second,'name',{value:'second',writable:true,configurable:true});
    check('second',second);
    thisScene.destroyLayer(second);
    check('second',null);
}
export function update() {
    check('second',thisScene.getLayer(0));
    return 'retired';
}
";
    let mut scene = json!({"objects":[
        {"id":10,"name":"20","text":{"value":"","script":script}},
        {"id":20,"name":"second"}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "retired");
}

#[test]
fn named_layers_create_images() {
    let script = r"
export function init() {
    if(thisScene.getLayer('created')!==null) throw Error('before create');
    const layer=thisScene.createLayer({name:'created',image:'models/white.json'});
    if(thisScene.getLayer('created')!==layer) throw Error('created');
    layer.name='renamed';
    if(thisScene.getLayer('created')!==null || thisScene.getLayer('renamed')!==layer) throw Error('renamed');
    thisScene.destroyLayer(layer);
    if(thisScene.getLayer('renamed')!==null) throw Error('pending');
}
export function update() {
    if(thisScene.getLayer('renamed')!==null) throw Error('retired');
}
";
    let mut scene =
        json!({"objects":[{"id":1,"name":"observer","text":{"value":"","script":script}}]});
    let mut host = image_host(&mut scene);
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn finite_updates_get_enough_time_and_reach_later_bindings() {
    let mut scene = json!({"objects":[
        {"id":1,"alpha":{"value":0,"script":"export function update(value){ const start=Date.now(); while(Date.now()-start<8){} return value+1; }"}},
        {"id":2,"alpha":{"value":0,"script":"export function update(value){ return value+1; }"}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    for frame in 1..=12 {
        host.tick(frame as f32 / 60.0, 1.0 / 60.0, [0.5; 2]).unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(host.scene["objects"][0]["alpha"], frame);
        assert_eq!(host.scene["objects"][1]["alpha"], frame);
    }
}

#[test]
fn infinite_loop_is_stopped_by_frame_budget() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":1.0,"script":"export function update(){while(true){}}"}}]});
    let start = std::time::Instant::now();
    let mut host = stalling_host(&mut scene);
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert!(!host.animated());
    assert!(!host.diagnostics.is_empty());
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn unchanged_text_does_not_request_a_render_update() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"hi","script":"export function update(value){return value;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(!host.tick(1.0, 0.016, [0.5; 2]).unwrap());
}

#[test]
fn saved_script_properties_override_declared_defaults() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"", "scriptproperties":{"label":"saved","choice":"b"},"script":"export const scriptProperties=createScriptProperties().addText({name:'label',value:'default'}).addCombo({name:'choice',options:[{value:'a'},{value:'b'}]}).finish(); export function update(){return scriptProperties.label+scriptProperties.choice;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "savedb");
}

#[test]
fn bound_script_properties_preserve_boolean_values_and_nested_defaults() {
    let project = json!({"general":{"properties":{"format":{"type":"bool","value":true},"date":{"type":"bool","value":true}}}});
    let raw = json!({"objects":[{"id":1,"text":{"value":"", "scriptproperties":{"clock":{"user":"format","value":{"user":"missing","value":true}},"date":{"user":"date","value":false}},"script":"export const scriptProperties=createScriptProperties().addCheckbox({name:'clock',value:true}).addCheckbox({name:'date',value:true}).finish(); export function update(){return (scriptProperties.clock?'12':'24')+(scriptProperties.date?' DD/MM':' MM/DD');}"}}]});
    for (format, date, expected) in [(0.0, 0.0, "24 MM/DD"), (1.0, 1.0, "12 DD/MM")] {
        let mut scene = raw.clone();
        let props =
            Properties::from([("format".into(), vec![format]), ("date".into(), vec![date])]);
        let mut host = SceneScripts::load(&mut scene, &props, &project).unwrap().unwrap();
        host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(host.scene["objects"][0]["text"], expected);
    }
}

#[test]
fn failed_update_does_not_stop_other_modules() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":1,"script":"export function update(){throw Error('bad script');}"}},{"id":2,"alpha":{"value":0,"script":"export function update(v){return v+1;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert!(host.animated());
    assert_eq!(host.diagnostics.len(), 1);
    assert_eq!(host.scene["objects"][1]["alpha"], 1);
}

#[test]
fn timer_only_script_remains_scheduled_until_deadline() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":0,"script":"export function init(){engine.setTimeout(()=>{thisLayer.alpha=1;},500);}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.animated());
    host.tick(0.2, 0.2, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["alpha"], 0);
    host.tick(0.6, 0.4, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["alpha"], 1);
    assert!(!host.animated());
}

#[test]
fn click_requires_press_and_release_over_the_same_layer() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":0,"script":"export function cursorClick(){thisLayer.alpha+=1;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.pointer([0.2; 2], [true, false, false], vec!["1".into()]).unwrap();
    host.pointer([0.2; 2], [false; 3], vec!["1".into()]).unwrap();
    host.tick(1.0, 0.016, [0.2; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["alpha"], 1);
    host.pointer([0.2; 2], [true, false, false], vec!["1".into()]).unwrap();
    host.pointer([0.8; 2], [false; 3], vec![]).unwrap();
    host.tick(2.0, 0.016, [0.8; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["alpha"], 1);
}

#[test]
fn solid_transparency_callbacks() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"default","alpha":{"value":0,"script":"export function cursorClick(){thisLayer.alpha=1;}"}},
        {"id":2,"name":"off","solid":false,"alpha":{"value":0,"script":"export function cursorClick(){thisLayer.alpha=1;}"}},
        {"id":3,"name":"alpha-zero","solid":true,"alpha":{"value":0,"script":"export function cursorClick(){thisLayer.alpha=1;}"}},
        {"id":4,"name":"hidden","solid":true,"visible":false,"alpha":{"value":0,"script":"export function cursorClick(){thisLayer.alpha=1;thisLayer.visible=true;}"}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    let hits = vec!["1".into(), "2".into(), "3".into(), "4".into()];
    host.pointer([0.2; 2], [true, false, false], hits.clone()).unwrap();
    host.pointer([0.2; 2], [false; 3], hits).unwrap();
    host.tick(1.0, 0.016, [0.2; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["alpha"], 1);
    assert_eq!(host.scene["objects"][1]["alpha"], 0);
    assert_eq!(host.scene["objects"][2]["alpha"], 1);
    assert_eq!(host.scene["objects"][3]["alpha"], 1);
    assert_eq!(host.scene["objects"][3]["visible"], true);
}

#[test]
fn cursor_current_order() {
    let script = "export function cursorEnter(){thisScene.getLayer('trace').text+='E';} export function cursorLeave(){thisScene.getLayer('trace').text+='L';}";
    let mut scene = json!({"objects":[
        {"id":1,"alpha":{"value":1,"script":script}},
        {"id":2,"alpha":{"value":1,"script":script}},
        {"id":3,"name":"trace","text":""}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.pointer([0.2; 2], [false; 3], vec!["1".into()]).unwrap();
    host.pointer([0.8; 2], [false; 3], vec!["2".into()]).unwrap();
    host.pointer([0.2; 2], [false; 3], vec!["1".into()]).unwrap();
    host.tick(1.0, 0.016, [0.2; 2]).unwrap();
    assert_eq!(host.scene["objects"][2]["text"], "EELLE");
}

#[test]
fn cursor_sort_revisits_layer() {
    let script = "function emit(label){thisScene.getLayer('trace').text+=thisLayer.name+label+';';} export function cursorEnter(){emit('E');} export function cursorLeave(){emit('L');} export function cursorUp(){emit('U');} export function cursorClick(){emit('C');thisScene.sortLayer('on',thisScene.getLayerCount()-1);}";
    let mut scene = json!({"objects":[
        {"id":1,"name":"on","alpha":{"value":1,"script":script}},
        {"id":2,"name":"alpha","alpha":{"value":1,"script":script}},
        {"id":3,"name":"hidden","visible":false,"alpha":{"value":1,"script":script}},
        {"id":4,"name":"trace","text":""}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    for (position, pressed, id) in [
        (0.25, false, "1"),
        (0.5, false, "2"),
        (0.75, false, "3"),
        (0.75, true, "3"),
        (0.75, false, "3"),
        (0.25, false, "1"),
        (0.5, false, "2"),
        (0.25, false, "1"),
    ] {
        host.pointer([position; 2], [pressed, false, false], vec![id.into()]).unwrap();
    }
    host.tick(1.0, 0.016, [0.25; 2]).unwrap();
    assert_eq!(
        host.scene["objects"][3]["text"],
        "onE;alphaE;onL;hiddenE;alphaL;hiddenU;hiddenC;hiddenU;hiddenC;onE;hiddenL;onL;alphaE;onE;alphaL;"
    );
    assert!(host.diagnostics.is_empty());
}

#[test]
fn cursor_release_outside() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":"export function cursorUp(){thisLayer.text+='U';} export function cursorClick(){thisLayer.text+='C';}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.pointer([0.25; 2], [true, false, false], vec!["1".into()]).unwrap();
    host.pointer([0.75; 2], [false; 3], vec![]).unwrap();
    host.pointer([0.25; 2], [false; 3], vec!["1".into()]).unwrap();
    host.tick(1.0, 0.016, [0.25; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "U");
    assert!(host.diagnostics.is_empty());
}

#[test]
fn cursor_local_y() {
    for (position, expected) in [([0.5, 0.25], "20 -22 0"), ([0.5, 0.75], "20 42 0")] {
        let mut scene = json!({"general":{"orthogonalprojection":{"width":128,"height":128}},"objects":[{"id":1,"origin":"64 64 0","size":"40 20","text":{"value":"","script":"export function cursorDown(event){const p=event.localPosition;thisLayer.text=[p.x,p.y,p.z].join(' ');}"}}]});
        let mut host =
            SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        host.pointer(position, [true, false, false], vec!["1".into()]).unwrap();
        host.tick(1.0, 0.016, position).unwrap();
        assert_eq!(host.scene["objects"][0]["text"], expected);
        assert!(host.diagnostics.is_empty());
    }
}

#[test]
fn cursor_parent_orientation() {
    let script = "export function cursorDown(event){const p=event.localPosition;thisLayer.text=[p.x>40,p.y>0,p.y<30].join(' ');}";
    for angles in ["0 0 0", "0 0 1.5707963267948966"] {
        let rotated = angles != "0 0 0";
        let position = if rotated {
            [990.0 / 1280.0, 516.0 / 720.0]
        } else {
            [1060.0 / 1280.0, 566.0 / 720.0]
        };
        let mut scene = json!({"general":{"orthogonalprojection":{"width":1280,"height":720}},"objects":[
            {"id":1,"origin":"1000 144 0","scale":"2 1 1","angles":angles},
            {"id":2,"parent":1,"origin":"20 0 0","size":"80 60","text":{"value":"","script":script}}
        ]});
        let mut host =
            SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        host.pointer(position, [true, false, false], vec!["2".into()]).unwrap();
        host.tick(1.0, 0.016, position).unwrap();
        assert_eq!(host.scene["objects"][1]["text"], "true true true");
        assert!(host.diagnostics.is_empty());
    }
}

#[test]
fn heap_exhaustion_stops_script_without_crashing_host() {
    let script = "export function update(){const a=new ArrayBuffer(SIZE);return a.byteLength;}"
        .replace("SIZE", &(2 * HEAP_LIMIT).to_string());
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":1,"script":script}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert!(!host.animated());
    assert_eq!(host.diagnostics.len(), 1);
    assert!(host.heap_bytes() < HEAP_LIMIT);
}

#[test]
fn runaway_pointer_is_disabled_after_one_failure() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":1,"script":"export function cursorEnter(){while(true){}}"}}]});
    let mut host = stalling_host(&mut scene);
    host.pointer([0.2; 2], [false; 3], vec!["1".into()]).unwrap();
    let count = host.diagnostics.len();
    assert_eq!(count, 1);
    host.pointer([0.3; 2], [false; 3], vec!["1".into()]).unwrap();
    assert_eq!(host.diagnostics.len(), count);
    assert!(!host.animated());
}

#[test]
fn oversized_frame_output_stops_scripts_and_preserves_scene() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"safe","script":"export function update(v){return engine.runtime > 0 ? 'x'.repeat(1100000) : v;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert!(!host.animated());
    assert_eq!(host.scene["objects"][0]["text"], "safe");
    assert_eq!(host.diagnostics.len(), 1);
}

#[test]
fn failed_compile_cursor() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":0,"script":"throw Error('failed'); export function cursorEnter(){thisLayer.alpha=1;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.pointer([0.2; 2], [false; 3], vec!["1".into()]).unwrap();
    host.tick(1.0, 0.016, [0.2; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["alpha"], 0);
    assert_eq!(host.diagnostics.len(), 1);
}

#[test]
fn scripts_receive_resolved_user_property_values() {
    let mut scene = json!({"objects":[{"id":1,"visible":{"value":true,"user":"enabled"},"alpha":{"value":0.2,"user":"opacity","script":"export function init(v){return thisLayer.visible ? 1 : v;}"}}]});
    let props = Properties::from([("enabled".into(), vec![0.0]), ("opacity".into(), vec![0.75])]);
    let host = SceneScripts::load(&mut scene, &props, &serde_json::Value::Null).unwrap().unwrap();
    assert!(host.diagnostics.is_empty());
    assert_eq!(scene["objects"][0]["alpha"], 0.75);
}

#[test]
fn project_properties_preserve_text_booleans_and_colors() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":"export function update(){return engine.userProperties.name + ':' + (engine.userProperties.Enabled === false) + ':' + engine.userProperties.tint.x;}"}}]});
    let project = json!({"general":{"properties":{"name":{"type":"textinput","value":"YOURNAME"},"Enabled":{"type":"bool","value":true},"tint":{"type":"color","value":"0.5 0.25 1"}}}});
    let mut props = crate::effects::parse_properties(&project);
    props.insert("enabled".into(), vec![0.0]);
    let mut host = SceneScripts::load(&mut scene, &props, &project).unwrap().unwrap();
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "YOURNAME:true:0.5");
}

#[test]
fn storage_preserves_vectors_across_reloads_and_isolates_screens() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let make = || json!({"objects":[{"id":1,"origin":{"value":"0 0 0","script":"export function init(v){const old=localStorage.get('position');localStorage.set('position',new Vec3(12,34,0));localStorage.set('global',7,localStorage.LOCATION_GLOBAL);return old ? old.add(new Vec3(1,0,0)) : v;}"}}]});
    let mut scene = make();
    let storage = super::Storage::at(root.path(), project.path(), "DP-1");
    SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        super::Load { storage, ..super::Load::default() },
    )
    .unwrap();
    let mut scene = make();
    let storage = super::Storage::at(root.path(), project.path(), "DP-1");
    let host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        super::Load { storage, ..super::Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["origin"], "13 34 0");
    let mut scene = make();
    let storage = super::Storage::at(root.path(), project.path(), "DP-2");
    let host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        super::Load { storage, ..super::Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty());
    assert_eq!(scene["objects"][0]["origin"], "0 0 0");
    assert_eq!(
        super::Storage::at(root.path(), project.path(), "DP-3").unwrap().data["global"]["global"],
        7
    );
}

#[test]
fn storage_limit_stops_only_the_offending_script() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":1,"script":"export function init(){localStorage.set('oversized','x'.repeat(102401));}"}},{"id":2,"alpha":{"value":0,"script":"export function update(){return 0.75;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert_eq!(host.diagnostics.len(), 1);
    assert_eq!(host.scene["objects"][1]["alpha"], 0.75);
}

#[test]
fn parent_access_and_documented_cursor_world_position_allow_dragging() {
    let mut scene = json!({"general":{"orthogonalprojection":{"width":100,"height":100}},"objects":[{"id":1,"name":"parent","origin":"10 20 0"},{"id":2,"parent":1,"origin":{"value":"0 0 0","script":"let down=false;export function init(v){return thisLayer.getParent().origin.copy();} export function cursorDown(e){down=true;} export function cursorMove(e){if(down)thisLayer.origin=e.worldPosition.copy();} export function cursorUp(e){down=false;localStorage.set('position',thisLayer.origin);}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert_eq!(scene["objects"][1]["origin"], "10 20 0");
    host.pointer([0.1, 0.2], [true, false, false], vec!["2".into()]).unwrap();
    host.pointer([0.7, 0.6], [true, false, false], vec![]).unwrap();
    host.pointer([0.7, 0.6], [false; 3], vec![]).unwrap();
    host.tick(1.0, 0.1, [0.7, 0.6]).unwrap();
    let origin = host.scene["objects"][1]["origin"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .map(|n| n.parse::<f32>().unwrap())
        .collect::<Vec<_>>();
    assert!((origin[0] - 70.0).abs() < 0.001 && (origin[1] - 40.0).abs() < 0.001);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn media_callbacks_keep_state_and_do_not_repeat_unchanged_events() {
    let mut scene = json!({"objects":[{"id":1,"visible":false,"text":{"value":"","script":"let n=0;let title='';export function mediaPropertiesChanged(e){n++;title=e.title;}export function mediaPlaybackChanged(e){thisLayer.visible=e.state===MediaPlaybackEvent.PLAYBACK_PLAYING;}export function update(){return title+':'+n;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.needs_media());
    let events =
        json!({"mediaPropertiesChanged":{"title":"Track"},"mediaPlaybackChanged":{"state":1}});
    host.media(&events).unwrap();
    host.tick(1.0, 0.1, [0.5; 2]).unwrap();
    host.media(&events).unwrap();
    host.tick(2.0, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "Track:1");
    assert_eq!(host.scene["objects"][0]["visible"], true);
    host.media(&json!({"mediaPlaybackChanged":{"state":0}})).unwrap();
    host.tick(3.0, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["visible"], false);
}

#[test]
fn color_module_converts_hsv_and_preserves_hdr_values() {
    let mut scene = serde_json::json!({"objects":[{"id":1,"color":{"value":"0 0 0","script":"import * as C from 'WEColor'; export function update() { const rgb=C.hsv2rgb(new Vec3(0.5,1,1.6)); const hsv=C.rgb2hsv(rgb); if(Math.abs(hsv.x-0.5)>0.0001 || Math.abs(hsv.z-1.6)>0.0001) throw Error('roundtrip'); if(!C.normalizeColor(C.expandColor(new Vec3(1,0,0))).equals(new Vec3(1,0,0))) throw Error('range'); return rgb; }"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::default(), &serde_json::json!({}))
        .unwrap()
        .unwrap();
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["color"], "0 1.600000023841858 1.600000023841858");
}

#[test]
fn cursor_local_position_accounts_for_parent_rotation_and_scale() {
    let mut scene = json!({"general":{"orthogonalprojection":{"width":100,"height":100}},"objects":[{"id":1,"origin":"10 20 0","scale":"2 2 1","angles":"0 0 1.5707963267948966"},{"id":2,"parent":1,"size":"20 10","origin":{"value":"5 0 0","script":"export function cursorDown(e){shared.local=e.localPosition;} export function update(v){return shared.local || v;}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.pointer([0.1, 0.7], [true, false, false], vec!["2".into()]).unwrap();
    host.tick(1.0, 0.1, [0.1, 0.7]).unwrap();
    let p = host.scene["objects"][1]["origin"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .map(|v| v.parse::<f32>().unwrap())
        .collect::<Vec<_>>();
    assert!((p[0] - 10.0).abs() < 0.001 && (p[1] - 5.0).abs() < 0.001, "{p:?}");
}

#[test]
fn user_properties_reach_scripts_after_init() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":0.1,"script":"export function init(v){return v;} export function applyUserProperties(p){ if (p.opacity !== undefined) thisLayer.alpha = p.opacity; }"}}]});
    let project = json!({"general":{"properties":{"opacity":{"type":"slider","value":0.7}}}});
    let props = crate::effects::parse_properties(&project);
    let host = SceneScripts::load(&mut scene, &props, &project).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert!((scene["objects"][0]["alpha"].as_f64().unwrap() - 0.7).abs() < 1e-6);
}

#[test]
fn sound_controls_keep_identity() {
    let mut scene = json!({"objects":[
        {"id":7,"name":"BGM","sound":["sounds/a.ogg"],"startsilent":true},
        {"id":2,"alpha":{"value":1.0,"script":"export function init(){ const s = thisScene.getLayer('BGM'); s.id = {custom:true}; s.play(); if (!s.isPlaying()) throw Error('not playing'); s.pause(); s.stop(); s.volume = 0.5; }"}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let commands = host.take_commands();
    assert_eq!(
        commands,
        vec![
            ScriptCommand::Sound { id: "7".into(), op: SoundOp::Gain(0.5) },
            ScriptCommand::Sound { id: "7".into(), op: SoundOp::Play },
            ScriptCommand::Sound { id: "7".into(), op: SoundOp::Pause },
            ScriptCommand::Sound { id: "7".into(), op: SoundOp::Stop },
        ]
    );
    assert!(host.take_commands().is_empty());
}

#[test]
fn texture_animation_controls_queue_sprite_commands() {
    let mut scene = json!({"objects":[{"id":1,"image":"models/a.json","alpha":{"value":1.0,"script":"export function init(){ const a = thisLayer.getTextureAnimation(); a.setFrame(1); a.pause(); a.rate = 2; a.play(); a.join(); }"}}]});
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &serde_json::Value::Null,
        super::Load { sprites: Some(&[(0, vec![0.5; 4])]), ..super::Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let ops: Vec<SpriteOp> = host
        .take_commands()
        .into_iter()
        .map(|command| match command {
            ScriptCommand::Sprite { object: 0, op } => op,
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(
        ops,
        vec![
            SpriteOp::Frame(1),
            SpriteOp::Pause,
            SpriteOp::Rate(2.0),
            SpriteOp::Play,
            SpriteOp::Join
        ]
    );
    assert_eq!(
        scene["objects"][0]["alpha"], 1.0,
        "sprite control must not touch the scene document"
    );
}

#[test]
fn sprite_frame_durations() {
    let mut scene = json!({"objects":[{"id":1,"image":"models/a.json","alpha":{"value":0,
        "script":"export function update(){return thisLayer.getTextureAnimation().getFrame();}"}}]});
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &serde_json::Value::Null,
        super::Load { sprites: Some(&[(0, vec![0.1, 0.7, 0.1, 0.1])]), ..super::Load::default() },
    )
    .unwrap()
    .unwrap();
    for (time, frame) in [(0.2, 1), (0.6, 1), (0.85, 2)] {
        host.tick(time, 0.01, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["alpha"], frame);
    }
}

#[test]
fn destroyed_layers_hide() {
    let mut scene = json!({"objects":[{"id":1,"name":"a","visible":true,"origin":"5 5 0"},{"id":2,"alpha":{"value":1.0,"script":"export function init(){ thisScene.getLayer('a').origin = new Vec3(9,9,0); const initial = thisScene.getInitialLayerConfig('a'); if (initial.origin !== '5 5 0') throw Error('initial ' + initial.origin); if (!thisScene.destroyLayer('a')) throw Error('destroy'); if (thisScene.getLayerCount() !== 1) throw Error('count'); }"}}]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["visible"], false);
    assert_eq!(scene["objects"][0]["origin"], "9 9 0");
}

#[test]
fn screen_resolution_reaches_init() {
    let mut scene = json!({"general":{"orthogonalprojection":{"width":1920,"height":1080}},
        "objects":[{"id":1,"name":"probe","alpha":{"value":1,"script":r"
export function init() {
    thisLayer.name = [engine.canvasSize.x, engine.canvasSize.y,
        engine.screenResolution.x, engine.screenResolution.y].join('x');
}
"}}]});
    let host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &serde_json::Value::Null,
        super::Load { screen_resolution: Some([1280, 720]), ..super::Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["name"], "1920x1080x1280x720");
}

#[test]
fn resize_callback_payload() {
    let mut scene = json!({"objects":[{"id":1,"name":"init","alpha":{"value":1,"script":r"
export function resizeScreen(size) {
    thisLayer.name = [arguments.length, size instanceof Vec2,
        size.x, size.y, engine.screenResolution.x, engine.screenResolution.y].join(',');
}
"}}]});
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &serde_json::Value::Null,
        super::Load { screen_resolution: Some([1280, 720]), ..super::Load::default() },
    )
    .unwrap()
    .unwrap();
    assert_eq!(host.scene["objects"][0]["name"], "init");
    assert!(host.resize_screen([1024, 640]));
    assert_eq!(host.scene["objects"][0]["name"], "1,true,1024,640,1024,640");
}

#[test]
fn text_background_vector() {
    let mut scene = json!({"objects":[{"id":1,"name":"probe","text":"Aa",
        "backgroundcolor":"0 0 0","alpha":{"value":1,"script":r"
export function init() {
    if (!(thisLayer.backgroundcolor instanceof Vec3)) throw Error('initial background color');
    thisLayer.backgroundcolor = new Vec3(0, 1, 0);
    if (!(thisLayer.backgroundcolor instanceof Vec3) || thisLayer.backgroundcolor.y !== 1)
        throw Error('updated background color');
}
"}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["backgroundcolor"], "0 1 0");
}

#[test]
fn text_background_precision() {
    let script = r"
export function init() {
    const initial = thisScene.getInitialLayerConfig(thisLayer);
    const rounded = Math.fround(0.2);
    if (initial.backgroundcolor !== '0.2 0.2 0.2' || thisLayer.backgroundcolor.x !== rounded) throw Error('authored color');
    const source = new Vec3(0.8, 0.2, 0.1);
    const image = thisScene.getLayer('image');
    image.backgroundcolor = source;
    if (image.backgroundcolor !== source || image.backgroundcolor.x !== 0.8) throw Error('custom image color');
    thisLayer.backgroundcolor = source;
    const color = thisLayer.backgroundcolor;
    if (!(color instanceof Vec3) || color.x !== Math.fround(0.8) || color.y !== rounded || color.z !== Math.fround(0.1)) throw Error('color precision');
    if (image.backgroundcolor !== source || image.backgroundcolor.x !== 0.8) throw Error('custom image identity');
}";
    let mut scene = json!({"objects":[
        {"id":1,"name":"image","image":"models/white.json"},
        {"id":2,"name":"text","text":"Aa","backgroundcolor":"0.2 0.2 0.2",
            "alpha":{"value":1,"script":script}}
    ]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(
        scene["objects"][1]["backgroundcolor"],
        "0.800000011920929 0.20000000298023224 0.10000000149011612"
    );
    assert!(scene["objects"][0].get("backgroundcolor").is_none());
}

#[test]
fn text_padding_is_vector() {
    let mut scene = json!({"objects":[{"id":1,"name":"probe","text":"Aa",
        "padding":0,"alpha":{"value":1,"script":r"
export function init() {
    thisLayer.padding = 40;
    if (!(thisLayer.padding instanceof Vec2)) throw Error('padding vector');
    if (thisLayer.padding.x !== 40 || thisLayer.padding.y !== 40)
        throw Error('padding components');
}
"}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["padding"], "40 40");
}

#[test]
fn general_settings_follow_properties() {
    let mut scene = json!({"objects":[{"id":1,"name":"probe","alpha":{"value":1,"script":r"
export function init() { thisLayer.name = 'init;'; }
export function applyUserProperties() { thisLayer.name += 'user;'; }
export function applyGeneralSettings(settings) {
    thisLayer.name += 'general:' + (typeof settings.language === 'string');
}
"}}]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["name"], "init;user;general:true");
}

#[test]
fn text_style_reaches_scene() {
    let mut scene = json!({"objects":[{"id":1,"text":"Same text","pointsize":30,
        "horizontalalign":"center","verticalalign":"center","limitwidth":false,
        "maxwidth":400,"alpha":{"value":1,"script":r"
export function update() {
    if (engine.runtime > 0) {
        thisLayer.pointsize = 60;
        thisLayer.horizontalalign = 'left';
        thisLayer.verticalalign = 'top';
        thisLayer.limitwidth = true;
        thisLayer.maxwidth = 100;
    }
    return 1;
}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    let object = &host.scene["objects"][0];
    assert_eq!(object["pointsize"], 60);
    assert_eq!(object["horizontalalign"], "left");
    assert_eq!(object["verticalalign"], "top");
    assert_eq!(object["limitwidth"], true);
    assert_eq!(object["maxwidth"], 100);
}

#[test]
fn scene_general_accessors_read_the_scene_and_report_setter_changes() {
    let mut scene = json!({"general":{"clearcolor":"0.3 0 0","fov":50.0,"cameraparallaxamount":{"user":"p","value":0.25},"bloom":false},
        "objects":[{"id":1,"name":"dot","origin":{"value":"0 0 0","script":"export function init(value){ thisScene.clearcolor = new Vec3(1,0,0); thisScene.bloom = true; return new Vec3(thisScene.fov, thisScene.cameraparallaxamount * 1000, thisScene.bloomstrength + thisScene.clearcolor.x); }"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["origin"], "50 250 3");
    assert_eq!(scene["general"]["clearcolor"], "1 0 0");
    assert_eq!(scene["general"]["bloom"], true);
    let changes = host.take_general_changes();
    assert!(changes.contains(&("clearcolor".to_string(), json!("1 0 0"))), "{changes:?}");
    assert!(changes.contains(&("bloom".to_string(), json!(true))), "{changes:?}");
    assert!(host.take_general_changes().is_empty());
}

#[test]
fn bloom_enabled_snapshot() {
    for enabled in [false, true] {
        let script = "export function init(){ const initial=thisScene.bloomEnabled; thisScene.bloomEnabled=!initial; thisScene.bloom=!initial; return JSON.stringify([initial,thisScene.bloomEnabled,thisScene.bloom]); }";
        let mut scene = json!({"general":{"bloom":enabled},"objects":[{"id":1,"text":{"value":"","script":script}}]});
        let mut host =
            SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(scene["objects"][0]["text"], json!([enabled, enabled, !enabled]).to_string());
        assert_eq!(scene["general"]["bloom"], !enabled);
        assert!(host.take_general_changes().contains(&("bloom".into(), json!(!enabled))));
    }
}

#[test]
fn engine_bloom_enabled_toggle() {
    for enabled in [false, true] {
        let script = "export function init(){ const initial=engine.bloomEnabled; engine.bloomEnabled=!initial; return JSON.stringify([initial,engine.bloomEnabled,thisScene.bloomEnabled,thisScene.bloom]); }";
        let mut scene = json!({"general":{"bloom":enabled},"objects":[{"id":1,"text":{"value":"","script":script}}]});
        let mut host =
            SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(
            scene["objects"][0]["text"],
            json!([enabled, !enabled, enabled, !enabled]).to_string()
        );
        assert_eq!(scene["general"]["bloom"], !enabled);
        assert!(host.take_general_changes().contains(&("bloom".into(), json!(!enabled))));
    }
}

#[test]
fn destroy_hook_runs_once_and_its_changes_reach_the_scene() {
    let mut scene = json!({"objects":[{"id":1,"visible":true,"alpha":{"value":1.0,"script":"export function destroy(){ thisLayer.visible = false; }"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert_eq!(host.scene["objects"][0]["visible"], true);
    host.destroy();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["visible"], false);
}

#[test]
fn large_unused_layer_metadata_does_not_exhaust_the_script_heap() {
    let metadata: Vec<_> = (0..18000).map(|i| json!({"frame":i,"value":[i,1,2]})).collect();
    let mut scene = json!({"objects":[{"id":1,"timeline":metadata,"text":{"value":"", "script":"let ticks=0; export function update(){return String(++ticks);}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    for frame in 1..=60 {
        host.tick(frame as f32 / 60.0, 1.0 / 60.0, [0.5; 2]).unwrap();
    }
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert!(host.animated());
    assert_eq!(host.scene["objects"][0]["text"], "60");
}

#[test]
fn legacy_layers_track_replacements() {
    let mut scene = json!({"objects":[{"id":1,"effects":[{"name":"test","amount":1,"a/b~c":{"value":2}},{"name":"second","amount":7,"a/b~c":{"value":2}}],"visible":{"value":true, "script":r"
export function init() {
    const effect = thisLayer.getEffect('test');
    if (effect !== thisLayer.effects[0]) throw Error('unstable effect');
    thisLayer.effects[1]['a/b~c'].value = 3;
    effect.amount = 4;
    thisLayer.effects[0] = {name:'replacement', nested:{amount:5}};
    thisLayer.effects[0].nested.amount = 6;
    thisLayer.effects[1].amount = 8;
    return true;
}
export function update() { return true; }"}}]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(
        scene["objects"][0]["effects"],
        json!([
            {"name":"replacement","nested":{"amount":6}},
            {"name":"second","amount":8,"a/b~c":{"value":3}}
        ])
    );
}

#[test]
fn nested_custom_writes_drain() {
    let mut scene = json!({"objects":[{"id":1,"origin":"1 2 3","effects":[{"name":"test","amount":1,"nested":{"value":2}}],"instance":{"native":true},"text":{"value":"", "script":r"
export function init() {
    const effect = thisLayer.getEffect(0);
    const cycle = {}; cycle.self = cycle;
    cycle.big = 1n;
    cycle.call = () => 7;
    const vector = new Vec3(4,5,6);
    effect.custom = cycle;
    effect.vector = vector;
    thisLayer.instance = {custom:cycle};
    const origin = thisLayer.origin;
    origin.custom = cycle;
    if (effect.custom !== cycle || effect.custom.self !== cycle || effect.vector !== vector || thisLayer.instance.custom !== cycle) throw Error('custom identity');
    if (origin.custom !== cycle || thisLayer.origin.custom !== undefined) throw Error('vector identity');
    if (effect.custom.call() !== 7 || effect.custom.big !== 1n || effect.vector.z !== 6) throw Error('custom values');
    if (!Object.getOwnPropertyDescriptor(effect, 'custom').writable) throw Error('custom descriptor');
    effect.amount = 4;
    effect.nested.value = 3;
    thisLayer.origin.x = 9;
    return 'ready';
}"}}]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "ready");
    assert_eq!(host.scene["objects"][0]["origin"], "1 2 3");
    assert_eq!(host.scene["objects"][0]["effects"][0]["amount"], 4);
    assert_eq!(host.scene["objects"][0]["effects"][0]["nested"]["value"], 3);
    assert!(host.scene["objects"][0]["effects"][0].get("custom").is_none());
    assert!(host.scene["objects"][0]["effects"][0].get("vector").is_none());
    assert!(host.scene["objects"][0]["instance"].get("custom").is_none());
}

#[test]
fn sound_visibility_stays_private() {
    let mut scene = json!({"objects":[{"id":1,"sound":"sound.mp3","visible":{"value":true,"script":r"
export function init() {
    if (thisLayer.visible !== undefined || Object.hasOwn(thisLayer, 'visible')) throw Error('visible exposed');
    return false;
}
export function update() {
    if (thisLayer.visible !== undefined || Object.hasOwn(thisLayer, 'visible')) throw Error('visible exposed');
    return false;
}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["visible"], false);
}

#[test]
fn kind_fields_survive_retirement() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"observer","text":{"value":"", "script":r"
let held;
export function init() {
    held = ['sound', 'particle'].map(name => thisScene.getLayer(name));
    const cycle = {}; cycle.self = cycle;
    const vector = new Vec3(1,2,3);
    for (const layer of held) {
        if (layer.alpha !== undefined || layer.color !== undefined || Object.hasOwn(layer, 'alpha') || Object.hasOwn(layer, 'color')) throw Error('field exposed');
        layer.alpha = cycle;
        layer.color = vector;
        if (layer.alpha !== cycle || layer.color !== vector || !Object.hasOwn(layer, 'alpha') || !Object.hasOwn(layer, 'color')) throw Error('custom value');
        thisScene.destroyLayer(layer);
    }
    return 'ready';
}
export function update() {
    if (engine.runtime === 0) return 'ready';
    return String(held.every(layer => layer.alpha.self === layer.alpha && layer.color.z === 3));
}"}},
        {"id":2,"name":"sound","sound":"sound.mp3","alpha":0.5,"color":"1 0 0"},
        {"id":3,"name":"particle","particle":"particle.json","alpha":0.5,"color":"0 1 0"}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "true");
    for slot in [1, 2] {
        assert_eq!(host.scene["objects"][slot]["alpha"], 0.5);
        assert!(host.scene["objects"][slot]["color"].is_string());
    }
}

#[test]
fn retired_nested_custom_survives() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"observer","text":{"value":"", "script":r"
let held;
export function init() {
    held = thisScene.getLayer('victim').getEffect(0);
    thisScene.destroyLayer('victim');
    return 'ready';
}
export function update() {
    if (engine.runtime === 0) return 'ready';
    const custom = {}; custom.self = custom;
    held.custom = custom;
    return String(held.custom === custom && held.custom.self === custom);
}"}},
        {"id":2,"name":"victim","effects":[{"name":"test","amount":1}]}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "true");
    assert!(host.scene["objects"][1]["effects"][0].get("custom").is_none());
}

#[test]
fn method_slots_keep_identity() {
    for kind in ["image", "text", "sound", "particle"] {
        let mut target = json!({"id":1,"name":"target"});
        target[kind] = json!("fixture");
        let mut scene = json!({"objects":[target,{"id":2,"text":{"value":"","script":r"
let held, object;
export function init() {
    held = thisScene.getLayer('target');
    if (!delete held.getParent || Object.hasOwn(held, 'getParent') || held.getParent !== undefined) throw Error('method deletion');
    const callback = () => 73;
    held.getParent = callback;
    if (held.getParent !== callback || held.getParent() !== 73) throw Error('function replacement');
    object = {nested:{value:7}};
    held.getParent = object;
    held.getParent.nested.value = 9;
    if (held.getParent !== object || object.nested.value !== 9) throw Error('object replacement');
    const vector = new Vec3(0.123456789,2,3);
    Object.defineProperty(held, 'getParent', {value:vector,enumerable:true,configurable:true,writable:true});
    vector.y = 8;
    if (held.getParent !== vector || held.getParent.x !== 0.123456789 || held.getParent.y !== 8) throw Error('vector replacement');
    held.getParent = 31;
    if (held.getParent !== 31) throw Error('number replacement');
    held.getParent = object;
    thisScene.destroyLayer(held);
    return 'ready';
}
export function update() {
    if (engine.runtime === 0) return 'ready';
    held.getParent.nested.value = 11;
    return String(held.getParent === object && object.nested.value === 11);
}"}}]});
        let mut host =
            SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        host.tick(1.0, 1.0, [0.5; 2]).unwrap();
        assert!(host.diagnostics.is_empty(), "{kind}: {:?}", host.diagnostics);
        assert!(host.scene["objects"][0].get("getParent").is_none());
        assert_eq!(host.scene["objects"][1]["text"], "true");
    }
}

#[test]
fn layer_methods_match_descriptors() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"image","image":"fixture"},
        {"id":2,"name":"text","text":"fixture"},
        {"id":3,"name":"sound","sound":"fixture"},
        {"id":4,"name":"particle","particle":"fixture"},
        {"id":5,"text":{"value":"","script":r"
export function init() {
    const common = ['getParent', 'getChildren', 'getAnimation'];
    const visual = ['getEffect', 'getEffectCount'];
    const playback = ['play', 'pause', 'stop', 'isPlaying'];
    for (const [kind, methods] of [
        ['image', [...common, ...visual, 'getTextureAnimation']],
        ['text', [...common, ...visual]],
        ['sound', [...common, ...playback]],
        ['particle', [...common, ...playback, 'emitParticles']]
    ]) {
        const layer = thisScene.getLayer(kind);
        for (const key of methods) {
            const desc = Object.getOwnPropertyDescriptor(layer, key);
            if (typeof desc.value !== 'function' || !desc.enumerable || !desc.configurable || !desc.writable || 'get' in desc || 'set' in desc) throw Error(kind + ':' + key);
            if (!Object.keys(layer).includes(key)) throw Error('missing enumeration');
        }
    }
    const instance = Object.getOwnPropertyDescriptor(thisScene.getLayer('particle'), 'instance');
    if (typeof instance.value !== 'object' || !instance.enumerable || !instance.configurable || instance.writable || 'get' in instance || 'set' in instance) throw Error('instance descriptor');
    return 'ready';
}"}}]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][4]["text"], "ready");
}

#[test]
fn authored_metadata_stays_private() {
    for kind in ["image", "text", "sound", "particle"] {
        let mut target = json!({"id":1,"name":"target","metadata":{"nested":{"value":42}}});
        target[kind] = json!("fixture");
        let mut scene = json!({"objects":[target,{"id":2,"text":{"value":"","script":r"
let held, object, vector;
export function init() {
    held = thisScene.getLayer('target');
    if (held.metadata !== undefined || Object.hasOwn(held, 'metadata') || Object.getOwnPropertyDescriptor(held, 'metadata')) throw Error('authored metadata');
    const initial = thisScene.getInitialLayerConfig(held);
    if (initial.metadata.nested.value !== 42) throw Error('initial metadata');
    object = {nested:{value:7}};
    held.metadata = object;
    held.metadata.nested.value = 9;
    if (held.metadata !== object || object.nested.value !== 9) throw Error('object alias');
    delete held.metadata;
    if (Object.hasOwn(held, 'metadata')) throw Error('custom deletion');
    vector = new Vec3(0.123456789,2,3);
    Object.defineProperty(held, 'metadata', {value:vector,writable:true,configurable:true,enumerable:true});
    vector.y = 8;
    if (held.metadata !== vector || held.metadata.x !== 0.123456789 || held.metadata.y !== 8) throw Error('vector alias');
    thisScene.destroyLayer(held);
    return 'ready';
}
export function update() {
    if (engine.runtime === 0) return 'ready';
    if (held.metadata !== vector || held.metadata.y !== 8) throw Error('retired custom');
    held.metadata = object;
    held.metadata.nested.value = 11;
    return String(held.metadata === object && object.nested.value === 11);
}"}}]});
        let mut host =
            SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        host.tick(1.0, 1.0, [0.5; 2]).unwrap();
        assert!(host.diagnostics.is_empty(), "{kind}: {:?}", host.diagnostics);
        assert_eq!(host.scene["objects"][0]["metadata"], json!({"nested":{"value":42}}));
        assert_eq!(host.scene["objects"][1]["text"], "true");
    }
}

#[test]
fn private_names_keep_hierarchy() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"root","image":"fixture"},
        {"id":2,"name":"child","image":"fixture","parent":1,"effects":[{"name":"probe","visible":true}]},
        {"id":3,"text":{"value":"","script":r"
export function init() {
    const root = thisScene.getLayer('root'), child = thisScene.getLayer('child');
    const custom = {nested:{value:7}};
    for (const layer of [root, child]) {
        for (const key of ['parent', 'attachment', 'image', 'sound', 'particle', 'effects', 'instance']) {
            if (layer[key] !== undefined || Object.hasOwn(layer, key)) throw Error('private exposure');
            layer[key] = custom;
            if (layer[key] !== custom) throw Error('private alias');
        }
    }
    if (root.getParent() !== null || child.getParent() !== root || root.getChildren()[0] !== child) throw Error('private hierarchy');
    child.getEffect(0).visible = false;
    if (child.getEffectCount() !== 1 || child.effects !== custom) throw Error('private effects');
    return 'ready';
}"}}]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][1]["parent"], 1);
    assert_eq!(scene["objects"][1]["effects"][0]["visible"], false);
    assert!(scene["objects"][0].get("parent").is_none());
    assert_eq!(scene["objects"][2]["text"], "ready");
}

#[test]
fn reparents_layer_tree() {
    let mut scene = json!({"objects":[
        {"id":10,"name":"old"},
        {"id":20,"name":"new"},
        {"id":30,"name":"child","parent":10,"text":{"value":"","script":r"
export function init() {
    const old = thisScene.getLayer('old'), next = thisScene.getLayer('new');
    if (thisLayer.getRoot() !== old || thisLayer.getDepth() !== 1 || !old.isAncestorOf(thisLayer)) throw Error('initial tree');
    if (thisLayer.setParent(next, 'missing') || thisLayer.getParent() !== old) throw Error('missing attachment');
    if (!thisLayer.setParent(20) || thisLayer.getParent() !== next || thisLayer.getRoot() !== next) throw Error('new parent');
    if (old.getChildren().length || next.getChildren()[0] !== thisLayer) throw Error('children');
    if (next.setParent(thisLayer) || thisLayer.setParent(thisLayer) || next.getParent()) throw Error('cycle');
    if (!thisLayer.removeFromParent() || thisLayer.getParent() !== null || thisLayer.getDepth() !== 0) throw Error('detach');
    return 'ready';
}"}}
    ]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][2]["parent"], serde_json::Value::Null);
    assert_eq!(scene["objects"][2]["text"], "ready");
}

#[test]
fn preserves_reparent_transform() {
    let mut scene = json!({"objects":[
        {"id":10,"name":"old","parent":40,"origin":"10 20 0","angles":"0 0 1.5707963267948966","scale":"2 2 1"},
        {"id":20,"name":"new","origin":"-20 5 0","angles":"0 0 -1.5707963267948966","scale":"0.5 0.5 1"},
        {"id":30,"name":"child","parent":10,"origin":"4 0 3","text":{"value":"","script":r"
export function init() {
    if (!thisLayer.setParent('new', true)) throw Error('reparent');
    return 'ready';
}"}},
        {"id":40,"name":"grand","origin":"3 7 0","angles":"0 0 0.7853981633974483","scale":"1.5 1.5 1"}
    ]});
    let mut before = scene.clone();
    before["objects"][2]["text"] = json!("");
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let layout = crate::model::script::Layout {
        id: "30".into(),
        size: [10.0, 10.0],
        offset: [0.0, 0.0],
        text: true,
        hidden_without_fx: false,
        puppet: false,
        fullscreen: false,
    };
    let attachments = crate::model::Attachments::new();
    let expected = crate::model::script::frames(
        &before,
        &[layout],
        (100.0, 100.0),
        &Properties::new(),
        &attachments,
    );
    let layout = crate::model::script::Layout {
        id: "30".into(),
        size: [10.0, 10.0],
        offset: [0.0, 0.0],
        text: true,
        hidden_without_fx: false,
        puppet: false,
        fullscreen: false,
    };
    let actual = crate::model::script::frames(
        &scene,
        &[layout],
        (100.0, 100.0),
        &Properties::new(),
        &attachments,
    );
    let expected = expected[0].as_ref().unwrap();
    let actual = actual[0].as_ref().unwrap();
    for (left, right) in expected.rect.into_iter().zip(actual.rect) {
        assert!((left - right).abs() < 0.001);
    }
    assert!((expected.angle.sin() - actual.angle.sin()).abs() < 0.001);
    assert!((expected.angle.cos() - actual.angle.cos()).abs() < 0.001);
    assert!((expected.depth - actual.depth).abs() < 0.001);
}

#[test]
fn preserves_mesh_parent_transform() {
    let mut scene = json!({"objects":[
        {"id":10,"name":"old","parent":40,"origin":"10 20 4","angles":"0.2 -0.4 0.1","scale":"2 2 2"},
        {"id":20,"name":"new","origin":"-20 5 6","angles":"-0.1 0.3 -0.2","scale":"0.5 0.5 0.5"},
        {"id":30,"name":"child","parent":10,"origin":"4 0 3","angles":"0.5 -0.2 0.4","scale":"1 1 1","text":{"value":"","script":r"
export function init() {
    if (!thisLayer.setParent('new', true)) throw Error('reparent');
    return 'ready';
}"}},
        {"id":40,"name":"grand","origin":"3 7 2","angles":"0.1 0.2 -0.3","scale":"1.5 1.5 1.5"}
    ]});
    let mut before = scene.clone();
    before["objects"][2]["text"] = json!("");
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let matrix = |scene: &serde_json::Value| {
        let objects = scene["objects"].as_array().unwrap();
        let by_id: std::collections::HashMap<String, &serde_json::Value> =
            objects.iter().map(|object| (object["id"].to_string(), object)).collect();
        let child = &objects[2];
        let pose = crate::model::mesh_pose(child, &Properties::new(), "test").unwrap();
        let parents =
            crate::model::mesh_parents(child, &by_id, &Properties::new(), "test").unwrap();
        parents.poses.iter().fold(
            crate::matrix::model_matrix(pose.origin, pose.scale, pose.angles),
            |matrix, parent| {
                crate::matrix::mat4_mul(
                    &crate::matrix::model_matrix(parent.origin, parent.scale, parent.angles),
                    &matrix,
                )
            },
        )
    };
    let expected = matrix(&before);
    let actual = matrix(&scene);
    for (left, right) in expected.into_iter().zip(actual) {
        assert!((left - right).abs() < 0.001, "expected {expected:?}, actual {actual:?}");
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Attachment reparenting preserves world pose across a live parent update"
)]
fn attached_parent_transform() {
    let mut mesh = include_bytes!("../fixtures/wires.mdl").to_vec();
    let skeleton = mesh.windows(9).position(|part| part == b"MDLS0001\0").unwrap();
    let at = u32::from_le_bytes(mesh[skeleton + 9..skeleton + 13].try_into().unwrap()) as usize;
    let mut section = b"MDAT0001\0".to_vec();
    let section_size = 9 + 4 + 2 + 2 + 5 + 16 * 4;
    section.extend_from_slice(&u32::try_from(at + section_size).unwrap().to_le_bytes());
    section.extend_from_slice(&1_u16.to_le_bytes());
    section.extend_from_slice(&0_u16.to_le_bytes());
    section.extend_from_slice(b"Grip\0");
    let mut matrix = [0.0_f32; 16];
    for axis in [0, 5, 10, 15] {
        matrix[axis] = 1.0;
    }
    matrix[12] = 7.0;
    for value in matrix {
        section.extend_from_slice(&value.to_le_bytes());
    }
    mesh.splice(at..at, section);
    let end = at + section_size + 9;
    let old = u32::from_le_bytes(mesh[end..end + 4].try_into().unwrap());
    mesh[end..end + 4].copy_from_slice(&(old + u32::try_from(section_size).unwrap()).to_le_bytes());
    let script = r"
export function init() {
    const parent = thisScene.getLayer('root');
    const origin = parent.getAttachmentOrigin('Grip');
    if (parent.getAttachmentIndex('Grip') !== 0 || parent.getAttachmentIndex('missing') !== -1) throw Error('index');
    if (!Number.isFinite(origin.x) || parent.getAttachmentMatrix('Grip').translation().x !== origin.x) throw Error('matrix');
    if (!Number.isFinite(parent.getAttachmentAngles('Grip').z)) throw Error('angle');
    if (!thisLayer.setParent(parent, 0, true)) throw Error('attachment');
    if (thisLayer.getParent() !== parent || thisLayer.attachment !== undefined) throw Error('parent');
    return thisLayer.origin.toString();
}
export function update() {
    return String(thisScene.getLayer('root').getAttachmentOrigin('Grip').x);
}";
    let mut scene = json!({"objects":[
        {"id":1,"name":"root","image":"models/attached.json","origin":"10 20 0"},
        {"id":2,"name":"child","origin":"17 20 0","text":{"value":"","script":script}}
    ]});
    let before = scene.clone();
    let source = scene.to_string();
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", source.as_bytes()),
        ("models/attached.json", br#"{"puppet":"models/attached.mdl"}"#),
        ("models/attached.mdl", &mesh),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some(""));
    let images = crate::model::ImageResources::new(&package, &assets);
    let rest = images.puppet_attachment_poses("models/attached.json").unwrap();
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        super::Load { images: Some(images), ..super::Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][1]["parent"], 1);
    assert_eq!(scene["objects"][1]["attachment"], "Grip");
    let layout = || crate::model::script::Layout {
        id: "2".into(),
        size: [10.0, 10.0],
        offset: [0.0, 0.0],
        text: true,
        hidden_without_fx: false,
        puppet: false,
        fullscreen: false,
    };
    let mut poses = crate::model::Attachments::new();
    poses.insert(("1", "Grip"), rest[0].1);
    let expected = crate::model::script::frames(
        &before,
        &[layout()],
        (100.0, 100.0),
        &Properties::new(),
        &poses,
    );
    let actual = crate::model::script::frames(
        &scene,
        &[layout()],
        (100.0, 100.0),
        &Properties::new(),
        &poses,
    );
    let expected = expected[0].as_ref().unwrap();
    let actual = actual[0].as_ref().unwrap();
    for (left, right) in expected.rect.into_iter().zip(actual.rect) {
        assert!((left - right).abs() < 0.001);
    }
    assert!((expected.angle - actual.angle).abs() < 0.001);
    assert!((expected.depth - actual.depth).abs() < 0.001);
    poses.insert(
        ("1", "Grip"),
        crate::puppet::AttachmentPose {
            affine: crate::affine::Affine::new([11.0, 0.0], [1.0, 1.0], 0.0),
            depth: 0.0,
        },
    );
    host.set_attachment_poses(&poses);
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][1]["text"], "21");
}

#[test]
fn parses_vector_colors() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":r"
export function init() {
    const first = new Vec3('0.2 0.4 0.6');
    const second = Vec4.fromString('1 2 3 4');
    first.g = 0.8;
    second.a = 5;
    if (!first.equals(new Vec3(0.2,0.8,0.6)) || second.w !== 5) throw Error('aliases');
    if (!Vec2.fromString('4 5').equals(new Vec2(4,5))) throw Error('parse');
    if (!new Vec3(7).equals(new Vec3(7,7,7)) || !new Vec3(7,8).equals(new Vec3(7,8,0))) throw Error('scalar');
    return first.toString() + '|' + second.toString();
}"}}]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["text"], "0.2 0.8 0.6|1 2 3 5");
}

#[test]
fn scheme_color_refresh() {
    let project =
        json!({"general":{"properties":{"schemecolor":{"type":"color","value":"1 0 0"}}}});
    let script = r"
export function update() {
    const scheme = engine.colorScheme;
    return JSON.stringify([scheme.x,scheme.g,scheme.primary.b,scheme.secondary.r,scheme.text.r,scheme.highContrast.r]);
}";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host =
        SceneScripts::load(&mut scene, &crate::effects::parse_properties(&project), &project)
            .unwrap()
            .unwrap();
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "[1,0,0,1,0,0]");
    host.update_properties(&Properties::from([("schemecolor".into(), vec![0.25, 0.5, 0.75])]))
        .unwrap();
    host.tick(2.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "[0.25,0.5,0.75,1,0,0]");
}

#[test]
fn rotates_local_axes() {
    let mut scene = json!({"objects":[{"id":1,"angles":"0 0 0","text":{"value":"","script":r"
export function init() {
    thisLayer.rotateObjectSpace(new Vec3(0, 0, 90));
    return String(thisLayer.angles.z);
}"}}]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["text"], "90");
    assert!(
        (scene["objects"][0]["angles"]
            .as_str()
            .unwrap()
            .split(' ')
            .nth(2)
            .unwrap()
            .parse::<f32>()
            .unwrap()
            - std::f32::consts::FRAC_PI_2)
            .abs()
            < 0.001
    );
}

#[test]
fn hidden_bindings_keep_state() {
    let mut scene = json!({"objects":[{"id":1,"image":"fixture","metadata":{"value":1,"script":r"
export function init(value) {
    if (thisLayer.metadata !== undefined) throw Error('authored metadata');
    thisLayer.metadata = {local:true};
    return value + 1;
}
export function update(value) {
    if (thisLayer.metadata.local !== true) throw Error('custom replaced');
    return value + 1;
}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert_eq!(scene["objects"][0]["metadata"], 2);
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["metadata"], 3);
}

#[test]
fn unknown_classes_keep_fallback() {
    for mut layer in [json!({}), json!({"image":"fixture","text":"fixture"})] {
        layer["id"] = json!(1);
        layer["metadata"] = json!({"value":1});
        layer["visible"] = json!({"value":true,"script":r"
export function init() {
    if (thisLayer.metadata.value !== 1) throw Error('legacy metadata');
    thisLayer.metadata.value = 2;
    return true;
}"});
        let mut scene = json!({"objects":[layer]});
        let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(scene["objects"][0]["metadata"]["value"], 2);
    }
}

#[test]
fn initial_config_ids() {
    let mut scene = json!({"objects":[
        {"id":10,"name":"first","text":{"value":"", "script":r"
export function init() {
    if (thisLayer.id !== undefined || 'id' in thisLayer || Object.hasOwn(thisLayer, 'id') || Object.keys(thisLayer).includes('id') || Object.getOwnPropertyDescriptor(thisLayer, 'id')) throw Error('public id');
    for (const key of [thisLayer, 0, 'first']) {
        const config = thisScene.getInitialLayerConfig(key);
        if (config.id !== undefined || Object.hasOwn(config, 'id') || Object.keys(config).includes('id')) throw Error('layer id exported');
        if (config.name !== 'first') throw Error('wrong config');
    }
    const particle = thisScene.getInitialLayerConfig('particle');
    if (particle.id !== undefined || particle.instanceoverride.id !== 1013) throw Error('nested id changed');
    particle.instanceoverride.id = 99;
    thisScene.sortLayer(thisLayer, 1);
    if (thisScene.getInitialLayerConfig('particle').instanceoverride.id !== 1013) throw Error('initial config changed');
    if (thisScene.getInitialLayerConfig(thisLayer).name !== 'first' || thisScene.getLayer('10') !== thisLayer) throw Error('layer identity changed');
    const custom = {value:10};
    thisLayer.id = custom;
    if (thisLayer.id !== custom || thisScene.getLayer('10') !== thisLayer || thisScene.getLayerByID('10') !== thisLayer) throw Error('assigned id');
    Object.defineProperty(thisLayer, 'id', {value:11, configurable:true});
    if (thisLayer.id !== 11 || thisScene.getLayer('11') === thisLayer) throw Error('defined id');
    delete thisLayer.id;
    if (thisLayer.id !== undefined || thisScene.getLayer('10') !== thisLayer) throw Error('deleted id');
    return 'ready';
}"}},
        {"id":20,"name":"particle","particle":"particles/probe.json","instanceoverride":{"id":1013}}
    ]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "ready");
    assert_eq!(host.scene["objects"][0]["id"], 10);
    assert_eq!(host.scene["objects"][1]["id"], 20);
}

#[test]
fn initial_config_keeps_bindings() {
    let mut scene = json!({"objects":[
        {"id":10,"name":"target","alpha":{"user":"opacity","value":0.7},"effects":[{"id":1000,"name":"probe","visible":false}],"metadata":{"nested":{"value":42}}},
        {"id":20,"text":{"value":"","script":r"
export function init() {
    const target = thisScene.getLayer('target');
    const first = thisScene.getInitialLayerConfig('target');
    if (first.id !== undefined || first.alpha.user !== 'opacity' || first.alpha.value !== 0.7) throw Error('authored alpha');
    if (Math.abs(target.alpha - 0.3) > 0.000001) throw Error('resolved alpha');
    if (first.effects[0].id !== 1000 || first.effects[0].name !== 'probe') throw Error('effect metadata');
    first.alpha.value = 1;
    first.effects[0].id = 2;
    first.metadata.nested.value = 99;
    const second = thisScene.getInitialLayerConfig(target);
    if (second.alpha.value !== 0.7 || second.effects[0].id !== 1000 || second.metadata.nested.value !== 42) throw Error('shared config');
    if (thisScene.getInitialLayerConfig('10').name !== 'target' || thisScene.getInitialLayerConfig(0).name !== 'target') throw Error('lookup');
    return 'ready';
}"}}
    ]});
    let props = Properties::from([("opacity".into(), vec![0.3])]);
    let host = SceneScripts::load(&mut scene, &props, &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["alpha"], json!({"user":"opacity","value":0.7}));
    assert_eq!(scene["objects"][1]["text"], "ready");
}

#[test]
fn initial_config_lookup_forms() {
    let mut objects = vec![json!({"id":10,"name":"target"})];
    for index in 0..10 {
        objects.push(json!({"id":100 + index,"name":format!("cell-{index}")}));
    }
    objects.push(json!({"id":20,"text":{"value":"","script":r"
export function init() {
    for (const key of ['missing', '0', 999, -1, undefined, null, {}]) {
        if (thisScene.getInitialLayerConfig(key) !== null) throw Error('missing config');
    }
    if (thisScene.getInitialLayerConfig('10').name !== 'target') throw Error('string id');
    if (thisScene.getInitialLayerConfig(0).name !== 'target') throw Error('first index');
    if (thisScene.getInitialLayerConfig(1).name !== 'cell-0') throw Error('second index');
    if (thisScene.getInitialLayerConfig(10).name !== 'cell-9') throw Error('last index');
    if (thisScene.getInitialLayerConfig(thisScene.getLayer('target')).name !== 'target') throw Error('layer object');
    return 'ready';
}"}}));
    let mut scene = json!({"objects":objects});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][11]["text"], "ready");
}

#[test]
fn omitted_sizes_reach_scene() {
    for (kind, value) in [("image", json!("image.json")), ("text", json!("hello"))] {
        let mut layer = json!({"id":1,"visible":{"value":true,"script":"export function init(){thisLayer.size=new Vec2(40,30);thisLayer.volume=0.25;return true;}"}});
        layer[kind] = value;
        let mut scene = json!({"objects":[layer]});
        let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        assert!(host.diagnostics.is_empty(), "{kind}: {:?}", host.diagnostics);
        assert_eq!(scene["objects"][0]["size"], "40 30");
        assert!(scene["objects"][0].get("volume").is_none());
    }
}

#[test]
fn omitted_layer_parallax() {
    for (kind, value) in [
        ("image", json!("image.json")),
        ("text", json!("hello")),
        ("sound", json!(["silent.wav"])),
        ("particle", json!("particle.json")),
    ] {
        let mut layer = json!({"id":1,"visible":{"value":true,"script":"export function init(){const p=thisLayer.parallaxDepth;if(!(p instanceof Vec2)||p.x!==1||p.y!==1||!Object.hasOwn(thisLayer,'parallaxDepth'))throw Error('parallax default');return true;}"}});
        layer[kind] = value;
        let mut scene = json!({"objects":[layer]});
        let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        assert!(host.diagnostics.is_empty(), "{kind}: {:?}", host.diagnostics);
    }
}

#[test]
fn custom_ids_preserve_dispatch() {
    let script = r"
export function init() {
    const parent = thisLayer.getParent();
    if (parent.getChildren()[0] !== thisLayer) throw Error('children');
    parent.id = new Vec3(1,2,3);
    thisLayer.id = {custom:20};
    if (thisLayer.getParent() !== parent || parent.getChildren()[0] !== thisLayer) throw Error('parent');
    if (thisScene.getLayer('10') !== parent || thisScene.getLayerByID('20') !== thisLayer) throw Error('lookup');
    return 'ready';
}
export function cursorClick() { thisLayer.text = 'clicked'; }
";
    let mut scene = json!({"objects":[
        {"id":10,"name":"parent","text":""},
        {"id":20,"name":"child","parent":10,"text":{"value":"", "script":script}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.pointer([0.2; 2], [true, false, false], vec!["20".into()]).unwrap();
    host.pointer([0.2; 2], [false; 3], vec!["20".into()]).unwrap();
    host.tick(1.0, 0.016, [0.2; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][1]["text"], "clicked");
    assert_eq!(host.scene["objects"][0]["id"], 10);
    assert_eq!(host.scene["objects"][1]["id"], 20);
}

#[test]
fn legacy_vectors_keep_snapshots() {
    let mut scene = json!({"objects":[{"id":1,"origin":"10 20 0","angles":"0 0 1.5707963267948966","metadata":{"values":["日本",2],"vector":{"origin":"1 2 3"}},"visible":{"value":true, "script":r"
export function init() {
    if (thisLayer.__proto__ !== Object.getPrototypeOf(thisLayer)) throw Error('prototype changed');
    const first = thisScene.getInitialLayerConfig(thisLayer);
    first.metadata.values[0] = 'changed';
    thisLayer.metadata.values[1] = 3;
    Object.freeze(thisLayer.metadata.vector);
    if (thisLayer.metadata.vector.origin.x !== 1) throw Error('frozen vector was not converted');
    const origin = thisLayer.origin;
    if (origin === thisLayer.origin) throw Error('shared origin');
    origin.x = 42;
    const angles = thisLayer.angles;
    if (angles !== thisLayer.angles || Math.abs(angles.z-90)>0.0001) throw Error('angles converted twice');
    Object.defineProperty(thisLayer, 'angles', {enumerable:false});
    Object.seal(thisLayer);
    if (thisLayer.angles !== angles || Math.abs(thisLayer.angles.z-90)>0.0001) throw Error('descriptor converted angles');
    angles.z = 180;
    const second = thisScene.getInitialLayerConfig(thisLayer);
    if (second.origin !== '10 20 0' || second.metadata.values[0] !== '日本' || second.metadata.values[1] !== 2 || second.scale !== undefined) throw Error('initial config changed');
    if (thisScene.getInitialLayerConfig('missing') !== null) throw Error('unknown layer');
    return true;
}
export function update() { return true; }"}}]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["origin"], "10 20 0");
    assert_eq!(scene["objects"][0]["angles"], "0 0 3.141592653589793");
    assert_eq!(scene["objects"][0]["metadata"]["values"][1], 3);
}

#[test]
fn custom_objects_keep_identity() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"", "script":r"
let supplied;
export function init() {
    supplied = {nested:{alpha:1}};
    thisLayer.custom = supplied;
    if (thisLayer.custom !== supplied) throw Error('identity');
    const before = Object.keys(thisLayer.custom).join(',');
    const missing = thisLayer.custom.size;
    if (missing !== undefined || before !== Object.keys(thisLayer.custom).join(',')) throw Error('missing read changed keys');
}
export function update() { supplied.nested.alpha -= 0.25; if (thisLayer.custom.nested.alpha !== supplied.nested.alpha) throw Error('alias'); return String(supplied.nested.alpha); }
"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "0.75");
    host.tick(2.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "0.5");
    assert!(host.scene["objects"][0].get("custom").is_none());
}

#[test]
fn custom_properties_keep_semantics() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"", "script":r"
let held;
export function init() {
    const cycle = {}; cycle.self = cycle;
    const symbol = Symbol('private');
    held = new Vec3(2,3,4);
    thisLayer.custom = cycle;
    thisLayer.vector = held;
    thisLayer[symbol] = held;
    thisLayer.call = () => held;
    if (thisLayer.custom !== cycle || thisLayer.custom.self !== cycle || thisLayer.vector !== held || thisLayer[symbol] !== held || thisLayer.call() !== held) throw Error('custom identity');
    Object.defineProperty(thisLayer, 'accessor', {configurable:true, get(){return this.vector;}, set(value){this.vector=value;}});
    if (thisLayer.accessor !== held) throw Error('getter receiver');
    thisLayer.accessor = cycle;
    if (thisLayer.vector !== cycle) throw Error('setter receiver');
    Object.defineProperty(thisLayer, 'locked', {value:3, writable:false});
    try { thisLayer.locked = 4; throw Error('write accepted'); } catch(error) { if (error.message === 'write accepted') throw error; }
    if (thisLayer.locked !== 3) throw Error('write changed');
    delete thisLayer.accessor;
    return 'ready';
}
export function update() { return thisLayer.custom.self === thisLayer.custom && thisLayer.call() === held ? 'stable' : 'lost'; }
"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "stable");
    for key in ["custom", "vector", "call", "accessor", "locked"] {
        assert!(host.scene["objects"][0].get(key).is_none());
    }
}

#[test]
fn audio_registrations_return_distinct_stable_views_of_shared_samples() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"", "script":r"
const a=engine.registerAudioBuffers(64), b=engine.registerAudioBuffers(64);
const other=engine.registerAudioBuffers(16), held=a.left;
export function init() {
    if (!(a.left instanceof Float32Array) || !(a.right instanceof Float32Array) || !(a.average instanceof Float32Array)) throw Error('audio type');
    if (a === b || a.left === b.left || a.left === a.right) throw Error('audio identity');
    a.left[0] = 42;
    if (b.left[0] !== 42 || a.right[0] !== 0 || other.left[0] !== 0) throw Error('audio sharing');
}
export function update() {
    if (held !== a.left) throw Error('audio view replaced');
    return [a.left[0],b.right[0],a.average[0],other.average[0]].join(',');
}
"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.needs_audio());
    host.audio(64, &[0.25; 64], &[0.75; 64]).unwrap();
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "0.25,0.75,0.5,0");
    host.audio(16, &[1.0; 16], &[2.0; 16]).unwrap();
    host.tick(2.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "0.25,0.75,0.5,1.5");
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn hundreds_of_audio_registrations_stay_active_across_frames() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"", "script":"const buffers=Array.from({length:432},()=>engine.registerAudioBuffers(64)); export function update(){return String(buffers[0].average[0]+buffers[431].average[63]);}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    for frame in 1..=60 {
        let value = frame as f32;
        host.audio(64, &[value; 64], &[value; 64]).unwrap();
        host.tick(value / 60.0, 1.0 / 60.0, [0.5; 2]).unwrap();
    }
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert!(host.animated());
    assert_eq!(host.scene["objects"][0]["text"], "120");
}

#[test]
fn audio_registration_phase() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"", "script":r"
const sizes = [0, 1, 8, 16, 24, 32, 64, 128, 129, -1];
const accepted = sizes.map(size => {
    try {
        const buffer = engine.registerAudioBuffers(size);
        return buffer.left.length === size && buffer.right.length === size && buffer.average.length === size;
    } catch { return false; }
});
let initRejected = false;
export function init() {
    try { engine.registerAudioBuffers(engine.AUDIO_RESOLUTION_16); }
    catch { initRejected = true; }
}
export function update() {
    let updateRejected = false;
    try { engine.registerAudioBuffers(engine.AUDIO_RESOLUTION_32); }
    catch { updateRejected = true; }
    return accepted.map(Number).join('') + String(Number(initRejected)) + String(Number(updateRejected));
}
"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.needs_audio());
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "000101100011");
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn vector_updates_preserve_units_dimensions_and_detached_values() {
    let mut scene = json!({"objects":[{"id":1,"origin":"1 2 3","angles":"0 0 0","size":"10 20","text":{"value":"", "script":r"
let old;
export function init() { old = thisLayer.origin;
    thisLayer.origin = new Vec4(4,5,6,7);
    thisLayer.angles = new Vec3(0,0,90);
    thisLayer.size = new Vec3(30,40,50);
    const custom = new Vec3(7,8,9);
    thisLayer.custom = custom;
    thisLayer.scale = new Vec3('4','5','6');
    if (thisLayer.scale.add(1).x !== 5) throw Error('numeric conversion');
    if (thisLayer.origin.w !== undefined || thisLayer.size.z !== undefined) throw Error('dimensions');
    if (old.x !== 1 || old.y !== 2 || old.z !== 3) throw Error('old value changed');
    if (thisLayer.custom !== custom || thisLayer.angles.z !== 90) throw Error('conversion');
    return 'ready';
}
export function update() { return 'ready'; }"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    for frame in 1..4 {
        assert!(!host.tick(frame as f32, 1.0, [0.5; 2]).unwrap());
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(host.scene["objects"][0]["origin"], "4 5 6");
        assert_eq!(host.scene["objects"][0]["angles"], "0 0 1.5707963267948966");
        assert_eq!(host.scene["objects"][0]["size"], "30 40");
        assert!(host.scene["objects"][0].get("custom").is_none());
    }
}

#[test]
fn plain_object_vectors() {
    let mut scene = json!({"objects":[{"id":1,"origin":"1 2 3","scale":"1 1 1","size":"10 20","text":{"value":"", "script":r"
export function update() {
    thisLayer.scale = { x: 2, y: 3, z: 4 };
    thisLayer.origin = { x: 5, y: 6 };
    thisLayer.size = { x: 7, y: 8 };
    if (thisLayer.scale.add(1).z !== 5) throw Error('vector');
    return 'ready';
}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["scale"], "2 3 4");
    assert_eq!(host.scene["objects"][0]["origin"], "5 6 0");
    assert_eq!(host.scene["objects"][0]["size"], "7 8");
}

#[test]
fn native_origin_copies_components() {
    let mut scene = json!({"objects":[{"id":1,"origin":"0 0 0","angles":"0 0 0",
        "alpha":{"value":1,"script":r"
export function init() {
    thisLayer.origin = new Vec3(4,5,6);
    thisLayer.angles = new Vec3(0,0,90);
}
export function update() {
    thisLayer.origin.x += 1;
    thisLayer.angles.z += 15;
    return 1;
}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    for frame in 1..4 {
        assert!(host.tick(frame as f32, 1.0, [0.5; 2]).unwrap());
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        let origin = crate::effects::json_numbers(&host.scene["objects"][0]["origin"]).unwrap();
        let angles = crate::effects::json_numbers(&host.scene["objects"][0]["angles"]).unwrap();
        assert_eq!(origin, vec![4.0, 5.0, 6.0]);
        assert!((angles[2] - (90.0 + frame as f32 * 15.0).to_radians()).abs() < 0.00001);
    }
}

#[test]
fn sorting_keeps_layer_owners() {
    let mut scene = json!({"objects":[
        {"id":10,"name":"red","alpha":0.2,"text":{"value":"", "script":"export function init(){const red=thisLayer; thisScene.sortLayer(red,1); red.alpha=0.4; return thisScene.enumerateLayers().map(layer=>layer.name).join(',')+':'+thisScene.getLayerIndex(red)+':'+thisScene.getInitialLayerConfig(red).alpha;} export function update(){thisLayer.origin=new Vec3(12,34,0);}"}},
        {"id":20,"name":"green","alpha":0.7,"text":{"value":"", "script":"export function init(){return thisLayer.name+':'+thisScene.getLayer(0).name;}"}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "green,red:1:0.2");
    assert_eq!(host.scene["objects"][1]["text"], "green:green");
    assert_eq!(host.scene["objects"][0]["alpha"], f64::from(0.4_f32));
    assert_eq!(host.scene["objects"][1]["alpha"], 0.7);
    assert_eq!(host.take_commands(), vec![ScriptCommand::Sort { order: vec![1, 0] }]);
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["origin"], "12 34 0");
    assert!(host.take_commands().is_empty());
}

#[test]
fn sorting_coalesces_commands() {
    let mut scene = json!({"objects":[
        {"id":10,"name":"red","text":{"value":"", "script":"export function init(){for(let i=0;i<5000;i++) thisScene.sortLayer(thisLayer,i%2); return thisScene.getLayerIndex(thisLayer);}"}},
        {"id":20,"name":"green"}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert_eq!(host.scene["objects"][0]["text"], 1);
    assert_eq!(host.take_commands(), vec![ScriptCommand::Sort { order: vec![1, 0] }]);
}

#[test]
fn sorting_matches_engine_arguments() {
    for (target, index, accepted, rank) in [
        ("thisLayer", "1", true, 1),
        ("'red'", "1", true, 1),
        ("'10'", "1", true, 1),
        ("0", "1", true, 1),
        ("0.9", "1", true, 1),
        ("{}", "1", false, 0),
        ("null", "1", false, 0),
        ("'missing'", "1", false, 0),
        ("thisLayer", "-1", true, 2),
        ("thisLayer", "999", true, 2),
        ("thisLayer", "0", true, 0),
        ("thisLayer", "1.9", true, 1),
        ("thisLayer", "NaN", true, 0),
        ("thisLayer", "Infinity", true, 0),
        ("thisLayer", "4294967296", true, 0),
        ("thisLayer", "-0.5", true, 0),
        ("thisLayer", "'1'", false, 0),
        ("thisLayer", "null", false, 0),
        ("thisLayer", "undefined", false, 0),
        ("thisLayer", "true", false, 0),
    ] {
        let script = format!(
            "export function init(){{return String(thisScene.sortLayer({target},{index}))+':'+thisScene.getLayerIndex(thisLayer);}}"
        );
        let mut scene = json!({"objects":[
            {"id":10,"name":"red","text":{"value":"", "script":script}},
            {"id":20,"name":"green"}, {"id":30,"name":"blue"}
        ]});
        let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        assert_eq!(
            host.scene["objects"][0]["text"],
            format!("{accepted}:{rank}"),
            "{target}, {index}"
        );
    }
}

#[test]
fn sorting_rejects_invalid_permutations() {
    for order in ["[]", "[0]", "[0,0]", "[0,2]", "[-1,0]", "[1.5,0]", "null", "[0,1,2]"] {
        let script = format!("export function init(){{__command('sort','','',{order});}}");
        let mut scene = json!({"objects":[
            {"id":10,"text":{"value":"", "script":script}}, {"id":20}
        ]});
        let mut host =
            SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        assert!(host.take_commands().is_empty(), "{order}");
    }
}

#[test]
fn sorting_preserves_sprite_slots() {
    let mut scene = json!({"objects":[
        {"id":10,"image":"red.json","text":{"value":"", "script":"const sprite=thisLayer.getTextureAnimation(); export function init(){thisScene.sortLayer(thisLayer,1);} export function update(){if(engine.runtime>0){sprite.pause();thisScene.getLayer(0).getTextureAnimation().play();}}"}},
        {"id":20,"image":"green.json"}
    ]});
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        super::Load {
            sprites: Some(&[(0, vec![0.5; 4]), (1, vec![0.5; 4])]),
            ..super::Load::default()
        },
    )
    .unwrap()
    .unwrap();
    host.take_commands();
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(
        host.take_commands(),
        vec![
            ScriptCommand::Sprite { object: 0, op: SpriteOp::Pause },
            ScriptCommand::Sprite { object: 1, op: SpriteOp::Play },
        ]
    );
}

#[test]
fn sorting_restores_accepted_order() {
    let mut scene = json!({"objects":[
        {"id":10,"name":"red","text":{"value":"", "script":"export function init(){thisScene.sortLayer(thisLayer,1);} export function update(){if(engine.runtime>0&&engine.runtime<3)thisScene.sortLayer(thisLayer,2);if(engine.runtime===3)thisScene.sortLayer(thisLayer,0);return thisScene.enumerateLayers().map(layer=>layer.name).join(',');}"}},
        {"id":20,"name":"green"},{"id":30,"name":"blue"}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    let ScriptCommand::Sort { order } = host.take_commands().remove(0) else {
        panic!("sort missing")
    };
    host.accept_layer_order(order);
    for time in [1.0, 2.0] {
        host.tick(time, 0.016, [0.5; 2]).unwrap();
        assert_eq!(host.take_commands(), vec![ScriptCommand::Sort { order: vec![1, 2, 0] }]);
        host.reject_layer_order();
        host.tick(0.0, 0.016, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["text"], "green,red,blue");
        assert!(host.take_commands().is_empty());
    }
    assert_eq!(host.diagnostics, ["layer sort rejected: unsupported scene target dependencies"]);
    host.tick(3.0, 0.016, [0.5; 2]).unwrap();
    let ScriptCommand::Sort { order } = host.take_commands().remove(0) else {
        panic!("sort missing")
    };
    assert_eq!(order, [0, 1, 2]);
    host.accept_layer_order(order);
    host.reject_layer_order();
    host.tick(4.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "red,green,blue");
    assert_eq!(host.diagnostics.len(), 1);
}

#[test]
fn destruction_preserves_dispatch() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"observer","text":{"value":"","script":r"
let victim, stage=0, ticks;
export function init(){ shared.ticks=0;shared.hooks=0;victim=thisScene.getLayer('victim'); }
export function update(){
    if(engine.runtime===0)return;
    if(stage++===0){
        ticks=shared.ticks;
        const first=thisScene.destroyLayer(victim);
        return JSON.stringify([first,thisScene.destroyLayer(victim),thisScene.getLayerCount(),thisScene.enumerateLayers().map(layer=>layer.name),thisScene.getLayer('victim'),thisScene.getLayer(1),thisScene.getLayer(2).name,thisScene.getLayerIndex(victim),thisScene.getLayerIndex(thisScene.getLayer('survivor')),thisScene.getLayer(victim)===victim,victim.visible,victim.name,victim.color.x,shared.hooks,shared.ticks===ticks]);
    }
    return JSON.stringify([shared.ticks-ticks,shared.hooks,shared.hookTicks-ticks,shared.hookVisible,shared.hookName,victim.visible===undefined,victim.name===undefined,victim.color===undefined,thisScene.getLayer(1).name,thisScene.getLayerIndex(victim),thisScene.destroyLayer(victim)]);
}"}},
        {"id":2,"name":"victim","color":"1 0 0","visible":{"value":true,"script":"export function update(){shared.ticks++;return true;}export function destroy(){shared.hooks++;shared.hookTicks=shared.ticks;shared.hookVisible=thisLayer.visible;shared.hookName=thisLayer.name;}"}},
        {"id":3,"name":"survivor"}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.tick(1.0, 1.0, [0.5; 2]).unwrap());
    let immediate: serde_json::Value =
        serde_json::from_str(host.scene["objects"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        immediate,
        json!([
            true,
            true,
            2,
            ["observer", "survivor"],
            null,
            null,
            "survivor",
            1,
            2,
            true,
            true,
            "victim",
            1,
            0,
            true
        ])
    );
    assert_eq!(host.scene["objects"][1]["visible"], false);
    for frame in [2.0, 3.0] {
        host.tick(frame, 1.0, [0.5; 2]).unwrap();
        let retired: serde_json::Value =
            serde_json::from_str(host.scene["objects"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(
            retired,
            json!([1, 1, 1, true, "victim", true, true, true, "survivor", -1, false])
        );
    }
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn destruction_cancels_timers() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"observer","text":{"value":"","script":"export function init(){engine.setTimeout(()=>thisScene.destroyLayer(thisScene.getLayer('victim')),100);engine.setTimeout(()=>thisLayer.text+='survivor;',800);}"}},
        {"id":2,"name":"victim","visible":{"value":true,"script":"export function init(){engine.setTimeout(()=>thisScene.getLayer('observer').text+='victim;',500);}export function update(){return true;}export function destroy(){thisScene.getLayer('observer').text+='hook;';engine.setTimeout(()=>thisScene.getLayer('observer').text+='hook-timer;',100);}"}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(0.2, 0.2, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "hook;");
    assert!(host.animated());
    host.tick(1.0, 0.8, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "hook;survivor;");
    assert!(!host.animated());
    host.destroy();
    host.destroy();
    assert_eq!(host.scene["objects"][0]["text"], "hook;survivor;");
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn destroyed_visibility_stays_hidden() {
    let project = json!({"general":{"properties":{"shown":{"type":"bool","value":true}}}});
    let mut scene = json!({"objects":[
        {"id":1,"text":{"value":"","script":"let victim;export function init(){victim=thisScene.getLayer('victim');}export function update(){if(engine.runtime>0){thisScene.destroyLayer(victim);victim.visible=true;victim.color=new Vec3(0,1,0);}return 'observer';}"}},
        {"id":2,"name":"victim","visible":{"user":"shown","value":true}}
    ]});
    let mut host =
        SceneScripts::load(&mut scene, &crate::effects::parse_properties(&project), &project)
            .unwrap()
            .unwrap();
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    host.scene["objects"][1]["visible"] = json!(true);
    assert!(host.tick(2.0, 1.0, [0.5; 2]).unwrap());
    assert_eq!(host.scene["objects"][1]["visible"], false);
    for (frame, shown) in [(3.0, 0.0), (4.0, 1.0)] {
        assert!(
            host.update_properties(&Properties::from([("shown".into(), vec![shown])])).unwrap()
        );
        host.tick(frame, 1.0, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][1]["visible"], false);
    }
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn destruction_keeps_sort_slots() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"observer","text":{"value":"","script":r"
let stage=0,victim;
export function init(){victim=thisScene.getLayer('victim');}
export function update(){
    if(engine.runtime===0)return;
    if(stage++===0){thisScene.destroyLayer(victim);thisScene.sortLayer('green',0);return JSON.stringify([thisScene.getLayerIndex(victim),thisScene.getLayer(2),thisScene.enumerateLayers().map(layer=>layer.name)]);}
    const before=thisScene.enumerateLayers().map(layer=>layer.name);
    thisScene.sortLayer('tail',0);
    return JSON.stringify([before,thisScene.getLayerIndex(victim),thisScene.enumerateLayers().map(layer=>layer.name)]);
}"}},
        {"id":2,"name":"victim"},{"id":3,"name":"green"},{"id":4,"name":"tail"}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "[2,null,[\"green\",\"observer\",\"tail\"]]");
    assert_eq!(host.take_commands(), vec![ScriptCommand::Sort { order: vec![2, 0, 1, 3] }]);
    host.reject_layer_order();
    host.tick(2.0, 1.0, [0.5; 2]).unwrap();
    assert_eq!(
        host.scene["objects"][0]["text"],
        "[[\"observer\",\"green\",\"tail\"],-1,[\"tail\",\"observer\",\"green\"]]"
    );
    assert_eq!(host.take_commands(), vec![ScriptCommand::Sort { order: vec![3, 1, 0, 2] }]);
    assert_eq!(host.scene["objects"][1]["visible"], false);
    assert_eq!(host.diagnostics.len(), 1);
}

#[test]
fn event_timers_keep_owners() {
    for event in ["cursorClick", "mediaStatusChanged", "applyUserProperties"] {
        let script = format!(
            "export function {event}(event){{if(event.armed===false)return;engine.setTimeout(()=>thisScene.getLayer('observer').text+='late;',500);thisScene.getLayer('observer').text+='event;';thisScene.destroyLayer(thisLayer);}}export function destroy(){{thisScene.getLayer('observer').text+='hook;';}}"
        );
        let project = json!({"general":{"properties":{"armed":{"type":"bool","value":false}}}});
        let mut scene = json!({"objects":[
            {"id":1,"name":"observer","text":"","alpha":{"user":"armed","value":1}},
            {"id":2,"name":"victim","visible":{"value":true,"script":script}}
        ]});
        let props = crate::effects::parse_properties(&project);
        let mut host = SceneScripts::load(&mut scene, &props, &project).unwrap().unwrap();
        assert!(!host.animated());
        match event {
            "cursorClick" => {
                host.pointer([0.5; 2], [true, false, false], vec!["2".into()]).unwrap();
                host.pointer([0.5; 2], [false; 3], vec!["2".into()]).unwrap();
            }
            "mediaStatusChanged" => {
                host.media(&json!({"mediaStatusChanged":{"armed":true}})).unwrap();
            }
            "applyUserProperties" => {
                host.update_properties(&Properties::from([("armed".into(), vec![1.0])])).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(host.animated(), "{event}: {:?}", host.diagnostics);
        host.tick(0.1, 0.1, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["text"], "event;hook;");
        assert!(!host.animated());
        assert!(!host.needs_media());
        host.pointer([0.6; 2], [true, false, false], vec!["2".into()]).unwrap();
        host.pointer([0.6; 2], [false; 3], vec!["2".into()]).unwrap();
        host.media(&json!({"mediaStatusChanged":{"armed":true,"changed":true}})).unwrap();
        host.update_properties(&Properties::from([("armed".into(), vec![0.0])])).unwrap();
        host.tick(1.0, 0.9, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["text"], "event;hook;");
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    }
}

#[test]
fn module_timers_keep_owners() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"observer","text":{"value":"","script":"export function init(){engine.setTimeout(()=>thisScene.destroyLayer(thisScene.getLayer('victim')),100);}"}},
        {"id":2,"name":"victim","visible":{"value":true,"script":"engine.setTimeout(()=>engine.setTimeout(()=>thisScene.getLayer('observer').text+='late;',500),50);export function destroy(){thisScene.getLayer('observer').text+='hook;';}"}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(0.05, 0.05, [0.5; 2]).unwrap();
    host.tick(0.1, 0.05, [0.5; 2]).unwrap();
    assert!(!host.animated());
    host.tick(1.0, 0.9, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "hook;");
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn destroy_hooks_keep_lookup() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"observer","text":{"value":"","script":"export function init(){engine.setTimeout(()=>thisScene.destroyLayer(thisScene.getLayer('victim')),100);}export function update(){if(engine.runtime>0.1){thisScene.sortLayer(thisLayer,0);return JSON.stringify(thisScene.enumerateLayers().map(layer=>layer.name));}}"}},
        {"id":2,"name":"victim","visible":{"value":true,"script":"export function destroy(){const observer=thisScene.getLayer('observer');observer.text=JSON.stringify([thisScene.getLayer('victim')===null,thisLayer.name,thisLayer.visible,thisScene.sortLayer('survivor',0),thisScene.enumerateLayers().map(layer=>layer.name),thisScene.sortLayer(thisLayer,0)]);}"}},
        {"id":3,"name":"survivor"}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(0.1, 0.1, [0.5; 2]).unwrap();
    assert_eq!(
        host.scene["objects"][0]["text"],
        "[true,\"victim\",true,true,[\"survivor\",\"observer\"],false]"
    );
    assert_eq!(host.take_commands(), vec![ScriptCommand::Sort { order: vec![2, 1, 0] }]);
    host.tick(0.2, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.take_commands(), vec![ScriptCommand::Sort { order: vec![0, 1, 2] }]);
    assert_eq!(host.scene["objects"][0]["text"], "[\"observer\",\"survivor\"]");
    assert_eq!(host.scene["objects"][1]["visible"], false);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn timer_budget_keeps_destruction() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"observer","text":{"value":"","script":"export function init(){engine.setTimeout(()=>{while(true){}},100);}export function update(){if(engine.runtime>0)thisScene.destroyLayer(thisScene.getLayer('victim'));return 'observer';}"}},
        {"id":2,"name":"victim","visible":true}
    ]});
    let mut host = stalling_host(&mut scene);
    assert!(host.tick(0.1, 0.1, [0.5; 2]).unwrap());
    assert_eq!(host.scene["objects"][1]["visible"], false);
    assert!(!host.animated());
    assert_eq!(host.diagnostics.len(), 1);
    host.scene["objects"][1]["visible"] = json!(true);
    assert!(host.tick(0.2, 0.1, [0.5; 2]).unwrap());
    assert_eq!(host.scene["objects"][1]["visible"], false);
    assert!(!host.tick(0.3, 0.1, [0.5; 2]).unwrap());
}

#[test]
fn timeout_errors_keep_callbacks() {
    for error in [
        "Error('timer failure')",
        "Object.create(null)",
        "{toString(){throw Error('format failure');}}",
    ] {
        let script = "export function init(){engine.setTimeout(()=>{thisLayer.text+='throw;';throw THROWN;},100);engine.setTimeout(()=>thisLayer.text+='same;',100);}export function update(value){if(engine.runtime>=0.2)return value+'update;';}".replace("THROWN", error);
        let mut scene = json!({"objects":[
            {"id":1,"text":{"value":"","script":script}},
            {"id":2,"text":{"value":"","script":"export function init(){engine.setTimeout(()=>thisLayer.text+='peer;',100);}export function update(value){if(engine.runtime>=0.2)return value+'update;';}"}}
        ]});
        let mut host =
            SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        host.tick(0.1, 0.1, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["text"], "throw;same;");
        assert_eq!(host.scene["objects"][1]["text"], "peer;");
        host.tick(0.21, 0.11, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["text"], "throw;same;update;");
        assert_eq!(host.scene["objects"][1]["text"], "peer;update;");
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    }
}

#[test]
fn interval_errors_keep_running() {
    let script = r"
let count = 0;
let cancel;
export function init() {
    cancel = engine.setInterval(() => {
        thisLayer.text += ++count;
        if (count === 3) cancel();
        if (count === 1) throw Error('timer failure');
    }, 100);
}
";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    for time in [0.1, 0.21, 0.32] {
        host.tick(time, 0.11, [0.5; 2]).unwrap();
    }
    assert_eq!(host.scene["objects"][0]["text"], "123");
    assert!(!host.animated());
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn cancellation_removes_first() {
    for (first, second) in [(900, 1400), (1400, 900)] {
        let script = format!(
            "let cancel,stage=0;export function init(){{engine.setTimeout(()=>thisLayer.text+='first;',{first});cancel=engine.setTimeout(()=>thisLayer.text+='second;',{second});engine.setTimeout(()=>thisLayer.text+='third;',1600);}}export function update(value){{if(stage===0&&engine.runtime>0.3){{stage++;return value+cancel()+';';}}if(stage===1&&engine.runtime>0.6){{stage++;return value+cancel()+';';}}}}"
        );
        let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
        let mut host =
            SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        for time in [0.31, 0.61, 2.0] {
            host.tick(time, 0.3, [0.5; 2]).unwrap();
        }
        assert_eq!(host.scene["objects"][0]["text"], "true;false;second;third;");
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    }
}

#[test]
fn cancellation_keeps_bindings() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"owner","text":"","alpha":{"value":1,"script":"export function init(){engine.setTimeout(()=>thisLayer.text+='alpha;',900);}"},"visible":{"value":true,"script":"export function init(){engine.setTimeout(()=>thisLayer.text+='first;',900);shared.cancel=engine.setTimeout(()=>thisLayer.text+='second;',1400);}"}},
        {"id":2,"text":{"value":"","script":"let done=false;export function init(){engine.setTimeout(()=>thisLayer.text+='peer;',1000);}export function update(value){if(!done&&engine.runtime>0.3){done=true;return value+shared.cancel()+';';}}"}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(0.31, 0.31, [0.5; 2]).unwrap();
    host.tick(2.0, 1.69, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "alpha;second;");
    assert_eq!(host.scene["objects"][1]["text"], "true;peer;");
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn cancellation_handles_are_independent() {
    let script = "let first,second,stage=0;export function init(){first=engine.setTimeout(()=>thisLayer.text+='first;',1400);second=engine.setTimeout(()=>thisLayer.text+='second;',1300);engine.setTimeout(()=>thisLayer.text+='third;',1600);}export function update(value){if(stage===0&&engine.runtime>0.3){stage++;return value+first()+';';}if(stage===1&&engine.runtime>0.6){stage++;return value+second()+';';}}";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    for time in [0.31, 0.61, 2.0] {
        host.tick(time, 0.3, [0.5; 2]).unwrap();
    }
    assert_eq!(host.scene["objects"][0]["text"], "true;true;third;");
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn dynamic_cancellation_keeps_order() {
    let script = "let done=false;export function init(){engine.setTimeout(()=>thisLayer.text+='first;',100);engine.setTimeout(()=>thisLayer.text+='second;',900);}export function update(value){if(!done&&engine.runtime>0.3){done=true;const cancel=engine.setTimeout(()=>thisLayer.text+='third;',700);return value+cancel()+';';}}";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    for time in [0.11, 0.31, 2.0] {
        host.tick(time, 0.2, [0.5; 2]).unwrap();
    }
    assert_eq!(host.scene["objects"][0]["text"], "first;true;third;");
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn expired_cancellation_stays_inert() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":"let cancel,done=false;export function init(){cancel=engine.setTimeout(()=>thisLayer.text+='first;',100);engine.setTimeout(()=>thisLayer.text+='second;',900);}export function update(value){if(!done&&engine.runtime>0.3){done=true;return value+cancel()+';';}}"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    for time in [0.11, 0.31, 1.0] {
        host.tick(time, 0.2, [0.5; 2]).unwrap();
    }
    assert_eq!(host.scene["objects"][0]["text"], "first;false;second;");
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn promise_allocation_failure() {
    let mut failures = 0;
    for room in (0..2048).step_by(8) {
        let runtime = rquickjs::Runtime::new().unwrap();
        let context = rquickjs::Context::full(&runtime).unwrap();
        context.with(|ctx| ctx.eval::<(), _>("globalThis.run = async function () {};").unwrap());
        runtime.set_memory_limit(runtime.memory_usage().malloc_size as usize + room);
        context.with(|ctx| {
            let run: rquickjs::Function = ctx.globals().get("run").unwrap();
            if run.call::<_, rquickjs::Value>(()).is_err() {
                failures += 1;
                let _ = ctx.catch();
            }
        });
    }
    assert!(failures > 0);
}

#[test]
fn timer_jobs_settle() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":0,"script":"export function init() { engine.setTimeout(() => { Promise.resolve().then(() => { thisLayer.alpha = 1; }); }, 100); }"}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(0.1, 0.1, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["alpha"], 1);
    assert!(!host.animated());
}

#[test]
fn timer_jobs_respect_budget() {
    let script = r"
export function init() {
    engine.setTimeout(() => {
        thisScene.destroyLayer(thisScene.getLayer('victim'));
        function again() { Promise.resolve().then(again); }
        again();
    }, 100);
}
export function update(value) { return value; }
";
    let mut scene = json!({"objects":[
        {"id":1,"visible":{"value":true,"script":script}},
        {"id":2,"name":"victim","visible":true}
    ]});
    let mut host = stalling_host(&mut scene);
    assert!(host.animated());
    let started = std::time::Instant::now();
    assert!(host.tick(0.1, 0.1, [0.5; 2]).unwrap());
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(host.scene["objects"][1]["visible"], false);
    assert!(!host.animated());
    assert!(!host.tick(0.2, 0.1, [0.5; 2]).unwrap());
    assert!(
        host.diagnostics.iter().any(|message| message.contains("job budget exhausted")),
        "{:?}",
        host.diagnostics
    );
}

#[test]
fn nested_jobs_keep_order() {
    let script = r"
let order = '';
export function init() {
    Promise.resolve().then(() => {
        order += 'a';
        thisScene.destroyLayer(thisScene.getLayer('victim'));
        order += 'c';
    });
    Promise.resolve().then(() => { order += 'd'; });
}
export function update() { return order; }
";
    let mut scene = json!({"objects":[
        {"id":1,"name":"victim","alpha":{"value":1,"script":"export function destroy() { thisScene.getLayer('observer').alpha = 0.5; }"}},
        {"id":2,"name":"observer","alpha":1,"text":{"value":"","script":script}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][1]["text"], "acd");
    assert_eq!(host.scene["objects"][1]["alpha"], 0.5);
}

#[test]
fn callback_jobs_settle() {
    for (script, first) in [
        (
            "let state = 'pending'; export function init() { Promise.resolve().then(() => { state = 'done'; }); } export function update() { return state; }",
            "done",
        ),
        (
            "let state = 'pending'; export function update() { if (state === 'pending') Promise.resolve().then(() => { state = 'done'; }); return state; }",
            "pending",
        ),
    ] {
        let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
        let mut host =
            SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        host.tick(0.1, 0.1, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["text"], first);
        host.tick(0.2, 0.1, [0.5; 2]).unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(host.scene["objects"][0]["text"], "done");
    }
}

#[test]
fn callback_return_wins() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"target","alpha":{"value":1,"script":"export function init() { Promise.resolve().then(() => { thisLayer.alpha = 1; thisScene.getLayer('control').alpha = 0.75; }); return 0.75; }"}},
        {"id":2,"name":"control","alpha":1}
    ]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["alpha"], 0.75);
    assert_eq!(scene["objects"][1]["alpha"], 0.75);
    assert!(!host.animated());
}

#[test]
fn callback_jobs_keep_owners() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"victim","alpha":{"value":1,"script":"export function init() { Promise.resolve().then(() => { engine.setTimeout(() => { thisScene.getLayer('observer').alpha = 0; }, 100); }); }"}},
        {"id":2,"name":"observer","alpha":{"value":1,"script":"export function init() { thisScene.destroyLayer(thisScene.getLayer('victim')); }"}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][1]["alpha"], 1);
    assert!(!host.animated());
}

#[test]
fn rejected_jobs_continue() {
    let script = r"
let state = 'pending';
Promise.resolve().then(() => { throw Error('expected rejection'); });
Promise.resolve().then(() => { state = 'done'; });
export function init() { return state; }
";
    assert_eq!(script_text(script), "done");
}

#[test]
fn module_jobs_precede_init() {
    let script = r"
let state = 1;
Promise.resolve().then(() => { state *= 2; });
export function init() { state += 3; return String(state); }
export function update() { return String(state); }
";
    assert_eq!(script_text(script), "5");
}

#[test]
fn module_jobs_settle_chains() {
    let script = r"
let state = '';
Promise.resolve().then(() => { state += 'a'; }).then(() => { state += 'c'; });
Promise.resolve().then(() => { state += 'b'; });
export function init() { return state; }
";
    assert_eq!(script_text(script), "abc");
}

#[test]
fn module_jobs_keep_owners() {
    let mut scene = json!({"objects":[
        {"id":1,"name":"victim","alpha":{"value":1,"script":"Promise.resolve().then(() => { engine.setTimeout(() => { thisScene.getLayer('observer').alpha = 0; }, 100); });"}},
        {"id":2,"name":"observer","alpha":{"value":1,"script":"export function init() { thisScene.destroyLayer(thisScene.getLayer('victim')); }"}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][1]["alpha"], 1);
    assert!(!host.animated());
}

#[test]
fn module_jobs_respect_budget() {
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":1,"script":"function again() { Promise.resolve().then(again); } again(); export function init() { return 0; }"}}]});
    let started = std::time::Instant::now();
    let host = stalling_host(&mut scene);
    assert!(!host.diagnostics.is_empty());
    assert!(!host.animated());
    assert_eq!(scene["objects"][0]["alpha"], 1);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

fn script_text(script: &str) -> String {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let host = stalling_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    scene["objects"][0]["text"].as_str().unwrap().to_owned()
}

#[test]
fn vector_distance_matches_native() {
    let script = r"
export function init() {
    const a = new Vec3(-2.5, 0.5, 3.5);
    const b = new Vec3(0.5, -3.5, 1.5);
    return String(Math.abs(a.distance(b) - 5.385164807134504) < 0.00001 &&
        a.distanceSqr(b) === 29 && a.x === -2.5 && b.y === -3.5);
}";
    assert_eq!(script_text(script), "true");
}

#[test]
fn vec2_operand_keeps_depth() {
    let script = r"
export function init() {
    const value = new Vec3(4, 6, 8);
    const scale = new Vec2(2);
    return [value.add(scale), value.subtract(scale), value.multiply(scale), value.divide(scale)]
        .map(String).join(',') + '|' + Number.isNaN(value.multiply({x: 1, y: 1}).z) +
        Number.isNaN(new Vec4(1, 2, 3, 4).multiply(scale).z);
}";
    assert_eq!(script_text(script), "6 8 8,2 4 8,8 12 8,2 3 8|truetrue");
}

#[test]
fn vector_finiteness_matches_native() {
    let script = r"
export function init() {
    const value = new Vec3(-2.5, 0.5, 3.5);
    const results = [value.isFinite()];
    for (const invalid of [NaN, Infinity, -Infinity]) {
        value.y = invalid;
        results.push(value.isFinite());
    }
    return JSON.stringify(results);
}";
    assert_eq!(script_text(script), "[true,false,false,false]");
}

#[test]
fn vector_components_match_native() {
    let cases = [
        ("negate", "new Vec3(-2.5,0.5,3.5).negate()", "[2.5,-0.5,-3.5]"),
        ("abs", "new Vec3(-2.5,0.5,3.5).abs()", "[2.5,0.5,3.5]"),
        ("sign", "new Vec3(-2,0,3).sign()", "[-1,0,1]"),
        ("floor", "new Vec3(-2.5,0.5,3.5).floor()", "[-3,0,3]"),
        ("ceil", "new Vec3(-2.5,0.5,3.5).ceil()", "[-2,1,4]"),
        ("round", "new Vec3(-2.4,0.4,3.4).round()", "[-2,0,3]"),
        ("fract", "new Vec3(-2.5,0.5,3.5).fract()", "[0.5,0.5,0.5]"),
        ("min", "new Vec3(-2.5,0.5,3.5).min(new Vec3(1,-1,2))", "[-2.5,-1,2]"),
        ("max", "new Vec3(-2.5,0.5,3.5).max(new Vec3(1,-1,2))", "[1,0.5,3.5]"),
        ("clamp", "new Vec3(-2.5,0.5,3.5).clamp(-1,2)", "[-1,0.5,2]"),
        ("mod", "new Vec3(-5,5,-5).mod(3)", "[1,2,1]"),
        ("step", "new Vec3(-1,0,1).step(0)", "[0,1,1]"),
    ];
    for (method, call, expected) in cases {
        let script = format!(
            "export function init() {{ const result = {call}; return JSON.stringify([result.x,result.y,result.z]); }}"
        );
        assert_eq!(script_text(&script), expected, "{method}");
    }
}

#[test]
fn vector_interpolation_matches_native() {
    let script = r"
export function init() {
    const mixed = new Vec3(0, 2, 4).mix(new Vec3(4, 6, 8), 0.25);
    const smooth = new Vec3(-1, 0, 1).smoothStep(-2, 2);
    return JSON.stringify([mixed.x, mixed.y, mixed.z,
        smooth.x, smooth.y, smooth.z]);
}";
    assert_eq!(script_text(script), "[1,3,5,0.15625,0.5,0.84375]");
}

#[test]
fn equal_edges_match_native() {
    let script = r"
export function init() {
    const result = new Vec4(1, 0, 2, 0).smoothStep(1, 1);
    return String(Number.isNaN(result.x) && Object.is(result.y, 0) &&
        Object.is(result.z, 1) && Object.is(result.w, 0));
}";
    assert_eq!(script_text(script), "true");
}

#[test]
fn reversed_clamp_matches_native() {
    let script = r"
export function init() {
    const result = new Vec4(0, 1, 2, 3).clamp(2, 1);
    return JSON.stringify([result.x, result.y, result.z, result.w]);
}";
    assert_eq!(script_text(script), "[2,2,2,2]");
}

#[test]
fn signed_zero_matches_native() {
    let script = r"
export function init() {
    const value = new Vec4(-0, -0.5, 0, 0);
    return String(Object.is(value.sign().x, -0) &&
        Object.is(value.round().y, -0) && Object.is(value.floor().x, -0) &&
        Object.is(value.ceil().x, -0) && Object.is(value.round().x, -0));
}";
    assert_eq!(script_text(script), "true");
}

#[test]
fn vector_precision_matches_native() {
    let script = r"
export function init() {
    const constructed = new Vec4(1 / 3, 0, 0, 0);
    const added = new Vec4().add(1 / 3);
    return String(constructed.x === 1 / 3 && added.x === 1 / 3 &&
        constructed.x !== 0.3333333432674408 && added.x !== 0.3333333432674408);
}";
    assert_eq!(script_text(script), "true");
}

#[test]
fn vector_returns_stay_distinct() {
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":r"
export function init() {
    const original = new Vec2(-2, 4);
    const bound = new Vec2(0, 3);
    const results = [original.negate(), original.abs(), original.sign(),
        original.floor(), original.ceil(), original.round(), original.fract(),
        original.min(bound), original.max(bound), original.clamp(0, bound),
        original.mix(bound, 0.5), original.mod(2), original.step(0),
        original.smoothStep(0, 4)];
    return String(results.every(result => result !== original && result !== bound &&
        result.constructor === Vec2) && original.x === -2 && original.y === 4 &&
        bound.x === 0 && bound.y === 3);
}"}}]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["text"], "true");
}

#[test]
fn vector_reflection_matches_native() {
    let script = r"
export function init() {
    const inputs = [new Vec2(2, 3), new Vec3(2, 3, 4), new Vec4(2, 3, 4, 5)];
    return String(inputs.every(input => {
        const normal = input.map((_component, key) => key === 'y' ? 2 : 0);
        const result = input.reflect(normal);
        return result !== input && result.constructor === input.constructor &&
            result.x === 2 && result.y === -21 &&
            input.y === 3 && normal.y === 2;
    }));
}";
    assert_eq!(script_text(script), "true");
}

#[test]
fn vector_projection_matches_native() {
    let script = r"
export function init() {
    const inputs = [new Vec2(3, 4), new Vec3(3, 4, 5), new Vec4(3, 4, 5, 6)];
    return String(inputs.every(input => {
        const axis = input.map((_component, key) => key === 'x' ? 2 : 0);
        const projected = input.project(axis);
        const zero = input.project(input.map(() => 0));
        return projected !== input && projected.constructor === input.constructor &&
            projected.x === 3 && projected.y === 0 && zero.isFinite() &&
            zero.lengthSqr() === 0 && input.x === 3 && axis.x === 2;
    }));
}";
    assert_eq!(script_text(script), "true");
}

#[test]
fn perpendicular_points_clockwise() {
    let script = r"
export function init() {
    const input = new Vec2(3, 4);
    const result = input.perpendicular();
    return String(result !== input && result.constructor === Vec2 &&
        result.x === 4 && result.y === -3 && result.lengthSqr() === 25 &&
        input.x === 3 && input.y === 4);
}";
    assert_eq!(script_text(script), "true");
}

#[test]
fn planar_angles_match_native() {
    let script = r"
export function init() {
    const left = new Vec2(-1, 1);
    const start = new Vec2(Math.cos(170 * Math.PI / 180), Math.sin(170 * Math.PI / 180));
    const end = new Vec2(Math.cos(-170 * Math.PI / 180), Math.sin(-170 * Math.PI / 180));
    return String(left.angle() === 135 && new Vec2(-1, 0).angle() === 180 &&
        new Vec2(1, 0).angleBetween(new Vec2(0, -1)) === -90 &&
        Math.abs(start.angleBetween(end) - 20) < 0.00001 &&
        new Vec2().angleBetween(end) === 0 && left.x === -1);
}";
    assert_eq!(script_text(script), "true");
}

#[test]
fn planar_rotation_matches_native() {
    let script = r"
export function init() {
    const input = new Vec2(2, 3);
    const result = input.rotate(45);
    return String(result !== input && result.constructor === Vec2 &&
        Math.abs(result.x + Math.SQRT1_2) < 0.00001 &&
        Math.abs(result.y - 5 * Math.SQRT1_2) < 0.00001 &&
        input.x === 2 && input.y === 3);
}";
    assert_eq!(script_text(script), "true");
}

#[test]
fn spatial_angles_match_native() {
    let script = r"
export function init() {
    const axis = new Vec3(2, 0, 0);
    return String(Math.abs(axis.angleBetween(new Vec3(1, 1, 0)) - 45) < 0.00001 &&
        axis.angleBetween(new Vec3(-1, 0, 0)) === 180 &&
        axis.angleBetween(new Vec3(1, 0, 0)) === 0 &&
        axis.angleBetween(new Vec3()) === 0 && axis.x === 2);
}";
    assert_eq!(script_text(script), "true");
}

#[test]
fn spherical_axes_match_native() {
    let script = r"
export function init() {
    const cartesian = Vec3.fromSpherical(2, 60, 30);
    const spherical = cartesian.toSpherical();
    const negative = new Vec3(-2, 0, 0).toSpherical();
    const zero = new Vec3().toSpherical();
    return String(cartesian.constructor === Vec3 && spherical !== cartesian &&
        Math.abs(cartesian.x - 1.5) < 0.00001 &&
        Math.abs(cartesian.y - 1) < 0.00001 &&
        Math.abs(cartesian.z - Math.sqrt(3) / 2) < 0.00001 &&
        Math.abs(spherical.x - 2) < 0.00001 &&
        Math.abs(spherical.y - 60) < 0.00001 &&
        Math.abs(spherical.z - 30) < 0.00001 &&
        negative.z === 180 && zero.lengthSqr() === 0);
}";
    assert_eq!(script_text(script), "true");
}

#[test]
fn refraction_matches_native() {
    let script = r"
export function init() {
    const input = new Vec3(1, -1, 0);
    const normal = new Vec3(0, 2, 0);
    const result = input.refract(normal, 0.5);
    const blocked = new Vec3(Math.sqrt(3) / 2, -0.5, 0)
        .refract(new Vec3(0, 1, 0), 1.5);
    return String(result !== input && result.constructor === Vec3 &&
        Math.abs(result.x - 0.5) < 0.00001 &&
        Math.abs(result.y - (1.5 - 2 * Math.sqrt(1.75))) < 0.00001 &&
        blocked.lengthSqr() === 0 && input.y === -1 && normal.y === 2);
}";
    assert_eq!(script_text(script), "true");
}

#[test]
fn vector_classes_stay_independent() {
    let cases = [
        ("Vec2", "new Vec2(1)", "[true,false,false]"),
        ("Vec3", "new Vec3(1)", "[false,true,false]"),
        ("Vec4", "new Vec4(1)", "[false,false,true]"),
    ];
    for (name, value, expected) in cases {
        let script = format!(
            "export function init() {{ const value = {value}; return JSON.stringify([value instanceof Vec2,value instanceof Vec3,value instanceof Vec4]); }}"
        );
        assert_eq!(script_text(&script), expected, "{name}");
    }
}

#[test]
fn vector_methods_follow_dimension() {
    let cases = [
        ("Vec2", "new Vec2()", "function,function,function,undefined,undefined,function,undefined"),
        ("Vec3", "new Vec3()", "undefined,undefined,undefined,function,function,function,function"),
        (
            "Vec4",
            "new Vec4()",
            "undefined,undefined,undefined,undefined,undefined,undefined,undefined",
        ),
    ];
    for (name, value, expected) in cases {
        let script = format!(
            "export function init() {{ const value = {value}; return ['perpendicular','angle','rotate','cross','refract','angleBetween','toSpherical'].map(method => typeof value[method]).join(','); }}"
        );
        assert_eq!(script_text(&script), expected, "{name}");
    }
    assert_eq!(
        script_text(
            "export function init() { return [typeof Vec2.fromSpherical,typeof Vec3.fromSpherical,typeof Vec4.fromSpherical].join(','); }"
        ),
        "undefined,function,undefined"
    );
}

#[test]
fn vector_storage_keeps_dimensions() {
    let script = r"
export function init() {
    localStorage.set('three', new Vec3(1, 2, 3));
    localStorage.set('four', new Vec4(4, 5, 6, 7));
    const three = localStorage.get('three');
    const four = localStorage.get('four');
    return String(three instanceof Vec3 && !(three instanceof Vec2) &&
        four instanceof Vec4 && !(four instanceof Vec3) &&
        three.x === 1 && three.y === 2 && three.z === 3 &&
        four.x === 4 && four.y === 5 && four.z === 6 && four.w === 7);
}";
    assert_eq!(script_text(script), "true");
}

#[test]
fn matrix_classes_stay_independent() {
    let script = r"
export function init() {
    const a = new Mat3(), b = new Mat4();
    let rejected = 0;
    for (const Type of [Mat3, Mat4]) { try { Type(); } catch (_) { rejected++; } }
    return JSON.stringify([a instanceof Mat3, a instanceof Mat4, b instanceof Mat4, b instanceof Mat3, rejected, a.m.length, b.m.length]);
}";
    assert_eq!(script_text(script), "[true,false,true,false,2,9,16]");
}

#[test]
fn matrix_arrays_keep_aliases() {
    let script = r"
export function init() {
    return JSON.stringify([Mat3, Mat4].map(Type => {
        const a = Type.identity(), elements = a.m.slice();
        a.m = elements;
        elements[0] = 2;
        const b = a.copy();
        a.m[0] = 3;
        return [a.m === elements, elements[0], b.m[0], b.m !== a.m, b instanceof Type];
    }));
}";
    assert_eq!(script_text(script), "[[true,3,2,true,true],[true,3,2,true,true]]");
}

#[test]
fn matrix_arithmetic_preserves_inputs() {
    let script = r"
export function init() {
    return JSON.stringify([Mat3, Mat4].map(Type => {
        const a = Type.identity(), b = Type.identity();
        const sum = a.add(b), difference = a.subtract(b), product = a.multiply(3);
        return [sum.m[0], difference.m[0], product.m[0], a.m[0], b.m[0], sum !== a && sum !== b];
    }));
}";
    assert_eq!(script_text(script), "[[2,0,3,1,1,true],[2,0,3,1,1,true]]");
}

#[test]
fn matrix_products_keep_order() {
    let script = r"
export function init() {
    const a = Mat3.fromTranslation(new Vec2(3,4)), b = Mat4.fromTranslation(new Vec3(3,4,5));
    return JSON.stringify([a.scale(new Vec2(2,3)).transformPoint(new Vec2(1,2)).toString(), b.scale(new Vec3(2,3,4)).transformPoint(new Vec3(1,2,3)).toString()]);
}";
    assert_eq!(script_text(script), r#"["5 10","5 10 17"]"#);
}

#[test]
fn matrix_translation_mutates_receiver() {
    let script = r"
export function init() {
    const a = Mat3.fromTranslation(new Vec2(3,4)), b = Mat4.fromTranslation(new Vec3(3,4,5));
    const before = [a.translation().toString(), b.translation().toString()];
    const same = [a.translation(new Vec2(7,8)) === a, b.translation(new Vec3(7,8,9)) === b];
    return JSON.stringify([...before, ...same, a.translation().toString(), b.translation().toString()]);
}";
    assert_eq!(script_text(script), r#"["3 4","3 4 5",true,true,"7 8","7 8 9"]"#);
}

#[test]
fn matrix_vectors_keep_dimensions() {
    let script = r"
export function init() {
    const a = Mat3.fromTranslation(new Vec2(3,4)).multiply(new Vec3(1,2,2));
    const b = Mat4.fromTranslation(new Vec2(3,4)).multiply(new Vec4(1,2,3,2));
    return JSON.stringify([a instanceof Vec3, a.toString(), b instanceof Vec4, b.toString()]);
}";
    assert_eq!(script_text(script), r#"[true,"7 10 2",true,"7 10 3 2"]"#);
}

#[test]
fn matrix_points_ignore_projection() {
    let script = r"
export function init() {
    const a = Mat3.fromTranslation(new Vec2(2,3)), b = Mat4.fromTranslation(new Vec3(2,3,4));
    a.m[2] = 4; a.m[8] = 0;
    b.m[3] = 4; b.m[15] = 0;
    return JSON.stringify([a.transformPoint(new Vec2(1,2)).toString(), b.transformPoint(new Vec3(1,2,3)).toString(), a.transformDirection(new Vec2(1,2)).toString(), b.transformDirection(new Vec3(1,2,3)).toString()]);
}";
    assert_eq!(script_text(script), r#"["3 5","3 5 7","1 2","1 2 3"]"#);
}

#[test]
fn matrix_determinant_matches_native() {
    let script = r"
export function init() {
    const a = new Mat3();
    a.m = [1,0,0,2,1,0,3,4,1];
    return String(a.determinant());
}";
    assert_eq!(script_text(script), "1");
}

#[test]
fn matrix_inverse_matches_native() {
    let script = r"
export function init() {
    const a = new Mat3();
    a.m = [1,0,0,2,1,0,3,4,1];
    return JSON.stringify(a.inverse().m);
}";
    assert_eq!(script_text(script), "[1,0,0,-2,1,0,5,-4,1]");
}

#[test]
fn matrix_transpose_matches_native() {
    let script = r"
export function init() {
    const a = new Mat3();
    a.m = [1,0,0,2,1,0,3,4,1];
    return JSON.stringify(a.transpose().m);
}";
    assert_eq!(script_text(script), "[1,2,3,0,1,4,0,0,1]");
}

#[test]
fn singular_inverse_returns_nan() {
    let script = r"
export function init() {
    return JSON.stringify([Mat3,Mat4].map(Type => Type.fromScale(0).inverse().m.every(Number.isNaN)));
}";
    assert_eq!(script_text(script), "[true,true]");
}

#[test]
fn matrix_angles_match_bytes() {
    let cases = [
        (
            "Mat3",
            "[30,-30,0,90].map(angle => Mat3.fromRotation(angle).angle())",
            "[[1,0,0,0,0,0,94,64],[1,0,0,0,0,0,78,64],[0,0,0,0,0,128,86,64],[0,0,0,0,0,128,102,64]]",
        ),
        (
            "Mat4",
            "(euler => [euler.x,euler.y,euler.z])(Mat4.compose(new Vec3(3,4,5),new Vec3(30,-40,70),new Vec3(2,3,4)).extractEuler())",
            "[[43,213,161,72,103,203,66,64],[207,14,196,81,226,247,55,192],[0,0,0,0,0,128,81,64]]",
        ),
    ];
    for (name, angles, expected) in cases {
        let script = format!(
            r"
export function init() {{
    return JSON.stringify(({angles}).map(number => {{
        const bytes = new ArrayBuffer(8);
        new DataView(bytes).setFloat64(0, number, true);
        return Array.from(new Uint8Array(bytes));
    }}));
}}"
        );
        assert_eq!(script_text(&script), expected, "{name}");
    }
}

#[test]
fn planar_extraction_keeps_shear() {
    let script = r"
export function init() {
    const a = new Mat3();
    a.m = [1,0,0,2,-3,0,0,0,1];
    return String(a.angle());
}";
    assert_eq!(script_text(script), "90");
}

#[test]
fn spatial_extraction_keeps_shear() {
    let script = r"
export function init() {
    const b = new Mat4();
    b.m = [1,0,0,0,0,1,1,0,0,0,1,0,0,0,0,1];
    return b.extractEuler().toString();
}";
    assert_eq!(script_text(script), "0 0 0");
}

#[test]
fn matrix_decomposition_removes_scale() {
    let script = r"
export function init() {
    const a = Mat3.compose(new Vec2(3,4),30,new Vec2(2,3)).decompose();
    const b = Mat4.compose(new Vec3(3,4,5),new Vec3(30,-40,70),new Vec3(2,3,4)).decompose();
    const rounded = vector => [vector.x,vector.y,vector.z].filter(value => value !== undefined).map(Math.round);
    return JSON.stringify([rounded(a.translation), Math.round(a.rotation), rounded(a.scale), rounded(b.translation), rounded(b.rotation), rounded(b.scale)]);
}";
    assert_eq!(script_text(script), "[[3,4],30,[2,3],[3,4,5],[30,-40,70],[2,3,4]]");
}

#[test]
fn matrix_normals_keep_shear() {
    let script = r"
export function init() {
    const a = new Mat4();
    a.m[4] = 2;
    const normal = a.normalMatrix();
    return JSON.stringify([normal instanceof Mat3, normal.m]);
}";
    assert_eq!(script_text(script), "[true,[1,-2,0,0,1,0,0,0,1]]");
}

#[test]
fn matrix_views_transform_eye() {
    let script = r"
export function init() {
    const eye = new Vec3(1,2,3), view = Mat4.lookAt(eye,new Vec3(1,2,2),new Vec3(0,1,0));
    return JSON.stringify([view.transformPoint(eye).toString(),view.transformPoint(new Vec3(0)).toString()]);
}";
    assert_eq!(script_text(script), r#"["0 0 0","-1 -2 -3"]"#);
}

#[test]
fn matrix_equality_matches_boundary() {
    let script = r"
export function init() {
    return JSON.stringify([Mat3,Mat4].map(Type => [0.000005,0.00001].map(delta => {
        const a = new Type(), b = a.copy();
        b.m[1] = delta;
        return a.equals(b);
    })));
}";
    assert_eq!(script_text(script), "[[true,false],[true,false]]");
}

#[test]
fn matrix_strings_flatten_columns() {
    let script = r"
export function init() {
    return JSON.stringify([Mat3.fromTranslation(new Vec2(3,4)).toString(),Mat4.fromTranslation(new Vec3(3,4,5)).toString()]);
}";
    assert_eq!(script_text(script), r#"["1 0 0 0 1 0 3 4 1","1 0 0 0 0 1 0 0 0 0 1 0 3 4 5 1"]"#);
}

#[test]
fn singular_inverse_keeps_infinities() {
    let script = r"
export function init() {
    return JSON.stringify([
        [Mat3,[0,0,0,0,2,0,0,0,1],[0,4,8]],
        [Mat4,[0,0,0,0,0,2,0,0,0,0,3,0,0,0,0,1],[0,5,15]],
        [Mat3,[1,2,0,2,4,0,0,0,1],[0,1,8]],
        [Mat4,[1,2,0,0,2,4,0,0,0,0,3,0,0,0,0,1],[0,1,15]]
    ].map(([Type,elements,slots]) => {
        const matrix = new Type();
        matrix.m = elements;
        const inverse = matrix.inverse();
        return slots.map(slot => String(inverse.m[slot]));
    }));
}";
    assert_eq!(
        script_text(script),
        r#"[["Infinity","NaN","NaN"],["Infinity","NaN","NaN"],["Infinity","-Infinity","NaN"],["Infinity","-Infinity","NaN"]]"#
    );
}

#[test]
fn vector_equality_matches_offsets() {
    let script = r"
export function init() {
    return JSON.stringify([[Vec2,'y'],[Vec3,'z'],[Vec4,'w']].map(([Type,last]) => ['x',last].map(component => [0,0.000005,0.00001,0.00005].map(delta => {
        const a = new Type(0), b = new Type(0);
        b[component] = delta;
        return a.equals(b);
    }))));
}";
    assert_eq!(
        script_text(script),
        "[[[true,true,false,false],[true,true,false,false]],[[true,true,false,false],[true,true,false,false]],[[true,true,false,false],[true,true,false,false]]]"
    );
}

#[test]
fn vector_assignment_preserves_delta() {
    let script = r"
export function init() {
    const layer = thisScene.getLayer('target');
    layer.origin = new Vec3(0.000005,0,0);
    return String(layer.origin.x);
}";
    let mut scene = json!({"objects":[
        {"id":1,"text":{"value":"","script":script}},
        {"id":2,"name":"target","origin":"0 0 0"}
    ]});
    let host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["text"], "0.000004999999873689376");
    assert_eq!(scene["objects"][1]["origin"], "0.000004999999873689376 0 0");
}

#[test]
fn particle_argument_types() {
    let cases = [
        ("", Some(1)),
        ("undefined", Some(1)),
        ("null", Some(1)),
        ("false", Some(1)),
        ("true", Some(1)),
        ("0", Some(1)),
        ("-0", Some(1)),
        ("-0.5", Some(1)),
        ("0.5", Some(1)),
        ("2.9", Some(1)),
        ("3", Some(3)),
        ("3.0", Some(3)),
        ("3.000000000000001", Some(1)),
        ("2147483647", Some(2_147_483_647)),
        ("2147483648", Some(1)),
        ("-2147483648", None),
        ("-2147483649", Some(1)),
        ("4294967296", Some(1)),
        ("-1", None),
        ("NaN", Some(1)),
        ("Infinity", Some(1)),
        ("'2'", Some(1)),
        ("2n", Some(1)),
        ("Symbol('count')", Some(1)),
        ("[]", Some(1)),
        ("[2]", Some(1)),
        ("({valueOf(){throw Error('coerced');}})", Some(1)),
    ];
    for (argument, count) in cases {
        let script = format!(
            "export function init(){{thisLayer.pause();thisLayer.emitParticles({argument});if(thisLayer.isPlaying())throw Error('resumed');}}"
        );
        let mut scene = json!({"objects":[{"id":1,"particle":"test.json","visible":{"value":true,"script":script}}]});
        let mut host =
            SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        let mut expected = vec![ScriptCommand::Particle { id: "1".into(), op: ParticleOp::Pause }];
        if let Some(count) = count {
            expected.push(ScriptCommand::Particle { id: "1".into(), op: ParticleOp::Emit(count) });
        }
        assert_eq!(host.take_commands(), expected, "{argument}");
        assert!(host.diagnostics.is_empty(), "{argument}: {:?}", host.diagnostics);
    }
}

#[test]
fn particle_playback_commands() {
    let script = r"
export function init() {
    const particle = thisScene.getLayer('particle');
    particle.stop();
    const stopped = particle.isPlaying();
    particle.emitParticles(0.5);
    const fractional = particle.isPlaying();
    particle.emitParticles();
    const emitted = particle.isPlaying();
    particle.pause();
    const paused = particle.isPlaying();
    particle.stop();
    particle.emitParticles(1);
    const stoppedPaused = particle.isPlaying();
    particle.play();
    return [stopped, fractional, emitted, paused, stoppedPaused, particle.isPlaying()].join(',');
}

export function update() {
    if (engine.runtime === 0) return;
    return String(thisScene.getLayer('particle').isPlaying());
}";
    let mut scene = json!({"objects":[
        {"id":1,"name":"particle","particle":"particles/test.json"},
        {"id":2,"text":{"value":"","script":script}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert_eq!(host.scene["objects"][1]["text"], "false,true,true,false,false,true");
    assert_eq!(
        host.take_commands(),
        vec![
            ScriptCommand::Particle { id: "1".into(), op: ParticleOp::Stop },
            ScriptCommand::Particle { id: "1".into(), op: ParticleOp::Emit(1) },
            ScriptCommand::Particle { id: "1".into(), op: ParticleOp::Emit(1) },
            ScriptCommand::Particle { id: "1".into(), op: ParticleOp::Pause },
            ScriptCommand::Particle { id: "1".into(), op: ParticleOp::Stop },
            ScriptCommand::Particle { id: "1".into(), op: ParticleOp::Emit(1) },
            ScriptCommand::Particle { id: "1".into(), op: ParticleOp::Play },
        ]
    );
    host.set_particle_states(&[("1".into(), false)]);
    host.tick(0.1, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][1]["text"], "false");
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn particle_instance_size_commands() {
    let script = r"
export function init() {
    const layer = thisScene.getLayer('particle');
    const instance = layer.instance;
    const desc = Object.getOwnPropertyDescriptor(layer, 'instance');
    if (desc.value !== instance || layer.instance !== instance || !desc.enumerable || !desc.configurable || desc.writable) throw Error('instance descriptor');
    const initial = instance.size;
    layer.instance.size = 0.5;
    const half = layer.instance.size;
    layer.instance.size = Infinity;
    layer.instance.size = 2;
    return [initial, half, layer.instance.size].join(',');
}";
    let mut scene = json!({"objects":[
        {"id":1,"name":"particle","particle":"particles/test.json","instanceoverride":{"size":2}},
        {"id":2,"text":{"value":"","script":script}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][1]["text"], "2,0.5,2");
    assert_eq!(
        host.take_commands(),
        vec![
            ScriptCommand::Particle { id: "1".into(), op: ParticleOp::Size(0.5) },
            ScriptCommand::Particle { id: "1".into(), op: ParticleOp::Size(2.0) },
        ]
    );
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn particle_instance_factors() {
    let script = r"
export function init() {
    const instance = thisScene.getLayer('particle').instance;
    const initial = [instance.speed, instance.lifetime, instance.colorn.toString(), instance.controlpoint0.toString()].join('|');
    instance.speed = 120;
    instance.lifetime = 0.5;
    instance.colorn = new Vec3(0.5, 1, 2);
    instance.controlpoint7 = new Vec3(4, 5, 6);
    instance.colorn = new Vec3(NaN, 0, 0);
    instance.controlpoint0 = new Vec3(Infinity, 0, 0);
    return [initial, instance.speed, instance.lifetime, instance.colorn.toString(), instance.controlpoint7.toString(), typeof instance.controlpoint8].join('|');
}";
    let mut scene = json!({"objects":[
        {"id":1,"name":"particle","particle":"particles/test.json","instanceoverride":{
            "speed":2,"lifetime":3,"colorn":"0 1 1","controlpoint0":"1 2 3"
        }},
        {"id":2,"text":{"value":"","script":script}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][1]["text"], "2|3|0 1 1|1 2 3|100|0.5|0.5 1 2|4 5 6|undefined");
    assert_eq!(
        host.take_commands(),
        vec![
            ScriptCommand::Particle { id: "1".into(), op: ParticleOp::Speed(100.0) },
            ScriptCommand::Particle { id: "1".into(), op: ParticleOp::Lifetime(0.5) },
            ScriptCommand::Particle { id: "1".into(), op: ParticleOp::Color([0.5, 1.0, 2.0]) },
            ScriptCommand::Particle {
                id: "1".into(),
                op: ParticleOp::ControlPoint(7, [4.0, 5.0, 6.0])
            },
        ]
    );
}

#[test]
fn sound_getter_tracks_receiver() {
    let script = r"
export function init() { return String(thisScene.getLayer('tone').isPlaying()); }
export function update() { return String(thisScene.getLayer('tone').isPlaying()); }
";
    let mut scene = json!({"objects":[
        {"id":1,"name":"tone","sound":"sounds/tone.ogg"},
        {"id":2,"text":{"value":"","script":script}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert_eq!(host.scene["objects"][1]["text"], "true");
    for (time, states, expected) in [
        (0.1, vec![("1".into(), false)], "false"),
        (0.2, vec![("1".into(), true)], "true"),
        (0.3, Vec::new(), "false"),
    ] {
        host.set_sound_states(&states);
        host.tick(time, 0.1, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][1]["text"], expected);
    }
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn camera_transform_contract() {
    let mut scene = json!({"objects":[{"id":1,"visible":{"value":true,"script":r"
export function init() {
    const initial = thisScene.getCameraTransforms();
    initial.eye.x = 9;
    if (thisScene.getCameraTransforms().eye.x !== 2) throw Error('camera aliases');
    if (!thisScene.setCameraTransforms({eye:new Vec3(32,16,0),center:new Vec3(8,4,-1),up:new Vec3(0,0.8,0.6),zoom:1.25})) throw Error('camera rejected');
    if (thisScene.setCameraTransforms(null) || thisScene.setCameraTransforms(undefined)) throw Error('invalid camera');
    if (!thisScene.setCameraTransforms({})) throw Error('empty camera');
    if (thisScene.getCameraTransforms().up.y !== Math.fround(0.8)) throw Error('camera precision');
    return true;
}"}}]});
    let mut host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let camera = host.take_camera().unwrap();
    let pose = camera.pose.unwrap();
    assert_eq!(pose.eye, [32.0, 16.0, 0.0]);
    assert_eq!(pose.center, [8.0, 4.0, -1.0]);
    assert_eq!(pose.up, [0.0, 0.8, 0.6]);
    assert_eq!(camera.zoom, Some(1.25));
    assert!(host.take_camera().is_none());
}

#[test]
fn camera_scene_zoom() {
    let mut scene = json!({"general":{"zoom":1.5},"objects":[{"id":1,"visible":{"value":true,"script":r"
export function init() {
    if (thisScene.getCameraTransforms().zoom !== 1.5) throw Error('scene zoom');
    return true;
}"}}]});
    let mut host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert!(host.take_camera().is_none_or(|camera| camera.zoom.is_none()));
}

#[test]
fn model_handle_contract() {
    let mut scene = json!({"objects":[{"id":1,"visible":{"value":true,"script":r"
export function init() {
    if (IModelData.POSITION !== 'position' || IModelData.NORMAL !== 'normal' || IModelData.UV !== 'uv') throw Error('model formats');
    if (thisScene.createModelData(null) !== null || thisScene.destroyModelData(null) !== undefined) throw Error('null model');
    for (const [config,message] of [[{shapes:[]},'Shapes missing.'],[{shapes:[{}]},'Vertex buffer missing.']]) {
        try { thisScene.createModelData(config); throw Error('accepted invalid model'); }
        catch(error) { if (error.name !== 'SyntaxError' || error.message !== message) throw error; }
    }
    const model = thisScene.createModelData({shapes:[{vertexBuffer:new Float32Array(24),vertexFormat:[IModelData.POSITION,IModelData.NORMAL,IModelData.UV],material:'materials/white.json'}]});
    if (!model || typeof model !== 'object') throw Error('model handle');
    thisScene.destroyModelData(model);
    thisScene.destroyModelData(model);
    return true;
}"}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert!(!host.has_models());
}

#[test]
fn flat_camera_starts_zeroed() {
    let mut scene = json!({"general":{"orthogonalprojection":{"width":64,"height":64}},
        "camera":{"eye":"300 100 5","center":"300 100 4","up":"0 1 0"},
        "objects":[{"id":1,"visible":{"value":true,"script":r"
export function update(value) {
    const camera = thisScene.getCameraTransforms();
    if (camera.eye.x !== 0 || camera.eye.y !== 0 || camera.center.x !== 0) throw Error('flat camera');
    return value;
}"}}]});
    let mut host = image_host(&mut scene);
    host.tick(0.1, 0.1, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert!(!host.take_camera().unwrap().scripted);
}

#[test]
fn camera_startup_phase() {
    let mut scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"objects":[{"id":1,"visible":{"value":true,"script":r"
let started = false;
export function init() {
    if (thisScene.getCameraTransforms().eye.x !== 2) throw Error('initial camera');
    thisScene.setCameraTransforms({eye:new Vec3(7,8,9),zoom:1.25});
    return true;
}
export function update(value) {
    const camera = thisScene.getCameraTransforms();
    if (!started) {
        if (camera.eye.z !== -4 || camera.zoom !== 1.25) throw Error('authored camera');
        thisScene.setCameraTransforms({eye:new Vec3(0.8,0.3,-4)});
        started = true;
    } else if (camera.eye.x !== Math.fround(0.8)) throw Error('camera reset');
    return value;
}"}}]});
    let mut host = image_host(&mut scene);
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    let camera = host.take_camera().unwrap();
    assert!(camera.initialized);
    assert!(camera.scripted);
    assert_eq!(camera.pose.unwrap().eye, [0.8, 0.3, -4.0]);
    assert_eq!(camera.zoom, Some(1.25));
    host.tick(0.1, 0.1, [0.5; 2]).unwrap();
    assert!(host.take_camera().is_none());
    host.tick(0.2, 0.1, [0.5; 2]).unwrap();
    assert!(host.take_camera().is_none());
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn native_api_contracts() {
    for (bytes, expected) in [
        (
            include_bytes!("../fixtures/a01/camera-contract.pkg").as_slice(),
            include_str!("../fixtures/a01/camera-contract.json"),
        ),
        (
            include_bytes!("../fixtures/a01/modeldata-contract.pkg").as_slice(),
            include_str!("../fixtures/a01/modeldata-contract.json"),
        ),
        (
            include_bytes!("../fixtures/a01/modeldata-valid-contract.pkg").as_slice(),
            include_str!("../fixtures/a01/modeldata-valid-contract.json"),
        ),
    ] {
        let package = crate::pkg::Package::parse(bytes.to_vec()).unwrap();
        let mut scene: serde_json::Value =
            serde_json::from_slice(package.find("scene.json").unwrap()).unwrap();
        let script = scene["objects"][0]["alpha"]["script"].as_str().unwrap();
        let sink = "console.log('SKWD_A01:' + JSON.stringify(result));";
        assert_eq!(script.matches(sink).count(), 1);
        scene["objects"][0]["alpha"]["script"] =
            script.replace(sink, "thisLayer.name = JSON.stringify(result);").into();
        let assets = crate::effects::Assets::discover(Some("/nonexistent"));
        let images = crate::model::ImageResources::new(&package, &assets);
        let host = SceneScripts::load_with_storage(
            &mut scene,
            &Properties::new(),
            &json!({}),
            super::Load { images: Some(images), ..super::Load::default() },
        )
        .unwrap()
        .unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        let actual: serde_json::Value =
            serde_json::from_str(host.scene["objects"][0]["name"].as_str().unwrap()).unwrap();
        let expected: serde_json::Value = serde_json::from_str(expected).unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn automatic_model_rejected() {
    let mut scene = json!({"general":{"orthogonalprojection":{"auto":true}},"objects":[{"id":1,"visible":{"value":true,"script":r"
export function init() {
    const handle = thisScene.createModelData({shapes:[{vertexBuffer:new Float32Array(24),vertexFormat:[IModelData.POSITION,IModelData.NORMAL,IModelData.UV],material:'materials/white.json'}]});
    try {
        thisScene.createLayer({name:'automatic',model:handle});
        throw Error('automatic accepted');
    } catch(error) {
        if (!String(error.message).includes('procedural model automatic projection unsupported')) throw error;
    }
    thisScene.destroyModelData(handle);
    return true;
}"}}]});
    let mut host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert!(host.take_created_models().is_empty());
    assert!(!host.has_models());
}

#[test]
fn mixed_text_rejected() {
    let script = r"
let checked = false;
function check() {
    checked = true;
    const before = thisScene.enumerateLayers().map(layer => layer.name);
    let rejected = 0;
    for (const media of [{shape:'quad'}, {particle:'particles/missing.json'}, {sound:'sound.wav'}]) {
        const config = Object.assign({name:'mixed',text:'A',font:'fonts/missing.ttf'},media);
        try { thisScene.createLayer(config); } catch (error) {
            if (String(error).includes('dynamic text competing media are unsupported')) rejected++;
        }
    }
    thisLayer.text = JSON.stringify([rejected,thisScene.getLayerCount(),
        before,thisScene.enumerateLayers().map(layer => layer.name),thisScene.getLayer('mixed')]);
}
export function init() {
    thisScene.createLayer({name:'seed',image:'models/white.json',text:'A',font:'fonts/missing.ttf'});
    if (RUN_IN_INIT) check();
}
export function update(value) {
    if (!checked && engine.runtime>=1) check();
    return thisLayer.text;
}";
    for initial in [true, false] {
        let script = script.replace("RUN_IN_INIT", if initial { "true" } else { "false" });
        let mut scene =
            json!({"objects":[{"id":1,"name":"observer","text":{"value":"","script":script}}]});
        let mut host = image_host(&mut scene);
        if !initial {
            host.tick(1.0, 1.0, [0.0; 2]).unwrap();
        }
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(host.scene["objects"].as_array().unwrap().len(), 2);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                host.scene["objects"][0]["text"].as_str().unwrap()
            )
            .unwrap(),
            json!([3, 2, ["observer", "seed"], ["observer", "seed"], null])
        );
        assert_eq!(host.created_layer_slots(), [1]);
        let queued = host.take_created_images();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].0, 1);
        assert!(!queued[0].1.layer.is_text());
        assert_eq!(
            host.image_resources().unwrap().payload_bytes(),
            queued[0].1.layer.texture.payload_bytes()
        );
        assert!(host.take_created_images().is_empty());
    }
}

#[test]
fn material_property_roundtrip() {
    let scene = json!({"objects":[{"id":1,"name":"target","image":"models/white.json",
        "effects":[{"file":"effects/probe/effect.json","name":"probe","visible":true,
            "passes":[{"constantshadervalues":{"u_probeLevel":0.25}},{},
                {"constantshadervalues":{"u_probeLevel":0.25}}]}],
        "visible":{"value":true,"script":r"
let effect, material, tick = 0;
export function init() {
    effect = thisLayer.getEffect(0);
    if (effect.getMaterialCount() !== 3) throw Error('material count');
    material = effect.getMaterial(0);
    if (!material || material.probeLevel !== 0.25) throw Error('material default');
    if (effect.getMaterial(1) !== null || effect.getMaterial(3) !== null) throw Error('material bounds');
    const second = effect.getMaterial(2);
    second.probeLevel = 0.6;
    if (second.probeLevel !== Math.fround(0.6) || material.probeLevel !== 0.25) throw Error('material index');
    effect.getMaterial().probeLevel = 0.4;
    if (effect.getMaterial().probeLevel !== Math.fround(0.4)) throw Error('effect material cache');
    return true;
}
export function update() {
    tick++;
    if (tick === 2) effect.setMaterialProperty('probeLevel', 0.75);
    if (tick === 3) material.probeLevel = 0.5;
    if (tick === 4) effect.setMaterialProperty('probeLevel', 0.25);
    const expected = tick === 2 ? 0.75 : tick === 3 ? 0.5 : 0.25;
    if (tick > 1 && material.probeLevel !== expected) throw Error('material write');
    return true;
}"}}]});
    for authored in [true, false] {
        let mut scene = scene.clone();
        if !authored {
            scene["objects"][0]["effects"][0].as_object_mut().unwrap().remove("passes");
        }
        let source = scene.to_string();
        let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", source.as_bytes()),
        ("models/white.json", br#"{"material":"materials/white.json"}"#),
        ("materials/white.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/probe/effect.json", br#"{"passes":[{"material":"materials/probe.json"},{"command":"swap"},{"material":"materials/probe.json"}]}"#),
        ("materials/probe.json", br#"{"passes":[{"shader":"probe"}]}"#),
        (
            "shaders/probe.vert",
            b"attribute vec3 a_Position; void main(){gl_Position=vec4(a_Position,1.0);}",
        ),
        (
            "shaders/probe.frag",
            br#"uniform float u_probeLevel; // {"material":"probeLevel","default":0.25}
void main(){gl_FragColor=vec4(u_probeLevel,0.0,0.0,1.0);}"#,
        ),
    ]))
    .unwrap();
        let assets = crate::effects::Assets::discover(Some(""));
        let images = crate::model::ImageResources::new(&package, &assets);
        let mut host = SceneScripts::load_with_storage(
            &mut scene,
            &Properties::new(),
            &json!({}),
            super::Load { images: Some(images), ..super::Load::default() },
        )
        .unwrap()
        .unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(
            host.take_commands(),
            [(2, 0.6), (0, 0.4)].map(|(pass, value)| ScriptCommand::Material {
                id: "1".into(),
                effect: (0, pass),
                name: "u_probeLevel".into(),
                values: vec![value],
            })
        );
        host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
        assert!(host.take_commands().is_empty());
        for (tick, (value, passes)) in
            [(0.75, vec![0, 2]), (0.5, vec![0]), (0.25, vec![0, 2])].into_iter().enumerate()
        {
            host.tick((tick + 1) as f32, 0.016, [0.5; 2]).unwrap();
            assert_eq!(
                host.take_commands(),
                passes
                    .into_iter()
                    .map(|pass| ScriptCommand::Material {
                        id: "1".into(),
                        effect: (0, pass),
                        name: "u_probeLevel".into(),
                        values: vec![value],
                    })
                    .collect::<Vec<_>>()
            );
        }
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    }
}

#[test]
fn material_metadata_wire() {
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("effects/probe/effect.json", br#"{"fbos":[{"name":"_rt_A"},{"name":"_rt_B"}],"functions":{"clearProbe":{"action":"clear","fbos":["_rt_A","_rt_B"]}},"passes":[{"material":"materials/probe.json"},{"command":"swap","source":"_rt_A","target":"_rt_B"},{"material":"materials/probe.json"}]}"#),
        ("materials/probe.json", br#"{"passes":[{"shader":"probe","textures":["missing-texture"]}]}"#),
        ("shaders/probe.frag", br#"uniform sampler2D g_Texture0;
uniform float u_probeLevel; // {"material":"probeLevel","default":0.25}
void main(){gl_FragColor=texture2D(g_Texture0,vec2(0.0))*vec4(u_probeLevel);}"#),
    ])).unwrap();
    let assets = crate::effects::Assets::discover(Some(""));
    let images = crate::model::ImageResources::new(&package, &assets);
    let object = json!({"effects":[{"file":"effects/probe/effect.json"}]});
    let info = images.effect_materials(&object).unwrap();
    let reply: Result<_, String> = Ok(info);
    assert_eq!(
        serde_json::to_value(reply).unwrap(),
        json!({"Ok":{
            "count":3,"materials":[
                {"source":0,"uniforms":{"probeLevel":{"uniform":"u_probeLevel","value":[0.25],"width":1}}},
                {"source":2,"uniforms":{"probeLevel":{"uniform":"u_probeLevel","value":[0.25],"width":1}}}
            ],
            "clear_functions":{"clearProbe":["_rt_A","_rt_B"]}
        }})
    );
    let invalid: Result<crate::model::EffectMaterialInfo, String> = Err("missing".into());
    assert_eq!(serde_json::to_value(invalid).unwrap(), json!({"Err":"missing"}));
}

#[test]
fn selected_material_metadata() {
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("effects/probe.json", br#"{"passes":[{"material":"materials/probe.json"}]}"#),
        (
            "materials/probe.json",
            br#"{"passes":[{"shader":{"user":"style"},"constantshadervalues":{"level":0.6}}]}"#,
        ),
        ("shaders/first.frag", br#"uniform float u_Level; // {"material":"level","default":0.25}"#),
        (
            "shaders/second.frag",
            br#"uniform float u_Other; // {"material":"other","default":0.75}"#,
        ),
    ]))
    .unwrap();
    let project = json!({"general":{"properties":{"style":{"type":"customshader","value":0,
        "options":[{"value":0,"shader":"first"},{"value":1,"shader":"second"}]}}}});
    for (choice, name, uniform, value) in
        [(0.0, "level", "u_Level", 0.6), (1.0, "other", "u_Other", 0.75)]
    {
        let assets = crate::effects::Assets::discover(Some(""))
            .with_properties(Properties::from([("style".into(), vec![choice])]))
            .with_custom_shaders(&project)
            .unwrap();
        let images = crate::model::ImageResources::new(&package, &assets);
        let info =
            images.effect_materials(&json!({"effects":[{"file":"effects/probe.json"}]})).unwrap();
        let info = serde_json::to_value(info).unwrap();
        assert_eq!(info["materials"][0]["uniforms"][name]["uniform"], uniform);
        let actual = info["materials"][0]["uniforms"][name]["value"][0].as_f64().unwrap();
        assert!((actual - value).abs() < f64::from(f32::EPSILON));
    }
}

#[test]
fn material_getter_values() {
    for (base, instance, level, color) in [
        (json!({}), json!({}), 0.25, 0.1),
        (json!({"u_Level":0.4,"u_Color":"0.4 0.5 0.6"}), json!({}), 0.25, 0.1),
        (json!({"level":0.6,"color":"0.7 0.8 0.9"}), json!({}), 0.6, 0.7),
        (json!({}), json!({"u_Level":0.8,"u_Color":"0.2 0.4 0.6"}), 0.25, 0.1),
        (json!({"level":0.6}), json!({"level":0.9,"color":"0.3 0.6 0.9"}), 0.9, 0.3),
        (json!({}), json!({"level":"0.9 1 2","color":"0.3 0.6 0.9 1 2"}), 0.9, 0.3),
    ] {
        let source = format!(
            r"
export function init() {{
    const material = thisLayer.getEffect(0).getMaterial(0);
    if (material.level !== Math.fround({level})) throw Error('scalar default');
    const color = material.color;
    if (!(color instanceof Vec3) || color.x !== Math.fround({color})) throw Error('vector default '+JSON.stringify([color, {color}]));
    color.x = 0.99;
    if (material.color.x !== Math.fround({color})) throw Error('vector copy');
    const assigned = new Vec3(0.2, 0.5, 0.8);
    material.color = assigned;
    assigned.x = 0.99;
    if (material.color.x !== Math.fround(0.2)) throw Error('vector assignment');
    return true;
}}"
        );
        let mut scene = json!({"objects":[{"id":1,"image":"models/white.json",
            "effects":[{"file":"effects/probe.json","passes":[{"constantshadervalues":instance}]}],
            "visible":{"value":true,"script":source}}]});
        let material =
            json!({"passes":[{"shader":"probe","constantshadervalues":base}]}).to_string();
        let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
            ("effects/probe.json", br#"{"passes":[{"material":"materials/probe.json"}]}"#),
            ("materials/probe.json", material.as_bytes()),
            (
                "shaders/probe.frag",
                br#"uniform float u_Level; // {"material":"level","default":0.25}
uniform vec3 u_Color; // {"material":"color","default":"0.1 0.2 0.3"}"#,
            ),
        ]))
        .unwrap();
        let assets = crate::effects::Assets::discover(Some(""));
        let images = crate::model::ImageResources::new(&package, &assets);
        let mut host = SceneScripts::load_with_storage(
            &mut scene,
            &Properties::new(),
            &json!({}),
            super::Load { images: Some(images), ..super::Load::default() },
        )
        .unwrap()
        .unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(
            host.take_commands(),
            [ScriptCommand::Material {
                id: "1".into(),
                effect: (0, 0),
                name: "u_Color".into(),
                values: vec![0.2, 0.5, 0.8],
            }]
        );
    }
}

#[test]
fn registered_asset_scope() {
    let script = r"
const first = engine.registerAsset('models/white.json');
const duplicate = engine.registerAsset('models/white.json');
const precached = engine.registerAsset('models/white.json',true);
const material = engine.registerAsset('materials/white.json',true);
export function init() { return JSON.stringify([first,duplicate,precached,material]); }
";
    let mut scene = json!({"objects":[
        {"id":1,"text":{"value":"","script":script}},
        {"id":2,"text":{"value":"","script":script}}
    ]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    for object in scene["objects"].as_array().unwrap() {
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(object["text"].as_str().unwrap()).unwrap(),
            json!(["models/white.json", null, null, "materials/white.json"])
        );
    }
}

#[test]
fn registered_asset_callback() {
    for event in ["init", "update"] {
        let script =
            format!("export function {event}() {{ engine.registerAsset('models/white.json'); }}");
        let mut scene = json!({"objects":[{"id":1,"text":{"value":"unchanged","script":script}}]});
        let mut host = image_host(&mut scene);
        host.tick(1.0, 0.1, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["text"], "unchanged");
        assert!(
            host.diagnostics
                .iter()
                .any(|error| error.contains("registerAsset can only be called from global scope"))
        );
        assert!(host.take_created_images().is_empty());
    }
}

#[test]
fn dynamic_slots_reuse() {
    let script = r"
let first, count = 0;
export function update() {
    const layer = thisScene.createLayer({image:'models/white.json',origin:'10 20 0'});
    if (!first) { first = layer; first.custom = 7; }
    else if (first === layer || thisScene.destroyLayer(first) || first.custom !== 7) throw Error('retired identity');
    thisScene.destroyLayer(layer);
    return String(++count);
}
";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host = image_host(&mut scene);
    for step in 0..=crate::model::MAX_SCENE_OBJECTS {
        host.tick(f32::from(u16::try_from(step).unwrap()), 0.016, [0.5; 2]).unwrap();
        assert_eq!(host.take_created_images().len(), 1);
        let retired = host.take_destroyed_images();
        host.acknowledge_destroyed(&retired);
        assert!(host.scene["objects"].as_array().unwrap().len() <= 3);
        assert!(host.created_layer_slots().len() <= 1);
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    }
    assert_eq!(host.scene["objects"][0]["text"], (crate::model::MAX_SCENE_OBJECTS + 1).to_string());
}

#[test]
fn discarded_layer_frees_slot() {
    let script = r"
let layer;
export function update() {
    if (!layer) layer = thisScene.createLayer({image:'models/white.json'});
    return String(thisScene.getLayerCount());
}
";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host = image_host(&mut scene);
    host.tick(0.0, 0.016, [0.5; 2]).unwrap();
    let created = host.take_created_images();
    assert_eq!(created[0].0, 1);
    let held = host.image_resources().unwrap().payload_bytes();
    host.discard_created(1);
    assert!(host.image_resources().unwrap().payload_bytes() < held);
    assert_eq!(host.take_destroyed_images(), [1]);
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    host.acknowledge_destroyed(&[1]);
    assert!(host.created_layer_slots().is_empty());
    assert_eq!(host.scene["objects"][1]["visible"], false);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn dynamic_slots_wait_cleanup() {
    let script = r"
export function update() {
    const layer = thisScene.createLayer({image:'models/white.json'});
    thisScene.destroyLayer(layer);
}
";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host = image_host(&mut scene);
    let mut retired = Vec::new();
    for step in 0..3 {
        host.tick(f32::from(u16::try_from(step).unwrap()), 0.016, [0.5; 2]).unwrap();
        assert_eq!(host.take_created_images().len(), 1);
        retired.extend(host.take_destroyed_images());
    }
    assert_eq!(host.scene["objects"].as_array().unwrap().len(), 4);
    assert_eq!(retired, [1, 2, 3]);
    assert!(host.release_slots(&[0]).is_err());
    host.acknowledge_destroyed(&retired);
    assert_eq!(host.scene["objects"][1], json!({"id":"@dynamic-1","visible":false}));
    host.tick(3.0, 0.016, [0.5; 2]).unwrap();
    let created = host.take_created_images();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].0, 1);
    assert_eq!(host.scene["objects"][1]["id"], "@dynamic-4");
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn idle_dynamic_cleanup() {
    let script = "export function init(){thisScene.destroyLayer(thisScene.createLayer({image:'models/white.json'}));}";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host = image_host(&mut scene);
    assert_eq!(host.take_created_images().len(), 1);
    let retired = host.take_destroyed_images();
    assert_eq!(retired, [1]);
    host.acknowledge_destroyed(&retired);
    assert!(host.created_layer_slots().is_empty());
    assert_eq!(host.scene["objects"][1], json!({"id":"@dynamic-1","visible":false}));
    assert!(!host.animated());
    assert!(host.diagnostics.is_empty());
}

#[test]
fn reused_slots_sort() {
    let script = r"
let first, second;
export function update() {
    if (engine.runtime === 0) {
        first = thisScene.createLayer({image:'models/white.json'});
        second = thisScene.createLayer({image:'models/white.json'});
        thisScene.sortLayer(second, 0);
        thisScene.destroyLayer(first);
    } else {
        thisScene.sortLayer(second, 1);
    }
}
";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host = image_host(&mut scene);
    host.tick(0.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.take_created_images().len(), 2);
    let retired = host.take_destroyed_images();
    host.acknowledge_destroyed(&retired);
    let commands = host.take_commands();
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, ScriptCommand::Sort { order } if order == &[2, 0]))
    );
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    let commands = host.take_commands();
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, ScriptCommand::Sort { order } if order == &[0, 2]))
    );
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn init_error_preserves_updates() {
    let script = r"
let count = 0;
export function init() { thisLayer.alpha = 0.25; throw Error('init failure'); }
export function update() {
    if (++count === 3) { thisLayer.alpha = 0.5; throw Error('update failure'); }
    thisLayer.alpha = 0.75;
    return String(count);
}
";
    let mut scene =
        json!({"objects":[{"id":1,"alpha":1,"text":{"value":"start","script":script}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    assert_eq!(scene["objects"][0]["alpha"], 0.25);
    assert_eq!(scene["objects"][0]["text"], "start");
    assert!(host.animated());
    assert_eq!(host.diagnostics.len(), 1);
    assert!(host.diagnostics[0].contains("init failure"));
    for step in 1..=2 {
        host.tick(f32::from(u16::try_from(step).unwrap()), 0.016, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["text"], step.to_string());
        assert_eq!(host.scene["objects"][0]["alpha"], 0.75);
    }
    host.tick(3.0, 0.016, [0.5; 2]).unwrap();
    host.tick(4.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "2");
    assert_eq!(host.scene["objects"][0]["alpha"], 0.5);
    assert_eq!(host.diagnostics.len(), 2);
    assert!(host.diagnostics[1].contains("update failure"));
    assert!(!host.animated());
}

#[test]
fn init_budget_stops_updates() {
    let script = "export function init(){thisLayer.alpha=0.25;while(true){}} export function update(){thisLayer.alpha=0.75;}";
    let mut scene =
        json!({"objects":[{"id":1,"alpha":1,"text":{"value":"start","script":script}}]});
    let mut host = stalling_host(&mut scene);
    host.tick(1.0, 0.016, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["alpha"], 0.25);
    assert!(!host.animated());
    assert!(!host.diagnostics.is_empty());
}

#[test]
fn init_resource_errors() {
    for failure in [
        format!("new ArrayBuffer({});", 2 * HEAP_LIMIT),
        "function recurse(){return 1+recurse();} recurse();".to_owned(),
    ] {
        let script = format!(
            "export function init(){{thisLayer.alpha=0.25;{failure}}} export function update(){{thisLayer.alpha=0.75;}}"
        );
        let mut scene =
            json!({"objects":[{"id":1,"alpha":1,"text":{"value":"start","script":script}}]});
        let mut host =
            SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
        assert_eq!(host.scene["objects"][0]["alpha"], 0.25);
        host.tick(1.0, 0.016, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["alpha"], 0.75);
        assert!(host.animated());
        assert_eq!(host.diagnostics.len(), 1);
        assert!(host.heap_bytes() < HEAP_LIMIT);
    }
}

#[test]
fn init_error_cursor() {
    let script = "export function init(){throw Error('failed');} export function cursorEnter(){thisLayer.alpha=1;}";
    let mut scene = json!({"objects":[{"id":1,"alpha":{"value":0,"script":script}}]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &json!({})).unwrap().unwrap();
    host.pointer([0.2; 2], [false; 3], vec!["1".into()]).unwrap();
    host.tick(1.0, 0.016, [0.2; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["alpha"], 1);
    assert_eq!(host.diagnostics.len(), 1);
}

#[test]
fn retired_parent_is_null() {
    let script = r"
let parent, child, step=0;
export function init() {
    if (thisLayer.getParent() !== null) throw Error('authored root');
    parent=thisScene.createLayer({image:'models/white.json'});
    child=thisScene.createLayer({image:'models/white.json'});
    if (child.getParent() !== null) throw Error('dynamic root');
    child.setParent(parent, false);
    if (child.getParent() !== parent) throw Error('attached');
    child.setParent(null);
    if (child.getParent() !== null) throw Error('manual detach');
    child.origin=new Vec3(40,20,0);
    child.setParent(parent, false);
}
export function update() {
    if (step++ === 0) thisScene.destroyLayer(parent);
    else {
        if (child.getParent() !== null) throw Error('retired parent');
        if (child.origin.x !== 40 || child.origin.y !== 20) throw Error('local origin');
        return 'detached';
    }
    return 'pending';
}
";
    let mut scene = json!({"objects":[{"id":1,"text":{"value":"","script":script}}]});
    let mut host = image_host(&mut scene);
    host.tick(0.0, 0.016, [0.5; 2]).unwrap();
    host.tick(0.016, 0.016, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "detached");
}

#[test]
fn scripted_instance_overrides() {
    let mut scene = json!({"objects":[
        {"id":4,"name":"stars","particle":"particles/stars.json","instanceoverride":{
            "lifetime":{"value":1.0,"script":"export function update(value) { return 0.25; }"},
            "colorn":{"value":"1 1 1","script":"export function update(value) { return new Vec3(0.5, 0.25, 1); }"}
        }},
        {"id":5,"alpha":{"value":1.0,"script":"export function update(value) { return thisScene.getLayer('stars').instance.lifetime; }"}}
    ]});
    let mut host = SceneScripts::load(&mut scene, &Properties::new(), &serde_json::Value::Null)
        .unwrap()
        .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    host.take_commands();
    host.tick(0.1, 0.1, [0.0, 0.0]).unwrap();
    let commands = host.take_commands();
    assert!(
        commands
            .contains(&ScriptCommand::Particle { id: "4".into(), op: ParticleOp::Lifetime(0.25) }),
        "{commands:?}"
    );
    assert!(
        commands.contains(&ScriptCommand::Particle {
            id: "4".into(),
            op: ParticleOp::Color([0.5, 0.25, 1.0])
        }),
        "{commands:?}"
    );
    assert_eq!(host.scene["objects"][1]["alpha"], 0.25);
}

#[test]
fn asset_model_layers() {
    let mut scene = json!({"camera":{"eye":"0 0 -4","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100},"objects":[{"id":1,"visible":{"value":true,"script":r"
const mesh = engine.registerAsset('models/mesh.mdl');
export function init() {
    const layer = thisScene.createLayer(mesh);
    layer.origin = new Vec3(1, 2, 3);
    thisScene.createLayer({name:'config', model:'models/mesh.mdl'});
    thisScene.createLayer({name:'collapsed', model:'models/mesh.mdl', scale:new Vec3(0)});
    try { thisScene.createLayer('models/missing.MDL'); throw Error('missing accepted'); } catch (error) {
        if (!String(error.message).includes('models/missing.MDL missing')) throw error;
    }
    return true;
}"}}]});
    let passes = json!([{"shader":"mesh","cullmode":"nocull","blending":"normal"}]);
    let package = crate::model::tests::mesh_package_scene(&passes, scene.to_string().as_bytes());
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let images = crate::model::ImageResources::new(&package, &assets);
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        super::Load { images: Some(images), ..super::Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let models = host.take_created_models();
    assert!(models.iter().all(|(_, model)| model.owner.is_none() && model.layer.reflected));
    let visible: Vec<_> = models.iter().map(|(_, model)| model.layer.visible).collect();
    assert_eq!(visible, [true, true, false]);
}

#[test]
fn shared_is_assignable() {
    let mut scene = json!({"objects":[{"id":1,"visible":{"value":true,"script":r"
shared = {flag: 7};
export function init() {
    if (shared.flag !== 7 || globalThis.shared.flag !== 7) throw Error('shared');
    return true;
}"}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn layer_binding_assignable() {
    let mut scene = json!({"objects":[{"id":1,"visible":{"value":true,"script":r"
export function init() {
    thisLayer = thisScene.getLayer(0);
    thisObject = null;
    return true;
}"}}]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn transform_matrix_composes() {
    let mut scene = json!({"objects":[
        {"id":1,"origin":"10 20 0"},
        {"id":2,"parent":1,"origin":"1 2 3","visible":{"value":true,"script":r"
export function init() {
    const m = thisLayer.getTransformMatrix().m;
    if (m[12] !== 11 || m[13] !== 22 || m[14] !== 3) throw Error(m.join(' '));
    return true;
}"}}
    ]});
    let host = image_host(&mut scene);
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn shader_vectors_reach_scripts() {
    let source = r"
export function update(value) {
    value.x = 5;
    return value;
}";
    let mut scene = json!({"objects":[{"id":1,"image":"models/white.json",
        "effects":[{"file":"effects/probe.json","passes":[{"constantshadervalues":{
            "offset":{"value":"1 2","script":source}}}]}]}]});
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("effects/probe.json", br#"{"passes":[{"material":"materials/probe.json"}]}"#),
        ("materials/probe.json", br#"{"passes":[{"shader":"probe"}]}"#),
        (
            "shaders/probe.frag",
            br#"uniform vec2 u_Offset; // {"material":"offset","default":"0 0"}"#,
        ),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some(""));
    let images = crate::model::ImageResources::new(&package, &assets);
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        super::Load { images: Some(images), ..super::Load::default() },
    )
    .unwrap()
    .unwrap();
    host.tick(0.1, 0.1, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let offset =
        &host.scene["objects"][0]["effects"][0]["passes"][0]["constantshadervalues"]["offset"];
    assert_eq!(offset, &json!("5 2"));
}
