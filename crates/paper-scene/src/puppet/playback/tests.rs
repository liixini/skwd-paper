use super::{Clip, Control, Playback, Sample, Transition};

fn playback() -> Playback {
    Playback::new(vec![Clip {
        slot: 0,
        object: "182".into(),
        name: "Animación 1".into(),
        animation: 220,
        fps: 60.0,
        length: 1380.0,
        rate: 0.0,
    }])
}

#[test]
fn puppet_seek_precision() {
    let mut playback = playback();
    let descriptor = &playback.descriptors()[0];
    assert_eq!(descriptor.fps, 59.999_996_185_302_734);
    assert_eq!(descriptor.duration, 23.0);
    assert_eq!(playback.control(0, Control::IsPlaying, 0.0), Some(1.0));
    playback.control(0, Control::SetFrame, 120.0);
    for time in [0.001_445_9, 0.628_121_1] {
        playback.advance(time);
        assert_eq!(playback.control(0, Control::GetFrame, 0.0), Some(119.999_99));
        assert_eq!(playback.sample("182", 220), Some(Sample::Frame(2.0)));
    }
}

#[test]
fn puppet_pause_stop() {
    let mut playback = playback();
    playback.control(0, Control::SetFrame, 840.0);
    playback.control(0, Control::Pause, 0.0);
    playback.advance(1.0);
    assert_eq!(playback.control(0, Control::IsPlaying, 0.0), Some(0.0));
    playback.control(0, Control::SetFrame, 36.0);
    playback.advance(2.0);
    assert_eq!(playback.control(0, Control::GetFrame, 0.0), Some(36.0));
    playback.control(0, Control::Stop, 0.0);
    assert_eq!(playback.control(0, Control::GetFrame, 0.0), Some(0.0));
    assert_eq!(playback.control(0, Control::IsPlaying, 0.0), Some(0.0));
}

#[test]
fn puppet_first_update() {
    let mut playback = playback();
    playback.control(0, Control::SetRate, 2.0);
    playback.advance(0.001_274_2);
    assert_eq!(playback.control(0, Control::GetFrame, 0.0), Some(0.152_904));
}

#[test]
fn puppet_retired_clock() {
    let mut playback = playback();
    playback.control(0, Control::SetRate, 1.0);
    playback.retire(0);
    playback.advance(1.0);
    assert!(!playback.animated());
    assert_eq!(playback.sample("182", 220), Some(Sample::Retired));
    assert_eq!(playback.control(0, Control::Play, 0.0), None);
}

#[test]
fn puppet_native_steps() {
    let mut playback = playback();
    playback.control(0, Control::SetRate, 2.0);
    for (index, (dt, frame)) in [
        (0.001_705_199_945_718_05_f64, 0.204_623_982_310_295_1),
        (0.131_990_596_652_030_94, 16.043_495_178_222_656),
        (0.023_586_999_624_967_575, 18.873_935_699_462_89),
        (0.029_905_900_359_153_748, 20.668_289_184_570_312),
        (0.029_886_700_212_955_475, 22.461_490_631_103_516),
        (0.029_823_299_497_365_95, 22.461_490_631_103_516),
        (0.030_441_600_829_362_87, 24.287_988_662_719_727),
        (0.030_936_000_868_678_093, 26.144_147_872_924_805),
        (0.030_109_399_929_642_677, 119.999_992_370_605_47),
    ]
    .into_iter()
    .enumerate()
    {
        playback.advance(dt as f32);
        assert_eq!(f64::from(playback.control(0, Control::GetFrame, 0.0).unwrap()), frame);
        match index {
            2 => {
                playback.control(0, Control::SetRate, 1.0);
            }
            4 => {
                playback.control(0, Control::Pause, 0.0);
            }
            5 => {
                playback.control(0, Control::Play, 0.0);
            }
            7 => {
                playback.control(0, Control::Pause, 0.0);
                playback.control(0, Control::SetFrame, 120.0);
            }
            _ => {}
        }
    }
}

