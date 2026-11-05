use super::{Load, SceneScripts};
use crate::model::Properties;
use crate::puppet::playback::Playback;
use serde_json::json;

fn host(scene: &mut serde_json::Value, slot: usize) -> SceneScripts {
    let mesh = crate::puppet::tests::hierarchy_fixture();
    let mut puppets = Playback::default();
    puppets.install_rig(slot, scene["objects"][slot]["id"].to_string(), mesh);
    SceneScripts::load_with_storage(
        scene,
        &Properties::new(),
        &json!({}),
        Load { puppets: Some(puppets), ..Load::default() },
    )
    .unwrap()
    .unwrap()
}

#[test]
fn bone_initial_state() {
    let script = r"
const samples = [];
function vector(value) { return [value.x, value.y, value.z]; }
function transform(matrix) {
    return [matrix.translation(), matrix.right(), matrix.up(), matrix.forward()].map(vector);
}
function sample() {
    samples.push([thisLayer.getBoneCount(), transform(thisLayer.getBoneTransform(0)),
        transform(thisLayer.getLocalBoneTransform(0))]);
}
export function init() {
    sample();
    thisLayer.setBoneTransform(0, Mat4.fromTranslation(new Vec3(80, 90, 7)));
    thisLayer.setLocalBoneTransform(0, Mat4.fromTranslation(new Vec3(80, 90, 7)));
    sample();
    return JSON.stringify(samples);
}
export function update() { sample(); return JSON.stringify(samples); }
";
    let mut scene = json!({"objects":[
        {"id":20,"text":{"value":"","script":script},"origin":"10 20 3","scale":"2 3 1"}
    ]});
    let mut host = host(&mut scene, 0);
    host.tick(0.0, 0.0, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    let values: serde_json::Value =
        serde_json::from_str(host.scene["objects"][0]["text"].as_str().unwrap()).unwrap();
    let identity = json!([[0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1]]);
    assert_eq!(
        values,
        json!([
            [3, identity, identity],
            [3, identity, identity],
            [3, [[10, 20, 3], [2, 0, 0], [0, 3, 0], [0, 0, 1]], identity]
        ])
    );
}

#[test]
fn bone_world_space() {
    let script = r"
function vector(value) {
    return [value.x, value.y, value.z].map(number => Math.round(number * 1000) / 1000);
}
function transform(matrix) {
    return [matrix.translation(), matrix.right(), matrix.up(), matrix.forward()].map(vector);
}
export function update() {
    thisLayer.setLocalBoneTransform(0, Mat4.fromTranslation(new Vec3(2, 3, 4)));
    const local = transform(thisLayer.getLocalBoneTransform(0));
    const world = transform(thisLayer.getBoneTransform(0));
    thisLayer.setBoneTransform(0, Mat4.fromTranslation(new Vec3(40, 260, 8)));
    return JSON.stringify([local, world, transform(thisLayer.getBoneTransform(0))]);
}
";
    for parented in [false, true] {
        let mut scene = json!({"objects":[
            {"id":10,"text":"parent","origin":"100 200 0",
                "angles":"0 0 1.5707963267948966","scale":"2 3 1"},
            {"id":20,"text":{"value":"","script":script},"origin":"10 20 3"}
        ]});
        if parented {
            scene["objects"][1]["parent"] = json!(10);
        }
        let mut host = host(&mut scene, 1);
        host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        let values: serde_json::Value =
            serde_json::from_str(host.scene["objects"][1]["text"].as_str().unwrap()).unwrap();
        let world = if parented { [31, 224, 7] } else { [12, 23, 7] };
        let right = if parented { [0, 2, 0] } else { [1, 0, 0] };
        let up = if parented { [-3, 0, 0] } else { [0, 1, 0] };
        assert_eq!(
            values,
            json!([
                [[2, 3, 4], [1, 0, 0], [0, 1, 0], [0, 0, 1]],
                [world, right, up, [0, 0, 1]],
                [[40, 260, 8], [1, 0, 0], [0, 1, 0], [0, 0, 1]]
            ])
        );
    }
}

#[test]
fn bone_frame_transform() {
    let script = r"
let first = true;
function position(matrix) {
    const value = matrix.translation();
    return [value.x, value.y, value.z].map(number => Math.round(number * 1000) / 1000);
}
export function update() {
    const rows = [position(thisLayer.getBoneTransform(0))];
    if (first) {
        first = false;
        thisLayer.getParent().origin = new Vec3(340, 200, 0);
        rows.push(position(thisLayer.getBoneTransform(0)));
        thisLayer.setBoneTransform(0, Mat4.fromTranslation(new Vec3(1000, 600, 7)));
        rows.push(position(thisLayer.getBoneTransform(0)));
        rows.push(position(thisLayer.getLocalBoneTransform(0)));
        thisLayer.setLocalBoneTransform(0, Mat4.fromTranslation(new Vec3(2, 3, 4)));
        rows.push(position(thisLayer.getBoneTransform(0)));
    }
    return JSON.stringify(rows);
}
";
    let mut scene = json!({"objects":[
        {"id":10,"text":"parent","origin":"100 200 0",
            "angles":"0 0 1.5707963267948966","scale":"2 3 1"},
        {"id":20,"text":{"value":"","script":script},"origin":"10 20 3","parent":10}
    ]});
    let mut host = host(&mut scene, 1);
    for expected in [
        json!([[40, 220, 3], [40, 220, 3], [1000, 600, 7], [0, 0, 0], [271, 224, 7]]),
        json!([[280, 220, 3]]),
    ] {
        host.tick(0.0, 0.0, [0.5; 2]).unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        let actual: serde_json::Value =
            serde_json::from_str(host.scene["objects"][1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn bone_mixed_basis() {
    let script = r"
let first = true;
function positions() {
    return [0, 1, 2].map(index => {
        const value = thisLayer.getBoneTransform(index).translation();
        return [value.x, value.y, value.z].map(number => Math.round(number * 1000) / 1000);
    });
}
export function update() {
    const rows = [positions()];
    if (first) {
        first = false;
        thisLayer.getParent().origin = new Vec3(340, 200, 0);
        thisLayer.setBoneTransform(0, Mat4.fromTranslation(new Vec3(1000, 600, 7)));
        rows.push(positions());
        thisLayer.setLocalBoneTransform(1, Mat4.fromTranslation(new Vec3(2, 3, 4)));
        rows.push(positions());
        thisLayer.getParent().origin = new Vec3(580, 200, 0);
        rows.push(positions());
        thisLayer.setLocalBoneTransform(1, Mat4.fromTranslation(new Vec3(3, 4, 5)));
        rows.push(positions());
        thisLayer.setLocalBoneTransform(0, Mat4.fromTranslation(new Vec3(1, 2, 3)));
        rows.push(positions());
        const invalid = Mat4.fromTranslation(new Vec3(9));
        invalid.m[15] = 0;
        thisLayer.setLocalBoneTransform(0, invalid);
        rows.push(positions());
    }
    return JSON.stringify(rows);
}
";
    let mut scene = json!({"objects":[
        {"id":10,"text":"parent","origin":"100 200 0",
            "angles":"0 0 1.5707963267948966","scale":"2 3 1"},
        {"id":20,"text":{"value":"","script":script},"origin":"10 20 3","parent":10}
    ]});
    let mut host = host(&mut scene, 1);
    let changed = json!([[514, 222, 6], [502, 228, 11], [40, 260, 3]]);
    for expected in [
        json!([
            [[40, 220, 3], [40, 230, 3], [40, 260, 3]],
            [[1000, 600, 7], [40, 230, 3], [40, 260, 3]],
            [[1000, 600, 7], [1002, 603, 11], [40, 260, 3]],
            [[1000, 600, 7], [1002, 603, 11], [40, 260, 3]],
            [[1000, 600, 7], [1243, 604, 12], [40, 260, 3]],
            changed,
            changed
        ]),
        json!([[[520, 220, 3], [520, 230, 3], [520, 260, 3]]]),
    ] {
        host.tick(0.0, 0.0, [0.5; 2]).unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        let actual: serde_json::Value =
            serde_json::from_str(host.scene["objects"][1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn spring_scripted_owner_scope() {
    for parented in [false, true] {
        let target = if parented { "thisScene.getLayer('parent')" } else { "thisLayer" };
        let script = format!(
            "let count=0; export function update() {{ {target}.angles = new Vec3(30,0,0); return String(++count); }}"
        );
        let mut scene = json!({"objects":[
            {"id":10,"name":"parent","origin":"100 200 0"},
            {"id":20,"text":{"value":"","script":script},"origin":"10 20 3"}
        ]});
        if parented {
            scene["objects"][1]["parent"] = json!(10);
        }
        let mut mesh = crate::puppet::tests::hierarchy_fixture();
        mesh.springs.push(crate::puppet::spring::Definition {
            bone: 0,
            translation: Some(crate::puppet::spring::Translation {
                stiffness: 8.0,
                friction: 2.0,
                inertia: 0.5,
                maximum: 2.0,
            }),
            rotation: None,
            gravity: None,
        });
        let mut puppets = Playback::default();
        puppets.install_rig(1, "20".into(), mesh);
        let mut host = SceneScripts::load_with_storage(
            &mut scene,
            &Properties::new(),
            &json!({}),
            Load { puppets: Some(puppets), ..Load::default() },
        )
        .unwrap()
        .unwrap();
        host.tick(0.125, 0.125, [0.5; 2]).unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(host.scene["objects"][1]["text"], "1");
        host.tick(0.25, 0.125, [0.5; 2]).unwrap();
        assert_eq!(host.scene["objects"][1]["text"], "1");
        assert!(host.diagnostics.iter().any(|error| error.contains("Puppet spring pose invalid")));
        assert!(!host.animated());
    }
}
