mod blur;

use super::{Mat4, vk};
use anyhow::{Context, Result, ensure};
use paper_scene::model::lighting::Light;
use paper_scene::shader::{self, Stage, Uniform};
use std::collections::BTreeMap;

const SHADOW_SOURCE_ERROR: &str = "shadow light activation requires scene reload";
const COOKIE_SOURCE_ERROR: &str = "light cookie activation requires scene reload";
const CAPACITY_ERROR: &str = "volumetric light capacity exceeded";

const LABEL: &str = "volumetric lights";
const CAPACITY: usize = paper_scene::model::lighting::MAX_LIGHTS;
const COMPONENTS: usize = 4;
const DEPTH: &str = "VOLUME_DEPTH";
const COOKIE: &str = "VOLUME_COOKIE";
const COOKIE_TRANSFORM: &str = "g_VolumeCookie";
const SPOT_COOKIE: &str = "g_LSpot_Cookie";
const SHADOW: &str = "VOLUME_SHADOW";
const SHADOW_SAMPLER: &str = "g_Texture1";
const SHADOW_PARAMETERS: &str = "g_VolumeShadow";
const SHADOW_SIZE: &str = "g_ShadowSize";
const LIGHT_COUNT: &str = "VOLUME_LIGHTS";
const FOG: &str = "g_VolumeFog";
const FOG_DISTANCE: &str = "g_VolumeFogDistance";
const FOG_HEIGHT: &str = "g_VolumeFogHeight";
const EYE: &str = "g_VolumeEye";
const PROJECTION: &str = "g_InverseProjection";
const RESOLUTION: &str = "g_Resolution";
const ORIGINS: &str = "g_VolumeOrigin";
const COLORS: &str = "g_VolumeColor";
const DIRECTIONS: &str = "g_VolumeDirection";
const PARAMETERS: &str = "g_VolumeParameters";
const RIGHT: &str = "g_VolumeRight";
const UP: &str = "g_VolumeUp";
const VERTEX: &str = "attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}";
const FRAGMENT: &str = include_str!("volume.frag");

#[derive(Clone, Copy)]
pub(super) struct Sources<'a> {
    pub lights: &'a [Light],
    pub fog: paper_scene::model::fog::Volume,
    pub eye: [f32; 3],
    pub shadows: Option<&'a super::shadow::Shadows>,
    pub cookies: Option<(
        &'a paper_scene::model::lighting::CookieLayout,
        (ash::vk::ImageView, ash::vk::Sampler),
    )>,
}

pub(super) struct Volumes {
    blur: blur::Blur,
    pipeline: vk::EffectPipeline,
    quad: vk::QuadBuffer,
    depth: Option<vk::DepthSample>,
    uniforms: Vec<Uniform>,
    shadow: bool,
    cookie: bool,
}

impl Volumes {
    pub fn new(
        renderer: &mut vk::Renderer,
        target: &vk::SceneTarget,
        lights: &[Light],
    ) -> Result<Option<Self>> {
        if !lights.iter().any(|light| light.volume.is_some()) {
            return Ok(None);
        }
        ensure!(lights.len() <= CAPACITY, CAPACITY_ERROR);
        let depth = vk::DepthSample::new(renderer, target)?;
        let blur = blur::Blur::new(renderer, target)?;
        let combos = BTreeMap::from([
            (DEPTH.into(), i64::from(depth.is_some())),
            (COOKIE.into(), i64::from(lights.iter().any(|light| light.cookie.is_some()))),
            (SHADOW.into(), i64::from(lights.iter().any(|light| light.shadow))),
            (LIGHT_COUNT.into(), CAPACITY as i64),
        ]);
        let mut vertex = shader::translate(VERTEX, Stage::Vertex, &combos);
        let source = format!(
            "#if VOLUME_SHADOW\n{}\n#endif\n{FRAGMENT}",
            shader::SHADOW_SAMPLE.replace(shader::SHADOW_TEXTURE, SHADOW_SAMPLER)
        );
        let mut fragment = shader::translate(&source, Stage::Fragment, &combos);
        shader::unify_uniforms(&mut vertex, &mut fragment);
        let mut spec = vk::EffectPipelineSpec::quad(target.format, vk::SceneBlend::Copy, None);
        spec.depth_attachment = false;
        let pipeline =
            renderer.create_effect_pipeline_for(&vertex, &fragment, None, LABEL, spec)?;
        let quad = renderer.create_quad_buffer_ndc()?;
        Ok(Some(Self {
            blur,
            pipeline,
            quad,
            depth,
            uniforms: fragment.uniforms,
            shadow: lights.iter().any(|light| light.shadow),
            cookie: lights.iter().any(|light| light.cookie.is_some()),
        }))
    }

