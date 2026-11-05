use super::{
    Attachments, Layer, Parallax, Properties, ancestor_chain, id_of, number, resolve_transform,
    vec3,
};
use serde_json::Value;

#[cfg(test)]
mod tests;

pub struct Layout {
    pub id: String,
    pub size: [f32; 2],
    pub offset: [f32; 2],
    pub text: bool,
    pub hidden_without_fx: bool,
    pub puppet: bool,
    pub fullscreen: bool,
}

pub struct Frame {
    pub planar_owner: bool,
    pub origin: [f32; 2],
    pub geometry: Option<super::Geometry>,
    pub visible: bool,
    pub rect: [f32; 4],
    pub angle: f32,
    pub tint: [f32; 4],
    pub source_tint: [f32; 4],
    pub scale: [f32; 2],
    pub depth: f32,
    pub text: Option<String>,
}

pub struct ParticleFrame {
    pub parallax: [f32; 2],
    pub visible: bool,
    origin: (f32, f32, f32),
    angle: f32,
    scale: [f32; 3],
    spatial: Option<crate::matrix::Mat4>,
}

impl ParticleFrame {
    pub fn apply(&self, system: &mut crate::particles::ParticleSystem) {
        let scale =
            if system.world { crate::particles::nonzero_scale(self.scale) } else { self.scale };
        system.origin = self.origin;
        system.angle = self.angle;
        system.spatial = self.spatial.filter(|_| !system.world);
        system.world_matrix = self.spatial.filter(|_| system.world && scale == self.scale);
        system.scale = scale[0];
        system.scale3 = scale;
    }
}

pub fn particle_frames<'a>(
    scene: &Value,
    ids: impl Iterator<Item = &'a str>,
    canvas: (f32, f32),
    props: &Properties,
    attachments: &Attachments<'_>,
) -> Vec<Option<ParticleFrame>> {
    let objects = scene["objects"].as_array().map_or(&[][..], Vec::as_slice);
    let by_id: std::collections::HashMap<String, &Value> =
        objects.iter().filter_map(|o| Some((o.get("id").and_then(id_of)?, o))).collect();
    let parallax = Parallax::of(scene, canvas, props);
    ids.map(|id| {
        let object = by_id.get(id)?;
        Some(particle_frame(object, &by_id, props, parallax.as_ref(), attachments))
    })
    .collect()
}

pub(super) fn particle_frame(
    object: &Value,
    by_id: &std::collections::HashMap<String, &Value>,
    props: &Properties,
    parallax: Option<&Parallax>,
    attachments: &Attachments<'_>,
) -> ParticleFrame {
    let transform = resolve_transform(object, by_id, props, parallax, attachments);
    let ancestors = ancestor_chain(object, by_id);
    let visible = super::visible_chain(&ancestors, props);
    let scale_z = ancestors
        .iter()
        .map(|node| vec3(node.get("scale"), props).map_or(1.0, |scale| scale.2))
        .product();
    ParticleFrame {
        parallax: {
            let root = ancestors.last().copied().unwrap_or(object);
            let depth = super::parallax_depth(root, props);
            [depth.0, depth.1]
        },
        visible,
        origin: transform.origin,
        angle: transform.angle,
        scale: [transform.scale.0, transform.scale.1, scale_z],
        spatial: transform.spatial,
    }
}

