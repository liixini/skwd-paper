use crate::model::FrameRate;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::Path;

const HEADER_BYTES: usize = 32;
const PACKET_HEADER_BYTES: usize = 12;
const MAX_INPUT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PACKETS: usize = 65_536;
const MAX_PACKET_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Packet {
    bytes: Vec<u8>,
}

impl AsRef<[u8]> for Packet {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PacketIndex {
    offset: usize,
    size: usize,
}

pub struct ResidentVideo {
    file: File,
    file_bytes: usize,
    packets: Vec<PacketIndex>,
    frame_rate: FrameRate,
}

impl ResidentVideo {
    pub fn load(path: &Path) -> Result<Self, String> {
        let file =
            File::open(path).map_err(|error| format!("cannot open {}: {error}", path.display()))?;
        let metadata = file.metadata().map_err(|error| format!("fstat failed: {error}"))?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_INPUT_BYTES {
            return Err(format!(
                "input must be a non-empty regular file no larger than {MAX_INPUT_BYTES} bytes"
            ));
        }
        let file_bytes = usize::try_from(metadata.len()).map_err(|_| "input size exceeds usize")?;
        let mut header = [0; HEADER_BYTES];
        read_exact(&file, &mut header, 0)?;
        let frame_rate = validate_header(&header)?;
        let declared = usize::try_from(read_u32(&header, 24)).unwrap();
        let packets = index_packets(&file, file_bytes, declared)?;
        Ok(Self { file, file_bytes, packets, frame_rate })
    }

    pub fn packet(&self, index: usize) -> Result<Packet, String> {
        let packet = self.packets[index];
        let mut bytes = vec![0; packet.size];
        read_exact(&self.file, &mut bytes, packet.offset)?;
        Ok(Packet { bytes })
    }

    pub const fn packet_count(&self) -> usize {
        self.packets.len()
    }

    pub const fn frame_rate(&self) -> FrameRate {
        self.frame_rate
    }

    pub const fn source_bytes(&self) -> usize {
        self.file_bytes
    }

    pub fn resident_bytes(&self) -> usize {
        self.packets.len() * std::mem::size_of::<PacketIndex>()
    }
}

fn validate_header(bytes: &[u8; HEADER_BYTES]) -> Result<FrameRate, String> {
    if &bytes[0..4] != b"DKIF"
        || read_u16(bytes, 4) != 0
        || usize::from(read_u16(bytes, 6)) != HEADER_BYTES
        || &bytes[8..12] != b"AV01"
    {
        return Err("invalid AV1 IVF header".into());
    }
    if read_u16(bytes, 12) == 0 || read_u16(bytes, 14) == 0 {
        return Err("IVF dimensions must be nonzero".into());
    }
    FrameRate::new(read_u32(bytes, 16), read_u32(bytes, 20))
}

fn index_packets(
    file: &File,
    file_bytes: usize,
    declared: usize,
) -> Result<Vec<PacketIndex>, String> {
    if declared == 0 || declared > MAX_PACKETS {
        return Err("IVF frame count is outside the supported range".into());
    }
    let mut packets = Vec::new();
    packets
        .try_reserve_exact(declared)
        .map_err(|error| format!("cannot allocate IVF index: {error}"))?;
    let mut offset = HEADER_BYTES;
    while offset < file_bytes {
        if packets.len() == declared || file_bytes - offset < PACKET_HEADER_BYTES {
            return Err("invalid IVF frame table".into());
        }
        let mut header = [0; PACKET_HEADER_BYTES];
        read_exact(file, &mut header, offset)?;
        let size = usize::try_from(read_u32(&header, 0)).unwrap();
        let timestamp = read_u64(&header, 4);
        offset += PACKET_HEADER_BYTES;
        if size == 0 || size > MAX_PACKET_BYTES || size > file_bytes - offset {
            return Err(format!("invalid IVF packet {}", packets.len()));
        }
        if timestamp != packets.len() as u64 {
            return Err(format!("IVF packet {} has a non-sequential timestamp", packets.len()));
        }
        packets.push(PacketIndex { offset, size });
        offset += size;
    }
    if packets.len() != declared {
        return Err(format!(
            "IVF declared {declared} frames but contains {} packets",
            packets.len()
        ));
    }
    Ok(packets)
}

fn read_exact(file: &File, bytes: &mut [u8], offset: usize) -> Result<(), String> {
    file.read_exact_at(bytes, u64::try_from(offset).unwrap())
        .map_err(|error| format!("read IVF at byte {offset}: {error}"))
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

#[cfg(test)]
#[path = "ivf/tests.rs"]
mod tests;