#[test]
fn puppet_loop_stop() {
    let mut playback = playback();
    playback.control(0, Control::SetFrame, 1378.0);
    playback.control(0, Control::SetRate, 1.0);
    playback.advance(0.001_157_4);
    assert_eq!(
        f64::from(playback.control(0, Control::GetFrame, 0.0).unwrap()),
        1_378.069_458_007_812_5
    );
    playback.advance(1.0);
    let Some(Sample::Frame(seconds)) = playback.sample("182", 220) else {
        panic!("live sample");
    };
    assert!(seconds < 1.0);
    assert_eq!(playback.control(0, Control::IsPlaying, 0.0), Some(1.0));
    playback.control(0, Control::Stop, 0.0);
    playback.advance(1.0);
    assert_eq!(playback.sample("182", 220), Some(Sample::Frame(0.0)));
    assert_eq!(playback.control(0, Control::IsPlaying, 0.0), Some(0.0));
}

#[test]
fn puppet_visibility_preserves_clock() {
    let mut playback = playback();
    playback.control(0, Control::SetFrame, 120.0);
    assert_eq!(playback.visibility(0, None), Some(true));
    assert_eq!(playback.visibility(0, Some(false)), Some(false));
    assert_eq!(playback.sample("182", 220), Some(Sample::Rest));
    assert_eq!(playback.control(0, Control::GetFrame, 0.0), Some(119.999_99));
    assert_eq!(playback.control(0, Control::IsPlaying, 0.0), Some(1.0));
    assert_eq!(playback.visibility(0, Some(true)), Some(true));
    assert_eq!(playback.sample("182", 220), Some(Sample::Frame(2.0)));
    playback.retire(0);
    assert_eq!(playback.visibility(0, Some(true)), None);
    assert_eq!(playback.sample("182", 220), Some(Sample::Retired));
}

#[test]
fn puppet_fade_autosort() {
    let clip = Clip {
        slot: 0,
        object: "182".into(),
        name: "idle".into(),
        animation: 1,
        fps: 30.0,
        length: 30.0,
        rate: 0.0,
    };
    let opaque = clip.clone();
    let additive = Clip { name: "additive".into(), animation: 2, ..clip.clone() };
    let jump = Clip { name: "jump".into(), animation: 3, rate: 1.0, ..clip };
    let mut playback = Playback::with_catalog(vec![opaque, additive], vec![jump]);
    playback.configure(1, 1.0, true, true);
    let transition = Transition { blendin: true, blendout: true, blendtime: 0.5, autosort: true };
    let clock = playback.create(0, "jump", true, transition).unwrap();
    assert_eq!(
        playback.descriptors().iter().map(|entry| entry.name).collect::<Vec<_>>(),
        ["idle", "jump", "additive"]
    );
    assert_eq!(playback.descriptors()[1].clock, clock);
    assert_eq!(playback.states("182")[1].layer.blend, 0.0);
    playback.advance(0.25);
    assert_eq!(playback.states("182")[1].layer.blend, 0.5);
    playback.advance(0.5);
    assert_eq!(playback.states("182")[1].layer.blend, 0.5);
    playback.advance(0.25);
    assert_eq!(playback.take_ended(), vec![clock]);
    assert_eq!(playback.descriptors().len(), 2);
    assert_eq!(playback.states("182").len(), 2);
}

#[test]
fn puppet_script_fade_defaults() {
    let transition: Transition = serde_json::from_str("{}").unwrap();
    assert!(transition.blendin);
    assert!(transition.blendout);
    assert_eq!(transition.blendtime, super::DEFAULT_FADE_SECONDS);
    let mut playback = Playback::with_catalog(
        Vec::new(),
        vec![Clip {
            slot: 0,
            object: "182".into(),
            name: "jump".into(),
            animation: 1,
            fps: 30.0,
            length: 60.0,
            rate: 1.0,
        }],
    );
    playback.create(0, "jump", true, transition).unwrap();
    assert_eq!(playback.states("182")[0].layer.blend, 0.0);
    playback.advance(transition.blendtime / 2.0);
    assert_eq!(playback.states("182")[0].layer.blend, 0.5);
}

fn event_playback(mirror: bool) -> (Playback, f32, f32) {
    let mut mesh = crate::puppet::parse(include_bytes!("../../fixtures/samurai.mdl")).unwrap();
    let animation =
        mesh.animations.iter_mut().find(|animation| !animation.events.is_empty()).unwrap();
    animation.mirror = mirror;
    animation.events.push(crate::puppet::AnimationEvent {
        field: 0,
        payload: crate::puppet::EventPayload { frame: 1, name: "step".into() },
    });
    let clip = Clip {
        slot: 0,
        object: "182".into(),
        name: animation.name.clone(),
        animation: animation.id,
        fps: animation.fps,
        length: animation.duration as f32,
        rate: 1.0,
    };
    let frame_time = animation.fps.recip();
    let duration = clip.length / clip.fps;
    let mut playback = Playback::new(vec![clip]);
    playback.install_rig(0, "182".into(), mesh);
    (playback, frame_time, duration)
}

