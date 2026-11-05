use super::super::{parse, tests::fixture};
use super::*;

const BINDS: [[u32; 16]; 2] = [
    [
        0x0000_0000,
        0x4000_0000,
        0x0000_0000,
        0x0000_0000,
        0xbf00_0000,
        0x0000_0000,
        0x0000_0000,
        0x0000_0000,
        0x0000_0000,
        0x0000_0000,
        0x3fc0_0000,
        0x0000_0000,
        0xc0a0_0000,
        0x4040_0000,
        0x3ec0_0000,
        0x3f80_0000,
    ],
    [
        0x3fa0_0000,
        0x0000_0000,
        0x0000_0000,
        0x0000_0000,
        0x0000_0000,
        0x0000_0000,
        0x3f40_0000,
        0x0000_0000,
        0x0000_0000,
        0xc000_0000,
        0x0000_0000,
        0x0000_0000,
        0x3fc0_0000,
        0xc020_0000,
        0x4080_0000,
        0x3f80_0000,
    ],
];
const POSES: [[u32; 9]; 2] = [
    [
        0x3fa0_0000,
        0xc020_0000,
        0x4070_0000,
        0x3e80_0000,
        0xbec0_0000,
        0x3f00_0000,
        0x3fa0_0000,
        0x3f40_0000,
        0x3fc0_0000,
    ],
    [
        0xc060_0000,
        0x3fe0_0000,
        0x3f20_0000,
        0xbf00_0000,
        0x3e80_0000,
        0x3ec0_0000,
        0x3f00_0000,
        0x3fc0_0000,
        0x3fa0_0000,
    ],
];
const BASIS: [[u32; 12]; 2] = [
    [
        0x3f50_e9da,
        0xbf9a_ea9b,
        0xbeb0_cd5f,
        0x3f02_a7d2,
        0x3e8e_c142,
        0x3e6a_6a15,
        0xbe45_7536,
        0xbec6_46c8,
        0x3f66_cdfa,
        0x4077_c732,
        0xc113_e1eb,
        0x3f7f_a1c6,
    ],
    [
        0xbf0a_b3be,
        0xbe4e_3825,
        0x3fc8_71bc,
        0x3e2d_7947,
        0x3e30_60f9,
        0x3ce4_82ed,
        0xbf54_4435,
        0x3f29_9cef,
        0xbf6c_57c7,
        0xbf7a_cff2,
        0xc11c_4a99,
        0x416e_2ad5,
    ],
];

fn spatial_mesh() -> Mesh {
    let mut bytes = fixture();
    let samples = bytes.len() - 35 - 108;
    for frame in 0..3 {
        let rotation = samples + frame * 36 + 12;
        bytes[rotation..rotation + 4].copy_from_slice(&0.25_f32.to_le_bytes());
    }
    parse(&bytes).unwrap()
}

fn layer() -> AnimationLayer {
    AnimationLayer { visible: true, id: 1, rate: 0.0, blend: 1.0, additive: false }
}

#[test]
fn hierarchy_native_bits() {
    let mut mesh = spatial_mesh();
    let compatibility = mesh.bones[0].bind;
    mesh.bones = BINDS
        .into_iter()
        .enumerate()
        .map(|(index, raw)| Bone {
            name: format!("bone{index}"),
            parent: (index == 1).then_some(0),
            raw_bind: raw.map(f32::from_bits),
            bind: compatibility,
            raw_rest: raw.map(f32::from_bits),
            rest: compatibility,
            native_rest: super::super::bind::Bind::new(&raw.map(f32::from_bits)),
        })
        .collect();
    mesh.native_bind_inverse = bind_inverse(&mesh.bones);
    mesh.animations[0].tracks = POSES
        .into_iter()
        .map(|bits| {
            let pose = bits.map(f32::from_bits);
            vec![
                Transform {
                    translation: pose[..3].try_into().unwrap(),
                    rotation: pose[3..6].try_into().unwrap(),
                    scale: pose[6..].try_into().unwrap(),
                };
                3
            ]
        })
        .collect();
    mesh.vertices = vec![Vertex {
        position: [2.0, -3.0, 5.0],
        uv: [0.0; 2],
        bones: [0, 1, 0, 0],
        weights: [0.25, 0.5, 0.0, 0.0],
    }];
    let frame = super::super::skin(&mesh, &[layer()], 0.0);
    for (bone, expected) in frame.bones.iter().zip(BASIS) {
        let Some(BonePose::Spatial(matrix)) = bone else {
            panic!("spatial palette");
        };
        let actual: Vec<u32> = matrix
            .chunks_exact(4)
            .flat_map(|column| column[..3].iter().map(|value| value.to_bits()))
            .collect();
        assert_eq!(actual, expected);
    }
    assert_eq!(frame.positions[0].map(f32::to_bits), [0xc026_d286, 0xc0e9_7cba, 0x40f6_1d67]);
}

