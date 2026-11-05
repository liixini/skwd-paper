use super::buffer_set::BufferSet;
use super::model::{App, SurfaceState};
use paper_control::MirrorTarget;
use paper_runtime::mirror::{Layout, Link, Message};
use std::os::fd::{AsFd, OwnedFd, RawFd};
use std::path::Path;

pub(super) struct Shown {
    pub(super) memory: OwnedFd,
    pub(super) layout: Layout,
    pub(super) source: [u32; 4],
}

pub(super) struct Mirror {
    link: Link,
    id: Option<u8>,
}

impl SurfaceState {
    pub(super) fn show(&mut self, buffer: &BufferSet, source: [u32; 4]) {
        self.shown_at += 1;
        self.shown = match buffer.share() {
            Ok((memory, layout)) => Some(Shown { memory, layout, source }),
            Err(error) => {
                tracing::warn!(%error, "mirror: buffer share failed");
                None
            }
        };
        self.publish();
    }

    pub(super) fn move_source(&mut self, source: [u32; 4]) {
        let Some(shown) = self.shown.as_mut() else { return };
        shown.source = source;
        self.shown_at += 1;
        let sent = self.mirror.as_ref().and_then(|mirror| {
            let id = mirror.id?;
            Some(mirror.link.send(&Message::Show { id, source }, &[]))
        });
        if let Some(Err(error)) = sent {
            self.drop_mirror(&error);
        }
    }

    pub(super) fn hide(&mut self) {
        self.shown = None;
        self.shown_at += 1;
        let sent = self.mirror.as_mut().and_then(|mirror| {
            let id = mirror.id.take()?;
            Some(mirror.link.send(&Message::Forget { id }, &[]))
        });
        if let Some(Err(error)) = sent {
            self.drop_mirror(&error);
        }
    }

    fn publish(&mut self) {
        let (Some(mirror), Some(shown)) = (self.mirror.as_mut(), self.shown.as_ref()) else {
            return;
        };
        let id = mirror.link.next_id();
        let previous = mirror.id.replace(id);
        let result = mirror
            .link
            .send(&Message::Buffer { id, layout: shown.layout }, &[shown.memory.as_fd()])
            .and_then(|()| mirror.link.send(&Message::Show { id, source: shown.source }, &[]))
            .and_then(|()| match previous {
                Some(old) => mirror.link.send(&Message::Forget { id: old }, &[]),
                None => Ok(()),
            });
        if let Err(error) = result {
            self.drop_mirror(&error);
        }
    }

    fn drop_mirror(&mut self, error: &std::io::Error) {
        tracing::info!(output = %self.output_name, %error, "mirror: consumer gone");
        self.mirror = None;
    }
}

impl App {
    pub(super) fn set_mirror(&mut self, target: &MirrorTarget) {
        let Some(surface) = self.surfaces.iter_mut().find(|surf| surf.output_name == target.output)
        else {
            tracing::warn!(output = %target.output, "mirror: no surface for output");
            return;
        };
        surface.mirror = None;
        if target.socket.is_empty() {
            tracing::info!(output = %target.output, "mirror: stopped");
            return;
        }
        match Link::connect(Path::new(&target.socket)) {
            Ok(link) => {
                tracing::info!(output = %target.output, "mirror: connected");
                surface.mirror = Some(Mirror { link, id: None });
                surface.publish();
            }
            Err(error) => tracing::warn!(output = %target.output, %error, "mirror: connect failed"),
        }
    }

    pub(super) fn mirror_fds(&self) -> Vec<(usize, RawFd)> {
        self.surfaces
            .iter()
            .enumerate()
            .filter_map(|(index, surf)| Some((index, surf.mirror.as_ref()?.link.fd())))
            .collect()
    }

    pub(super) fn service_mirror(&mut self, index: usize) {
        let Some(surface) = self.surfaces.get_mut(index) else { return };
        let Some(mirror) = surface.mirror.as_ref() else { return };
        if let Err(error) = mirror.link.drain() {
            surface.drop_mirror(&error);
        }
    }
}

pub(super) fn full(width: u32, height: u32) -> [u32; 4] {
    [0, 0, width, height]
}
