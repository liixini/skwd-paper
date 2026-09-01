use anyhow::{Context, Result, anyhow};
use ffmpeg_the_third as ff;

const HW_THREAD_COUNT: i32 = 1;
const CONSECUTIVE_PACKET_ERROR_LIMIT: usize = 16;
const EMPTY_CONTAINER_WRAP_LIMIT: usize = 2;

fn packet_error_is_fatal(consecutive_errors: &mut usize) -> bool {
    *consecutive_errors += 1;
    *consecutive_errors >= CONSECUTIVE_PACKET_ERROR_LIMIT
}

fn container_wrap_is_fatal(wraps: &mut usize) -> bool {
    *wraps += 1;
    *wraps >= EMPTY_CONTAINER_WRAP_LIMIT
}

pub fn probe_dims(path: &str) -> Option<(u32, u32)> {
    ff::format::input(path).ok().and_then(|ictx| {
        let stream = ictx.streams().best(ff::media::Type::Video)?;
        let parameters = stream.parameters();
        let dims = (parameters.width(), parameters.height());
        (dims.0 > 0 && dims.1 > 0).then_some(dims)
    })
}

struct Container {
    ictx: ff::format::context::Input,
    stream_index: usize,
    time_base_secs: f64,
    still: bool,
}

impl Container {
    fn open(path: &str) -> Result<Self> {
        ff::init().context("ffmpeg init")?;
        let ictx = ff::format::input(path).with_context(|| format!("open {path}"))?;
        let demuxer = ictx.format().name().to_string();
        let still = demuxer.contains("image2") || demuxer.ends_with("_pipe");
        let stream = ictx
            .streams()
            .best(ff::media::Type::Video)
            .ok_or_else(|| anyhow!("no video stream"))?;
        let stream_index = stream.index();
        let tb = stream.time_base();
        let time_base_secs = f64::from(tb.numerator()) / f64::from(tb.denominator()).max(1.0);
        Ok(Self { ictx, stream_index, time_base_secs, still })
    }

    fn open_hardware(path: &str) -> Result<Self> {
        let container = Self::open(path)?;
        if container.still {
            return Err(anyhow!("still image container has no hardware decode path"));
        }
        Ok(container)
    }

    fn codec_context(&self) -> Result<ff::codec::context::Context> {
        let stream =
            self.ictx.stream(self.stream_index).ok_or_else(|| anyhow!("no video stream"))?;
        Ok(ff::codec::context::Context::from_parameters(stream.parameters())?)
    }
}

pub struct VulkanDecoder {
    ictx: ff::format::context::Input,
    abort: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    decoder: ff::decoder::Video,
    draining: bool,
    stream_index: usize,
    pub time_base_secs: f64,
    pub width: u32,
    pub height: u32,
    primed: Option<(ff::frame::Video, f64)>,
    _hw: HwDevice,
}

struct HwDevice(*mut ff::ffi::AVBufferRef);
unsafe impl Send for HwDevice {}
unsafe impl Sync for HwDevice {}

impl Drop for HwDevice {
    fn drop(&mut self) {
        unsafe { ff::ffi::av_buffer_unref(&mut self.0) };
    }
}

struct SharedVaapiDevice {
    render_node: Option<std::path::PathBuf>,
    device: std::sync::Weak<HwDevice>,
}

static VAAPI_DEVICE: std::sync::Mutex<Option<SharedVaapiDevice>> = std::sync::Mutex::new(None);
// FFmpeg's hardware-device constructors may enter Vulkan loader/ICD discovery.  In particular,
// the NVIDIA ICD can fault when two first-time discoveries race in one process.  Device creation
// is a startup-only operation, so keep that native boundary process-serial while allowing the
// resulting decoders to run concurrently.
static HARDWARE_DEVICE_CREATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn hardware_device_create_guard() -> std::sync::MutexGuard<'static, ()> {
    HARDWARE_DEVICE_CREATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn create_vaapi_device(render_node: Option<&std::path::Path>) -> Result<HwDevice> {
    let node = render_node
        .map(|path| std::ffi::CString::new(path.as_os_str().as_encoded_bytes()))
        .transpose()
        .context("VAAPI render node contains NUL")?;
    let mut dev: *mut ff::ffi::AVBufferRef = std::ptr::null_mut();
    let rc = {
        let _create_guard = hardware_device_create_guard();
        unsafe {
            ff::ffi::av_hwdevice_ctx_create(
                &mut dev,
                ff::ffi::AVHWDeviceType::VAAPI,
                node.as_ref().map_or(std::ptr::null(), |path| path.as_ptr()),
                std::ptr::null_mut(),
                0,
            )
        }
    };
    if rc < 0 || dev.is_null() {
        return Err(anyhow!("VAAPI hwdevice create failed ({rc})"));
    }
    Ok(HwDevice(dev))
}

