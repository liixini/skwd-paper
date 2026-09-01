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

fn open_gate(flow: &Flow) -> ParkGate<'_> {
    ParkGate { mute: &flow.mute, paused: &flow.paused, wake: &flow.wake }
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