#[test]
fn puppet_event_crossings() {
    let (mut playback, frame_time, duration) = event_playback(false);
    playback.advance(frame_time / 2.0);
    assert_eq!(
        playback.take_events().iter().map(|event| event.name.as_str()).collect::<Vec<_>>(),
        ["sword"]
    );
    playback.advance(frame_time);
    assert_eq!(
        playback.take_events().iter().map(|event| event.name.as_str()).collect::<Vec<_>>(),
        ["step"]
    );
    playback.control(0, Control::Pause, 0.0);
    playback.advance(duration);
    assert!(playback.take_events().is_empty());
    playback.control(0, Control::Play, 0.0);
    playback.advance(duration);
    assert_eq!(
        playback.take_events().iter().map(|event| event.name.as_str()).collect::<Vec<_>>(),
        ["sword", "step"]
    );
}

#[test]
fn puppet_event_order() {
    let (mut playback, frame_time, duration) = event_playback(false);
    playback
        .rigs
        .get_mut(&0)
        .unwrap()
        .mesh
        .animations
        .iter_mut()
        .find(|clip| !clip.events.is_empty())
        .unwrap()
        .events
        .reverse();
    playback.advance(duration + frame_time * 2.0);
    assert_eq!(
        playback.take_events().iter().map(|event| event.name.as_str()).collect::<Vec<_>>(),
        ["sword", "step", "sword", "step"]
    );
}

#[test]
fn puppet_mirror_events() {
    let (mut playback, frame_time, duration) = event_playback(true);
    playback.advance(duration - frame_time / 2.0);
    assert_eq!(
        playback.take_events().iter().map(|event| event.name.as_str()).collect::<Vec<_>>(),
        ["sword", "step"]
    );
    playback.advance(duration);
    assert!(
        matches!(playback.sample("182", playback.layers[0].clip.animation), Some(Sample::Frame(time)) if time > duration)
    );
    assert_eq!(
        playback.take_events().iter().map(|event| event.name.as_str()).collect::<Vec<_>>(),
        ["step"]
    );
    playback.advance(frame_time);
    assert_eq!(
        playback.take_events().iter().map(|event| event.name.as_str()).collect::<Vec<_>>(),
        ["sword"]
    );
}

#[test]
fn puppet_mirror_fade() {
    let (mut playback, _, duration) = event_playback(true);
    playback.layers[0].transition =
        Transition { blendin: false, blendout: true, blendtime: duration / 4.0, autosort: false };
    playback.advance(duration + duration / 4.0);
    assert_eq!(playback.states("182")[0].layer.blend, 1.0);
}

#[test]
fn puppet_reuses_clocks() {
    let clip = Clip {
        slot: 0,
        object: "182".into(),
        name: "jump".into(),
        animation: 1,
        fps: 30.0,
        length: 30.0,
        rate: 1.0,
    };
    let mut playback = Playback::with_catalog(Vec::new(), vec![clip]);
    let first = playback.create(0, "jump", true, Transition::default()).unwrap();
    playback.advance(1.0);
    assert_eq!(playback.take_ended(), vec![first]);
    for _ in 0..=super::MAX_ANIMATION_LAYERS {
        let clock = playback.create(0, "jump", true, Transition::default()).unwrap();
        assert_ne!(clock, first);
        assert_eq!(playback.control(first, Control::Play, 0.0), None);
        assert!(!playback.destroy(first));
        playback.advance(1.0);
        assert_eq!(playback.take_ended(), vec![clock]);
        assert!(playback.order.is_empty());
    }
    assert_eq!(playback.layers.len(), 1);
}

