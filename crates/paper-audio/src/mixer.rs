use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ffmpeg_the_third as ff;
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Observer, Split};

use crate::decoder::{Repeat, decode_loop, signal_stop, wake_parked};

const TARGET_SAMPLE_RATE: u32 = 48_000;
const OUTPUT_CHANNELS: u16 = 2;
const RING_FRAMES: usize = TARGET_SAMPLE_RATE as usize / 5;
const MAX_VOICES: usize = 32;
const MAX_VOLUME: u32 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceMode {
    Loop,
    Once,
    Random,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    pub id: String,
    pub name: String,
    pub clips: Vec<String>,
    pub gain: f32,
    pub mode: VoiceMode,
    pub min_gap: f32,
    pub max_gap: f32,
    pub autostart: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VoiceOp {
    Play,
    Stop,
    Pause,
    Gain(f32),
}

pub struct SceneConfig {
    pub mute: bool,
    pub volume: u32,
    pub wake: Option<paper_runtime::wake::Pipe>,
    pub tap: Option<crate::spectrum::PlaybackTap>,
}

impl Default for SceneConfig {
    fn default() -> Self {
        Self { mute: false, volume: MAX_VOLUME, wake: None, tap: None }
    }
}

struct SceneEvents {
    pending: AtomicBool,
    wake: Option<paper_runtime::wake::Pipe>,
}

impl SceneEvents {
    fn notify(&self) {
        self.pending.store(true, Ordering::Release);
        if let Some(wake) = &self.wake {
            wake.poke();
        }
    }

    fn take(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }
}

#[derive(Clone, Copy)]
struct Playback {
    timestamp: cpal::OutputStreamTimestamp,
    duration: Duration,
}

impl Playback {
    fn new(info: &cpal::OutputCallbackInfo, samples: usize) -> Self {
        let frames = samples / usize::from(OUTPUT_CHANNELS);
        let nanos = (frames as u128 * Duration::from_secs(1).as_nanos())
            .div_ceil(u128::from(TARGET_SAMPLE_RATE));
        Self {
            timestamp: info.timestamp(),
            duration: Duration::from_nanos(u64::try_from(nanos).expect("audio buffer duration")),
        }
    }
}

#[derive(Default)]
struct VoiceState {
    generation: u64,
    decoding: bool,
    drain: Option<Playback>,
}

struct VoiceOutput {
    consumer: ringbuf::HeapCons<f32>,
    gain: Arc<AtomicU32>,
    hold: Arc<AtomicBool>,
    flush: Arc<AtomicBool>,
    eof_pending: Arc<AtomicBool>,
    state: Arc<Mutex<VoiceState>>,
    decoder_thread: std::thread::Thread,
    events: Arc<SceneEvents>,
}

impl VoiceOutput {
    fn mix(
        &mut self,
        data: &mut [f32],
        scratch: &mut [f32],
        muted: bool,
        playback: Option<Playback>,
    ) {
        let flush = self.flush.load(Ordering::Relaxed);
        if self.hold.load(Ordering::Relaxed) && !flush {
            return;
        }
        if muted {
            let _ = self.consumer.skip(data.len());
            if flush {
                self.finish_flush();
            } else {
                self.finish_eof(playback);
            }
            return;
        }
        let popped = self.consumer.pop_slice(&mut scratch[..data.len()]);
        if flush {
            self.finish_flush();
            return;
        }
        let gain = f32::from_bits(self.gain.load(Ordering::Relaxed));
        if gain != 0.0 {
            for (sample, voice) in data.iter_mut().zip(&scratch[..popped]) {
                *sample += voice * gain;
            }
        }
        self.finish_eof(playback);
    }

    fn finish_flush(&self) {
        let Ok(state) = self.state.try_lock() else {
            return;
        };
        if !state.decoding && self.consumer.occupied_len() == 0 {
            self.flush.store(false, Ordering::Relaxed);
            drop(state);
            self.decoder_thread.unpark();
            self.events.notify();
        }
    }

    fn finish_eof(&self, playback: Option<Playback>) {
        let Ok(mut state) = self.state.try_lock() else {
            return;
        };
        if self.eof_pending.load(Ordering::Acquire) && self.consumer.occupied_len() == 0 {
            if let Some(playback) = playback {
                let drain = state.drain.get_or_insert(playback);
                if playback
                    .timestamp
                    .callback
                    .checked_duration_since(drain.timestamp.playback)
                    .is_none_or(|elapsed| elapsed < drain.duration)
                {
                    return;
                }
            }
            state.drain = None;
            self.hold.store(true, Ordering::Release);
            self.eof_pending.store(false, Ordering::Release);
            drop(state);
            self.decoder_thread.unpark();
            self.events.notify();
        }
    }
}

fn mix_outputs(
    outputs: &mut [VoiceOutput],
    data: &mut [f32],
    scratch: &mut Vec<f32>,
    muted: bool,
    volume: u32,
    playback: Option<Playback>,
) {
    data.fill(0.0);
    if scratch.len() < data.len() {
        scratch.resize(data.len(), 0.0);
    }
    for output in outputs {
        output.mix(data, scratch, muted, playback);
    }
    let master = volume as f32 / MAX_VOLUME as f32;
    for sample in data {
        *sample = (*sample * master).clamp(-1.0, 1.0);
    }
}

struct VoiceControl {
    id: String,
    gain: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    hold: Arc<AtomicBool>,
    restart: Arc<AtomicBool>,
    flush: Arc<AtomicBool>,
    eof_pending: Arc<AtomicBool>,
    state: Arc<Mutex<VoiceState>>,
    wake: Arc<(Mutex<()>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl VoiceControl {
    fn is_playing(&self) -> bool {
        !self.hold.load(Ordering::Relaxed)
    }

    fn needs_output(&self) -> bool {
        self.is_playing() || self.flush.load(Ordering::Relaxed)
    }

    fn apply(&self, op: VoiceOp) {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match op {
            VoiceOp::Play => {
                if self.hold.swap(false, Ordering::Relaxed) {
                    state.drain = None;
                }
            }
            VoiceOp::Stop => {
                state.drain = None;
                state.generation = state.generation.wrapping_add(1);
                self.eof_pending.store(false, Ordering::Release);
                self.hold.store(true, Ordering::Relaxed);
                self.restart.store(true, Ordering::Relaxed);
                self.flush.store(true, Ordering::Relaxed);
            }
            VoiceOp::Pause => {
                if !self.hold.swap(true, Ordering::Relaxed) {
                    state.drain = None;
                }
            }
            VoiceOp::Gain(gain) => self.gain.store(gain_bits(gain), Ordering::Relaxed),
        }
    }

    fn wake(&self) {
        wake_parked(&self.wake);
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }

    fn signal(&self) {
        signal_stop(&self.stop, &self.wake);
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }

    fn join(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for VoiceControl {
    fn drop(&mut self) {
        // SceneMixer construction can fail after one or more decoder workers
        // have started (for example when no output device exists). Keep each
        // worker self-owning so every partial-construction path stops and joins.
        self.signal();
        self.join();
    }
}

fn set_transport(flag: &AtomicBool, value: bool, voices: &[VoiceControl]) -> bool {
    if flag.swap(value, Ordering::Relaxed) == value {
        return false;
    }
    for voice in voices {
        voice.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).drain = None;
    }
    true
}

pub struct SceneMixer {
    mute: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    volume_x100: Arc<AtomicU32>,
    voices: Vec<VoiceControl>,
    events: Arc<SceneEvents>,
    stream: cpal::Stream,
}

pub fn playable(path: &str) -> bool {
    let Ok(probe) = ff::format::input(path) else {
        return false;
    };
    let has_audio = probe.streams().best(ff::media::Type::Audio).is_some();
    drop(probe);
    has_audio
}

fn gain_bits(gain: f32) -> u32 {
    gain.max(0.0).to_bits()
}

fn seed_from_clock(salt: u64) -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);
    (nanos ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15)) | 1
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn park(wake: &(Mutex<()>, Condvar), stop: &AtomicBool, mut remaining: Duration) {
    while !stop.load(Ordering::Relaxed) && !remaining.is_zero() {
        let slice = remaining.min(Duration::from_millis(250));
        let guard = wake.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_guard, timeout) =
            wake.1.wait_timeout(guard, slice).unwrap_or_else(std::sync::PoisonError::into_inner);
        if !timeout.timed_out() {
            return;
        }
        remaining = remaining.saturating_sub(slice);
    }
}

impl SceneMixer {
    pub fn new(voices: &[Voice], config: SceneConfig) -> Result<Option<Self>> {
        let SceneConfig { mute, volume, wake, tap } = config;
        let events = Arc::new(SceneEvents { pending: AtomicBool::new(false), wake });
        let playable_voices: Vec<Voice> = voices
            .iter()
            .take(MAX_VOICES)
            .filter_map(|voice| {
                let clips: Vec<String> =
                    voice.clips.iter().filter(|clip| playable(clip)).cloned().collect();
                (!clips.is_empty()).then(|| Voice { clips, ..voice.clone() })
            })
            .collect();
        if playable_voices.is_empty() {
            return Ok(None);
        }

        let mute_flag = Arc::new(AtomicBool::new(mute));
        let paused_flag = Arc::new(AtomicBool::new(false));
        let volume_flag = Arc::new(AtomicU32::new(volume.min(MAX_VOLUME)));

        let mut outputs = Vec::with_capacity(playable_voices.len());
        let mut controls = Vec::with_capacity(playable_voices.len());
        for (index, voice) in playable_voices.into_iter().enumerate() {
            let ring = HeapRb::<f32>::new(RING_FRAMES * 2);
            let (producer, consumer) = ring.split();
            let gain = Arc::new(AtomicU32::new(gain_bits(voice.gain)));
            let stop = Arc::new(AtomicBool::new(false));
            let hold = Arc::new(AtomicBool::new(!voice.autostart));
            let restart = Arc::new(AtomicBool::new(false));
            let flush = Arc::new(AtomicBool::new(false));
            let eof_pending = Arc::new(AtomicBool::new(false));
            let state = Arc::new(Mutex::new(VoiceState::default()));
            let wake = Arc::new((Mutex::new(()), Condvar::new()));
            let thread_stop = stop.clone();
            let thread_wake = wake.clone();
            let thread_mute = mute_flag.clone();
            let thread_paused = paused_flag.clone();
            let thread_hold = hold.clone();
            let thread_restart = restart.clone();
            let thread_flush = flush.clone();
            let thread_eof_pending = eof_pending.clone();
            let thread_state = state.clone();
            let thread_events = events.clone();
            let id = voice.id.clone();
            let thread = std::thread::Builder::new()
                .name("skwd-scene-voice".into())
                .spawn(move || {
                    let mut producer = producer;
                    run_voice(
                        &voice,
                        index,
                        &mut producer,
                        &VoiceFlags {
                            stop: &thread_stop,
                            mute: &thread_mute,
                            paused: &thread_paused,
                            hold: &thread_hold,
                            restart: &thread_restart,
                            flush: &thread_flush,
                            eof_pending: &thread_eof_pending,
                            state: &thread_state,
                            events: &thread_events,
                            #[cfg(test)]
                            decode_hook: None,
                        },
                        &thread_wake,
                    );
                })
                .map_err(|error| anyhow!("spawn scene voice thread: {error:?}"))?;
            outputs.push(VoiceOutput {
                consumer,
                gain: gain.clone(),
                hold: hold.clone(),
                flush: flush.clone(),
                eof_pending: eof_pending.clone(),
                state: state.clone(),
                decoder_thread: thread.thread().clone(),
                events: events.clone(),
            });
            controls.push(VoiceControl {
                id,
                gain,
                stop,
                hold,
                restart,
                flush,
                eof_pending,
                state,
                wake,
                thread: Some(thread),
            });
        }

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no default audio output device"))?;
        let config = cpal::StreamConfig {
            channels: OUTPUT_CHANNELS,
            sample_rate: TARGET_SAMPLE_RATE,
            buffer_size: cpal::BufferSize::Default,
        };
        let callback_mute = mute_flag.clone();
        let callback_volume = volume_flag.clone();
        let mut scratch: Vec<f32> = Vec::new();
        let stream = device
            .build_output_stream(
                config,
                move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                    let playback = Playback::new(info, data.len());
                    mix_outputs(
                        &mut outputs,
                        data,
                        &mut scratch,
                        callback_mute.load(Ordering::Relaxed),
                        callback_volume.load(Ordering::Relaxed),
                        Some(playback),
                    );
                    if let Some(tap) = &tap {
                        tap.write(data);
                    }
                },
                |error| tracing::warn!("cpal scene mixer stream: {error:?}"),
                None,
            )
            .map_err(|error| anyhow!("cpal build_output_stream: {error:?}"))?;
        if !mute && controls.iter().any(VoiceControl::needs_output) {
            stream.play().map_err(|error| anyhow!("cpal play: {error:?}"))?;
        }

        Ok(Some(Self {
            mute: mute_flag,
            paused: paused_flag,
            volume_x100: volume_flag,
            voices: controls,
            events,
            stream,
        }))
    }

    fn refresh_stream(&self) {
        let all_held = !self.voices.iter().any(VoiceControl::needs_output);
        if self.mute.load(Ordering::Relaxed) || self.paused.load(Ordering::Relaxed) || all_held {
            let _ = self.stream.pause();
        } else {
            let _ = self.stream.play();
        }
        for voice in &self.voices {
            voice.wake();
        }
    }

    pub fn poll(&self) {
        if self.events.take() {
            self.refresh_stream();
        }
    }

    pub fn voice(&self, id: &str, op: VoiceOp) -> bool {
        let Some(voice) = self.voices.iter().find(|voice| voice.id == id) else {
            return false;
        };
        voice.apply(op);
        self.refresh_stream();
        true
    }

    pub fn voice_playing(&self, id: &str) -> bool {
        self.voices.iter().find(|voice| voice.id == id).is_some_and(VoiceControl::is_playing)
    }

    pub fn set_mute(&self, mute: bool) {
        if set_transport(&self.mute, mute, &self.voices) {
            self.refresh_stream();
        }
    }

    pub fn set_pause(&self, paused: bool) {
        if set_transport(&self.paused, paused, &self.voices) {
            self.refresh_stream();
        }
    }

    pub fn set_volume(&self, volume: u32) {
        self.volume_x100.store(volume.min(MAX_VOLUME), Ordering::Relaxed);
    }

    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }
}

