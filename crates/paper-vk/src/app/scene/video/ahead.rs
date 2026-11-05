use crate::app::shared_support::SendFrame;
use crate::decode::{AnyDecoder, RenderFrame};
use anyhow::{Context, Result, anyhow};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, sync_channel};
use std::thread::JoinHandle;

const DEPTH: usize = 1;
const THREAD: &str = "scene-video";
const STOPPED: &str = "scene video decoder stopped";

pub(super) struct Ahead {
    frames: Option<Receiver<Result<SendFrame>>>,
    worker: Option<JoinHandle<AnyDecoder>>,
    transfer: Arc<AtomicBool>,
}

impl Ahead {
    pub(super) fn start(mut decoder: AnyDecoder) -> Result<Self> {
        let (tx, frames) = sync_channel(DEPTH);
        let transfer = Arc::new(AtomicBool::new(false));
        let requested = transfer.clone();
        let worker = std::thread::Builder::new()
            .name(THREAD.into())
            .spawn(move || {
                loop {
                    if requested.load(Ordering::Relaxed) {
                        decoder.transfer_frames();
                    }
                    let decoded = decoder.next_render().map(|(frame, pts)| SendFrame(frame, pts));
                    let failed = decoded.is_err();
                    if tx.send(decoded).is_err() || failed {
                        return decoder;
                    }
                }
            })
            .context("spawn scene video decoder")?;
        Ok(Self { frames: Some(frames), worker: Some(worker), transfer })
    }

    pub(super) fn next(&mut self) -> Result<(RenderFrame, f64)> {
        let frames = self.frames.as_ref().ok_or_else(|| anyhow!(STOPPED))?;
        match frames.recv().map_err(|_| anyhow!(STOPPED))? {
            Ok(SendFrame(frame, pts)) => Ok((frame, pts)),
            Err(error) => {
                *self = Self::start(self.stop()?)?;
                Err(error)
            }
        }
    }

    pub(super) fn transfer_frames(&self) {
        self.transfer.store(true, Ordering::Relaxed);
    }

    pub(super) fn stop(&mut self) -> Result<AnyDecoder> {
        self.frames = None;
        let worker = self.worker.take().ok_or_else(|| anyhow!(STOPPED))?;
        worker.join().map_err(|_| anyhow!("scene video decoder panicked"))
    }
}

impl Drop for Ahead {
    fn drop(&mut self) {
        self.frames = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests;