impl Layout {
    pub fn new(layer: &Layer, scene: &Value, canvas: (f32, f32), props: &Properties) -> Self {
        let objects = scene["objects"].as_array().map_or(&[][..], Vec::as_slice);
        let by_id =
            objects.iter().filter_map(|o| Some((o.get("id").and_then(id_of)?, o))).collect();
        let object = objects
            .iter()
            .find(|o| o.get("id").and_then(id_of).as_deref() == Some(layer.id.as_str()));
        let transform = object.map(|o| {
            resolve_transform(
                o,
                &by_id,
                props,
                Parallax::of(scene, canvas, props).as_ref(),
                &Attachments::new(),
            )
        });
        let scale = layer.scale;
        let fallback = object
            .and_then(|o| vec3(o.get("size"), props))
            .map_or([layer.texture.width as f32, layer.texture.height as f32], |v| [v.0, v.1]);
        let size = [
            if scale.0.abs() > 0.00001 { layer.size.0 / scale.0 } else { fallback[0] },
            if scale.1.abs() > 0.00001 { layer.size.1 / scale.1 } else { fallback[1] },
        ];
        let offset = transform.map_or([0.0; 2], |t| {
            let dx = layer.center.0 - t.origin.0;
            let dy = layer.center.1 - (canvas.1 - t.origin.1);
            let (sin, cos) = (-t.angle).sin_cos();
            [
                (dx * cos + dy * sin) / if scale.0.abs() > 0.00001 { scale.0 } else { 1.0 },
                (-dx * sin + dy * cos) / if scale.1.abs() > 0.00001 { scale.1 } else { 1.0 },
            ]
        });
        let (size, offset) =
            layer.geometry.map_or((size, offset), |geometry| (geometry.size, geometry.offset));
        Self {
            id: layer.id.clone(),
            size,
            offset,
            text: layer.is_text(),
            hidden_without_fx: layer.passthrough
                || (layer.solid && layer.effects.iter().any(|effect| effect.state.visible)),
            puppet: layer.puppet.is_some(),
            fullscreen: layer.fullscreen,
        }
    }
}

pub fn frames(
    scene: &Value,
    layouts: &[Layout],
    canvas: (f32, f32),
    props: &Properties,
    attachments: &Attachments<'_>,
) -> Vec<Option<Frame>> {
    let objects = scene["objects"].as_array().map_or(&[][..], Vec::as_slice);
    let by_id: std::collections::HashMap<String, &Value> =
        objects.iter().filter_map(|o| Some((o.get("id").and_then(id_of)?, o))).collect();
    let parallax = Parallax::of(scene, canvas, props);
    let hdr = super::hdr::enabled(scene.get(super::GENERAL_FIELD), props);
    let spatial = super::spatial_scene(scene, props);
    layouts
        .iter()
        .map(|layout| {
            let object = by_id.get(&layout.id)?;
            let t = resolve_transform(object, &by_id, props, parallax.as_ref(), attachments)
                .placed(object, spatial && !layout.puppet);
            let ancestors = ancestor_chain(object, &by_id);
            let visible = super::visible_chain(&ancestors, props);
            let planar_owner = ancestors.iter().all(|node| {
                vec3(node.get(super::ANGLES_FIELD), props)
                    .is_none_or(|(x, y, _)| x == 0.0 && y == 0.0)
            });
            let mut color = vec3(object.get("color"), props).unwrap_or((1.0, 1.0, 1.0));
            if hdr && !layout.text {
                let brightness = number(object.get(super::hdr::BRIGHTNESS_FIELD), props, 1.0);
                color = (color.0 * brightness, color.1 * brightness, color.2 * brightness);
            }
            let alpha = number(object.get("alpha"), props, 1.0).clamp(0.0, 1.0);
            let geometry = (!layout.fullscreen).then(|| {
                super::Geometry::new(t, (layout.size[0], layout.size[1]), layout.offset, canvas)
            });
            let rect =
                geometry.map_or([canvas.0 * 0.5, canvas.1 * 0.5, canvas.0, canvas.1], |geometry| {
                    [
                        geometry.transform.x,
                        geometry.transform.y,
                        layout.size[0] * t.scale.0,
                        layout.size[1] * t.scale.1,
                    ]
                });
            Some(Frame {
                planar_owner,
                origin: [t.origin.0, t.origin.1],
                geometry,
                visible,
                rect,
                angle: if layout.fullscreen { 0.0 } else { -t.angle },
                tint: [color.0, color.1, color.2, if visible { alpha } else { 0.0 }],
                source_tint: [color.0, color.1, color.2, alpha],
                scale: [t.scale.0, t.scale.1],
                depth: t.origin.2,
                text: layout
                    .text
                    .then(|| crate::text::text_value(object.get("text")).unwrap_or_default()),
            })
        })
        .collect()
}