#[test]
fn parsed_spatial_consumer() {
    let mesh = spatial_mesh();
    let moving = AnimationLayer { rate: 1.0, ..layer() };
    for time in [0.0, 1.0 / 30.0, 2.0 / 30.0, 3.0 / 30.0] {
        let frame = super::super::skin(&mesh, &[moving], time);
        let Some(BonePose::Spatial(matrix)) = frame.bones[0] else {
            panic!("spatial palette");
        };
        assert_ne!(frame.positions[0][2], 0.0);
        assert_eq!(frame.positions[0], point(&matrix, mesh.vertices[0].position));
    }
}

#[test]
fn unsupported_layers_stay_planar() {
    let mesh = spatial_mesh();
    for layers in [
        vec![AnimationLayer { blend: 0.5, ..layer() }],
        vec![AnimationLayer { additive: true, ..layer() }],
        vec![layer(), layer()],
    ] {
        let frame = super::super::skin(&mesh, &layers, 0.0);
        assert!(matches!(frame.bones[0], Some(BonePose::Planar(_))));
        assert_eq!(frame.positions[0][2], 0.0);
    }
    let mut disabled = spatial_mesh();
    disabled.animations[0].disabled = Some(vec![true]);
    assert!(skin(&disabled, &[layer()], 0.0, Mode::Legacy).is_none());
    disabled.animations[0].disabled = None;
    disabled.animations[0].tracks.clear();
    assert!(skin(&disabled, &[layer()], 0.0, Mode::Legacy).is_none());
}

#[test]
fn invalid_influences_stay_planar() {
    let mut mesh = spatial_mesh();
    for bones in [[u16::MAX, 0, 0, 0], [1, 0, 0, 0]] {
        mesh.vertices[0].bones = bones;
        let frame = super::super::skin(&mesh, &[layer()], 0.0);
        assert!(matches!(frame.bones[0], Some(BonePose::Planar(_))));
        assert_eq!(frame.positions[0], mesh.vertices[0].position);
    }
}

#[test]
fn raw_singular_rejected() {
    let mut mesh = spatial_mesh();
    mesh.bones[0].raw_bind[10] = 0.0;
    mesh.native_bind_inverse = bind_inverse(&mesh.bones);
    assert_eq!(mesh.native_bind_inverse, [None]);
    assert!(skin(&mesh, &[layer()], 0.0, Mode::Legacy).is_none());
}

struct SamplePin {
    first: [u32; 9],
    second: [u32; 9],
    bind: [u32; 16],
    amount: u32,
    basis: [u32; 12],
}

fn transform(bits: [u32; 9]) -> Transform {
    let pose = bits.map(f32::from_bits);
    Transform {
        translation: pose[..3].try_into().unwrap(),
        rotation: pose[3..6].try_into().unwrap(),
        scale: pose[6..].try_into().unwrap(),
    }
}

fn check_pins(pins: &[SamplePin]) {
    for pin in pins {
        for fma in [false, true] {
            let local = sample(
                transform(pin.first),
                transform(pin.second),
                f32::from_bits(pin.amount),
                fma,
            )
            .unwrap()
            .matrix();
            let matrix = compose(&local, &inverse::inverse(&pin.bind.map(f32::from_bits)).unwrap());
            let basis: Vec<u32> = (0..4)
                .flat_map(|col| {
                    let mut input = [0.0; 4];
                    input[col] = 1.0;
                    (0..3).map(move |row| {
                        (((input[0] * matrix[row] + input[1] * matrix[4 + row])
                            + input[2] * matrix[8 + row])
                            + input[3] * matrix[12 + row])
                            .to_bits()
                    })
                })
                .collect();
            assert_eq!(basis, pin.basis);
        }
    }
}

