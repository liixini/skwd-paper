use super::model::App;
use super::shm_pixels::choose_shm_format;
use anyhow::Result;
use paper_runtime::mirror::{Layout, Memory, Plane};
use paper_runtime::shm::SharedMemory;
use smithay_client_toolkit::shm::Shm;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    protocol::{
        wl_buffer::{self, WlBuffer},
        wl_shm::Format,
        wl_shm_pool::{self, WlShmPool},
        wl_surface::WlSurface,
    },
};

pub(super) struct BufferSet {
    memory: SharedMemory,
    pool: WlShmPool,
    qh: QueueHandle<App>,
    buffers: Vec<(WlBuffer, Arc<AtomicBool>)>,
    busy: Vec<(WlBuffer, Arc<AtomicBool>)>,
    width: u32,
    height: u32,
    stride: i32,
    format: Format,
}

impl BufferSet {
    pub(super) fn new(
        shm: &Shm,
        qh: &QueueHandle<App>,
        width: u32,
        height: u32,
        surface_count: usize,
        fill: impl FnOnce(&mut [u8], Format),
    ) -> Result<Self> {
        let stride = (width as i32) * 4;
        let byte_len = (stride as usize) * (height as usize);
        let memory = SharedMemory::new(byte_len)?;
        let format = choose_shm_format(shm.formats());
        let pool = shm.wl_shm().create_pool(memory.as_fd(), byte_len.max(1) as i32, qh, ());
        let mut set = Self {
            memory,
            pool,
            qh: qh.clone(),
            buffers: Vec::with_capacity(surface_count),
            busy: Vec::new(),
            width,
            height,
            stride,
            format,
        };
        fill(set.bytes(), format);
        set.ensure_count(surface_count);
        Ok(set)
    }

    fn bytes(&mut self) -> &mut [u8] {
        self.memory.bytes()
    }

    fn create_buffer(&self) -> (WlBuffer, Arc<AtomicBool>) {
        let active = Arc::new(AtomicBool::new(false));
        let buffer = self.pool.create_buffer(
            0,
            self.width as i32,
            self.height as i32,
            self.stride,
            self.format,
            &self.qh,
            active.clone(),
        );
        (buffer, active)
    }

    fn ensure_count(&mut self, count: usize) {
        while self.buffers.len() < count {
            let buffer = self.create_buffer();
            self.buffers.push(buffer);
        }
    }

    pub(super) fn attach_to(&mut self, index: usize, surface: &WlSurface) {
        self.ensure_count(index + 1);
        self.busy.retain(|(buffer, active)| {
            let keep = active.load(Ordering::Acquire);
            if !keep {
                buffer.destroy();
            }
            keep
        });
        if self.buffers[index].1.swap(true, Ordering::AcqRel) {
            let fresh = self.create_buffer();
            fresh.1.store(true, Ordering::Release);
            let old = std::mem::replace(&mut self.buffers[index], fresh);
            self.busy.push(old);
        }
        surface.attach(Some(&self.buffers[index].0), 0, 0);
    }

    pub(super) fn drop_local_pages(&mut self) {
        self.memory.drop_local_pages();
    }

    pub(super) fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub(super) fn has_active_buffers(&self) -> bool {
        self.buffers.iter().chain(&self.busy).any(|(_, active)| active.load(Ordering::Acquire))
    }

    pub(super) fn write(&mut self, fill: impl FnOnce(&mut [u8], Format)) -> Option<()> {
        let format = self.format;
        self.canvas().map(|bytes| fill(bytes, format))
    }

    pub(super) fn canvas(&mut self) -> Option<&mut [u8]> {
        (!self.has_active_buffers()).then(|| self.bytes())
    }

    pub(super) fn share(&self) -> std::io::Result<(OwnedFd, Layout)> {
        let fourcc = match self.format {
            Format::Argb8888 => paper_runtime::mirror::AR24,
            Format::Xrgb8888 => paper_runtime::mirror::XR24,
            other => u32::from(other),
        };
        Ok((
            self.memory.as_fd().try_clone_to_owned()?,
            Layout {
                width: self.width,
                height: self.height,
                fourcc,
                modifier: 0,
                memory: Memory::Shm,
                planes: [Plane { stride: self.stride as u32, offset: 0 }, Plane::default()],
                plane_count: 1,
            },
        ))
    }
}

impl Drop for BufferSet {
    fn drop(&mut self) {
        for (buffer, _) in self.buffers.drain(..).chain(self.busy.drain(..)) {
            buffer.destroy();
        }
        self.pool.destroy();
    }
}

impl Dispatch<WlBuffer, Arc<AtomicBool>> for App {
    fn event(
        _: &mut Self,
        _: &WlBuffer,
        event: wl_buffer::Event,
        active: &Arc<AtomicBool>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            active.store(false, Ordering::Release);
        }
    }
}

impl Dispatch<WlShmPool, ()> for App {
    fn event(
        _: &mut Self,
        _: &WlShmPool,
        _: wl_shm_pool::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
