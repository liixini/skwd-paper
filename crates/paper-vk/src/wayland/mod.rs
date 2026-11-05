mod mirror;
mod model;
mod pointer;
mod protocol;
mod target;

pub(crate) use mirror::request as request_mirror;
#[cfg(feature = "shared-device")]
pub use model::SurfaceUi;
pub use model::Target;
#[cfg(feature = "shared-device")]
pub(crate) use model::compositor_drm_device;
#[cfg(feature = "shared-device")]
pub(crate) use target::is_kwin_session;
pub use target::setup;
#[cfg(feature = "shared-device")]
pub use target::{GroupBufferWait, ReexecSource};