const TRIG_PINS: [SamplePin; 7] = [
    SamplePin {
        first: [
            0x4419_b0ca,
            0xc3a5_bc4d,
            0x0000_0000,
            0x397f_ffff,
            0x3980_0000,
            0xb980_0001,
            0x3f80_0001,
            0x3f80_0001,
            0x3f80_0001,
        ],
        second: [
            0x4419_b0ca,
            0xc3a5_bc4d,
            0x0000_0000,
            0x397f_ffff,
            0x3980_0000,
            0xb980_0001,
            0x3f80_0001,
            0x3f80_0001,
            0x3f80_0001,
        ],
        bind: [
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0xc0ae_0bf6,
            0xc048_6c76,
            0x0000_0000,
            0x3f80_0000,
        ],
        amount: 0x0000_0000,
        basis: [
            0x3f80_0000,
            0xb980_0000,
            0xb980_0000,
            0x3980_0800,
            0x3f80_0000,
            0x397f_fffe,
            0x397f_f000,
            0xb980_07ff,
            0x3f80_0000,
            0x441b_0cef,
            0xc3a4_2ba0,
            0xba13_ab78,
        ],
    },
    SamplePin {
        first: [
            0x4419_b0ca,
            0xc3a5_bc4d,
            0x0000_0000,
            0x3c7f_ffff,
            0x3c80_0000,
            0xbc80_0001,
            0x3f80_0001,
            0x3f80_0001,
            0x3f80_0001,
        ],
        second: [
            0x4419_b0ca,
            0xc3a5_bc4d,
            0x0000_0000,
            0x3c7f_ffff,
            0x3c80_0000,
            0xbc80_0001,
            0x3f80_0001,
            0x3f80_0001,
            0x3f80_0001,
        ],
        bind: [
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0xc0ae_0bf6,
            0xc048_6c76,
            0x0000_0000,
            0x3f80_0000,
        ],
        amount: 0x0000_0000,
        basis: [
            0x3f7f_f002,
            0xbc7f_f557,
            0xbc7f_fd55,
            0x3c81_fa91,
            0x3f7f_efc2,
            0x3c7f_f555,
            0x3c7b_ed6b,
            0xbc81_fa90,
            0x3f7f_f002,
            0x441b_0ffa,
            0xc3a4_366e,
            0xbd13_b030,
        ],
    },
    SamplePin {
        first: [
            0x4419_b0ca,
            0xc3a5_bc4d,
            0x0000_0000,
            0x4974_23ff,
            0x4974_2400,
            0xc974_2401,
            0x3f80_0001,
            0x3f80_0001,
            0x3f80_0001,
        ],
        second: [
            0x4419_b0ca,
            0xc3a5_bc4d,
            0x0000_0000,
            0x4974_23ff,
            0x4974_2400,
            0xc974_2401,
            0x3f80_0001,
            0x3f80_0001,
            0x3f80_0001,
        ],
        bind: [
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0xc0ae_0bf6,
            0xc048_6c76,
            0x0000_0000,
            0x3f80_0000,
        ],
        amount: 0x0000_0000,
        basis: [
            0x3f65_71e4,
            0x3e8b_7926,
            0x3eb3_3258,
            0xbe04_0c95,
            0x3f6a_44b3,
            0xbec3_98d2,
            0xbed9_445f,
            0x3e98_3305,
            0x3f5a_f5d6,
            0x441a_ceee,
            0xc3a3_8fd5,
            0x3f35_0dd6,
        ],
    },
    SamplePin {
        first: [
            0x4419_b0ca,
            0xc3a5_bc4d,
            0x0000_0000,
            0x4bf4_23ff,
            0x4bf4_2400,
            0xcbf4_2401,
            0x3f80_0001,
            0x3f80_0001,
            0x3f80_0001,
        ],
        second: [
            0x4419_b0ca,
            0xc3a5_bc4d,
            0x0000_0000,
            0x4bf4_23ff,
            0x4bf4_2400,
            0xcbf4_2401,
            0x3f80_0001,
            0x3f80_0001,
            0x3f80_0001,
        ],
        bind: [
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0xc0ae_0bf6,
            0xc048_6c76,
            0x0000_0000,
            0x3f80_0000,
        ],
        amount: 0x0000_0000,
        basis: [
            0xbedd_0b6a,
            0xbbf4_6a82,
            0xbf66_e7b2,
            0x3f34_22f6,
            0xbf20_df85,
            0xbea9_c88f,
            0xbf10_7857,
            0xbf47_20e3,
            0x3e8d_9829,
            0x4419_a78a,
            0xc3a6_bd64,
            0xc0be_376d,
        ],
    },
    SamplePin {
        first: [
            0x4419_b0ca,
            0xc3a5_bc4d,
            0x0000_0000,
            0x4c00_0455,
            0x4c00_0456,
            0xcc00_0457,
            0x3f80_0001,
            0x3f80_0001,
            0x3f80_0001,
        ],
        second: [
            0x4419_b0ca,
            0xc3a5_bc4d,
            0x0000_0000,
            0x4c00_0455,
            0x4c00_0456,
            0xcc00_0457,
            0x3f80_0001,
            0x3f80_0001,
            0x3f80_0001,
        ],
        bind: [
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0xc0ae_0bf6,
            0xc048_6c76,
            0x0000_0000,
            0x3f80_0000,
        ],
        amount: 0x0000_0000,
        basis: [
            0xbf27_57a7,
            0x3809_8001,
            0xbf41_bba8,
            0xbf3f_acb5,
            0xbe14_e711,
            0x3f25_9017,
            0xbde1_53ba,
            0x3f7d_476a,
            0x3dc2_b88a,
            0x4418_372f,
            0xc3a5_f691,
            0xc005_ce7b,
        ],
    },
    SamplePin {
        first: [
            0x4419_b0ca,
            0xc3a5_bc4d,
            0x0000_0000,
            0x4fc9_0fda,
            0x4fc9_0fdb,
            0xcfc9_0fdc,
            0x3f80_0001,
            0x3f80_0001,
            0x3f80_0001,
        ],
        second: [
            0x4419_b0ca,
            0xc3a5_bc4d,
            0x0000_0000,
            0x4fc9_0fda,
            0x4fc9_0fdb,
            0xcfc9_0fdc,
            0x3f80_0001,
            0x3f80_0001,
            0x3f80_0001,
        ],
        bind: [
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0xc0ae_0bf6,
            0xc048_6c76,
            0x0000_0000,
            0x3f80_0000,
        ],
        amount: 0x0000_0000,
        basis: [
            0xbef9_9272,
            0xbf0a_25c4,
            0x3f2f_b9e2,
            0xbe94_955d,
            0x3f57_723a,
            0x3ee9_3c42,
            0xbf52_d1f7,
            0x3cbb_28a9,
            0xbf11_1be1,
            0x4418_ccf3,
            0xc3a5_e2a4,
            0x40a5_1f06,
        ],
    },
    SamplePin {
        first: [
            0x4419_b0ca,
            0xc3a5_bc4d,
            0x0000_0000,
            0x7f7f_ffff,
            0xf200_0000,
            0x6000_0000,
            0x3f80_0001,
            0x3f80_0001,
            0x3f80_0001,
        ],
        second: [
            0x4419_b0ca,
            0xc3a5_bc4d,
            0x0000_0000,
            0x7f7f_ffff,
            0xf200_0000,
            0x6000_0000,
            0x3f80_0001,
            0x3f80_0001,
            0x3f80_0001,
        ],
        bind: [
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0xc0ae_0bf6,
            0xc048_6c76,
            0x0000_0000,
            0x3f80_0000,
        ],
        amount: 0x0000_0000,
        basis: [
            0xbf05_54f7,
            0x3cc9_8a3a,
            0xbf5a_7258,
            0xbecf_249c,
            0x3f5f_825f,
            0x3e8b_521c,
            0x3f40_6f92,
            0x3ef9_5199,
            0xbee3_b936,
            0x4418_aa68,
            0xc3a4_4d33,
            0xc072_7e44,
        ],
    },
];

