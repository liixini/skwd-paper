use super::{Group, vk};
use paper_scene::mouse::{Clock3d, LayerMouse, Parallax, Shake};

pub(super) struct SceneMouse {
    pub enabled: bool,
    pub reads_pointer: bool,
    config: Parallax,
    base: Parallax,
    shake: Shake,
    shake_on: bool,
    authored_camera: bool,
    shake_offset: [f32; 2],
    layers: Vec<MouseLayer>,
    pub position: [f32; 2],
    pub inside: bool,
    previous: [f32; 2],
    pub(super) buttons: [bool; 3],
    displacement: [f32; 2],
    parallax_position: Option<[f32; 2]>,
    revision: u64,
    dirty: bool,
    settling: bool,
    canvas: [f32; 2],
    fov: f32,
    shadows: Vec<(usize, usize)>,
}

#[derive(Clone, Copy)]
struct MouseLayer {
    mouse: LayerMouse,
    rect: [f32; 4],
    projection: Option<[[f32; 4]; 3]>,
    pose: Option<(super::LayerPose, (f32, f32))>,
}

impl MouseLayer {
    fn new(layer: &paper_scene::model::Layer, canvas: (f32, f32)) -> Self {
        Self {
            mouse: layer.mouse,
            rect: [layer.center.0, layer.center.1, layer.size.0, layer.size.1],
            projection: layer.geometry.map(|geometry| geometry.projection(canvas)),
            pose: None,
        }
    }
}

impl SceneMouse {
    pub fn new(model: &paper_scene::model::SceneModel) -> Self {
        let reads_pointer =
            model.scripts.is_some()
                || model.particles.iter().any(|particle| particle.system.follows_mouse())
                || (model.mouse.amount != 0.0
                    && model.mouse.influence != 0.0
                    && model.particles.iter().any(|p| p.parallax != [0.0; 2]))
                || model.layers.iter().any(|layer| {
                    layer.material.as_ref().is_some_and(|material| {
                        paper_scene::effects::PassMeta::of(&material.pass).pointer_dependent()
                    }) || layer.mouse.clock.is_some()
                        || layer.mouse.parallax != [0.0; 2]
                        || layer.effects.iter().flat_map(|effect| &effect.passes).any(|pass| {
                            paper_scene::effects::PassMeta::of(pass).pointer_dependent()
                        })
                });
        Self {
            enabled: reads_pointer || model.shake.is_some(),
            reads_pointer,
            config: model.mouse,
            base: model.mouse,
            shake: model.shake.unwrap_or_default(),
            shake_on: model.shake.is_some(),
            authored_camera: model.camera.is_some(),
            shake_offset: [0.0; 2],
            layers: model.layers.iter().map(|layer| MouseLayer::new(layer, model.canvas)).collect(),
            position: [0.5; 2],
            inside: true,
            previous: [0.5; 2],
            buttons: [false; 3],
            displacement: [0.0; 2],
            parallax_position: model
                .layers
                .iter()
                .flat_map(|layer| {
                    layer
                        .material
                        .iter()
                        .map(|material| &material.pass)
                        .chain(layer.effects.iter().flat_map(|effect| &effect.passes))
                })
                .any(|pass| {
                    pass.fragment
                        .uniforms
                        .iter()
                        .chain(&pass.vertex.uniforms)
                        .any(|u| u.name == "g_ParallaxPosition")
                })
                .then_some(model.mouse.camera_offset.map(|offset| 0.5 + offset)),
            revision: 0,
            dirty: reads_pointer,
            settling: false,
            canvas: [model.canvas.0, model.canvas.1],
            fov: model.camera_fov,
            shadows: Vec::new(),
        }
    }

    pub(super) fn particle_offset(&self, depth: [f32; 2]) -> [f32; 2] {
        [
            depth[0] * self.displacement[0] * self.canvas[0] + self.shake_offset[0],
            -depth[1] * self.displacement[1] * self.canvas[1] - self.shake_offset[1],
        ]
    }

    pub(super) fn particle_pointer(&self, offset: [f32; 2]) -> [f32; 2] {
        [
            self.position[0] * self.canvas[0] - offset[0],
            (1.0 - self.position[1]) * self.canvas[1] - offset[1],
        ]
    }

    pub fn update(
        &mut self,
        revision: u64,
        position: [f32; 2],
        buttons: [bool; 3],
        surface: (u32, u32),
        mode: paper_geom::FillMode,
    ) {
        if !self.enabled {
            return;
        }
        let inside = position.into_iter().all(|value| (0.0..=1.0).contains(&value));
        let (scale, offset) = paper_geom::fill_uv_remap(
            self.canvas[0] as u32,
            self.canvas[1] as u32,
            surface.0,
            surface.1,
            mode,
        );
        let mapped = std::array::from_fn(|axis| {
            if !inside {
                return self.position[axis];
            }
            let value = position[axis] * scale[axis] + offset[axis];
            if mode == paper_geom::FillMode::Tile {
                value.rem_euclid(1.0)
            } else {
                value.clamp(0.0, 1.0)
            }
        });
        if self.revision == revision
            && self.position == mapped
            && self.buttons == buttons
            && self.inside == inside
        {
            return;
        }
        self.revision = revision;
        self.position = mapped;
        self.inside = inside;
        self.buttons = buttons;
        self.dirty = true;
    }

