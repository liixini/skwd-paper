#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub x: f32,
    pub y: f32,
}

impl Affine {
    pub fn new(origin: [f32; 2], scale: [f32; 2], angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self {
            a: cos * scale[0],
            b: sin * scale[0],
            c: -sin * scale[1],
            d: cos * scale[1],
            x: origin[0],
            y: origin[1],
        }
    }

    #[must_use]
    pub fn then(self, child: Self) -> Self {
        Self {
            a: self.a * child.a + self.c * child.b,
            b: self.b * child.a + self.d * child.b,
            c: self.a * child.c + self.c * child.d,
            d: self.b * child.c + self.d * child.d,
            x: self.a * child.x + self.c * child.y + self.x,
            y: self.b * child.x + self.d * child.y + self.y,
        }
    }

    #[expect(clippy::min_ident_chars, reason = "affine inversion coefficients")]
    pub fn inverse(self) -> Option<Self> {
        let (a, b, c, d) =
            (f64::from(self.a), f64::from(self.b), f64::from(self.c), f64::from(self.d));
        let determinant = a * d - b * c;
        let magnitude = a.hypot(b) * c.hypot(d);
        if !determinant.is_finite()
            || !magnitude.is_finite()
            || determinant.abs() <= magnitude * f64::from(f32::EPSILON)
        {
            return None;
        }
        let (a, b, c, d) = (d / determinant, -b / determinant, -c / determinant, a / determinant);
        let inverse = Self {
            a: a as f32,
            b: b as f32,
            c: c as f32,
            d: d as f32,
            x: -(a * f64::from(self.x) + c * f64::from(self.y)) as f32,
            y: -(b * f64::from(self.x) + d * f64::from(self.y)) as f32,
        };
        [inverse.a, inverse.b, inverse.c, inverse.d, inverse.x, inverse.y]
            .iter()
            .all(|value| value.is_finite())
            .then_some(inverse)
    }

    pub fn point(self, point: [f32; 3]) -> [f32; 3] {
        [
            self.a * point[0] + self.c * point[1] + self.x,
            self.b * point[0] + self.d * point[1] + self.y,
            point[2],
        ]
    }
}

impl Default for Affine {
    fn default() -> Self {
        Self::new([0.0; 2], [1.0; 2], 0.0)
    }
}

#[cfg(test)]
mod tests;
