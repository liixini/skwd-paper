mod model;
mod protocol;
mod target;

#[cfg(feature = "shared-device")]
pub use model::SurfaceUi;
pub use model::Target;
#[cfg(feature = "shared-device")]
pub(crate) use model::compositor_drm_device;
pub(crate) use target::is_kwin_session;
pub use target::{GroupBufferWait, ReexecSource, setup};
