use serde_json::Value;

#[derive(Clone, Copy)]
pub struct AudioResponse {
    mode: u32,
    exponent: f32,
    bounds: [f32; 2],
    frequencies: [usize; 2],
}

impl AudioResponse {
    pub(super) fn parse(entry: &Value) -> Option<Self> {
        let mode = super::num_of(entry.get("audioprocessingmode"), 0.0) as u32;
        if mode == 0 {
            return None;
        }
        let bounds = super::vec3_of(entry.get("audioprocessingbounds"), [0.8, 1.0, 0.0]);
        let mut frequencies = [
            super::num_of(entry.get("audioprocessingfrequencystart"), 0.0).clamp(0.0, 15.0)
                as usize,
            super::num_of(entry.get("audioprocessingfrequencyend"), 1.0).clamp(0.0, 15.0) as usize,
        ];
        frequencies.sort_unstable();
        Some(Self {
            mode,
            exponent: super::num_of(entry.get("audioprocessingexponent"), 2.0),
            bounds: [bounds[0], bounds[1]],
            frequencies,
        })
    }

    pub(super) fn amount(self, left: &[f32], right: &[f32]) -> f32 {
        let mut peak = 0.0f32;
        for index in self.frequencies[0]..=self.frequencies[1] {
            let left = left.get(index).copied().unwrap_or(0.0);
            let right = right.get(index).copied().unwrap_or(0.0);
            let value = match self.mode {
                1 => left,
                2 => right,
                3 => (left + right) * 0.5,
                _ => 0.0,
            };
            peak = peak.max(value);
        }
        let range = self.bounds[1] - self.bounds[0];
        let amount = if range == 0.0 {
            f32::from(peak > self.bounds[1])
        } else {
            ((peak - self.bounds[0]) / range).clamp(0.0, 1.0)
        };
        (amount * amount * (3.0 - 2.0 * amount)).powf(self.exponent).clamp(0.0, 1.0)
    }
}
