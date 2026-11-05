use anyhow::{Result, anyhow};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use std::sync::{Arc, Mutex};

pub const BAND_COUNTS: [usize; 3] = [16, 32, 64];
const WINDOW: usize = 2048;
const SPECTRUM_BINS: usize = 640;
const HOLD: f32 = 0.25;
const ATTACK_TAU: f32 = 0.045;
const RELEASE_TAU: f32 = 0.28;
const GAIN: f32 = 30.0;
const DEFAULT_MONITOR: &str = "@DEFAULT_MONITOR@";
const SOURCE_ENV: &str = "SKWD_VK_AUDIO_SOURCE";
const MODE_ENV: &str = "SKWD_VK_AUDIO_MODE";
const SYSTEM_MODE: &str = "system";
const PLAYBACK_MODE: &str = "wallpaper";
const BUFFER_WINDOWS: usize = 8;

#[derive(Clone, Default)]
pub struct PlaybackTap {
    producer: Arc<Mutex<Option<ringbuf::HeapProd<f32>>>>,
}

impl PlaybackTap {
    pub(crate) fn write(&self, samples: &[f32]) {
        let Ok(mut slot) = self.producer.try_lock() else {
            return;
        };
        let Some(producer) = slot.as_mut() else {
            return;
        };
        let channels = usize::from(crate::pulse::CHANNELS);
        let count = samples.len().min(producer.vacant_len()) / channels * channels;
        producer.push_slice(&samples[..count]);
    }
}

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

    fn update(&mut self, right: bool, levels: &[f32; 64], dt: f32, attack: f32, release: f32) {
        let lanes = if right { &mut self.right } else { &mut self.left };
        for (current, &target) in lanes[2].iter_mut().zip(levels) {
            let tau = if target > *current { attack } else { release };
            let blend = 1.0 - (-dt / tau.max(1.0e-4)).exp();
            *current += (target - *current) * blend;
        }
        let (reduced, full) = lanes.split_at_mut(2);
        for lane in reduced {
            let width = full[0].len() / lane.len();
            for (value, group) in lane.iter_mut().zip(full[0].chunks_exact(width)) {
                *value = group.iter().copied().fold(0.0, f32::max);
            }
        }
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

fn monitor_source(source: Option<&std::ffi::OsStr>) -> Result<&'static str> {
    match source {
        None => Ok(DEFAULT_MONITOR),
        Some(source) if source == DEFAULT_MONITOR => Ok(DEFAULT_MONITOR),
        Some(source) => {
            Err(anyhow!("SKWD_VK_AUDIO_SOURCE must be {DEFAULT_MONITOR}, got {source:?}"))
        }
    }
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
    let source_window = ((rate / 44_100.0).max(1.0) * 1920.0).floor();
    let bin_of = |bin: usize| (bin as f32 * WINDOW as f32 / source_window).round() as usize;
    let mut ranges = vec![(usize::MAX, 0); count];
    let mut band = 0;
    for bin in 1..SPECTRUM_BINS {
        let position = ((bin - 1) as f32 / (SPECTRUM_BINS - 1) as f32).powf(0.25);
        band = (band + 1).min((position * 64.0) as usize);
        let range = &mut ranges[band * count / 64];
        range.0 = range.0.min(bin);
        range.1 = bin + 1;
    }
    ranges
        .into_iter()
        .map(|(from, to)| {
            let from = bin_of(from).clamp(1, WINDOW / 2 - 1);
            let to = bin_of(to).clamp(from + 1, WINDOW / 2);
            (from, to)
        })
        .collect()
}

fn spectrum_levels(re: &[f32], im: &[f32], edges: &[(usize, usize)], gain: f32) -> [f32; 64] {
    let mut levels = [0.0; 64];
    let scale = 2.0 / WINDOW as f32;
    for (level, &(from, to)) in levels.iter_mut().zip(edges) {
        let magnitude = (from..to).map(|bin| re[bin].hypot(im[bin])).sum::<f32>();
        let width = (to - from).max(1) as f32;
        let value = (magnitude * scale / width.sqrt() * gain).sqrt();
        *level = if value.is_nan() { 0.0 } else { value.clamp(0.0, 1.0) };
    }
    levels
}

