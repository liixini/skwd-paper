mod clip_view;
pub use clip_view::ClipView;
mod depth;
pub use depth::DepthOrder;
#[cfg(feature = "shared-device")]
mod depth_sample;
#[cfg(feature = "shared-device")]
pub use depth_sample::DepthSample;
mod draw;
mod effect;
mod export;
mod history;
mod images;
#[cfg(feature = "shared-device")]
mod nv12;
mod pipelines;
mod renderer;
#[cfg(feature = "shared-device")]
mod sampling;
mod scene;
#[cfg(feature = "shared-device")]
mod shadow;
#[cfg(feature = "shared-device")]
mod strokes;
#[cfg(feature = "shared-device")]
pub use sampling::Sampling;
#[cfg(feature = "shared-device")]
pub use shadow::{ShadowAlpha, ShadowAtlas, ShadowDraw, ShadowSample};
#[cfg(feature = "shared-device")]
pub use strokes::{DrawingStrokes, StrokeSpec};
mod swapchain;
#[cfg(feature = "shared-device")]
mod transition;
mod upload;
mod viewport;
pub use viewport::SceneViewport;

#[cfg(feature = "shared-device")]
pub use draw::Src;
#[cfg(feature = "shared-device")]
pub use effect::{
    DynBuffer, EffectCoordinates, EffectDepth, EffectDraw, EffectGeometry, EffectPipelineSpec,
    ParticleMesh, ParticlePipelineSpec, ScenePresent, d3d_clip_enabled,
};
pub use effect::{EffectPipeline, QuadBuffer};
#[cfg(feature = "shared-device")]
pub use export::{ExportImage, ExternalSemaphore, Nv12Export, ReadbackBuf, RenderTarget};
pub use export::{FrameImages, FrameViews};
#[cfg(feature = "shared-device")]
pub use history::SceneHistory;
#[cfg(feature = "shared-device")]
pub use nv12::Nv12Presenter;
pub use renderer::Renderer;
#[cfg(feature = "shared-device")]
pub use scene::clip::{ClipPlanes, ClipSource, PuppetClip};
#[cfg(feature = "shared-device")]
pub use scene::{
    EffectProjection, LayerProjection, MeshGeometry, MipSampling, ParticleDraw, SceneEffect,
    SceneIndices, SceneLayers, SceneMesh, SceneQuad, SceneTexture,
};
pub use scene::{PresentedTarget, SceneBlend, SceneConfig, SceneTarget, SceneTargetSpec};
pub use upload::{FrameStream, UploadPath};

pub fn device_lost(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<ash::vk::Result>() == Some(&ash::vk::Result::ERROR_DEVICE_LOST)
    })
}

#[cfg(test)]
mod device_lost_tests {
    use super::device_lost;
    use anyhow::Context;

    #[test]
    fn device_lost_in_chain() {
        let lost: anyhow::Result<()> = Err(ash::vk::Result::ERROR_DEVICE_LOST.into());
        assert!(device_lost(&lost.context("submit").unwrap_err()));
        assert!(!device_lost(&anyhow::anyhow!("decode failed")));
        assert!(!device_lost(&anyhow::Error::from(ash::vk::Result::ERROR_OUT_OF_DATE_KHR)));
    }
}
