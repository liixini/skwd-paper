use super::{Group, Presenter, TransitionStyle, vk};
use anyhow::Result;

pub(super) struct StreamFrame {
    pub(super) frozen: Option<vk::SceneTarget>,
    pub(super) progress: f32,
}

pub(super) struct FrameStatus {
    pub(super) emitted: bool,
    pub(super) paused: bool,
    pub(super) available: bool,
}

impl Default for StreamFrame {
    fn default() -> Self {
        Self { frozen: None, progress: 1.0 }
    }
}

impl StreamFrame {
    pub(super) fn hold(
        &mut self,
        status: FrameStatus,
        presenter: &mut Presenter,
        group: &Group,
        style: TransitionStyle,
    ) -> Result<()> {
        if !status.emitted {
            self.frozen = None;
        } else if self.frozen.is_none() && (status.paused || !status.available) {
            self.frozen = Some(presenter.freeze(group, self.progress, style)?);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "stream_frame_tests.rs"]
mod tests;
