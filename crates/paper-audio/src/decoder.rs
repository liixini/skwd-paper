use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use ffmpeg_the_third as ff;
use ringbuf::traits::Producer;

const TARGET_SAMPLE_RATE: u32 = 48_000;
pub(super) type RingProducer = ringbuf::HeapProd<f32>;

pub(super) fn wake_parked(wake: &(Mutex<()>, Condvar)) {
    let _guard = wake.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    wake.1.notify_all();
}

pub(super) fn signal_stop(stop: &AtomicBool, wake: &(Mutex<()>, Condvar)) {
    stop.store(true, Ordering::Relaxed);
    wake_parked(wake);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Repeat {
    Forever,
    Once,
}

pub(super) fn decode_loop(
    path: &str,
    producer: &mut RingProducer,
    stop: &AtomicBool,
    mute: &AtomicBool,
    paused: &AtomicBool,
    wake: &(Mutex<()>, Condvar),
    repeat: Repeat,
) -> Result<()> {
    let mut input_options = ff::Dictionary::new();
    input_options.set("probesize", "65536");
    input_options.set("analyzeduration", "500000");
    let mut input = ff::format::input_with_dictionary(path, input_options)
        .with_context(|| format!("audio open: {path}"))?;

    let (audio_index, mut decoder, input_format, input_rate, input_channels): (
        usize,
        ff::codec::decoder::Audio,
        ff::format::Sample,
        u32,
        u32,
    ) = {
        let stream = input
            .streams()
            .best(ff::media::Type::Audio)
            .ok_or_else(|| anyhow!("no audio stream"))?;
        let index = stream.index();
        let parameters = stream.parameters();
        let mut context = ff::codec::context::Context::from_parameters(parameters)
            .with_context(|| "audio decoder ctx")?;
        context.set_threading(ff::codec::threading::Config {
            kind: ff::codec::threading::Type::None,
            count: 1,
        });
        let decoder = context.decoder().audio().with_context(|| "open audio decoder")?;
        let format = decoder.format();
        let rate = decoder.rate();
        let channels = decoder.ch_layout().channels();
        (index, decoder, format, rate, channels)
    };

    let input_layout = ff::ChannelLayout::default_for_channels(input_channels);
    let target_layout = ff::ChannelLayout::default_for_channels(2);
    let target_format = ff::format::Sample::F32(ff::format::sample::Type::Planar);
    tracing::info!(
        "audio decode_loop: in_format={:?} in_rate={input_rate} in_channels={input_channels} target_format={:?} target_rate={TARGET_SAMPLE_RATE} target_channels=2",
        input_format,
        target_format,
    );
    let mut resampler = ff::software::resampling::Context::get2(
        input_format,
        input_layout,
        input_rate,
        target_format,
        target_layout,
        TARGET_SAMPLE_RATE,
    )
    .map_err(|error| anyhow!("swr init: {error}"))?;

    let mut decoded = ff::frame::Audio::empty();
    let mut resampled = ff::frame::Audio::empty();
    let mut interleave_buffer = Vec::new();
    let mut traced_pushes = 0;
    let gate = ParkGate { mute, paused, wake };

    while !stop.load(Ordering::Relaxed) {
        if mute.load(Ordering::Relaxed) || paused.load(Ordering::Relaxed) {
            let guard = wake.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if should_park(
                mute.load(Ordering::Relaxed),
                paused.load(Ordering::Relaxed),
                stop.load(Ordering::Relaxed),
            ) {
                let _unused = wake.1.wait(guard);
            }
            continue;
        }
        match read_audio_packet(&mut input, audio_index, stop) {
            Some(packet) => decode_packet(
                &mut decoder,
                &packet,
                &mut resampler,
                &mut decoded,
                &mut resampled,
                producer,
                &mut interleave_buffer,
                &mut traced_pushes,
                stop,
                &gate,
            ),
            None => {
                if repeat == Repeat::Once || input.seek(0, ..).is_err() {
                    break;
                }
                decoder.flush();
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn decode_packet(
    decoder: &mut ff::codec::decoder::Audio,
    packet: &ff::Packet,
    resampler: &mut ff::software::resampling::Context,
    decoded: &mut ff::frame::Audio,
    resampled: &mut ff::frame::Audio,
    producer: &mut RingProducer,
    interleave_buffer: &mut Vec<f32>,
    traced_pushes: &mut u32,
    stop: &AtomicBool,
    gate: &ParkGate<'_>,
) {
    if decoder.send_packet(packet).is_err() {
        return;
    }
    while !stop.load(Ordering::Relaxed) && decoder.receive_frame(decoded).is_ok() {
        if resampler.run(decoded, resampled).is_err() {
            continue;
        }
        trace_push(traced_pushes, resampled);
        push_to_ring(producer, resampled, interleave_buffer, stop, gate);
        drain_resampler(
            resampler,
            resampled,
            producer,
            interleave_buffer,
            traced_pushes,
            stop,
            gate,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn drain_resampler(
    resampler: &mut ff::software::resampling::Context,
    resampled: &mut ff::frame::Audio,
    producer: &mut RingProducer,
    interleave_buffer: &mut Vec<f32>,
    traced_pushes: &mut u32,
    stop: &AtomicBool,
    gate: &ParkGate<'_>,
) {
    while !stop.load(Ordering::Relaxed) && resampler.delay().is_some() {
        if resampler.flush(resampled).is_err() || resampled.samples() == 0 {
            break;
        }
        trace_push(traced_pushes, resampled);
        push_to_ring(producer, resampled, interleave_buffer, stop, gate);
    }
}

fn trace_push(traced_pushes: &mut u32, frame: &ff::frame::Audio) {
    if *traced_pushes >= 3 {
        return;
    }
    tracing::info!(
        "audio push #{}: format={:?} rate={} samples={} planes={} plane0_len={}",
        *traced_pushes,
        frame.format(),
        frame.rate(),
        frame.samples(),
        frame.planes(),
        if frame.planes() > 0 { frame.plane::<f32>(0).len() } else { 0 },
    );
    *traced_pushes += 1;
}

fn read_audio_packet(
    input: &mut ff::format::context::Input,
    audio_index: usize,
    stop: &AtomicBool,
) -> Option<ff::Packet> {
    for item in input.packets() {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        match item {
            Ok((stream, packet)) if stream.index() == audio_index => return Some(packet),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
}

pub(super) fn should_park(mute: bool, paused: bool, stop: bool) -> bool {
    (mute || paused) && !stop
}

pub(super) struct ParkGate<'a> {
    pub(super) mute: &'a AtomicBool,
    pub(super) paused: &'a AtomicBool,
    pub(super) wake: &'a (Mutex<()>, Condvar),
}

pub(super) fn push_to_ring(
    producer: &mut RingProducer,
    frame: &ff::frame::Audio,
    interleave_buffer: &mut Vec<f32>,
    stop: &AtomicBool,
    gate: &ParkGate<'_>,
) {
    let format = frame.format();
    if !matches!(format, ff::format::Sample::F32(_)) || frame.samples() == 0 || frame.planes() == 0
    {
        return;
    }
    interleave_buffer.clear();
    if format.is_packed() {
        interleave_packed(frame, interleave_buffer, frame.samples());
    } else {
        interleave_planar(frame, interleave_buffer, frame.samples(), frame.planes());
    }
    if !interleave_buffer.is_empty() {
        write_all(producer, interleave_buffer, stop, gate);
    }
}

fn interleave_packed(frame: &ff::frame::Audio, output: &mut Vec<f32>, sample_count: usize) {
    let plane = frame.plane::<f32>(0);
    let values_per_frame = (plane.len() / sample_count).max(1);
    let needed = sample_count * values_per_frame;
    if plane.len() >= needed {
        output.extend_from_slice(&plane[..needed]);
    }
}

fn interleave_planar(
    frame: &ff::frame::Audio,
    output: &mut Vec<f32>,
    sample_count: usize,
    plane_count: usize,
) {
    output.reserve(sample_count * plane_count);
    let planes: Vec<&[f32]> = (0..plane_count).map(|channel| frame.plane::<f32>(channel)).collect();
    for index in 0..sample_count {
        for plane in &planes {
            if index < plane.len() {
                output.push(plane[index]);
            }
        }
    }
}

fn write_all(producer: &mut RingProducer, buffer: &[f32], stop: &AtomicBool, gate: &ParkGate<'_>) {
    let mut written = 0;
    while written < buffer.len() {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let pushed = producer.push_slice(&buffer[written..]);
        written += pushed;
        if pushed > 0 {
            continue;
        }
        if should_park(
            gate.mute.load(Ordering::Relaxed),
            gate.paused.load(Ordering::Relaxed),
            false,
        ) {
            let guard = gate.wake.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if should_park(
                gate.mute.load(Ordering::Relaxed),
                gate.paused.load(Ordering::Relaxed),
                stop.load(Ordering::Relaxed),
            ) {
                let _unused = gate.wake.1.wait(guard);
            }
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[cfg(test)]
#[path = "decoder_tests.rs"]
mod tests;