    pub(super) fn set_layer(&mut self, index: usize, layer: &paper_scene::model::Layer) {
        let entry = MouseLayer::new(layer, (self.canvas[0], self.canvas[1]));
        if let Some(previous) = self.layers.get_mut(index) {
            *previous = entry;
        } else {
            self.layers.push(entry);
            for (_, shadow) in &mut self.shadows {
                if *shadow >= index {
                    *shadow += 1;
                }
            }
        }
        self.dirty = true;
    }

    pub fn set_script_quad(&mut self, index: usize, quad: &vk::SceneQuad) {
        if let Some(layer) = self.layers.get_mut(index) {
            layer.rect = quad.rect;
            layer.projection = quad.projection;
        }
    }

    pub(super) fn set_pose(&mut self, index: usize, pose: super::LayerPose, canvas: (f32, f32)) {
        if let Some(layer) = self.layers.get_mut(index) {
            layer.pose = Some((pose, canvas));
        }
    }

    pub fn pending(&self) -> bool {
        self.enabled && (self.dirty || self.settling || self.shake_on)
    }

    pub(super) fn set_fov(&mut self, fov: f32) {
        self.fov = fov;
        self.dirty = true;
    }

    pub(super) fn set_parallax(&mut self, key: &str, flag: Option<bool>, scalar: Option<f32>) {
        match (key, flag, scalar) {
            ("cameraparallax", Some(on), _) => {
                self.config.amount = if !on {
                    0.0
                } else if self.base.amount > 0.0 {
                    self.base.amount
                } else {
                    0.5
                };
            }
            ("cameraparallaxamount", _, Some(amount)) => self.config.amount = amount,
            ("cameraparallaxdelay", _, Some(delay)) => self.config.delay = delay,
            ("cameraparallaxmouseinfluence", _, Some(influence)) => {
                self.config.influence = influence;
            }
            _ => return,
        }
        self.enabled = true;
        self.dirty = true;
        self.settling = true;
    }

    pub(super) fn eye_offset(&self, time: f32) -> Option<[f32; 3]> {
        self.authored_camera.then(|| {
            if self.shake_on { self.shake.eye_offset(time, None) } else { Default::default() }
        })
    }

    pub(super) fn reproject(&mut self, view: &super::Mat4) {
        for layer in &mut self.layers {
            if let Some((pose, canvas)) = layer.pose {
                layer.projection = Some(super::camera_quad(pose, canvas, view));
            }
        }
    }

    pub(super) fn set_shake(&mut self, key: &str, flag: Option<bool>, scalar: Option<f32>) {
        match (key, flag, scalar) {
            ("camerashake", Some(on), _) => {
                self.shake_on = on;
                if !on {
                    self.shake_offset = [0.0; 2];
                }
            }
            ("camerashakeamplitude", _, Some(amplitude)) => self.shake.amplitude = amplitude,
            ("camerashakespeed", _, Some(speed)) => self.shake.speed = speed,
            ("camerashakeroughness", _, Some(roughness)) => self.shake.roughness = roughness,
            _ => return,
        }
        self.enabled = true;
        self.dirty = true;
        self.settling = true;
    }
}

fn rotate(point: [f32; 3], angles: [f32; 3]) -> [f32; 3] {
    let (sx, cx) = angles[0].sin_cos();
    let (sy, cy) = angles[1].sin_cos();
    let (sz, cz) = angles[2].sin_cos();
    let x = point[0] * cz + point[1] * sz;
    let y = -point[0] * sz + point[1] * cz;
    let z = -x * sy + point[2] * cy;
    [x * cy + point[2] * sy, y * cx - z * sx, y * sx + z * cx]
}

