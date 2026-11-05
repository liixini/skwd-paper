use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use ffmpeg_the_third as ff;
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Split};

use super::*;
use crate::test_pcm::pcm_wav;

fn ring(capacity: usize) -> (RingProducer, ringbuf::HeapCons<f32>) {
    HeapRb::<f32>::new(capacity).split()
}

struct Flow {
    mute: AtomicBool,
    paused: AtomicBool,
    wake: (Mutex<()>, Condvar),
}

static FLOW: std::sync::LazyLock<Flow> = std::sync::LazyLock::new(|| Flow {
    mute: AtomicBool::new(false),
    paused: AtomicBool::new(false),
    wake: (Mutex::new(()), Condvar::new()),
});
static HOLD: AtomicBool = AtomicBool::new(false);
static RESTART: AtomicBool = AtomicBool::new(false);

fn open_gate(flow: &Flow) -> ParkGate<'_> {
    ParkGate {
        mute: &flow.mute,
        paused: &flow.paused,
        hold: &HOLD,
        restart: &RESTART,
        wake: &flow.wake,
    }
}

fn drain(consumer: &mut ringbuf::HeapCons<f32>) -> Vec<f32> {
    let mut output = Vec::new();
    while let Some(value) = consumer.try_pop() {
        output.push(value);
    }
    output
}

fn planar_stereo(left: &[f32], right: &[f32]) -> ff::frame::Audio {
    let mut frame = ff::frame::Audio::new(
        ff::format::Sample::F32(ff::format::sample::Type::Planar),
        left.len(),
        ff::ChannelLayoutMask::STEREO,
    );
    frame.plane_mut::<f32>(0).copy_from_slice(left);
    frame.plane_mut::<f32>(1).copy_from_slice(right);
    frame
}

fn decode_wav(channels: u16, channel_mask: Option<u32>) -> Vec<f32> {
    let samples = decode_fixture(&pcm_wav(channels, channel_mask), "wav", Repeat::Once);
    assert_eq!(samples.len(), 9_600, "a complete PCM tone must reach the mixer");
    samples
}

fn decode_fixture(bytes: &[u8], extension: &str, repeat: Repeat) -> Vec<f32> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(format!("tail.{extension}"));
    std::fs::write(&path, bytes).unwrap();
    let (mut producer, mut consumer) = ring(48_000);
    let stopped = AtomicBool::new(false);
    let restart = decode_loop(
        path.to_str().unwrap(),
        &mut producer,
        &stopped,
        &FLOW.mute,
        &FLOW.paused,
        &HOLD,
        &RESTART,
        &FLOW.wake,
        repeat,
    )
    .unwrap();
    assert!(!restart);
    drain(&mut consumer)
}

fn reference(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap())).collect()
}

fn max_error(actual: &[f32], expected: &[f32]) -> f32 {
    assert!(actual.iter().chain(expected).all(|sample| sample.is_finite()));
    actual
        .iter()
        .zip(expected)
        .map(|(actual, expected)| (actual - expected).abs())
        .fold(0.0, f32::max)
}

fn ffmpeg_pcm(media: &[u8], extension: &str) -> Vec<f32> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(format!("tail.{extension}"));
    std::fs::write(&path, media).unwrap();
    let output = std::process::Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-ac", "2", "-ar", "48000", "-f", "f32le", "pipe:1"])
        .output()
        .expect("run ffmpeg reference");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!output.stdout.is_empty());
    assert_eq!(output.stdout.len() % std::mem::size_of::<f32>(), 0);
    reference(&output.stdout)
}

const TAIL_FIXTURES: [(&str, &[u8]); 4] = [
    ("wav", include_bytes!("fixtures/tail-44100.wav")),
    ("m4a", include_bytes!("fixtures/tail-aac.m4a")),
    ("ogg", include_bytes!("fixtures/tail-vorbis.ogg")),
    ("opus", include_bytes!("fixtures/tail-opus.ogg")),
];

#[test]
fn pcm_tail_matches_golden() {
    let actual = decode_fixture(include_bytes!("fixtures/tail-44100.wav"), "wav", Repeat::Once);
    let expected = reference(include_bytes!("fixtures/tail-44100.f32"));
    assert_eq!(actual.len(), expected.len());
    assert!(max_error(&actual, &expected) <= 0.00001);
}