const fn vaapi_dimensions_supported(width: u32, height: u32) -> bool {
    width.is_multiple_of(2) && height.is_multiple_of(2)
}

fn forget_vaapi_device(device: &std::sync::Arc<HwDevice>) {
    let mut cached = VAAPI_DEVICE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if cached.as_ref().is_some_and(|shared| {
        shared.device.upgrade().is_some_and(|held| std::sync::Arc::ptr_eq(&held, device))
    }) {
        *cached = None;
    }
}

fn vaapi_device(render_node: Option<&std::path::Path>) -> Result<std::sync::Arc<HwDevice>> {
    let mut cached = VAAPI_DEVICE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(shared) = cached.as_ref()
        && shared.render_node.as_deref() == render_node
        && let Some(device) = shared.device.upgrade()
    {
        return Ok(device);
    }
    let device = std::sync::Arc::new(create_vaapi_device(render_node)?);
    *cached = Some(SharedVaapiDevice {
        render_node: render_node.map(std::path::Path::to_path_buf),
        device: std::sync::Arc::downgrade(&device),
    });
    Ok(device)
}

unsafe extern "C" fn pick_vulkan(
    _ctx: *mut ff::ffi::AVCodecContext,
    mut fmts: *const ff::ffi::AVPixelFormat,
) -> ff::ffi::AVPixelFormat {
    unsafe {
        while (*fmts).0 != -1 {
            if *fmts == ff::ffi::AVPixelFormat::VULKAN {
                return *fmts;
            }
            fmts = fmts.add(1);
        }
    }
    ff::ffi::AVPixelFormat(-1)
}

unsafe extern "C" fn pick_vaapi(
    _ctx: *mut ff::ffi::AVCodecContext,
    mut fmts: *const ff::ffi::AVPixelFormat,
) -> ff::ffi::AVPixelFormat {
    unsafe {
        while (*fmts).0 != -1 {
            if *fmts == ff::ffi::AVPixelFormat::VAAPI {
                return *fmts;
            }
            fmts = fmts.add(1);
        }
    }
    ff::ffi::AVPixelFormat(-1)
}

impl VulkanDecoder {
    #[cfg(feature = "shared-device")]
    pub fn open_with(path: &str, hwdev: *mut ff::ffi::AVBufferRef) -> Result<Self> {
        let dev = unsafe { ff::ffi::av_buffer_ref(hwdev) };
        tracing::info!("skwd-wall-vk: decoder on shared device");
        Self::finish_open(path, dev)
    }

    pub fn open(path: &str) -> Result<Self> {
        let mut dev: *mut ff::ffi::AVBufferRef = std::ptr::null_mut();
        let mode = std::env::var("SKWD_VK_FRAMES").unwrap_or_else(|_| "multiplane_off".into());
        let rc = {
            let _create_guard = hardware_device_create_guard();
            unsafe {
                let mut opts: *mut ff::ffi::AVDictionary = std::ptr::null_mut();
                match mode.as_str() {
                    "linear" => {
                        ff::ffi::av_dict_set(
                            &mut opts,
                            c"linear_images".as_ptr(),
                            c"1".as_ptr(),
                            0,
                        );
                    }
                    "multiplane_off" => {
                        ff::ffi::av_dict_set(
                            &mut opts,
                            c"disable_multiplane".as_ptr(),
                            c"1".as_ptr(),
                            0,
                        );
                    }
                    _ => {}
                }
                let rc = ff::ffi::av_hwdevice_ctx_create(
                    &mut dev,
                    ff::ffi::AVHWDeviceType::VULKAN,
                    std::ptr::null(),
                    opts,
                    0,
                );
                ff::ffi::av_dict_free(&mut opts);
                rc
            }
        };
        if rc < 0 || dev.is_null() {
            return Err(anyhow!("vulkan hwdevice create failed ({rc}, mode={mode})"));
        }
        tracing::info!("skwd-wall-vk: vulkan frames mode={mode}");
        Self::finish_open(path, dev)
    }

