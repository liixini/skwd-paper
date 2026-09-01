#[cfg(feature = "shared-device")]
use ash::vk;

#[cfg(feature = "shared-device")]
pub enum Src<'a> {
    Avvk(&'a ffmpeg_the_third::frame::Video),
    Imported(&'a crate::vk::FrameImages),
    Views(vk::ImageView, vk::ImageView),
    Rgba(vk::ImageView),
}

#[cfg(feature = "shared-device")]
impl Src<'_> {
    pub(in crate::vk) fn texture_uv(&self, uv: [f32; 4]) -> [f32; 4] {
        let Self::Avvk(frame) = self else {
            return uv;
        };
        unsafe {
            let raw = frame.as_ptr();
            let hw_ref = (*raw).hw_frames_ctx;
            if hw_ref.is_null() || (*hw_ref).data.is_null() {
                return uv;
            }
            let hw = (*hw_ref).data.cast::<ffmpeg_the_third::ffi::AVHWFramesContext>();
            let texture = ((*hw).width.max(1) as u32, (*hw).height.max(1) as u32);
            let crop = (
                (*raw).crop_left as u32,
                (*raw).crop_top as u32,
                (*raw).crop_right as u32,
                (*raw).crop_bottom as u32,
            );
            let visible = (
                ((*raw).width.max(1) as u32).saturating_sub(crop.0 + crop.2).max(1),
                ((*raw).height.max(1) as u32).saturating_sub(crop.1 + crop.3).max(1),
            );
            if texture != visible || crop.0 != 0 || crop.1 != 0 {
                static LOGGED_PADDED_FRAME: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !LOGGED_PADDED_FRAME.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    tracing::info!(
                        "skwd-wall-vk: decoder allocation {}x{} contains visible {}x{} at {},{}; correcting shader UV",
                        texture.0,
                        texture.1,
                        visible.0,
                        visible.1,
                        crop.0,
                        crop.1
                    );
                }
            }
            remap_texture_uv(uv, texture, visible, (crop.0, crop.1))
        }
    }
}

#[cfg(feature = "shared-device")]
fn remap_texture_uv(
    uv: [f32; 4],
    texture: (u32, u32),
    visible: (u32, u32),
    origin: (u32, u32),
) -> [f32; 4] {
    let x_scale = visible.0 as f32 / texture.0.max(1) as f32;
    let y_scale = visible.1 as f32 / texture.1.max(1) as f32;
    [
        uv[0] * x_scale,
        uv[1] * y_scale,
        (origin.0 as f32 + uv[2] * visible.0 as f32) / texture.0.max(1) as f32,
        (origin.1 as f32 + uv[3] * visible.1 as f32) / texture.1.max(1) as f32,
    ]
}

#[cfg(all(test, feature = "shared-device"))]
mod tests;

pub(in crate::vk) struct QueueGuard {
    #[cfg(feature = "shared-device")]
    pub(super) family: u32,
}

impl Drop for QueueGuard {
    fn drop(&mut self) {
        #[cfg(feature = "shared-device")]
        crate::shared::unlock_queue_family(self.family);
    }
}
