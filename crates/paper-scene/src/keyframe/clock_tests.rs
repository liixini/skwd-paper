use super::{Clock, Clocks, Control, Mode};

fn clocks() -> Clocks {
    Clocks {
        entries: vec![Clock {
            fps: 10.0,
            length: 20.0,
            mode: Mode::Single,
            paused: true,
            finished: false,
            reversed: false,
            seconds: 0.0,
            since: 0.0,
            rate: 1.0,
        }],
        time: 0.0,
        pending: false,
    }
}

#[test]
fn pause_retains_position() {
    let mut clocks = clocks();
    clocks.control(0, Control::SetFrame, 10.0);
    clocks.control(0, Control::Play, 0.0);
    clocks.time = 0.5;
    clocks.control(0, Control::Pause, 0.0);
    clocks.time = 2.0;
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(15.0));
    clocks.control(0, Control::Play, 0.0);
    clocks.time = 2.2;
    assert!((clocks.control(0, Control::GetFrame, 0.0).unwrap() - 17.0).abs() < 0.001);
}

#[test]
fn seek_keeps_raw_frame() {
    let mut clocks = clocks();
    for frame in [-5.0, 10.0, 100.0] {
        clocks.control(0, Control::SetFrame, frame);
        assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(frame));
    }
    clocks.control(0, Control::Stop, 0.0);
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(0.0));
}

#[test]
fn paused_modes_sample_raw() {
    for mode in [Mode::Single, Mode::Loop, Mode::Mirror] {
        let mut clocks = clocks();
        clocks.entries[0].mode = mode;
        clocks.entries[0].fps = 8.0;
        clocks.entries[0].length = 16.0;
        for (frame, sample) in [
            (-0.5, (0.0, -0.5)),
            (0.0, (0.0, 0.0)),
            (15.5, (15.0, 0.5)),
            (16.0, (15.0, 0.0)),
            (16.5, (15.0, 0.5)),
            (32.5, (15.0, 0.5)),
        ] {
            clocks.control(0, Control::SetFrame, frame);
            clocks.advance(1.0);
            assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(frame));
            assert_eq!(clocks.entries[0].sample(clocks.time), sample);
        }
    }
}

#[test]
fn zero_rate_wraps_boundaries() {
    for (mode, frame, expected) in [
        (Mode::Loop, -0.5, 15.5),
        (Mode::Loop, 16.0, 0.0),
        (Mode::Loop, 32.5, 0.5),
        (Mode::Mirror, -0.5, -0.5),
        (Mode::Mirror, 16.0, 16.0),
        (Mode::Mirror, 32.5, 15.5),
    ] {
        let mut clocks = clocks();
        clocks.entries[0].mode = mode;
        clocks.entries[0].fps = 8.0;
        clocks.entries[0].length = 16.0;
        clocks.control(0, Control::SetRate, 0.0);
        clocks.control(0, Control::Play, 0.0);
        clocks.control(0, Control::SetFrame, frame);
        for tick in 1..=4 {
            clocks.advance(tick as f32 * 0.25);
        }
        assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(expected));
        assert_eq!(clocks.control(0, Control::IsPlaying, 0.0), Some(1.0));
        assert_eq!(clocks.control(0, Control::GetRate, 0.0), Some(0.0));
    }
}

#[test]
fn mirror_preserves_direction() {
    let mut clocks = clocks();
    clocks.entries[0].mode = Mode::Mirror;
    clocks.entries[0].fps = 2.0;
    clocks.entries[0].length = 1.0;
    clocks.control(0, Control::SetRate, 0.0);
    clocks.control(0, Control::Play, 0.0);
    clocks.control(0, Control::SetFrame, 1.0);
    clocks.advance(0.0);
    clocks.control(0, Control::Pause, 0.0);
    clocks.control(0, Control::SetFrame, 0.5);
    clocks.control(0, Control::Play, 0.0);
    clocks.control(0, Control::SetRate, 1.0);
    clocks.advance(0.125);
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(0.25));
    clocks.control(0, Control::Stop, 0.0);
    clocks.control(0, Control::SetFrame, 0.5);
    clocks.control(0, Control::Play, 0.0);
    clocks.advance(0.25);
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(0.75));
}

#[test]
fn mirror_crosses_each_edge() {
    let mut clocks = clocks();
    clocks.entries[0].mode = Mode::Mirror;
    clocks.entries[0].fps = 8.0;
    clocks.entries[0].length = 16.0;
    clocks.control(0, Control::SetFrame, 15.0);
    clocks.control(0, Control::Play, 0.0);
    clocks.advance(0.25);
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(15.0));
    assert!(clocks.entries[0].reversed);
    clocks.advance(2.25);
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(1.0));
    assert!(!clocks.entries[0].reversed);
}