const FRACTION_PINS: [SamplePin; 6] = [
    SamplePin {
        first: [
            0x3fa0_0000,
            0xc020_0000,
            0x4070_0000,
            0x3e80_0000,
            0xbec0_0000,
            0x3f00_0000,
            0x3fa0_0000,
            0x3f40_0000,
            0x3fc0_0000,
        ],
        second: [
            0xc060_0000,
            0x3fe0_0000,
            0x3f20_0000,
            0xbf00_0000,
            0x3e80_0000,
            0x3ec0_0000,
            0x3f00_0000,
            0x3fc0_0000,
            0x3fa0_0000,
        ],
        bind: [
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0xc0ae_0bf6,
            0xc048_6c76,
            0x0000_0000,
            0x3f80_0000,
        ],
        amount: 0x3e80_0000,
        basis: [
            0x3f67_6b95,
            0x3f02_9702,
            0x3e68_60b3,
            0xbef1_3a39,
            0x3f4e_ec81,
            0x3d75_331e,
            0xbe61_efff,
            0xbe6d_f892,
            0x3fb3_5ead,
            0x4060_3e06,
            0x4077_91eb,
            0x408c_7ee4,
        ],
    },
    SamplePin {
        first: [
            0x3fa0_0000,
            0xc020_0000,
            0x4070_0000,
            0x3e80_0000,
            0xbec0_0000,
            0x3f00_0000,
            0x3fa0_0000,
            0x3f40_0000,
            0x3fc0_0000,
        ],
        second: [
            0xc060_0000,
            0x3fe0_0000,
            0x3f20_0000,
            0xbf00_0000,
            0x3e80_0000,
            0x3ec0_0000,
            0x3f00_0000,
            0x3fc0_0000,
            0x3fa0_0000,
        ],
        bind: [
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0xc0ae_0bf6,
            0xc048_6c76,
            0x0000_0000,
            0x3f80_0000,
        ],
        amount: 0x3f00_0000,
        basis: [
            0x3f44_aab2,
            0x3ed5_3f32,
            0x3d36_29d2,
            0xbf06_c7aa,
            0x3f7c_3bb1,
            0xbe08_0c0a,
            0xbe0d_d347,
            0x3de0_ff52,
            0x3fae_8a0e,
            0x3fb3_caa6,
            0x409f_3a36,
            0x4000_da5b,
        ],
    },
    SamplePin {
        first: [
            0x3fa0_0000,
            0xc020_0000,
            0x4070_0000,
            0x3e80_0000,
            0xbec0_0000,
            0x3f00_0000,
            0x3fa0_0000,
            0x3f40_0000,
            0x3fc0_0000,
        ],
        second: [
            0xc060_0000,
            0x3fe0_0000,
            0x3f20_0000,
            0xbf00_0000,
            0x3e80_0000,
            0x3ec0_0000,
            0x3f00_0000,
            0x3fc0_0000,
            0x3fa0_0000,
        ],
        bind: [
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0xc0ae_0bf6,
            0xc048_6c76,
            0x0000_0000,
            0x3f80_0000,
        ],
        amount: 0x3f40_0000,
        basis: [
            0x3f1d_90ff,
            0x3e98_33b1,
            0xbd97_419a,
            0xbf15_21b8,
            0x3f8d_e286,
            0xbec9_4cf4,
            0xbd50_938e,
            0x3ed4_4228,
            0x3f9f_441a,
            0xbf4a_06c8,
            0x40b8_d236,
            0xbe68_21e0,
        ],
    },
    SamplePin {
        first: [
            0x3fa0_0000,
            0xc020_0000,
            0x4070_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3fa0_0000,
            0x3f40_0000,
            0x3fc0_0000,
        ],
        second: [
            0xc060_0000,
            0x3fe0_0000,
            0x3f20_0000,
            0x4080_0000,
            0x3e80_0000,
            0xbec0_0000,
            0x3f00_0000,
            0x3fc0_0000,
            0x3fa0_0000,
        ],
        bind: [
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0xc0ae_0bf6,
            0xc048_6c76,
            0x0000_0000,
            0x3f80_0000,
        ],
        amount: 0x3e80_0000,
        basis: [
            0x3f86_ef33,
            0xbc4d_ac9a,
            0xbe07_52d7,
            0xbd3d_9415,
            0x3f52_94f9,
            0xbee5_0a52,
            0x3e28_f11e,
            0x3f30_7b3c,
            0x3fa0_12e4,
            0x40b4_d665,
            0x3f88_fde5,
            0x3f59_5c8b,
        ],
    },
    SamplePin {
        first: [
            0x3fa0_0000,
            0xc020_0000,
            0x4070_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3fa0_0000,
            0x3f40_0000,
            0x3fc0_0000,
        ],
        second: [
            0xc060_0000,
            0x3fe0_0000,
            0x3f20_0000,
            0x4080_0000,
            0x3e80_0000,
            0xbec0_0000,
            0x3f00_0000,
            0x3fc0_0000,
            0x3fa0_0000,
        ],
        bind: [
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0xc0ae_0bf6,
            0xc048_6c76,
            0x0000_0000,
            0x3f80_0000,
        ],
        amount: 0x3f00_0000,
        basis: [
            0x3f58_4638,
            0xbdb3_a3b8,
            0xbe57_4d82,
            0xbe42_c57c,
            0x3f04_9c66,
            0xbf7a_f909,
            0x3e8b_6493,
            0x3f9b_3b9c,
            0x3f16_8625,
            0x4037_f476,
            0x3f45_27d0,
            0xc001_ad43,
        ],
    },
    SamplePin {
        first: [
            0x3fa0_0000,
            0xc020_0000,
            0x4070_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3fa0_0000,
            0x3f40_0000,
            0x3fc0_0000,
        ],
        second: [
            0xc060_0000,
            0x3fe0_0000,
            0x3f20_0000,
            0x4080_0000,
            0x3e80_0000,
            0xbec0_0000,
            0x3f00_0000,
            0x3fc0_0000,
            0x3fa0_0000,
        ],
        bind: [
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x0000_0000,
            0x3f80_0000,
            0x0000_0000,
            0xc0ae_0bf6,
            0xc048_6c76,
            0x0000_0000,
            0x3f80_0000,
        ],
        amount: 0x3f40_0000,
        basis: [
            0x3f23_96af,
            0xbe27_7109,
            0xbe46_7e4a,
            0xbed2_3651,
            0xbe0a_abed,
            0xbf9e_9fda,
            0x3e83_5c93,
            0x3fa2_4176,
            0xbe64_e336,
            0xbdfb_3420,
            0xbf20_3e60,
            0xc061_d9bc,
        ],
    },
];

