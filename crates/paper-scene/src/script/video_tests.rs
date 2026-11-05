use super::{SceneScripts, ScriptCommand, VideoOp, VideoState};
use crate::model::{ImageResources, Properties};
use serde_json::json;

fn host(script: &str) -> SceneScripts {
    let mut texture = b"TEXV0005\0TEXI0001\0".to_vec();
    for value in [0_u32, crate::tex::FLAG_IS_VIDEO, 2, 2, 2, 2, 0] {
        texture.extend(value.to_le_bytes());
    }
    texture.extend(b"TEXB0003\0");
    for value in [1_i32, -1, 1, 2, 2, 0, 1, 1] {
        texture.extend(value.to_le_bytes());
    }
    texture.push(0);
    assert_ne!(crate::tex::parse_meta(&texture).unwrap().flags & crate::tex::FLAG_IS_VIDEO, 0);
    let mut scene = json!({"objects":[
        {"id":1,"name":"video","image":"models/video.json","size":"2 2"},
        {"id":2,"name":"observer","text":{"value":"","script":script}},
        {"id":3,"name":"second","image":"models/video.json","size":"2 2"}
    ]});
    let source = scene.to_string();
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", source.as_bytes()),
        ("models/video.json", br#"{"material":"materials/video.json"}"#),
        (
            "materials/video.json",
            br#"{"passes":[{"shader":"genericimage2","textures":["video"]}]}"#,
        ),
        ("materials/video.tex", &texture),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some(""));
    let images = ImageResources::new(&package, &assets);
    SceneScripts::load_with_storage(
        &mut scene,
        &Properties::new(),
        &json!({}),
        super::Load { images: Some(images), ..super::Load::default() },
    )
    .unwrap()
    .unwrap()
}

#[test]
fn video_commands() {
    let mut host = host(
        r"
export function init() {
    const video=thisScene.getLayer('video').getVideoTexture();
    video.pause(); video.setCurrentTime(1.5); video.rate=2; video.loop=false;
    thisLayer.text=JSON.stringify([video.isPlaying(),video.getCurrentTime(),video.rate,video.loop]);
    video.play(); video.stop();
}
",
    );
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][1]["text"], "[false,1.5,2,false]");
    assert_eq!(
        host.take_commands(),
        [
            VideoOp::Pause,
            VideoOp::Seek(1.5),
            VideoOp::Rate(2.0),
            VideoOp::Loop(false),
            VideoOp::Play,
            VideoOp::Stop
        ]
        .map(|op| ScriptCommand::Video { object: 0, op })
    );
}

#[test]
fn video_ended_callbacks() {
    let mut host = host(
        r"
let count=0;
const video=thisScene.getLayer('video').getVideoTexture();
video.addEndedCallback(() => { count++; });
export function update() { return JSON.stringify([video.duration,video.getCurrentTime(),video.rate,video.loop,video.isPlaying(),count]); }
",
    );
    let mut state = VideoState {
        object: 0,
        duration: 2.0,
        position: 0.25,
        rate: 1.0,
        looping: true,
        playing: true,
        ended: 1,
    };
    host.set_videos(std::slice::from_ref(&state), false);
    host.tick(2.25, 0.25, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][1]["text"], "[2,0.25,1,true,true,1]");
    state = VideoState {
        object: 0,
        duration: 2.0,
        position: 0.5,
        rate: 1.0,
        looping: true,
        playing: true,
        ended: 1,
    };
    host.set_videos(std::slice::from_ref(&state), false);
    host.tick(2.5, 0.25, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][1]["text"], "[2,0.5,1,true,true,1]");
}

#[test]
fn ended_commands_wake() {
    let mut host = host(
        r"
const video=thisScene.getLayer('video').getVideoTexture();
video.addEndedCallback(() => { video.pause(); });
",
    );
    assert!(!host.animated());
    let state = VideoState {
        object: 0,
        duration: 2.0,
        position: 0.0,
        rate: 1.0,
        looping: false,
        playing: false,
        ended: 1,
    };
    host.set_videos(std::slice::from_ref(&state), true);
    assert!(host.animated());
    host.tick(2.0, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.take_commands(), [ScriptCommand::Video { object: 0, op: VideoOp::Pause }]);
    assert!(!host.animated());
}

#[test]
fn video_late_callback() {
    let mut host = host(
        r"
let video, count=0;
export function update() {
    if (engine.runtime < 4) return String(count);
    if (!video) {
        video=thisScene.getLayer('video').getVideoTexture();
        video.addEndedCallback(() => { count++; });
    }
    return String(count);
}

",
    );
    let mut state = VideoState {
        object: 0,
        duration: 2.0,
        position: 0.5,
        rate: 1.0,
        looping: true,
        playing: true,
        ended: 2,
    };
    host.set_videos(std::slice::from_ref(&state), false);
    host.tick(4.5, 0.1, [0.5; 2]).unwrap();
    host.set_videos(std::slice::from_ref(&state), true);
    host.tick(4.6, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][1]["text"], "0");
    state.ended = 3;
    host.set_videos(std::slice::from_ref(&state), false);
    host.tick(6.0, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][1]["text"], "1");
}

#[test]
fn callbacks_share_snapshot() {
    let mut host = host(
        r"
const first=thisScene.getLayer('video').getVideoTexture();
let second, count=0;
first.addEndedCallback(() => {
    if (!second) {
        second=thisScene.getLayer('second').getVideoTexture();
        second.addEndedCallback(() => { count++; });
    }
});
export function update() { return String(count); }
",
    );
    let mut states = [
        VideoState {
            object: 0,
            duration: 2.0,
            position: 0.0,
            rate: 1.0,
            looping: true,
            playing: true,
            ended: 1,
        },
        VideoState {
            object: 2,
            duration: 1.0,
            position: 0.0,
            rate: 1.0,
            looping: true,
            playing: true,
            ended: 3,
        },
    ];
    host.set_videos(&states, false);
    host.tick(3.0, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][1]["text"], "0");
    states[1].ended = 4;
    host.set_videos(&states, false);
    host.tick(4.0, 0.1, [0.5; 2]).unwrap();
    assert_eq!(host.scene["objects"][1]["text"], "1");
}
