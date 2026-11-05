use super::{
    AUDIO_PREFIX, DAY_TIME_UNIFORM, FRAME_TIME_UNIFORM, FrameClock, IDENTITY_ROTATION,
    LAST_POINTER_UNIFORM, PARALLAX_UNIFORM, POINTER_STATE_UNIFORM, POINTER_UNIFORM, PassMeta,
    SOURCE_COLOR_UNIFORM, TEXTURE_RESOLUTION, TEXTURE_ROTATION, TEXTURE_TRANSLATION, TIME_UNIFORM,
    ZERO_TRANSLATION, texture_uniform,
};
use crate::shader::{Uniform, UniformKind};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
struct Case {
    clock: FrameClock,
    quad: Option<(u32, u32)>,
    width: u32,
    height: u32,
    screen: (u32, u32),
    d3d_clip: bool,
}

fn reference(
    meta: &PassMeta,
    case: Case,
    slot_sizes: &[Option<[f32; 4]>],
    overrides: &BTreeMap<String, Vec<f32>>,
) -> Vec<u8> {
    let Case { clock, quad, width, height, screen, d3d_clip } = case;
    let mut values = meta.constants.clone();
    values.insert(TIME_UNIFORM.into(), vec![clock.time]);
    values.insert(FRAME_TIME_UNIFORM.into(), vec![clock.dt]);
    values.insert(DAY_TIME_UNIFORM.into(), vec![clock.daytime]);
    values.insert(PARALLAX_UNIFORM.into(), vec![0.5, 0.5]);
    values.insert(POINTER_UNIFORM.into(), vec![0.5, 0.5]);
    values.insert(LAST_POINTER_UNIFORM.into(), vec![0.5, 0.5]);
    values.insert(POINTER_STATE_UNIFORM.into(), vec![0.0, 0.0, 0.0, 0.0]);
    values.entry("g_TextureReductionScale".into()).or_insert_with(|| vec![1.0]);
    let (sw, sh) = (screen.0.max(1) as f32, screen.1.max(1) as f32);
    values.entry("g_Screen".into()).or_insert_with(|| vec![sw, sh, sw / sh]);
    values.entry("g_Alpha".into()).or_insert_with(|| vec![1.0]);
    values.entry("g_UserAlpha".into()).or_insert_with(|| vec![1.0]);
    values.entry("g_Brightness".into()).or_insert_with(|| vec![1.0]);
    values.entry("g_Color".into()).or_insert_with(|| vec![1.0, 1.0, 1.0]);
    values.entry(SOURCE_COLOR_UNIFORM.into()).or_insert_with(|| vec![1.0, 1.0, 1.0, 1.0]);
    values.insert("g_TexelSize".into(), vec![1.0 / sw, 1.0 / sh]);
    values.insert("g_TexelSizeHalf".into(), vec![0.5 / sw, 0.5 / sh]);
    let ys = if d3d_clip { 1.0 } else { -1.0 };
    let projection = if let Some((quad_w, quad_h)) = quad {
        let (qw, qh) = ((quad_w.max(1)) as f32, (quad_h.max(1)) as f32);
        Some((
            vec![
                2.0 / qw,
                0.0,
                0.0,
                0.0,
                0.0,
                ys * 2.0 / qh,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                -1.0,
                -ys,
                0.0,
                1.0,
            ],
            vec![
                qw / 2.0,
                0.0,
                0.0,
                0.0,
                0.0,
                ys * qh / 2.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                qw / 2.0,
                qh / 2.0,
                0.0,
                1.0,
            ],
        ))
    } else {
        None
    };
    if let Some((mvp, inverse)) = projection {
        values.insert("g_EffectModelViewProjectionMatrix".into(), mvp.clone());
        values.insert("g_EffectModelViewProjectionMatrixInverse".into(), inverse.clone());
        values.insert("g_ModelViewProjectionMatrix".into(), mvp);
        values.insert("g_ModelViewProjectionMatrixInverse".into(), inverse);
    }
    values.entry("g_ViewProjectionMatrix".into()).or_insert_with(|| {
        vec![
            2.0 / sw,
            0.0,
            0.0,
            0.0,
            0.0,
            ys * 2.0 / sh,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            -1.0,
            -ys,
            0.5,
            1.0,
        ]
    });
    let (w, h) = (width as f32, height as f32);
    values.insert("g_Resolution".into(), vec![w, h, 1.0 / w.max(1.0), 1.0 / h.max(1.0)]);
    for uniform in &meta.uniforms {
        let Some((slot, property)) = texture_uniform(&uniform.name) else {
            continue;
        };
        let metadata = match property {
            TEXTURE_RESOLUTION => slot_sizes
                .get(slot)
                .copied()
                .flatten()
                .or_else(|| meta.resolutions.get(slot).copied().flatten())
                .unwrap_or([w, h, w, h])
                .to_vec(),
            TEXTURE_ROTATION if !values.contains_key(&uniform.name) => IDENTITY_ROTATION.to_vec(),
            TEXTURE_TRANSLATION if !values.contains_key(&uniform.name) => ZERO_TRANSLATION.to_vec(),
            _ => continue,
        };
        values.insert(uniform.name.clone(), metadata);
    }
    for (name, value) in overrides {
        values.insert(name.clone(), value.clone());
    }
    for uniform in &meta.uniforms {
        let Some((base, _)) = uniform.name.split_once('[') else {
            continue;
        };
        if !base.starts_with(AUDIO_PREFIX)
            && !base.starts_with(crate::model::lighting::UNIFORM_PREFIX)
            && !base.starts_with(crate::model::lighting::SHADOW_PREFIX)
            && !base.starts_with(crate::model::lighting::SHADOW_FEATURE_PREFIX)
            && !base.starts_with(crate::model::lighting::spot::PREFIX)
            && !base.starts_with(crate::model::lighting::LEGACY_UNIFORM_PREFIX)
        {
            continue;
        }
        if let Some(bands) = values.get(base).cloned() {
            values.insert(uniform.name.clone(), bands);
        }
    }
    crate::shader::pack_uniforms(&meta.uniforms, &values)
}

