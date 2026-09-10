use super::{MAX_EFFECT_SAMPLERS, MAX_EFFECT_UBO_BYTES, effect_caps};
use paper_scene::shader::{Sampler, Translated};

fn translated(sampler_indices: &[u32], block_size: usize) -> Translated {
    Translated {
        source: String::new(),
        uniforms: Vec::new(),
        samplers: sampler_indices
            .iter()
            .map(|&index| Sampler { name: String::new(), index, default: None, combo: None })
            .collect(),
        block_size,
    }
}

#[test]
fn caps_accept_real_shapes() {
    let vert = translated(&[0], 64);
    let frag = translated(&[0, 1, 7], 256);
    assert_eq!(effect_caps(&vert, &frag).unwrap(), (8, 256));
}

#[test]
fn sampler_index_rejected() {
    let vert = translated(&[], 16);
    let frag = translated(&[u32::MAX], 16);
    assert!(effect_caps(&vert, &frag).is_err());
    let frag = translated(&[MAX_EFFECT_SAMPLERS], 16);
    assert!(effect_caps(&vert, &frag).is_err());
    let frag = translated(&[MAX_EFFECT_SAMPLERS - 1], 16);
    assert!(effect_caps(&vert, &frag).is_ok());
}

#[test]
fn uniform_block_rejected() {
    let vert = translated(&[], 16);
    let frag = translated(&[], MAX_EFFECT_UBO_BYTES + 1);
    assert!(effect_caps(&vert, &frag).is_err());
    let frag = translated(&[], MAX_EFFECT_UBO_BYTES);
    assert!(effect_caps(&vert, &frag).is_ok());
}

#[test]
fn dxc_pipelines_get_a_negative_height_viewport_over_the_same_rows() {
    let extent = ash::vk::Extent2D { width: 640, height: 360 };
    let gl = super::viewport(extent, false);
    let d3d = super::viewport(extent, true);
    assert_eq!((gl.y, gl.height), (0.0, 360.0));
    assert_eq!((d3d.y, d3d.height), (360.0, -360.0));
    assert_eq!(gl.y + gl.height, d3d.y + d3d.height + 360.0);
    assert_eq!((gl.width, d3d.width), (640.0, 640.0));
}
