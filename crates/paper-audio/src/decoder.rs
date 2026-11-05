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

#[allow(clippy::too_many_arguments)]
pub(super) fn decode_loop(
    path: &str,
    producer: &mut RingProducer,
    stop: &AtomicBool,
    mute: &AtomicBool,
    paused: &AtomicBool,
    hold: &AtomicBool,
    restart: &AtomicBool,
    wake: &(Mutex<()>, Condvar),
    repeat: Repeat,
) -> Result<bool> {
    let mut input = open_input(path)?;

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
        let decoder = open_decoder(parameters)?;
        let format = decoder.format();
        let rate = decoder.rate();
        let channels = decoder.ch_layout().channels();
        (index, decoder, format, rate, channels)
    };

    let input_layout = if decoder.ch_layout().order() == ff::ChannelOrder::Unspecified {
        ff::ChannelLayout::default_for_channels(input_channels)
    } else {
        ff::ChannelLayout::from(decoder.ch_layout().into_owned())
    };
    if input_layout.mask().is_none() {
        return Err(anyhow!("unsupported audio channel layout: {}", input_layout.description()));
    }
    let target_layout = ff::ChannelLayout::default_for_channels(2);
    let target_format = ff::format::Sample::F32(ff::format::sample::Type::Planar);
    tracing::info!(
        "audio decode_loop: in_format={:?} in_rate={input_rate} in_channels={input_channels} target_format={:?} target_rate={TARGET_SAMPLE_RATE} target_channels=2",
        input_format,
        target_format,
    );
    let make_resampler = || {
        ff::software::resampling::Context::get2(
            input_format,
            input_layout.clone(),
            input_rate,
            target_format,
            target_layout.clone(),
            TARGET_SAMPLE_RATE,
        )
        .map_err(|error| anyhow!("swr init: {error}"))
    };
    let mut resampler = make_resampler()?;

    let mut decoded = ff::frame::Audio::empty();
    let mut resampled = ff::frame::Audio::empty();
    let mut interleave_buffer = Vec::new();
    let mut traced_pushes = 0;
    let gate = ParkGate { mute, paused, hold, restart, wake };

    while !stop.load(Ordering::Relaxed) {
        if restart.load(Ordering::Relaxed) {
            return Ok(true);
        }
        if mute.load(Ordering::Relaxed)
            || paused.load(Ordering::Relaxed)
            || hold.load(Ordering::Relaxed)
        {
            let guard = wake.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if should_park(
                mute.load(Ordering::Relaxed) || hold.load(Ordering::Relaxed),
                paused.load(Ordering::Relaxed),
                stop.load(Ordering::Relaxed) || restart.load(Ordering::Relaxed),
            ) {
                let _unused = wake.1.wait(guard);
            }
            continue;
        }
        let packet = read_audio_packet(&mut input, audio_index, stop);
        if packet.is_none() && stop.load(Ordering::Relaxed) {
            break;
        }
        decode_packet(
            &mut decoder,
            packet.as_ref(),
            &mut resampler,
            &mut decoded,
            &mut resampled,
            producer,
            &mut interleave_buffer,
            &mut traced_pushes,
            stop,
            &gate,
        );
        if packet.is_some() {
            continue;
        }
        drain_resampler(
            &mut resampler,
            &mut resampled,
            producer,
            &mut interleave_buffer,
            &mut traced_pushes,
            stop,
            &gate,
        );
        if repeat == Repeat::Once
            || interleave_buffer.is_empty()
            || stop.load(Ordering::Relaxed)
            || restart.load(Ordering::Relaxed)
        {
            break;
        }
        drop(resampler);
        drop(decoder);
        drop(input);
        input = open_input(path)?;
        let stream = input.stream(audio_index).ok_or_else(|| anyhow!("audio stream changed"))?;
        decoder = open_decoder(stream.parameters())?;
        resampler = make_resampler()?;
        resampled = ff::frame::Audio::empty();
        interleave_buffer.clear();
    }
    Ok(restart.load(Ordering::Relaxed))
}

fn open_input(path: &str) -> Result<ff::format::context::Input> {
    let mut input_options = ff::Dictionary::new();
    input_options.set("probesize", "65536");
    input_options.set("analyzeduration", "500000");
    ff::format::input_with_dictionary(path, input_options)
        .with_context(|| format!("audio open: {path}"))
}

fn open_decoder(parameters: ff::codec::ParametersRef<'_>) -> Result<ff::codec::decoder::Audio> {
    let mut context = ff::codec::context::Context::from_parameters(parameters)
        .with_context(|| "audio decoder ctx")?;
    context.set_threading(ff::codec::threading::Config {
        kind: ff::codec::threading::Type::None,
        count: 1,
    });
    context.decoder().audio().with_context(|| "open audio decoder")
}

#[allow(clippy::too_many_arguments)]
fn decode_packet(
    decoder: &mut ff::codec::decoder::Audio,
    packet: Option<&ff::Packet>,
    resampler: &mut ff::software::resampling::Context,
    decoded: &mut ff::frame::Audio,
    resampled: &mut ff::frame::Audio,
    producer: &mut RingProducer,
    interleave_buffer: &mut Vec<f32>,
    traced_pushes: &mut u32,
    stop: &AtomicBool,
    gate: &ParkGate<'_>,
) {
    let sent = match packet {
        Some(packet) => decoder.send_packet(packet),
        None => decoder.send_eof(),
    };
    if sent.is_err() {
        return;
    }
    while !stop.load(Ordering::Relaxed) && decoder.receive_frame(decoded).is_ok() {
        if decoded.ch_layout().order() == ff::ChannelOrder::Unspecified {
            decoded.set_ch_layout(ff::ChannelLayout::default_for_channels(
                decoded.ch_layout().channels(),
            ));
        }
        if resampler.run(decoded, resampled).is_err() {
            continue;
        }
        trace_push(traced_pushes, resampled);
        push_to_ring(producer, resampled, interleave_buffer, stop, gate);
    }
}

fn drain_resampler(
    resampler: &mut ff::software::resampling::Context,
    resampled: &mut ff::frame::Audio,
    producer: &mut RingProducer,
    interleave_buffer: &mut Vec<f32>,
    traced_pushes: &mut u32,
    stop: &AtomicBool,
    gate: &ParkGate<'_>,
) {
    while !stop.load(Ordering::Relaxed) && !gate.restart.load(Ordering::Relaxed) {
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
    pub(super) hold: &'a AtomicBool,
    pub(super) restart: &'a AtomicBool,
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
        if stop.load(Ordering::Relaxed) || gate.restart.load(Ordering::Relaxed) {
            return;
        }
        let pushed = producer.push_slice(&buffer[written..]);
        written += pushed;
        if pushed > 0 {
            continue;
        }
        if should_park(
            gate.mute.load(Ordering::Relaxed) || gate.hold.load(Ordering::Relaxed),
            gate.paused.load(Ordering::Relaxed),
            false,
        ) {
            let guard = gate.wake.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if should_park(
                gate.mute.load(Ordering::Relaxed) || gate.hold.load(Ordering::Relaxed),
                gate.paused.load(Ordering::Relaxed),
                stop.load(Ordering::Relaxed) || gate.restart.load(Ordering::Relaxed),
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
