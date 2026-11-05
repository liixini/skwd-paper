use super::{
    Group, SceneModel, build_group, locate_pkg, shared, validate_scene_content,
    validate_scene_skips,
};
use anyhow::{Result, ensure};
use paper_geom::FillMode;

pub(super) struct SceneSource<'a> {
    pub shared: &'a shared::SharedDevice,
    pub dir: &'a str,
    pub identity: &'a paper_scene::source::Identity,
    pub properties: &'a paper_scene::model::Properties,
    pub outputs: &'a [(u32, u32)],
    pub mode: FillMode,
    pub strict: bool,
    pub particles_disabled: bool,
}

impl SceneSource<'_> {
    pub fn build(&self) -> Result<Group> {
        let capture = paper_scene::source::Capture::start()?;
        let pkg = paper_scene::pkg::Package::open(&locate_pkg(self.dir)?)?;
        super::validate_native_compatibility(&pkg, self.strict)?;
        let mut model = paper_scene::model::load_from_dir_with(
            &pkg,
            std::path::Path::new(self.dir),
            self.properties,
        )?;
        ensure!(self.identity.matches(&capture.finish()?), "scene source changed since load");
        if self.particles_disabled {
            model.particles.clear();
        }
        validate_scene_content(&model)?;
        validate_scene_skips(self.strict, &model.skipped)?;
        self.build_model(&mut model)
    }

    fn build_model(&self, model: &mut SceneModel) -> Result<Group> {
        ensure!(model.scripts.is_none(), "resized scene gained scripts");
        let group = build_group(self.shared, model, self.strict, self.outputs, self.mode)?;
        if !group.reconstructable() {
            group.destroy();
            anyhow::bail!("resized scene has retained temporal state");
        }
        Ok(group)
    }
}

#[cfg(test)]
#[path = "resize_tests.rs"]
pub(super) mod tests;

pub(super) fn composition_outputs(
    outputs: &[(u32, u32)],
    mode: FillMode,
    span: Option<(u32, u32)>,
) -> Vec<(u32, u32)> {
    if mode == FillMode::Span {
        span.map_or_else(|| outputs.to_vec(), |size| vec![size])
    } else {
        outputs.to_vec()
    }
}

pub(super) fn freezes_composition(
    mode: FillMode,
    shared_composition: bool,
    snapshot_output: Option<&str>,
) -> bool {
    mode == FillMode::Span && shared_composition && snapshot_output.is_none()
}

fn static_texture(texture: &paper_scene::model::Texture) -> bool {
    texture.frames.len() <= 1
        && texture.image_count <= 1
        && texture.video.is_none()
        && texture.system_texture.is_none()
}

fn static_pass(pass: &paper_scene::effects::EffectPass) -> bool {
    !pass.frame_dependent() && pass.textures.iter().flatten().all(static_texture)
}

pub(super) fn static_inputs(model: &SceneModel) -> bool {
    if model.hdr
        || model.scripts.is_some()
        || model.animated_scene.is_some()
        || !model.keyframes.is_empty()
        || !model.particles.is_empty()
        || model.shake.is_some()
        || model.mouse.amount != 0.0
        || model.mouse.influence != 0.0
    {
        return false;
    }
    model.layers.iter().all(|layer| {
        layer.puppet.is_none()
            && layer.live_text.is_none()
            && layer.script_text.is_none()
            && !layer.spritesheet
            && layer.mouse.clock.is_none()
            && layer.mouse.parallax == [0.0; 2]
            && static_texture(&layer.texture)
            && layer.material.as_ref().is_none_or(|material| static_pass(&material.pass))
            && layer.effects.iter().all(|effect| {
                effect.swaps.is_empty()
                    && effect.passes.iter().all(static_pass)
                    && paper_scene::effect_lifetime::plan_targets(
                        &effect.fbos.iter().map(|fbo| fbo.name.clone()).collect::<Vec<_>>(),
                        &effect.passes.iter().map(|pass| pass.target.clone()).collect::<Vec<_>>(),
                        &effect.passes.iter().map(|pass| pass.binds.clone()).collect::<Vec<_>>(),
                        &vec![0; effect.passes.len()],
                    )
                    .is_ok_and(|plan| plan.lifetimes.iter().all(|lifetime| !lifetime.loop_carried))
            })
    }) && model
        .meshes
        .iter()
        .flat_map(|mesh| &mesh.materials)
        .all(|material| static_pass(&material.pass))
}