pub struct Analyser {
    _capture: Option<crate::pulse::MonitorCapture>,
    tap: PlaybackTap,
    consumer: ringbuf::HeapCons<f32>,
    channels: usize,
    edges: Vec<(usize, usize)>,
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
        let source = std::env::var_os(SOURCE_ENV);
        let mode = std::env::var_os(MODE_ENV);
        Self::select(source.as_deref(), mode.as_deref(), crate::pulse::MonitorCapture::start)
    }

    fn select(
        source: Option<&std::ffi::OsStr>,
        mode: Option<&std::ffi::OsStr>,
        capture: impl FnOnce(&str, ringbuf::HeapProd<f32>) -> Result<crate::pulse::MonitorCapture>,
    ) -> Result<Self> {
        monitor_source(source)?;
        match mode {
            Some(mode) if mode == PLAYBACK_MODE => Ok(Self::playback()),
            None => Ok(Self::monitor_or_playback(source, capture)),
            Some(mode) if mode == SYSTEM_MODE => Ok(Self::monitor_or_playback(source, capture)),
            Some(mode) => Err(anyhow!("invalid audio spectrum mode: {}", mode.display())),
        }
    }

    fn monitor_or_playback(
        source: Option<&std::ffi::OsStr>,
        capture: impl FnOnce(&str, ringbuf::HeapProd<f32>) -> Result<crate::pulse::MonitorCapture>,
    ) -> Self {
        match Self::start_with_capture(source, capture) {
            Ok(analyser) => analyser,
            Err(error) => {
                tracing::warn!(%error, "audio monitor unavailable; using wallpaper playback");
                Self::playback()
            }
        }
    }

    fn playback() -> Self {
        let (producer, consumer) = ringbuf::HeapRb::<f32>::new(
            WINDOW * usize::from(crate::pulse::CHANNELS) * BUFFER_WINDOWS,
        )
        .split();
        let mut analyser = Self::with_capture(
            None,
            consumer,
            crate::pulse::RATE as f32,
            usize::from(crate::pulse::CHANNELS),
        );
        analyser.tap = PlaybackTap { producer: Arc::new(Mutex::new(Some(producer))) };
        analyser
    }

    pub fn playback_tap(&self) -> PlaybackTap {
        self.tap.clone()
    }

    pub fn set_system_capture(&mut self, enabled: bool) -> Result<()> {
        let mode = if enabled { SYSTEM_MODE } else { PLAYBACK_MODE };
        let source = std::env::var_os(SOURCE_ENV);
        let next = Self::select(
            source.as_deref(),
            Some(std::ffi::OsStr::new(mode)),
            crate::pulse::MonitorCapture::start,
        )?;
        self.replace(next);
        Ok(())
    }

    fn replace(&mut self, mut next: Self) {
        let producer =
            next.tap.producer.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        *self.tap.producer.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = producer;
        next.tap = self.tap.clone();
        *self = next;
    }

    fn start_with_capture(
        source: Option<&std::ffi::OsStr>,
        capture: impl FnOnce(&str, ringbuf::HeapProd<f32>) -> Result<crate::pulse::MonitorCapture>,
    ) -> Result<Self> {
        let source = monitor_source(source)?;
        let (producer, consumer) = ringbuf::HeapRb::<f32>::new(
            WINDOW * usize::from(crate::pulse::CHANNELS) * BUFFER_WINDOWS,
        )
        .split();
        let capture = capture(source, producer)?;
        tracing::info!(
            "skwd-wall-vk: audio capture on pulse monitor source={source} rate={} channels={}",
            crate::pulse::RATE,
            crate::pulse::CHANNELS
        );
        Ok(Self::with_capture(
            Some(capture),
            consumer,
            crate::pulse::RATE as f32,
            usize::from(crate::pulse::CHANNELS),
        ))
    }

    fn with_capture(
        capture: Option<crate::pulse::MonitorCapture>,
        consumer: ringbuf::HeapCons<f32>,
        rate: f32,
        channels: usize,
    ) -> Self {
        let gain = std::env::var("SKWD_VK_AUDIO_GAIN")
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or(GAIN);
        Self {
            _capture: capture,
            tap: PlaybackTap::default(),
            consumer,
            channels,
            edges: band_edges(64, rate),
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
        }
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
        let levels = spectrum_levels(&self.re, &self.im, &self.edges, self.gain);
        bands.update(right, &levels, dt, self.attack, self.release);
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