const LAYOUT: [(&str, UniformKind, usize); 38] = [
    ("g_Time", UniformKind::Float, 1),
    ("g_Frametime", UniformKind::Float, 1),
    ("g_Daytime", UniformKind::Float, 1),
    ("g_ParallaxPosition", UniformKind::Vec2, 1),
    ("g_PointerPosition", UniformKind::Vec2, 1),
    ("g_PointerPositionLast", UniformKind::Vec2, 1),
    ("g_PointerState", UniformKind::Vec4, 1),
    ("g_TextureReductionScale", UniformKind::Float, 1),
    ("g_Screen", UniformKind::Vec3, 1),
    ("g_Alpha", UniformKind::Float, 1),
    ("g_UserAlpha", UniformKind::Float, 1),
    ("g_Brightness", UniformKind::Float, 1),
    ("g_Color", UniformKind::Vec3, 1),
    ("g_Color4", UniformKind::Vec4, 1),
    ("g_TexelSize", UniformKind::Vec2, 1),
    ("g_TexelSizeHalf", UniformKind::Vec2, 1),
    ("g_EffectModelViewProjectionMatrix", UniformKind::Mat4, 1),
    ("g_EffectModelViewProjectionMatrixInverse", UniformKind::Mat4, 1),
    ("g_ModelViewProjectionMatrix", UniformKind::Mat4, 1),
    ("g_ModelViewProjectionMatrixInverse", UniformKind::Mat4, 1),
    ("g_ViewProjectionMatrix", UniformKind::Mat4, 1),
    ("g_Resolution", UniformKind::Vec4, 1),
    ("g_Texture0Resolution", UniformKind::Vec4, 1),
    ("g_Texture1Resolution", UniformKind::Vec4, 1),
    ("g_Texture15Resolution", UniformKind::Vec4, 1),
    ("g_Texture2Rotation", UniformKind::Vec4, 1),
    ("g_Texture5Rotation", UniformKind::Vec4, 1),
    ("g_Texture3Translation", UniformKind::Vec2, 1),
    ("g_Texture4MipMapInfo", UniformKind::Vec4, 1),
    ("g_AudioSpectrum16Left[16]", UniformKind::Float, 16),
    ("g_LPoint_Color[4]", UniformKind::Vec4, 4),
    ("g_Lights[2]", UniformKind::Vec4, 2),
    ("g_LShadow_Matrix[2]", UniformKind::Mat4, 2),
    ("u_Speed", UniformKind::Float, 1),
    ("u_Count", UniformKind::Uint, 1),
    ("u_Splat", UniformKind::Vec4, 1),
    ("m_Basis", UniformKind::Mat3, 1),
    ("g_Alpha", UniformKind::Float, 1),
];

