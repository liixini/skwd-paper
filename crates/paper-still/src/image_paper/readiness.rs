use anyhow::{Result, bail};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct StartupReadiness {
    outputs: Option<HashMap<u32, bool>>,
    sync_pending: bool,
    signaled: bool,
}

pub(super) struct StartupSync;

impl StartupReadiness {
    pub(super) fn finish_enumeration(&mut self, outputs: impl Iterator<Item = (u32, bool)>) {
        self.outputs = Some(outputs.collect());
    }

    pub(super) fn committed(&mut self, output: u32) {
        if let Some(attached) = self.outputs.as_mut().and_then(|outputs| outputs.get_mut(&output)) {
            *attached = true;
        }
    }

    pub(super) fn removed(&mut self, output: u32) {
        if let Some(outputs) = self.outputs.as_mut() {
            outputs.remove(&output);
        }
    }

    pub(super) fn begin_sync(&mut self) -> Result<bool> {
        if self.signaled {
            return Ok(false);
        }
        let Some(outputs) = &self.outputs else {
            return Ok(false);
        };
        if outputs.is_empty() {
            bail!("no initial wallpaper outputs remain before readiness");
        }
        if self.sync_pending || !outputs.values().all(|attached| *attached) {
            return Ok(false);
        }
        self.sync_pending = true;
        Ok(true)
    }

    pub(super) fn complete_sync(&mut self) -> bool {
        if !std::mem::take(&mut self.sync_pending) || self.signaled {
            return false;
        }
        let ready = self.outputs.as_ref().is_some_and(|outputs| {
            !outputs.is_empty() && outputs.values().all(|attached| *attached)
        });
        self.signaled = ready;
        ready
    }
}

#[cfg(test)]
#[path = "readiness_tests.rs"]
mod tests;