    fn finish_open(path: &str, dev: *mut ff::ffi::AVBufferRef) -> Result<Self> {
        let hw = HwDevice(dev);
        let container = Container::open_hardware(path)?;
        let mut ctx = container.codec_context()?;
        unsafe {
            let raw = ctx.as_mut_ptr();
            (*raw).hw_device_ctx = ff::ffi::av_buffer_ref(hw.0);
            (*raw).get_format = Some(pick_vulkan);
            (*raw).thread_count = HW_THREAD_COUNT;
            (*raw).thread_type = 0;
        }
        let is_av1 = unsafe { (*ctx.as_mut_ptr()).codec_id == ff::ffi::AVCodecID::AV1 };
        let decoder = if is_av1 {
            let named =
                ff::codec::decoder::find_by_name("av1").ok_or_else(|| anyhow!("no native av1"))?;
            ctx.decoder().open_as(named).context("open native av1")?.video().context("av1 video")?
        } else {
            ctx.decoder().video().context("open decoder")?
        };
        let (width, height) = (decoder.width(), decoder.height());
        let Container { ictx, stream_index, time_base_secs, .. } = container;
        let mut decoder = Self {
            ictx,
            abort: None,
            decoder,
            draining: false,
            stream_index,
            time_base_secs,
            width,
            height,
            primed: None,
            _hw: hw,
        };
        let first = decoder.decode_hw_frame().context("decode first Vulkan frame")?;
        if first.0.format() != ff::format::Pixel::VULKAN {
            return Err(anyhow!("Vulkan decoder returned {:?}", first.0.format()));
        }
        decoder.primed = Some(first);
        Ok(decoder)
    }

    pub fn next_frame(&mut self) -> Result<(MappedFrame, f64)> {
        let (frame, pts) = self.next_hw_frame()?;
        Ok((map_to_drm(&frame)?, pts))
    }

    pub fn next_frame_retained(
        &mut self,
        retained: &mut Option<ff::frame::Video>,
    ) -> Result<(MappedFrame, f64)> {
        let (frame, pts) = self.next_hw_frame()?;
        *retained = Some(retain_frame(&frame)?);
        Ok((map_to_drm(&frame)?, pts))
    }

    pub fn next_frame_cpu(&mut self, out: &mut ff::frame::Video) -> Result<f64> {
        let (frame, pts) = self.next_hw_frame()?;
        let rc = unsafe { ff::ffi::av_hwframe_transfer_data(out.as_mut_ptr(), frame.as_ptr(), 0) };
        if rc < 0 {
            return Err(anyhow!("hwframe transfer failed ({rc})"));
        }
        Ok(pts)
    }

    pub fn next_hw_frame(&mut self) -> Result<(ff::frame::Video, f64)> {
        if let Some(frame) = self.primed.take() {
            return Ok(frame);
        }
        self.decode_hw_frame()
    }

    fn decode_hw_frame(&mut self) -> Result<(ff::frame::Video, f64)> {
        pump_decode(
            &mut self.ictx,
            &mut self.decoder,
            &mut self.draining,
            self.stream_index,
            self.time_base_secs,
            self.abort.as_deref(),
        )
    }
}

pub struct VaapiDecoder {
    ictx: ff::format::context::Input,
    abort: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    decoder: ff::decoder::Video,
    draining: bool,
    stream_index: usize,
    pub time_base_secs: f64,
    pub width: u32,
    pub height: u32,
    primed: Option<(ff::frame::Video, f64)>,
    scaler: Option<ff::software::scaling::Context>,
    nv12_ring: [ff::frame::Video; 3],
    nv12_idx: usize,
    drm_prime: bool,
    _hw: std::sync::Arc<HwDevice>,
}

unsafe impl Send for VaapiDecoder {}

impl VaapiDecoder {
    pub fn open(path: &str, render_node: Option<&std::path::Path>) -> Result<Self> {
        let container = Container::open_hardware(path)?;
        let hw = vaapi_device(render_node)?;
        let mut ctx = match container.codec_context() {
            Ok(ctx) => ctx,
            Err(error) => {
                forget_vaapi_device(&hw);
                return Err(error);
            }
        };
        unsafe {
            let raw = ctx.as_mut_ptr();
            (*raw).hw_device_ctx = ff::ffi::av_buffer_ref(hw.0);
            (*raw).get_format = Some(pick_vaapi);
            (*raw).thread_count = HW_THREAD_COUNT;
            (*raw).thread_type = 0;
        }
        let decoder = match ctx.decoder().video().context("open VAAPI decoder") {
            Ok(decoder) => decoder,
            Err(error) => {
                forget_vaapi_device(&hw);
                return Err(error);
            }
        };
        let (width, height) = (decoder.width(), decoder.height());
        if !vaapi_dimensions_supported(width, height) {
            forget_vaapi_device(&hw);
            return Err(anyhow!("VAAPI needs even dimensions, got {width}x{height}"));
        }
        let Container { ictx, stream_index, time_base_secs, .. } = container;
        let mut decoder = Self {
            ictx,
            abort: None,
            decoder,
            draining: false,
            stream_index,
            time_base_secs,
            width,
            height,
            primed: None,
            scaler: None,
            nv12_ring: std::array::from_fn(|_| ff::frame::Video::empty()),
            nv12_idx: 0,
            drm_prime: true,
            _hw: hw,
        };
        let first = match decoder.decode_hw_frame().context("decode first VAAPI frame") {
            Ok(first) => first,
            Err(error) => {
                forget_vaapi_device(&decoder._hw);
                return Err(error);
            }
        };
        decoder.primed = Some(first);
        tracing::info!(
            "skwd-wall-vk: VAAPI hardware decode {width}x{height} on {}",
            render_node.map_or_else(|| "default device".into(), |node| node.display().to_string())
        );
        Ok(decoder)
    }