    fn compatible(&self, target: &vk::SceneTarget, lights: &[Light]) -> bool {
        self.depth.is_some() == target.has_depth()
            && self.shadow == lights.iter().any(|light| light.shadow)
            && self.cookie == lights.iter().any(|light| light.cookie.is_some())
    }

    pub fn allocation_bytes(&self) -> u64 {
        self.depth.as_ref().map_or(0, vk::DepthSample::allocation_bytes)
            + self.blur.allocation_bytes()
    }

    pub fn record(
        &self,
        renderer: &mut vk::Renderer,
        target: &vk::SceneTarget,
        projection: &Mat4,
        sources: Sources<'_>,
    ) -> Result<()> {
        let Sources { lights, shadows, cookies, .. } = sources;
        ensure!(lights.len() <= CAPACITY, CAPACITY_ERROR);
        let bytes = self.uniform_bytes(&self.blur.accumulation, projection, sources)?;
        if let Some(depth) = &self.depth {
            depth.record(renderer, target)?;
        }
        let mut inputs: Vec<_> = self.depth.iter().map(vk::DepthSample::sample).collect();
        if let Some(shadows) = shadows {
            if inputs.is_empty() {
                inputs.push(shadows.sample().comparison());
            }
            inputs.push(shadows.sample().comparison());
        }
        if let Some((_, sample)) = cookies {
            inputs.resize(COMPONENTS - 2, sample);
            inputs.push(sample);
        }
        renderer.record_effect_pass(
            &self.pipeline,
            &self.quad,
            vk::EffectDraw {
                target: &self.blur.accumulation,
                preserve: false,
                final_vertices: false,
            },
            &inputs,
            &bytes,
        )?;
        self.blur.record(renderer, &self.quad, target)
    }
    fn uniform_bytes(
        &self,
        target: &vk::SceneTarget,
        projection: &Mat4,
        sources: Sources<'_>,
    ) -> Result<Vec<u8>> {
        let Sources { lights, shadows, cookies, fog, eye } = sources;
        let inverse =
            paper_scene::matrix::mat4_inverse(projection).context("volumetric camera singular")?;
        let mut origins = vec![0.0; CAPACITY * COMPONENTS];
        let mut colors = origins.clone();
        let mut directions = origins.clone();
        let mut parameters = origins.clone();
        let mut right = origins.clone();
        let mut up = origins.clone();
        let mut shadow = origins.clone();
        let mut cookie = origins.clone();
        let cookie_values = cookies
            .as_ref()
            .map(|(layout, _)| layout.uniforms(lights))
            .and_then(|mut values| values.remove(SPOT_COOKIE));
        let mut spot_index = 0;

        for (slot, light) in
            lights.iter().filter(|light| light.visible && light.sun.is_none()).enumerate()
        {
            let cookie_slot = spot_index;
            if light.cone.is_some() {
                spot_index += 1;
            }
            let Some(volume) = light.volume else { continue };
            let offset = slot * COMPONENTS;
            origins[offset..offset + COMPONENTS].copy_from_slice(&[
                light.origin[0],
                light.origin[1],
                light.origin[2],
                light.radius,
            ]);
            colors[offset..offset + COMPONENTS].copy_from_slice(&[
                light.color[0],
                light.color[1],
                light.color[2],
                volume.density,
            ]);
            directions[offset..offset + COMPONENTS].copy_from_slice(&[
                light.forward[0],
                light.forward[1],
                light.forward[2],
                light.cone.map_or(-1.0, |cone| cone[0]),
            ]);
            parameters[offset..offset + COMPONENTS].copy_from_slice(&[
                volume.exponent,
                light.cone.map_or(-1.0, |cone| cone[1]),
                f32::from(light.cone.is_some()),
                0.0,
            ]);
            right[offset..offset + COMPONENTS - 1].copy_from_slice(&light.right);
            up[offset..offset + COMPONENTS - 1].copy_from_slice(&light.up);
            if light.cone.is_some()
                && let Some(transform) = cookie_values.as_ref().and_then(|values| {
                    values.get(cookie_slot * COMPONENTS..(cookie_slot + 1) * COMPONENTS)
                })
            {
                cookie[offset..offset + COMPONENTS].copy_from_slice(transform);
            }
            if let Some(shadows) = shadows {
                shadow[offset..offset + COMPONENTS].copy_from_slice(&shadows.parameters(light));
            }
        }
        let values: BTreeMap<String, Vec<f32>> = BTreeMap::from([
            (PROJECTION.into(), inverse.to_vec()),
            (FOG.into(), fog.enabled.to_vec()),
            (FOG_DISTANCE.into(), fog.distance.to_vec()),
            (FOG_HEIGHT.into(), fog.height.to_vec()),
            (EYE.into(), eye.to_vec()),
            (RESOLUTION.into(), vec![target.extent.width as f32, target.extent.height as f32]),
            (ORIGINS.into(), origins),
            (COLORS.into(), colors),
            (DIRECTIONS.into(), directions),
            (PARAMETERS.into(), parameters),
            (RIGHT.into(), right),
            (UP.into(), up),
            (SHADOW_PARAMETERS.into(), shadow),
            (COOKIE_TRANSFORM.into(), cookie),
            (
                SHADOW_SIZE.into(),
                shadows.map_or([0.0; COMPONENTS], super::shadow::Shadows::dimensions).to_vec(),
            ),
        ]);
        let reflected = self
            .uniforms
            .iter()
            .filter_map(|uniform| {
                let name = uniform.name.split('[').next()?;
                Some((uniform.name.clone(), values.get(name)?.clone()))
            })
            .collect();
        Ok(shader::pack_uniforms(&self.uniforms, &reflected))
    }
}

