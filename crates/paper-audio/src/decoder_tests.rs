use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use ffmpeg_the_third as ff;
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Split};

use super::*;

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

fn pcm_wav(channels: u16, channel_mask: Option<u32>) -> Vec<u8> {
    let frames = 4_800;
    let format_size = if channel_mask.is_some() { 40_u32 } else { 16 };
    let data_size = frames * u32::from(channels) * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(20 + format_size + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&format_size.to_le_bytes());
    bytes.extend_from_slice(&if channel_mask.is_some() { 0xfffe_u16 } else { 1 }.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&(48_000 * u32::from(channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    if let Some(mask) = channel_mask {
        bytes.extend_from_slice(&22_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(&mask.to_le_bytes());
        bytes.extend_from_slice(&[1, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113]);
    }
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    for index in 0..frames {
        let sample = ((index as f32 * std::f32::consts::TAU / 100.0).sin() * 8_192.0) as i16;
        for channel in 0..channels {
            bytes.extend_from_slice(&if channel % 2 == 0 { sample } else { -sample }.to_le_bytes());
        }
    }
    bytes
}

fn decode_wav(channels: u16, channel_mask: Option<u32>) -> Vec<f32> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tone.wav");
    std::fs::write(&path, pcm_wav(channels, channel_mask)).unwrap();
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
        Repeat::Once,
    )
    .unwrap();
    assert!(!restart);
    let samples = drain(&mut consumer);
    assert_eq!(samples.len(), 9_600, "a complete PCM tone must reach the mixer");
    samples
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
