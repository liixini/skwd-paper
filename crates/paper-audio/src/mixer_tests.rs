#![cfg(test)]

use super::*;
use std::io::{ErrorKind, Read};
use std::os::unix::fs::OpenOptionsExt;

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
        id: "1".into(),
        gain: Arc::new(AtomicU32::new(gain_bits(1.0))),
        stop: stop.clone(),
        hold: Arc::new(AtomicBool::new(false)),
        restart: Arc::new(AtomicBool::new(false)),
        flush: Arc::new(AtomicBool::new(false)),
        eof_pending: Arc::new(AtomicBool::new(false)),
        state: Arc::new(Mutex::new(VoiceState::default())),
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
        id: "1".into(),
        name: "missing".into(),
        clips: vec!["/nonexistent/skwd-test-clip.mp3".into()],
        gain: 1.0,
        mode: VoiceMode::Loop,
        min_gap: 0.0,
        max_gap: 0.0,
        autostart: true,
    }];
    assert!(
        SceneMixer::new(&voices, SceneConfig { mute: true, volume: 50, wake: None, tap: None })
            .unwrap()
            .is_none()
    );
    assert!(
        SceneMixer::new(&[], SceneConfig { mute: true, volume: 50, wake: None, tap: None })
            .unwrap()
            .is_none()
    );
    assert!(!playable("/nonexistent/skwd-test-clip.mp3"));
}

struct ReceiverHarness {
    outputs: Vec<VoiceOutput>,
    control: VoiceControl,
    scratch: Vec<f32>,
    _directory: tempfile::TempDir,
}

const MAX_MIX_BLOCKS: usize = 64;
const TONE_SAMPLES: usize = 9_600;

impl ReceiverHarness {
    fn new(decode_hook: Option<Arc<dyn Fn(DecodeStage) + Send + Sync>>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("once.wav");
        std::fs::write(&path, crate::test_pcm::pcm_wav(2, None)).unwrap();
        let voice = Voice {
            id: "once".into(),
            name: "once".into(),
            clips: vec![path.to_str().unwrap().into()],
            gain: 1.0,
            mode: VoiceMode::Once,
            min_gap: 0.0,
            max_gap: 0.0,
            autostart: true,
        };
        let (mut producer, consumer) = HeapRb::<f32>::new(RING_FRAMES * 2).split();
        let gain = Arc::new(AtomicU32::new(gain_bits(1.0)));
        let stop = Arc::new(AtomicBool::new(false));
        let hold = Arc::new(AtomicBool::new(false));
        let restart = Arc::new(AtomicBool::new(false));
        let flush = Arc::new(AtomicBool::new(false));
        let eof_pending = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(VoiceState::default()));
        let wake = Arc::new((Mutex::new(()), Condvar::new()));
        let mute = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread_hold = hold.clone();
        let thread_restart = restart.clone();
        let thread_flush = flush.clone();
        let thread_eof_pending = eof_pending.clone();
        let thread_state = state.clone();
        let events = Arc::new(SceneEvents {
            pending: AtomicBool::new(false),
            wake: paper_runtime::wake::make_pipe(),
        });
        let thread_events = events.clone();
        let thread_wake = wake.clone();
        let thread = std::thread::spawn(move || {
            run_voice(
                &voice,
                0,
                &mut producer,
                &VoiceFlags {
                    stop: &thread_stop,
                    mute: &mute,
                    paused: &paused,
                    hold: &thread_hold,
                    restart: &thread_restart,
                    flush: &thread_flush,
                    eof_pending: &thread_eof_pending,
                    state: &thread_state,
                    events: &thread_events,
                    decode_hook: decode_hook
                        .as_deref()
                        .map(|hook| hook as &(dyn Fn(DecodeStage) + Sync)),
                },
                &thread_wake,
            );
        });
        let outputs = vec![VoiceOutput {
            consumer,
            gain: gain.clone(),
            hold: hold.clone(),
            flush: flush.clone(),
            eof_pending: eof_pending.clone(),
            state: state.clone(),
            decoder_thread: thread.thread().clone(),
            events,
        }];
        let control = VoiceControl {
            id: "once".into(),
            gain,
            stop,
            hold,
            restart,
            flush,
            eof_pending,
            state,
            wake,
            thread: Some(thread),
        };
        Self { outputs, control, scratch: Vec::new(), _directory: directory }
    }

    fn wait_for_eof(&self) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !self.control.eof_pending.load(Ordering::Acquire)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(self.control.eof_pending.load(Ordering::Acquire));
        assert!(self.control.is_playing());
        assert_eq!(self.outputs[0].consumer.occupied_len(), TONE_SAMPLES);
    }

    fn mix(&mut self, muted: bool, playback: Option<Playback>) -> [f32; 512] {
        let mut block = [0.0; 512];
        mix_outputs(&mut self.outputs, &mut block, &mut self.scratch, muted, 100, playback);
        block
    }

    fn command(&self, op: VoiceOp) {
        self.control.apply(op);
        self.control.wake();
    }

    fn reader(&self) -> std::fs::File {
        let wake = self.outputs[0].events.wake.as_ref().unwrap();
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(format!("/proc/self/fd/{}", wake.read_fd()))
            .unwrap()
    }
}

