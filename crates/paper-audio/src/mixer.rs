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
const RING_FRAMES: usize = TARGET_SAMPLE_RATE as usize / 5;
const MAX_VOICES: usize = 32;

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

struct VoiceOutput {
    consumer: ringbuf::HeapCons<f32>,
    gain: Arc<AtomicU32>,
    hold: Arc<AtomicBool>,
    flush: Arc<AtomicBool>,
}

struct VoiceControl {
    id: String,
    gain: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    hold: Arc<AtomicBool>,
    restart: Arc<AtomicBool>,
    flush: Arc<AtomicBool>,
    wake: Arc<(Mutex<()>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl VoiceControl {
    fn signal(&self) {
        signal_stop(&self.stop, &self.wake);
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

pub struct SceneMixer {
    mute: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    volume_x100: Arc<AtomicU32>,
    voices: Vec<VoiceControl>,
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
    pub fn new(voices: &[Voice], mute: bool, volume: u32) -> Result<Option<Self>> {
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
        let volume_flag = Arc::new(AtomicU32::new(volume.min(100)));

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
            let wake = Arc::new((Mutex::new(()), Condvar::new()));
            outputs.push(VoiceOutput {
                consumer,
                gain: gain.clone(),
                hold: hold.clone(),
                flush: flush.clone(),
            });

            let thread_stop = stop.clone();
            let thread_wake = wake.clone();
            let thread_mute = mute_flag.clone();
            let thread_paused = paused_flag.clone();
            let thread_hold = hold.clone();
            let thread_restart = restart.clone();
            let thread_flush = flush.clone();
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
                        },
                        &thread_wake,
                    );
                })
                .map_err(|error| anyhow!("spawn scene voice thread: {error:?}"))?;
            controls.push(VoiceControl {
                id,
                gain,
                stop,
                hold,
                restart,
                flush,
                wake,
                thread: Some(thread),
            });
        }

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no default audio output device"))?;
        let config = cpal::StreamConfig {
            channels: 2,
            sample_rate: cpal::SampleRate(TARGET_SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Default,
        };
        let callback_mute = mute_flag.clone();
        let callback_volume = volume_flag.clone();
        let mut scratch: Vec<f32> = Vec::new();
        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if callback_mute.load(Ordering::Relaxed) {
                        for output in &mut outputs {
                            let _ = output.consumer.skip(data.len());
                        }
                        data.fill(0.0);
                        return;
                    }
                    data.fill(0.0);
                    if scratch.len() < data.len() {
                        scratch.resize(data.len(), 0.0);
                    }
                    for output in &mut outputs {
                        let flush = output.flush.load(Ordering::Relaxed);
                        if output.hold.load(Ordering::Relaxed) && !flush {
                            continue;
                        }
                        let gain = f32::from_bits(output.gain.load(Ordering::Relaxed));
                        let popped = output.consumer.pop_slice(&mut scratch[..data.len()]);
                        if flush {
                            if output.consumer.occupied_len() == 0 {
                                output.flush.store(false, Ordering::Relaxed);
                            }
                            continue;
                        }
                        if gain == 0.0 {
                            continue;
                        }
                        for (sample, voice) in data.iter_mut().zip(&scratch[..popped]) {
                            *sample += voice * gain;
                        }
                    }
                    let master = callback_volume.load(Ordering::Relaxed) as f32 / 100.0;
                    for sample in data.iter_mut() {
                        *sample = (*sample * master).clamp(-1.0, 1.0);
                    }
                },
                |error| tracing::warn!("cpal scene mixer stream: {error:?}"),
                None,
            )
            .map_err(|error| anyhow!("cpal build_output_stream: {error:?}"))?;
        if !mute {
            stream.play().map_err(|error| anyhow!("cpal play: {error:?}"))?;
        }

        Ok(Some(Self {
            mute: mute_flag,
            paused: paused_flag,
            volume_x100: volume_flag,
            voices: controls,
            stream,
        }))
    }

    fn refresh_stream(&self) {
        let all_held = self.voices.iter().all(|voice| {
            voice.hold.load(Ordering::Relaxed) && !voice.flush.load(Ordering::Relaxed)
        });
        if self.mute.load(Ordering::Relaxed) || self.paused.load(Ordering::Relaxed) || all_held {
            let _ = self.stream.pause();
        } else {
            let _ = self.stream.play();
        }
        for voice in &self.voices {
            wake_parked(&voice.wake);
        }
    }

    pub fn voice(&self, id: &str, op: VoiceOp) -> bool {
        let Some(voice) = self.voices.iter().find(|voice| voice.id == id) else {
            return false;
        };
        match op {
            VoiceOp::Play => voice.hold.store(false, Ordering::Relaxed),
            VoiceOp::Stop => {
                voice.hold.store(true, Ordering::Relaxed);
                voice.restart.store(true, Ordering::Relaxed);
                voice.flush.store(true, Ordering::Relaxed);
            }
            VoiceOp::Pause => voice.hold.store(true, Ordering::Relaxed),
            VoiceOp::Gain(gain) => voice.gain.store(gain_bits(gain), Ordering::Relaxed),
        }
        self.refresh_stream();
        true
    }

    pub fn voice_playing(&self, id: &str) -> bool {
        self.voices
            .iter()
            .find(|voice| voice.id == id)
            .is_some_and(|voice| !voice.hold.load(Ordering::Relaxed))
    }

    pub fn set_mute(&self, mute: bool) {
        self.mute.store(mute, Ordering::Relaxed);
        self.refresh_stream();
    }

    pub fn set_pause(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
        self.refresh_stream();
    }

    pub fn set_volume(&self, volume: u32) {
        self.volume_x100.store(volume.min(100), Ordering::Relaxed);
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

struct VoiceFlags<'a> {
    stop: &'a AtomicBool,
    mute: &'a AtomicBool,
    paused: &'a AtomicBool,
    hold: &'a AtomicBool,
    restart: &'a AtomicBool,
    flush: &'a AtomicBool,
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
        std::thread::sleep(Duration::from_millis(2));
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
        flags.restart.store(false, Ordering::Relaxed);
        wait_while_flushing(flags);
        let clip = match voice.mode {
            VoiceMode::Random if voice.clips.len() > 1 => {
                &voice.clips[(next_random(&mut random) as usize) % voice.clips.len()]
            }
            _ => &voice.clips[0],
        };
        let repeat = if voice.mode == VoiceMode::Loop { Repeat::Forever } else { Repeat::Once };
        match decode_loop(
            clip,
            producer,
            flags.stop,
            flags.mute,
            flags.paused,
            flags.hold,
            flags.restart,
            wake,
            repeat,
        ) {
            Err(error) => {
                tracing::warn!("scene voice {} decode exited: {error:?}", voice.name);
                return;
            }
            Ok(true) => continue,
            Ok(false) => {}
        }
        match voice.mode {
            VoiceMode::Loop | VoiceMode::Once => {
                flags.hold.store(true, Ordering::Relaxed);
                flags.restart.store(true, Ordering::Relaxed);
            }
            VoiceMode::Random => {
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
