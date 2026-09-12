use super::{
    Layer, Parallax, Properties, ancestor_chain, id_of, number, resolve_transform, truthy, vec3,
};
use serde_json::Value;

pub struct Layout {
    pub id: String,
    pub size: [f32; 2],
    pub offset: [f32; 2],
    pub text: bool,
}

pub struct Frame {
    pub rect: [f32; 4],
    pub angle: f32,
    pub tint: [f32; 4],
    pub scale: [f32; 2],
    pub depth: f32,
    pub text: Option<String>,
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
            resolve_transform(o, &by_id, props, Parallax::of(scene, canvas, props).as_ref())
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
        Self { id: layer.id.clone(), size, offset, text: layer.is_text }
    }
}

pub fn frames(
    scene: &Value,
    layouts: &[Layout],
    canvas: (f32, f32),
    props: &Properties,
) -> Vec<Option<Frame>> {
    let objects = scene["objects"].as_array().map_or(&[][..], Vec::as_slice);
    let by_id: std::collections::HashMap<String, &Value> =
        objects.iter().filter_map(|o| Some((o.get("id").and_then(id_of)?, o))).collect();
    let parallax = Parallax::of(scene, canvas, props);
    layouts
        .iter()
        .map(|layout| {
            let object = by_id.get(&layout.id)?;
            let t = resolve_transform(object, &by_id, props, parallax.as_ref());
            let visible = ancestor_chain(object, &by_id)
                .iter()
                .all(|o| truthy(o.get("visible"), props, true));
            let color = vec3(object.get("color"), props).unwrap_or((1.0, 1.0, 1.0));
            let alpha =
                if visible { number(object.get("alpha"), props, 1.0).clamp(0.0, 1.0) } else { 0.0 };
            let (sin, cos) = (-t.angle).sin_cos();
            let ox = layout.offset[0] * t.scale.0;
            let oy = layout.offset[1] * t.scale.1;
            Some(Frame {
                rect: [
                    t.origin.0 + ox * cos - oy * sin,
                    canvas.1 - t.origin.1 + ox * sin + oy * cos,
                    layout.size[0] * t.scale.0,
                    layout.size[1] * t.scale.1,
                ],
                angle: -t.angle,
                tint: [color.0, color.1, color.2, alpha],
                scale: [t.scale.0, t.scale.1],
                depth: t.origin.2,
                text: layout
                    .text
                    .then(|| crate::text::text_value(object.get("text")).unwrap_or_default()),
            })
        })
        .collect()
}