#[test]
fn puppet_child_bone_transform() {
    let mesh = crate::puppet::parse(include_bytes!("../../fixtures/samurai.mdl")).unwrap();
    let child = mesh.bones.iter().position(|bone| bone.parent.is_some()).unwrap();
    let parent = usize::from(mesh.bones[child].parent.unwrap());
    let mut playback = Playback::default();
    playback.install_rig(0, "182".into(), mesh);
    let original_parent = playback.bone_matrix(0, parent, false).unwrap();
    let original_child = playback.bone_matrix(0, child, false).unwrap();
    let mut local = playback.bone_matrix(0, child, true).unwrap();
    local[12] += 2.0;
    assert!(playback.set_bone_matrix(0, child, local, true));
    assert_eq!(playback.bone_matrix(0, parent, false).unwrap(), original_parent);
    let changed_child = playback.bone_matrix(0, child, false).unwrap();
    assert!((changed_child[12] - original_child[12] - 2.0 * original_parent[0]).abs() < 0.001);
    assert!((changed_child[13] - original_child[13] - 2.0 * original_parent[1]).abs() < 0.001);
    let mut target = changed_child;
    target[12] += 3.0;
    assert!(playback.set_bone_matrix(0, child, target, false));
    assert_eq!(playback.bone_matrix(0, parent, false).unwrap(), original_parent);
    assert!((playback.bone_matrix(0, child, false).unwrap()[12] - target[12]).abs() < 0.001);
    target[15] = 0.0;
    assert!(!playback.set_bone_matrix(0, child, target, false));
}

#[test]
fn bones_without_clips() {
    let mesh = crate::puppet::parse(&crate::puppet::tests::empty_old_fixture()).unwrap();
    let mut playback = Playback::default();
    playback.install_rig(3, "jelly".into(), mesh);
    let mut pose = playback.bone_matrix(3, 0, false).unwrap();
    pose[12] += 80.0;
    assert!(playback.set_bone_matrix(3, 0, pose, false));
    assert_eq!(playback.bone_world("jelly").unwrap().1[0], pose);
    assert!(playback.bone_palette("jelly").is_some());
    assert!(playback.bone_revision("jelly") > 0);
    assert!(!playback.owner_retired("jelly"));
    playback.retire(3);
    assert!(playback.owner_retired("jelly"));
    assert!(playback.bone_world("jelly").is_none());
}