#[test]
fn compressed_tail_matches_ffmpeg() {
    for (extension, media) in TAIL_FIXTURES {
        let actual = decode_fixture(media, extension, Repeat::Once);
        let expected = ffmpeg_pcm(media, extension);
        assert_eq!(actual.len(), expected.len(), "{extension} sample count");
        let error = max_error(&actual, &expected);
        assert!(error <= 0.00001, "{extension} max error: {error}");
    }
}

#[test]
fn loop_preserves_tail() {
    for (extension, media) in TAIL_FIXTURES {
        check_loop(extension, media);
    }
}

fn check_loop(extension: &str, media: &[u8]) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(format!("tail.{extension}"));
    std::fs::write(&path, media).unwrap();
    let expected = ffmpeg_pcm(media, extension);
    let needed = expected.len() * 2;
    let (mut producer, mut consumer) = ring(48_000);
    let stopped = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stopped);
    let path = path.to_str().unwrap().to_owned();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = decode_loop(
            &path,
            &mut producer,
            &thread_stop,
            &FLOW.mute,
            &FLOW.paused,
            &HOLD,
            &RESTART,
            &FLOW.wake,
            Repeat::Forever,
        );
        done_tx.send(result).unwrap();
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut actual = Vec::with_capacity(needed);
    while actual.len() < needed && std::time::Instant::now() < deadline {
        if let Some(sample) = consumer.try_pop() {
            actual.push(sample);
        } else {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    signal_stop(&stopped, &FLOW.wake);
    assert!(!done_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap().unwrap());
    worker.join().unwrap();
    assert_eq!(actual.len(), needed);
    for cycle in actual.chunks_exact(expected.len()) {
        let error = max_error(cycle, &expected);
        assert!(error <= 0.00001, "{extension} loop error: {error}");
    }
}

#[test]
fn empty_loop_finishes() {
    let mut wav = pcm_wav(2, None);
    wav.truncate(44);
    wav[4..8].copy_from_slice(&36_u32.to_le_bytes());
    wav[40..44].copy_from_slice(&0_u32.to_le_bytes());
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        done_tx.send(decode_fixture(&wav, "wav", Repeat::Forever)).unwrap();
    });
    assert!(done_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap().is_empty());
    worker.join().unwrap();
}

#[test]
fn paused_decode_stops() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tail.wav");
    std::fs::write(&path, include_bytes!("fixtures/tail-44100.wav")).unwrap();
    let flow = Arc::new(Flow {
        mute: AtomicBool::new(false),
        paused: AtomicBool::new(true),
        wake: (Mutex::new(()), Condvar::new()),
    });
    let (mut producer, mut consumer) = ring(48_000);
    let stopped = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stopped);
    let thread_flow = Arc::clone(&flow);
    let path = path.to_str().unwrap().to_owned();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = decode_loop(
            &path,
            &mut producer,
            &thread_stop,
            &thread_flow.mute,
            &thread_flow.paused,
            &HOLD,
            &RESTART,
            &thread_flow.wake,
            Repeat::Forever,
        );
        done_tx.send(result).unwrap();
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(drain(&mut consumer).is_empty());
    signal_stop(&stopped, &flow.wake);
    assert!(!done_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap().unwrap());
    worker.join().unwrap();
}

#[test]
fn pcm_wav_without_channel_mask_produces_stereo_sound() {
    for channels in [1, 2] {
        let samples = decode_wav(channels, None);
        let rms = (samples.iter().map(|sample| sample * sample).sum::<f32>()
            / samples.len() as f32)
            .sqrt();
        assert!((0.12..0.19).contains(&rms), "audible PCM tone: channels={channels}, rms={rms}");
        for frame in samples.chunks_exact(2) {
            let expected_right = if channels == 1 { frame[0] } else { -frame[0] };
            assert!((frame[1] - expected_right).abs() < 0.0001);
        }
    }
}

