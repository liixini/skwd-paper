use serde_json::Value;

#[derive(Clone, Copy, Debug, Default)]
pub struct Parallax {
    pub amount: f32,
    pub influence: f32,
    pub delay: f32,
    pub camera_offset: [f32; 2],
}

impl Parallax {
    fn blend(self, dt: f32) -> f32 {
        if self.delay > 0.0 { ((1.0 - self.delay / 3.0) * 10.0 * dt).min(1.0) } else { 1.0 }
    }

    pub fn position(self, pointer: [f32; 2], previous: [f32; 2], dt: f32) -> [f32; 2] {
        let blend = self.blend(dt);
        std::array::from_fn(|axis| {
            let direction = if axis == 0 { 1.0 } else { -1.0 };
            let target = self.camera_offset[axis]
                + 0.5
                + direction * (pointer[axis].clamp(0.0, 1.0) - 0.5) * self.influence;
            let next = previous[axis] + (target - previous[axis]) * blend;
            if (target - next).abs() < 0.00001 { target } else { next }
        })
    }

    pub fn displacement(self, pointer: [f32; 2], previous: [f32; 2], dt: f32) -> [f32; 2] {
        let blend = self.blend(dt);
        std::array::from_fn(|axis| {
            let target = -(pointer[axis].clamp(0.0, 1.0) - 0.5) * self.amount * self.influence;
            let next = previous[axis] + (target - previous[axis]) * blend;
            if (target - next).abs() < 0.00001 { target } else { next }
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Clock3d {
    pub origin: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct ClockPose {
    pub angles: [f32; 3],
    pub shadow: [f32; 2],
}

impl Clock3d {
    pub fn from_text(text: &Value, origin: [f32; 2]) -> Option<Self> {
        let script: String =
            text.get("script")?.as_str()?.chars().filter(|c| !c.is_whitespace()).collect();
        [
            "thisLayer.origin.subtract(input.cursorWorldPosition)",
            "delta.divide(newVec3(engine.canvasSize,1))",
            "newVec3(delta.y,-delta.x,4*WEMath.mix(delta.x,-delta.x,Math.min(1,Math.max(0,delta.y*0.1+0.5)))).multiply(50)",
            "thisLayer.angles=rotation",
            "shadowLayer.angles=rotation",
            "thisLayer.origin.add(shadowOffset.multiply(0.01))",
            "shadowLayer.text=value",
            "thisScene.createLayer(",
        ].iter().all(|part| script.contains(part)).then_some(Self { origin })
    }

    pub fn pose(self, pointer: [f32; 2], canvas: [f32; 2]) -> ClockPose {
        let world = [pointer[0] * canvas[0], (1.0 - pointer[1]) * canvas[1]];
        let delta = [self.origin[0] - world[0], self.origin[1] - world[1]];
        let x = delta[0] / canvas[0].max(1.0);
        let y = delta[1] / canvas[1].max(1.0);
        let mix = (y * 0.1 + 0.5).clamp(0.0, 1.0);
        ClockPose {
            angles: [
                (y * 50.0).to_radians(),
                (-x * 50.0).to_radians(),
                (200.0 * (x + (-2.0 * x) * mix)).to_radians(),
            ],
            shadow: [delta[0] * 0.01, -delta[1] * 0.01],
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LayerMouse {
    pub parallax: [f32; 2],
    pub clock: Option<Clock3d>,
}

#[cfg(test)]
mod tests;