#[test]
fn bone_override_order() {
    let mesh = crate::puppet::tests::hierarchy_fixture();
    let mut playback = Playback::default();
    playback.install_rig(0, "jelly".into(), mesh);
    let rest = playback.bone_matrix(0, 0, false).unwrap();
    let world = playback.rigs[&0].world.as_ptr();
    let locals = playback.rigs[&0].locals.as_ptr();
    let descendants = playback.rigs[&0].descendants.as_ptr();
    for _ in 0..100 {
        let mut parent = rest;
        parent[12] = 80.0;
        assert!(playback.set_bone_matrix(0, 0, parent, false));
        assert_eq!(playback.bone_matrix(0, 0, true).unwrap()[12], 0.0);
        assert_eq!(playback.bone_matrix(0, 1, false).unwrap()[12], 5.0);
        let mut child = playback.bone_matrix(0, 1, true).unwrap();
        child[12] = 7.0;
        assert!(playback.set_bone_matrix(0, 1, child, true));
        assert_eq!(playback.bone_matrix(0, 1, false).unwrap()[12], 87.0);
        child[12] = 100.0;
        assert!(playback.set_bone_matrix(0, 1, child, false));
        assert_eq!(playback.bone_matrix(0, 1, true).unwrap()[12], 7.0);
        parent[12] = 20.0;
        assert!(playback.set_bone_matrix(0, 0, parent, true));
        assert_eq!(playback.bone_matrix(0, 1, false).unwrap()[12], 27.0);
        assert_eq!(playback.bone_matrix(0, 2, false).unwrap()[12], 20.0);
        playback.reset_bones();
        assert_eq!(playback.bone_matrix(0, 0, false), Some(rest));
        assert_eq!(playback.bone_matrix(0, 1, false).unwrap()[12], 5.0);
        assert_eq!(playback.rigs[&0].world.as_ptr(), world);
        assert_eq!(playback.rigs[&0].locals.as_ptr(), locals);
        assert_eq!(playback.rigs[&0].descendants.as_ptr(), descendants);
    }
}
#[test]
fn animated_bone_locals() {
    use crate::puppet::{Bone, Transform};
    for (blend, parent_x, local_x, world_x) in [(1.0, 20.0, 7.0, 34.0), (0.5, 15.0, 5.0, 25.0)] {
        let mut mesh = crate::puppet::parse(&crate::puppet::tests::fixture()).unwrap();
        let parent = crate::matrix::model_matrix([10.0, 0.0, 0.0], [2.0; 3], [0.0; 3]);
        let child = crate::matrix::model_matrix([3.0, 0.0, 0.0], [1.0; 3], [0.0; 3]);
        mesh.bones[0] = Bone::from_matrix("parent".into(), None, parent, false).unwrap();
        mesh.bones.push(Bone::from_matrix("child".into(), Some(0), child, false).unwrap());
        mesh.native_bind_inverse = crate::puppet::pose::bind_inverse(&mesh.bones);
        mesh.animations[0].tracks[0].fill(Transform {
            translation: [20.0, 0.0, 0.0],
            rotation: [0.0; 3],
            scale: [2.0; 3],
        });
        mesh.animations[0].tracks.push(vec![
            Transform {
                translation: [7.0, 0.0, 0.0],
                rotation: [0.0; 3],
                scale: [1.0; 3],
            };
            3
        ]);
        let mut playback = Playback::new(vec![Clip {
            slot: 0,
            object: "rig".into(),
            name: "move".into(),
            animation: 1,
            fps: 30.0,
            length: 2.0,
            rate: 0.0,
        }]);
        playback.install_rig(0, "rig".into(), mesh);
        playback.blend(0, Some(blend)).unwrap();
        assert_eq!(playback.bone_matrix(0, 0, true).unwrap()[12], parent_x);
        assert_eq!(playback.bone_matrix(0, 1, true).unwrap()[12], local_x);
        assert_eq!(playback.bone_matrix(0, 1, false).unwrap()[12], world_x);
        let mut parent = playback.bone_matrix(0, 0, false).unwrap();
        parent[12] = 80.0;
        assert!(playback.set_bone_matrix(0, 0, parent, false));
        assert_eq!(playback.bone_matrix(0, 1, false).unwrap()[12], world_x);
        let mut child = playback.bone_matrix(0, 1, true).unwrap();
        child[12] += 1.0;
        assert!(playback.set_bone_matrix(0, 1, child, true));
        assert_eq!(playback.bone_matrix(0, 1, false).unwrap()[12], 80.0 + 2.0 * (local_x + 1.0));
        playback.reset_bones();
        assert_eq!(playback.bone_matrix(0, 1, true).unwrap()[12], local_x);
        assert_eq!(playback.bone_matrix(0, 1, false).unwrap()[12], world_x);
    }
}
#[test]
fn bone_reset_keeps_morphs() {
    for paused in [false, true] {
        let mesh = crate::puppet::parse(&crate::puppet::tests::fixture()).unwrap();
        let clips = if paused {
            vec![Clip {
                slot: 0,
                object: "rig".into(),
                name: "move".into(),
                animation: 1,
                fps: 30.0,
                length: 2.0,
                rate: 1.0,
            }]
        } else {
            Vec::new()
        };
        let mut playback = Playback::new(clips);
        playback.install_rig(0, "rig".into(), mesh);
        if paused {
            playback.control(0, Control::Pause, 0.0).unwrap();
        }
        let original = playback.bone_matrix(0, 0, false).unwrap();
        let mut edited = original;
        edited[12] += 8.0;
        playback.rigs.get_mut(&0).unwrap().weights = Some(vec![0.75]);
        assert!(playback.set_bone_matrix(0, 0, edited, false));
        playback.reset_bones();
        playback.advance(0.25);
        assert_eq!(playback.bone_matrix(0, 0, false), Some(original));
        assert_eq!(playback.morph_weights("rig"), Some(vec![0.75]));
        playback.invalidate_rig(0);
        assert_eq!(playback.morph_weights("rig"), None);
    }
}

fn bone_playback() -> Playback {
    let mesh = crate::puppet::parse(&crate::puppet::tests::fixture()).unwrap();
    let mut playback = Playback::new(vec![Clip {
        slot: 0,
        object: "rig".into(),
        name: "move".into(),
        animation: 1,
        fps: 30.0,
        length: 2.0,
        rate: 0.0,
    }]);
    playback.install_rig(0, "rig".into(), mesh);
    playback
}