#[test]
fn decoder_eof_clears_voice() {
    let mut receiver = ReceiverHarness::new(None);
    receiver.wait_for_eof();
    let mut audible = 0;
    let mut blocks = 0;
    while receiver.control.is_playing() {
        blocks += 1;
        assert!(blocks <= MAX_MIX_BLOCKS);
        let block = receiver.mix(false, None);
        audible += block.iter().filter(|sample| sample.abs() > f32::EPSILON).count();
        let remaining = receiver.outputs[0].consumer.occupied_len();
        if receiver.control.is_playing() {
            assert!(remaining > 0);
        } else {
            assert_eq!(remaining, 0);
            assert!(block.iter().any(|sample| sample.abs() > f32::EPSILON));
        }
    }
    assert!(audible > 0);
}

#[test]
fn stopped_tail_replays_cleanly() {
    let mut receiver = ReceiverHarness::new(None);
    receiver.wait_for_eof();
    receiver.command(VoiceOp::Pause);
    let pending = receiver.outputs[0].consumer.occupied_len();
    assert!(receiver.mix(true, None).iter().all(|sample| *sample == 0.0));
    assert_eq!(receiver.outputs[0].consumer.occupied_len(), pending);
    receiver.command(VoiceOp::Play);
    assert!(receiver.mix(false, None).iter().any(|sample| sample.abs() > f32::EPSILON));
    receiver.command(VoiceOp::Stop);
    assert!(!receiver.control.is_playing());
    assert!(!receiver.control.eof_pending.load(Ordering::Acquire));
    let mut blocks = 0;
    while receiver.control.flush.load(Ordering::Relaxed) {
        blocks += 1;
        assert!(blocks <= MAX_MIX_BLOCKS);
        assert!(receiver.mix(false, None).iter().all(|sample| *sample == 0.0));
    }
    assert_eq!(receiver.outputs[0].consumer.occupied_len(), 0);
    receiver.command(VoiceOp::Play);
    receiver.wait_for_eof();
    let mut blocks = 0;
    while receiver.control.is_playing() {
        blocks += 1;
        assert!(blocks <= MAX_MIX_BLOCKS);
        receiver.mix(false, None);
    }
    assert_eq!(receiver.outputs[0].consumer.occupied_len(), 0);
}

struct DecodeGate {
    stages: std::sync::mpsc::Receiver<DecodeStage>,
    resume: std::sync::mpsc::SyncSender<()>,
}

impl DecodeGate {
    fn receiver() -> (Self, ReceiverHarness) {
        let (stage_tx, stages) = std::sync::mpsc::channel();
        let (resume, resume_rx) = std::sync::mpsc::sync_channel(0);
        let resume_rx = Mutex::new(resume_rx);
        let hook = Arc::new(move |stage| {
            stage_tx.send(stage).unwrap();
            resume_rx.lock().unwrap().recv_timeout(Duration::from_secs(3)).unwrap();
        });
        (Self { stages, resume }, ReceiverHarness::new(Some(hook)))
    }

    fn wait(&self, stage: DecodeStage) {
        assert_eq!(self.stages.recv_timeout(Duration::from_secs(3)).unwrap(), stage);
    }

    fn release(&self) {
        self.resume.send(()).unwrap();
    }
}

