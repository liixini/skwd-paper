use std::collections::BTreeMap;

use anyhow::{Result, bail};
use paper_control::{PaperCommand, PauseRequest};

use crate::backend::Worker;

use super::{ALL_OUTPUT, Manager};

#[derive(Clone, Default)]
pub(crate) struct Control {
    pub(super) all: bool,
    outputs: BTreeMap<String, bool>,
}

impl Control {
    pub(crate) fn set(&mut self, outputs: &[String], value: bool) {
        if outputs.is_empty() || outputs == [ALL_OUTPUT] {
            self.all = value;
            self.outputs.clear();
        } else {
            self.outputs.extend(outputs.iter().map(|output| (output.clone(), value)));
        }
    }

    pub(super) fn validate(&self, outputs: &[String]) -> Result<()> {
        let values: std::collections::BTreeSet<_> = outputs
            .iter()
            .map(|output| self.outputs.get(output).copied().unwrap_or(self.all))
            .collect();
        if values.len() > 1
            || (outputs == [ALL_OUTPUT] && self.outputs.values().any(|value| *value != self.all))
        {
            bail!("playback controls require independent output assignments");
        }
        Ok(())
    }

    pub(super) fn effective(&self, worker: &Worker) -> bool {
        self.outputs(&worker.assignment.outputs)
    }

    pub(crate) fn outputs(&self, outputs: &[String]) -> bool {
        outputs.iter().all(|output| self.outputs.get(output).copied().unwrap_or(self.all))
    }
}

pub(super) fn selected(outputs: &[String], worker: &Worker) -> bool {
    outputs.is_empty()
        || outputs == [ALL_OUTPUT]
        || worker.assignment.outputs.iter().any(|output| outputs.contains(output))
}

pub(super) fn validate_scope<'a>(
    outputs: &[String],
    workers: impl Iterator<Item = &'a Worker>,
) -> Result<()> {
    if outputs.is_empty() || outputs == [ALL_OUTPUT] {
        return Ok(());
    }
    for worker in workers {
        if worker.output == ALL_OUTPUT
            || (selected(outputs, worker)
                && !worker.assignment.outputs.iter().all(|output| outputs.contains(output)))
        {
            bail!("targeted playback requires independent output assignments");
        }
    }
    Ok(())
}

impl Manager {
    pub(super) async fn restore_audio(&mut self, outputs: &[String]) {
        for worker in &mut self.workers {
            if worker.dynamic() && worker.drawing_hold.is_none() && selected(outputs, worker) {
                let request = paper_control::AudioSetRequest {
                    mute: Some(worker.assignment.mute),
                    volume: Some(worker.assignment.volume),
                    duck: Some(self.duck.effective(worker)),
                    outputs: Vec::new(),
                    system_audio: Some(worker.assignment.system_audio.unwrap_or(true)),
                };
                let _ = worker.send_audio(&request).await;
            }
        }
        for worker in &mut self.overlays {
            if selected(outputs, worker) {
                let _ = worker.send(&PaperCommand::duck(self.duck.effective(worker))).await;
            }
        }
    }

    pub(crate) async fn playback_pause(&mut self, request: &PauseRequest) -> Result<()> {
        request.validate()?;
        self.refresh()?;
        validate_scope(&request.outputs, self.workers.iter().chain(&self.overlays))?;
        let mut next = self.playback_pause.clone();
        next.set(&request.outputs, request.paused);
        next.outputs.extend(request.overrides.clone());
        for worker in self.workers.iter().chain(&self.overlays) {
            next.validate(&worker.assignment.outputs)?;
        }
        let mut workers: Vec<_> = self
            .workers
            .iter_mut()
            .chain(&mut self.overlays)
            .filter(|worker| {
                worker.dynamic()
                    && worker.drawing_hold.is_none()
                    && selected(&request.outputs, worker)
            })
            .collect();
        for index in 0..workers.len() {
            let paused = self.paused || next.effective(workers[index]);
            if let Err(error) = workers[index].send(&PaperCommand::pause(paused)).await {
                for worker in &mut workers[..index] {
                    let previous = self.paused || self.playback_pause.effective(worker);
                    let _ = worker.send(&PaperCommand::pause(previous)).await;
                }
                return Err(error);
            }
        }
        self.playback_pause = next;
        Ok(())
    }
}