#[test]
#[ignore = "requires Intel RSQRT reference profile"]
fn native_trig_basis() {
    use std::arch::x86_64::{_mm_cvtss_f32, _mm_rsqrt_ss, _mm_set_ss};
    let seed = unsafe { _mm_cvtss_f32(_mm_rsqrt_ss(_mm_set_ss(1.0))) };
    assert_eq!(seed.to_bits(), 0x3f7f_f000);
    check_pins(&TRIG_PINS);
}

#[test]
fn native_fraction_basis() {
    check_pins(&FRACTION_PINS);
}

#[test]
fn neutral_keys_stay_spatial() {
    let mut bytes = fixture();
    let middle_rotation = bytes.len() - 35 - 108 + 36 + 12;
    bytes[middle_rotation..middle_rotation + 4].copy_from_slice(&0.25_f32.to_le_bytes());
    let mut spatial = parse(&bytes).unwrap();
    let mut planar = parse(&fixture()).unwrap();
    let moving = AnimationLayer { rate: 1.0, ..layer() };
    for mirror in [false, true] {
        spatial.animations[0].mirror = mirror;
        planar.animations[0].mirror = mirror;
        for frame in [0.0, 0.25, 1.0, 1.75, 2.0, 2.25, 3.75, 4.0] {
            let time = frame * (1.0 / 30.0);
            let pose = super::super::skin(&spatial, &[moving], time);
            assert!(matches!(pose.bones[0], Some(BonePose::Spatial(_))));
            let pose = super::super::skin(&planar, &[moving], time);
            assert!(matches!(pose.bones[0], Some(BonePose::Planar(_))));
        }
    }
}

