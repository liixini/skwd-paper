use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer, Producer, Split};

pub const BAND_COUNTS: [usize; 3] = [16, 32, 64];
const WINDOW: usize = 2048;
const LOW_HZ: f32 = 30.0;
const HIGH_HZ: f32 = 20_000.0;
const HOLD: f32 = 0.25;
const ATTACK_TAU: f32 = 0.045;
const RELEASE_TAU: f32 = 0.28;
const GAIN: f32 = 30.0;
const BRIDGES: [&str; 2] = ["pulse", "pipewire"];

#[derive(Clone)]
pub struct Bands {
    left: [Vec<f32>; 3],
    right: [Vec<f32>; 3],
}

impl Default for Bands {
    fn default() -> Self {
        Self {
            left: BAND_COUNTS.map(|count| vec![0.0; count]),
            right: BAND_COUNTS.map(|count| vec![0.0; count]),
        }
    }
}

impl Bands {
    #[must_use]
    pub fn slice(&self, count: usize, right: bool) -> Option<&[f32]> {
        let index = BAND_COUNTS.iter().position(|known| *known == count)?;
        Some(if right { &self.right[index] } else { &self.left[index] })
    }

    fn silence(&mut self, dt: f32) {
        let keep = (-dt / RELEASE_TAU).exp();
        for lane in self.left.iter_mut().chain(self.right.iter_mut()) {
            for value in lane.iter_mut() {
                *value *= keep;
            }
        }
    }
}

pub fn source_name() -> String {
    std::env::var("SKWD_VK_AUDIO_SOURCE").unwrap_or_else(|_| "@DEFAULT_MONITOR@".to_string())
}

fn tuned(name: &str, fallback: f32) -> f32 {
    std::env::var(name).ok().and_then(|text| text.parse().ok()).unwrap_or(fallback)
}

fn hann() -> Vec<f32> {
    (0..WINDOW)
        .map(|index| {
            let phase = std::f32::consts::TAU * index as f32 / (WINDOW as f32 - 1.0);
            0.5 - 0.5 * phase.cos()
        })
        .collect()
}

fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut target = 0usize;
    for source in 1..n {
        let mut bit = n >> 1;
        while target & bit != 0 {
            target ^= bit;
            bit >>= 1;
        }
        target |= bit;
        if source < target {
            re.swap(source, target);
            im.swap(source, target);
        }
    }
    let mut span = 2usize;
    while span <= n {
        let step = -std::f32::consts::TAU / span as f32;
        let half = span / 2;
        for start in (0..n).step_by(span) {
            for offset in 0..half {
                let angle = step * offset as f32;
                let (sin, cos) = angle.sin_cos();
                let at = start + offset;
                let mate = at + half;
                let tre = re[mate] * cos - im[mate] * sin;
                let tim = re[mate] * sin + im[mate] * cos;
                re[mate] = re[at] - tre;
                im[mate] = im[at] - tim;
                re[at] += tre;
                im[at] += tim;
            }
        }
        span <<= 1;
    }
}

fn band_edges(count: usize, rate: f32) -> Vec<(usize, usize)> {
    let nyquist = rate * 0.5;
    let high = HIGH_HZ.min(nyquist);
    let ratio = (high / LOW_HZ).ln();
    let bin_of = |hz: f32| ((hz / rate) * WINDOW as f32).round() as usize;
    (0..count)
        .map(|index| {
            let lo = LOW_HZ * (ratio * index as f32 / count as f32).exp();
            let hi = LOW_HZ * (ratio * (index + 1) as f32 / count as f32).exp();
            let (mut a, mut b) = (bin_of(lo), bin_of(hi));
            a = a.clamp(1, WINDOW / 2 - 1);
            b = b.clamp(a + 1, WINDOW / 2);
            (a, b)
        })
        .collect()
}

pub struct Analyser {
    _stream: cpal::Stream,
    consumer: ringbuf::HeapCons<f32>,
    channels: usize,
    edges: Vec<Vec<(usize, usize)>>,
    window: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    gain: f32,
    attack: f32,
    release: f32,
    starved: f32,
    cursor: usize,
}