impl super::Group {
    pub(super) fn refresh_volumes(&mut self) -> Result<()> {
        let needed = self.lights.iter().any(|light| light.volume.is_some());
        if !needed && self.volumes.is_none() {
            return Ok(());
        }
        if needed
            && self
                .volumes
                .as_ref()
                .is_some_and(|volume| volume.compatible(&self.target, &self.lights))
        {
            return Ok(());
        }
        ensure!(
            !self.lights.iter().any(|light| light.shadow) || self.shadows.is_some(),
            SHADOW_SOURCE_ERROR
        );
        ensure!(
            !self.lights.iter().any(|light| light.cookie.is_some()) || self.cookie_slot.is_some(),
            COOKIE_SOURCE_ERROR
        );
        self.renderer.wait_render()?;
        self.volumes = Volumes::new(&mut self.renderer, &self.target, &self.lights)?;
        Ok(())
    }

    pub(super) fn volume_projection(&self) -> Result<Mat4> {
        if self.target.viewport.is_some()
            || self.camera.is_some()
            || (!self.meshes.is_empty()
                && self.mesh_settings.projection
                    == paper_scene::model::ProjectionMode::Orthographic)
        {
            Ok(self
                .mesh_settings
                .view(self.canvas, self.target.viewport, self.clip, false)?
                .projection)
        } else {
            let mut projection = super::scene_view_projection(self.canvas);
            self.clip.apply(&mut projection, false);
            Ok(projection)
        }
    }

    pub(super) fn record_volumes(&mut self) -> Result<()> {
        let Some(volumes) = &self.volumes else { return Ok(()) };
        let projection = self.volume_projection()?;
        let cookies =
            self.cookie_sample().map(|(view, sampler, _)| (&self.cookie_layout, (view, sampler)));
        volumes.record(
            &mut self.renderer,
            &self.target,
            &projection,
            Sources {
                lights: &self.lights,
                shadows: self.shadows.as_ref(),
                cookies,
                fog: self.fog,
                eye: self
                    .camera
                    .map_or_else(|| super::orthographic_eye(self.canvas), |camera| camera.eye),
            },
        )
    }
}