fn hierarchy_bytes(binds: [Mat4; 2], tracks: [[Transform; 3]; 2]) -> Vec<u8> {
    let mut bytes = fixture();
    let skeleton = bytes.windows(9).position(|bytes| bytes == b"MDLS0004\0").unwrap();
    bytes.truncate(skeleton);
    bytes.extend_from_slice(b"MDLS0004\0");
    let offset = bytes.len();
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    for (parent, bind) in [u32::MAX, 0].into_iter().zip(binds) {
        bytes.push(0);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&parent.to_le_bytes());
        bytes.extend_from_slice(&64_u32.to_le_bytes());
        bytes.extend(bind.into_iter().flat_map(f32::to_le_bytes));
        bytes.push(0);
    }
    let animation = bytes.len() as u32;
    bytes[offset..offset + 4].copy_from_slice(&animation.to_le_bytes());
    bytes.extend_from_slice(b"MDLA0006\0");
    let end_offset = bytes.len();
    for value in [0_u32, 1, 1, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(b"move\0loop\0");
    bytes.extend_from_slice(&30.0_f32.to_le_bytes());
    for value in [2_u32, 0, 2] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for track in tracks {
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&108_u32.to_le_bytes());
        for sample in track {
            bytes.extend(
                sample
                    .translation
                    .into_iter()
                    .chain(sample.rotation)
                    .chain(sample.scale)
                    .flat_map(f32::to_le_bytes),
            );
        }
    }
    bytes.resize(bytes.len() + 35, 0);
    let end = bytes.len() as u32;
    bytes[end_offset..end_offset + 4].copy_from_slice(&end.to_le_bytes());
    bytes
}

const IDENTITY: Mat4 =
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
const TILT: Transform =
    Transform { translation: [0.0; 3], rotation: [0.25, 0.0, 0.0], scale: [1.0; 3] };