impl Analyser {
    pub fn start() -> Result<Self> {
        let host = cpal::default_host();
        let mut device = None;
        for wanted in BRIDGES {
            if let Ok(mut found) = host.input_devices()
                && let Some(hit) =
                    found.find(|candidate| candidate.name().is_ok_and(|name| name == wanted))
            {
                device = Some(hit);
                break;
            }
        }
        let device = device
            .or_else(|| host.default_input_device())
            .ok_or_else(|| anyhow!("no audio capture device"))?;
        let name = device.name().unwrap_or_else(|_| "?".to_string());
        let config = device.default_input_config().context("capture config")?;
        let rate = config.sample_rate().0 as f32;
        let channels = config.channels().max(1) as usize;
        let (mut producer, consumer) = ringbuf::HeapRb::<f32>::new(WINDOW * channels * 8).split();
        let stream = device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    for sample in data {
                        let _ = producer.try_push(*sample);
                    }
                },
                |error| tracing::warn!("skwd-wall-vk: audio capture error {error}"),
                None,
            )
            .context("capture stream")?;
        stream.play().context("capture start")?;
        let gain = std::env::var("SKWD_VK_AUDIO_GAIN")
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or(GAIN);
        tracing::info!(
            "skwd-wall-vk: audio capture on {name} source={} rate={rate} channels={channels}",
            source_name()
        );
        Ok(Self {
            _stream: stream,
            consumer,
            channels,
            edges: BAND_COUNTS.iter().map(|count| band_edges(*count, rate)).collect(),
            window: hann(),
            left: vec![0.0; WINDOW],
            right: vec![0.0; WINDOW],
            re: vec![0.0; WINDOW],
            im: vec![0.0; WINDOW],
            gain,
            attack: tuned("SKWD_VK_AUDIO_ATTACK", ATTACK_TAU),
            release: tuned("SKWD_VK_AUDIO_RELEASE", RELEASE_TAU),
            starved: 0.0,
            cursor: 0,
        })
    }

    fn drain(&mut self) -> usize {
        let mut taken = 0usize;
        loop {
            let mut left = 0.0f32;
            let mut right = 0.0f32;
            for slot in 0..self.channels {
                let Some(sample) = self.consumer.try_pop() else {
                    return taken;
                };
                if slot == 0 {
                    left = sample;
                    right = sample;
                } else if slot == 1 {
                    right = sample;
                }
            }
            self.left[self.cursor] = left;
            self.right[self.cursor] = right;
            self.cursor = (self.cursor + 1) % WINDOW;
            taken += 1;
        }
    }

    fn channel(&mut self, right: bool, bands: &mut Bands, dt: f32) {
        let source = if right { &self.right } else { &self.left };
        for index in 0..WINDOW {
            self.re[index] = source[(self.cursor + index) % WINDOW] * self.window[index];
            self.im[index] = 0.0;
        }
        fft(&mut self.re, &mut self.im);
        let scale = 2.0 / WINDOW as f32;
        for (slot, edges) in self.edges.iter().enumerate() {
            let lane = if right { &mut bands.right[slot] } else { &mut bands.left[slot] };
            for (band, (from, to)) in edges.iter().enumerate() {
                let mut sum = 0.0f32;
                for bin in *from..*to {
                    sum += (self.re[bin] * self.re[bin] + self.im[bin] * self.im[bin]).sqrt();
                }
                let width = (to - from).max(1) as f32;
                let value = (sum * scale / width.sqrt() * self.gain).sqrt().min(4.0);
                let tau = if value > lane[band] { self.attack } else { self.release };
                let blend = 1.0 - (-dt / tau.max(1.0e-4)).exp();
                lane[band] += (value - lane[band]) * blend;
            }
        }
    }

    pub fn fill(&mut self, bands: &mut Bands, dt: f32) {
        let dt = dt.clamp(1.0 / 480.0, 0.25);
        if self.drain() == 0 {
            self.starved += dt;
            if self.starved >= HOLD {
                bands.silence(dt);
            }
            return;
        }
        let elapsed = dt + std::mem::take(&mut self.starved);
        self.channel(false, bands, elapsed);
        self.channel(true, bands, elapsed);
    }
}

#[cfg(test)]
#[path = "spectrum_tests.rs"]
mod tests;
