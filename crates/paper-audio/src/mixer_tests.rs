#![cfg(test)]

use super::*;

#[test]
fn gain_round_trip() {
    for gain in [0.0_f32, 0.5, 1.0, 1.3, 4.0] {
        assert!((f32::from_bits(gain_bits(gain)) - gain).abs() < f32::EPSILON);
    }
    assert!((f32::from_bits(gain_bits(-2.0)) - 0.0).abs() < f32::EPSILON);
}

#[test]
fn random_advances() {
    let mut state = seed_from_clock(3);
    assert_ne!(state, 0);
    let first = next_random(&mut state);
    let second = next_random(&mut state);
    assert_ne!(first, second);
    assert_ne!(first, 0);
}

#[test]
fn seed_never_zero() {
    for salt in [0_u64, 1, u64::MAX] {
        assert_ne!(seed_from_clock(salt), 0);
    }
}

#[test]
fn stopped_voice_returns_fast() {
    let wake = (Mutex::new(()), Condvar::new());
    let stop = AtomicBool::new(true);
    let started = std::time::Instant::now();
    park(&wake, &stop, Duration::from_secs(30));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn partial_mixer_construction_owns_and_joins_voice_workers() {
    let stop = Arc::new(AtomicBool::new(false));
    let wake = Arc::new((Mutex::new(()), Condvar::new()));
    let thread_stop = stop.clone();
    let thread_wake = wake.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let guard = thread_wake.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut guard = guard;
        while !thread_stop.load(Ordering::Relaxed) {
            guard = thread_wake.1.wait(guard).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        done_tx.send(()).unwrap();
    });
    let control = VoiceControl {
        gain: Arc::new(AtomicU32::new(gain_bits(1.0))),
        stop: stop.clone(),
        wake,
        thread: Some(thread),
    };

    drop(control);

    assert!(stop.load(Ordering::Relaxed));
    assert!(done_rx.recv_timeout(Duration::from_secs(1)).is_ok());
}

#[test]
fn unplayable_clips_no_mixer() {
    let voices = vec![Voice {
        name: "missing".into(),
        clips: vec!["/nonexistent/skwd-test-clip.mp3".into()],
        gain: 1.0,
        mode: VoiceMode::Loop,
        min_gap: 0.0,
        max_gap: 0.0,
    }];
    assert!(SceneMixer::new(&voices, true, 50).unwrap().is_none());
    assert!(SceneMixer::new(&[], true, 50).unwrap().is_none());
    assert!(!playable("/nonexistent/skwd-test-clip.mp3"));
}

fn tone(index: usize) -> Option<String> {
    let path = std::env::var("SKWD_AUDIO_TEST_CLIPS").ok()?;
    let clip = path.split(',').nth(index)?.trim().to_string();
    std::path::Path::new(&clip).is_file().then_some(clip)
}

#[test]
#[ignore = "needs an output device: SKWD_AUDIO_TEST_CLIPS=a.wav,b.wav cargo test -p paper-audio -- --ignored"]
fn real_clips_one_stream() {
    let (Some(first), Some(second)) = (tone(0), tone(1)) else {
        panic!("set SKWD_AUDIO_TEST_CLIPS to two decodable audio files");
    };
    assert!(playable(&first) && playable(&second));

    let voices = vec![
        Voice {
            name: "a".into(),
            clips: vec![first],
            gain: 1.0,
            mode: VoiceMode::Loop,
            min_gap: 0.0,
            max_gap: 0.0,
        },
        Voice {
            name: "b".into(),
            clips: vec![second],
            gain: 0.4,
            mode: VoiceMode::Once,
            min_gap: 0.0,
            max_gap: 0.0,
        },
    ];
    let mixer = SceneMixer::new(&voices, true, 60).unwrap().expect("mixer built");
    assert_eq!(mixer.voice_count(), 2);
    mixer.set_mute(false);
    mixer.set_volume(30);
    std::thread::sleep(Duration::from_millis(400));
    mixer.set_pause(true);
    mixer.set_pause(false);
    drop(mixer);
}
