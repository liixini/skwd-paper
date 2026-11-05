use super::{Group, LayerPose, ParticleGroup, PuppetGroup, SceneModel, layer_affine, vk};
use anyhow::Result;
use ash::vk::Handle;
use paper_scene::particles::image::{
    ImageSource, SampleGrid, SamplePose, SampleSource, SampleState,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

pub(super) struct ImageSample {
    pub layer: usize,
    pub pose: LayerPose,
    slot: usize,
    mask: Option<usize>,
    source: ImageSource,
    sampler: Option<vk::Sampling>,
    fresh: Vec<paper_scene::particles::image::SampleCell>,
    capture: Capture,
    posed: bool,
    prepared_frame: Option<u32>,
}

pub(super) struct ImageQueries<'a> {
    pub samples: &'a mut [ImageSample],
    pub renderer: &'a mut vk::Renderer,
    pub textures: &'a [vk::SceneTexture],
    pub puppets: &'a [PuppetGroup],
    pub batched: bool,
    error: Option<anyhow::Error>,
}

impl<'a> ImageQueries<'a> {
    pub(super) fn new(
        samples: &'a mut [ImageSample],
        renderer: &'a mut vk::Renderer,
        textures: &'a [vk::SceneTexture],
        puppets: &'a [PuppetGroup],
        batched: bool,
    ) -> Self {
        Self { samples, renderer, textures, puppets, batched, error: None }
    }

    pub(super) fn query(&mut self, source: &ImageSource) {
        if self.error.is_some() {
            return;
        }
        let Some(sample) = self.samples.iter_mut().find(|sample| sample.matches(source)) else {
            return;
        };
        if let Err(err) = sample.query(self.renderer, self.textures, self.puppets, self.batched) {
            self.error = Some(err);
        }
    }

    pub(super) fn finish(self) -> Result<()> {
        self.error.map_or(Ok(()), Err)
    }
}

impl ImageSample {
    pub(super) fn matches(&self, source: &ImageSource) -> bool {
        Rc::ptr_eq(&self.source, source)
    }

    fn harvest(&mut self, puppets: &[PuppetGroup]) -> Result<()> {
        let Some(sampler) = &mut self.sampler else {
            return Ok(());
        };
        if !sampler.read(&mut self.fresh)? {
            return Ok(());
        }
        if self.capture.finish() {
            if let Some(puppet) = super::layer_puppet(puppets, self.layer) {
                paper_scene::puppet::assign_regions(&puppet.puppet.mesh, &mut self.fresh);
            }
            replace_cells(&mut self.source.borrow_mut(), &mut self.fresh, true);
        } else {
            replace_cells(&mut self.source.borrow_mut(), &mut self.fresh, false);
        }
        Ok(())
    }

    fn record(
        &mut self,
        renderer: &mut vk::Renderer,
        textures: &[vk::SceneTexture],
    ) -> Result<bool> {
        let Some(grid) = SampleGrid::new(self.capture.size) else {
            return Ok(false);
        };
        if self.sampler.is_none() {
            self.sampler = Some(vk::Sampling::new(renderer, grid)?);
        }
        let texture = &textures[self.slot];
        let mask = self.mask.map(|slot| {
            let texture = &textures[slot];
            (texture.view, texture.sampler)
        });
        self.sampler.as_mut().expect("image sampler").record(
            renderer,
            (texture.view, texture.sampler),
            mask,
        )?;
        let mut source = self.source.borrow_mut();
        source.state = SampleState::Pending;
        source.requested = false;
        self.capture.start();
        Ok(true)
    }

    pub(super) fn query(
        &mut self,
        renderer: &mut vk::Renderer,
        textures: &[vk::SceneTexture],
        puppets: &[PuppetGroup],
        batched: bool,
    ) -> Result<()> {
        let state = self.source.borrow().state;
        match state {
            SampleState::Ready | SampleState::Resized => Ok(()),
            SampleState::Pending => self.harvest(puppets),
            SampleState::Unready => {
                if SampleGrid::new(self.capture.size).is_none() {
                    return Ok(());
                }
                if !batched {
                    renderer.begin_scene_batch()?;
                }
                self.record(renderer, textures)?;
                renderer.submit_scene_batch()?;
                self.sampler.as_mut().expect("image sampler").complete_batch();
                if batched {
                    renderer.begin_scene_batch()?;
                }
                Ok(())
            }
        }
    }
}

fn replace_cells(
    source: &mut SampleSource,
    fresh: &mut Vec<paper_scene::particles::image::SampleCell>,
    accepted: bool,
) {
    if accepted {
        source.cells = std::mem::take(fresh);
        source.state = SampleState::Ready;
        source.requested = false;
    } else {
        *fresh = Vec::new();
        source.state = SampleState::Unready;
    }
}

#[derive(Default)]
struct Capture {
    allocation: u64,
    allocation_generation: u64,
    content_generation: u64,
    size: [u32; 2],
    request: Option<u64>,
}

impl Capture {
    fn bind(&mut self, allocation: u64, size: [u32; 2]) -> bool {
        if self.allocation != allocation {
            self.allocation = allocation;
            self.allocation_generation = self.allocation_generation.wrapping_add(1);
        }
        if self.size == size {
            return false;
        }
        self.size = size;
        self.content_generation = self.content_generation.wrapping_add(1);
        true
    }

    fn start(&mut self) {
        self.request = Some(self.content_generation);
    }

    fn finish(&mut self) -> bool {
        self.request.take() == Some(self.content_generation)
    }
}