    pub fn next(&mut self) -> Result<(ff::frame::Video, f64)> {
        let (frame, pts) = self.next_hw_frame()?;
        self.transfer_frame(frame, pts)
    }

    pub fn next_hw_frame(&mut self) -> Result<(ff::frame::Video, f64)> {
        if let Some(frame) = self.primed.take() {
            return Ok(frame);
        }
        self.decode_hw_frame()
    }

    pub fn next_render(&mut self) -> Result<(RenderFrame, f64)> {
        let (frame, pts) = self.next_hw_frame()?;
        if !self.drm_prime {
            return self
                .transfer_frame(frame, pts)
                .map(|(frame, pts)| (RenderFrame::plain(frame), pts));
        }
        match map_to_drm(&frame) {
            Ok(mapped) => Ok((RenderFrame::mapped(frame, mapped), pts)),
            Err(error) => {
                self.drm_prime = false;
                tracing::info!(
                    target: "skwd_wall::video_path",
                    "skwd-wall-vk: VAAPI DRM-PRIME mapping unavailable ({error:#}), using CPU transfer"
                );
                self.transfer_frame(frame, pts).map(|(frame, pts)| (RenderFrame::plain(frame), pts))
            }
        }
    }

    fn decode_hw_frame(&mut self) -> Result<(ff::frame::Video, f64)> {
        let (frame, pts) = pump_decode(
            &mut self.ictx,
            &mut self.decoder,
            &mut self.draining,
            self.stream_index,
            self.time_base_secs,
            self.abort.as_deref(),
        )?;
        if frame.format() != ff::format::Pixel::VAAPI {
            return Err(anyhow!("VAAPI decoder returned {:?}", frame.format()));
        }
        Ok((frame, pts))
    }

    fn transfer_frame(
        &mut self,
        frame: ff::frame::Video,
        pts: f64,
    ) -> Result<(ff::frame::Video, f64)> {
        let mut transferred = ff::frame::Video::empty();
        let rc = unsafe {
            ff::ffi::av_hwframe_transfer_data(transferred.as_mut_ptr(), frame.as_ptr(), 0)
        };
        if rc < 0 {
            return Err(anyhow!("VAAPI frame transfer failed ({rc})"));
        }
        if transferred.format() == ff::format::Pixel::NV12 {
            return Ok((transferred, pts));
        }
        let scaler = match &mut self.scaler {
            Some(scaler) => scaler,
            None => self.scaler.insert(
                ff::software::scaling::Context::get(
                    transferred.format(),
                    transferred.width(),
                    transferred.height(),
                    ff::format::Pixel::NV12,
                    self.width,
                    self.height,
                    ff::software::scaling::Flags::BILINEAR,
                )
                .context("VAAPI NV12 scaler")?,
            ),
        };
        self.nv12_idx = (self.nv12_idx + 1) % self.nv12_ring.len();
        let output = &mut self.nv12_ring[self.nv12_idx];
        if unsafe { !output.is_empty() && ff::ffi::av_frame_is_writable(output.as_mut_ptr()) <= 0 }
        {
            *output = ff::frame::Video::empty();
        }
        scaler.run(&transferred, output).context("VAAPI NV12 convert")?;
        let cloned = unsafe { ff::ffi::av_frame_clone(output.as_ptr()) };
        if cloned.is_null() {
            return Err(anyhow!("av_frame_clone failed"));
        }
        Ok((unsafe { ff::frame::Video::wrap(cloned) }, pts))
    }
}

