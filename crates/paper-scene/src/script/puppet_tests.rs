use super::{Load, SceneScripts};
use crate::model::Properties;
use crate::puppet::playback::{Clip, Playback};
use serde_json::json;

#[test]
fn puppet_receiver_controls() {
    let script = r"
export function init() {
    const layer = thisLayer.getAnimationLayer(0);
    const values = [thisLayer.getAnimationLayerCount(), layer === thisLayer.getAnimationLayer('Animación 1'),
        thisLayer.getAnimationLayer(-1) === null, thisLayer.getAnimationLayer(1) === null,
        thisLayer.getAnimationLayer() === null, layer.fps, layer.frameCount, layer.duration,
        layer.rate, layer.isPlaying(), layer.play.length, layer.setFrame.length,
        layer.getAnimation() === undefined];
    values.push(typeof layer.setFrame(120), layer.getFrame());
    values.push(typeof layer.pause(), layer.isPlaying());
    values.push(typeof layer.stop(), layer.getFrame(), layer.isPlaying());
    thisLayer.text = JSON.stringify(values);
}
";
    let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":script}}]});
    let puppets = Playback::new(vec![Clip {
        slot: 0,
        object: "182".into(),
        name: "Animación 1".into(),
        animation: 220,
        fps: 60.0,
        length: 1380.0,
        rate: 0.0,
    }]);
    let host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        Load { puppets: Some(puppets), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(
        scene["objects"][0]["text"],
        "[1,true,true,true,true,59.999996185302734,1380,23,0,true,0,0,true,\"undefined\",119.99999237060547,\"undefined\",false,\"undefined\",0,false]"
    );
}

#[test]
fn puppet_script_clock() {
    let script = r"
export function init() { thisLayer.getAnimationLayer(0).rate = 2; }
export function update() { return String(thisLayer.getAnimationLayer(0).getFrame()); }
";
    let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":script}}]});
    let puppets = Playback::new(vec![Clip {
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
        &Properties::new(),
        &json!({}),
        Load { puppets: Some(puppets), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    host.tick(0.001_705_2, 0.001_705_2, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "0.2046239823102951");
    host.tick(0.133_695_8, 0.131_990_6, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "16.043495178222656");
}

#[test]
fn puppet_multiple_layers() {
    let script = r"
export function init() {
    const first = thisLayer.getAnimationLayer(0);
    const second = thisLayer.getAnimationLayer('second');
    if (thisLayer.getAnimationLayerCount() !== 2 || second !== thisLayer.getAnimationLayer(1)) throw Error('layers');
    first.blend = 0.25;
    second.visible = false;
    second.rate = 2;
    second.setFrame(30);
    thisLayer.text = JSON.stringify([first.blend, second.visible, second.rate, Math.round(second.getFrame())]);
}
";
    let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":script}}]});
    let clips = ["first", "second"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| Clip {
            slot: 0,
            object: "182".into(),
            name: name.into(),
            animation: index as u32 + 1,
            fps: 30.0,
            length: 60.0,
            rate: 1.0,
        })
        .collect();
    let puppets = Playback::new(clips);
    let host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        Load { puppets: Some(puppets), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["text"], "[0.25,false,2,30]");
    let states = host.puppet_states("182");
    assert_eq!(states.len(), 2);
    assert_eq!(states[0].layer.blend, 0.25);
    assert_eq!(states[1].layer.blend, 0.0);
}

#[test]
fn puppet_end_callback() {
    let script = r"
export function init() {
    thisLayer.getAnimationLayer(0).addEndedCallback(() => {
        thisLayer.text = String(Number(thisLayer.text || 0) + 1);
    });
}

";
    let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":script}}]});
    let puppets = Playback::new(vec![Clip {
        slot: 0,
        object: "182".into(),
        name: "loop".into(),
        animation: 1,
        fps: 30.0,
        length: 30.0,
        rate: 1.0,
    }]);
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        Load { puppets: Some(puppets), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "1");
    host.tick(3.0, 2.0, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "3");
}

#[test]
fn puppet_event_dispatch() {
    let script = r"
export function init() {
    scene.on('animationEvent', event => {
        thisLayer.text += 'scene:' + event.name + ':' + event.frame + ':' + event.nodeId + ';';
    });
}
export function animationEvent(event, value) {
    thisLayer.text += 'owner:' + event.name + ':' + event.frame + ':' + value + ';';
}
";
    let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":script}}]});
    let mesh = crate::puppet::parse(include_bytes!("../fixtures/samurai.mdl")).unwrap();
    let animation = mesh.animations.iter().find(|animation| !animation.events.is_empty()).unwrap();
    let clip = Clip {
        slot: 0,
        object: "182".into(),
        name: animation.name.clone(),
        animation: animation.id,
        fps: animation.fps,
        length: animation.duration as f32,
        rate: 1.0,
    };
    let mut playback = Playback::new(vec![clip]);
    playback.install_rig(0, "182".into(), mesh);
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        Load { puppets: Some(playback), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    host.tick(0.01, 0.01, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "owner:sword:0:;scene:sword:0:182;");
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
#[ignore = "requires an installed authored modern puppet model"]
fn modern_event_dispatch() {
    let path = std::env::var("SKWD_MODERN_EVENT_MDL").unwrap();
    let mesh = crate::puppet::parse(&std::fs::read(path).unwrap()).unwrap();
    let clip = mesh.animations.iter().find(|clip| !clip.events.is_empty()).unwrap();
    let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":"export function animationEvent(event, value) { thisLayer.text += event.name + ':' + event.frame; }"}}]});
    let mut playback = Playback::new(vec![Clip {
        slot: 0,
        object: "182".into(),
        name: clip.name.clone(),
        animation: clip.id,
        fps: clip.fps,
        length: clip.duration as f32,
        rate: 1.0,
    }]);
    playback.install_rig(0, "182".into(), mesh);
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        Load { puppets: Some(playback), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    host.tick(1.1, 1.1, [0.5; 2]).unwrap();
    assert!(host.scene["objects"][0]["text"].as_str().unwrap().ends_with(":30"));
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn puppet_dynamic_layers() {
    let script = r"
let ended = 0;
export function init() {
    const walk = thisLayer.createAnimationLayer('walk');
    if (!walk || thisLayer.getAnimationLayerCount() !== 2) throw Error('create');
    walk.blend = 0.25;
    if (thisLayer.getAnimationLayer('walk') !== walk || !thisLayer.destroyAnimationLayer(walk)) throw Error('destroy');
    const jump = thisLayer.playSingleAnimation('jump');
    if (!jump || thisLayer.getAnimationLayerCount() !== 2) throw Error('single');
    jump.addEndedCallback(() => ended++);
}

export function update() { return [thisLayer.getAnimationLayerCount(), ended].join(','); }
";
    let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":script}}]});
    let authored = Clip {
        slot: 0,
        object: "182".into(),
        name: "idle".into(),
        animation: 1,
        fps: 30.0,
        length: 30.0,
        rate: 0.0,
    };
    let available = ["walk", "jump"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| Clip {
            name: name.into(),
            animation: index as u32 + 2,
            rate: 1.0,
            ..authored.clone()
        })
        .collect();
    let puppets = Playback::with_catalog(vec![authored], available);
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        Load { puppets: Some(puppets), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.puppet_states("182").len(), 2);
    host.tick(0.0, 0.0, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "2,0");
    host.tick(1.0, 1.0, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][0]["text"], "1,1");
    assert_eq!(host.puppet_states("182").len(), 1);
}

#[test]
fn unopened_puppet_layers() {
    let script = r"
export function init() {
    if (thisLayer.destroyAnimationLayer() || thisLayer.destroyAnimationLayer(null)) throw Error('missing');
    if (!thisLayer.destroyAnimationLayer('first')) throw Error('name');
    if (!thisLayer.destroyAnimationLayer(0)) throw Error('index');
    thisLayer.text = String(thisLayer.getAnimationLayerCount());
}
";
    let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":script}}]});
    let clips = ["first", "second"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| Clip {
            slot: 0,
            object: "182".into(),
            name: name.into(),
            animation: index as u32,
            fps: 30.0,
            length: 30.0,
            rate: 0.0,
        })
        .collect();
    let host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        Load { puppets: Some(Playback::new(clips)), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "0");
    assert!(host.puppet_states("182").is_empty());
}

#[test]
fn puppet_script_reuses_clocks() {
    let script = r"
let first;
let turns = 0;
export function init() { first = thisLayer.playSingleAnimation('jump'); }
export function update() {
    if (first.rate !== undefined || first.isPlaying()) throw Error('stale clock');
    if (thisLayer.getAnimationLayerCount() !== 1) throw Error('retirement');
    if (!thisLayer.playSingleAnimation('jump')) throw Error('clock capacity');
    return String(++turns);
}

";
    let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":script}}]});
    let clip = Clip {
        slot: 0,
        object: "182".into(),
        name: "jump".into(),
        animation: 1,
        fps: 30.0,
        length: 30.0,
        rate: 1.0,
    };
    let authored = Clip { name: "idle".into(), rate: 0.0, ..clip.clone() };
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        Load {
            puppets: Some(Playback::with_catalog(vec![authored], vec![clip])),
            ..Load::default()
        },
    )
    .unwrap()
    .unwrap();
    for turn in 1..=130 {
        host.tick(turn as f32, 1.0, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["text"], turn.to_string());
        assert_eq!(host.puppet_states("182").len(), 2);
    }
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
}

#[test]
fn puppet_bone_queries() {
    let script = r"
export function init() {
    thisLayer.text = JSON.stringify([
        thisLayer.getBoneCount(), thisLayer.getBoneIndex('root'), thisLayer.getBoneIndex('child'),
        thisLayer.getBoneIndex('missing'), thisLayer.getBoneIndex(1),
        thisLayer.getBoneParentIndex('root'), thisLayer.getBoneParentIndex('child'),
        thisLayer.getBoneParentIndex(1), thisLayer.getBoneParentIndex('missing')]);
}
";
    let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":script}}]});
    let mut puppets = Playback::default();
    puppets.install_bone_metadata(0, vec![("root".into(), None), ("child".into(), Some(0))]);
    let host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        Load { puppets: Some(puppets), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["text"], "[2,0,1,-1,-1,-1,0,0,-1]");
}

#[test]
fn puppet_morph_weights() {
    let mut mesh = crate::puppet::parse(include_bytes!("../fixtures/samurai.mdl")).unwrap();
    mesh.springs.clear();
    mesh.morph = Some(crate::puppet::morph::Morph {
        scale: 2.0,
        samples: 1,
        indices: Vec::new(),
        shapes: vec![crate::puppet::morph::Shape {
            id: 7,
            name: "smile".into(),
            deltas: vec![[0; 3]],
            alpha: vec![i16::MAX],
            modifier: None,
        }],
    });
    let animation = &mesh.animations[0];
    let mut playback = Playback::new(vec![Clip {
        slot: 0,
        object: "182".into(),
        name: animation.name.clone(),
        animation: animation.id,
        fps: animation.fps,
        length: animation.duration as f32,
        rate: 0.0,
    }]);
    playback.install_rig(0, "182".into(), mesh);
    let script = "export function update() { thisLayer.setBlendShapeWeight('smile',0.75); thisLayer.text = JSON.stringify([thisLayer.getBlendShapeIndex('smile'),thisLayer.getBlendShapeWeight(0)]); }";
    let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":script}}]});
    let mut host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        Load { puppets: Some(playback), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    host.tick(0.1, 0.1, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "[0,0.75]");
    assert_eq!(host.puppet_morph_weights("182"), Some(vec![0.75]));
    let revision = host.puppet_bone_revision("182");
    host.tick(0.2, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.puppet_bone_revision("182"), revision);
}

#[test]
fn bone_edit_lifetime() {
    for repeated in [false, true] {
        let script = format!(
            r"
let count = 0;
export function update() {{
    const before = thisLayer.getBoneTransform(0).translation().x;
    if (count++ === 0 || {repeated}) {{
        thisLayer.setBoneTransform(0, thisLayer.getBoneTransform(0).translation(new Vec3(80, 0, 0)));
    }}
    return String(before);
}}
"
        );
        let mesh = crate::puppet::parse(&crate::puppet::tests::empty_old_fixture()).unwrap();
        let mut puppets = Playback::default();
        puppets.install_rig(0, "182".into(), mesh);
        let mut scene = json!({"objects":[{"id":182,"text":{"value":"","script":script}}]});
        let mut host = SceneScripts::load_with_storage(
            &mut scene,
            &Properties::new(),
            &json!({}),
            Load { puppets: Some(puppets), ..Load::default() },
        )
        .unwrap()
        .unwrap();
        assert!(host.puppet_bone_world("182").is_none());
        host.tick(0.0, 0.0, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["text"], "0");
        assert_eq!(host.puppet_bone_world("182").unwrap().1[0][12], 80.0);
        let revision = host.puppet_bone_revision("182");
        host.tick(0.5, 0.5, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][0]["text"], "0");
        assert_eq!(host.puppet_bone_world("182").is_some(), repeated);
        assert!(host.puppet_bone_revision("182") > revision);
        assert!(host.diagnostics.is_empty());
    }
}

#[test]
fn animation_layer_owner() {
    let script = r"
export function init(value) {
    const handle = 'addEndedCallback' in thisObject ? thisObject : thisObject.getAnimation();
    handle.setFrame(30);
    return handle === thisLayer.getAnimationLayer(0) && handle.getFrame() > 0;
}
";
    let mut scene = json!({"objects":[{"id":182,"animationlayers":[
        {"id":1,"name":"Idle","visible":{"value":false,"script":script}}
    ]}]});
    let puppets = Playback::new(vec![Clip {
        slot: 0,
        object: "182".into(),
        name: "Idle".into(),
        animation: 220,
        fps: 60.0,
        length: 1380.0,
        rate: 1.0,
    }]);
    let host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        Load { puppets: Some(puppets), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["animationlayers"][0]["visible"], true);
}

#[test]
fn filtered_layers_keep_owner() {
    let script = "export function init(value) { thisLayer.text = thisObject.fps; return value; }";
    let clip = |animation, fps| Clip {
        slot: 0,
        object: "182".into(),
        name: String::new(),
        animation,
        fps,
        length: 60.0,
        rate: 1.0,
    };
    let mut puppets = Playback::new(vec![clip(1, 10.0), clip(2, 20.0)]);
    puppets.set_authored(0, 1);
    puppets.set_authored(1, 2);
    let mut scene = json!({"objects":[{"id":182,"text":"","animationlayers":[
        {},
        {"rate":{"value":1,"script":script}},
        {}
    ]}]});
    let host = SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        Load { puppets: Some(puppets), ..Load::default() },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(scene["objects"][0]["text"], 10);
}
