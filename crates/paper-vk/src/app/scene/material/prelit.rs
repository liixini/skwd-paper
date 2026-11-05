use super::{BasePass, BaseSource, Build, Gpu, HALF, Mat4, channels, vk};
use anyhow::Result;

pub(super) struct Prelit {
    gpu: Gpu,
    pub output: usize,
    pub pass: BasePass,
    pub projection: Mat4,
    pub source_uv: [f32; 2],
    quad: vk::SceneQuad,
    sampled: Option<Vec<u8>>,
    stale: bool,
}

impl Prelit {
    pub fn new(
        build: &mut Build<'_, '_>,
        mut authored: paper_scene::effects::material::Prelit,
        source: &BaseSource<'_>,
        mesh: vk::MeshGeometry,
        texture: &paper_scene::model::Texture,
    ) -> Result<Self> {
        let mut gpu = Gpu {
            device: build.renderer.device.clone(),
            pipelines: Vec::new(),
            mirror: None,
            vertices: None,
            mesh: None,
            target: None,
        };
        let format = build.renderer.scene_format();
        gpu.target = Some(build.renderer.create_scene_target_fmt(
            texture.img_width,
            texture.img_height,
            false,
            format,
        )?);
        let pass = build.base_pass(&mut authored.pass, source, &mut gpu)?;
        let effect = vk::SceneEffect::mesh(&gpu.pipelines[0], mesh, None)?;
        let target = gpu.target.as_ref().expect("prelit target");
        let draw = channels::view(build, target, Some(effect))?;
        let output = channels::view(build, target, None)?;
        let size = [texture.img_width as f32, texture.img_height as f32];
        let [width, height, image_width, image_height] = source.resolution;
        let quad = vk::SceneQuad {
            rect: [size[0] * HALF, size[1] * HALF, size[0], size[1]],
            uv: [0.0, 0.0, 1.0, 1.0],
            uv_axes: None,
            uv_padding: false,
            tint: [1.0; super::CHANNELS],
            angle: 0.0,
            projection: None,
            order_bias: 0,
            texture: draw,
            blend: vk::SceneBlend::Copy,
        };
        Ok(Self {
            gpu,
            output,
            pass,
            projection: authored.projection,
            source_uv: [image_width / width, image_height / height],
            quad,
            sampled: None,
            stale: false,
        })
    }

    pub fn bytes(&self) -> u64 {
        self.gpu.target.as_ref().map_or(0, |target| target.allocation_bytes)
    }

    pub fn pipeline(&self) -> &vk::EffectPipeline {
        &self.gpu.pipelines[0]
    }

    pub fn uses_history(&self) -> bool {
        self.pass.inputs.iter().any(|input| matches!(input, super::Input::History))
    }

    pub fn sample(&mut self, bytes: Vec<u8>, source_changes: bool) -> Option<Vec<u8>> {
        if !stale(self.sampled.as_deref(), &bytes, source_changes, &self.pass.inputs) {
            return None;
        }
        self.stale = true;
        self.sampled = Some(bytes.clone());
        Some(bytes)
    }

    pub fn render(
        &mut self,
        renderer: &mut vk::Renderer,
        textures: &[vk::SceneTexture],
        batched: bool,
    ) -> Result<()> {
        if !self.stale {
            return Ok(());
        }
        let target = self.gpu.target.as_ref().expect("prelit target");
        let canvas = [target.extent.width as f32, target.extent.height as f32];
        let clear = [0.0; super::CHANNELS];
        let quads = std::slice::from_ref(&self.quad);
        if batched {
            renderer.record_scene_with_canvas(target, canvas, clear, quads, textures, &[]);
        } else {
            renderer.render_scene_with_canvas(target, canvas, clear, quads, textures, &[])?;
        }
        self.stale = false;
        Ok(())
    }
}

pub(super) fn stale(
    sampled: Option<&[u8]>,
    bytes: &[u8],
    source_changes: bool,
    inputs: &[super::Input],
) -> bool {
    source_changes
        || sampled != Some(bytes)
        || inputs.iter().any(|input| !matches!(input, super::Input::Texture(_)))
}