fn drain_tail(receiver: &mut ReceiverHarness) {
    let mut drained = 0;
    let mut audible = 0;
    for _ in 0..MAX_MIX_BLOCKS {
        let before = receiver.outputs[0].consumer.occupied_len();
        audible +=
            receiver.mix(false, None).iter().filter(|sample| sample.abs() > f32::EPSILON).count();
        drained += before - receiver.outputs[0].consumer.occupied_len();
        if !receiver.control.is_playing() {
            break;
        }
    }
    assert_eq!(drained, TONE_SAMPLES);
    assert!(audible > 0);
    assert!(!receiver.control.is_playing());
    assert!(!receiver.control.eof_pending.load(Ordering::Acquire));
    assert_eq!(receiver.outputs[0].consumer.occupied_len(), 0);
    assert!(receiver.mix(false, None).iter().all(|sample| *sample == 0.0));
}

#[test]
fn stopped_decode_replays() {
    let (gate, mut receiver) = DecodeGate::receiver();
    gate.wait(DecodeStage::Started);
    gate.release();
    gate.wait(DecodeStage::Finished);
    receiver.command(VoiceOp::Stop);
    for _ in 0..MAX_MIX_BLOCKS {
        assert!(receiver.mix(false, None).iter().all(|sample| *sample == 0.0));
    }
    assert!(receiver.control.flush.load(Ordering::Relaxed));
    assert_eq!(receiver.outputs[0].consumer.occupied_len(), 0);
    gate.release();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while receiver.control.flush.load(Ordering::Relaxed) {
        assert!(std::time::Instant::now() < deadline);
        assert!(receiver.mix(false, None).iter().all(|sample| *sample == 0.0));
        std::thread::yield_now();
    }
    receiver.command(VoiceOp::Play);
    gate.wait(DecodeStage::Started);
    assert!(receiver.mix(false, None).iter().all(|sample| *sample == 0.0));
    assert!(receiver.control.is_playing());
    assert!(!receiver.control.eof_pending.load(Ordering::Acquire));
    gate.release();
    gate.wait(DecodeStage::Finished);
    gate.release();
    receiver.wait_for_eof();
    drain_tail(&mut receiver);
    assert!(gate.stages.try_recv().is_err());
}

#[test]
fn completion_yields_control() {
    let (gate, mut receiver) = DecodeGate::receiver();
    gate.wait(DecodeStage::Started);
    gate.release();
    gate.wait(DecodeStage::Finished);
    gate.release();
    receiver.wait_for_eof();
    let state = receiver.control.state.clone();
    let guard = state.lock().unwrap();
    let mut audible = 0;
    for _ in 0..MAX_MIX_BLOCKS {
        audible +=
            receiver.mix(false, None).iter().filter(|sample| sample.abs() > f32::EPSILON).count();
    }
    assert!(audible > 0);
    assert_eq!(receiver.outputs[0].consumer.occupied_len(), 0);
    assert!(receiver.control.is_playing());
    assert!(receiver.control.eof_pending.load(Ordering::Acquire));
    drop(guard);
    receiver.command(VoiceOp::Stop);
    receiver.command(VoiceOp::Play);
    receiver.mix(false, None);
    gate.wait(DecodeStage::Started);
    assert!(receiver.mix(false, None).iter().all(|sample| *sample == 0.0));
    assert!(receiver.control.is_playing());
    assert!(!receiver.control.eof_pending.load(Ordering::Acquire));
    gate.release();
    gate.wait(DecodeStage::Finished);
    gate.release();
    receiver.wait_for_eof();
    drain_tail(&mut receiver);
}

