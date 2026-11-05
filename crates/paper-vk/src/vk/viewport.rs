const CROP_TOLERANCE: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneViewport {
    pub canvas: [f32; 2],
    pub source: [f32; 4],
    pub raster: [u32; 2],
}

impl SceneViewport {
    pub fn cover(canvas: [f32; 2], raster: [u32; 2]) -> Option<Self> {
        if canvas.iter().any(|value| !value.is_finite() || *value <= 0.0) || raster.contains(&0) {
            return None;
        }
        let scale = (raster[0] as f32 / canvas[0]).max(raster[1] as f32 / canvas[1]);
        let width = raster[0] as f32 / scale;
        let height = raster[1] as f32 / scale;
        let source = [(canvas[0] - width) * 0.5, (canvas[1] - height) * 0.5, width, height];
        let view = Self { canvas, source, raster };
        view.source.iter().all(|value| value.is_finite()).then_some(view)
    }

    pub fn authored_viewport(self) -> [f32; 4] {
        let [x, y, width, height] = self.source;
        let scale = [self.raster[0] as f32 / width, self.raster[1] as f32 / height];
        [-x * scale[0], -y * scale[1], self.canvas[0] * scale[0], self.canvas[1] * scale[1]]
    }

    pub fn whole(canvas: [f32; 2]) -> Self {
        Self {
            canvas,
            source: [0.0, 0.0, canvas[0], canvas[1]],
            raster: canvas.map(|extent| extent.round() as u32),
        }
    }

    pub fn uncropped(self) -> bool {
        self.source[..2].iter().all(|offset| offset.abs() < CROP_TOLERANCE)
    }

    pub fn same_outputs(self, outputs: &[(u32, u32)]) -> bool {
        !outputs.is_empty() && outputs.iter().all(|&(width, height)| self.raster == [width, height])
    }

    pub fn supported(self, limits: &ash::vk::PhysicalDeviceLimits) -> bool {
        let [x, y, width, height] = self.authored_viewport();
        let [min, max] = limits.viewport_bounds_range;
        [x, y, width, height].iter().all(|value| value.is_finite())
            && width > 0.0
            && height > 0.0
            && width <= limits.max_viewport_dimensions[0] as f32
            && height <= limits.max_viewport_dimensions[1] as f32
            && x >= min
            && y >= min
            && x + width <= max
            && y + height <= max
    }
}

#[cfg(test)]
#[path = "viewport_tests.rs"]
mod tests;