#[test]
fn parsed_tilted_hierarchy() {
    let child_bind =
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.75, 0.25, 0.0, 0.0, -0.25, 0.75, 0.0, 2.0, 3.0, 4.0, 1.0];
    let child = Transform { translation: [2.0, 3.0, 4.0], rotation: [0.0, 0.5, 0.0], ..TILT };
    let mut bytes = hierarchy_bytes([IDENTITY, child_bind], [[TILT; 3], [child; 3]]);
    let second_vertex = 32 + 8 + 80 + 40;
    bytes[second_vertex..second_vertex + 4].copy_from_slice(&1_u32.to_le_bytes());
    let mesh = parse(&bytes).unwrap();
    assert_eq!(mesh.bones[1].parent, Some(0));
    assert_eq!(mesh.bones[1].raw_bind, child_bind);
    assert!(mesh.native_bind_inverse.iter().all(Option::is_some));
    let frame = super::super::skin(&mesh, &[layer()], 0.0);
    assert!(frame.bones.iter().all(|bone| matches!(bone, Some(BonePose::Spatial(_)))));
    assert_ne!(frame.positions[1][2], 0.0);
    assert_eq!(frame.positions[1], frame.bones[1].unwrap().point(mesh.vertices[1].position));
    assert_ne!(frame.positions[1], frame.bones[0].unwrap().point(mesh.vertices[1].position));
}

#[test]
fn unused_child_overflow() {
    for (parent_scale, child_scale, bind_scale) in [(1.0e30, 1.0e30, 1.0), (1.0, 1.0e20, 1.0e-20)] {
        let parent = Transform { scale: [1.0, 1.0, parent_scale], ..TILT };
        let child = Transform { scale: [1.0, 1.0, child_scale], ..TILT };
        let mut bind = IDENTITY;
        bind[10] = bind_scale;
        let mesh = parse(&hierarchy_bytes([IDENTITY, bind], [[parent; 3], [child; 3]])).unwrap();
        assert!(mesh.vertices.iter().all(|vertex| vertex.bones == [0; 4]));
        assert!(mesh.native_bind_inverse.iter().all(Option::is_some));
        assert!(skin(&mesh, &[layer()], 0.0, Mode::Legacy).is_none());
        let frame = super::super::skin(&mesh, &[layer()], 0.0);
        assert!(frame.bones.iter().all(|bone| matches!(bone, Some(BonePose::Planar(_)))));
        assert!(frame.positions.iter().flatten().all(|value| value.is_finite()));
        let sampled_child = frame.bones[1].unwrap().point([1.0, 2.0, 3.0]);
        assert!(sampled_child.iter().all(|value| value.is_finite()));
    }
}

#[test]
fn authored_weighted_hierarchy() {
    let parent = Transform { translation: [4.0, 0.0, 0.0], rotation: [0.0; 3], scale: [1.0; 3] };
    let child = Transform { translation: [0.0, 8.0, 0.0], ..parent };
    let mut mesh = parse(&hierarchy_bytes([IDENTITY; 2], [[parent; 3], [child; 3]])).unwrap();
    mesh.vertices[0].position = [2.0, 3.0, 0.0];
    mesh.vertices[0].bones = [0, 1, 0, 0];
    mesh.vertices[0].weights = [0.25, 0.75, 0.0, 0.0];
    let layers = [AnimationLayer { rate: 0.0, ..layer() }];
    let frame = super::super::authored(&mesh, &layers, None).unwrap();
    assert_eq!(frame.positions[0], [6.0, 9.0, 0.0]);
    assert_eq!(frame.bones[0].unwrap().point([2.0, 3.0, 0.0]), [6.0, 3.0, 0.0]);
    assert_eq!(frame.bones[1].unwrap().point([2.0, 3.0, 0.0]), [6.0, 11.0, 0.0]);
    assert_eq!(super::super::authored(&mesh, &layers, Some(0.0)).unwrap(), frame);
    for weights in [[0.0; 4], [-0.25, 1.25, 0.0, 0.0]] {
        mesh.vertices[0].weights = weights;
        assert!(super::super::authored(&mesh, &layers, None).is_err());
    }
    mesh.vertices[0].weights = [0.25, 0.75, 0.0, 0.0];
    mesh.vertices[0].bones[1] = 2;
    assert!(super::super::authored(&mesh, &layers, None).is_err());
}

