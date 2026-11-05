use super::{channels, cstring, draw_order, read_u32, take};
use anyhow::{Result, anyhow};
use std::ops::Range;

const ID_BYTES: usize = size_of::<u64>();
const ADDITIVE: u32 = 1;
const INVERTED: u32 = 2;

#[derive(Debug, PartialEq)]
pub enum Command {
    Draw(Range<u32>),
    Clip(Clip),
}

#[derive(Debug, PartialEq)]
pub struct Clip {
    pub mask: usize,
    pub base: Range<u32>,
    pub clipped: Range<u32>,
    pub ancestors: Vec<(usize, Range<u32>)>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub commands: Vec<Command>,
    pub indices: Vec<u16>,
}

struct Region {
    mask: usize,
    last: usize,
    span: Vec<usize>,
    clipped: Vec<usize>,
}

struct Source<'a> {
    spans: &'a [Range<usize>],
    order: &'a [usize],
    indices: &'a [u16],
}

#[derive(Debug, Default, PartialEq)]
pub struct Masks {
    pub records: Vec<Mask>,
    pub spans: Vec<Range<usize>>,
}

#[derive(Debug, PartialEq)]
pub struct Mask {
    pub texture: String,
    pub flags: u32,
    pub clipped: Vec<u32>,
    pub base: Vec<u32>,
}

impl Mask {
    pub fn additive(&self) -> bool {
        self.flags & ADDITIVE != 0
    }

    pub fn inverted(&self) -> bool {
        self.flags & INVERTED != 0
    }
}

pub(super) fn parse(bytes: &[u8], mut at: usize, version: &[u8]) -> Result<Masks> {
    if version != super::MAGIC_23 || at == bytes.len() {
        return Ok(Masks::default());
    }
    let table = channels::blobs(bytes, &mut at, version)?;
    if at == bytes.len() {
        return Ok(Masks::default());
    }
    let count = read_u32(bytes, &mut at)?;
    let mut records = Vec::new();
    for _ in 0..count {
        take(bytes, &mut at, ID_BYTES)?;
        let texture = cstring(bytes, &mut at)?.to_owned();
        let flags = read_u32(bytes, &mut at)?;
        let clipped = sections(bytes, &mut at)?;
        let base = sections(bytes, &mut at)?;
        records.push(Mask { texture, flags, clipped, base });
    }
    if records.is_empty() {
        return Ok(Masks::default());
    }
    let spans = draw_order::spans(table).ok_or_else(|| anyhow!("puppet mask sections invalid"))?;
    Ok(Masks { records, spans })
}

impl Masks {
    pub fn supported(&self, indices: usize) -> bool {
        let sections = self.spans.len();
        let parents = self.parents();
        draw_order::tiled(self.spans.iter().cloned(), indices)
            && self.records.iter().all(|mask| {
                mask.flags & !(ADDITIVE | INVERTED) == 0
                    && mask
                        .clipped
                        .iter()
                        .chain(&mask.base)
                        .all(|section| (*section as usize) < sections)
            })
            && (0..self.records.len())
                .all(|mask| ancestors(&parents, mask).nth(self.records.len()).is_none())
    }

    pub fn capacity(&self, indices: usize) -> usize {
        indices * (self.records.len() + 1)
    }

    pub fn nested(&self) -> bool {
        self.parents().iter().any(Option::is_some)
    }

    fn parents(&self) -> Vec<Option<usize>> {
        self.records
            .iter()
            .enumerate()
            .map(|(index, mask)| {
                self.records.iter().enumerate().position(|(other, parent)| {
                    other != index
                        && !mask.base.is_empty()
                        && mask.base.iter().all(|section| parent.clipped.contains(section))
                })
            })
            .collect()
    }

    pub fn plan(&self, order: &[usize], indices: &[u16]) -> Plan {
        let source = Source { spans: &self.spans, order, indices };
        let ordinal = |position: usize| order[position] as u32;
        let positions = |list: &[u32]| -> Vec<usize> {
            (0..order.len()).filter(|position| list.contains(&ordinal(*position))).collect()
        };
        let parents = self.parents();
        let mut regions: Vec<Region> = self
            .records
            .iter()
            .enumerate()
            .filter_map(|(index, mask)| {
                let base = positions(&mask.base);
                let (first, last) = (*base.first()?, *base.last()?);
                let clipped = positions(&mask.clipped);
                let span = (first..=last).filter(|position| !clipped.contains(position)).collect();
                Some(Region { mask: index, last, span, clipped })
            })
            .collect();
        let raw: Vec<_> = regions.iter().map(|region| (region.mask, region.last)).collect();
        for region in &mut regions {
            for ancestor in ancestors(&parents, region.mask) {
                if let Some((_, last)) = raw.iter().find(|(mask, _)| *mask == ancestor) {
                    region.last = region.last.max(*last);
                }
            }
        }
        regions.sort_by_key(|region| region.clipped.first().copied().unwrap_or(usize::MAX));
        let clipped = |position: usize| {
            self.records.iter().any(|mask| mask.clipped.contains(&ordinal(position)))
        };
        let mut plan = Plan::default();
        let mut bases = vec![None; self.records.len()];
        let mut pending = Vec::new();
        for position in 0..order.len() {
            if !clipped(position) {
                pending.push(position);
            }
            for region in regions.iter().filter(|region| region.last == position) {
                plan.draw(&mut pending, &source);
                let base = plan.append(&region.span, &source);
                let clipped = plan.append(&region.clipped, &source);
                bases[region.mask] = Some(base.clone());
                let ancestors = ancestors(&parents, region.mask)
                    .filter_map(|ancestor| Some((ancestor, bases[ancestor].clone()?)))
                    .collect();
                plan.commands.push(Command::Clip(Clip {
                    mask: region.mask,
                    base,
                    clipped,
                    ancestors,
                }));
            }
        }
        plan.draw(&mut pending, &source);
        if plan.indices.len() > self.capacity(indices.len()) {
            let mut unmasked = Plan::default();
            unmasked.draw(&mut (0..order.len()).collect(), &source);
            return unmasked;
        }
        plan
    }
}

impl Plan {
    fn draw(&mut self, pending: &mut Vec<usize>, source: &Source<'_>) {
        if pending.is_empty() {
            return;
        }
        let range = self.append(pending, source);
        self.commands.push(Command::Draw(range));
        pending.clear();
    }

    fn append(&mut self, positions: &[usize], source: &Source<'_>) -> Range<u32> {
        let start = self.indices.len() as u32;
        for position in positions {
            let span = source.spans[source.order[*position]].clone();
            self.indices.extend_from_slice(&source.indices[span]);
        }
        start..self.indices.len() as u32
    }
}

fn ancestors(parents: &[Option<usize>], mask: usize) -> impl Iterator<Item = usize> {
    std::iter::successors(parents[mask], |ancestor| parents[*ancestor])
}

fn sections(bytes: &[u8], at: &mut usize) -> Result<Vec<u32>> {
    let count = read_u32(bytes, at)? as usize;
    let words = take(bytes, at, count.saturating_mul(size_of::<u32>()))?;
    let mut cursor = 0;
    (0..count).map(|_| read_u32(words, &mut cursor)).collect()
}

#[cfg(test)]
pub(crate) mod tests;
