use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ffmpeg_the_third as ff;
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Split};

use crate::decoder::{Repeat, decode_loop, signal_stop, wake_parked};

const TARGET_SAMPLE_RATE: u32 = 48_000;
const RING_FRAMES: usize = TARGET_SAMPLE_RATE as usize / 5;

pub struct AudioPlayer {
    mute: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    volume_x100: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    wake: Arc<(Mutex<()>, Condvar)>,
    decoder_thread: Option<JoinHandle<()>>,
    stream: cpal::Stream,
}

impl AudioPlayer {
    pub fn new(file_path: &str, mute: bool, volume: u32) -> Result<Option<Self>> {
        let probe =
            ff::format::input(file_path).with_context(|| format!("audio probe: {file_path}"))?;
        let has_audio = probe.streams().best(ff::media::Type::Audio).is_some();
        drop(probe);
        if !has_audio {
            return Ok(None);
        }

        let mute_flag = Arc::new(AtomicBool::new(mute));
        let paused_flag = Arc::new(AtomicBool::new(false));
        let volume_flag = Arc::new(AtomicU32::new(volume.min(100)));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let wake = Arc::new((Mutex::new(()), Condvar::new()));

        let ring = HeapRb::<f32>::new(RING_FRAMES * 2);
        let (producer, mut consumer) = ring.split();

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
        if let Ok(default_config) = device.default_output_config() {
            tracing::info!("cpal device default: {:?}, requesting: {:?}", default_config, config,);
        }
        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if callback_mute.load(Ordering::Relaxed) {
                        let _ = consumer.skip(data.len());
                        data.fill(0.0);
                        return;
                    }
                    let volume = callback_volume.load(Ordering::Relaxed) as f32 / 100.0;
                    let popped = consumer.pop_slice(data);
                    for sample in &mut data[..popped] {
                        *sample *= volume;
                    }
                    data[popped..].fill(0.0);
                },
                |error| tracing::warn!("cpal stream: {error:?}"),
                None,
            )
            .map_err(|error| anyhow!("cpal build_output_stream: {error:?}"))?;
        if !mute {
            stream.play().map_err(|error| anyhow!("cpal play: {error:?}"))?;
        }

        let path = file_path.to_string();
        let thread_stop = stop_flag.clone();
        let thread_mute = mute_flag.clone();
        let thread_paused = paused_flag.clone();
        let thread_wake = wake.clone();
        let thread = std::thread::Builder::new()
            .name("skwd-audio".into())
            .spawn(move || {
                let mut producer = producer;
                if let Err(error) = decode_loop(
                    &path,
                    &mut producer,
                    &thread_stop,
                    &thread_mute,
                    &thread_paused,
                    &thread_wake,
                    Repeat::Forever,
                ) {
                    tracing::warn!("audio decode_loop exited: {error:?}");
                }
            })
            .map_err(|error| anyhow!("spawn audio thread: {error:?}"))?;

        Ok(Some(Self {
            mute: mute_flag,
            paused: paused_flag,
            volume_x100: volume_flag,
            stop: stop_flag,
            wake,
            decoder_thread: Some(thread),
            stream,
        }))
    }

    fn refresh_stream(&self) {
        if self.mute.load(Ordering::Relaxed) || self.paused.load(Ordering::Relaxed) {
            let _ = self.stream.pause();
        } else {
            let _ = self.stream.play();
        }
        wake_parked(&self.wake);
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
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        signal_stop(&self.stop, &self.wake);
        if let Some(thread) = self.decoder_thread.take() {
            let _ = thread.join();
        }
    }
}
