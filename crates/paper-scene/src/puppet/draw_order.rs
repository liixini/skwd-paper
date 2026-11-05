use super::{AnimationLayer, Mesh, channels, read_u32};
use std::ops::Range;

pub(super) const FLAG: u32 = 8;
pub(super) const SECTION_BYTES: usize = size_of::<[u32; 4]>();

#[derive(Clone, Copy, Debug, PartialEq)]
struct Section {
    bone: usize,
    bias: i32,
    first: usize,
    count: usize,
}

#[derive(Debug, PartialEq)]
pub struct Layout {
    sections: Vec<Section>,
    rest: Vec<i32>,
}

impl Layout {
    pub(super) fn parse(
        bytes: &[u8],
        mut at: usize,
        version: &[u8],
        rest: Vec<i32>,
        indices: usize,
    ) -> Option<Self> {
        let table = channels::blobs(bytes, &mut at, version).ok()?;
        if !table.len().is_multiple_of(SECTION_BYTES) {
            return None;
        }
        let sections =
            table.chunks_exact(SECTION_BYTES).map(section).collect::<Option<Vec<_>>>()?;
        let bones = rest.len();
        let spans = sections.iter().map(|section| section.first..section.first + section.count);
        (sections.iter().all(|section| section.bone < bones) && tiled(spans, indices))
            .then_some(Self { sections, rest })
    }
}

fn section(record: &[u8]) -> Option<Section> {
    let mut at = 0;
    let mut word = || read_u32(record, &mut at).ok();
    Some(Section {
        bone: word()? as usize,
        bias: word()?.cast_signed(),
        first: word()? as usize,
        count: word()? as usize,
    })
}

pub(super) fn spans(table: &[u8]) -> Option<Vec<Range<usize>>> {
    if !table.len().is_multiple_of(SECTION_BYTES) {
        return None;
    }
    table
        .chunks_exact(SECTION_BYTES)
        .map(|record| section(record).map(|section| section.first..section.first + section.count))
        .collect()
}

pub(super) fn tiled(spans: impl Iterator<Item = Range<usize>>, indices: usize) -> bool {
    let mut spans: Vec<_> = spans.map(|span| (span.start, span.end)).collect();
    spans.sort_unstable();
    let mut next = 0;
    for (start, end) in &spans {
        if *start != next {
            return false;
        }
        next = *end;
    }
    spans.is_empty() || next == indices
}

struct Slot {
    index: usize,
    order: i32,
    base: i32,
    section: Section,
}

pub struct Sorter {
    slots: Vec<Slot>,
    current: Vec<f32>,
}

impl Sorter {
    pub fn of(mesh: &Mesh) -> Option<Self> {
        let layout = mesh.draw_order.as_ref()?;
        let indexed: usize = layout.sections.iter().map(|section| section.count).sum();
        if indexed != mesh.indices.len()
            || mesh.animations.iter().all(|clip| clip.draw_order.tracks.is_empty())
        {
            return None;
        }
        let slots = layout
            .sections
            .iter()
            .enumerate()
            .map(|(index, section)| Slot {
                index,
                order: 0,
                base: layout.rest[section.bone] + section.bias,
                section: *section,
            })
            .collect();
        Some(Self { slots, current: Vec::with_capacity(layout.rest.len()) })
    }

    pub fn update(&mut self, mesh: &Mesh, samples: &[(AnimationLayer, f32)]) -> bool {
        let Some(layout) = mesh.draw_order.as_ref() else {
            return false;
        };
        if !self.blend(mesh, &layout.rest, samples) {
            return false;
        }
        for slot in &mut self.slots {
            slot.order = (slot.base as f32 + self.current[slot.section.bone]) as i32;
        }
        if self.slots.is_sorted_by_key(|slot| slot.order) {
            return false;
        }
        self.slots.sort_by_key(|slot| slot.order);
        true
    }

    pub fn sections(&self) -> Vec<usize> {
        self.slots.iter().map(|slot| slot.index).collect()
    }

    fn blend(&mut self, mesh: &Mesh, rest: &[i32], samples: &[(AnimationLayer, f32)]) -> bool {
        self.current.clear();
        self.current.extend(rest.iter().map(|order| *order as f32));
        let mut written = false;
        for (layer, seconds) in samples.iter().filter(|(layer, _)| layer.visible) {
            let Some(animation) = mesh.animations.iter().find(|clip| clip.id == layer.id) else {
                continue;
            };
            let (frame, _, _) = animation.sample_frame(*seconds);
            let tracks = animation.draw_order.tracks.iter().zip(rest);
            for (target, (track, rest)) in self.current.iter_mut().zip(tracks) {
                let Some(track) = track else { continue };
                *target = blended(*target, track[frame], *rest as f32, *layer);
                written = true;
            }
        }
        written
    }

    pub fn bytes(&self, indices: &[u16]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(size_of_val(indices));
        for slot in &self.slots {
            let Section { first, count, .. } = slot.section;
            bytes
                .extend(indices[first..first + count].iter().flat_map(|index| index.to_ne_bytes()));
        }
        bytes
    }
}

fn blended(current: f32, value: f32, rest: f32, layer: AnimationLayer) -> f32 {
    if layer.additive {
        (current + (value - rest) * layer.blend).clamp(current.min(value), current.max(value))
    } else {
        current * (1.0 - layer.blend) + value * layer.blend
    }
}

#[cfg(test)]
mod tests;