pub(super) fn bind_sources(
    model: &SceneModel,
    slots: &[usize],
    puppets: &[PuppetGroup],
    particles: &mut [ParticleGroup],
) -> Vec<ImageSample> {
    let mut samples: Vec<ImageSample> = Vec::new();
    let mut keys = BTreeMap::new();
    for particle in particles {
        let mut sources = Vec::new();
        for dependency in &particle.system.image_dependencies {
            let key = (dependency.source, dependency.mask.clone());
            let index = if let Some(&index) = keys.get(&key) {
                index
            } else {
                let Some(layer) = model
                    .layers
                    .iter()
                    .position(|layer| layer.id.parse::<u32>().ok() == Some(dependency.source))
                else {
                    continue;
                };
                let slot = super::flat_slot(puppets, layer, slots);
                let mask = model
                    .layers
                    .iter()
                    .position(|layer| {
                        layer.texture_name.as_deref() == Some(dependency.mask.as_str())
                    })
                    .map(|layer| slots[layer]);
                let index = samples.len();
                samples.push(ImageSample {
                    layer,
                    pose: LayerPose::from(&model.layers[layer]),
                    slot,
                    mask,
                    source: Rc::new(RefCell::new(SampleSource::default())),
                    sampler: None,
                    fresh: Vec::new(),
                    capture: Capture::default(),
                    posed: false,
                    prepared_frame: None,
                });
                keys.insert(key, index);
                index
            };
            sources.resize(sources.len().max(dependency.ordinal + 1), None);
            sources[dependency.ordinal] = Some(Rc::clone(&samples[index].source));
        }
        particle.sim.bind_images(sources);
    }
    samples
}

impl Group {
    pub(super) fn prepare_image_samples(&mut self, dt: f32) -> Result<()> {
        for sample in &mut self.image_samples {
            let texture = &self.textures[sample.slot];
            let size = [texture.extent.width, texture.extent.height];
            let resized = sample.capture.bind(texture.view.as_raw(), size);
            if resized {
                sample.source.borrow_mut().resize();
            }
            if resized && sample.sampler.is_some() {
                sample.harvest(&self.puppets)?;
            }
            if let Some(sampler) = &mut sample.sampler
                && resized
                && let Some(grid) = SampleGrid::new(size)
            {
                sampler.resize(&mut self.renderer, grid)?;
            }
            let bones = super::layer_puppet(&self.puppets, sample.layer)
                .map_or_else(Default::default, |puppet| puppet.pose.bones.clone());
            let pose = SamplePose { transform: layer_affine(sample.pose, self.canvas), bones };
            let mut source = sample.source.borrow_mut();
            if sample.prepared_frame != Some(self.particle_frame) {
                source.previous = if sample.posed { source.current.clone() } else { pose.clone() };
                sample.prepared_frame = Some(self.particle_frame);
            }
            source.current = pose;
            source.dt = dt;
            sample.posed = true;
        }
        Ok(())
    }

    pub(super) fn record_image_samples(&mut self, batched: bool) -> Result<()> {
        if self.image_samples.is_empty() {
            return Ok(());
        }
        if !batched {
            self.renderer.begin_scene_batch()?;
        }
        for sample in &mut self.image_samples {
            let source = sample.source.borrow();
            if source.state != SampleState::Unready || !source.requested {
                continue;
            }
            drop(source);
            sample.record(&mut self.renderer, &self.textures)?;
        }
        if !batched {
            self.renderer.submit_scene_batch()?;
            self.complete_image_samples();
        }
        Ok(())
    }

    pub(super) fn complete_image_samples(&mut self) {
        for sampler in self.image_samples.iter_mut().filter_map(|sample| sample.sampler.as_mut()) {
            sampler.complete_batch();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Capture, SampleSource, SampleState, replace_cells};
    use paper_scene::particles::image::SampleCell;

    #[test]
    fn replacement_releases_staging() {
        let mut source = SampleSource {
            cells: Vec::with_capacity(1024),
            state: SampleState::Pending,
            ..SampleSource::default()
        };
        let mut fresh = vec![SampleCell { color: [1; 3], bone: 255, position: [0; 2] }];
        replace_cells(&mut source, &mut fresh, true);
        assert_eq!(source.cells.len(), 1);
        assert_eq!(fresh.capacity(), 0);
        assert_eq!(source.state, SampleState::Ready);
        source.state = SampleState::Pending;
        let mut rejected = Vec::with_capacity(1024);
        replace_cells(&mut source, &mut rejected, false);
        assert_eq!(source.cells.len(), 1);
        assert_eq!(rejected.capacity(), 0);
        assert_eq!(source.state, SampleState::Unready);
    }

    #[test]
    fn replacement_preserves_capture() {
        let mut capture = Capture::default();
        assert!(capture.bind(1, [64, 64]));
        capture.start();
        assert!(!capture.bind(2, [64, 64]));
        assert!(capture.finish());
        assert_eq!(capture.allocation_generation, 2);
        assert_eq!(capture.content_generation, 1);
    }

    #[test]
    fn refresh_uses_replacement() {
        let mut capture = Capture::default();
        capture.bind(1, [64, 64]);
        capture.start();
        assert!(capture.finish());
        capture.bind(2, [64, 64]);
        capture.start();
        assert_eq!(capture.allocation, 2);
        assert!(capture.finish());
    }

    #[test]
    fn resize_rejects_capture() {
        let mut capture = Capture::default();
        capture.bind(1, [64, 64]);
        capture.start();
        assert!(capture.bind(2, [128, 64]));
        assert!(!capture.finish());
        capture.start();
        assert!(capture.finish());
    }
}
