use super::MAX_ANIMATION_LAYERS;
use crate::keyframe::Control;
use crate::matrix::{Mat4, mat4_mul};
use serde::Serialize;
use std::collections::HashMap;

const MAX_END_EVENTS_PER_FRAME: usize = 1024;
const MATRIX_SIDE: usize = 4;
const DEFAULT_FADE_SECONDS: f32 = 0.5;
const HANDLE_SLOT_BITS: u32 = usize::BITS / 2;
const HANDLE_SLOT_MASK: usize = (1 << HANDLE_SLOT_BITS) - 1;

fn enabled() -> bool {
    true
}

fn default_blendtime() -> f32 {
    DEFAULT_FADE_SECONDS
}

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct Clip {
    pub slot: usize,
    pub object: String,
    pub name: String,
    pub animation: u32,
    pub fps: f32,
    pub length: f32,
    pub rate: f32,
}

#[derive(Clone, Copy, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    #[serde(default = "enabled")]
    pub blendin: bool,
    #[serde(default = "enabled")]
    pub blendout: bool,
    #[serde(default = "default_blendtime")]
    pub blendtime: f32,
    #[serde(default)]
    pub autosort: bool,
}

#[derive(Serialize)]
pub(crate) struct Descriptor<'a> {
    pub(crate) slot: usize,
    pub(crate) layer: usize,
    pub(crate) authored: Option<usize>,
    pub(crate) name: &'a str,
    pub(crate) fps: f64,
    pub(crate) frame_count: f32,
    pub(crate) duration: f32,
    pub(crate) clock: usize,
}

#[derive(Serialize)]
pub(crate) struct BoneDescriptor {
    name: String,
    parent: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sample {
    Frame(f32),
    Rest,
    Retired,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub layer: crate::puppet::AnimationLayer,
    pub sample: Sample,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Playing,
    Paused,
    Retired,
}

#[derive(Clone, Copy, PartialEq)]
enum Cycle {
    Once,
    Repeat,
    Mirror,
}

#[derive(Clone, Copy, PartialEq)]
enum Owner {
    Authored,
    Script,
}

struct Layer {
    clip: Clip,
    authored: Option<usize>,
    blend: f32,
    additive: bool,
    seconds: f32,
    phase: Phase,
    visible: bool,
    cycle: Cycle,
    transition: Transition,
    generation: usize,
    owner: Owner,
    first_forward: bool,
}

pub(crate) struct PendingEvent {
    pub(crate) slot: usize,
    pub(crate) name: String,
    pub(crate) frame: u32,
}

struct Rig {
    object: String,
    mesh: crate::puppet::Mesh,
    world: Vec<Mat4>,
    locals: Vec<Mat4>,
    descendants: Vec<bool>,
    spring: Option<Box<super::spring::Simulation>>,
    samples: Vec<(crate::puppet::AnimationLayer, f32)>,
    resting: Vec<bool>,
    resolved: bool,
    pending: bool,
    weights: Option<Vec<f32>>,
    edited: bool,
    revision: u64,
}

impl Rig {
    fn states(&self) -> impl Iterator<Item = State> + '_ {
        self.samples.iter().zip(&self.resting).map(|(&(layer, seconds), &resting)| State {
            layer,
            sample: if resting { Sample::Rest } else { Sample::Frame(seconds) },
        })
    }