#[test]
fn rate_preserves_position() {
    let mut clocks = clocks();
    clocks.control(0, Control::Play, 0.0);
    clocks.time = 0.5;
    clocks.control(0, Control::SetRate, 2.0);
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(5.0));
    clocks.time = 1.0;
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(15.0));
    clocks.control(0, Control::SetRate, 0.0);
    clocks.time = 10.0;
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(15.0));
    assert_eq!(clocks.control(0, Control::IsPlaying, 0.0), Some(1.0));
}

#[test]
fn single_finishes_on_advance() {
    let mut clocks = clocks();
    clocks.control(0, Control::Play, 0.0);
    clocks.advance(3.0);
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(20.0));
    assert_eq!(clocks.control(0, Control::IsPlaying, 0.0), Some(0.0));
    clocks.control(0, Control::Play, 0.0);
    assert_eq!(clocks.control(0, Control::IsPlaying, 0.0), Some(1.0));
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(0.0));
    clocks.advance(3.1);
    assert_eq!(clocks.control(0, Control::IsPlaying, 0.0), Some(1.0));
}

#[test]
fn paused_seek_survives_advance() {
    let mut clocks = clocks();
    clocks.control(0, Control::SetFrame, 100.0);
    clocks.advance(1.0);
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(100.0));
    clocks.control(0, Control::Play, 0.0);
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(100.0));
    clocks.advance(1.1);
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(100.0));
    assert_eq!(clocks.control(0, Control::IsPlaying, 0.0), Some(1.0));
}

#[test]
fn finished_seek_restarts() {
    let mut clocks = clocks();
    clocks.control(0, Control::Play, 0.0);
    clocks.advance(3.0);
    clocks.control(0, Control::SetFrame, 5.0);
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(5.0));
    assert_eq!(clocks.control(0, Control::IsPlaying, 0.0), Some(0.0));
    clocks.control(0, Control::Pause, 0.0);
    clocks.control(0, Control::Play, 0.0);
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(0.0));
    assert_eq!(clocks.control(0, Control::IsPlaying, 0.0), Some(1.0));
    clocks.control(0, Control::Stop, 0.0);
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(0.0));
    assert_eq!(clocks.control(0, Control::IsPlaying, 0.0), Some(0.0));
}

#[test]
fn seek_schedules_one_sample() {
    let mut scene = serde_json::json!({"value": 1, "animation": {
        "options": {"fps": 10, "length": 20, "mode": "single", "startpaused": true},
        "c0": [{"frame": 0, "value": 1}, {"frame": 20, "value": 2}]
    }});
    let mut bindings = super::Bindings::parse(&mut scene).unwrap();
    bindings.apply(&mut scene, 0.0);
    assert!(!bindings.animated());
    bindings.clocks.borrow_mut().control(0, Control::SetFrame, 10.0);
    assert!(bindings.animated());
    assert!(bindings.apply(&mut scene, 0.0));
    assert!(!bindings.animated());
    bindings.clocks.borrow_mut().control(0, Control::SetFrame, 20.0);
    bindings.apply(&mut scene, 0.0);
    bindings.clocks.borrow_mut().control(0, Control::Play, 0.0);
    assert!(bindings.animated());
    assert_eq!(bindings.clocks.borrow_mut().control(0, Control::IsPlaying, 0.0), Some(1.0));
    bindings.apply(&mut scene, 0.1);
    assert!(!bindings.animated());
    assert_eq!(bindings.clocks.borrow_mut().control(0, Control::IsPlaying, 0.0), Some(1.0));
}

#[test]
fn seek_roundtrips_native_seconds() {
    let mut clocks = clocks();
    let frame = 10.5_f32;
    let period = 1.0_f32 / 10.0;
    clocks.control(0, Control::SetFrame, frame);
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some((frame * period) / period));
}

#[test]
fn reverse_clock_keeps_ticking() {
    let mut scene = serde_json::json!({"value":0,"animation":{"options":{
        "fps":10,"length":20,"mode":"single","startpaused":true},
        "c0":[{"frame":0,"value":0},{"frame":20,"value":1}]}});
    let mut bindings = super::Bindings::parse(&mut scene).unwrap();
    {
        let mut clocks = bindings.clocks.borrow_mut();
        clocks.control(0, Control::SetFrame, 5.0);
        clocks.control(0, Control::SetRate, -1.0);
        clocks.control(0, Control::Play, 0.0);
    }
    bindings.apply(&mut scene, 1.0);
    assert!(bindings.animated());
    let mut clocks = bindings.clocks.borrow_mut();
    assert_eq!(clocks.control(0, Control::IsPlaying, 0.0), Some(1.0));
    assert_eq!(clocks.control(0, Control::GetFrame, 0.0), Some(-5.0));
}
