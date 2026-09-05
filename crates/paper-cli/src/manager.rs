use crate::backend::{BackendPaths, Worker, transition_source};
use anyhow::{Context, Result, anyhow};
use paper_control::{
    ApplyRequest, Assignment, AudioSetRequest, PaperCommand, RendererFailed, RendererPolicy,
    RendererReady, SandScope, StopRequest, VideoEngine,
};
use std::collections::BTreeSet;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

async fn any_worker_exit(workers: &mut [Worker]) {
    if workers.is_empty() {
        std::future::pending::<()>().await;
    }
    let mut exits: Vec<_> =
        workers.iter_mut().map(|worker| Box::pin(worker.child.wait())).collect();
    std::future::poll_fn(|context| {
        for exit in &mut exits {
            if exit.as_mut().poll(context).is_ready() {
                return std::task::Poll::Ready(());
            }
        }
        std::task::Poll::Pending
    })
    .await;
}

pub(crate) struct WorkerStatus {
    pub(crate) assignment: Assignment,
    pub(crate) pid: u32,
    pub(crate) generation: u64,
}

pub(crate) struct ApplyTransaction {
    candidates: Vec<Worker>,
    overlays: Vec<Worker>,
    next: Option<ApplyStage>,
    ready: BTreeSet<u32>,
    touched: BTreeSet<String>,
    replace_all: bool,
    generation: u64,
    policy: Option<RendererPolicy>,
    reported_failure: Option<String>,
}

struct ApplyStage {
    assignments: Vec<(Assignment, String)>,
    backends: BackendPaths,
    socket: PathBuf,
}

impl ApplyTransaction {
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn note_ready(&mut self, ready: &RendererReady) -> bool {
        if ready.generation != self.generation {
            return false;
        }
        if self.candidates.iter().any(|worker| worker.pid() == ready.pid) {
            self.ready.insert(ready.pid);
            true
        } else {
            false
        }
    }

    pub(crate) fn all_ready(&self) -> bool {
        self.ready.len() == self.candidates.len()
    }

    pub(crate) fn prepare_next(&mut self) -> Result<bool> {
        let Some(stage) = self.next.take() else { return Ok(false) };
        self.overlays = std::mem::take(&mut self.candidates);
        self.ready.clear();
        for (assignment, output) in stage.assignments {
            self.candidates.push(stage.backends.spawn(
                assignment,
                output,
                &stage.socket,
                self.generation,
                self.policy.as_ref(),
            )?);
        }
        Ok(true)
    }

    pub(crate) fn note_failed(&mut self, failed: &RendererFailed) -> bool {
        if failed.generation != self.generation
            || !self
                .candidates
                .iter()
                .chain(&self.overlays)
                .any(|worker| worker.pid() == failed.pid)
        {
            return false;
        }
        self.reported_failure = Some(format!("{}: {}", failed.code, failed.message));
        true
    }

    pub(crate) fn failed(&mut self) -> Result<Option<String>> {
        if self.reported_failure.is_some() {
            return Ok(self.reported_failure.take());
        }
        for worker in self.candidates.iter_mut().chain(&mut self.overlays) {
            if let Some(status) = worker.exited()? {
                return Ok(Some(format!(
                    "Paper worker {} for {} exited before readiness: {status}",
                    worker.pid(),
                    worker.output
                )));
            }
        }
        Ok(None)
    }

    pub(crate) async fn rollback(self) {
        for worker in self.candidates.into_iter().chain(self.overlays) {
            worker.stop().await;
        }
    }

    pub(crate) async fn candidate_exit(&mut self) {
        tokio::select! {
            () = any_worker_exit(&mut self.candidates) => {},
            () = any_worker_exit(&mut self.overlays) => {},
        }
    }

    #[cfg(test)]
    fn candidate_readiness(&self) -> Vec<RendererReady> {
        self.candidates
            .iter()
            .map(|worker| RendererReady { pid: worker.pid(), generation: self.generation })
            .collect()
    }
}

