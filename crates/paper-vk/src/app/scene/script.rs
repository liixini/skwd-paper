use super::{Group, SceneModel};
use anyhow::Result;
use paper_scene::model::script::{Layout, frames};
use paper_scene::script::SceneScripts;
use paper_scene::text::script::ScriptText;

pub(super) struct Scripts {
    pub host: SceneScripts,
    layouts: Vec<Layout>,
    text: Vec<(usize, usize, ScriptText)>,
    pending: bool,
}

impl Scripts {
    pub fn take(model: &mut SceneModel, slots: &[usize]) -> Option<Self> {
        let host = model.scripts.take()?;
        let layouts = model
            .layers
            .iter()
            .map(|l| Layout::new(l, &host.scene, model.canvas, &host.properties))
            .collect();
        let text = model
            .layers
            .iter_mut()
            .enumerate()
            .filter_map(|(i, l)| Some((i, slots[i], l.script_text.take()?)))
            .collect();
        Some(Self { host, layouts, text, pending: true })
    }
}

impl Group {
    fn apply_general(&mut self, key: &str, value: &serde_json::Value) {
        let numbers = paper_scene::effects::json_numbers(value);
        let scalar = numbers.as_ref().and_then(|list| list.first().copied());
        let flag = value.as_bool().or(scalar.map(|v| v != 0.0));
        let rgb =
            numbers.as_ref().filter(|list| list.len() >= 3).map(|list| [list[0], list[1], list[2]]);
        match key {
            "clearcolor" => {
                if let Some(rgb) = rgb {
                    self.clear = rgb;
                }
            }
            "ambientcolor" | "skylightcolor" => {
                let name = if key == "ambientcolor" {
                    "g_LightAmbientColor"
                } else {
                    "g_LightSkylightColor"
                };
                if let Some(rgb) = rgb {
                    self.scene_uniforms.insert(name.to_owned(), rgb.to_vec());
                    for fx in &mut self.fx {
                        fx.uniforms.insert(name.to_owned(), rgb.to_vec());
                    }
                }
            }
            "bloom" => {
                if let Some(on) = flag
                    && let Some(quad) =
                        self.fx.iter().find(|fx| fx.layer_id == "@bloom").map(|fx| fx.quad)
                {
                    self.quads[quad].tint[3] = if on { 1.0 } else { 0.0 };
                }
            }
            "bloomstrength" | "bloomthreshold" | "bloomtint" => {
                let name = match key {
                    "bloomstrength" => "g_BloomStrength",
                    "bloomthreshold" => "g_BloomThreshold",
                    _ => "g_BloomTint",
                };
                let value = if key == "bloomtint" {
                    rgb.map(|c| c.to_vec())
                } else {
                    scalar.map(|v| vec![v])
                };
                if let Some(value) = value
                    && let Some(fx) = self.fx.iter_mut().find(|fx| fx.layer_id == "@bloom")
                {
                    fx.uniforms.insert(name.to_owned(), value);
                }
            }
            "fov" => {
                if let Some(fov) = scalar {
                    self.mouse.set_fov(fov.clamp(1.0, 179.0));
                }
            }
            "cameraparallax"
            | "cameraparallaxamount"
            | "cameraparallaxdelay"
            | "cameraparallaxmouseinfluence" => self.mouse.set_parallax(key, flag, scalar),
            "camerashake"
            | "camerashakeamplitude"
            | "camerashakespeed"
            | "camerashakeroughness" => {
                self.mouse.set_shake(key, flag, scalar);
            }
            _ => {}
        }
    }

    pub(super) fn advance_scripts(&mut self, time: f32, dt: f32) -> Result<()> {
        let Some(mut scripts) = self.scripts.take() else {
            return Ok(());
        };
        let result = self.script_frame(&mut scripts, time, dt);
        self.scripts = Some(scripts);
        result
    }

