use super::{COLOR_CHANNELS, Rng, SPATIAL_AXES, num_of, vec3_of};
use crate::affine::Affine;
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;

pub const NAME: &str = "layerimage";
const FLAGS: &str = "flags";
const OFFSET_MIN: &str = "offsetmin";
const OFFSET_MAX: &str = "offsetmax";
const SPEED_MIN: &str = "speedmin";
const SPEED_MAX: &str = "speedmax";
pub const REFRESH_SECONDS: f32 = 1.0;
pub const UNBOUND_BONE: u8 = u8::MAX;
pub const ALPHA_THRESHOLD: u8 = 127;
pub const MAX_TARGET: [u32; 2] = [960, 540];

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ImageFlags: u32 {
        const COLOR = 0x10000;
        const REFRESH = 0x20000;
        const MOTION = 0x40000;
        const OFFSET = 0x80000;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageDependency {
    pub source: u32,
    pub ordinal: usize,
    pub mask: String,
}

impl ImageDependency {
    pub fn parse(object: &Value) -> Vec<Self> {
        super::dependency::parse(object, super::dependency::Kind::Image)
            .map(|entry| Self {
                source: entry.source,
                ordinal: entry.ordinal,
                mask: entry.mask.to_owned(),
            })
            .collect()
    }
}

pub struct ImageEmitter {
    pub ordinal: usize,
    pub flags: ImageFlags,
    pub offset: [[f32; 3]; 2],
    pub speed: [f32; 2],
}

impl ImageEmitter {
    pub fn parse(entry: &Value) -> Self {
        Self {
            ordinal: 0,
            flags: ImageFlags::from_bits_retain(num_of(
                entry.get(FLAGS),
                ImageFlags::COLOR.bits() as f32,
            ) as u32),
            offset: [
                vec3_of(entry.get(OFFSET_MIN), [0.0; SPATIAL_AXES]),
                vec3_of(entry.get(OFFSET_MAX), [0.0; SPATIAL_AXES]),
            ],
            speed: [
                num_of(entry.get(SPEED_MIN), Self::DEFAULT_SPEED[0]),
                num_of(entry.get(SPEED_MAX), Self::DEFAULT_SPEED[1]),
            ],
        }
    }

    const DEFAULT_SPEED: [f32; 2] = [0.1, 0.2];

    pub(super) fn birth(&self, source: &SampleSource, rng: &mut Rng) -> Option<ImageBirth> {
        if source.cells.is_empty() {
            return None;
        }
        let cell = source.cells[rng.index(source.cells.len() as u32 - 1)];
        let mut point = [f32::from(cell.position[0]), -f32::from(cell.position[1]), 0.0];
        if self.flags.contains(ImageFlags::OFFSET) {
            for (axis, value) in point.iter_mut().enumerate() {
                *value += rng.range(self.offset[0][axis], self.offset[1][axis]);
            }
        }
        let position = source.current.point(point, cell.bone)?;
        let velocity = if self.flags.contains(ImageFlags::MOTION) && source.dt > 0.0 {
            let previous = source.previous.point(point, cell.bone)?;
            let speed = rng.range(self.speed[0], self.speed[1]) / source.dt;
            std::array::from_fn(|axis| (position[axis] - previous[axis]) * speed)
        } else {
            [0.0; SPATIAL_AXES]
        };
        let color = if self.flags.contains(ImageFlags::COLOR) {
            cell.color.map(|channel| f32::from(channel) / f32::from(u8::MAX))
        } else {
            [1.0; COLOR_CHANNELS]
        };
        Some(ImageBirth { position, velocity, color })
    }
}

pub(super) struct ImageBirth {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub color: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct SampleCell {
    pub color: [u8; 3],
    pub bone: u8,
    pub position: [i16; 2],
}

#[derive(Clone, Debug, Default)]
pub struct SamplePose {
    pub transform: Affine,
    pub bones: std::sync::Arc<[Option<crate::puppet::BonePose>]>,
}

impl SamplePose {
    fn point(&self, point: [f32; 3], bone: u8) -> Option<[f32; 3]> {
        let point = if bone == UNBOUND_BONE {
            point
        } else {
            self.bones.get(usize::from(bone)).copied().flatten()?.point(point)
        };
        Some(self.transform.point(point))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleState {
    Unready,
    Pending,
    Ready,
    Resized,
}

pub type ImageSource = Rc<RefCell<SampleSource>>;

pub struct SampleSource {
    pub cells: Vec<SampleCell>,
    pub state: SampleState,
    pub requested: bool,
    pub first_query_frame: Option<u32>,
    pub current: SamplePose,
    pub previous: SamplePose,
    pub dt: f32,
}

impl Default for SampleSource {
    fn default() -> Self {
        Self {
            cells: Vec::new(),
            state: SampleState::Unready,
            requested: false,
            first_query_frame: None,
            current: SamplePose::default(),
            previous: SamplePose::default(),
            dt: 0.0,
        }
    }
}

impl SampleSource {
    pub fn query(&mut self, frame: u32) -> bool {
        if self.state == SampleState::Resized {
            self.state = SampleState::Unready;
            self.requested = false;
            return true;
        }
        if self.state == SampleState::Ready {
            return true;
        }
        if self.first_query_frame.is_none() || self.first_query_frame == Some(frame) {
            self.first_query_frame = Some(frame);
            return false;
        }
        self.request();
        true
    }

    pub fn request(&mut self) {
        if self.state == SampleState::Unready {
            self.requested = true;
        }
    }

    pub fn invalidate(&mut self) {
        if self.state != SampleState::Pending {
            self.state = SampleState::Unready;
        }
    }

    pub fn resize(&mut self) {
        self.state = match self.state {
            SampleState::Ready | SampleState::Resized => SampleState::Resized,
            SampleState::Unready | SampleState::Pending => SampleState::Unready,
        };
    }
}

#[cfg(test)]
#[path = "particles_image_tests.rs"]
mod tests;

const DOWNSAMPLE: u32 = 4;
const RGBA_BYTES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleGrid {
    source: [u32; 2],
    target: [u32; 2],
}

impl SampleGrid {
    pub fn new(source: [u32; 2]) -> Option<Self> {
        if source.contains(&0) {
            return None;
        }
        let limit = MAX_TARGET.map(|value| value * DOWNSAMPLE);
        let mut fitted = source;
        if source[0] > limit[0] || source[1] > limit[1] {
            let ratio = source[0] as f32 / source[1] as f32;
            fitted = if ratio < limit[0] as f32 / limit[1] as f32 {
                [(ratio * limit[1] as f32) as u32, limit[1]]
            } else {
                [limit[0], (limit[0] as f32 / ratio) as u32]
            };
        }
        Some(Self { source, target: fitted.map(|value| (value / DOWNSAMPLE).max(2)) })
    }

    pub fn source(self) -> [u32; 2] {
        self.source
    }

    pub fn target(self) -> [u32; 2] {
        self.target
    }

    pub fn bytes(self) -> usize {
        self.target[0] as usize * self.target[1] as usize * RGBA_BYTES
    }

    pub fn read(self, pixels: &[u8], cells: &mut Vec<SampleCell>) -> anyhow::Result<()> {
        anyhow::ensure!(pixels.len() == self.bytes(), "sample readback size mismatch");
        cells.clear();
        let scale = self.source[0] as f32 / self.target[0] as f32;
        let center = self.source.map(|value| (value / 2) as f32);
        for x in 0..self.target[0] {
            for y in 0..self.target[1] {
                let offset = (y as usize * self.target[0] as usize + x as usize) * RGBA_BYTES;
                let rgba = &pixels[offset..offset + RGBA_BYTES];
                if rgba[RGBA_BYTES - 1] < ALPHA_THRESHOLD {
                    continue;
                }
                let position = [x, y].map(|value| value as f32 * scale + scale * 0.5);
                cells.push(SampleCell {
                    color: [rgba[0], rgba[1], rgba[2]],
                    bone: UNBOUND_BONE,
                    position: std::array::from_fn(|axis| {
                        (position[axis] - center[axis]) as i32 as i16
                    }),
                });
            }
        }
        Ok(())
    }
}
