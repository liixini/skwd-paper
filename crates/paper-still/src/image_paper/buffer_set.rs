use super::shm_pixels::choose_shm_format;
use anyhow::{Result, anyhow};
use smithay_client_toolkit::shm::{
    Shm,
    slot::{Buffer, Slot, SlotPool},
};
use wayland_client::protocol::{wl_shm::Format, wl_surface::WlSurface};

pub(super) struct BufferSet {
    pool: SlotPool,
    slot: Slot,
    buffers: Vec<Buffer>,
    width: u32,
    height: u32,
    stride: i32,
    format: Format,
    byte_len: usize,
}

impl BufferSet {
    pub(super) fn new(
        shm: &Shm,
        width: u32,
        height: u32,
        surface_count: usize,
        fill: impl FnOnce(&mut [u8], Format),
    ) -> Result<Self> {
        let stride = (width as i32) * 4;
        let byte_len = (stride as usize) * (height as usize);
        let pool_len = (byte_len + 63) & !63;
        let mut pool =
            SlotPool::new(pool_len, shm).map_err(|err| anyhow!("SlotPool::new: {err}"))?;
        let slot = pool.new_slot(byte_len).map_err(|err| anyhow!("new_slot: {err}"))?;
        let format = choose_shm_format(shm.formats());
        fill(&mut pool.raw_data_mut(&slot)[..byte_len], format);
        let mut set = Self {
            pool,
            slot,
            buffers: Vec::with_capacity(surface_count),
            width,
            height,
            stride,
            format,
            byte_len,
        };
        set.ensure_count(surface_count)?;
        Ok(set)
    }

    fn create_buffer(&mut self) -> Result<Buffer> {
        self.pool
            .create_buffer_in(
                &self.slot,
                self.width as i32,
                self.height as i32,
                self.stride,
                self.format,
            )
            .map_err(|err| anyhow!("create_buffer_in: {err}"))
    }

    fn ensure_count(&mut self, count: usize) -> Result<()> {
        while self.buffers.len() < count {
            let buffer = self.create_buffer()?;
            self.buffers.push(buffer);
        }
        Ok(())
    }

    pub(super) fn attach_to(&mut self, index: usize, surface: &WlSurface) -> Result<()> {
        self.ensure_count(index + 1)?;
        if self.buffers[index].attach_to(surface).is_ok() {
            return Ok(());
        }
        let buffer = self.create_buffer()?;
        buffer.attach_to(surface).map_err(|err| anyhow!("activate fresh buffer: {err}"))?;
        self.buffers[index] = buffer;
        Ok(())
    }

    pub(super) fn drop_local_pages(&mut self) {
        let data = self.pool.raw_data_mut(&self.slot);
        let (start, len) = (data.as_mut_ptr() as usize, data.len());
        if len == 0 {
            return;
        }
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as usize;
        let head = start.div_ceil(page) * page;
        let tail = (start + len) / page * page;
        if tail > head {
            unsafe {
                libc::madvise(head as *mut libc::c_void, tail - head, libc::MADV_DONTNEED);
            }
        }
    }

    pub(super) fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub(super) fn has_active_buffers(&self) -> bool {
        self.slot.has_active_buffers()
    }

    pub(super) fn canvas(&mut self) -> Option<&mut [u8]> {
        self.slot.canvas(&mut self.pool).map(|canvas| &mut canvas[..self.byte_len])
    }
}
