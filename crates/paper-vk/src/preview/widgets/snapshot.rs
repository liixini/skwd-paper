use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};

use paper_control::Face;
use paper_widgets::{Place, Scene};

use crate::preview::{StreamTarget, send_packet};

pub(super) const CAPABILITY: &str = "SKWD_PAPER_WIDGET_STREAM_FDS";
const NAME: &std::ffi::CStr = c"skwd-paper-widgets";
const KIND: u8 = 9;
const VERSION: u32 = 1;
const LIMIT: usize = 16 * 1024 * 1024;
const ENTRY_BYTES: usize = 24;
const CHANNELS: usize = 4;
const SLOT_COUNT: usize = 3;
const SEALS: i32 = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;

pub(super) struct Snapshot {
    file: File,
    count: u32,
}

impl Snapshot {
    pub(super) fn paint(
        scene: &Scene<'_>,
        placed: &[(Face, Place)],
        extent: [u32; 2],
    ) -> io::Result<Option<Self>> {
        let placed: Vec<_> = placed
            .iter()
            .filter_map(|&(face, mut place)| {
                if (0..2).any(|axis| place.origin[axis] >= extent[axis]) {
                    return None;
                }
                place.size = std::array::from_fn(|axis| {
                    place.size[axis].min((extent[axis] - place.origin[axis]) as usize)
                });
                (!place.size.contains(&0)).then_some((face, place))
            })
            .collect();
        let Some(size) = snapshot_size(&placed) else {
            return Err(io::Error::other("widget snapshot exceeds fast-path budget"));
        };
        if placed.is_empty() {
            return Ok(None);
        }
        let raw = unsafe {
            libc::memfd_create(NAME.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING)
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut file = unsafe { File::from_raw_fd(raw) };
        file.set_len(size as u64)?;
        let mut offset = placed.len() * ENTRY_BYTES;
        for (_, place) in &placed {
            for value in place.origin.into_iter().chain(place.size.map(|side| side as u32)) {
                file.write_all(&value.to_le_bytes())?;
            }
            file.write_all(&(offset as u64).to_le_bytes())?;
            offset += place.size[0] * place.size[1] * CHANNELS;
        }
        for (face, place) in &placed {
            let canvas = scene.paint(*face, *place, None);
            file.write_all(canvas.pixels.as_flattened())?;
        }
        if unsafe { libc::fcntl(raw, libc::F_ADD_SEALS, SEALS) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(Self { file, count: placed.len() as u32 }))
    }
}

fn snapshot_size(placed: &[(Face, Place)]) -> Option<usize> {
    placed.iter().try_fold(placed.len().checked_mul(ENTRY_BYTES)?, |size, (_, place)| {
        place.size[0]
            .checked_mul(place.size[1])?
            .checked_mul(CHANNELS)?
            .checked_add(size)
            .filter(|size| *size <= LIMIT)
    })
}

#[derive(Default)]
pub(super) struct Snapshots {
    current: Option<Snapshot>,
    revision: u64,
    slots: [u64; SLOT_COUNT],
    pub(super) fallback: bool,
}

impl Snapshots {
    pub(super) fn replace(&mut self, snapshot: Option<Snapshot>) {
        self.current = snapshot;
        self.revision += 1;
    }

    pub(super) fn publish(&mut self, target: &StreamTarget, slot: usize) -> io::Result<()> {
        if self.slots[slot] == self.revision {
            return Ok(());
        }
        let count = self.current.as_ref().map_or(0, |snapshot| snapshot.count);
        let bytes = crate::preview::packet(
            KIND,
            slot as u8,
            VERSION,
            count,
            target.width,
            target.height,
            self.revision,
        );
        let fd = self.current.as_ref().map(|snapshot| snapshot.file.as_raw_fd());
        send_packet(target.socket, &bytes, fd)?;
        self.slots[slot] = self.revision;
        Ok(())
    }
}

pub(super) fn enabled(socket: RawFd, capability: Option<&str>) -> bool {
    socket >= 0 && paper_runtime::plasma::ready_fds(capability).contains(&socket)
}

#[cfg(test)]
mod tests;