#[test]
fn deferred_flush_joins() {
    let (gate, receiver) = DecodeGate::receiver();
    gate.wait(DecodeStage::Started);
    gate.release();
    gate.wait(DecodeStage::Finished);
    receiver.command(VoiceOp::Stop);
    gate.release();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while receiver.control.state.lock().unwrap().decoding {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    receiver.command(VoiceOp::Play);
    assert!(gate.stages.recv_timeout(Duration::from_millis(30)).is_err());
    assert!(receiver.control.flush.load(Ordering::Relaxed));
    let started = std::time::Instant::now();
    drop(receiver);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn held_output_stays_idle() {
    let mut receiver = ReceiverHarness::new(None);
    receiver.wait_for_eof();
    receiver.command(VoiceOp::Pause);
    assert!(!receiver.control.needs_output());
    receiver.command(VoiceOp::Play);
    assert!(receiver.control.needs_output());
    receiver.command(VoiceOp::Stop);
    assert!(receiver.control.needs_output());
    for _ in 0..MAX_MIX_BLOCKS {
        receiver.mix(false, None);
    }
    assert!(!receiver.control.needs_output());
}

#[test]
fn decoder_error_clears_voice() {
    let voice = Voice {
        id: "missing".into(),
        name: "missing".into(),
        clips: vec!["/nonexistent/skwd-test-clip.wav".into()],
        gain: 1.0,
        mode: VoiceMode::Once,
        min_gap: 0.0,
        max_gap: 0.0,
        autostart: true,
    };
    let (mut producer, _) = HeapRb::<f32>::new(RING_FRAMES * 2).split();
    let stop = AtomicBool::new(false);
    let mute = AtomicBool::new(false);
    let paused = AtomicBool::new(false);
    let hold = AtomicBool::new(false);
    let restart = AtomicBool::new(false);
    let flush = AtomicBool::new(false);
    let eof_pending = AtomicBool::new(false);
    let state = Mutex::new(VoiceState::default());
    let events = SceneEvents { pending: AtomicBool::new(false), wake: None };
    let wake = (Mutex::new(()), Condvar::new());
    let flags = VoiceFlags {
        stop: &stop,
        mute: &mute,
        paused: &paused,
        hold: &hold,
        restart: &restart,
        flush: &flush,
        eof_pending: &eof_pending,
        state: &state,
        events: &events,
        decode_hook: None,
    };
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| run_voice(&voice, 0, &mut producer, &flags, &wake));
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !hold.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let stopped = hold.load(Ordering::Relaxed);
        stop.store(true, Ordering::Relaxed);
        wake_parked(&wake);
        worker.thread().unpark();
        worker.join().unwrap();
        assert!(stopped);
    });
}

fn tone(index: usize) -> Option<String> {
    let path = std::env::var("SKWD_AUDIO_TEST_CLIPS").ok()?;
    let clip = path.split(',').nth(index)?.trim().to_string();
    std::path::Path::new(&clip).is_file().then_some(clip)
}

#[test]
#[ignore = "checks lifecycle with an output device: SKWD_AUDIO_TEST_CLIPS=a.wav,b.wav cargo test -p paper-audio -- --ignored"]
fn real_clips_mixer_lifecycle() {
    let (Some(first), Some(second)) = (tone(0), tone(1)) else {
        panic!("set SKWD_AUDIO_TEST_CLIPS to two decodable audio files");
    };
    assert!(playable(&first) && playable(&second));

    let voices = vec![
        Voice {
            id: "a".into(),
            name: "a".into(),
            clips: vec![first],
            gain: 1.0,
            mode: VoiceMode::Loop,
            min_gap: 0.0,
            max_gap: 0.0,
            autostart: true,
        },
        Voice {
            id: "b".into(),
            name: "b".into(),
            clips: vec![second],
            gain: 0.4,
            mode: VoiceMode::Once,
            min_gap: 0.0,
            max_gap: 0.0,
            autostart: true,
        },
    ];
    let mixer =
        SceneMixer::new(&voices, SceneConfig { mute: true, volume: 60, wake: None, tap: None })
            .unwrap()
            .expect("mixer built");
    assert_eq!(mixer.voice_count(), 2);
    mixer.set_mute(false);
    mixer.set_volume(30);
    std::thread::sleep(Duration::from_millis(400));
    mixer.set_pause(true);
    mixer.set_pause(false);
    drop(mixer);
}

#[test]
fn drained_tail_wakes_owner() {
    let mut receiver = ReceiverHarness::new(None);
    let events = receiver.outputs[0].events.clone();
    let mut reader = receiver.reader();
    let mut byte = [0];
    receiver.wait_for_eof();
    assert_eq!(reader.read(&mut byte).unwrap_err().kind(), ErrorKind::WouldBlock);
    assert!(!events.take());
    drain_tail(&mut receiver);
    assert_eq!(reader.read(&mut byte).unwrap(), 1);
    assert_eq!(byte, [1]);
    assert!(events.take());
    assert!(!events.take());
    assert!(!receiver.control.needs_output());
    receiver.mix(false, None);
    assert_eq!(reader.read(&mut byte).unwrap_err().kind(), ErrorKind::WouldBlock);
}