fn projection(
    clock: Clock3d,
    layer: MouseLayer,
    canvas: [f32; 2],
    pointer: [f32; 2],
    shadow: bool,
    fov: f32,
) -> [[f32; 4]; 3] {
    let pose = clock.pose(pointer, canvas);
    let [center_x, center_y, width, height] = layer.rect;
    let offset = [center_x - clock.origin[0], canvas[1] - center_y - clock.origin[1], 0.0];
    let center = rotate(offset, pose.angles);
    let (x, y) = layer.projection.map_or(([width, 0.0, 0.0], [0.0, -height, 0.0]), |[x, y, _]| {
        (
            [x[0] * canvas[0] * 0.5, -y[0] * canvas[1] * 0.5, 0.0],
            [x[1] * canvas[0] * 0.5, -y[1] * canvas[1] * 0.5, 0.0],
        )
    });
    let x = rotate(x, pose.angles);
    let y = rotate(y, pose.angles);
    let shift = if shadow { pose.shadow } else { [0.0; 2] };
    let focal = canvas[1] * 0.5 / ((fov * 0.5).to_radians().tan());
    [
        [
            2.0 * x[0] / canvas[0],
            2.0 * y[0] / canvas[0],
            0.0,
            2.0 * (clock.origin[0] + center[0] + shift[0]) / canvas[0] - 1.0,
        ],
        [
            -2.0 * x[1] / canvas[1],
            -2.0 * y[1] / canvas[1],
            0.0,
            1.0 - 2.0 * (clock.origin[1] + center[1] - shift[1]) / canvas[1],
        ],
        [-x[2] / focal, -y[2] / focal, 0.0, 1.0 - center[2] / focal],
    ]
}

fn write_uniforms(values: &mut std::collections::BTreeMap<String, Vec<f32>>, mouse: &SceneMouse) {
    let buttons = [
        f32::from(mouse.buttons[0]),
        f32::from(mouse.buttons[1]),
        f32::from(mouse.buttons[2]),
        0.0,
    ];
    let parallax_position = mouse.parallax_position.unwrap_or([0.5; 2]).map(|v| v.clamp(0.0, 1.0));
    for (name, value) in [
        ("g_PointerPosition", mouse.position.as_slice()),
        ("g_PointerPositionLast", mouse.previous.as_slice()),
        ("g_ParallaxPosition", parallax_position.as_slice()),
        ("g_PointerState", buttons.as_slice()),
    ] {
        if let Some(existing) = values.get_mut(name) {
            existing.clear();
            existing.extend_from_slice(value);
        } else {
            values.insert(name.to_owned(), value.to_vec());
        }
    }
}

impl Group {
    pub(super) fn advance_mouse(&mut self, time: f32, dt: f32) {
        if !self.mouse.enabled {
            return;
        }
        let mouse = &mut self.mouse;
        let before = mouse.displacement;
        mouse.displacement = mouse.config.displacement(mouse.position, before, dt);
        mouse.settling = mouse.displacement != before;
        if mouse.shake_on && !mouse.authored_camera {
            let [x, y, _] = mouse.shake.eye_offset(time, Some(mouse.canvas[1]));
            mouse.shake_offset = [-x, y];
            mouse.settling = true;
        }
        if let Some(position) = &mut mouse.parallax_position {
            let before = *position;
            *position = mouse.config.position(mouse.position, before, dt);
            mouse.settling |= *position != before;
        }
        for (index, layer) in mouse.layers.iter().enumerate() {
            let quad = &mut self.quads[index];
            quad.rect = layer.rect;
            quad.projection = layer.projection;
            for axis in 0..2 {
                quad.rect[axis] +=
                    layer.mouse.parallax[axis] * mouse.displacement[axis] * mouse.canvas[axis]
                        + mouse.shake_offset[axis];
            }
            if let Some(clock) = layer.mouse.clock {
                quad.projection =
                    Some(projection(clock, *layer, mouse.canvas, mouse.position, false, mouse.fov));
            } else if let Some(projection) = &mut quad.projection {
                for (axis, row) in projection.iter_mut().take(2).enumerate() {
                    row[3] += (quad.rect[axis] - layer.rect[axis]) * 2.0 / mouse.canvas[axis];
                }
            }
        }
        if mouse.shadows.is_empty() {
            for (source, layer) in mouse.layers.iter().enumerate() {
                if layer.mouse.clock.is_some() {
                    let index = self.quads.len();
                    self.quads.push(self.quads[source].clone());
                    self.quad_scene_order.push(self.quad_scene_order[source]);
                    mouse.shadows.push((source, index));
                }
            }
        }
        for &(source, index) in &mouse.shadows {
            let layer = mouse.layers[source];
            let mut shadow = self.quads[source].clone();
            shadow.tint = [0.0, 0.0, 0.0, shadow.tint[3]];
            shadow.blend = vk::SceneBlend::Alpha;
            shadow.order_bias = -1;
            shadow.projection = layer.mouse.clock.map(|clock| {
                projection(clock, layer, mouse.canvas, mouse.position, true, mouse.fov)
            });
            self.quads[index] = shadow;
        }
        write_uniforms(&mut self.scene_uniforms, mouse);
        for effect in &mut self.fx {
            write_uniforms(&mut effect.uniforms, mouse);
        }
        mouse.dirty = mouse.previous != mouse.position;
        mouse.previous = mouse.position;
    }
}

#[cfg(test)]
mod tests;
