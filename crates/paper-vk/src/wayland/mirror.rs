use super::model::{App, SurfaceUi};
use paper_control::MirrorTarget;
use paper_runtime::mirror::{Layout, Link, MAX_PLANES, Memory, Message, Plane};
use std::collections::HashMap;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd, RawFd};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use wayland_client::protocol::wl_shm;
use wayland_client::{Proxy, protocol::wl_buffer::WlBuffer};

const REPLACED: &str = "replaced";
const RESIZED: &str = "presentation resized";
const STALLED: &str = "consumer did not release its frames";
const HOLD_LIMIT: Duration = Duration::from_millis(250);
const UNCLAIMED_LIMIT: Duration = Duration::from_secs(2);

static REQUESTS: Mutex<Vec<(MirrorTarget, Instant)>> = Mutex::new(Vec::new());

pub fn request(target: MirrorTarget) {
    REQUESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((target, Instant::now()));
}

fn take_requests(owns: impl Fn(&str) -> bool) -> Vec<MirrorTarget> {
    let mut queue = REQUESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if queue.is_empty() {
        return Vec::new();
    }
    let mut taken = Vec::new();
    queue.retain(|(target, at)| {
        if owns(&target.output) {
            taken.push(target.clone());
            return false;
        }
        let fresh = at.elapsed() < UNCLAIMED_LIMIT;
        if !fresh {
            tracing::warn!(output = %target.output, "skwd-wall-vk: mirror for unknown output");
        }
        fresh
    });
    taken
}

pub(super) fn dmabuf_layout(
    width: u32,
    height: u32,
    fourcc: u32,
    modifier: u64,
    planes: &[Plane],
) -> Layout {
    let mut padded = [Plane::default(); MAX_PLANES];
    padded[..planes.len()].copy_from_slice(planes);
    Layout {
        width,
        height,
        fourcc,
        modifier,
        memory: Memory::Dmabuf,
        planes: padded,
        plane_count: planes.len() as u8,
    }
}

pub(super) fn shm_fourcc(format: wl_shm::Format) -> u32 {
    match format {
        wl_shm::Format::Argb8888 => paper_runtime::mirror::AR24,
        wl_shm::Format::Xrgb8888 => paper_runtime::mirror::XR24,
        other => other.into(),
    }
}

struct Shared {
    buffer: WlBuffer,
    serial: u64,
    fds: Vec<OwnedFd>,
    layout: Layout,
}

#[derive(Default)]
pub struct Buffers {
    by_buffer: Vec<Shared>,
    serial: u64,
}

impl Buffers {
    pub(super) fn record(&mut self, buffer: &WlBuffer, fds: &[RawFd], layout: Layout) {
        self.by_buffer.retain(|shared| shared.buffer.is_alive());
        let fds = fds
            .iter()
            .filter_map(|&fd| unsafe { BorrowedFd::borrow_raw(fd) }.try_clone_to_owned().ok())
            .collect::<Vec<_>>();
        if fds.len() == usize::from(layout.plane_count) {
            self.serial += 1;
            self.by_buffer.push(Shared {
                buffer: buffer.clone(),
                serial: self.serial,
                fds,
                layout,
            });
        }
    }

    pub(super) fn retire(&mut self, buffer: &WlBuffer) {
        self.by_buffer.retain(|shared| shared.buffer != *buffer);
    }

    fn find(&self, buffer: &WlBuffer) -> Option<&Shared> {
        self.by_buffer.iter().find(|shared| shared.buffer == *buffer && buffer.is_alive())
    }

    fn live(&self, serial: u64) -> bool {
        self.by_buffer.iter().any(|shared| shared.serial == serial && shared.buffer.is_alive())
    }
}

pub struct SurfaceMirror {
    socket: String,
    link: Link,
    sent: HashMap<u64, u8>,
    held: HashMap<u8, usize>,
    deferred: Vec<(usize, Instant)>,
}

impl SurfaceMirror {
    fn holds(&self, bi: usize) -> bool {
        self.held.values().any(|&held| held == bi)
    }

    fn stalled(&self, now: Instant) -> bool {
        self.deferred.iter().any(|&(_, since)| now - since >= HOLD_LIMIT)
    }

    fn settle(&mut self, free: &mut Vec<usize>) {
        let held = &self.held;
        self.deferred.retain(|&(bi, _)| {
            let keep = held.values().any(|&id_bi| id_bi == bi);
            if !keep && !free.contains(&bi) {
                free.push(bi);
            }
            keep
        });
    }
}

impl SurfaceUi {
    pub(super) fn give_back(&mut self, bi: usize) {
        if let Some(mirror) = self.mirror.as_mut()
            && mirror.holds(bi)
        {
            if !mirror.deferred.iter().any(|&(deferred, _)| deferred == bi) {
                mirror.deferred.push((bi, Instant::now()));
            }
            return;
        }
        if !self.free_buffers.contains(&bi) {
            self.free_buffers.push(bi);
        }
    }