fn pump_decode(
    ictx: &mut ff::format::context::Input,
    decoder: &mut ff::decoder::Video,
    draining: &mut bool,
    stream_index: usize,
    time_base_secs: f64,
    abort: Option<&std::sync::atomic::AtomicBool>,
) -> Result<(ff::frame::Video, f64)> {
    static PTS_LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let pts_log = *PTS_LOG.get_or_init(|| std::env::var("SKWD_VK_PTS_LOG").is_ok());
    let mut consecutive_packet_errors = 0;
    let mut container_wraps = 0;
    loop {
        if abort.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
            return Err(anyhow!("decode canceled"));
        }
        let mut frame = ff::frame::Video::empty();
        if decoder.receive_frame(&mut frame).is_ok() {
            let pts = frame.pts().unwrap_or(0) as f64 * time_base_secs;
            if pts_log {
                tracing::info!("pts raw={:?} secs={:.4}", frame.pts(), pts);
            }
            return Ok((frame, pts));
        }
        if *draining {
            decoder.flush();
            ictx.seek(0, ..).context("loop seek")?;
            *draining = false;
            if container_wrap_is_fatal(&mut container_wraps) {
                return Err(anyhow!("no frame decoded across a full pass of the container"));
            }
            continue;
        }
        let mut packet = ff::codec::packet::Packet::empty();
        if packet.read(ictx).is_ok() {
            if packet.stream() == stream_index {
                match decoder.send_packet(&packet) {
                    Ok(()) => {
                        consecutive_packet_errors = 0;
                    }
                    Err(error) if packet_error_is_fatal(&mut consecutive_packet_errors) => {
                        return Err(error).context("send video packets repeatedly");
                    }
                    Err(_) => {}
                }
            }
        } else {
            decoder.send_eof().context("send video eof")?;
            *draining = true;
        }
    }
}

pub struct SwDecoder {
    ictx: ff::format::context::Input,
    abort: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    decoder: ff::decoder::Video,
    draining: bool,
    stream_index: usize,
    pub time_base_secs: f64,
    pub width: u32,
    pub height: u32,
    pub still: bool,
    served: bool,
    scaler: Option<ff::software::scaling::Context>,
    nv12_ring: [ff::frame::Video; 3],
    nv12_idx: usize,
}

fn sw_thread_count(width: u32, height: u32) -> i32 {
    let cap = if u64::from(width) * u64::from(height) > 2_100_000 { 8 } else { 4 };
    std::thread::available_parallelism().map_or(2, |count| count.get() as i32).clamp(2, cap)
}

fn resolved_thread_count(requested: i32, width: u32, height: u32) -> i32 {
    if requested == -1 {
        if u64::from(width) * u64::from(height) <= 1366 * 768 { 1 } else { 2 }
    } else if requested > 0 {
        requested
    } else {
        sw_thread_count(width, height)
    }
}

unsafe impl Send for SwDecoder {}

impl SwDecoder {
    pub fn open(path: &str) -> Result<Self> {
        Self::open_threads(path, 0)
    }

    pub fn open_threads(path: &str, threads: i32) -> Result<Self> {
        let container = Container::open(path)?;
        let mut ctx = container.codec_context()?;
        unsafe {
            let raw = ctx.as_mut_ptr();
            (*raw).thread_count =
                resolved_thread_count(threads, (*raw).width as u32, (*raw).height as u32);
        }
        let decoder = ctx.decoder().video().context("open sw decoder")?;
        let (width, height) = (decoder.width(), decoder.height());
        let Container { ictx, stream_index, time_base_secs, still } = container;
        tracing::info!(
            "skwd-wall-vk: software decode {width}x{height}{}",
            if still { " (still)" } else { "" }
        );
        Ok(Self {
            ictx,
            abort: None,
            decoder,
            draining: false,
            stream_index,
            time_base_secs,
            width,
            height,
            still,
            served: false,
            scaler: None,
            nv12_ring: std::array::from_fn(|_| ff::frame::Video::empty()),
            nv12_idx: 0,
        })
    }

    pub fn next_raw(&mut self) -> Result<(ff::frame::Video, f64)> {
        if self.still && self.served {
            return Err(anyhow!("still source exhausted"));
        }
        let (frame, pts) = pump_decode(
            &mut self.ictx,
            &mut self.decoder,
            &mut self.draining,
            self.stream_index,
            self.time_base_secs,
            self.abort.as_deref(),
        )?;
        self.served = true;
        Ok((frame, pts))
    }

