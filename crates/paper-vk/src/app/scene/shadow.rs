use super::{mesh, vk};
use anyhow::{Result, ensure};
use ash::vk as raw;
use paper_scene::matrix::Mat4;
use paper_scene::model::lighting::{Light, ShadowLayout, ShadowView, SunView};

const ALPHA: usize = 3;
const BUDGET_ERROR: &str = "shadow atlas exceeds effect budget";

pub(super) struct Shadows {
    atlas: vk::ShadowAtlas,
    layout: ShadowLayout,
    initialized: bool,
    views: Vec<(raw::Rect2D, Mat4)>,
    draws: Vec<vk::ShadowDraw>,
    recorded: Vec<vk::ShadowDraw>,
    #[cfg(test)]
    pub renders: usize,
}

impl Shadows {
    pub fn new(renderer: &vk::Renderer, lights: &[Light], mutable: bool) -> Result<Option<Self>> {
        let layout = match ShadowLayout::new(lights) {
            Some(layout) => layout,
            None if mutable => ShadowLayout::default(),
            None => return Ok(None),
        };
        let [width, height] = layout.extent();
        let atlas = vk::ShadowAtlas::new(renderer, raw::Extent2D { width, height })?;
        Ok(Some(Self {
            atlas,
            layout,
            initialized: false,
            views: Vec::new(),
            draws: Vec::new(),
            recorded: Vec::new(),
            #[cfg(test)]
            renders: 0,
        }))
    }

    pub fn invalidate(&mut self) {
        self.initialized = false;
    }

    pub fn render_pass(&self) -> raw::RenderPass {
        self.atlas.render_pass()
    }

    pub fn sample(&self) -> vk::ShadowSample {
        self.atlas.samples()
    }

    pub fn extent(&self) -> raw::Extent2D {
        let [width, height] = self.layout.extent();
        raw::Extent2D { width, height }
    }

    pub fn allocation_bytes(&self) -> u64 {
        self.atlas.allocation_bytes()
    }

    pub fn dimensions(&self) -> [f32; 4] {
        self.layout.dimensions()
    }

    pub fn parameters(&self, light: &Light) -> [f32; 4] {
        self.layout.parameters(light)
    }

    fn collect<'a>(
        &mut self,
        lights: &[Light],
        sun: &SunView,
        meshes: impl Iterator<Item = &'a mesh::MeshActor>,
        quads: &[vk::SceneQuad],
    ) -> bool {
        self.views.clear();
        for light in lights.iter().filter(|light| light.casting()) {
            for (face, projection) in light.shadow_views(ShadowView::Caster, sun).enumerate() {
                let Some([x, y, width, height]) = self.layout.tile(light, face) else { continue };
                let rect = raw::Rect2D {
                    offset: raw::Offset2D { x: x as i32, y: y as i32 },
                    extent: raw::Extent2D { width, height },
                };
                self.views.push((rect, projection));
            }
        }
        self.draws.clear();
        for actor in meshes.filter(|actor| actor.cast_shadow) {
            if quads.iter().any(|quad| quad.texture == actor.texture && quad.tint[ALPHA] > 0.0) {
                self.draws.extend(
                    self.views
                        .iter()
                        .map(|(rect, projection)| actor.shadow_draw(projection, *rect)),
                );
            }
        }
        !self.initialized || self.draws != self.recorded
    }

    fn record(&mut self, renderer: &vk::Renderer) -> Result<()> {
        self.atlas.record(renderer, &self.draws)?;
        std::mem::swap(&mut self.draws, &mut self.recorded);
        self.initialized = true;
        #[cfg(test)]
        {
            self.renders += 1;
        }
        Ok(())
    }
}

impl super::Group {
    pub(super) fn refresh_shadows(&mut self) -> Result<()> {
        if self.shadows.as_ref().is_none_or(|shadows| shadows.layout.matches(&self.lights)) {
            return Ok(());
        }
        self.renderer.wait_render()?;
        let candidate = Shadows::new(&self.renderer, &self.lights, true)?.unwrap();
        let bytes = self
            .effect_target_allocation_bytes()
            .saturating_sub(self.shadows.as_ref().map_or(0, Shadows::allocation_bytes))
            .saturating_add(candidate.allocation_bytes());
        ensure!(bytes <= self.effect_budget, BUDGET_ERROR);
        for actor in self
            .meshes
            .iter()
            .chain(self.scripts.iter().flat_map(super::script::Scripts::shadow_meshes))
        {
            actor.bind_shadow(&self.renderer, candidate.sample(), self.reflection.as_ref());
        }
        self.shadows = Some(candidate);
        Ok(())
    }

    pub(super) fn render_shadows(&mut self) -> Result<()> {
        let Some(shadows) = &mut self.shadows else { return Ok(()) };
        let meshes = self
            .meshes
            .iter()
            .chain(self.scripts.iter().flat_map(super::script::Scripts::shadow_meshes));
        if !shadows.collect(&self.lights, &self.sun_view, meshes, &self.quads) {
            return Ok(());
        }
        self.renderer.begin_scene_batch()?;
        shadows.record(&self.renderer)?;
        self.renderer.submit_scene_batch()
    }
}
