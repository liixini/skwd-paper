use ash::vk;
use ash::vk::Handle;
use ffmpeg_the_third as ff;
use std::ffi::{c_char, c_void};
use std::marker::PhantomData;

unsafe extern "C" {
    fn skwd_av_vk_device_configure(
        context: *mut c_void,
        get_proc_addr: usize,
        instance: u64,
        physical_device: u64,
        device: u64,
        features: *const c_void,
        instance_extensions: *const *const c_char,
        instance_extension_count: i32,
        device_extensions: *const *const c_char,
        device_extension_count: i32,
        lock_queue: usize,
        unlock_queue: usize,
    ) -> i32;
    fn skwd_av_vk_device_add_queue(
        context: *mut c_void,
        index: i32,
        flags: u32,
        video_caps: u32,
    ) -> i32;
    fn skwd_av_vk_device_set_legacy_queues(
        context: *mut c_void,
        graphics: i32,
        transfer: i32,
        compute: i32,
        encode: i32,
        decode: i32,
    );
    fn skwd_av_vk_frame_image_count(frame: *const c_void) -> u32;
    fn skwd_av_vk_frame_image(frame: *const c_void, index: u32) -> u64;
    fn skwd_av_vk_frame_layout(frame: *const c_void, index: u32) -> i32;
    fn skwd_av_vk_frame_semaphore(frame: *const c_void, index: u32) -> u64;
    fn skwd_av_vk_frame_semaphore_value(frame: *const c_void, index: u32) -> u64;
    fn skwd_av_vk_frame_format(video: *const ff::ffi::AVFrame, index: u32) -> i32;
    fn skwd_av_vk_frame_lock(video: *const ff::ffi::AVFrame) -> i32;
    fn skwd_av_vk_frame_unlock(video: *const ff::ffi::AVFrame);
    fn skwd_av_vk_frame_set_state_and_semaphore(
        frame: *mut c_void,
        index: u32,
        layout: i32,
        access: u64,
        semaphore_value: u64,
    );
}

pub(crate) struct DeviceQueue {
    pub(crate) index: u32,
    pub(crate) flags: vk::QueueFlags,
    pub(crate) video_caps: vk::VideoCodecOperationFlagsKHR,
}

pub(crate) struct LegacyQueues {
    pub(crate) graphics: i32,
    pub(crate) transfer: i32,
    pub(crate) compute: i32,
    pub(crate) encode: i32,
    pub(crate) decode: i32,
}

fn queue_sync_from_status(status: i32) -> Option<bool> {
    match status {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

pub(crate) unsafe fn configure_device(
    context: *mut c_void,
    get_proc_addr: usize,
    instance: vk::Instance,
    physical_device: vk::PhysicalDevice,
    device: vk::Device,
    features: &vk::PhysicalDeviceFeatures2<'_>,
    instance_extensions: &[*const c_char],
    device_extensions: &[*const c_char],
    queues: &[DeviceQueue],
    legacy: LegacyQueues,
    lock_queue: usize,
    unlock_queue: usize,
) -> Option<bool> {
    let status = unsafe {
        skwd_av_vk_device_configure(
            context,
            get_proc_addr,
            instance.as_raw(),
            physical_device.as_raw(),
            device.as_raw(),
            std::ptr::from_ref(features).cast(),
            instance_extensions.as_ptr(),
            instance_extensions.len() as i32,
            device_extensions.as_ptr(),
            device_extensions.len() as i32,
            lock_queue,
            unlock_queue,
        )
    };
    let queue_sync = queue_sync_from_status(status)?;
    unsafe {
        skwd_av_vk_device_set_legacy_queues(
            context,
            legacy.graphics,
            legacy.transfer,
            legacy.compute,
            legacy.encode,
            legacy.decode,
        );
    }
    queues
        .iter()
        .all(|queue| unsafe {
            skwd_av_vk_device_add_queue(
                context,
                queue.index as i32,
                queue.flags.as_raw(),
                queue.video_caps.as_raw(),
            ) != 0
        })
        .then_some(queue_sync)
}

pub(crate) struct Frame<'a> {
    pointer: *mut c_void,
    video_pointer: *const ff::ffi::AVFrame,
    video: PhantomData<&'a ff::frame::Video>,
}

pub(crate) struct LockedFrame<'a> {
    frame: Frame<'a>,
    held: bool,
}

impl<'a> Frame<'a> {
    pub(crate) fn from_video(video: &'a ff::frame::Video) -> Option<Self> {
        let video_pointer = unsafe { video.as_ptr() };
        let pointer: *mut c_void = unsafe { (*video_pointer).data[0].cast() };
        (!pointer.is_null()).then_some(Self { pointer, video_pointer, video: PhantomData })
    }

    pub(crate) fn lock(self) -> LockedFrame<'a> {
        let held = unsafe { skwd_av_vk_frame_lock(self.video_pointer) } != 0;
        LockedFrame { frame: self, held }
    }

    pub(crate) fn image_count(&self) -> usize {
        unsafe { skwd_av_vk_frame_image_count(self.pointer) as usize }
    }

    pub(crate) fn image(&self, index: usize) -> vk::Image {
        vk::Image::from_raw(unsafe { skwd_av_vk_frame_image(self.pointer, index as u32) })
    }

    pub(crate) fn semaphore(&self, index: usize) -> vk::Semaphore {
        vk::Semaphore::from_raw(unsafe { skwd_av_vk_frame_semaphore(self.pointer, index as u32) })
    }
}

impl LockedFrame<'_> {
    pub(crate) fn held(&self) -> bool {
        self.held
    }

    pub(crate) fn image_count(&self) -> usize {
        self.frame.image_count()
    }

    pub(crate) fn image(&self, index: usize) -> vk::Image {
        self.frame.image(index)
    }

    pub(crate) fn layout(&self, index: usize) -> vk::ImageLayout {
        vk::ImageLayout::from_raw(unsafe {
            skwd_av_vk_frame_layout(self.frame.pointer, index as u32)
        })
    }

    pub(crate) fn semaphore(&self, index: usize) -> vk::Semaphore {
        self.frame.semaphore(index)
    }

    pub(crate) fn semaphore_value(&self, index: usize) -> u64 {
        unsafe { skwd_av_vk_frame_semaphore_value(self.frame.pointer, index as u32) }
    }

    pub(crate) fn format(&self, index: usize) -> vk::Format {
        vk::Format::from_raw(unsafe {
            skwd_av_vk_frame_format(self.frame.video_pointer, index as u32)
        })
    }

    pub(crate) fn commit(
        self,
        index: usize,
        layout: vk::ImageLayout,
        access: vk::AccessFlags,
        semaphore_value: u64,
    ) {
        unsafe {
            skwd_av_vk_frame_set_state_and_semaphore(
                self.frame.pointer,
                index as u32,
                layout.as_raw(),
                u64::from(access.as_raw()),
                semaphore_value,
            );
        }
    }
}

impl Drop for LockedFrame<'_> {
    fn drop(&mut self) {
        if self.held {
            unsafe { skwd_av_vk_frame_unlock(self.frame.video_pointer) };
        }
    }
}

#[cfg(test)]
mod tests;