#[test]
fn pcm_wav_preserves_explicit_quad_channel_layout() {
    let samples = decode_wav(4, Some(0x33));
    let peak = samples.iter().map(|sample| sample.abs()).fold(0.0_f32, f32::max);
    assert!(peak > 0.24, "quad tone is missing: {peak}");
    for frame in samples.chunks_exact(2) {
        assert!((frame[0] + frame[1]).abs() < 0.0001, "opposite left/right tones were remapped");
    }
}

#[test]
fn stop_wakes_parked() {
    let wake = Arc::new((Mutex::new(()), Condvar::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let (checked_tx, checked_rx) = std::sync::mpsc::channel();
    let (woke_tx, woke_rx) = std::sync::mpsc::channel();
    let (thread_wake, thread_stop) = (Arc::clone(&wake), Arc::clone(&stop));
    let parker = std::thread::spawn(move || {
        let guard = thread_wake.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let park = should_park(true, false, thread_stop.load(Ordering::Relaxed));
        checked_tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        if park {
            let _unused = thread_wake.1.wait(guard);
        }
        woke_tx.send(()).unwrap();
    });
    checked_rx.recv().unwrap();
    signal_stop(&stop, &wake);
    assert!(woke_rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok());
    parker.join().unwrap();
}

#[test]
fn planar_interleaved() {
    let (mut producer, mut consumer) = ring(64);
    let frame = planar_stereo(&[1.0, 2.0, 3.0, 4.0], &[10.0, 20.0, 30.0, 40.0]);
    push_to_ring(
        &mut producer,
        &frame,
        &mut Vec::new(),
        &AtomicBool::new(false),
        &open_gate(&FLOW),
    );
    assert_eq!(drain(&mut consumer), vec![1.0, 10.0, 2.0, 20.0, 3.0, 30.0, 4.0, 40.0]);
}

#[test]
fn invalid_frames_dropped() {
    let (mut producer, mut consumer) = ring(64);
    let non_f32 = ff::frame::Audio::new(
        ff::format::Sample::I16(ff::format::sample::Type::Planar),
        4,
        ff::ChannelLayoutMask::STEREO,
    );
    push_to_ring(
        &mut producer,
        &non_f32,
        &mut Vec::new(),
        &AtomicBool::new(false),
        &open_gate(&FLOW),
    );
    push_to_ring(
        &mut producer,
        &ff::frame::Audio::empty(),
        &mut Vec::new(),
        &AtomicBool::new(false),
        &open_gate(&FLOW),
    );
    let frame = planar_stereo(&[1.0, 2.0], &[3.0, 4.0]);
    push_to_ring(&mut producer, &frame, &mut Vec::new(), &AtomicBool::new(true), &open_gate(&FLOW));
    assert!(drain(&mut consumer).is_empty());
}

#[test]
fn park_policy_observes_stop() {
    assert!(should_park(true, false, false));
    assert!(should_park(false, true, false));
    assert!(!should_park(false, false, false));
    assert!(!should_park(true, false, true));
}

#[test]
fn paused_full_ring_parks() {
    let flow = Arc::new(Flow {
        mute: AtomicBool::new(false),
        paused: AtomicBool::new(true),
        wake: (Mutex::new(()), Condvar::new()),
    });
    let (mut producer, mut consumer) = ring(4);
    let filler = planar_stereo(&[9.0, 9.0], &[9.0, 9.0]);
    push_to_ring(
        &mut producer,
        &filler,
        &mut Vec::new(),
        &AtomicBool::new(false),
        &open_gate(&flow),
    );
    let writer_flow = flow.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        let frame = planar_stereo(&[1.0, 2.0], &[3.0, 4.0]);
        let mut producer = producer;
        push_to_ring(
            &mut producer,
            &frame,
            &mut Vec::new(),
            &AtomicBool::new(false),
            &open_gate(&writer_flow),
        );
        done_tx.send(()).unwrap();
    });
    assert!(done_rx.recv_timeout(std::time::Duration::from_millis(200)).is_err());
    flow.paused.store(false, Ordering::Relaxed);
    wake_parked(&flow.wake);
    assert_eq!(drain(&mut consumer).len(), 4);
    done_rx.recv_timeout(std::time::Duration::from_secs(5)).expect("unpaused producer finishes");
    writer.join().unwrap();
    drop(flow);
}