    fn end_mirror(&mut self, reason: &dyn std::fmt::Display) {
        let Some(mirror) = self.mirror.take() else { return };
        tracing::info!(output = %self.name, %reason, "skwd-wall-vk: mirror ended");
        for (bi, _) in mirror.deferred {
            if !self.free_buffers.contains(&bi) {
                self.free_buffers.push(bi);
            }
        }
    }

    fn source_for(&self, layout: &Layout) -> [u32; 4] {
        self.source.unwrap_or([0, 0, layout.width, layout.height])
    }
}

impl App {
    pub(super) fn end_mirrors_for_resize(&mut self) {
        for surface in &mut self.surfaces {
            surface.end_mirror(&RESIZED);
        }
    }

    pub(super) fn apply_mirror_requests(&mut self) {
        let names: Vec<String> = self.surfaces.iter().map(|surf| surf.name.clone()).collect();
        for target in take_requests(|output| names.iter().any(|name| name == output)) {
            let Some(si) = self.surfaces.iter().position(|surf| surf.name == target.output) else {
                continue;
            };
            if !target.socket.is_empty()
                && self.surfaces[si]
                    .mirror
                    .as_ref()
                    .is_some_and(|mirror| mirror.socket == target.socket)
            {
                continue;
            }
            self.surfaces[si].end_mirror(&REPLACED);
            if target.socket.is_empty() {
                continue;
            }
            match Link::connect(std::path::Path::new(&target.socket)) {
                Ok(link) => {
                    tracing::info!(output = %target.output, "skwd-wall-vk: mirror connected");
                    self.surfaces[si].mirror = Some(SurfaceMirror {
                        socket: target.socket.clone(),
                        link,
                        sent: HashMap::new(),
                        held: HashMap::new(),
                        deferred: Vec::new(),
                    });
                    if let Some(buffer) = self.surfaces[si].attached.clone() {
                        self.publish_mirror(si, &buffer);
                    }
                }
                Err(error) => {
                    tracing::warn!(output = %target.output, %error, "skwd-wall-vk: mirror connect failed");
                }
            }
        }
    }

    pub(super) fn service_mirrors(&mut self) {
        let now = Instant::now();
        for surf in &mut self.surfaces {
            let Some(mirror) = surf.mirror.as_mut() else { continue };
            match mirror.link.drain() {
                Ok(released) => {
                    for id in released {
                        mirror.held.remove(&id);
                    }
                    mirror.settle(&mut surf.free_buffers);
                    if surf.free_buffers.is_empty() && mirror.stalled(now) {
                        surf.end_mirror(&STALLED);
                    }
                }
                Err(error) => surf.end_mirror(&error),
            }
        }
    }

    pub(super) fn publish_mirror(&mut self, si: usize, buffer: &WlBuffer) {
        let Some(shared) = self.mirror_buffers.find(buffer) else { return };
        let Some(surf) = self.surfaces.get_mut(si) else { return };
        let source = surf.source_for(&shared.layout);
        let Some(mirror) = surf.mirror.as_mut() else { return };
        let bi = buffer.data::<(usize, usize, u64)>().map_or(usize::MAX, |&(_, bi, _)| bi);
        let buffers = &self.mirror_buffers;
        let forgotten: Vec<u8> = mirror
            .sent
            .iter()
            .filter(|&(&serial, _)| !buffers.live(serial))
            .map(|(_, &id)| id)
            .collect();
        mirror.sent.retain(|&serial, _| buffers.live(serial));
        for id in &forgotten {
            mirror.held.remove(id);
        }
        mirror.settle(&mut surf.free_buffers);
        let result = forgotten
            .iter()
            .try_for_each(|&id| mirror.link.send(&Message::Forget { id }, &[]))
            .and_then(|()| {
                let id = if let Some(&id) = mirror.sent.get(&shared.serial) {
                    id
                } else {
                    let id = mirror.link.next_id();
                    let fds: Vec<BorrowedFd<'_>> = shared.fds.iter().map(AsFd::as_fd).collect();
                    mirror.link.send(&Message::Buffer { id, layout: shared.layout }, &fds)?;
                    mirror.sent.insert(shared.serial, id);
                    id
                };
                mirror.link.send(&Message::Show { id, source }, &[])?;
                mirror.held.insert(id, bi);
                Ok(())
            });
        if let Err(error) = result {
            surf.end_mirror(&error);
        }
    }

    pub(super) fn mirror_fds(&self) -> impl Iterator<Item = RawFd> + '_ {
        self.surfaces.iter().filter_map(|surf| Some(surf.mirror.as_ref()?.link.fd()))
    }
}

#[cfg(test)]
mod tests;
