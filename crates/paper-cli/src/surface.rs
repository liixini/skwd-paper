use anyhow::{Result, anyhow};
use paper_control::{
    PaperCommand, RendererPolicy, SourceKind, SurfaceOverridesRequest, SurfacePolicy,
};
use std::sync::Arc;

use super::{DRAWING_DIRECTORY, DRAWING_SNAPSHOT, Manager, freeze_parent, wait_snapshots};

const DRAWING_ACK: &str = "applied.ppm";

fn validate_overrides(
    request: &SurfaceOverridesRequest,
    policy: Option<&RendererPolicy>,
) -> Result<()> {
    if let Some(surface) = request.surfaces.values().next() {
        validate_namespace(policy, &surface.namespace)?;
    }
    Ok(())
}

pub(super) fn validate_namespace(policy: Option<&RendererPolicy>, namespace: &str) -> Result<()> {
    let base = policy
        .and_then(|policy| policy.surface.as_deref())
        .ok_or_else(|| anyhow!("active composition has no surface policy"))?;
    if base.namespace != namespace {
        return Err(anyhow!("surface namespace does not match active composition"));
    }
    Ok(())
}

fn effective_surface<'a>(
    request: &'a SurfaceOverridesRequest,
    kind: SourceKind,
    base: Option<&'a SurfacePolicy>,
) -> Option<&'a SurfacePolicy> {
    request.surfaces.get(&kind).or(base)
}

impl Manager {
    pub(super) fn launch_surfaces(
        &self,
        policy: Option<&RendererPolicy>,
        replace_all: bool,
    ) -> Result<SurfaceOverridesRequest> {
        let namespace = |policy: Option<&RendererPolicy>| {
            policy
                .and_then(|policy| policy.surface.as_ref())
                .map(|surface| surface.namespace.clone())
        };
        let request = if replace_all
            && !self.is_empty()
            && namespace(self.policy.as_ref()) != namespace(policy)
        {
            SurfaceOverridesRequest::default()
        } else {
            self.surface_overrides.clone()
        };
        validate_overrides(&request, policy)?;
        Ok(request)
    }

    pub(crate) async fn set_surface_overrides(
        &mut self,
        request: SurfaceOverridesRequest,
    ) -> Result<()> {
        request.validate()?;
        if !self.is_empty() {
            validate_overrides(&request, self.policy.as_ref())?;
        }
        let base = self.policy.as_ref().and_then(|policy| policy.surface.clone());
        self.apply_surfaces(&request, base.as_deref()).await?;
        self.surface_overrides = request;
        Ok(())
    }

    pub(super) async fn apply_surfaces(
        &mut self,
        request: &SurfaceOverridesRequest,
        base: Option<&SurfacePolicy>,
    ) -> Result<()> {
        self.surface_retry.retain(|pid| self.workers.iter().any(|worker| worker.pid() == *pid));
        let previous_base = self.policy.as_ref().and_then(|policy| policy.surface.clone());
        let previous = self.surface_overrides.clone();
        let held: Vec<_> = self
            .workers
            .iter()
            .filter(|worker| {
                worker.drawing_hold.is_some()
                    && (self.surface_retry.contains(&worker.pid())
                        || effective_surface(
                            &previous,
                            worker.assignment.source.kind,
                            previous_base.as_deref(),
                        ) != effective_surface(request, worker.assignment.source.kind, base))
            })
            .map(super::Worker::pid)
            .collect();
        let result = async {
            self.write_surfaces(request, base).await?;
            self.refresh_photos(&held).await
        }
        .await;
        if let Err(error) = result {
            if let Err(rollback) = self.write_surfaces(&previous, previous_base.as_deref()).await {
                tracing::warn!("restore surface effects failed: {rollback:#}");
            }
            if let Err(rollback) = self.refresh_photos(&held).await {
                self.surface_retry.extend(held);
                tracing::warn!("restore drawing photo failed: {rollback:#}");
            }
            return Err(error);
        }
        for pid in held {
            self.surface_retry.remove(&pid);
        }
        Ok(())
    }

    async fn write_surfaces(
        &mut self,
        request: &SurfaceOverridesRequest,
        base: Option<&SurfacePolicy>,
    ) -> Result<()> {
        let mut failure = None;
        for worker in self.workers.iter_mut().chain(&mut self.overlays) {
            if !worker.drawing_overlay()
                && let Some(surface) =
                    effective_surface(request, worker.assignment.source.kind, base)
                && let Err(error) = worker.set_surface(surface).await
            {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    async fn refresh_photos(&mut self, pids: &[u32]) -> Result<()> {
        for pid in pids {
            let Some(index) = self.workers.iter().position(|worker| worker.pid() == *pid) else {
                continue;
            };
            let worker = &self.workers[index];
            let Some(overlay) = self.overlays.iter().position(|overlay| {
                overlay.generation == worker.generation
                    && overlay.output == worker.output
                    && overlay.drawing_overlay()
            }) else {
                continue;
            };
            if self.overlays[overlay].exited()?.is_some() {
                continue;
            }
            let output = worker.output.clone();
            let directory = Arc::new(
                tempfile::Builder::new()
                    .prefix(DRAWING_DIRECTORY)
                    .tempdir_in(self.freeze_parent.clone().map_or_else(freeze_parent, Ok)?)?,
            );
            let photo = directory.path().join(DRAWING_SNAPSHOT);
            self.workers[index].snapshot(&photo, &output, true).await?;
            wait_snapshots(&mut self.workers, &[index], std::slice::from_ref(&photo)).await?;
            self.workers[index].pending_photos.push(Arc::clone(&directory));
            let ack = directory.path().join(DRAWING_ACK);
            let mut command = PaperCommand::snapshot(&ack.display().to_string(), &output);
            command.drawing_photo = Some(photo.display().to_string());
            self.overlays[overlay].send(&command).await?;
            wait_snapshots(&mut self.overlays, &[overlay], &[ack]).await?;
            self.workers[index].pending_photos.clear();
            self.workers[index].drawing_hold = Some(directory);
        }
        Ok(())
    }
}