    pub fn next(&mut self) -> Result<(ff::frame::Video, f64)> {
        if self.still && self.served {
            return Err(anyhow!("still source exhausted"));
        }
        let (frame, pts) = pump_decode(
            &mut self.ictx,
            &mut self.decoder,
            &mut self.draining,
            self.stream_index,
            self.time_base_secs,
            self.abort.as_deref(),
        )?;
        self.served = true;
        if frame.format() == ff::format::Pixel::NV12 {
            return Ok((frame, pts));
        }
        let scaler = match &mut self.scaler {
            Some(sc) => sc,
            None => self.scaler.insert(
                ff::software::scaling::Context::get(
                    frame.format(),
                    frame.width(),
                    frame.height(),
                    ff::format::Pixel::NV12,
                    self.width,
                    self.height,
                    ff::software::scaling::Flags::BILINEAR,
                )
                .context("nv12 scaler")?,
            ),
        };
        self.nv12_idx = (self.nv12_idx + 1) % self.nv12_ring.len();
        let entry = &mut self.nv12_ring[self.nv12_idx];
        let stale =
            unsafe { !entry.is_empty() && ff::ffi::av_frame_is_writable(entry.as_mut_ptr()) <= 0 };
        if stale {
            *entry = ff::frame::Video::empty();
        }
        scaler.run(&frame, entry).context("nv12 convert")?;
        let cloned = unsafe { ff::ffi::av_frame_clone(entry.as_ptr()) };
        if cloned.is_null() {
            return Err(anyhow!("av_frame_clone failed"));
        }
        Ok((unsafe { ff::frame::Video::wrap(cloned) }, pts))
    }
}

unsafe extern "C" fn interrupt_cb(opaque: *mut std::ffi::c_void) -> i32 {
    let flag = unsafe { &*(opaque as *const std::sync::atomic::AtomicBool) };
    i32::from(flag.load(std::sync::atomic::Ordering::Relaxed))
}

pub enum AnyDecoder {
    Vk(VulkanDecoder),
    Vaapi(VaapiDecoder),
    Sw(SwDecoder),
}

#[derive(Clone)]
pub struct RenderFrame {
    frame: ff::frame::Video,
    mapped: Option<std::sync::Arc<MappedFrame>>,
}

impl RenderFrame {
    pub(crate) fn plain(frame: ff::frame::Video) -> Self {
        Self { frame, mapped: None }
    }

    fn mapped(frame: ff::frame::Video, mapped: MappedFrame) -> Self {
        Self { frame, mapped: Some(std::sync::Arc::new(mapped)) }
    }

    pub fn video(&self) -> &ff::frame::Video {
        &self.frame
    }

    pub(crate) fn mapped_frame(&self) -> Option<&MappedFrame> {
        self.mapped.as_deref()
    }
}

impl std::ops::Deref for RenderFrame {
    type Target = ff::frame::Video;

    fn deref(&self) -> &Self::Target {
        self.video()
    }
}

impl AnyDecoder {
    pub fn dims(&self) -> (u32, u32) {
        match self {
            AnyDecoder::Vk(dec) => (dec.width, dec.height),
            AnyDecoder::Vaapi(dec) => (dec.width, dec.height),
            AnyDecoder::Sw(dec) => (dec.width, dec.height),
        }
    }

    pub fn still(&self) -> bool {
        match self {
            AnyDecoder::Vk(_) => false,
            AnyDecoder::Vaapi(_) => false,
            AnyDecoder::Sw(dec) => dec.still,
        }
    }

    pub fn next(&mut self) -> Result<(ff::frame::Video, f64)> {
        match self {
            AnyDecoder::Vk(dec) => dec.next_hw_frame(),
            AnyDecoder::Vaapi(dec) => dec.next(),
            AnyDecoder::Sw(dec) => dec.next(),
        }
    }

    pub fn next_render(&mut self) -> Result<(RenderFrame, f64)> {
        match self {
            AnyDecoder::Vk(dec) => {
                dec.next_hw_frame().map(|(frame, pts)| (RenderFrame::plain(frame), pts))
            }
            AnyDecoder::Vaapi(dec) => dec.next_render(),
            AnyDecoder::Sw(dec) => dec.next().map(|(frame, pts)| (RenderFrame::plain(frame), pts)),
        }
    }

    pub fn next_cpu(&mut self, out: &mut ff::frame::Video) -> Result<f64> {
        if let AnyDecoder::Vk(dec) = self {
            return dec.next_frame_cpu(out);
        }
        let (frame, pts) = self.next()?;
        *out = frame;
        Ok(pts)
    }

    pub fn install_abort(&mut self, flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        let ictx = match self {
            AnyDecoder::Vk(dec) => &mut dec.ictx,
            AnyDecoder::Vaapi(dec) => &mut dec.ictx,
            AnyDecoder::Sw(dec) => &mut dec.ictx,
        };
        unsafe {
            (*ictx.as_mut_ptr()).interrupt_callback = ff::ffi::AVIOInterruptCB {
                callback: Some(interrupt_cb),
                opaque: std::sync::Arc::as_ptr(&flag) as *mut std::ffi::c_void,
            };
        }
        match self {
            AnyDecoder::Vk(dec) => dec.abort = Some(flag),
            AnyDecoder::Vaapi(dec) => dec.abort = Some(flag),
            AnyDecoder::Sw(dec) => dec.abort = Some(flag),
        }
    }
}