#[test]
fn bone_frame_snapshot() {
    let mut immediate = bone_playback();
    immediate.blend(0, Some(0.0));
    let rest = immediate.bone_matrix(0, 0, false).unwrap();
    immediate.control(0, Control::SetFrame, 1.0);
    immediate.blend(0, Some(1.0));
    let animated = immediate.bone_matrix(0, 0, false).unwrap();
    assert_ne!(rest, animated);

    let mut playback = bone_playback();
    playback.blend(0, Some(0.0));
    playback.bone_frame(0.0);
    assert!(playback.rigs[&0].world.is_empty());
    playback.control(0, Control::SetFrame, 1.0);
    playback.blend(0, Some(1.0));
    assert_eq!(playback.control(0, Control::GetFrame, 0.0), Some(1.0));
    assert_eq!(playback.bone_matrix(0, 0, false), Some(rest));
    playback.bone_frame(0.0);
    assert_eq!(playback.bone_matrix(0, 0, false), Some(animated));
}

#[test]
fn bone_edit_snapshot() {
    let mut playback = bone_playback();
    playback.bone_frame(0.0);
    let local = playback.bone_matrix(0, 0, true).unwrap();
    let mut edited = local;
    edited[12] = 1000.0;
    assert!(playback.set_bone_matrix(0, 0, edited, false));
    playback.rigs.get_mut(&0).unwrap().weights = Some(vec![0.75]);
    playback.control(0, Control::SetFrame, 1.0);
    playback.blend(0, Some(0.5));
    assert!(playback.rigs[&0].weights.is_none());
    assert_eq!(playback.bone_matrix(0, 0, false), Some(edited));
    assert_eq!(playback.bone_matrix(0, 0, true), Some(local));
    assert!(playback.bone_palette("rig").is_some());
    let samples = playback.rigs[&0].samples.as_ptr();
    playback.bone_frame(0.0);
    assert_ne!(playback.bone_matrix(0, 0, false), Some(edited));
    assert!(playback.bone_palette("rig").is_none());
    let world = playback.rigs[&0].world.as_ptr();
    let locals = playback.rigs[&0].locals.as_ptr();
    for _ in 0..100 {
        playback.bone_frame(0.0);
        assert!(playback.rigs[&0].resolved);
        assert_eq!(playback.rigs[&0].world.as_ptr(), world);
        assert_eq!(playback.rigs[&0].locals.as_ptr(), locals);
        assert_eq!(playback.rigs[&0].samples.as_ptr(), samples);
    }
}

#[test]
fn idle_bone_snapshot() {
    let mut clipless = Playback::default();
    clipless.install_rig(0, "rig".into(), crate::puppet::tests::hierarchy_fixture());
    for mut playback in [bone_playback(), clipless] {
        playback.control(0, Control::Pause, 0.0);
        playback.bone_frame(0.0);
        let pose = playback.bone_matrix(0, 0, false);
        let world = playback.rigs[&0].world.as_ptr();
        let locals = playback.rigs[&0].locals.as_ptr();
        for _ in 0..100 {
            playback.bone_frame(0.25);
            assert!(playback.rigs[&0].resolved);
            assert_eq!(playback.rigs[&0].world.as_ptr(), world);
            assert_eq!(playback.rigs[&0].locals.as_ptr(), locals);
            assert_eq!(playback.bone_matrix(0, 0, false), pose);
        }
    }
}