    fn invalidate_weights(&mut self) {
        if self.weights.take().is_some() {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn reset_bones(&mut self) {
        self.pending = false;
        self.resolved = false;
        self.world.clear();
        self.locals.clear();
        if self.edited {
            self.edited = false;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn resolve(
        &mut self,
        samples: &[(crate::puppet::AnimationLayer, f32)],
        snapshot: bool,
    ) -> Option<&[Mat4]> {
        if !self.resolved {
            let samples = if snapshot { &self.samples } else { samples };
            if snapshot && samples.iter().all(|(layer, _)| layer.blend == 0.0) {
                let layers = samples
                    .iter()
                    .zip(&self.resting)
                    .filter_map(|((layer, _), resting)| (!resting).then_some(layer));
                crate::puppet::pose::rest_world(
                    &self.mesh,
                    &layers,
                    &mut self.world,
                    Some(&mut self.locals),
                )?;
            } else {
                crate::puppet::sampled_world(
                    &self.mesh,
                    samples,
                    &mut self.world,
                    Some(&mut self.locals),
                )
                .ok()?;
            }
            self.descendants.resize(self.world.len(), false);
            self.resolved = true;
        }
        Some(&self.world)
    }

    fn local(&self, index: usize) -> Option<Mat4> {
        self.locals.get(index).copied()
    }

    fn set(&mut self, index: usize, matrix: Mat4, local: bool) -> bool {
        if !matrix.iter().all(|value| value.is_finite())
            || (0..MATRIX_SIDE - 1)
                .any(|column| matrix[column * MATRIX_SIDE + MATRIX_SIDE - 1] != 0.0)
            || matrix[MATRIX_SIDE * MATRIX_SIDE - 1] != 1.0
        {
            return false;
        }
        if !self.resolved || index >= self.world.len() {
            return false;
        }
        if local {
            self.locals[index] = matrix;
            self.descendants.fill(false);
            self.descendants[index] = true;
            for child in index..self.world.len() {
                let parent = self.mesh.bones[child].parent;
                if child != index
                    && !parent.is_some_and(|parent| self.descendants[usize::from(parent)])
                {
                    continue;
                }
                let matrix = self.locals[child];
                self.world[child] = parent
                    .map_or(matrix, |parent| mat4_mul(&self.world[usize::from(parent)], &matrix));
                self.descendants[child] = true;
            }
        } else {
            self.world[index] = matrix;
        }
        self.edited = true;
        self.revision = self.revision.wrapping_add(1);
        true
    }
}

impl Layer {
    fn sample(&self) -> (crate::puppet::AnimationLayer, f32) {
        let state = self.state();
        let seconds = match state.sample {
            Sample::Frame(seconds) => seconds,
            Sample::Rest | Sample::Retired => 0.0,
        };
        (state.layer, seconds)
    }

    fn state(&self) -> State {
        State {
            layer: crate::puppet::AnimationLayer {
                visible: self.visible && self.phase != Phase::Retired,
                id: self.clip.animation,
                rate: 1.0,
                blend: if self.visible { self.blend * self.fade() } else { 0.0 },
                additive: self.additive,
            },
            sample: if self.phase == Phase::Retired {
                Sample::Retired
            } else if self.visible {
                Sample::Frame(self.seconds)
            } else {
                Sample::Rest
            },
        }
    }

    fn fps(&self) -> f32 {
        self.clip.fps.recip().recip()
    }

    fn duration(&self) -> f32 {
        self.clip.length / self.clip.fps
    }

    fn cycle_duration(&self) -> f32 {
        if self.cycle == Cycle::Mirror { self.duration() * 2.0 } else { self.duration() }
    }

    fn fade(&self) -> f32 {
        let time = self.transition.blendtime;
        if time == 0.0 {
            return 1.0;
        }
        let start =
            if self.transition.blendin { (self.seconds / time).clamp(0.0, 1.0) } else { 1.0 };
        let end = if self.transition.blendout {
            ((self.cycle_duration() - self.seconds) / time).clamp(0.0, 1.0)
        } else {
            1.0
        };
        start.min(end)
    }

    fn handle(&self, slot: usize) -> usize {
        (self.generation << HANDLE_SLOT_BITS) | slot
    }
}

fn collect_events(
    pending: &mut Vec<PendingEvent>,
    animation: &crate::puppet::Animation,
    layer: &Layer,
    previous: f32,
) {
    let next = layer.seconds;
    let duration = layer.duration();
    if !duration.is_finite() || duration <= 0.0 || !next.is_finite() {
        return;
    }
    let previous = f64::from(previous);
    let next = f64::from(next);
    let duration = f64::from(duration);
    let period = f64::from(layer.cycle_duration());
    if !period.is_finite() || period <= 0.0 {
        return;
    }
    let mut crossings = Vec::with_capacity(animation.events.len() * 2);
    for (index, event) in animation.events.iter().enumerate() {
        let at = (f64::from(event.payload.frame) / f64::from(animation.fps)).min(duration);
        crossings.push((at, index));
        if layer.cycle == Cycle::Mirror && at > 0.0 && at < duration {
            crossings.push((period - at, index));
        }
    }
    if crossings.is_empty() {
        return;
    }
    crossings.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));
    let mut cycle = if layer.cycle == Cycle::Once { 0.0 } else { (previous / period).floor() };
    loop {
        let base = cycle * period;
        if base > next || pending.len() >= MAX_END_EVENTS_PER_FRAME {
            break;
        }
        for &(phase, index) in &crossings {
            let crossing = base + phase;
            if crossing > next {
                break;
            }
            if crossing > previous || (layer.first_forward && previous == 0.0 && crossing == 0.0) {
                let event = &animation.events[index];
                pending.push(PendingEvent {
                    slot: layer.clip.slot,
                    name: event.payload.name.clone(),
                    frame: event.payload.frame,
                });
                if pending.len() >= MAX_END_EVENTS_PER_FRAME {
                    return;
                }
            }
        }
        if layer.cycle == Cycle::Once {
            break;
        }
        let next_cycle = cycle + 1.0;
        if next_cycle == cycle {
            break;
        }
        cycle = next_cycle;
    }
}

#[derive(Default)]
pub struct Playback {
    layers: Vec<Layer>,
    order: Vec<usize>,
    available: Vec<Clip>,
    bones: Vec<(usize, Vec<BoneDescriptor>)>,
    rigs: HashMap<usize, Rig>,
    retired_slots: std::collections::HashSet<usize>,
    dirty: bool,
    bone_snapshots: bool,
    ended: Vec<usize>,
    events: Vec<PendingEvent>,
}

impl Playback {
    fn invalidate_rig(&mut self, slot: usize) {
        if let Some(rig) = self.rigs.get_mut(&slot) {
            if self.bone_snapshots {
                rig.pending = true;
            } else {
                rig.reset_bones();
            }
            rig.invalidate_weights();
        }
    }

    pub fn new(clips: Vec<Clip>) -> Self {
        let order = (0..clips.len()).collect();
        let mut positions = HashMap::new();
        Self {
            layers: clips
                .into_iter()
                .map(|clip| Layer {
                    authored: Some({
                        let position = positions.entry(clip.slot).or_insert(0);
                        *position += 1;
                        *position - 1
                    }),
                    clip,
                    blend: 1.0,
                    additive: false,
                    seconds: 0.0,
                    phase: Phase::Playing,
                    visible: true,
                    cycle: Cycle::Repeat,
                    transition: Transition::default(),
                    generation: 0,
                    owner: Owner::Authored,
                    first_forward: true,
                })
                .collect(),
            order,
            available: Vec::new(),
            bones: Vec::new(),
            rigs: HashMap::new(),
            retired_slots: std::collections::HashSet::new(),
            dirty: false,
            bone_snapshots: false,
            ended: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn with_catalog(clips: Vec<Clip>, available: Vec<Clip>) -> Self {
        let mut playback = Self::new(clips);
        playback.available = available;
        playback
    }

    pub fn authored(
        object: &str,
        mesh: &crate::puppet::Mesh,
        layers: &[crate::puppet::AnimationLayer],
    ) -> anyhow::Result<Self> {
        let clips = layers
            .iter()
            .map(|layer| {
                let animation = mesh
                    .animations
                    .iter()
                    .find(|animation| animation.id == layer.id)
                    .ok_or_else(|| anyhow::anyhow!("puppet authored clip missing"))?;
                Ok(Clip {
                    slot: 0,
                    object: object.into(),
                    name: animation.name.clone(),
                    animation: layer.id,
                    fps: animation.fps,
                    length: animation.duration as f32,
                    rate: layer.rate,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let mut playback = Self::new(clips);
        for (index, layer) in layers.iter().enumerate() {
            playback.configure(index, layer.blend, layer.additive, layer.visible);
        }
        playback.set_cycles(0, mesh);
        Ok(playback)
    }

    pub(crate) fn install_bone_metadata(&mut self, slot: usize, bones: Vec<(String, Option<u16>)>) {
        self.bones.push((
            slot,
            bones.into_iter().map(|(name, parent)| BoneDescriptor { name, parent }).collect(),
        ));
    }

    pub(crate) fn bone_descriptors(&self) -> &[(usize, Vec<BoneDescriptor>)] {
        &self.bones
    }

    pub fn install_rig(&mut self, slot: usize, object: String, mut mesh: crate::puppet::Mesh) {
        self.set_cycles(slot, &mesh);
        self.install_bone_metadata(
            slot,
            mesh.bones.iter().map(|bone| (bone.name.clone(), bone.parent)).collect(),
        );
        crate::puppet::strip_geometry(&mut mesh);
        let spring = super::spring::Simulation::of(&mesh);
        self.rigs.insert(
            slot,
            Rig {
                object,
                mesh,
                world: Vec::new(),
                locals: Vec::new(),
                descendants: Vec::new(),
                spring,
                samples: Vec::new(),
                resting: Vec::new(),
                resolved: false,
                pending: false,
                weights: None,
                edited: false,
                revision: 0,
            },
        );
    }

    fn set_cycles(&mut self, slot: usize, mesh: &crate::puppet::Mesh) {
        for layer in &mut self.layers {
            if layer.clip.slot == slot && layer.cycle != Cycle::Once {
                layer.cycle = if mesh
                    .animations
                    .iter()
                    .any(|animation| animation.id == layer.clip.animation && animation.mirror)
                {
                    Cycle::Mirror
                } else {
                    Cycle::Repeat
                };
            }
        }
    }

    fn samples_for_slot(&self, slot: usize) -> Vec<(crate::puppet::AnimationLayer, f32)> {
        self.order
            .iter()
            .map(|&index| &self.layers[index])
            .filter(|layer| {
                layer.clip.slot == slot && layer.visible && layer.phase != Phase::Retired
            })
            .map(Layer::sample)
            .collect()
    }

    pub(crate) fn bone_matrix(&mut self, slot: usize, index: usize, local: bool) -> Option<Mat4> {
        let samples = if self.bone_snapshots { Vec::new() } else { self.samples_for_slot(slot) };
        let rig = self.rigs.get_mut(&slot)?;
        rig.resolve(&samples, self.bone_snapshots)?;
        if local { rig.local(index) } else { rig.world.get(index).copied() }
    }

    pub(crate) fn set_bone_matrix(
        &mut self,
        slot: usize,
        index: usize,
        matrix: Mat4,
        local: bool,
    ) -> bool {
        let samples = if self.bone_snapshots { Vec::new() } else { self.samples_for_slot(slot) };
        let Some(rig) = self.rigs.get_mut(&slot) else {
            return false;
        };
        if rig.resolve(&samples, self.bone_snapshots).is_none() {
            return false;
        }
        rig.set(index, matrix, local)
    }

    pub(crate) fn has_springs(&self, slot: usize) -> bool {
        !self.retired_slots.contains(&slot)
            && self.rigs.get(&slot).is_some_and(|rig| rig.spring.is_some())
    }

    pub(crate) fn step_springs(&mut self, slot: usize, owner: Mat4, dt: f32) -> bool {
        if !self.has_springs(slot) || !owner.iter().all(|value| value.is_finite()) {
            return false;
        }
        if !super::spring::owner_supported(&owner) {
            return false;
        }
        let Some(rig) = self.rigs.get_mut(&slot) else { return false };
        if rig.resolve(&[], true).is_none() {
            return false;
        }
        let Some(spring) = &mut rig.spring else { return false };
        if !spring.step(
            &rig.mesh.springs,
            &rig.mesh.bones,
            &mut rig.world,
            &super::spring::Frame { dt, owner: &owner, scale: super::spring::owner_scale(&owner) },
        ) {
            rig.reset_bones();
            return false;
        }
        rig.edited = true;
        rig.revision = rig.revision.wrapping_add(1);
        true
    }

    pub(crate) fn record_springs(&mut self, slot: usize, index: usize, local: bool, owner: &Mat4) {
        let Some(rig) = self.rigs.get_mut(&slot) else { return };
        let Some(spring) = &mut rig.spring else { return };
        for definition in &rig.mesh.springs {
            let bone = usize::from(definition.bone);
            if bone == index || (local && rig.descendants[bone]) {
                let world = crate::matrix::mat4_mul(owner, &rig.world[bone]);
                spring.record(&rig.mesh.springs, definition.bone, &world);
            }
        }
    }

    pub fn bone_palette(&self, object: &str) -> Option<(u64, Vec<Mat4>)> {
        let rig = self.rig(object)?;
        if !rig.edited {
            return None;
        }
        let palette = crate::puppet::palette_world(&rig.mesh, &rig.world).ok()?;
        Some((rig.revision, palette))
    }

    pub fn bone_world(&self, object: &str) -> Option<(u64, Vec<Mat4>)> {
        let rig = self.rig(object)?;
        if !rig.edited {
            return None;
        }
        Some((rig.revision, rig.world.clone()))
    }

    pub(crate) fn morph_descriptors(&self) -> Vec<(usize, Vec<&str>)> {
        self.rigs
            .iter()
            .filter_map(|(slot, rig)| {
                let morph = rig.mesh.morph.as_ref()?;
                Some((*slot, morph.shapes.iter().map(|shape| shape.name.as_str()).collect()))
            })
            .collect()
    }

    pub fn morph_weight(&mut self, slot: usize, index: usize, value: Option<f32>) -> Option<f32> {
        if value.is_some_and(|value| !value.is_finite()) {
            return None;
        }
        let samples = self.samples_for_slot(slot);
        let rig = self.rigs.get_mut(&slot)?;
        if rig.weights.is_none() {
            rig.weights = crate::puppet::morph::weights(&rig.mesh, &samples).ok()?;
        }
        let weight = rig.weights.as_mut()?.get_mut(index)?;
        if let Some(value) = value.filter(|value| *value != *weight) {
            *weight = value;
            rig.revision = rig.revision.wrapping_add(1);
        }
        Some(*weight)
    }

    pub fn morph_weights(&self, object: &str) -> Option<Vec<f32>> {
        self.rig(object)?.weights.clone()
    }

    fn rig(&self, object: &str) -> Option<&Rig> {
        self.rigs.values().find(|rig| rig.object == object)
    }

    pub fn bone_revision(&self, object: &str) -> u64 {
        self.rig(object).map_or(0, |rig| rig.revision)
    }

    pub fn attachment_pose(
        &self,
        object: &str,
        name: &str,
    ) -> Option<crate::puppet::AttachmentPose> {
        let rig = self.rig(object)?;
        if !rig.edited {
            return None;
        }
        crate::puppet::attachment_pose_world(&rig.mesh, &rig.world, name)
    }

    pub(crate) fn create(
        &mut self,
        slot: usize,
        name: &str,
        single: bool,
        transition: Transition,
    ) -> Option<usize> {
        if self.order.len() >= MAX_ANIMATION_LAYERS || self.retired_slots.contains(&slot) {
            return None;
        }
        if !transition.blendtime.is_finite()
            || transition.blendtime < 0.0
            || ((transition.blendin || transition.blendout) && transition.blendtime == 0.0)
        {
            return None;
        }
        let clip =
            self.available.iter().find(|clip| clip.slot == slot && clip.name == name)?.clone();
        let reusable = self
            .layers
            .iter()
            .position(|layer| layer.phase == Phase::Retired && layer.owner == Owner::Script);
        let index = reusable.unwrap_or(self.layers.len());
        if index > HANDLE_SLOT_MASK {
            return None;
        }
        let generation =
            reusable.map_or(Some(0), |slot| self.layers[slot].generation.checked_add(1))?;
        if generation > HANDLE_SLOT_MASK {
            return None;
        }
        let mirror = self.rigs.get(&slot).is_some_and(|rig| {
            rig.mesh
                .animations
                .iter()
                .any(|animation| animation.id == clip.animation && animation.mirror)
        });
        let layer = Layer {
            clip,
            authored: None,
            blend: 1.0,
            additive: false,
            seconds: 0.0,
            phase: Phase::Playing,
            visible: true,
            cycle: if single {
                Cycle::Once
            } else if mirror {
                Cycle::Mirror
            } else {
                Cycle::Repeat
            },
            transition,
            generation,
            owner: Owner::Script,
            first_forward: true,
        };
        if reusable.is_some() {
            self.layers[index] = layer;
        } else {
            self.layers.push(layer);
        }
        let position = if transition.autosort {
            self.order
                .iter()
                .position(|&clock| {
                    self.layers[clock].clip.slot == slot && self.layers[clock].additive
                })
                .unwrap_or(self.order.len())
        } else {
            self.order.len()
        };
        self.order.insert(position, index);
        self.dirty = true;
        self.invalidate_rig(slot);
        Some(self.layers[index].handle(index))
    }

    pub(crate) fn destroy(&mut self, handle: usize) -> bool {
        let index = handle & HANDLE_SLOT_MASK;
        let Some(layer) = self.layers.get_mut(index) else {
            return false;
        };
        if layer.handle(index) != handle {
            return false;
        }
        if layer.phase == Phase::Retired {
            return false;
        }
        layer.phase = Phase::Retired;
        let slot = layer.clip.slot;
        self.order.retain(|&clock| clock != index);
        self.dirty = true;
        self.invalidate_rig(slot);
        true
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.layers.is_empty() && !self.rigs.keys().any(|slot| self.has_springs(*slot))
    }

    pub(crate) fn descriptors(&self) -> Vec<Descriptor<'_>> {
        self.order
            .iter()
            .map(|&clock| {
                let layer = &self.layers[clock];
                let index = self
                    .order
                    .iter()
                    .take_while(|&&previous| previous != clock)
                    .filter(|&&previous| self.layers[previous].clip.slot == layer.clip.slot)
                    .count();
                Descriptor {
                    slot: layer.clip.slot,
                    layer: index,
                    authored: layer.authored,
                    name: &layer.clip.name,
                    fps: f64::from(layer.fps()),
                    frame_count: layer.clip.length,
                    duration: layer.duration(),
                    clock: layer.handle(clock),
                }
            })
            .collect()
    }

    pub(crate) fn set_authored(&mut self, index: usize, authored: usize) {
        if let Some(layer) = self.layers.get_mut(index) {
            layer.authored = Some(authored);
        }
    }

    pub(crate) fn configure(&mut self, index: usize, blend: f32, additive: bool, visible: bool) {
        if let Some(layer) = self.layers.get_mut(index) {
            layer.blend = blend.clamp(0.0, 1.0);
            layer.additive = additive;
            layer.visible = visible;
        }
    }

    pub(crate) fn blend(&mut self, handle: usize, value: Option<f32>) -> Option<f32> {
        let index = handle & HANDLE_SLOT_MASK;
        let layer = self.layers.get_mut(index)?;
        if layer.phase == Phase::Retired || layer.handle(index) != handle {
            return None;
        }
        let invalidated = if let Some(value) = value.filter(|value| value.is_finite()) {
            let value = value.clamp(0.0, 1.0);
            let changed = layer.blend != value;
            self.dirty |= changed;
            layer.blend = value;
            changed.then_some(layer.clip.slot)
        } else {
            None
        };
        let blend = layer.blend;
        if let Some(slot) = invalidated {
            self.invalidate_rig(slot);
        }
        Some(blend)
    }

    pub fn states(&self, object: &str) -> Vec<State> {
        if self.bone_snapshots
            && let Some(rig) = self.rig(object)
        {
            return if self.owner_retired(object) { Vec::new() } else { rig.states().collect() };
        }
        self.order
            .iter()
            .filter_map(|&clock| {
                let layer = &self.layers[clock];
                (layer.clip.object == object && layer.phase != Phase::Retired)
                    .then(|| layer.state())
            })
            .collect()
    }

    pub(crate) fn bone_frame(&mut self, dt: f32) {
        self.bone_snapshots = false;
        self.reset_bones();
        self.advance(dt);
        for rig in self.rigs.values_mut() {
            rig.samples.clear();
            rig.resting.clear();
        }
        for &index in &self.order {
            let layer = &self.layers[index];
            if let Some(rig) = self.rigs.get_mut(&layer.clip.slot) {
                rig.samples.push(layer.sample());
                rig.resting.push(!layer.visible);
            }
        }
        self.bone_snapshots = true;
    }

    fn reset_bones(&mut self) {
        for rig in self.rigs.values_mut().filter(|rig| rig.edited || rig.pending) {
            rig.reset_bones();
        }
    }

    pub fn advance(&mut self, dt: f32) {
        if !dt.is_finite() || dt < 0.0 {
            return;
        }
        if dt > 0.0 {
            let moving: std::collections::HashSet<_> = self
                .layers
                .iter()
                .filter(|layer| layer.phase == Phase::Playing && layer.clip.rate != 0.0)
                .map(|layer| layer.clip.slot)
                .collect();
            for slot in moving {
                self.invalidate_rig(slot);
            }
        }
        self.dirty = false;
        self.ended.clear();
        self.events.clear();
        for (index, layer) in self.layers.iter_mut().enumerate() {
            if layer.phase != Phase::Playing {
                continue;
            }
            let previous = layer.seconds;
            layer.seconds += dt * layer.clip.rate;
            let animation = self.rigs.get(&layer.clip.slot).and_then(|rig| {
                rig.mesh.animations.iter().find(|animation| animation.id == layer.clip.animation)
            });
            let loop_duration = layer.cycle_duration();
            if layer.seconds > previous {
                if let Some(animation) = animation {
                    collect_events(&mut self.events, animation, layer, previous);
                }
                layer.first_forward = false;
            }
            if loop_duration > 0.0
                && loop_duration.is_finite()
                && layer.seconds.is_finite()
                && layer.seconds >= loop_duration
            {
                let count = (layer.seconds / loop_duration) as usize;
                let count = if layer.cycle == Cycle::Once { 1 } else { count };
                self.ended.extend(std::iter::repeat_n(
                    layer.handle(index),
                    count.min(MAX_END_EVENTS_PER_FRAME.saturating_sub(self.ended.len())),
                ));
                if layer.cycle == Cycle::Once {
                    layer.phase = Phase::Retired;
                    self.dirty = true;
                } else {
                    layer.seconds = libm::fmodf(layer.seconds, loop_duration);
                }
            }
        }
        self.order.retain(|&clock| self.layers[clock].phase != Phase::Retired);
    }

    pub(crate) fn take_ended(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.ended)
    }

    pub(crate) fn take_events(&mut self) -> Vec<PendingEvent> {
        std::mem::take(&mut self.events)
    }

    pub(crate) fn control(&mut self, handle: usize, op: Control, value: f32) -> Option<f32> {
        let index = handle & HANDLE_SLOT_MASK;
        let layer = self.layers.get_mut(index)?;
        if layer.phase == Phase::Retired || layer.handle(index) != handle {
            return None;
        }
        match op {
            Control::GetFrame => return Some(layer.seconds / layer.clip.fps.recip()),
            Control::GetRate => return Some(layer.clip.rate),
            Control::IsPlaying => return Some(f32::from(layer.phase == Phase::Playing)),
            Control::SetFrame if !(value * layer.clip.fps.recip()).is_finite() => return None,
            Control::SetFrame | Control::SetRate if !value.is_finite() || value < 0.0 => {
                return None;
            }
            _ => {}
        }
        match op {
            Control::Play => layer.phase = Phase::Playing,
            Control::Pause => layer.phase = Phase::Paused,
            Control::Stop => {
                layer.seconds = 0.0;
                layer.phase = Phase::Paused;
            }
            Control::SetFrame => layer.seconds = value * layer.clip.fps.recip(),
            Control::SetRate => layer.clip.rate = value,
            Control::GetFrame | Control::GetRate | Control::IsPlaying => unreachable!(),
        }
        self.dirty = true;
        let frame = layer.seconds / layer.clip.fps.recip();
        let slot = layer.clip.slot;
        self.invalidate_rig(slot);
        Some(frame)
    }

    pub(crate) fn visibility(&mut self, handle: usize, value: Option<bool>) -> Option<bool> {
        let index = handle & HANDLE_SLOT_MASK;
        let layer = self.layers.get_mut(index)?;
        if layer.phase == Phase::Retired || layer.handle(index) != handle {
            return None;
        }
        let invalidated = if let Some(value) = value {
            let changed = layer.visible != value;
            self.dirty |= changed;
            layer.visible = value;
            changed.then_some(layer.clip.slot)
        } else {
            None
        };
        let visible = layer.visible;
        if let Some(slot) = invalidated {
            self.invalidate_rig(slot);
        }
        Some(visible)
    }

    pub fn sample(&self, object: &str, animation: u32) -> Option<Sample> {
        if self.bone_snapshots
            && !self.owner_retired(object)
            && let Some(rig) = self.rig(object)
            && let Some(state) = rig.states().find(|state| state.layer.id == animation)
        {
            return Some(state.sample);
        }
        let layer = self
            .layers
            .iter()
            .find(|layer| layer.clip.object == object && layer.clip.animation == animation)?;
        Some(if layer.phase == Phase::Retired {
            Sample::Retired
        } else if layer.visible {
            Sample::Frame(layer.seconds)
        } else {
            Sample::Rest
        })
    }

    pub(crate) fn animated(&self) -> bool {
        self.rigs.keys().any(|slot| self.has_springs(*slot))
            || self.dirty
            || self
                .layers
                .iter()
                .any(|layer| layer.phase == Phase::Playing && layer.clip.rate != 0.0)
    }

    pub(crate) fn retire(&mut self, slot: usize) {
        self.retired_slots.insert(slot);
        for layer in &mut self.layers {
            if layer.clip.slot == slot {
                layer.phase = Phase::Retired;
            }
        }
        self.order.retain(|&index| self.layers[index].clip.slot != slot);
        self.invalidate_rig(slot);
    }

    pub fn owner_retired(&self, object: &str) -> bool {
        self.rigs
            .iter()
            .any(|(slot, rig)| rig.object == object && self.retired_slots.contains(slot))
            || self.layers.iter().any(|layer| {
                layer.clip.object == object && self.retired_slots.contains(&layer.clip.slot)
            })
    }
}