fn meta() -> PassMeta {
    let mut offset = 0;
    let uniforms = LAYOUT
        .iter()
        .map(|(name, kind, count)| {
            let uniform = Uniform {
                name: (*name).to_string(),
                kind: kind.clone(),
                default: match *name {
                    "u_Speed" => Some(vec![2.0]),
                    "u_Splat" => Some(vec![3.0]),
                    _ => None,
                },
                material: None,
                offset,
                count: *count,
            };
            offset += uniform.std140_span().div_ceil(16) * 16;
            uniform
        })
        .collect();
    PassMeta {
        property_source: None,
        user_uniforms: std::collections::BTreeSet::new(),
        name: String::new(),
        uniforms,
        constants: BTreeMap::from([
            ("g_Alpha".into(), vec![0.25]),
            ("g_Time".into(), vec![99.0]),
            ("g_Texture2Rotation".into(), vec![0.0, 1.0, -1.0, 0.0]),
            ("g_ViewProjectionMatrix".into(), (0..16).map(|value| value as f32).collect()),
            ("u_Speed".into(), vec![5.0]),
            ("g_AudioSpectrum16Left".into(), (0..16).map(|band| band as f32 / 16.0).collect()),
            ("g_LPoint_Color".into(), (0..16).map(|value| value as f32 * 0.5).collect()),
        ]),
        resolutions: vec![None, Some([64.0, 32.0, 60.0, 30.0])],
        ndc: false,
    }
}

#[test]
fn packing_matches_reference() {
    let meta = meta();
    let clock = FrameClock { time: 1.5, dt: 0.016, daytime: 0.25 };
    let overrides = [
        BTreeMap::new(),
        BTreeMap::from([
            ("g_Color".into(), vec![0.1, 0.2, 0.3]),
            ("g_Texture0Resolution".into(), vec![8.0, 4.0, 8.0, 4.0]),
            ("g_Texture3Translation".into(), vec![0.5, -0.5]),
            ("g_AudioSpectrum16Left".into(), vec![0.75; 16]),
            ("g_Resolution".into(), vec![1.0, 2.0, 3.0, 4.0]),
            ("g_ModelViewProjectionMatrix".into(), vec![7.0; 16]),
            ("g_Lights".into(), vec![0.5; 8]),
            ("u_Count".into(), vec![3.0]),
        ]),
    ];
    let slot_sizes = [Some([128.0, 64.0, 100.0, 50.0]), None, None];
    for overrides in &overrides {
        for quad in [None, Some((320, 180)), Some((0, 0))] {
            for screen in [(0, 0), (1920, 1080)] {
                for d3d_clip in [false, true] {
                    let case = Case { clock, quad, width: 640, height: 360, screen, d3d_clip };
                    let packed = meta.uniform_bytes_with(
                        clock,
                        quad,
                        640,
                        360,
                        screen,
                        &slot_sizes,
                        overrides,
                        d3d_clip,
                    );
                    let expected = reference(&meta, case, &slot_sizes, overrides);
                    assert_eq!(packed, expected, "{quad:?} {screen:?} {d3d_clip}");
                }
            }
        }
    }
}