#[test]
fn render_state_snapshot() {
    let mut playback = bone_playback();
    playback.bone_frame(0.0);
    let first = playback.states("rig");
    playback.control(0, Control::SetFrame, 1.0);
    playback.blend(0, Some(0.5));
    assert_eq!(playback.states("rig"), first);
    assert_eq!(playback.sample("rig", 1), Some(first[0].sample));
    assert_eq!(playback.control(0, Control::GetFrame, 0.0), Some(1.0));
    assert_eq!(playback.blend(0, None), Some(0.5));
    playback.bone_frame(0.0);
    let next = playback.states("rig");
    assert_ne!(next, first);
    assert_eq!(next[0].layer.blend, 0.5);
    assert_eq!(playback.sample("rig", 1), Some(next[0].sample));
    let samples = next
        .iter()
        .map(|state| {
            let Sample::Frame(seconds) = state.sample else { panic!("active sample") };
            (state.layer, seconds)
        })
        .collect::<Vec<_>>();
    let mesh = bone_playback().rigs.remove(&0).unwrap().mesh;
    let mut sampler = crate::puppet::sampler::Sampler::new(std::rc::Rc::new(mesh)).unwrap();
    let palette = sampler.sample(&samples, 0.0).unwrap().matrices.to_vec();
    let world = playback.bone_matrix(0, 0, false).unwrap();
    let expected = crate::puppet::palette_world(&playback.rigs[&0].mesh, &[world]).unwrap();
    assert_eq!(palette, expected);
    playback.visibility(0, Some(false));
    playback.bone_frame(0.0);
    assert_eq!(playback.states("rig")[0].sample, Sample::Rest);
    assert_eq!(playback.sample("rig", 1), Some(Sample::Rest));
    playback.retire(0);
    assert!(playback.states("rig").is_empty());
    assert_eq!(playback.sample("rig", 1), Some(Sample::Retired));
}

#[test]
fn spring_frame_edits() {
    let mut mesh = crate::puppet::tests::hierarchy_fixture();
    mesh.springs.push(crate::puppet::spring::Definition {
        bone: 0,
        translation: Some(crate::puppet::spring::Translation {
            stiffness: 8.0,
            friction: 2.0,
            inertia: 0.5,
            maximum: 0.0,
        }),
        rotation: None,
        gravity: None,
    });
    let mut playback = Playback::new(Vec::new());
    playback.install_rig(0, "spring".into(), mesh);
    let owner = crate::matrix::model_matrix([0.0; 3], [1.0; 3], [0.0; 3]);
    assert!(playback.animated());
    assert!(!playback.is_empty());
    playback.bone_frame(0.125);
    assert!(playback.step_springs(0, owner, 0.125));
    let local = playback.bone_matrix(0, 0, true).unwrap();
    let mut edited = playback.bone_matrix(0, 0, false).unwrap();
    edited[12] = 8.0;
    assert!(playback.set_bone_matrix(0, 0, edited, false));
    playback.record_springs(0, 0, false, &owner);
    assert_eq!(playback.bone_matrix(0, 0, false).unwrap()[12], 8.0);
    assert_eq!(playback.bone_matrix(0, 0, true).unwrap(), local);
    playback.bone_frame(0.125);
    assert!(playback.step_springs(0, owner, 0.125));
    assert_eq!(playback.bone_matrix(0, 0, false).unwrap()[12], 3.5);
    assert_eq!(playback.bone_matrix(0, 1, false).unwrap()[12], 8.5);
    assert_eq!(playback.bone_matrix(0, 2, false).unwrap()[12], 20.0);
    assert_eq!(playback.bone_matrix(0, 0, true).unwrap(), local);
    assert!(playback.bone_palette("spring").is_some());
    playback.retire(0);
    assert!(!playback.has_springs(0));
}

#[test]
fn hidden_zero_bone_snapshot() {
    let mut playback = bone_playback();
    let raw = crate::matrix::model_matrix([3.0, 4.0, 0.0], [1.0; 3], [0.0, 0.0, 0.25]);
    let mesh = &mut playback.rigs.get_mut(&0).unwrap().mesh;
    mesh.bones[0] = crate::puppet::Bone::from_matrix("root".into(), None, raw, false).unwrap();
    mesh.native_bind_inverse = crate::puppet::pose::bind_inverse(&mesh.bones);
    playback.blend(0, Some(0.0));
    playback.visibility(0, Some(false));
    assert_eq!(playback.bone_matrix(0, 0, true), Some(raw));
    playback.visibility(0, Some(true));
    let active = playback.bone_matrix(0, 0, true).unwrap();
    #[cfg(target_arch = "x86_64")]
    assert_ne!(active, raw);
    #[cfg(not(target_arch = "x86_64"))]
    assert_eq!(active, raw);
    playback.bone_frame(0.0);
    playback.visibility(0, Some(false));
    assert_eq!(playback.bone_matrix(0, 0, true), Some(active));
    playback.bone_frame(0.0);
    assert_eq!(playback.bone_matrix(0, 0, true), Some(raw));
    assert_eq!(playback.states("rig")[0].sample, Sample::Rest);
    assert_eq!(playback.sample("rig", 1), Some(Sample::Rest));
}
