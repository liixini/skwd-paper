#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, anyhow};
use libloading::{Library, Symbol};
use ringbuf::traits::Producer;

const PA_STREAM_RECORD: c_int = 2;
const PA_SAMPLE_FLOAT32LE: c_int = 5;
pub const RATE: u32 = 48_000;
pub const CHANNELS: u8 = 2;
const FRAGMENT_FRAMES: usize = 1024;

#[repr(C)]
struct SampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}

#[repr(C)]
struct BufferAttr {
    maxlength: u32,
    tlength: u32,
    prebuf: u32,
    minreq: u32,
    fragsize: u32,
}

type NewFn = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    c_int,
    *const c_char,
    *const c_char,
    *const SampleSpec,
    *const c_void,
    *const BufferAttr,
    *mut c_int,
) -> *mut c_void;
type ReadFn = unsafe extern "C" fn(*mut c_void, *mut c_void, usize, *mut c_int) -> c_int;
type FreeFn = unsafe extern "C" fn(*mut c_void);
type StrerrorFn = unsafe extern "C" fn(c_int) -> *const c_char;

struct Simple {
    library: Library,
    handle: *mut c_void,
}

unsafe impl Send for Simple {}

impl Simple {
    fn open(source: &str) -> Result<Self> {
        let library = unsafe { Library::new("libpulse-simple.so.0") }
            .map_err(|error| anyhow!("libpulse-simple unavailable: {error}"))?;
        let spec = SampleSpec { format: PA_SAMPLE_FLOAT32LE, rate: RATE, channels: CHANNELS };
        let frame_bytes = u32::from(CHANNELS) * 4;
        let attr = BufferAttr {
            maxlength: u32::MAX,
            tlength: u32::MAX,
            prebuf: u32::MAX,
            minreq: u32::MAX,
            fragsize: FRAGMENT_FRAMES as u32 * frame_bytes,
        };
        let name = CString::new("skwd-wall-vk")?;
        let stream = CString::new("scene audio spectrum")?;
        let device = CString::new(source)?;
        let mut error: c_int = 0;
        let handle = unsafe {
            let new: Symbol<NewFn> = library.get(b"pa_simple_new\0")?;
            new(
                std::ptr::null(),
                name.as_ptr(),
                PA_STREAM_RECORD,
                device.as_ptr(),
                stream.as_ptr(),
                &raw const spec,
                std::ptr::null(),
                &raw const attr,
                &raw mut error,
            )
        };
        if handle.is_null() {
            let message = unsafe {
                library
                    .get::<StrerrorFn>(b"pa_strerror\0")
                    .map(|strerror| CStr::from_ptr(strerror(error)).to_string_lossy().into_owned())
                    .unwrap_or_else(|_| format!("error {error}"))
            };
            return Err(anyhow!("pa_simple_new({source}): {message}"));
        }
        Ok(Self { library, handle })
    }

    fn read(&self, buffer: &mut [f32]) -> Result<()> {
        let mut error: c_int = 0;
        let status = unsafe {
            let read: Symbol<ReadFn> = self.library.get(b"pa_simple_read\0")?;
            read(
                self.handle,
                buffer.as_mut_ptr().cast::<c_void>(),
                std::mem::size_of_val(buffer),
                &raw mut error,
            )
        };
        if status < 0 {
            return Err(anyhow!("pa_simple_read failed with {error}"));
        }
        Ok(())
    }
}

impl Drop for Simple {
    fn drop(&mut self) {
        if let Ok(free) = unsafe { self.library.get::<FreeFn>(b"pa_simple_free\0") } {
            unsafe { free(self.handle) };
        }
    }
}

pub struct MonitorCapture {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl MonitorCapture {
    pub fn start(source: &str, mut producer: ringbuf::HeapProd<f32>) -> Result<Self> {
        let simple = Simple::open(source)?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = std::thread::Builder::new()
            .name("skwd-audio-monitor".into())
            .spawn(move || {
                let mut buffer = vec![0.0f32; FRAGMENT_FRAMES * usize::from(CHANNELS)];
                while !thread_stop.load(Ordering::Relaxed) {
                    if let Err(error) = simple.read(&mut buffer) {
                        tracing::warn!("skwd-wall-vk: audio monitor capture stopped: {error}");
                        return;
                    }
                    for sample in &buffer {
                        let _ = producer.try_push(*sample);
                    }
                }
            })
            .map_err(|error| anyhow!("spawn audio monitor thread: {error:?}"))?;
        Ok(Self { stop, thread: Some(thread) })
    }
}

impl Drop for MonitorCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