fn source_requires_software(path: &str) -> bool {
    !paper_control::is_video_path(path)
}

pub fn sw_decode_forced() -> bool {
    std::env::var("SKWD_VK_DECODE").as_deref() == Ok("sw")
}

pub fn vaapi_decode_forced() -> bool {
    std::env::var("SKWD_VK_DECODE").as_deref() == Ok("vaapi")
}

pub fn decoder_requires_software(path: &str, force_software: bool) -> bool {
    force_software || sw_decode_forced() || source_requires_software(path)
}

fn open_vulkan(path: &str, hwdev: Option<*mut ff::ffi::AVBufferRef>) -> Result<VulkanDecoder> {
    #[cfg(feature = "shared-device")]
    if let Some(device) = hwdev {
        return VulkanDecoder::open_with(path, device);
    }
    #[cfg(not(feature = "shared-device"))]
    let _ = hwdev;
    VulkanDecoder::open(path)
}

pub fn open_vulkan_decoder(
    path: &str,
    hwdev: Option<*mut ff::ffi::AVBufferRef>,
    vulkan_decode: bool,
) -> Option<VulkanDecoder> {
    if !vulkan_decode {
        tracing::info!("skwd-wall-vk: Vulkan video decode queue unavailable, trying VAAPI");
        return None;
    }
    match open_vulkan(path, hwdev) {
        Ok(decoder) => Some(decoder),
        Err(error) => {
            tracing::info!("skwd-wall-vk: Vulkan decode rejected {path} ({error:#}), trying VAAPI");
            None
        }
    }
}

pub fn open_decoder(
    path: &str,
    hwdev: Option<*mut ff::ffi::AVBufferRef>,
    vulkan_decode: bool,
    render_node: Option<&std::path::Path>,
    force_software: bool,
) -> Result<AnyDecoder> {
    if decoder_requires_software(path, force_software) {
        return Ok(AnyDecoder::Sw(SwDecoder::open(path)?));
    }
    if vaapi_decode_forced() {
        tracing::info!("skwd-wall-vk: VAAPI decode forced, skipping Vulkan video");
    } else if let Some(decoder) = open_vulkan_decoder(path, hwdev, vulkan_decode) {
        return Ok(AnyDecoder::Vk(decoder));
    }
    match VaapiDecoder::open(path, render_node) {
        Ok(decoder) => Ok(AnyDecoder::Vaapi(decoder)),
        Err(error) => {
            tracing::info!(
                "skwd-wall-vk: VAAPI decode rejected {path} ({error:#}), software fallback"
            );
            Ok(AnyDecoder::Sw(SwDecoder::open(path)?))
        }
    }
}

pub struct PlaneDesc {
    pub fd: std::os::fd::RawFd,
    pub object_size: usize,
    pub modifier: u64,
    pub offset: usize,
    pub pitch: usize,
}

pub struct MappedFrame {
    _drm: ff::frame::Video,
    pub luma: PlaneDesc,
    pub chroma: PlaneDesc,
}

pub(crate) fn retain_frame(frame: &ff::frame::Video) -> Result<ff::frame::Video> {
    let retained = unsafe { ff::ffi::av_frame_clone(frame.as_ptr()) };
    if retained.is_null() {
        return Err(anyhow!("av_frame_clone failed"));
    }
    Ok(unsafe { ff::frame::Video::wrap(retained) })
}

impl MappedFrame {
    pub(crate) fn wait_ready(&self) -> Result<()> {
        wait_dma_buf(self.luma.fd)?;
        if self.chroma.fd != self.luma.fd {
            wait_dma_buf(self.chroma.fd)?;
        }
        Ok(())
    }
}

const FOURCC_NV12: u32 = u32::from_le_bytes(*b"NV12");
const FOURCC_R8: u32 = u32::from_le_bytes(*b"R8  ");
const FOURCC_RG88: u32 = u32::from_le_bytes(*b"RG88");
const FOURCC_GR88: u32 = u32::from_le_bytes(*b"GR88");

fn checked_descriptor_count(value: i32, capacity: usize, name: &str) -> Result<usize> {
    let count = usize::try_from(value).map_err(|_| anyhow!("negative DRM {name} count"))?;
    if count > capacity {
        return Err(anyhow!("DRM {name} count {count} exceeds capacity {capacity}"));
    }
    Ok(count)
}

