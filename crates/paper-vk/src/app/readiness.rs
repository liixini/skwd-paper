use std::sync::atomic::{AtomicBool, Ordering};

static READY_SENT: AtomicBool = AtomicBool::new(false);

pub(super) fn signal_ready() {
    crate::memory::schedule_cold_start_reclaim();
    READY_SENT.store(true, Ordering::Release);
    if let Err(err) = paper_control::signal_paper_ready() {
        tracing::info!("skwd-wall-vk: paper.ready send failed: {err}");
    }
}

pub(crate) fn signal_startup_failure(message: &str) {
    if READY_SENT.load(Ordering::Acquire) {
        return;
    }
    if let Err(error) = paper_control::signal_paper_failed("renderer_startup", message) {
        tracing::info!("skwd-wall-vk: paper.failed send failed: {error}");
    }
}

pub(super) struct PresentationReadiness {
    pending: bool,
}

impl PresentationReadiness {
    pub(super) const fn startup() -> Self {
        Self { pending: true }
    }

    pub(super) fn arm_swap(&mut self) {
        self.pending = true;
    }

    #[must_use]
    pub(super) fn take_if_committed(&mut self, committed: bool) -> bool {
        committed && std::mem::take(&mut self.pending)
    }

    pub(super) fn complete_after_presentation<E>(
        &mut self,
        committed: bool,
        wait_for_presentation: impl FnOnce() -> Result<(), E>,
        notify: impl FnOnce(),
    ) -> Result<bool, E> {
        if !self.take_if_committed(committed) {
            return Ok(false);
        }
        wait_for_presentation()?;
        notify();
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