    fn script_frame(&mut self, scripts: &mut Scripts, time: f32, dt: f32) -> Result<()> {
        if scripts.host.needs_audio() {
            for count in paper_audio::spectrum::BAND_COUNTS {
                if let (Some(left), Some(right)) =
                    (self.bands.slice(count, false), self.bands.slice(count, true))
                {
                    scripts.host.audio(count, left, right)?;
                }
            }
        }
        let x = self.mouse.position[0] * self.canvas.0;
        let y = self.mouse.position[1] * self.canvas.1;
        let hits = scripts
            .layouts
            .iter()
            .zip(&self.quads)
            .filter_map(|(layout, q)| {
                let (sin, cos) = q.angle.sin_cos();
                let dx = x - q.rect[0];
                let dy = y - q.rect[1];
                (q.tint[3] > 0.0
                    && (dx * cos + dy * sin).abs() <= q.rect[2].abs() * 0.5
                    && (-dx * sin + dy * cos).abs() <= q.rect[3].abs() * 0.5)
                    .then(|| layout.id.clone())
            })
            .collect();
        scripts.host.pointer(self.mouse.position, self.mouse.buttons, hits)?;
        let changed = scripts.host.tick(time, dt, self.mouse.position)?;
        for command in scripts.host.take_commands() {
            match command {
                paper_scene::script::ScriptCommand::Sprite { object, op } => {
                    self.sprite_command(object, op, time);
                }
                paper_scene::script::ScriptCommand::Sound { id, op } => {
                    use paper_scene::script::SoundOp;
                    let op = match op {
                        SoundOp::Play => paper_audio::VoiceOp::Play,
                        SoundOp::Stop => paper_audio::VoiceOp::Stop,
                        SoundOp::Pause => paper_audio::VoiceOp::Pause,
                        SoundOp::Gain(gain) => paper_audio::VoiceOp::Gain(gain),
                    };
                    self.script_sounds.push((id, op));
                }
            }
        }
        for (key, value) in scripts.host.take_general_changes() {
            self.apply_general(&key, &value);
        }
        if !changed && !std::mem::take(&mut scripts.pending) {
            return Ok(());
        }
        let states =
            frames(&scripts.host.scene, &scripts.layouts, self.canvas, &scripts.host.properties);
        for (index, slot, text) in &mut scripts.text {
            let Some(wanted) = states[*index].as_ref().and_then(|s| s.text.as_ref()) else {
                continue;
            };
            if wanted == &text.shown {
                continue;
            }
            let Some(rendered) = text.render(wanted) else {
                continue;
            };
            let texture = &rendered.texture;
            let replacement =
                self.renderer.create_scene_texture_pixels(&texture.pixels, true, false, false)?;
            let previous = std::mem::replace(&mut self.textures[*slot], replacement);
            self.renderer.destroy_scene_texture(previous);
            scripts.layouts[*index].size = [texture.width as f32, texture.height as f32];
            scripts.layouts[*index].offset = [
                rendered.offset.0 + texture.width as f32 * 0.5,
                rendered.offset.1 + texture.height as f32 * 0.5,
            ];
            text.shown.clone_from(wanted);
        }
        let states =
            frames(&scripts.host.scene, &scripts.layouts, self.canvas, &scripts.host.properties);
        for (index, state) in states.into_iter().enumerate() {
            let Some(state) = state else {
                continue;
            };
            if state.rect.iter().chain(state.tint.iter()).any(|v| !v.is_finite())
                || !state.angle.is_finite()
            {
                continue;
            }
            let quad = &mut self.quads[index];
            if scripts.layouts[index].passthrough {
                quad.rect =
                    [state.rect[0], state.rect[1], state.rect[2].abs(), state.rect[3].abs()];
                quad.angle = 0.0;
            } else {
                quad.rect = state.rect;
                quad.angle = state.angle;
            }
            self.mouse.set_script_rect(index, quad.rect);
            if let Some(fx) = self.fx.iter_mut().find(|fx| fx.quad == index) {
                let origin = (state.rect[0], self.canvas.1 - state.rect[1], state.depth);
                let scale = [state.scale[0], state.scale[1], 1.0];
                let matrix = super::model_matrix(origin, scale, -state.angle);
                let inverse = super::model_inverse(origin, scale, -state.angle);
                for (name, value) in [
                    ("g_LayerModelMatrix", matrix),
                    ("g_ModelMatrix", matrix),
                    ("g_ModelMatrixInverse", inverse),
                    (
                        "g_ModelViewProjectionMatrixInverse",
                        super::layer_projection_inverse(&inverse, self.canvas),
                    ),
                ] {
                    fx.uniforms.insert(name.into(), value.to_vec());
                }
                fx.base_quad.tint = state.source_tint;
                fx.fallback_tint = state.tint;
                quad.tint = [1.0, 1.0, 1.0, f32::from(state.tint[3] > 0.0)];
                let node = scripts.host.scene["objects"].as_array().and_then(|nodes| {
                    nodes.iter().find(|n| n["id"].to_string().trim_matches('"') == fx.layer_id)
                });
                let mut local = std::collections::HashMap::new();
                for (pass, owner) in fx.passes.iter_mut().zip(&fx.owner) {
                    let ordinal = local.entry(*owner).or_insert(0usize);
                    if let Some(values) = node.and_then(|n| {
                        n.get("effects")?
                            .get(*owner)?
                            .get("passes")?
                            .get(*ordinal)?
                            .get("constantshadervalues")
                    }) {
                        pass.apply_script_values(values, &scripts.host.properties);
                    }
                    *ordinal += 1;
                }
            } else if scripts.layouts[index].hidden_without_fx {
                quad.tint = [1.0, 1.0, 1.0, 0.0];
            } else {
                quad.tint = state.tint;
            }
        }
        Ok(())
    }
}