#[test]
fn sampled_rotation_basis() {
    for fma in [false, true] {
        for angle in [0.0_f32, 0.25, -1.0, std::f32::consts::FRAC_PI_2] {
            let first = Transform {
                translation: [2.0, -3.0, 4.0],
                rotation: [0.0, 0.0, angle],
                scale: [1.0; 3],
            };
            let second = Transform { translation: [6.0, 1.0, 8.0], ..first };
            let matrix = sample(first, second, 0.5, fma).unwrap().matrix();
            let (sine, cosine) = f64::from(angle).sin_cos();
            let expected = [
                cosine, sine, 0.0, 0.0, -sine, cosine, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 4.0, -1.0,
                6.0, 1.0,
            ];
            for (actual, expected) in matrix.into_iter().zip(expected) {
                assert!((f64::from(actual) - expected).abs() <= 8.0 * f64::from(f32::EPSILON));
            }
            for col in 0..3 {
                let norm: f64 = (0..3).map(|row| f64::from(matrix[col * 4 + row]).powi(2)).sum();
                assert!((norm - 1.0).abs() <= 8.0 * f64::from(f32::EPSILON));
            }
            assert_eq!(&matrix[12..], &[4.0, -1.0, 6.0, 1.0]);
        }
    }
}

#[test]
fn authored_raw_weights() {
    let parent = Transform { translation: [4.0, 0.0, 0.0], rotation: [0.0; 3], scale: [1.0; 3] };
    let child = Transform { translation: [0.0, 8.0, 0.0], ..parent };
    let mut mesh = parse(&hierarchy_bytes([IDENTITY; 2], [[parent; 3], [child; 3]])).unwrap();
    mesh.vertices[0].position = [2.0, 3.0, 0.0];
    mesh.vertices[0].bones = [0, 1, 0, 0];
    let layers = [AnimationLayer { rate: 0.0, ..layer() }];
    for (weights, expected) in [
        ([0.5, 0.0, 0.0, 0.0], [3.0, 1.5, 0.0]),
        ([2.0, 0.0, 0.0, 0.0], [12.0, 6.0, 0.0]),
        ([0.5, 0.75, 0.0, 0.0], [7.5, 9.75, 0.0]),
    ] {
        mesh.vertices[0].weights = weights;
        let frame = super::super::authored(&mesh, &layers, None).unwrap();
        assert_eq!(frame.positions[0], expected);
        assert_eq!(mesh.vertices[0].weights, weights);
    }
    for weights in [
        [0.0; 4],
        [-0.5, 1.5, 0.0, 0.0],
        [f32::NAN, 0.0, 0.0, 0.0],
        [f32::INFINITY, 0.0, 0.0, 0.0],
        [f32::MAX, f32::MAX, 0.0, 0.0],
        [f32::MAX, 0.0, 0.0, 0.0],
    ] {
        mesh.vertices[0].weights = weights;
        assert!(super::super::authored(&mesh, &layers, None).is_err());
    }
    mesh.vertices[0].weights = [0.5, 0.0, 0.0, 0.0];
    for (slot, index) in [(0, 2), (1, 2)] {
        mesh.vertices[0].bones = [0, 1, 0, 0];
        mesh.vertices[0].bones[slot] = index;
        assert!(super::super::authored(&mesh, &layers, None).is_err());
    }
}

#[test]
fn authored_sentinel_compatibility() {
    let mut mesh = parse(&fixture()).unwrap();
    mesh.vertices[0].position = [2.0, 3.0, 0.0];
    mesh.vertices[0].bones = [0, u16::MAX, 0, 0];
    let layers = [AnimationLayer { rate: 0.0, blend: 0.0, ..layer() }];
    for weights in [[1.0, 0.0, 0.0, 0.0], [0.5, 0.5, 0.0, 0.0]] {
        mesh.vertices[0].weights = weights;
        let frame = super::super::authored(&mesh, &layers, None).unwrap();
        assert_eq!(frame.positions[0], [2.0, 3.0, 0.0]);
    }
    mesh.vertices[0].weights = [0.5, 0.0, 0.0, 0.0];
    assert!(super::super::authored(&mesh, &layers, None).is_err());
}

#[test]
fn rounded_bind_inverts() {
    let bind = |weight: f32| {
        let matrix = crate::matrix::model_matrix([0.0, 1.0, 0.0], [1.0; 3], [0.0; 3]);
        let root = Bone::from_matrix(String::new(), None, crate::matrix::IDENTITY, true).unwrap();
        let mut child = Bone::from_matrix(String::new(), Some(0), matrix, true).unwrap();
        child.raw_bind[HOMOGENEOUS] = weight;
        bind_inverse(&[root, child])[1].is_some()
    };
    assert!(bind(1.0 - f32::EPSILON / 2.0));
    assert!(!bind(0.5));
}
