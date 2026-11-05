use super::{ALL_OUTPUT, Manager, freeze_parent, wait_snapshots};
use crate::backend::Worker;
use anyhow::{Context, Result, anyhow};
use paper_control::{Assignment, FillMode, PaperCommand};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Write;
use std::sync::Arc;

const DIRECTORY_PREFIX: &str = "handoff-";
const ERROR_EXTENSION: &str = "ppm.error";

pub(super) struct Handoff {
    pub(super) directory: Arc<tempfile::TempDir>,
    controls: Vec<(File, bool)>,
}

impl Handoff {
    pub(super) fn disarm(&mut self) {
        self.controls.clear();
    }
}

impl Drop for Handoff {
    fn drop(&mut self) {
        for (control, paused) in &mut self.controls {
            let _ = control.write_all(PaperCommand::pause(*paused).line().as_bytes());
        }
    }
}

impl Manager {
    #[expect(
        clippy::too_many_lines,
        reason = "one transactional handoff capture with rollback controls"
    )]
    pub(super) async fn capture_overlays(
        &mut self,
        assignments: &mut [(Assignment, String)],
    ) -> Result<Option<Handoff>> {
        let mut sources = BTreeMap::new();
        for (position, (assignment, _)) in assignments.iter().enumerate() {
            if assignment.transition.is_none() {
                continue;
            }
            let matching = self
                .overlays
                .iter()
                .enumerate()
                .filter(|(_, worker)| {
                    worker
                        .assignment
                        .transition
                        .as_ref()
                        .is_some_and(|transition| transition.drawing.is_some())
                        && overlaps(assignment, worker)
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if matching.len() > 1 {
                return Err(anyhow!("drawing handoff requires named outputs"));
            }
            if let Some(&index) = matching.first() {
                let outputs = &self.overlays[index].assignment.outputs;
                if outputs.len() != 1 || outputs[0] == ALL_OUTPUT || *outputs != assignment.outputs
                {
                    return Err(anyhow!("drawing handoff requires matching named outputs"));
                }
                sources.insert(position, (true, index));
                continue;
            }
            let drawing = assignment
                .transition
                .as_ref()
                .is_some_and(|transition| transition.drawing.is_some());
            let live = self
                .workers
                .iter()
                .enumerate()
                .filter(|(_, worker)| {
                    (worker.freeze_capable() || drawing) && overlaps(assignment, worker)
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if drawing && live.len() > 1 {
                return Err(anyhow!("drawing handoff requires matching named outputs"));
            }
            if !drawing && !frame_fits(assignment, &self.workers, &live) {
                continue;
            }
            if let Some(&index) = live.first() {
                sources.insert(position, (false, index));
            }
        }
        if sources.is_empty() {
            return Ok(None);
        }
        let parent = self.freeze_parent.clone().map_or_else(freeze_parent, Ok)?;
        let directory =
            Arc::new(tempfile::Builder::new().prefix(DIRECTORY_PREFIX).tempdir_in(parent)?);
        let mut handoff = Handoff { directory, controls: Vec::new() };
        let mut paths = BTreeMap::new();
        for (&position, &(overlay, index)) in &sources {
            let workers = if overlay { &mut self.overlays } else { &mut self.workers };
            let worker = &mut workers[index];
            let paused = self.paused
                || self.playback_pause.effective(worker)
                || worker.drawing_hold.is_some();
            if worker.dynamic() {
                handoff.controls.push((worker.control_file()?, paused));
            }
            let path = handoff.directory.path().join(format!("{}-{position}.ppm", worker.pid()));
            let output = assignments[position].0.outputs.first().map_or(ALL_OUTPUT, String::as_str);
            let drawing = assignments[position]
                .0
                .transition
                .as_ref()
                .is_some_and(|transition| transition.drawing.is_some());
            let leaves_span = worker.assignment.fill_mode == FillMode::Span
                && assignments[position].0.fill_mode != FillMode::Span;
            let shared_view = leaves_span
                || (!same_outputs(&assignments[position].0, worker)
                    && assignments[position].0.outputs != [ALL_OUTPUT]);
            let capture = if drawing || shared_view {
                worker.snapshot(&path, output, true).await
            } else {
                worker.send(&PaperCommand::freeze(&path.display().to_string())).await
            }
            .context("capture drawing handoff");
            let capture = match capture {
                Ok(()) => wait_snapshots(workers, &[index], std::slice::from_ref(&path)).await,
                Err(error) => Err(error),
            };
            if let Err(error) = capture {
                let error_path = path.with_extension(ERROR_EXTENSION);
                if !path.exists()
                    && !error_path.exists()
                    && workers[index].exited()?.is_some_and(|status| status.success())
                {
                    continue;
                }
                return Err(error);
            }
            paths.insert(position, path);
        }
        for (position, path) in &paths {
            assignments[*position].0.transition.as_mut().unwrap().from =
                Some(path.display().to_string());
        }
        if paths.is_empty() {
            return Ok(None);
        }
        Ok(Some(handoff))
    }
}

fn frame_fits(assignment: &Assignment, workers: &[Worker], live: &[usize]) -> bool {
    let Some(first) = live.first().map(|&index| &workers[index]) else { return false };
    if assignment.fill_mode == FillMode::Span {
        return live.len() == 1
            && first.assignment.fill_mode == FillMode::Span
            && same_outputs(assignment, first);
    }
    live.len() == 1
        || workers
            .iter()
            .filter(|worker| overlaps(assignment, worker))
            .all(|worker| worker.assignment.source.path == first.assignment.source.path)
}

fn same_outputs(assignment: &Assignment, worker: &Worker) -> bool {
    let requested = assignment.outputs.iter().collect::<BTreeSet<_>>();
    requested == worker.assignment.outputs.iter().collect()
}

fn overlaps(assignment: &Assignment, worker: &Worker) -> bool {
    assignment.outputs.iter().any(|output| output == ALL_OUTPUT)
        || worker
            .assignment
            .outputs
            .iter()
            .any(|output| output == ALL_OUTPUT || assignment.outputs.contains(output))
}
