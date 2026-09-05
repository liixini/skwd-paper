mod cli;
mod decoder;
mod ffi;
mod ivf;
mod model;
mod wayland;

use cli::{Options, ParseResult};
use decoder::{DecodedFrame, Decoder};
use ivf::ResidentVideo;
use model::{FrameClock, RuntimeStats, VideoInfo, monotonic_ns};
use paper_control::{CommandClass, PaperCommand, classify_command};
use paper_geom::{FillMode, fill_crop_rect, fit_scaled_size};
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use wayland::Wallpaper;

static RUNNING: AtomicBool = AtomicBool::new(true);
static PAUSED: AtomicBool = AtomicBool::new(false);
static WAKE_FD: AtomicI32 = AtomicI32::new(-1);
static FREEZE: Mutex<Option<String>> = Mutex::new(None);
static OUTPUTS: Mutex<Option<Vec<String>>> = Mutex::new(None);

extern "C" fn stop_running(_: libc::c_int) {
    RUNNING.store(false, Ordering::Relaxed);
    notify(WAKE_FD.load(Ordering::Relaxed));
}

fn running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

fn main() {
    let options = match Options::parse() {
        Ok(ParseResult::Run(options)) => options,
        Ok(ParseResult::Help) => {
            println!("{}", cli::usage());
            return;
        }
        Ok(ParseResult::Version) => {
            println!("skwd-paper-tinier {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Err(error) => {
            eprintln!("skwd-paper-tinier: {error}");
            eprintln!("{}", cli::usage());
            std::process::exit(2);
        }
    };
    unsafe {
        libc::signal(libc::SIGINT, stop_running as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, stop_running as *const () as libc::sighandler_t);
    }
    if let Err(error) = run(options) {
        eprintln!("skwd-paper-tinier: {error}");
        std::process::exit(1);
    }
}

fn run(options: Options) -> Result<(), String> {
    let source = ResidentVideo::load(&options.input)?;
    if !source.frame_rate().equivalent(options.frame_rate) {
        return Err(format!(
            "declared frame rate {} does not match IVF rate {}",
            options.frame_rate.label(),
            source.frame_rate().label()
        ));
    }
    let mut decoder = Decoder::open()?;
    let first_frame = decoder.decode(&source, 0)?;
    let video =
        VideoInfo::new(first_frame.width, first_frame.height, options.frame_rate, options.matrix)?;
    if options.probe_loops > 0 {
        return probe(
            &mut decoder,
            &source,
            video,
            &first_frame,
            options.probe_loops,
            options.decode_only,
        );
    }
    if let Some((width, height)) = options.stream_size {
        play_stream(
            decoder,
            &source,
            video,
            &first_frame,
            width,
            height,
            options.fill_mode,
            options.paused,
            !options.stream_no_header,
        )
    } else {
        play(decoder, &source, video, &first_frame, options.outputs, options.paused)
    }
}

fn probe(
    decoder: &mut Decoder,
    source: &ResidentVideo,
    video: VideoInfo,
    first_frame: &DecodedFrame,
    loop_count: u32,
    decode_only: bool,
) -> Result<(), String> {
    let mut canvas = if decode_only {
        Vec::new()
    } else {
        let mut canvas = Vec::new();
        canvas
            .try_reserve_exact(video.frame_bytes)
            .map_err(|error| format!("cannot allocate probe canvas: {error}"))?;
        canvas.resize(video.frame_bytes, 0);
        Decoder::convert(first_frame, video, &mut canvas)?;
        canvas
    };
    for pass in 0..loop_count {
        let first_packet = usize::from(pass == 0);
        for packet in first_packet..source.packet_count() {
            let frame = decoder.decode(source, packet)?;
            if !decode_only {
                Decoder::convert(&frame, video, &mut canvas)?;
            }
        }
    }
    eprintln!(
        "skwd-paper-tinier: decoded {loop_count} complete {}probe loops ({} AV1 packets each)",
        if decode_only { "decode-only " } else { "" },
        source.packet_count()
    );
    Ok(())
}

fn play(
    mut decoder: Decoder,
    source: &ResidentVideo,
    video: VideoInfo,
    first_frame: &DecodedFrame,
    outputs: Option<Vec<String>>,
    paused: bool,
) -> Result<(), String> {
    let wake = start_control(paused)?;
    let raw_fd = wake.as_raw_fd();
    let mut wallpaper = Wallpaper::connect(outputs, video)?;
    Decoder::convert(first_frame, video, wallpaper.pixels_mut(0))?;
    wallpaper.present(0)?;
    if let Err(error) = paper_control::signal_paper_ready() {
        eprintln!("skwd-paper-tinier: readiness signal failed: {error}");
    }
    let committed_at = monotonic_ns()?;
    let mut stats = RuntimeStats::default();
    stats.record(committed_at, 0, false);
    let output_sizes = wallpaper.output_sizes();
    eprintln!(
        "skwd-paper-tinier: {}x{}, {} AV1 packets, {} source bytes, {} indexed bytes at {:.3} fps ({}) -> {output_sizes}",
        video.width,
        video.height,
        source.packet_count(),
        source.source_bytes(),
        source.resident_bytes(),
        video.frame_rate.value(),
        video.matrix.label()
    );

    let result = playback_loop(
        &mut decoder,
        source,
        video,
        &mut wallpaper,
        &mut stats,
        committed_at,
        raw_fd,
    );
    WAKE_FD.store(-1, Ordering::Relaxed);
    stats.print();
    result
}

fn start_control(paused: bool) -> Result<Arc<OwnedFd>, String> {
    PAUSED.store(paused, Ordering::Relaxed);
    *FREEZE.lock().unwrap() = None;
    let wake = Arc::new(control_wake()?);
    let raw_fd = wake.as_raw_fd();
    WAKE_FD.store(raw_fd, Ordering::Relaxed);
    let control_wake = Arc::clone(&wake);
    paper_control::spawn_stdin_line_reader("skwd-paper-tinier", move |line| {
        let Ok(command) = serde_json::from_str::<PaperCommand>(line) else {
            return;
        };
        match classify_command(command) {
            CommandClass::Pause(paused) => PAUSED.store(paused, Ordering::Relaxed),
            CommandClass::Freeze(path) => {
                *FREEZE.lock().unwrap() = Some(path);
                PAUSED.store(true, Ordering::Relaxed);
            }
            CommandClass::RetainOutputs(outputs) => *OUTPUTS.lock().unwrap() = Some(outputs),
            _ => return,
        }
        notify(control_wake.as_raw_fd());
    });
    *OUTPUTS.lock().unwrap() = None;
    Ok(wake)
}

#[allow(clippy::too_many_arguments)]
fn play_stream(
    mut decoder: Decoder,
    source: &ResidentVideo,
    video: VideoInfo,
    first_frame: &DecodedFrame,
    width: u32,
    height: u32,
    fill_mode: FillMode,
    paused: bool,
    write_header: bool,
) -> Result<(), String> {
    let wake = start_control(paused)?;
    let wake_fd = wake.as_raw_fd();
    let mut source_pixels = vec![0; video.frame_bytes];
    Decoder::convert(first_frame, video, &mut source_pixels)?;
    let mut output = std::io::BufWriter::new(std::io::stdout().lock());
    if write_header {
        output.write_all(b"SKWP").map_err(|error| error.to_string())?;
        output.write_all(&width.to_le_bytes()).map_err(|error| error.to_string())?;
        output.write_all(&height.to_le_bytes()).map_err(|error| error.to_string())?;
    }
    let mut stream_frame = Vec::new();
    if !write_stream_frame(
        &mut output,
        &mut stream_frame,
        &source_pixels,
        video,
        width,
        height,
        fill_mode,
    )? {
        return Ok(());
    }
    paper_runtime::plasma::frame_ready().map_err(|error| error.to_string())?;
    if let Err(error) = paper_control::signal_paper_ready() {
        eprintln!("skwd-paper-tinier: readiness signal failed: {error}");
    }
    let mut next_packet = usize::from(source.packet_count() > 1);
    let mut clock = FrameClock::new(video.frame_rate, monotonic_ns()?);
    clock.advance();
    while running() {
        if let Some(path) = FREEZE.lock().unwrap().take()
            && let Err(error) = write_freeze(&path, &source_pixels, video)
        {
            let _ = write_freeze_error(&format!("{path}.error"), &error);
            PAUSED.store(false, Ordering::Relaxed);
        }
        if PAUSED.load(Ordering::Relaxed) {
            while PAUSED.load(Ordering::Relaxed) && running() {
                wait_stream(wake_fd, None)?;
                drain(wake_fd)?;
            }
            clock = FrameClock::new(video.frame_rate, monotonic_ns()?);
            clock.advance();
            continue;
        }
        if next_packet == source.packet_count() {
            next_packet = 0;
        }
        let frame = decoder.decode(source, next_packet)?;
        Decoder::convert(&frame, video, &mut source_pixels)?;
        next_packet += 1;
        wait_stream(wake_fd, Some(clock.deadline))?;
        if !running() || PAUSED.load(Ordering::Relaxed) {
            continue;
        }
        if !write_stream_frame(
            &mut output,
            &mut stream_frame,
            &source_pixels,
            video,
            width,
            height,
            fill_mode,
        )? {
            break;
        }
        let now = monotonic_ns()?;
        clock.advance();
        clock.recover_lag(now);
    }
    WAKE_FD.store(-1, Ordering::Relaxed);
    Ok(())
}

fn write_stream_frame(
    output: &mut impl Write,
    frame: &mut Vec<u8>,
    source: &[u8],
    video: VideoInfo,
    width: u32,
    height: u32,
    fill_mode: FillMode,
) -> Result<bool, String> {
    render_stream_frame(frame, source, video.width, video.height, width, height, fill_mode);
    match output.write_all(frame).and_then(|()| output.flush()) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

fn render_stream_frame(
    output: &mut Vec<u8>,
    source: &[u8],
    source_width: u32,
    source_height: u32,
    width: u32,
    height: u32,
    fill_mode: FillMode,
) {
    output.resize(width as usize * height as usize * 4, 0);
    for pixel in output.chunks_exact_mut(4) {
        pixel[0] = 0;
        pixel[1] = 0;
        pixel[2] = 0;
        pixel[3] = 255;
    }
    match fill_mode {
        FillMode::Stretch => copy_scaled(
            source,
            source_width,
            source_height,
            (0, 0, source_width, source_height),
            output,
            width,
            height,
            (0, 0, width, height),
        ),
        FillMode::Fill | FillMode::Span => {
            let source_rect = fill_crop_rect(source_width, source_height, width, height);
            copy_scaled(
                source,
                source_width,
                source_height,
                source_rect,
                output,
                width,
                height,
                (0, 0, width, height),
            );
        }
        FillMode::Fit => {
            let (target_width, target_height) =
                fit_scaled_size(source_width, source_height, width, height);
            copy_scaled(
                source,
                source_width,
                source_height,
                (0, 0, source_width, source_height),
                output,
                width,
                height,
                (
                    (width - target_width) / 2,
                    (height - target_height) / 2,
                    target_width,
                    target_height,
                ),
            );
        }
        FillMode::Center => {
            let copy_width = source_width.min(width);
            let copy_height = source_height.min(height);
            copy_scaled(
                source,
                source_width,
                source_height,
                (
                    (source_width - copy_width) / 2,
                    (source_height - copy_height) / 2,
                    copy_width,
                    copy_height,
                ),
                output,
                width,
                height,
                ((width - copy_width) / 2, (height - copy_height) / 2, copy_width, copy_height),
            );
        }
        FillMode::Tile => {
            for y in 0..height {
                for x in 0..width {
                    copy_pixel(
                        source,
                        source_width,
                        x % source_width,
                        y % source_height,
                        output,
                        width,
                        x,
                        y,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
fn compose_stream_frame(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    width: u32,
    height: u32,
    fill_mode: FillMode,
) -> Vec<u8> {
    let mut output = Vec::new();
    render_stream_frame(&mut output, source, source_width, source_height, width, height, fill_mode);
    output
}

#[allow(clippy::too_many_arguments)]
fn copy_scaled(
    source: &[u8],
    source_width: u32,
    _source_height: u32,
    (source_x, source_y, source_rect_width, source_rect_height): (u32, u32, u32, u32),
    output: &mut [u8],
    output_width: u32,
    _output_height: u32,
    (target_x, target_y, target_width, target_height): (u32, u32, u32, u32),
) {
    for y in 0..target_height {
        let sy = source_y + y * source_rect_height / target_height;
        for x in 0..target_width {
            let sx = source_x + x * source_rect_width / target_width;
            copy_pixel(
                source,
                source_width,
                sx,
                sy,
                output,
                output_width,
                target_x + x,
                target_y + y,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn copy_pixel(
    source: &[u8],
    source_width: u32,
    source_x: u32,
    source_y: u32,
    output: &mut [u8],
    output_width: u32,
    target_x: u32,
    target_y: u32,
) {
    let source_offset = (source_y as usize * source_width as usize + source_x as usize) * 4;
    let target_offset = (target_y as usize * output_width as usize + target_x as usize) * 4;
    output[target_offset] = source[source_offset + 2];
    output[target_offset + 1] = source[source_offset + 1];
    output[target_offset + 2] = source[source_offset];
    output[target_offset + 3] = 255;
}

fn wait_stream(wake_fd: RawFd, deadline: Option<u64>) -> Result<(), String> {
    loop {
        let timeout = if let Some(deadline) = deadline {
            let now = monotonic_ns()?;
            if now >= deadline {
                return Ok(());
            }
            i32::try_from((deadline - now).div_ceil(1_000_000).min(i32::MAX as u64)).unwrap()
        } else {
            -1
        };
        let mut descriptor = libc::pollfd { fd: wake_fd, events: libc::POLLIN, revents: 0 };
        let result = unsafe { libc::poll(&raw mut descriptor, 1, timeout) };
        if result >= 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(format!("stream wait failed: {error}"));
        }
    }
}

fn playback_loop(
    decoder: &mut Decoder,
    source: &ResidentVideo,
    video: VideoInfo,
    wallpaper: &mut Wallpaper,
    stats: &mut RuntimeStats,
    first_commit_ns: u64,
    wake_fd: RawFd,
) -> Result<(), String> {
    let mut current_buffer = 0;
    let mut next_packet = usize::from(source.packet_count() > 1);
    let mut clock = FrameClock::new(video.frame_rate, first_commit_ns);
    clock.advance();
    while wallpaper.alive() {
        if let Some(outputs) = OUTPUTS.lock().unwrap().take() {
            wallpaper.retain_outputs(&outputs)?;
        }
        if let Some(path) = FREEZE.lock().unwrap().take()
            && let Err(error) = write_freeze(&path, wallpaper.pixels_mut(current_buffer), video)
        {
            let _ = write_freeze_error(&format!("{path}.error"), &error);
            PAUSED.store(false, Ordering::Relaxed);
            eprintln!("skwd-paper-tinier: freeze frame failed: {error}");
        }
        if PAUSED.load(Ordering::Relaxed) {
            drain(wake_fd)?;
            while PAUSED.load(Ordering::Relaxed) && wallpaper.alive() {
                wallpaper.wait_idle(wake_fd)?;
                drain(wake_fd)?;
            }
            let now = monotonic_ns()?;
            clock = FrameClock::new(video.frame_rate, now);
            clock.advance();
            continue;
        }
        let next_buffer = current_buffer ^ 1;
        let target_deadline = clock.deadline;
        if !wallpaper.wait_for_buffer(next_buffer)? {
            break;
        }
        if next_packet == source.packet_count() {
            next_packet = 0;
        }
        let frame = decoder.decode(source, next_packet)?;
        Decoder::convert(&frame, video, wallpaper.pixels_mut(next_buffer))?;
        next_packet += 1;
        if !wallpaper.wait_until(target_deadline)? {
            break;
        }
        wallpaper.present(next_buffer)?;
        let committed_at = monotonic_ns()?;
        stats.record(committed_at, target_deadline, true);
        current_buffer = next_buffer;
        clock.advance();
        if clock.recover_lag(committed_at) {
            stats.deadline_reanchors += 1;
        }
    }
    Ok(())
}

fn write_freeze(path: &str, pixels: &[u8], video: VideoInfo) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let path = std::path::Path::new(path);
    let parent = path.parent().ok_or_else(|| "freeze-frame path has no parent".to_string())?;
    let name = path.file_name().ok_or_else(|| "freeze-frame path has no name".to_string())?;
    let temporary = parent.join(format!(".{}.part-{}", name.to_string_lossy(), std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| format!("create freeze frame: {error}"))?;
    let result = (|| {
        writeln!(file, "P6\n{} {}\n255", video.width, video.height)
            .map_err(|error| error.to_string())?;
        let width = usize::try_from(video.width).map_err(|error| error.to_string())?;
        let height = usize::try_from(video.height).map_err(|error| error.to_string())?;
        let stride = usize::try_from(video.stride).map_err(|error| error.to_string())?;
        let mut row = vec![0u8; width * 3];
        for y in 0..height {
            let source = &pixels[y * stride..y * stride + width * 4];
            for (bgra, rgb) in source.chunks_exact(4).zip(row.chunks_exact_mut(3)) {
                rgb.copy_from_slice(&[bgra[2], bgra[1], bgra[0]]);
            }
            file.write_all(&row).map_err(|error| error.to_string())?;
        }
        file.sync_all().map_err(|error| error.to_string())?;
        std::fs::hard_link(&temporary, path).map_err(|error| error.to_string())?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&temporary);
    result
}

fn write_freeze_error(path: &str, message: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let path = std::path::Path::new(path);
    let parent = path.parent().ok_or_else(|| "freeze error path has no parent".to_string())?;
    let name = path.file_name().ok_or_else(|| "freeze error path has no name".to_string())?;
    let temporary = parent.join(format!(".{}.part-{}", name.to_string_lossy(), std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let result = (|| {
        file.write_all(message.as_bytes()).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        std::fs::hard_link(&temporary, path).map_err(|error| error.to_string())?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&temporary);
    result
}

fn notify(fd: RawFd) {
    if fd < 0 {
        return;
    }
    let value = 1_u64;
    unsafe {
        libc::write(fd, (&raw const value).cast(), std::mem::size_of::<u64>());
    }
}

fn control_wake() -> Result<OwnedFd, String> {
    let raw_fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    if raw_fd < 0 {
        Err(format!("eventfd failed: {}", std::io::Error::last_os_error()))
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(raw_fd) })
    }
}

fn drain(fd: RawFd) -> Result<(), String> {
    let mut value = 0_u64;
    let result = unsafe { libc::read(fd, (&raw mut value).cast(), std::mem::size_of::<u64>()) };
    if result >= 0 || std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
        Ok(())
    } else {
        Err(format!("control wake read failed: {}", std::io::Error::last_os_error()))
    }
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