impl Drop for SceneMixer {
    fn drop(&mut self) {
        let _ = self.stream.pause();
        for voice in &self.voices {
            voice.signal();
        }
        for voice in &mut self.voices {
            voice.join();
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecodeStage {
    Started,
    Finished,
}

struct VoiceFlags<'a> {
    stop: &'a AtomicBool,
    mute: &'a AtomicBool,
    paused: &'a AtomicBool,
    hold: &'a AtomicBool,
    restart: &'a AtomicBool,
    flush: &'a AtomicBool,
    eof_pending: &'a AtomicBool,
    state: &'a Mutex<VoiceState>,
    events: &'a SceneEvents,
    #[cfg(test)]
    decode_hook: Option<&'a (dyn Fn(DecodeStage) + Sync)>,
}

fn wait_while_held(flags: &VoiceFlags<'_>, wake: &(Mutex<()>, Condvar)) {
    while flags.hold.load(Ordering::Relaxed) && !flags.stop.load(Ordering::Relaxed) {
        let guard = wake.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if flags.hold.load(Ordering::Relaxed) && !flags.stop.load(Ordering::Relaxed) {
            let _unused = wake.1.wait(guard);
        }
    }
}

fn wait_while_flushing(flags: &VoiceFlags<'_>) {
    while flags.flush.load(Ordering::Relaxed) && !flags.stop.load(Ordering::Relaxed) {
        std::thread::park();
    }
}

fn run_voice(
    voice: &Voice,
    index: usize,
    producer: &mut ringbuf::HeapProd<f32>,
    flags: &VoiceFlags<'_>,
    wake: &(Mutex<()>, Condvar),
) {
    let mut random = seed_from_clock(index as u64);
    let (min_gap, max_gap) = {
        let min = voice.min_gap.max(0.0);
        (min, voice.max_gap.max(min))
    };
    loop {
        wait_while_held(flags, wake);
        if flags.stop.load(Ordering::Relaxed) {
            return;
        }
        wait_while_flushing(flags);
        if flags.stop.load(Ordering::Relaxed) {
            return;
        }
        let generation = {
            let mut state = flags.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if flags.hold.load(Ordering::Relaxed) || flags.flush.load(Ordering::Relaxed) {
                continue;
            }
            flags.restart.store(false, Ordering::Relaxed);
            state.decoding = true;
            state.generation
        };
        let clip = match voice.mode {
            VoiceMode::Random if voice.clips.len() > 1 => {
                &voice.clips[(next_random(&mut random) as usize) % voice.clips.len()]
            }
            _ => &voice.clips[0],
        };
        let repeat = if voice.mode == VoiceMode::Loop { Repeat::Forever } else { Repeat::Once };
        #[cfg(test)]
        if let Some(hook) = flags.decode_hook {
            hook(DecodeStage::Started);
        }
        let decoded = decode_loop(
            clip,
            producer,
            flags.stop,
            flags.mute,
            flags.paused,
            flags.hold,
            flags.restart,
            wake,
            repeat,
        );
        #[cfg(test)]
        if let Some(hook) = flags.decode_hook {
            hook(DecodeStage::Finished);
        }
        let mut state = flags.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.decoding = false;
        if state.generation != generation {
            continue;
        }
        match decoded {
            Err(error) => {
                flags.eof_pending.store(false, Ordering::Release);
                flags.hold.store(true, Ordering::Relaxed);
                flags.flush.store(true, Ordering::Relaxed);
                flags.restart.store(true, Ordering::Relaxed);
                drop(state);
                flags.events.notify();
                tracing::warn!("scene voice {} decode exited: {error:?}", voice.name);
                continue;
            }
            Ok(true) => continue,
            Ok(false) => {}
        }
        match voice.mode {
            VoiceMode::Loop | VoiceMode::Once => {
                flags.eof_pending.store(true, Ordering::Release);
                drop(state);
                while flags.eof_pending.load(Ordering::Acquire)
                    && !flags.stop.load(Ordering::Relaxed)
                    && !flags.restart.load(Ordering::Relaxed)
                {
                    std::thread::park();
                }
                if flags.stop.load(Ordering::Relaxed) {
                    return;
                }
            }
            VoiceMode::Random => {
                drop(state);
                let span = f64::from(max_gap - min_gap);
                let jitter = if span > 0.0 {
                    (next_random(&mut random) % 1_000_000) as f64 / 1_000_000.0 * span
                } else {
                    0.0
                };
                park(wake, flags.stop, Duration::from_secs_f64(f64::from(min_gap) + jitter));
            }
        }
    }
}

#[cfg(test)]
#[path = "mixer_tests.rs"]
mod tests;
