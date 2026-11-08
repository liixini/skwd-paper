use super::{Group, SceneModel, sampling};
use anyhow::Result;
use paper_scene::model::script::{Layout, frames};
use paper_scene::script::SceneScripts;
use paper_scene::text::script::{Backdrop, ScriptText};

mod images;
mod models;

const MAX_PARTICLE_SIM_FACTOR: f32 = 10.0;

#[cfg(test)]
pub(super) mod tests;

pub(super) fn screen_resolution(surfaces: &[crate::wayland::SurfaceUi]) -> Option<[u32; 2]> {
    let surface = surfaces.first()?;
    let scale = surface.scale.max(1) as u32;
    Some([surface.width.saturating_mul(scale), surface.height.saturating_mul(scale)])
}

fn effect_objects(
    scene: &serde_json::Value,
) -> std::collections::HashMap<String, &serde_json::Value> {
    let objects = scene["objects"].as_array().map_or(&[][..], Vec::as_slice);
    let mut indexed = std::collections::HashMap::with_capacity(objects.len());
    for object in objects {
        indexed.entry(object["id"].to_string().trim_matches('"').to_owned()).or_insert(object);
    }
    indexed
}

fn puppet_attachments(puppets: &[super::PuppetGroup]) -> paper_scene::model::Attachments<'_> {
    let mut attachments = paper_scene::model::Attachments::new();
    for puppet in puppets {
        for (attachment, pose) in
            puppet.puppet.mesh.attachments.iter().zip(&puppet.pose.attachments)
        {
            attachments.entry((puppet.id.as_str(), attachment.name.as_str())).or_insert(*pose);
        }
    }
    attachments
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Startup {
    Pending,
    Shown(f32),
    Running,
}