#[test]
fn flushed_stop_wakes_owner() {
    let mut receiver = ReceiverHarness::new(None);
    receiver.wait_for_eof();
    let events = receiver.outputs[0].events.clone();
    let mut reader = receiver.reader();
    receiver.command(VoiceOp::Stop);
    assert!(receiver.control.needs_output());
    for _ in 0..MAX_MIX_BLOCKS {
        assert!(receiver.mix(false, None).iter().all(|sample| *sample == 0.0));
    }
    assert!(!receiver.control.needs_output());
    assert!(events.take());
    let mut byte = [0];
    assert_eq!(reader.read(&mut byte).unwrap(), 1);
    receiver.mix(false, None);
    assert_eq!(reader.read(&mut byte).unwrap_err().kind(), ErrorKind::WouldBlock);
}

fn read_signal(fd: std::os::fd::RawFd) {
    let mut reader = std::fs::File::open(format!("/proc/self/fd/{fd}")).unwrap();
    let (sent, received) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let mut byte = [0];
        reader.read_exact(&mut byte).unwrap();
        sent.send(byte).unwrap();
    });
    assert_eq!(received.recv_timeout(Duration::from_secs(10)).unwrap(), [1]);
    thread.join().unwrap();
}

fn audio_checkpoint(stage: &str) {
    use std::io::Write;
    println!("audio-checkpoint: {stage}");
    std::io::stdout().flush().unwrap();
    read_signal(libc::STDIN_FILENO);
}

#[test]
#[ignore = "requires isolated output, monitor capture and stdin handshake"]
fn real_once_completion() {
    once_completion(false);
}

#[test]
#[ignore = "requires isolated output, monitor capture and stdin handshake"]
fn real_gain_completion() {
    once_completion(true);
}

fn once_completion(churn: bool) {
    let voice = Voice {
        id: "once".into(),
        name: "once".into(),
        clips: vec![tone(0).expect("audio fixture required")],
        gain: 1.0,
        mode: VoiceMode::Once,
        min_gap: 0.0,
        max_gap: 0.0,
        autostart: false,
    };
    let wake = paper_runtime::wake::make_pipe().unwrap();
    let mixer = SceneMixer::new(
        &[voice],
        SceneConfig { wake: Some(wake.clone()), ..SceneConfig::default() },
    )
    .unwrap()
    .unwrap();
    audio_checkpoint("ready");
    for _ in 0..2 {
        assert!(mixer.voice("once", VoiceOp::Play));
        assert!(mixer.voice_playing("once"));
        if churn {
            let deadline = std::time::Instant::now() + Duration::from_secs(6);
            while mixer.voice_playing("once") {
                assert!(std::time::Instant::now() < deadline);
                assert!(mixer.voice("once", VoiceOp::Gain(1.0)));
                mixer.events.notify();
                mixer.poll();
                std::thread::sleep(Duration::from_millis(1));
            }
        } else {
            read_signal(wake.read_fd());
            assert!(mixer.events.pending.load(Ordering::Acquire));
        }
        wake.drain();
        assert!(!mixer.voice_playing("once"));
        assert!(!mixer.voices[0].eof_pending.load(Ordering::Acquire));
        assert!(!mixer.voices[0].needs_output());
        mixer.poll();
        assert!(!mixer.events.pending.load(Ordering::Acquire));
        audio_checkpoint("paused");
    }
    drop(mixer);
    audio_checkpoint("dropped");
}

fn playback(millis: u32) -> Playback {
    let callback = cpal::StreamInstant::new(u64::from(millis / 1000), (millis % 1000) * 1_000_000);
    Playback::new(
        &cpal::OutputCallbackInfo::new(cpal::OutputStreamTimestamp {
            callback,
            playback: callback.checked_add(Duration::from_millis(100)).unwrap(),
        }),
        512,
    )
}