pub(crate) struct Manager {
    backends: Option<BackendPaths>,
    workers: Vec<Worker>,
    overlays: Vec<Worker>,
    next_generation: u64,
    paused: bool,
    policy: Option<RendererPolicy>,
    freeze_parent: Option<PathBuf>,
    #[cfg(test)]
    refresh_sweeps: Arc<std::sync::atomic::AtomicUsize>,
}

impl Manager {
    pub(crate) fn new() -> Self {
        Self {
            backends: None,
            workers: Vec::new(),
            overlays: Vec::new(),
            next_generation: 1,
            paused: false,
            policy: None,
            freeze_parent: None,
            #[cfg(test)]
            refresh_sweeps: Arc::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_backends(backends: BackendPaths, freeze_parent: PathBuf) -> Self {
        Self {
            backends: Some(backends),
            workers: Vec::new(),
            overlays: Vec::new(),
            next_generation: 1,
            paused: false,
            policy: None,
            freeze_parent: Some(freeze_parent),
            refresh_sweeps: Arc::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn refresh_sweeps(&self) -> Arc<std::sync::atomic::AtomicUsize> {
        Arc::clone(&self.refresh_sweeps)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.workers.is_empty() && self.overlays.is_empty()
    }

    pub(crate) fn paused(&self) -> bool {
        self.paused
    }

    pub(crate) fn policy(&self) -> Option<RendererPolicy> {
        self.policy.clone()
    }

    pub(crate) fn refresh(&mut self) -> Result<()> {
        let mut overlays = Vec::new();
        for mut worker in self.overlays.drain(..) {
            if worker.exited()?.is_none() {
                overlays.push(worker);
            }
        }
        self.overlays = overlays;
        #[cfg(test)]
        self.refresh_sweeps.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut live = Vec::with_capacity(self.workers.len());
        for mut worker in self.workers.drain(..) {
            match worker.exited()? {
                None => live.push(worker),
                Some(status) => {
                    tracing::warn!(
                        output = %worker.output,
                        pid = worker.pid(),
                        %status,
                        "paper worker exited, assignment dropped"
                    );
                }
            }
        }
        self.workers = live;
        Ok(())
    }

    pub(crate) async fn worker_exit(&mut self) {
        tokio::select! {
            () = any_worker_exit(&mut self.workers) => {},
            () = any_worker_exit(&mut self.overlays) => {},
        }
    }

    pub(crate) async fn begin_apply(
        &mut self,
        request: &ApplyRequest,
        socket: &Path,
    ) -> Result<ApplyTransaction> {
        request.validate().map_err(|error| anyhow!(error.to_string()))?;
        self.refresh()?;
        if !self.workers.is_empty()
            && !request.replace_all
            && request.policy.as_ref().is_some_and(|policy| Some(policy) != self.policy.as_ref())
        {
            return Err(anyhow!("changing renderer policy requires replace_all"));
        }
        if self.paused {
            return Err(anyhow!("resume Paper before applying a new composition"));
        }
        let policy = request.policy.clone().or_else(|| self.policy.clone());
        if policy.as_ref().and_then(|policy| policy.transitions_enabled) == Some(false)
            && request.assignments.iter().any(|assignment| assignment.transition.is_some())
        {
            return Err(anyhow!("assignment transitions conflict with the active renderer policy"));
        }
        let transition_primary =
            primary_sand_transition_output(policy.as_ref(), &request.assignments);
        let mut expanded = expand(&request.assignments, transition_primary.as_deref());
        for (assignment, output) in &mut expanded {
            resolve_transition_from(assignment, output, &self.workers)?;
        }
        let touched: BTreeSet<String> = request
            .assignments
            .iter()
            .flat_map(|assignment| assignment.outputs.iter().cloned())
            .collect();
        if !request.replace_all
            && self.workers.iter().any(|worker| worker.assignment.outputs == ["*"])
            && !touched.contains("*")
        {
            return Err(anyhow!("an active wildcard assignment requires replace_all"));
        }
        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        let backends = self.backend_paths();
        let overlays = expanded
            .iter()
            .filter(|(assignment, _)| {
                assignment.source.kind == paper_control::SourceKind::Static
                    && assignment
                        .transition
                        .as_ref()
                        .and_then(|transition| transition.from.as_ref())
                        .is_some_and(|from| from != &assignment.source.path)
            })
            .cloned()
            .collect::<Vec<_>>();
        let next = (!overlays.is_empty()).then(|| ApplyStage {
            assignments: expanded.clone(),
            backends: backends.clone(),
            socket: socket.to_path_buf(),
        });
        let staged = next.is_some();
        let mut candidates = Vec::with_capacity(expanded.len());
        for (assignment, output) in if staged { overlays } else { expanded } {
            let spawned = if staged {
                backends.spawn_overlay(assignment, output, socket, generation, policy.as_ref())
            } else {
                backends.spawn(assignment, output, socket, generation, policy.as_ref())
            };
            match spawned {
                Ok(worker) => candidates.push(worker),
                Err(error) => {
                    for worker in candidates {
                        worker.stop().await;
                    }
                    return Err(error);
                }
            }
        }
        Ok(ApplyTransaction {
            candidates,
            overlays: Vec::new(),
            next,
            ready: BTreeSet::new(),
            touched,
            replace_all: request.replace_all,
            generation,
            policy,
            reported_failure: None,
        })
    }

    pub(crate) async fn commit(
        &mut self,
        mut transaction: ApplyTransaction,
    ) -> Result<Vec<WorkerStatus>> {
        if !transaction.all_ready() || transaction.next.is_some() {
            transaction.rollback().await;
            return Err(anyhow!("Paper composition has not completed presentation readiness"));
        }
        if self.paused {
            for candidate in &mut transaction.candidates {
                if candidate.dynamic()
                    && let Err(error) = candidate.send(&PaperCommand::pause(true)).await
                {
                    transaction.rollback().await;
                    return Err(error.context("pause ready Paper candidate"));
                }
            }
        }
        for candidate in &mut transaction.candidates {
            candidate.assignment.transition = None;
        }
        for overlay in &mut transaction.overlays {
            if let Err(error) = overlay.send(&PaperCommand::pause(false)).await {
                transaction.rollback().await;
                return Err(error.context("release Paper static transition"));
            }
        }
        if let Err(error) = self.retain_untouched(&transaction).await {
            transaction.rollback().await;
            return Err(error);
        }
        let mut overlays = Vec::new();
        for overlay in self.overlays.drain(..) {
            if transaction.replace_all
                || transaction.touched.contains("*")
                || overlay.output == "*"
                || transaction.touched.contains(&overlay.output)
            {
                overlay.stop().await;
            } else {
                overlays.push(overlay);
            }
        }
        overlays.extend(transaction.overlays);
        self.overlays = overlays;
        let mut retained = Vec::new();
        let mut retired = Vec::new();
        let touches_all = transaction.touched.contains("*");
        for worker in self.workers.drain(..) {
            if transaction.replace_all
                || touches_all
                || worker.assignment.outputs == ["*"]
                || worker
                    .assignment
                    .outputs
                    .iter()
                    .all(|output| transaction.touched.contains(output))
            {
                retired.push(worker);
            } else {
                retained.push(worker);
            }
        }
        retained.extend(transaction.candidates);
        self.workers = retained;
        self.policy = transaction.policy;
        for worker in retired {
            worker.stop().await;
        }
        Ok(self.status())
    }

    pub(crate) async fn begin_pause(&mut self, socket: &Path) -> Result<Option<ApplyTransaction>> {
        self.refresh()?;
        if self.paused {
            return Ok(None);
        }
        let freeze_indices = self
            .workers
            .iter()
            .enumerate()
            .filter_map(|(index, worker)| worker.freeze_capable().then_some(index))
            .collect::<Vec<_>>();
        if freeze_indices.is_empty() {
            self.set_dynamic_pause(true).await?;
            self.paused = true;
            return Ok(None);
        }
        let parent = self.freeze_parent.clone().map_or_else(freeze_parent, Ok)?;
        let directory = Arc::new(tempfile::Builder::new().prefix("freeze-").tempdir_in(parent)?);
        let snapshots = freeze_indices
            .iter()
            .map(|index| {
                directory.path().join(format!("{}-{}.ppm", self.workers[*index].pid(), index))
            })
            .collect::<Vec<_>>();
        let mut changed: Vec<usize> = Vec::new();
        for index in 0..self.workers.len() {
            if !self.workers[index].dynamic() {
                continue;
            }
            let command =
                freeze_indices.iter().position(|candidate| *candidate == index).map_or_else(
                    || PaperCommand::pause(true),
                    |position| PaperCommand::freeze(&snapshots[position].display().to_string()),
                );
            if let Err(error) = self.workers[index].send(&command).await {
                for changed_index in changed {
                    let _ = self.workers[changed_index].send(&PaperCommand::pause(false)).await;
                }
                return Err(error.context("prepare Paper freeze frame"));
            }
            changed.push(index);
        }
        if let Err(error) = wait_snapshots(&mut self.workers, &freeze_indices, &snapshots).await {
            for index in changed {
                let _ = self.workers[index].send(&PaperCommand::pause(false)).await;
            }
            return Err(error);
        }
        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        let backends = self.backend_paths();
        let mut candidates = Vec::with_capacity(freeze_indices.len());
        for (index, snapshot) in freeze_indices.iter().zip(&snapshots) {
            let worker = &self.workers[*index];
            match backends.spawn_frozen(
                worker.assignment.clone(),
                snapshot,
                worker.output.clone(),
                socket,
                generation,
                self.policy.as_ref(),
                Arc::clone(&directory),
            ) {
                Ok(candidate) => candidates.push(candidate),
                Err(error) => {
                    for candidate in candidates {
                        candidate.stop().await;
                    }
                    self.abort_pause().await;
                    return Err(error);
                }
            }
        }
        let touched = worker_outputs(&candidates);
        Ok(Some(ApplyTransaction {
            candidates,
            overlays: Vec::new(),
            next: None,
            ready: BTreeSet::new(),
            touched,
            replace_all: false,
            generation,
            policy: self.policy.clone(),
            reported_failure: None,
        }))
    }

    pub(crate) async fn commit_pause(
        &mut self,
        transaction: ApplyTransaction,
    ) -> Result<Vec<WorkerStatus>> {
        self.paused = true;
        match self.commit(transaction).await {
            Ok(status) => Ok(status),
            Err(error) => {
                self.paused = false;
                self.abort_pause().await;
                Err(error)
            }
        }
    }

    pub(crate) async fn abort_pause(&mut self) {
        for worker in &mut self.workers {
            if worker.dynamic() {
                let _ = worker.send(&PaperCommand::pause(false)).await;
            }
        }
    }

    pub(crate) async fn begin_resume(&mut self, socket: &Path) -> Result<Option<ApplyTransaction>> {
        self.refresh()?;
        if !self.paused {
            return Ok(None);
        }
        let frozen = self
            .workers
            .iter()
            .filter(|worker| {
                !worker.dynamic()
                    && worker.assignment.source.kind != paper_control::SourceKind::Static
            })
            .map(|worker| (worker.assignment.clone(), worker.output.clone()))
            .collect::<Vec<_>>();
        if frozen.is_empty() {
            self.set_dynamic_pause(false).await?;
            self.paused = false;
            return Ok(None);
        }
        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        let backends = self.backend_paths();
        let mut candidates = Vec::with_capacity(frozen.len());
        for (mut assignment, output) in frozen {
            assignment.transition = None;
            match backends.spawn(assignment, output, socket, generation, self.policy.as_ref()) {
                Ok(candidate) => candidates.push(candidate),
                Err(error) => {
                    for candidate in candidates {
                        candidate.stop().await;
                    }
                    return Err(error);
                }
            }
        }
        let touched = worker_outputs(&candidates);
        Ok(Some(ApplyTransaction {
            candidates,
            overlays: Vec::new(),
            next: None,
            ready: BTreeSet::new(),
            touched,
            replace_all: false,
            generation,
            policy: self.policy.clone(),
            reported_failure: None,
        }))
    }

    pub(crate) async fn commit_resume(
        &mut self,
        transaction: ApplyTransaction,
    ) -> Result<Vec<WorkerStatus>> {
        self.set_dynamic_pause(false).await?;
        self.paused = false;
        match self.commit(transaction).await {
            Ok(status) => Ok(status),
            Err(error) => {
                self.paused = true;
                let _ = self.set_dynamic_pause(true).await;
                Err(error)
            }
        }
    }

    async fn set_dynamic_pause(&mut self, paused: bool) -> Result<()> {
        let mut changed: Vec<usize> = Vec::new();
        for index in 0..self.workers.len() {
            if !self.workers[index].dynamic() {
                continue;
            }
            if let Err(error) = self.workers[index].send(&PaperCommand::pause(paused)).await {
                for changed_index in changed {
                    let _ = self.workers[changed_index].send(&PaperCommand::pause(!paused)).await;
                }
                return Err(error.context("set Paper pause state"));
            }
            changed.push(index);
        }
        Ok(())
    }

    pub(crate) async fn audio(
        &mut self,
        request: &AudioSetRequest,
    ) -> Result<(usize, Vec<WorkerStatus>)> {
        request.validate().map_err(|error| anyhow!(error.to_string()))?;
        self.refresh()?;
        let all = request.outputs.is_empty() || request.outputs.as_slice() == ["*"];
        if !all && self.workers.iter().any(|worker| worker.output == "*") {
            return Err(anyhow!(
                "targeted audio is unavailable while a wildcard assignment is active"
            ));
        }
        let wanted: BTreeSet<&str> = request.outputs.iter().map(String::as_str).collect();
        let targets = self
            .workers
            .iter()
            .enumerate()
            .filter_map(|(index, worker)| {
                (all || wanted.contains(worker.output.as_str())).then_some(index)
            })
            .collect::<Vec<_>>();
        let previous = targets
            .iter()
            .map(|index| {
                let assignment = &self.workers[*index].assignment;
                (*index, assignment.mute, assignment.volume)
            })
            .collect::<Vec<_>>();
        let mut changed: Vec<(usize, bool, u32)> = Vec::new();
        for (index, mute, volume) in &previous {
            if !self.workers[*index].dynamic() {
                continue;
            }
            if let Err(error) =
                self.workers[*index].send(&PaperCommand::audio(request.mute, request.volume)).await
            {
                for (changed_index, old_mute, old_volume) in changed {
                    let _ = self.workers[changed_index]
                        .send(&PaperCommand::audio(Some(old_mute), Some(old_volume)))
                        .await;
                }
                return Err(error.context("set Paper audio state"));
            }
            changed.push((*index, *mute, *volume));
        }
        for index in &targets {
            if let Some(mute) = request.mute {
                self.workers[*index].assignment.mute = mute;
            }
            if let Some(volume) = request.volume {
                self.workers[*index].assignment.volume = volume;
            }
        }
        let statuses = targets.iter().map(|index| worker_status(&self.workers[*index])).collect();
        Ok((targets.len(), statuses))
    }

    pub(crate) async fn stop(&mut self, request: &StopRequest) -> Result<usize> {
        let all = request.outputs.is_empty() || request.outputs.iter().any(|output| output == "*");
        let wanted: BTreeSet<&str> = request.outputs.iter().map(String::as_str).collect();
        let mut changes = Vec::new();
        let mut count = 0;
        if !all {
            for index in 0..self.workers.len() {
                let old = self.workers[index].assignment.outputs.clone();
                let keep = old
                    .iter()
                    .filter(|output| !wanted.contains(output.as_str()))
                    .cloned()
                    .collect::<Vec<_>>();
                count += old.len() - keep.len();
                if !keep.is_empty() && keep.len() != old.len() {
                    if let Err(error) = retarget(&mut self.workers[index], &keep).await {
                        rollback_retargets(&mut self.workers, changes).await;
                        return Err(error.context("retarget Paper worker during stop"));
                    }
                    changes.push((index, old));
                }
            }
        }
        let mut retained = Vec::new();
        let mut stopped = Vec::new();
        for worker in self.workers.drain(..) {
            if all
                || worker.assignment.outputs == ["*"]
                || worker.assignment.outputs.iter().all(|output| wanted.contains(output.as_str()))
            {
                stopped.push(worker);
            } else {
                retained.push(worker);
            }
        }
        self.workers = retained;
        if all {
            count = stopped.iter().map(|worker| worker.assignment.outputs.len()).sum();
        }
        for worker in stopped {
            worker.stop().await;
        }
        let mut overlays = Vec::new();
        for overlay in self.overlays.drain(..) {
            if all || overlay.output == "*" || wanted.contains(overlay.output.as_str()) {
                overlay.stop().await;
            } else {
                overlays.push(overlay);
            }
        }
        self.overlays = overlays;
        Ok(count)
    }

    async fn retain_untouched(&mut self, transaction: &ApplyTransaction) -> Result<()> {
        if transaction.replace_all || transaction.touched.contains("*") {
            return Ok(());
        }
        let mut changes = Vec::new();
        for index in 0..self.workers.len() {
            let old = self.workers[index].assignment.outputs.clone();
            let keep = old
                .iter()
                .filter(|output| !transaction.touched.contains(*output))
                .cloned()
                .collect::<Vec<_>>();
            if !keep.is_empty() && keep.len() != old.len() {
                if let Err(error) = retarget(&mut self.workers[index], &keep).await {
                    rollback_retargets(&mut self.workers, changes).await;
                    return Err(error.context("retarget incumbent Paper worker"));
                }
                changes.push((index, old));
            }
        }
        Ok(())
    }

    pub(crate) fn status(&self) -> Vec<WorkerStatus> {
        self.workers.iter().map(worker_status).collect()
    }

    pub(crate) fn renderer_capabilities(&self) -> Vec<paper_control::RendererCapability> {
        self.backend_paths().capabilities()
    }

    fn backend_paths(&self) -> BackendPaths {
        self.backends.clone().unwrap_or_else(BackendPaths::discover)
    }
}

fn worker_status(worker: &Worker) -> WorkerStatus {
    WorkerStatus {
        assignment: worker.assignment.clone(),
        pid: worker.pid(),
        generation: worker.generation,
    }
}

fn worker_outputs(workers: &[Worker]) -> BTreeSet<String> {
    workers.iter().flat_map(|worker| worker.assignment.outputs.iter().cloned()).collect()
}

fn resolve_transition_from(
    assignment: &mut Assignment,
    output: &str,
    workers: &[Worker],
) -> Result<()> {
    let Some(transition) = &mut assignment.transition else {
        return Ok(());
    };
    if transition.from.is_some() {
        return Ok(());
    }
    let incumbents = workers
        .iter()
        .filter(|worker| {
            output == "*"
                || worker.assignment.outputs == ["*"]
                || worker.assignment.outputs.iter().any(|candidate| candidate == output)
        })
        .collect::<Vec<_>>();
    if incumbents.is_empty() {
        return Ok(());
    }
    let mut paths = BTreeSet::new();
    for incumbent in incumbents {
        paths.insert(transition_source(&incumbent.assignment.source)?);
    }
    if paths.len() != 1 {
        return Err(anyhow!(
            "wildcard transition requires an explicit from hint when incumbents differ"
        ));
    }
    transition.from = paths.into_iter().next();
    Ok(())
}

fn freeze_parent() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or_else(|| anyhow!("Paper freeze cache has no XDG_CACHE_HOME or HOME"))?;
    let path = base.join("skwd-paper-v2").join("freeze");
    std::fs::create_dir_all(&path)
        .with_context(|| format!("create Paper freeze cache {}", path.display()))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    let metadata = std::fs::symlink_metadata(&path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(anyhow!("Paper freeze cache must be a private owned directory"));
    }
    Ok(path)
}

async fn wait_snapshots(
    workers: &mut [Worker],
    indices: &[usize],
    paths: &[PathBuf],
) -> Result<()> {
    let timeout = std::env::var("SKWD_PAPER_FREEZE_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(Duration::from_secs(5), Duration::from_millis);
    let deadline = Instant::now() + timeout;
    loop {
        let mut complete = true;
        for (index, path) in indices.iter().zip(paths) {
            let error_path = PathBuf::from(format!("{}.error", path.display()));
            if let Ok(metadata) = std::fs::symlink_metadata(&error_path) {
                if metadata.nlink() != 1 {
                    complete = false;
                    continue;
                }
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.uid() != unsafe { libc::geteuid() }
                    || metadata.mode() & 0o777 != 0o600
                {
                    return Err(anyhow!("Paper freeze error failed file validation"));
                }
                let message = std::fs::read_to_string(&error_path)
                    .unwrap_or_else(|_| "Paper worker could not capture a freeze frame".into());
                return Err(anyhow!(message));
            }
            match std::fs::symlink_metadata(path) {
                Ok(metadata) => {
                    if metadata.nlink() != 1 {
                        complete = false;
                        continue;
                    }
                    if !metadata.is_file()
                        || metadata.file_type().is_symlink()
                        || metadata.uid() != unsafe { libc::geteuid() }
                        || metadata.mode() & 0o777 != 0o600
                        || metadata.len() <= 12
                    {
                        return Err(anyhow!("Paper freeze frame failed file validation"));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    complete = false;
                    if let Some(status) = workers[*index].exited()? {
                        return Err(anyhow!(
                            "Paper worker {} exited during freeze: {status}",
                            workers[*index].pid()
                        ));
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }
        if complete {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("Paper freeze-frame capture timed out"));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        for worker in self.workers.drain(..).chain(self.overlays.drain(..)) {
            worker.stop_blocking();
        }
    }
}

fn primary_sand_transition_output(
    policy: Option<&RendererPolicy>,
    assignments: &[Assignment],
) -> Option<String> {
    let sand = policy.and_then(|policy| policy.sand.as_ref())?;
    if sand.scope != Some(SandScope::Primary) {
        return None;
    }
    let outputs = assignments
        .iter()
        .filter(|assignment| {
            assignment
                .transition
                .as_ref()
                .is_some_and(|transition| transition.effect().starts_with("sand-"))
        })
        .flat_map(|assignment| assignment.outputs.iter());
    let configured = sand.primary.as_deref().map(str::trim).filter(|output| !output.is_empty());
    outputs
        .clone()
        .find(|output| configured == Some(output.as_str()))
        .or_else(|| outputs.into_iter().next())
        .cloned()
}

fn expand(
    assignments: &[Assignment],
    transition_primary: Option<&str>,
) -> Vec<(Assignment, String)> {
    let mut expanded: Vec<(Assignment, String)> = Vec::new();
    for assignment in assignments {
        for output in &assignment.outputs {
            let mut assignment = assignment.clone();
            assignment.outputs = vec![output.clone()];
            if assignment
                .transition
                .as_ref()
                .is_some_and(|transition| transition.effect().starts_with("sand-"))
                && transition_primary.is_some_and(|primary| output != primary)
            {
                assignment.transition = None;
            }
            if assignment.source.effective_video_engine() == Some(VideoEngine::Tinier)
                && let Some((group, target)) =
                    expanded.iter_mut().find(|(candidate, _)| same_renderer(candidate, &assignment))
            {
                group.outputs.push(output.clone());
                *target = group.outputs.join(",");
                continue;
            }
            expanded.push((assignment, output.clone()));
        }
    }
    expanded
}

fn same_renderer(left: &Assignment, right: &Assignment) -> bool {
    left.source == right.source
        && left.fill_mode == right.fill_mode
        && left.mute == right.mute
        && left.volume == right.volume
        && left.layer == right.layer
        && left.transition == right.transition
}

async fn retarget(worker: &mut Worker, outputs: &[String]) -> Result<()> {
    if !worker.retain_capable() {
        return Err(anyhow!("Paper worker cannot retain a subset of its outputs"));
    }
    worker.send(&PaperCommand::retain_outputs(outputs)).await?;
    worker.assignment.outputs = outputs.to_vec();
    worker.output = outputs.join(",");
    Ok(())
}

async fn rollback_retargets(workers: &mut [Worker], changes: Vec<(usize, Vec<String>)>) {
    for (index, outputs) in changes.into_iter().rev() {
        let _ = retarget(&mut workers[index], &outputs).await;
    }
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
