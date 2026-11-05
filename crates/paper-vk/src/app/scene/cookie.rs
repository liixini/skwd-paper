use super::{script::Scripts, vk};
use anyhow::{Context, Result};
use ash::vk as raw;
use paper_scene::model::lighting::{CookieLayout, Light};

const SOURCE_ERROR: &str = "light cookie resources unavailable";

pub(super) fn selection(lights: &[Light]) -> Option<(String, String)> {
    lights
        .iter()
        .rev()
        .find(|light| light.visible && light.cookie.is_some())
        .map(|light| (light.id.clone(), light.cookie.clone().unwrap()))
}

impl super::Group {
    pub(super) fn cookie_sample(&self) -> Option<(raw::ImageView, raw::Sampler, raw::Extent2D)> {
        let texture = self.textures.get(self.cookie_slot?)?;
        Some((texture.view, texture.sampler, texture.extent))
    }

    pub(super) fn refresh_cookies(&mut self, scripts: &mut Scripts) -> Result<()> {
        let selected = selection(&self.lights);
        if selected == self.cookie_current {
            return Ok(());
        }
        let Some(slot) = self.cookie_slot else { return Ok(()) };
        let cookies = if let Some((id, _)) = &selected {
            let light = self.lights.iter().find(|light| &light.id == id).unwrap();
            scripts
                .host
                .as_ref()
                .and_then(paper_scene::script::SceneScripts::image_resources)
                .context(SOURCE_ERROR)?
                .light_cookies(std::slice::from_ref(light))?
        } else {
            None
        };
        let (texture, layout) = if let Some(cookies) = cookies {
            let texture = self.renderer.create_scene_texture_pixels(
                &cookies.texture.pixels,
                cookies.texture.clamp,
                cookies.texture.nearest,
                vk::MipSampling::Linear,
            )?;
            (texture, cookies.layout)
        } else {
            (
                self.renderer.create_scene_texture(1, 1, &[0; super::RGBA_CHANNELS])?,
                CookieLayout::default(),
            )
        };
        self.renderer.wait_render()?;
        let sample = (texture.view, texture.sampler);
        let values = layout.uniforms(&self.lights);
        for actor in self.meshes.iter_mut().chain(scripts.shadow_meshes_mut()) {
            actor.bind_cookie(&self.renderer, sample, self.reflection.as_ref());
            actor.update_lighting(&self.renderer, &values);
        }
        let previous = std::mem::replace(&mut self.textures[slot], texture);
        self.renderer.destroy_scene_texture(previous);
        self.cookie_layout = layout;
        self.cookie_current = selected;
        self.scene_uniforms.extend(values.clone());
        for fx in &mut self.fx {
            fx.uniforms.extend(values.clone());
        }
        for material in &mut self.materials {
            material.uniforms.extend(values.clone());
        }
        Ok(())
    }
}