fn script_update_due(startup: &mut Startup, time: f32) -> bool {
    match *startup {
        Startup::Pending => *startup = Startup::Shown(time),
        Startup::Shown(shown) if time == shown => return false,
        Startup::Shown(_) => *startup = Startup::Running,
        Startup::Running => {}
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TextBackdrop {
    layer: usize,
    quad: usize,
    bounds: paper_scene::text::Bounds,
    padding: Option<[f32; 2]>,
}

pub(super) struct Scripts {
    pub host: Option<SceneScripts>,
    keyframes: paper_scene::keyframe::Bindings,
    native: Option<(serde_json::Value, paper_scene::model::Properties)>,
    layouts: Vec<Layout>,
    text: Vec<(usize, usize, ScriptText)>,
    backdrops: Vec<TextBackdrop>,
    backdrop_texture: Option<usize>,
    #[cfg(test)]
    text_upload_error: Option<anyhow::Error>,
    spare_backdrops: std::collections::BTreeMap<usize, usize>,
    pub(super) pending: bool,
    images: std::collections::BTreeMap<usize, (usize, usize)>,
    particles: std::collections::BTreeMap<usize, String>,
    spare_images: Vec<(usize, usize)>,
    models: std::collections::BTreeMap<usize, models::Model>,
    spare_model_quads: Vec<usize>,
    discard_warned: std::collections::HashSet<usize>,
    spare_model_textures: Vec<usize>,
    order: Vec<(Option<usize>, usize)>,
    pub(super) startup: Startup,
    particle_state_ready: bool,
}

impl Scripts {
    pub fn animated(&self) -> bool {
        self.keyframes.animated() || self.host.as_ref().is_some_and(SceneScripts::animated)
    }

    pub fn take(
        model: &mut SceneModel,
        slots: &[usize],
        particles: &[super::ParticleGroup],
        meshes: Vec<(Option<usize>, usize)>,
    ) -> Option<Self> {
        let host = model.scripts.take();
        let native = model.animated_scene.take();
        if host.is_none() && native.is_none() {
            return None;
        }
        let (scene, properties) = host
            .as_ref()
            .map(|h| (&h.scene, &h.properties))
            .or_else(|| native.as_ref().map(|(s, p)| (s, p)))
            .unwrap();
        let mut order: Vec<_> = model
            .layers
            .iter()
            .map(|layer| (layer.object_index(scene), layer.scene_order))
            .chain(particles.iter().map(|particle| {
                (paper_scene::model::object_index(scene, &particle.id), particle.scene_order)
            }))
            .chain(meshes)
            .collect();
        order.sort_unstable_by_key(|(_, rank)| *rank);
        order.dedup_by_key(|(_, rank)| *rank);
        let layouts =
            model.layers.iter().map(|l| Layout::new(l, scene, model.canvas, properties)).collect();
        let text = model
            .layers
            .iter_mut()
            .enumerate()
            .filter_map(|(i, l)| Some((i, slots[i], l.script_text.take()?)))
            .collect();
        let images = model
            .layers
            .iter()
            .enumerate()
            .filter_map(|(quad, layer)| {
                let slot = layer.object_index(scene)?;
                host.as_ref()?
                    .created_layer_slots()
                    .contains(&slot)
                    .then_some((slot, (quad, slots[quad])))
            })
            .collect();
        Some(Self {
            host,
            native,
            keyframes: std::mem::take(&mut model.keyframes),
            layouts,
            text,
            backdrops: Vec::new(),
            backdrop_texture: None,
            #[cfg(test)]
            text_upload_error: None,
            spare_backdrops: std::collections::BTreeMap::new(),
            pending: true,
            images,
            particles: std::collections::BTreeMap::new(),
            spare_images: Vec::new(),
            models: std::collections::BTreeMap::new(),
            spare_model_quads: Vec::new(),
            discard_warned: std::collections::HashSet::new(),
            spare_model_textures: Vec::new(),
            order,
            startup: Startup::Pending,
            particle_state_ready: false,
        })
    }

    pub fn install_backdrops(
        &mut self,
        renderer: &mut crate::vk::Renderer,
        quads: &mut Vec<crate::vk::SceneQuad>,
        order: &mut Vec<usize>,
        textures: &mut Vec<crate::vk::SceneTexture>,
    ) -> Result<()> {
        let staged = if self.text.is_empty() { None } else { self.stage_backdrop(renderer)? };
        self.accept_backdrop(staged, textures);
        for index in 0..self.text.len() {
            self.install_backdrop(self.text[index].0, quads, order);
        }
        Ok(())
    }

    fn discard_created(&mut self, slot: usize, error: anyhow::Error) -> Result<()> {
        if crate::vk::device_lost(&error) {
            return Err(error);
        }
        if self.discard_warned.insert(slot) {
            tracing::warn!("scene: script layer {slot} discarded: {error:#}");
        } else {
            tracing::debug!("scene: script layer {slot} discarded again: {error:#}");
        }
        if let Some(host) = &mut self.host {
            host.discard_created(slot);
        }
        Ok(())
    }

    fn stage_backdrop(
        &self,
        renderer: &mut crate::vk::Renderer,
    ) -> Result<Option<crate::vk::SceneTexture>> {
        if self.backdrop_texture.is_some() {
            return Ok(None);
        }
        #[cfg(test)]
        tests::backdrop_upload(crate::vk::Renderer::scene_texture_live_allocations())?;
        let white = paper_scene::tex::Pixels::rgba(1, 1, vec![u8::MAX; super::RGBA_CHANNELS]);
        renderer
            .create_scene_texture_pixels(&white, true, false, crate::vk::MipSampling::Disabled)
            .map(Some)
    }

    fn accept_backdrop(
        &mut self,
        staged: Option<crate::vk::SceneTexture>,
        textures: &mut Vec<crate::vk::SceneTexture>,
    ) {
        if let Some(texture) = staged {
            self.backdrop_texture = Some(textures.len());
            textures.push(texture);
        }
    }

    fn install_backdrop(
        &mut self,
        layer: usize,
        quads: &mut Vec<crate::vk::SceneQuad>,
        order: &mut Vec<usize>,
    ) {
        #[cfg(test)]
        tests::backdrop_installed();
        let mut quad = quads[layer].clone();
        quad.texture = self.backdrop_texture.expect("staged text backdrop");
        quad.tint = [0.0; super::RGBA_CHANNELS];
        quad.order_bias = -1;
        quad.blend = crate::vk::SceneBlend::Alpha;
        let spare = self
            .spare_backdrops
            .remove(&layer)
            .or_else(|| self.spare_backdrops.pop_last().map(|(_, index)| index));
        let backdrop = if let Some(index) = spare {
            quads[index] = quad;
            order[index] = order[layer];
            index
        } else {
            let index = quads.len();
            quads.push(quad);
            order.push(order[layer]);
            index
        };
        let text =
            &self.text.iter().find(|(owner, _, _)| *owner == layer).expect("text backdrop owner").2;
        self.backdrops.push(TextBackdrop {
            layer,
            quad: backdrop,
            bounds: text.bounds,
            padding: text.effect_padding(),
        });
    }
}

fn update_backdrop(
    quad: &mut crate::vk::SceneQuad,
    frame: &paper_scene::model::script::Frame,
    style: &Backdrop,
    bounds: paper_scene::text::Bounds,
    canvas: (f32, f32),
) {
    let [horizontal, vertical] = style.padding;
    let mut geometry = frame.geometry.expect("text geometry");
    let center = geometry.transform.point([
        bounds.offset[0] - geometry.offset[0],
        bounds.offset[1] - geometry.offset[1],
        0.0,
    ]);
    geometry.transform.x = center[0];
    geometry.transform.y = center[1];
    geometry.offset = bounds.offset;
    geometry.size = [bounds.size[0] + horizontal * 2.0, bounds.size[1] + vertical * 2.0];
    quad.rect = [
        center[0],
        center[1],
        geometry.size[0] * frame.scale[0],
        geometry.size[1] * frame.scale[1],
    ];
    quad.angle = frame.angle;
    quad.projection = Some(geometry.projection(canvas));
    let [red, green, blue] = style.color;
    let [_, _, _, alpha] = frame.tint;
    quad.tint = [red, green, blue, if style.visible { alpha } else { 0.0 }];
    if quad.rect.iter().chain(quad.tint.iter()).any(|value| !value.is_finite()) {
        quad.tint = [0.0; super::RGBA_CHANNELS];
    }
}

impl Group {
    pub(super) fn sync_sound_states(&mut self, ctl: &crate::ctl::Ctl) {
        if let Some(host) = self.scripts.as_mut().and_then(|scripts| scripts.host.as_mut()) {
            host.set_sound_states(&ctl.scene_voice_states());
        }
    }

    pub(super) fn resize_screen(&mut self, resolution: [u32; 2]) {
        if let Some(scripts) = &mut self.scripts
            && let Some(host) = &mut scripts.host
        {
            host.resize_screen(resolution);
            scripts.pending = true;
        }
    }

    fn remap_scene_order(&mut self, remap: &std::collections::HashMap<usize, usize>) {
        for rank in &mut self.quad_scene_order {
            *rank = remap[rank];
        }
        for fx in &mut self.fx {
            fx.scene_order = remap[&fx.scene_order];
        }
        for particle in &mut self.particles {
            particle.scene_order = remap[&particle.scene_order];
        }
        if let Some(reflection) = &mut self.reflection {
            for twin in reflection.twins.values_mut() {
                twin.order = remap[&twin.order];
            }
        }
    }

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
            paper_scene::model::CLEAR_ENABLED_FIELD => {
                if let Some(flag) = flag.filter(|flag| *flag != self.clear_enabled) {
                    self.clear_enabled = flag;
                    if let Some(reflection) = &mut self.reflection {
                        reflection.ready = false;
                    }
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
            paper_scene::model::ZOOM_FIELD => {
                if let Some(zoom) = scalar.filter(|zoom| zoom.is_finite() && *zoom > 0.0) {
                    self.set_scene_view(paper_scene::model::CameraView { zoom, ..self.scene_view });
                }
            }
            "fov" => {
                if let Some(fov) = scalar {
                    let (min, max) = paper_scene::model::FOV_RANGE;
                    self.mouse.set_fov(fov.clamp(min, max));
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
        if let Some(host) = &mut scripts.host {
            host.finish_compose();
        }
        scripts.pending |= result.is_err();
        self.scripts = Some(scripts);
        result
    }

    fn script_frame(&mut self, scripts: &mut Scripts, time: f32, dt: f32) -> Result<()> {
        let mut changed = match &mut scripts.host {
            Some(host) => {
                let changed = scripts.keyframes.apply(&mut host.scene, time);
                host.sync_keyframes(&scripts.keyframes);
                changed
            }
            None => scripts.keyframes.apply(&mut scripts.native.as_mut().unwrap().0, time),
        };
        if let Some(host) = &mut scripts.host {
            let videos = self
                .video_objects
                .iter()
                .map(|(&object, &video)| self.videos[video].state(object))
                .chain(self.image_videos.iter().map(|(&object, video)| video.state(object)))
                .collect::<Vec<_>>();
            host.set_videos(&videos, false);
            let attachments = puppet_attachments(&self.puppets);
            host.set_attachment_poses(&attachments);
            if scripts.particle_state_ready {
                let states = self
                    .particles
                    .iter()
                    .map(|group| (group.id.clone(), group.sim.is_playing()))
                    .collect::<Vec<_>>();
                host.set_particle_states(&states);
            }
            if host.needs_audio() {
                for count in paper_audio::spectrum::BAND_COUNTS {
                    if let (Some(left), Some(right)) =
                        (self.bands.slice(count, false), self.bands.slice(count, true))
                    {
                        host.audio(count, left, right)?;
                    }
                }
            }
            let pointer = if self.mouse.inside { self.mouse.position } else { [-1.0; 2] };
            let hits = if self.mouse.inside {
                let point = [pointer[0] * self.canvas.0, pointer[1] * self.canvas.1];
                script_hits(&scripts.layouts, &self.view_quads(), point, self.canvas)
            } else {
                Vec::new()
            };
            host.pointer(pointer, self.mouse.buttons, hits)?;
            if script_update_due(&mut scripts.startup, time) {
                changed |= host.tick(time, dt, self.mouse.position)?;
            }
        }
        changed |= self.image_changes(scripts)?;
        if let Some(host) = &mut scripts.host {
            if let Some(camera) = host.take_camera() {
                if let Some(zoom) = camera.zoom {
                    self.set_scene_view(paper_scene::model::CameraView { zoom, ..self.scene_view });
                }
                if let Some(pose) = camera.pose
                    && camera.initialized
                    && camera.oriented
                {
                    self.camera_source.set_script(pose.eye, pose.center, pose.up, camera.scripted);
                }
                changed = true;
            }
            if host.needs_media() && self.media.is_none() {
                self.media = Some(super::media::Media::start()?);
            }
            let mut rejected_sort = false;
            for command in host.take_commands() {
                match command {
                    paper_scene::script::ScriptCommand::Video { object, op } => {
                        let video = self
                            .video_objects
                            .get(&object)
                            .map(|&index| &mut self.videos[index])
                            .or_else(|| self.image_videos.get_mut(&object));
                        if let Some(video) = video {
                            video.command(op)?;
                            changed = true;
                        }
                    }
                    paper_scene::script::ScriptCommand::Shortcut { name } => {
                        if let Some(action) = super::media::Shortcut::named(&name) {
                            if self.media.is_none() {
                                self.media = Some(super::media::Media::start()?);
                            }
                            if let Some(media) = &mut self.media
                                && let Err(error) = media.control(action)
                            {
                                tracing::warn!(%name, %error, "media shortcut unavailable");
                            }
                        }
                    }
                    paper_scene::script::ScriptCommand::Sort { order } => {
                        let ranks: std::collections::HashMap<_, _> =
                            order.iter().enumerate().map(|(rank, slot)| (*slot, rank)).collect();
                        let remap: std::collections::HashMap<_, _> = scripts
                            .order
                            .iter_mut()
                            .enumerate()
                            .map(|(tail, (slot, rank))| {
                                let next = slot.map_or(order.len() + tail, |slot| ranks[&slot]);
                                (std::mem::replace(rank, next), next)
                            })
                            .collect();
                        self.remap_scene_order(&remap);
                        let plans = self.scene_target_plan(true).and_then(|plan| {
                            self.scene_target_plan(false).map(|animation| (plan, animation))
                        });
                        let Ok((plan, animation)) = plans else {
                            let reverse = remap.into_iter().map(|(old, new)| (new, old)).collect();
                            self.remap_scene_order(&reverse);
                            for (_, rank) in &mut scripts.order {
                                *rank = reverse[rank];
                            }
                            rejected_sort = true;
                            continue;
                        };
                        self.effects_animated = animation.dynamic.iter().any(|dynamic| *dynamic);
                        self.apply_scene_target_plan(&plan)?;
                        host.accept_layer_order(order);
                    }
                    paper_scene::script::ScriptCommand::Sprite { object, op } => {
                        self.sprite_command(object, op, time);
                    }
                    paper_scene::script::ScriptCommand::EffectFunction { id, effect, name } => {
                        if let Some(fx) = self.fx.iter_mut().find(|fx| fx.layer_id == id) {
                            if fx.queue_clear_function(effect, &name) {
                                changed = true;
                            } else {
                                tracing::warn!(layer = %id, effect, function = %name, "effect function unavailable");
                            }
                        }
                        if let Some(twin) = self.mirror_fx.iter_mut().find(|fx| fx.layer_id == id) {
                            twin.queue_clear_function(effect, &name);
                        }
                    }
                    paper_scene::script::ScriptCommand::Material { id, effect, name, values } => {
                        if let Some(fx) = self.fx.iter_mut().find(|fx| fx.layer_id == id)
                            && let Some(index) =
                                fx.passes.iter().position(|pass| pass.source() == Some(effect))
                        {
                            let resolved = fx.passes[index].apply_script_values(
                                &serde_json::json!({(name): values}),
                                &std::collections::BTreeMap::new(),
                            );
                            fx.material_values[index].extend(resolved);
                            changed = true;
                        }
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
                    paper_scene::script::ScriptCommand::Particle { id, op } => {
                        let Some(index) = self.particles.iter().position(|group| group.id == id)
                        else {
                            continue;
                        };
                        match op {
                            paper_scene::script::ParticleOp::Play => {
                                self.particles[index].sim.play();
                            }
                            paper_scene::script::ParticleOp::Pause => {
                                self.particles[index].sim.pause();
                            }
                            paper_scene::script::ParticleOp::Stop => {
                                self.particles[index].sim.stop();
                            }
                            paper_scene::script::ParticleOp::Emit(count) => {
                                self.prepare_image_samples(dt)?;
                                let mut samples = sampling::ImageQueries::new(
                                    &mut self.image_samples,
                                    &mut self.renderer,
                                    &self.textures,
                                    &self.puppets,
                                    false,
                                );
                                let group = &mut self.particles[index];
                                group.sim.set_scene_frame(self.particle_frame);
                                group.sim.emit_particles(&group.system, count, &mut |source| {
                                    samples.query(source);
                                });
                                samples.finish()?;
                            }
                            paper_scene::script::ParticleOp::Size(size) => {
                                self.particles[index].system.size_scale = size
                                    .clamp(0.0, paper_scene::particles::MAX_INSTANCE_SIZE_SCALE);
                            }
                            paper_scene::script::ParticleOp::Alpha(alpha) => {
                                self.particles[index].system.alpha = alpha.clamp(0.0, 1.0);
                            }
                            paper_scene::script::ParticleOp::Count(count) => {
                                self.particles[index].system.count_scale =
                                    count.clamp(0.0, MAX_PARTICLE_SIM_FACTOR);
                            }
                            paper_scene::script::ParticleOp::Rate(rate) => {
                                self.particles[index].system.rate_scale =
                                    rate.clamp(0.0, MAX_PARTICLE_SIM_FACTOR);
                            }
                            paper_scene::script::ParticleOp::Speed(speed) => {
                                self.particles[index].system.set_speed(speed);
                            }
                            paper_scene::script::ParticleOp::Lifetime(lifetime) => {
                                self.particles[index].system.set_lifetime(lifetime);
                            }
                            paper_scene::script::ParticleOp::Color(color) => {
                                self.particles[index].system.set_color(color);
                            }
                            paper_scene::script::ParticleOp::ControlPoint(point, position) => {
                                self.particles[index].system.set_control_point(point, position);
                            }
                        }
                        changed = true;
                    }
                }
            }
            if !self.video_objects.is_empty() || !self.image_videos.is_empty() {
                let videos = self
                    .video_objects
                    .iter()
                    .map(|(&object, &video)| self.videos[video].state(object))
                    .chain(self.image_videos.iter().map(|(&object, video)| video.state(object)))
                    .collect::<Vec<_>>();
                host.set_videos(&videos, true);
            }
            scripts.particle_state_ready = true;
            if rejected_sort {
                host.reject_layer_order();
            }
            for (key, value) in host.take_general_changes() {
                self.apply_general(&key, &value);
            }
            for object in host.take_effect_resets() {
                for fx in self.fx.iter_mut().chain(&mut self.mirror_fx) {
                    if paper_scene::model::object_index(&host.scene, &fx.layer_id) == Some(object) {
                        fx.reset_targets = true;
                        changed = true;
                    }
                }
            }
            if let Some(resources) = host.image_resources() {
                self.sync_bloom(&host.scene, &host.properties, &resources, &mut scripts.order)?;
            }
        }
        let resources = scripts.host.as_ref().and_then(SceneScripts::image_resources);
        let (scene, properties) = match &mut scripts.host {
            Some(host) => (&mut host.scene, &host.properties),
            None => {
                let (scene, properties) = scripts.native.as_mut().unwrap();
                (scene, &*properties)
            }
        };
        changed |= self.puppets.iter().any(|puppet| !puppet.pose.attachments.is_empty());
        changed |= self.camera_source.play_paths(scene, properties, dt);
        if !changed && !std::mem::take(&mut scripts.pending) {
            return Ok(());
        }
        for key in scripts.keyframes.general_keys() {
            if let Some(value) = scene["general"].get(key) {
                self.apply_general(key, value);
            }
        }
        let flat = self.mesh_settings.projection != paper_scene::model::ProjectionMode::Other;
        if let Some(placement) = self.camera_source.next(scene, properties, self.cameras_move)
            && !flat
        {
            self.orient_camera(placement.eye, placement.center, placement.up, placement.fov);
        }
        let mut view =
            paper_scene::model::active_camera_view(scene, properties, self.scene_view.zoom, flat);
        if flat {
            view = self.camera_source.flat_view(view);
        }
        if view != self.scene_view {
            self.set_scene_view(view);
        }
        let attachments = puppet_attachments(&self.puppets);
        let particle_states = paper_scene::model::script::particle_frames(
            scene,
            self.particles.iter().map(|group| group.id.as_str()),
            self.canvas,
            properties,
            &attachments,
        );
        for (group, state) in self.particles.iter_mut().zip(particle_states) {
            if group.parent.is_some() {
                continue;
            }
            if let Some(state) = state {
                state.apply(&mut group.system);
                group.parallax = state.parallax;
                group.visible = state.visible;
            }
        }
        let mut states = frames(scene, &scripts.layouts, self.canvas, properties, &attachments);
        let mut lights =
            paper_scene::model::lighting::uniforms(scene, properties, &attachments, &self.sun_view);
        self.lights = paper_scene::model::lighting::lights(scene, properties, &attachments);
        self.fog = paper_scene::model::fog::Volume::load(scene, properties);
        lights.extend(self.cookie_layout.uniforms(&self.lights));
        if !lights.is_empty() {
            self.scene_uniforms.extend(lights.clone());
            for fx in &mut self.fx {
                fx.uniforms.extend(lights.clone());
            }
            for material in &mut self.materials {
                material.uniforms.extend(lights.clone());
            }
        }
        drop(attachments);
        let objects = effect_objects(scene);
        for fx in &mut self.fx {
            if let Some(node) = objects.get(&fx.layer_id) {
                for effect in &mut fx.effects {
                    effect.apply_object(node, properties);
                }
            }
        }
        let mut text_layout_changed = false;
        for (index, slot, text) in &mut scripts.text {
            let Some(wanted) = states[*index].as_ref().and_then(|s| s.text.as_ref()) else {
                continue;
            };
            let mode = if self
                .fx
                .iter()
                .find(|fx| fx.quad == *index)
                .is_some_and(|fx| fx.owner.iter().any(|owner| fx.effects[*owner].visible))
            {
                paper_scene::text::RasterMode::Effect
            } else {
                paper_scene::text::RasterMode::Direct
            };
            text.set_mode(mode);
            if text.has_rejection()
                && objects.get(&scripts.layouts[*index].id).is_some_and(|object| {
                    text.rejected(
                        wanted,
                        object,
                        properties,
                        resources.as_ref().map_or(0, |resources| resources.revision()),
                    )
                })
            {
                continue;
            }
            let previous_style = objects
                .get(&scripts.layouts[*index].id)
                .and_then(|object| text.update_style(object, properties));
            if wanted == &text.shown && !text.needs_render {
                continue;
            }
            #[cfg(test)]
            tests::text_raster_attempt();
            let Some(rendered) = text.render(wanted) else {
                text.reject(wanted, None);
                if let Some(style) = previous_style {
                    text.restore(style);
                }
                continue;
            };
            let object_slot = paper_scene::model::object_index(scene, &scripts.layouts[*index].id);
            let payload = rendered.texture.payload_bytes();
            if let (Some(resources), Some(object_slot)) = (&resources, object_slot)
                && resources.replacement(object_slot, payload).is_err()
            {
                text.reject(wanted, Some(resources.revision()));
                if let Some(style) = previous_style {
                    text.restore(style);
                }
                continue;
            }
            let texture = &rendered.texture;
            let upload = |renderer: &mut crate::vk::Renderer| {
                renderer.create_text_texture(
                    &texture.pixels,
                    crate::vk::MipSampling::Disabled,
                    texture.text_batch.as_ref(),
                )
            };
            #[cfg(test)]
            let upload =
                scripts.text_upload_error.take().map_or_else(|| upload(&mut self.renderer), Err);
            #[cfg(not(test))]
            let upload = upload(&mut self.renderer);
            let replacement = match upload {
                Ok(replacement) => replacement,
                Err(error) => {
                    if let Some(style) = previous_style {
                        text.restore(style);
                    }
                    return Err(error);
                }
            };
            if let Err(error) = self.resize_text_effect(*index, (texture.width, texture.height)) {
                tracing::warn!("skwd-wall-vk: text effect resize failed: {error:#}");
                self.renderer.destroy_scene_texture(replacement);
                text.reject(wanted, resources.as_ref().map(|resources| resources.revision()));
                if let Some(style) = previous_style {
                    text.restore(style);
                }
                continue;
            }
            if let (Some(resources), Some(object_slot)) = (&resources, object_slot)
                && let Err(error) = resources.replace(object_slot, payload)
            {
                self.renderer.destroy_scene_texture(replacement);
                if let Some(style) = previous_style {
                    text.restore(style);
                }
                return Err(error);
            }
            let previous = std::mem::replace(&mut self.textures[*slot], replacement);
            self.renderer.destroy_scene_texture(previous);
            (scripts.layouts[*index].size, scripts.layouts[*index].offset) = rendered.layout();
            text.accepted(rendered.bounds);
            if let Some(backdrop) =
                scripts.backdrops.iter_mut().find(|backdrop| backdrop.layer == *index)
            {
                backdrop.bounds = rendered.bounds;
                backdrop.padding = text.effect_padding();
            }
            text.shown.clone_from(wanted);
            text.needs_render = false;
            text_layout_changed = true;
        }
        if text_layout_changed {
            let attachments = puppet_attachments(&self.puppets);
            states = frames(scene, &scripts.layouts, self.canvas, properties, &attachments);
        }
        for &TextBackdrop { layer, quad: backdrop, bounds, padding } in &scripts.backdrops {
            let Some(frame) = states[layer].as_ref() else {
                self.quads[backdrop].tint = [0.0; super::RGBA_CHANNELS];
                continue;
            };
            let Some(object) = objects.get(&scripts.layouts[layer].id) else {
                self.quads[backdrop].tint = [0.0; super::RGBA_CHANNELS];
                continue;
            };
            let mut style = Backdrop::of(object, properties);
            if let Some(padding) = padding {
                style.padding = padding;
            }
            update_backdrop(&mut self.quads[backdrop], frame, &style, bounds, self.canvas);
        }
        for (layer, effect) in &mut self.unavailable_effects {
            if let Some(object) = objects.get(layer) {
                effect.apply_object(object, properties);
                if effect.visible {
                    return Err(anyhow::anyhow!(
                        "effect {:?} on layer {layer} is unavailable",
                        effect.source
                    ));
                }
            }
        }
        for puppet in &mut self.puppets {
            if !puppet.puppet.mesh.springs.is_empty()
                && let Some(Some(state)) = states.get(puppet.layer)
            {
                puppet.puppet.planar_owner = state.planar_owner;
                puppet.puppet.world = super::spring_owner(state, self.canvas);
            }
        }
        for (index, state) in states.into_iter().enumerate() {
            let Some(state) = state else {
                continue;
            };
            if state.rect.iter().chain(state.tint.iter()).any(|v| !v.is_finite())
                || !state.angle.is_finite()
            {
                continue;
            }
            for sample in self.image_samples.iter_mut().filter(|sample| sample.layer == index) {
                sample.pose = super::LayerPose::from(&state);
            }
            let custom_base = self.materials.iter_mut().find(|material| material.quad == index);
            let mut has_base = custom_base.is_some();
            let skinned =
                custom_base.as_ref().is_some_and(|material| material.skin_source().is_some());
            if let Some(material) = custom_base {
                material.update(&state, self.canvas, self.camera.is_some());
            }
            let quad = &mut self.quads[index];
            quad.rect = state.rect;
            quad.angle = state.angle;
            quad.projection = state.geometry.map(|geometry| geometry.projection(self.canvas));
            if state.geometry.is_some() {
                self.layer_poses.insert(index, super::LayerPose::from(&state));
            }
            if state.geometry.is_some()
                && let Some(camera) = self.camera
            {
                let (view, ..) = super::authored_perspective(camera, self.canvas, false);
                quad.projection =
                    Some(super::camera_quad(super::LayerPose::from(&state), self.canvas, &view));
                self.mouse.set_pose(index, super::LayerPose::from(&state), self.canvas);
            }
            if state.geometry.is_some()
                && self.camera.is_none()
                && self.mesh_settings.projection == paper_scene::model::ProjectionMode::Other
                && let Some(orientation) = self.mesh_settings.orientation
            {
                let view = super::mat4_mul(&super::scene_ortho(self.canvas, false), &orientation);
                quad.projection =
                    Some(super::camera_quad(super::LayerPose::from(&state), self.canvas, &view));
            }
            self.mouse.set_script_quad(index, quad);
            if let Some(fx) = self.fx.iter_mut().find(|fx| fx.quad == index) {
                has_base |= fx.base_scene_input;
                if fx.base_scene_input {
                    fx.passes
                        .first_mut()
                        .expect("base scene input pass")
                        .set_source_tint(state.source_tint);
                }
                fx.uniforms.extend(super::pose_uniforms(
                    super::LayerPose::from(&state),
                    self.canvas,
                    self.camera.is_some(),
                ));
                let tints = chain_tints(
                    &state,
                    has_base,
                    skinned,
                    fx.passthrough.then_some(fx.copy_background),
                );
                fx.base_quad.tint = tints.base;
                fx.fallback_tint = tints.fallback;
                quad.tint = [1.0, 1.0, 1.0, tints.alpha];
                if let Some(node) = objects.get(&fx.layer_id) {
                    for (pass, values) in fx.passes.iter_mut().zip(&fx.material_values) {
                        pass.apply_object_values(node, properties);
                        pass.apply_resolved_values(values);
                    }
                }
            } else if has_base {
                quad.tint = [1.0, 1.0, 1.0, f32::from(state.visible)];
            } else if self.baked_fx_quads.contains(&index) {
                quad.tint = [1.0, 1.0, 1.0, f32::from(state.tint[super::ALPHA_CHANNEL] > 0.0)];
            } else if scripts.layouts[index].hidden_without_fx {
                quad.tint = [1.0, 1.0, 1.0, 0.0];
            } else {
                quad.tint = state.tint;
            }
        }
        self.mesh_camera(scripts)?;
        self.refresh_cookies(scripts)?;
        Ok(())
    }
}

struct ChainTints {
    base: [f32; 4],
    fallback: [f32; 4],
    alpha: f32,
}

fn chain_tints(
    state: &paper_scene::model::script::Frame,
    has_base: bool,
    skinned: bool,
    passthrough: Option<bool>,
) -> ChainTints {
    let shown = f32::from(state.visible);
    if let Some(copy_background) = passthrough {
        return ChainTints {
            base: [f32::from(copy_background); super::RGBA_CHANNELS],
            fallback: super::HIDDEN_WHITE,
            alpha: shown,
        };
    }
    ChainTints {
        base: if has_base && !skinned { [1.0; super::RGBA_CHANNELS] } else { state.source_tint },
        fallback: if has_base { [1.0, 1.0, 1.0, shown] } else { state.tint },
        alpha: if has_base { shown } else { f32::from(state.tint[super::ALPHA_CHANNEL] > 0.0) },
    }
}

fn script_hits(
    layouts: &[Layout],
    quads: &[crate::vk::SceneQuad],
    point: [f32; 2],
    canvas: (f32, f32),
) -> Vec<String> {
    layouts
        .iter()
        .zip(quads)
        .filter(|(_, quad)| quad_contains(quad, point, canvas))
        .map(|(layout, _)| layout.id.clone())
        .collect()
}

fn quad_contains(quad: &crate::vk::SceneQuad, point: [f32; 2], canvas: (f32, f32)) -> bool {
    let Some(inverse) = super::inverse_projection(super::quad_projection(quad, canvas)) else {
        return false;
    };
    let point = [point[0] * 2.0 / canvas.0 - 1.0, point[1] * 2.0 / canvas.1 - 1.0];
    let [x, y, w] = inverse.map(|row| row[0] * point[0] + row[1] * point[1] + row[3]);
    w != 0.0 && (x / w).abs() <= 0.5 && (y / w).abs() <= 0.5
}