fn pending_tail() -> ReceiverHarness {
    let mut receiver = ReceiverHarness::new(None);
    receiver.wait_for_eof();
    for _ in 0..MAX_MIX_BLOCKS {
        receiver.mix(false, Some(playback(0)));
    }
    assert_eq!(receiver.outputs[0].consumer.occupied_len(), 0);
    assert!(receiver.control.is_playing());
    assert!(receiver.control.eof_pending.load(Ordering::Acquire));
    assert!(!receiver.outputs[0].events.take());
    receiver
}

#[test]
fn audible_tail_wakes() {
    let mut receiver = pending_tail();
    receiver.mix(false, Some(playback(105)));
    assert!(receiver.control.is_playing());
    assert!(!receiver.outputs[0].events.take());
    receiver.mix(false, Some(playback(106)));
    assert!(!receiver.control.is_playing());
    assert!(!receiver.control.eof_pending.load(Ordering::Acquire));
    assert!(receiver.outputs[0].events.take());
}

#[test]
fn paused_tail_rebases() {
    let mut receiver = pending_tail();
    receiver.command(VoiceOp::Pause);
    receiver.command(VoiceOp::Play);
    receiver.mix(false, Some(playback(1000)));
    receiver.mix(false, Some(playback(1105)));
    assert!(receiver.control.is_playing());
    receiver.mix(false, Some(playback(1106)));
    assert!(!receiver.control.is_playing());
}

#[test]
fn stopped_tail_resets() {
    let mut receiver = pending_tail();
    receiver.command(VoiceOp::Stop);
    assert!(receiver.control.state.lock().unwrap().drain.is_none());
    receiver.mix(false, Some(playback(1000)));
    receiver.command(VoiceOp::Play);
    receiver.wait_for_eof();
    receiver.mix(false, Some(playback(2000)));
    assert!(receiver.control.is_playing());
}

#[test]
fn contended_tail_waits() {
    let mut receiver = pending_tail();
    let state = receiver.control.state.clone();
    let guard = state.lock().unwrap();
    receiver.mix(false, Some(playback(1000)));
    assert!(receiver.control.is_playing());
    assert!(!receiver.outputs[0].events.take());
    drop(guard);
    receiver.mix(false, Some(playback(1000)));
    assert!(!receiver.control.is_playing());
}

#[test]
fn buffer_duration_rounds_up() {
    let clock = playback(0);
    assert_eq!(clock.duration, Duration::from_nanos(5_333_334));
    let frame = Playback::new(&cpal::OutputCallbackInfo::new(clock.timestamp), 2);
    assert_eq!(frame.duration, Duration::from_nanos(20_834));
}

#[test]
#[ignore = "requires isolated output, monitor capture and stdin handshake"]
fn real_player_lifecycle() {
    let player = crate::AudioPlayer::new(&tone(0).expect("audio fixture required"), true, 100)
        .unwrap()
        .unwrap();
    audio_checkpoint("ready");
    player.set_mute(false);
    audio_checkpoint("playing");
    player.set_volume(50);
    audio_checkpoint("quiet");
    player.set_pause(true);
    audio_checkpoint("paused");
    player.set_pause(false);
    audio_checkpoint("resumed");
    player.set_mute(true);
    audio_checkpoint("muted");
    player.set_mute(false);
    audio_checkpoint("unmuted");
    drop(player);
    audio_checkpoint("dropped");
}

#[test]
fn redundant_commands_preserve_tail() {
    for op in [VoiceOp::Gain(0.5), VoiceOp::Play] {
        let mut receiver = pending_tail();
        receiver.command(op);
        receiver.mix(false, Some(playback(106)));
        assert!(!receiver.control.is_playing());
        assert!(receiver.outputs[0].events.take());
    }
}

#[test]
fn global_transport_rebases_tail() {
    for value in [true, false] {
        let mut receiver = pending_tail();
        let flag = AtomicBool::new(!value);
        assert!(set_transport(&flag, value, std::slice::from_ref(&receiver.control)));
        assert!(receiver.control.state.lock().unwrap().drain.is_none());
        receiver.mix(false, Some(playback(1000)));
        assert!(!set_transport(&flag, value, std::slice::from_ref(&receiver.control)));
        receiver.mix(false, Some(playback(1105)));
        assert!(receiver.control.is_playing());
        receiver.mix(false, Some(playback(1106)));
        assert!(!receiver.control.is_playing());
    }
}
