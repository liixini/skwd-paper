use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FillMode {
    #[default]
    Fill,
    Fit,
    Stretch,
    Center,
    Tile,
    Span,
}

impl FillMode {
    pub const ALL: [Self; 6] =
        [Self::Fill, Self::Fit, Self::Stretch, Self::Center, Self::Tile, Self::Span];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Fit => "fit",
            Self::Stretch => "stretch",
            Self::Center => "center",
            Self::Tile => "tile",
            Self::Span => "span",
        }
    }
}

impl std::str::FromStr for FillMode {
    type Err = ();

    fn from_str(name: &str) -> Result<Self, ()> {
        Self::ALL.into_iter().find(|mode| mode.as_str() == name).ok_or(())
    }
}

pub fn fill_uv_remap(
    src_w: u32,
    src_h: u32,
    target_w: u32,
    target_h: u32,
    fill_mode: FillMode,
) -> ([f32; 2], [f32; 2]) {
    let sw = src_w.max(1) as f32;
    let sh = src_h.max(1) as f32;
    let tw = target_w.max(1) as f32;
    let th = target_h.max(1) as f32;
    let s_aspect = sw / sh;
    let t_aspect = tw / th;
    match fill_mode {
        FillMode::Stretch => ([1.0, 1.0], [0.0, 0.0]),
        FillMode::Fill | FillMode::Span => {
            let [sx, sy, ox, oy] = super::cover_uv(src_w, src_h, target_w, target_h);
            ([sx, sy], [ox, oy])
        }
        FillMode::Fit => {
            if s_aspect > t_aspect {
                let scale = s_aspect / t_aspect;
                ([1.0, scale], [0.0, (1.0 - scale) * 0.5])
            } else {
                let scale = t_aspect / s_aspect;
                ([scale, 1.0], [(1.0 - scale) * 0.5, 0.0])
            }
        }
        FillMode::Center => {
            let sx = tw / sw;
            let sy = th / sh;
            ([sx, sy], [(1.0 - sx) * 0.5, (1.0 - sy) * 0.5])
        }
        FillMode::Tile => ([tw / sw, th / sh], [0.0, 0.0]),
    }
}
