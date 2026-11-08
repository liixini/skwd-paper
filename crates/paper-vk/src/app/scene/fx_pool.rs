use super::{FxTargetClass, FxTargetId, LayerFx, RGBA_CHANNELS, TargetPlan, vk};
use anyhow::Result;

pub(super) const STARVED_BACKOFF_FRAMES: u32 = 64;
const PIXEL_KEEPERS: [&str; 3] = ["discard", "texkill", "demote"];
const PIXEL_CLIP: &str = "clip";
const CALL_OPEN: char = '(';
const WORD_JOINER: char = '_';
const UNRESOLVED_INCLUDE: &str = "#include";
const CLEARED: [f32; RGBA_CHANNELS] = [0.0; RGBA_CHANNELS];

pub(super) trait TargetSource {
    type Target;
    fn create(&mut self, class: FxTargetClass) -> Result<Self::Target>;
    fn destroy(&mut self, target: Self::Target);
}

impl TargetSource for vk::Renderer {
    type Target = vk::SceneTarget;

    fn create(&mut self, class: FxTargetClass) -> Result<vk::SceneTarget> {
        let format = ash::vk::Format::from_raw(class.format);
        self.create_scene_target_fmt(class.width, class.height, class.repeat, format)
    }

    fn destroy(&mut self, target: vk::SceneTarget) {
        self.destroy_scene_target(target);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Lease {
    pub index: usize,
    generation: u64,
}

#[derive(Clone, Copy)]
pub(super) struct Starvation {
    pub wait: u32,
    pub backoff: u32,
}

impl Starvation {
    pub fn after(previous: Option<Self>) -> Self {
        let backoff =
            previous.map_or(0, |starved| (starved.backoff * 2).clamp(1, STARVED_BACKOFF_FRAMES));
        Self { wait: backoff, backoff }
    }
}

struct Entry<T> {
    target: T,
    class: FxTargetClass,
    held: bool,
    fresh: bool,
    generation: u64,
}

pub(super) struct FxPool<T = vk::SceneTarget> {
    entries: Vec<Option<Entry<T>>>,
    generation: u64,
}

impl<T> Default for FxPool<T> {
    fn default() -> Self {
        Self { entries: Vec::new(), generation: 0 }
    }
}

impl<T> FxPool<T> {
    fn entry(&self, lease: Lease) -> Option<&Entry<T>> {
        self.entries.get(lease.index)?.as_ref().filter(|entry| entry.generation == lease.generation)
    }

    fn entry_mut(&mut self, lease: Lease) -> Option<&mut Entry<T>> {
        self.entries
            .get_mut(lease.index)?
            .as_mut()
            .filter(|entry| entry.generation == lease.generation)
    }

    pub fn target(&self, lease: Lease) -> Option<&T> {
        self.entry(lease).map(|entry| &entry.target)
    }

    pub fn len(&self) -> usize {
        self.entries.iter().flatten().count()
    }

    fn insert(&mut self, entry: Entry<T>) -> usize {
        if let Some(index) = self.entries.iter().position(Option::is_none) {
            self.entries[index] = Some(entry);
            return index;
        }
        self.entries.push(Some(entry));
        self.entries.len() - 1
    }

    fn hand_out(&mut self, index: usize) -> Lease {
        self.generation += 1;
        let entry = self.entries[index].as_mut().expect("pool entry");
        entry.held = true;
        entry.generation = self.generation;
        Lease { index, generation: self.generation }
    }

    pub fn lease(
        &mut self,
        source: &mut impl TargetSource<Target = T>,
        classes: &[FxTargetClass],
    ) -> Result<Vec<Lease>> {
        let mut leases = Vec::with_capacity(classes.len());
        let mut created = Vec::new();
        for &class in classes {
            let free = self.entries.iter().position(|entry| {
                entry.as_ref().is_some_and(|entry| !entry.held && entry.class == class)
            });
            let index = match free {
                Some(index) => index,
                None => match source.create(class) {
                    Ok(target) => {
                        let entry =
                            Entry { target, class, held: false, fresh: true, generation: 0 };
                        let index = self.insert(entry);
                        created.push(index);
                        index
                    }
                    Err(err) => {
                        self.release(&leases);
                        for index in created {
                            if let Some(entry) = self.entries[index].take() {
                                source.destroy(entry.target);
                            }
                        }
                        return Err(err);
                    }
                },
            };
            leases.push(self.hand_out(index));
        }
        Ok(leases)
    }

    pub fn initialise(&mut self, mut init: impl FnMut(&T) -> Result<()>) -> Result<()> {
        for entry in self.entries.iter_mut().flatten().filter(|entry| entry.fresh) {
            init(&entry.target)?;
            entry.fresh = false;
        }
        Ok(())
    }

    pub fn settle(&mut self, lease: Lease, held: bool) {
        if let Some(entry) = self.entry_mut(lease) {
            entry.held = held;
        }
    }

    pub fn release(&mut self, leases: &[Lease]) {
        for &lease in leases {
            self.settle(lease, false);
        }
    }

    pub fn release_all(&mut self) {
        for entry in self.entries.iter_mut().flatten() {
            entry.held = false;
        }
    }

    pub fn take(&mut self, lease: Lease) -> Option<T> {
        self.entry(lease)?;
        self.entries[lease.index].take().map(|entry| entry.target)
    }

    pub fn trim(&mut self, source: &mut impl TargetSource<Target = T>, keep: &[Lease]) {
        for (index, slot) in self.entries.iter_mut().enumerate() {
            let kept = |entry: &Entry<T>| {
                entry.held || keep.contains(&Lease { index, generation: entry.generation })
            };
            if slot.as_ref().is_some_and(|entry| !kept(entry))
                && let Some(entry) = slot.take()
            {
                source.destroy(entry.target);
            }
        }
        while self.entries.last().is_some_and(Option::is_none) {
            self.entries.pop();
        }
    }

    pub fn destroy(&mut self, source: &mut impl TargetSource<Target = T>) {
        for entry in self.entries.drain(..).flatten() {
            source.destroy(entry.target);
        }
    }
}

impl FxPool {
    pub fn allocation_bytes(&self) -> u64 {
        self.entries.iter().flatten().map(|entry| entry.target.allocation_bytes).sum()
    }
}

fn keeps_pixels(source: &str) -> bool {
    if source.contains(UNRESOLVED_INCLUDE) {
        return true;
    }
    let word_char = |ch: char| ch.is_ascii_alphanumeric() || ch == WORD_JOINER;
    let mut rest = source;
    while let Some(start) = rest.find(word_char) {
        let tail = &rest[start..];
        let (word, after) = tail.split_at(tail.find(|ch| !word_char(ch)).unwrap_or(tail.len()));
        if PIXEL_KEEPERS.contains(&word)
            || (word == PIXEL_CLIP && after.trim_start().starts_with(CALL_OPEN))
        {
            return true;
        }
        rest = after;
    }
    false
}

pub(super) fn overwrites(pass: &paper_scene::effects::EffectPass, blend: vk::SceneBlend) -> bool {
    let sources = std::iter::once(pass.fragment.source.as_str())
        .chain(pass.hlsl.as_ref().map(|hlsl| hlsl.fragment.as_str()));
    blend == vk::SceneBlend::Copy && !sources.into_iter().any(keeps_pixels)
}

pub(super) fn scratch_targets(
    plan: &TargetPlan,
    owners: &[usize],
    overwrites: &[bool],
) -> Vec<bool> {
    let mut scratch: Vec<bool> =
        plan.lifetimes.iter().map(|lifetime| !lifetime.loop_carried).collect();
    let mut writers: Vec<Vec<usize>> = vec![Vec::new(); scratch.len()];
    for ((access, owner), overwrites) in plan.passes.iter().zip(owners).zip(overwrites) {
        for read in access.reads.iter().filter(|read| matches!(read, FxTargetId::Fbo(_))) {
            if !writers[read.index()].contains(owner) {
                scratch[read.index()] = false;
            }
        }
        let write = access.write.index();
        if !writers[write].contains(owner) {
            writers[write].push(*owner);
            scratch[write] &= !matches!(access.write, FxTargetId::Fbo(_)) || *overwrites;
        }
    }
    scratch
}

pub(super) fn cleared_target(
    renderer: &mut vk::Renderer,
    class: FxTargetClass,
    clear: [f32; RGBA_CHANNELS],
) -> Result<vk::SceneTarget> {
    let target = renderer.create(class)?;
    if let Err(err) = renderer.render_scene(&target, clear, &[], &[]) {
        renderer.destroy_scene_target(target);
        return Err(err);
    }
    Ok(target)
}

pub(super) fn lease_chain(
    pool: &mut FxPool,
    renderer: &mut vk::Renderer,
    fx: &mut LayerFx,
    batched: bool,
) -> Result<()> {
    let leased = pool.lease(renderer, &fx.pooled_classes());
    let mut leases = leased.as_ref().ok().into_iter().flatten().copied();
    for pooled in fx.pooled_mut() {
        pooled.lease = leases.next();
    }
    leased?;
    pool.initialise(|target| {
        if !batched {
            return renderer.render_scene(target, CLEARED, &[], &[]);
        }
        renderer.record_scene(target, CLEARED, &[], &[]);
        Ok(())
    })
}

#[cfg(test)]
mod tests;