fn wait_dma_buf(fd: std::os::fd::RawFd) -> Result<()> {
    let mut poll_fd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    loop {
        let result = unsafe { libc::poll(&mut poll_fd, 1, 5000) };
        if result > 0 && poll_fd.revents & libc::POLLIN != 0 {
            return Ok(());
        }
        if result == 0 {
            return Err(anyhow!("timed out waiting for hardware dma-buf"));
        }
        if result < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if result > 0 {
            return Err(anyhow!("dma-buf poll returned events {:#x}", poll_fd.revents));
        }
        return Err(anyhow!("dma-buf poll failed: {}", std::io::Error::last_os_error()));
    }
}

fn descriptor_planes(desc: &ff::ffi::AVDRMFrameDescriptor) -> Result<(PlaneDesc, PlaneDesc)> {
    let object_count = checked_descriptor_count(desc.nb_objects, desc.objects.len(), "object")?;
    let layer_count = checked_descriptor_count(desc.nb_layers, desc.layers.len(), "layer")?;
    let layer_indices = match &desc.layers[..layer_count] {
        [layer] if layer.format == FOURCC_NV12 && layer.nb_planes == 2 => vec![0],
        [luma, chroma]
            if luma.format == FOURCC_R8
                && matches!(chroma.format, FOURCC_RG88 | FOURCC_GR88)
                && luma.nb_planes == 1
                && chroma.nb_planes == 1 =>
        {
            vec![0, 1]
        }
        layers => {
            let layout = layers
                .iter()
                .map(|layer| format!("{:#010x}/{}", layer.format, layer.nb_planes))
                .collect::<Vec<_>>()
                .join(",");
            return Err(anyhow!("DRM frame is not NV12: {layout}"));
        }
    };
    let mut planes = Vec::with_capacity(2);
    for layer_index in layer_indices {
        let layer = &desc.layers[layer_index];
        let plane_count = checked_descriptor_count(layer.nb_planes, layer.planes.len(), "plane")?;
        for plane in &layer.planes[..plane_count] {
            let object_index = usize::try_from(plane.object_index)
                .map_err(|_| anyhow!("negative DRM object index"))?;
            let object =
                desc.objects.get(object_index).filter(|_| object_index < object_count).ok_or_else(
                    || anyhow!("DRM plane references object {object_index}/{object_count}"),
                )?;
            if object.fd < 0 || object.size == 0 || plane.pitch == 0 {
                return Err(anyhow!("DRM plane has an invalid object, size, or pitch"));
            }
            planes.push(PlaneDesc {
                fd: object.fd,
                object_size: object.size,
                modifier: object.format_modifier,
                offset: plane.offset as usize,
                pitch: plane.pitch as usize,
            });
        }
    }
    let [luma, chroma] = <[PlaneDesc; 2]>::try_from(planes)
        .map_err(|planes: Vec<PlaneDesc>| anyhow!("NV12 descriptor has {} planes", planes.len()))?;
    Ok((luma, chroma))
}

pub(crate) fn map_to_drm(src: &ff::frame::Video) -> Result<MappedFrame> {
    let mut drm = ff::frame::Video::empty();
    unsafe {
        (*drm.as_mut_ptr()).format = ff::ffi::AVPixelFormat::DRM_PRIME.0;
        let rc = ff::ffi::av_hwframe_map(
            drm.as_mut_ptr(),
            src.as_ptr(),
            ff::ffi::AV_HWFRAME_MAP_READ.0 as i32,
        );
        if rc < 0 {
            return Err(anyhow!("av_hwframe_map to DRM_PRIME failed ({rc})"));
        }
        let desc = (*drm.as_ptr()).data[0] as *const ff::ffi::AVDRMFrameDescriptor;
        if desc.is_null() {
            return Err(anyhow!("null AVDRMFrameDescriptor"));
        }
        let (luma, chroma) = descriptor_planes(&*desc)?;
        Ok(MappedFrame { _drm: drm, luma, chroma })
    }
}

pub(crate) fn transfer_hardware_nv12(frame: &ff::frame::Video) -> Result<ff::frame::Video> {
    let mut transferred = ff::frame::Video::empty();
    let rc =
        unsafe { ff::ffi::av_hwframe_transfer_data(transferred.as_mut_ptr(), frame.as_ptr(), 0) };
    if rc < 0 {
        return Err(anyhow!("hardware frame transfer failed ({rc})"));
    }
    if transferred.format() == ff::format::Pixel::NV12 {
        return Ok(transferred);
    }
    let mut output = ff::frame::Video::empty();
    let mut scaler = ff::software::scaling::Context::get(
        transferred.format(),
        transferred.width(),
        transferred.height(),
        ff::format::Pixel::NV12,
        transferred.width(),
        transferred.height(),
        ff::software::scaling::Flags::BILINEAR,
    )
    .context("hardware NV12 scaler")?;
    scaler.run(&transferred, &mut output).context("hardware NV12 convert")?;
    Ok(output)
}

mod tests;
