use super::{
    EffectGeometry, MAX_EFFECT_SAMPLERS, MAX_EFFECT_UBO_BYTES, effect_caps, mesh_attributes,
};
use paper_scene::model::mesh::VertexFormat;
use paper_scene::shader::{Sampler, Translated};

fn translated(sampler_indices: &[u32], block_size: usize) -> Translated {
    Translated {
        source: String::new(),
        uniforms: Vec::new(),
        samplers: sampler_indices
            .iter()
            .map(|&index| Sampler {
                comparison: false,
                material: None,
                name: String::new(),
                index,
                default: None,
                combo: None,
                components: Vec::new(),
            })
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
fn indexed_vertex_layout() {
    let source = "layout(location = 0) in vec3 a_Position;\nlayout(location = 1) in vec3 a_Normal;\nlayout(location = 2) in vec2 a_TexCoord;\n";
    let attrs = mesh_attributes(source, EffectGeometry::Mesh(VertexFormat::Standard)).unwrap();
    assert_eq!(attrs.iter().map(|attr| attr.offset).collect::<Vec<_>>(), [0, 12, 24]);
    assert_eq!(attrs.iter().map(|attr| attr.location).collect::<Vec<_>>(), [0, 1, 2]);
    assert!(
        mesh_attributes(
            "layout(location = 0) in vec3 a_Unknown;",
            EffectGeometry::Mesh(VertexFormat::Standard)
        )
        .is_err()
    );
    let extended = "layout(location = 0) in vec3 a_Position;\nlayout(location = 1) in vec3 a_Normal;\nlayout(location = 2) in vec4 a_Tangent4;\nlayout(location = 3) in vec4 a_TexCoordVec4;\n";
    let attrs =
        mesh_attributes(extended, EffectGeometry::Mesh(VertexFormat::TangentLightmap)).unwrap();
    assert_eq!(attrs.iter().map(|attr| attr.offset).collect::<Vec<_>>(), [0, 12, 24, 40]);
    assert!(mesh_attributes(extended, EffectGeometry::Mesh(VertexFormat::Standard)).is_err());
    let tangent = "layout(location = 0) in vec3 a_Position;\nlayout(location = 1) in vec3 a_Normal;\nlayout(location = 2) in vec4 a_Tangent4;\nlayout(location = 3) in vec2 a_TexCoord;\n";
    let attrs = mesh_attributes(tangent, EffectGeometry::Mesh(VertexFormat::Tangent)).unwrap();
    assert_eq!(attrs.iter().map(|attr| attr.offset).collect::<Vec<_>>(), [0, 12, 24, 40]);
    assert_eq!(VertexFormat::Tangent.floats() * std::mem::size_of::<f32>(), 48);
    assert!(mesh_attributes(extended, EffectGeometry::Mesh(VertexFormat::Tangent)).is_err());
    assert!(mesh_attributes(tangent, EffectGeometry::Mesh(VertexFormat::Standard)).is_err());
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

#[test]
fn orthographic_depth_order() {
    let order = crate::vk::DepthOrder::Greater;
    assert_eq!(order.compare().as_raw(), ash::vk::CompareOp::GREATER.as_raw());
    assert_eq!(order.clear(), 0.0);
    let depth = |z: f32| z / 4000.0 + 0.5;
    assert!(depth(100.0) > depth(-100.0));
    assert_eq!(crate::vk::DepthOrder::Less.compare().as_raw(), ash::vk::CompareOp::LESS.as_raw());
    assert_eq!(crate::vk::DepthOrder::Less.clear(), 1.0);
}

#[test]
#[cfg(feature = "shared-device")]
#[ignore = "requires isolated Vulkan qualification and DXC"]
#[expect(clippy::too_many_lines, reason = "one matrix uniform GPU roundtrip transaction")]
fn rectangular_matrix_roundtrip() -> anyhow::Result<()> {
    use crate::vk::{EffectDraw, EffectPipelineSpec, Renderer, SceneBlend};
    use paper_scene::effects::{EffectPass, FrameClock, PassBlend, PassMeta};
    use paper_scene::shader::{Stage, translate, unify_uniforms};
    use std::collections::BTreeMap;

    let shared = crate::shared::test_device();
    let mut renderer = Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        4,
        1,
    )?;
    renderer.ensure_scene_pipelines()?;
    let target = renderer.create_scene_target_fmt(4, 1, false, ash::vk::Format::R8G8B8A8_UNORM)?;
    let cases = [
        ([1.0, 0.0, 0.0, 0.0], [[1.0, 2.0, 3.0], [21.0, 22.0, 23.0]]),
        ([0.0, 1.0, 0.0, 0.0], [[4.0, 5.0, 6.0], [24.0, 25.0, 26.0]]),
        ([0.0, 0.0, 1.0, 0.0], [[7.0, 8.0, 9.0], [27.0, 28.0, 29.0]]),
        ([0.0, 0.0, 0.0, 1.0], [[10.0, 11.0, 12.0], [30.0, 31.0, 32.0]]),
        ([2.0, 3.0, 5.0, 7.0], [[119.0, 136.0, 153.0], [459.0, 476.0, 493.0]]),
    ];
    let values = BTreeMap::from([
        ("g_Before".into(), vec![3.0]),
        (
            "g_Bones[2]".into(),
            vec![
                1.0, 2.0, 3.0, 91.0, 4.0, 5.0, 6.0, 92.0, 7.0, 8.0, 9.0, 93.0, 10.0, 11.0, 12.0,
                94.0, 21.0, 22.0, 23.0, 95.0, 24.0, 25.0, 26.0, 96.0, 27.0, 28.0, 29.0, 97.0, 30.0,
                31.0, 32.0, 98.0,
            ],
        ),
        ("g_After".into(), vec![5.0]),
    ]);
    let fragment_source = "varying vec4 v_Result;\nvarying vec2 v_Uv;\nvoid main() {\n uint bits = floatBitsToUint(v_Result[int(v_Uv.x * 4.0)]);\n gl_FragColor = vec4(bits & 255u, (bits >> 8u) & 255u, (bits >> 16u) & 255u, (bits >> 24u) & 255u) / 255.0;\n}\n";
    let result = (|| {
        for hlsl in [false, true] {
            if hlsl {
                anyhow::ensure!(paper_scene::hlsl::available(), "DXC unavailable");
            }
            let quad = if hlsl {
                renderer.create_quad_buffer_ndc_d3d()?
            } else {
                renderer.create_quad_buffer_ndc()?
            };
            for bone in 0..2 {
                let vertex_source = format!(
                    "uniform float g_Before;\nuniform mat4x3 g_Bones[2];\nuniform float g_After;\nuniform vec4 g_Input;\nattribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec4 v_Result;\nvarying vec2 v_Uv;\nvoid main() {{\n v_Result = vec4(mul(g_Input, g_Bones[{bone}]), g_Before + g_After);\n v_Uv = a_TexCoord;\n gl_Position = vec4(a_Position, 1.0);\n}}\n"
                );
                let combos = BTreeMap::new();
                let mut vertex = translate(&vertex_source, Stage::Vertex, &combos);
                let mut fragment = translate(fragment_source, Stage::Fragment, &combos);
                unify_uniforms(&mut vertex, &mut fragment);
                let pair = hlsl.then(|| {
                    paper_scene::hlsl::rewrite(
                        &vertex_source,
                        &fragment_source.replace("floatBitsToUint", "asuint"),
                        &combos,
                        &vertex.uniforms,
                        &[],
                    )
                });
                let pass = EffectPass {
                    blend: PassBlend::Copy,
                    property_source: None,
                    user_uniforms: std::collections::BTreeSet::new(),
                    name: format!("rectangular-matrix-{hlsl}-{bone}"),
                    vertex,
                    geometry: None,
                    fragment,
                    hlsl: pair,
                    textures: Vec::new(),
                    values: values.clone(),
                    target: None,
                    binds: Vec::new(),
                };
                let pipeline = renderer.create_effect_pipeline_for(
                    &pass.vertex,
                    &pass.fragment,
                    pass.hlsl.as_ref(),
                    &pass.name,
                    EffectPipelineSpec::quad(
                        ash::vk::Format::R8G8B8A8_UNORM,
                        SceneBlend::Copy,
                        None,
                    ),
                )?;
                anyhow::ensure!(pipeline.hlsl == hlsl, "shader backend mismatch");
                let meta = PassMeta::of(&pass);
                for (input, expected) in cases {
                    let overrides = BTreeMap::from([("g_Input".into(), input.to_vec())]);
                    let bytes = meta.uniform_bytes_with(
                        FrameClock::default(),
                        None,
                        4,
                        1,
                        (4, 1),
                        &[],
                        &overrides,
                        hlsl,
                    );
                    renderer.run_effect_pass(
                        &pipeline,
                        &quad,
                        EffectDraw { target: &target, preserve: false, final_vertices: false },
                        &[],
                        &bytes,
                    )?;
                    let (width, height, pixels) = renderer.read_scene_target(&target)?;
                    anyhow::ensure!((width, height, pixels.len()) == (4, 1, 16), "readback shape");
                    let [x, y, z] = expected[bone];
                    for (pixel, expected) in pixels.chunks_exact(4).zip([x, y, z, 8.0_f32]) {
                        anyhow::ensure!(
                            pixel == expected.to_bits().to_le_bytes(),
                            "matrix bytes {pixel:?}, expected {expected}, backend {hlsl}, bone {bone}, input {input:?}"
                        );
                    }
                }
            }
        }
        Ok(())
    })();
    renderer.destroy_scene_target(target);
    result
}

#[test]
#[cfg(feature = "shared-device")]
#[ignore = "requires isolated Vulkan qualification and DXC"]
fn packed_varying_roundtrip() -> anyhow::Result<()> {
    use crate::vk::{EffectDraw, EffectPipelineSpec, Renderer, SceneBlend};
    use paper_scene::shader::{Stage, translate, unify_uniforms};
    use std::collections::BTreeMap;

    anyhow::ensure!(paper_scene::hlsl::available(), "DXC unavailable");
    let shared = crate::shared::test_device();
    let mut renderer = Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        4,
        1,
    )?;
    renderer.ensure_scene_pipelines()?;
    let target = renderer.create_scene_target_fmt(4, 1, false, ash::vk::Format::R8G8B8A8_UNORM)?;
    let result = (|| {
        for (ty, count, value, color) in [
            ("float", 32, "float(i) / 49.0", "vec4(v_Array[tap], 0.0, 0.0, 1.0)"),
            (
                "vec2",
                49,
                "vec2(float(i) / 49.0, float(49 - i) / 49.0)",
                "vec4(v_Array[tap], 0.0, 1.0)",
            ),
            (
                "vec3",
                40,
                "vec3(float(i) / 49.0, float(49 - i) / 49.0, float(i % 4) / 4.0)",
                "vec4(v_Array[tap], 1.0)",
            ),
        ] {
            let vert_source = format!(
                "attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_Uv;\nvarying {ty} v_Array[{count}];\nvoid fill() {{ for (int i = 0; i < {count}; i++) v_Array[i] = {value}; }}\nvoid main() {{ gl_Position = vec4(a_Position, 1.0); v_Uv = a_TexCoord; fill(); }}\n"
            );
            let frag_source = format!(
                "varying vec2 v_Uv;\nvarying {ty} v_Array[{count}];\nvoid main() {{ int idx = int(v_Uv.x * 4.0); int tap = idx == 3 ? {last} : idx == 2 ? {middle} : idx; gl_FragColor = {color}; }}\n",
                last = count - 1,
                middle = count / 2,
            );
            let combos = BTreeMap::new();
            let mut vertex = translate(&vert_source, Stage::Vertex, &combos);
            let mut fragment = translate(&frag_source, Stage::Fragment, &combos);
            unify_uniforms(&mut vertex, &mut fragment);
            let pair = paper_scene::hlsl::rewrite(&vert_source, &frag_source, &combos, &[], &[]);
            let mut backends = Vec::new();
            for hlsl in [false, true] {
                let quad = if hlsl {
                    renderer.create_quad_buffer_ndc_d3d()?
                } else {
                    renderer.create_quad_buffer_ndc()?
                };
                let pipeline = renderer.create_effect_pipeline_for(
                    &vertex,
                    &fragment,
                    hlsl.then_some(&pair),
                    "packed-varying",
                    EffectPipelineSpec::quad(
                        ash::vk::Format::R8G8B8A8_UNORM,
                        SceneBlend::Copy,
                        None,
                    ),
                )?;
                anyhow::ensure!(pipeline.hlsl == hlsl, "shader backend mismatch");
                renderer.run_effect_pass(
                    &pipeline,
                    &quad,
                    EffectDraw { target: &target, preserve: false, final_vertices: false },
                    &[],
                    &[],
                )?;
                let (width, height, pixels) = renderer.read_scene_target(&target)?;
                anyhow::ensure!((width, height, pixels.len()) == (4, 1, 16), "readback shape");
                let taps = [0, 1, count / 2, count - 1];
                for (pixel, tap) in pixels.chunks_exact(4).zip(taps) {
                    let red = (tap as f32 / 49.0 * 255.0).round() as u8;
                    let green = if ty == "float" {
                        0
                    } else {
                        ((49 - tap) as f32 / 49.0 * 255.0).round() as u8
                    };
                    let blue = if ty == "vec3" {
                        ((tap % 4) as f32 / 4.0 * 255.0).round() as u8
                    } else {
                        0
                    };
                    anyhow::ensure!(
                        pixel
                            .iter()
                            .zip([red, green, blue, 255])
                            .all(|(actual, expected)| actual.abs_diff(expected) <= 1),
                        "color {pixel:?}, tap {tap}, {ty}, hlsl {hlsl}"
                    );
                }
                backends.push(pixels);
            }
            anyhow::ensure!(backends[0] == backends[1], "backend pixels differ for {ty}");
        }
        Ok(())
    })();
    renderer.destroy_scene_target(target);
    result
}

#[test]
#[cfg(feature = "shared-device")]
#[ignore = "requires isolated Vulkan qualification and DXC"]
fn packed_field_roundtrip() -> anyhow::Result<()> {
    use crate::vk::{EffectDraw, EffectPipelineSpec, Renderer, SceneBlend};
    use paper_scene::shader::{Stage, translate, unify_uniforms};
    use std::collections::BTreeMap;
    use std::fmt::Write;

    anyhow::ensure!(paper_scene::hlsl::available(), "DXC unavailable");
    let mut vert =
        String::from("attribute vec3 a_Position;\nvarying vec3 v_A;\nvarying vec2 v_B;\n");
    let mut frag = String::from("varying vec3 v_A;\nvarying vec2 v_B;\n");
    for index in 0..34 {
        let _ = writeln!(vert, "varying float v_{index};");
        let _ = writeln!(frag, "varying float v_{index};");
    }
    vert.push_str("void main() { gl_Position = vec4(a_Position, 1.0); v_A = vec3(0.1, 0.2, 0.3); v_B = vec2(0.4, 0.5);");
    for index in 0..34 {
        let _ = write!(vert, " v_{index} = float({index}) / 34.0;");
    }
    vert.push_str(" }\n");
    frag.push_str("void main() { gl_FragColor = vec4(v_A.z, v_B.y, v_33, 1.0); }\n");
    let combos = BTreeMap::new();
    let mut vertex = translate(&vert, Stage::Vertex, &combos);
    let mut fragment = translate(&frag, Stage::Fragment, &combos);
    unify_uniforms(&mut vertex, &mut fragment);
    let pair = paper_scene::hlsl::rewrite(&vert, &frag, &combos, &[], &[]);
    let shared = crate::shared::test_device();
    let mut renderer = Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        4,
        1,
    )?;
    renderer.ensure_scene_pipelines()?;
    let target = renderer.create_scene_target_fmt(4, 1, false, ash::vk::Format::R8G8B8A8_UNORM)?;
    let result = (|| {
        let quad = renderer.create_quad_buffer_ndc_d3d()?;
        let pipeline = renderer.create_effect_pipeline_for(
            &vertex,
            &fragment,
            Some(&pair),
            "packed-fields",
            EffectPipelineSpec::quad(ash::vk::Format::R8G8B8A8_UNORM, SceneBlend::Copy, None),
        )?;
        anyhow::ensure!(pipeline.hlsl, "HLSL fallback");
        renderer.run_effect_pass(
            &pipeline,
            &quad,
            EffectDraw { target: &target, preserve: false, final_vertices: false },
            &[],
            &[],
        )?;
        let (width, height, pixels) = renderer.read_scene_target(&target)?;
        anyhow::ensure!((width, height, pixels.len()) == (4, 1, 16), "readback shape");
        for pixel in pixels.chunks_exact(4) {
            for (actual, expected) in pixel.iter().zip([77_u8, 128, 248, 255]) {
                anyhow::ensure!(actual.abs_diff(expected) <= 1, "field pixel {pixel:?}");
            }
        }
        Ok(())
    })();
    renderer.destroy_scene_target(target);
    result
}

#[path = "puppet_tests.rs"]
mod puppet;

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn descriptor_pools_grow() -> anyhow::Result<()> {
    use super::{EffectPipelineSpec, Renderer};
    use paper_scene::shader::{Stage, translate};

    let shared = crate::shared::test_device();
    let mut renderer = Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        4,
        4,
    )?;
    let vertex = translate(
        "attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}",
        Stage::Vertex,
        &std::collections::BTreeMap::new(),
    );
    let fragment = translate(
        "uniform vec4 g_Tint;\nvoid main(){gl_FragColor=g_Tint;}",
        Stage::Fragment,
        &std::collections::BTreeMap::new(),
    );
    let target = renderer.create_scene_target(4, 4)?;
    let spec = EffectPipelineSpec::quad(target.format, crate::vk::SceneBlend::Copy, None);
    let held = (0..=super::super::scene::EFFECT_POOL_SETS)
        .map(|_| renderer.create_effect_pipeline_for(&vertex, &fragment, None, "pool growth", spec))
        .collect::<anyhow::Result<Vec<_>>>();
    let pools = renderer.fx_pools.len();
    renderer.destroy_scene_target(target);
    drop(held?);
    assert_eq!(pools, 2);
    Ok(())
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn descriptor_pool_reuses_slots() -> anyhow::Result<()> {
    use super::{EffectPipelineSpec, Renderer};
    use paper_scene::shader::{Stage, translate};

    let shared = crate::shared::test_device();
    let mut renderer = Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        4,
        4,
    )?;
    let vertex = translate(
        "attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_Uv;\nvoid main(){v_Uv=a_TexCoord;gl_Position=vec4(a_Position,1.0);}",
        Stage::Vertex,
        &std::collections::BTreeMap::new(),
    );
    let fragment = translate(
        "varying vec2 v_Uv;\nuniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_Uv);}",
        Stage::Fragment,
        &std::collections::BTreeMap::new(),
    );
    let target = renderer.create_scene_target(4, 4)?;
    let spec = EffectPipelineSpec::quad(target.format, crate::vk::SceneBlend::Copy, None);
    let pipeline =
        renderer.create_effect_pipeline_for(&vertex, &fragment, None, "descriptor reuse", spec)?;
    assert!(paper_scene::shader::cached_spirv(&vertex.source, "vert").is_some());
    assert!(paper_scene::shader::cached_spirv(&fragment.source, "frag").is_some());
    let baseline = Renderer::effect_live_allocations();
    for _ in 0..=super::super::scene::EFFECT_POOL_SETS {
        let transient = renderer.create_effect_pipeline_for(
            &vertex,
            &fragment,
            None,
            "descriptor reuse",
            spec,
        )?;
        renderer.destroy_effect_pipeline(transient);
    }
    assert_eq!(Renderer::effect_live_allocations(), baseline);
    renderer.ensure_scene_pool(1)?;
    let texture = renderer.create_scene_texture(1, 1, &[17, 83, 211, 255])?;
    let vertices = renderer.create_quad_buffer_ndc()?;
    renderer.run_effect_pass(
        &pipeline,
        &vertices,
        crate::vk::EffectDraw { target: &target, preserve: false, final_vertices: false },
        &[(texture.view, texture.sampler)],
        &[],
    )?;
    assert!(
        renderer
            .read_scene_target(&target)?
            .2
            .chunks_exact(4)
            .all(|pixel| pixel == [17, 83, 211, 255])
    );
    renderer.destroy_quad_buffer(vertices);
    renderer.destroy_scene_texture(texture);
    renderer.destroy_effect_pipeline(pipeline);
    renderer.destroy_scene_target(target);
    Ok(())
}

#[test]
#[cfg(feature = "shared-device")]
#[ignore = "requires isolated Vulkan qualification and DXC"]
fn bounded_varying_roundtrip() -> anyhow::Result<()> {
    use crate::vk::{EffectDraw, EffectPipelineSpec, Renderer, SceneBlend};
    use paper_scene::shader::{Stage, translate, unify_uniforms};
    use std::collections::BTreeMap;

    anyhow::ensure!(paper_scene::hlsl::available(), "DXC unavailable");
    let shared = crate::shared::test_device();
    let mut renderer = Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        4,
        1,
    )?;
    renderer.ensure_scene_pipelines()?;
    let target = renderer.create_scene_target_fmt(4, 1, false, ash::vk::Format::R8G8B8A8_UNORM)?;
    let result = (|| {
        for count in [32, 64] {
            let vert = format!(
                "attribute vec3 a_Position;\nvarying vec4 bins[{count}];\nvoid main() {{ int index; for(index=0;index<{count};index+=4) {{ bins[uint(index)/uint(4)]=CAST4(float(index)/float({count})); }} gl_Position=vec4(a_Position,1); }}"
            );
            let frag = format!(
                "varying vec4 bins[{count}];\nvoid main() {{ float frequency,amplitude,total=0; for(int index=0;index<{count};index+=1) {{ frequency=float(index); amplitude=bins[int(frequency*0.25)][index%int(4)]; total+=amplitude; }} gl_FragColor=vec4(total/float({count}),0,0,1); }}"
            );
            let combos = BTreeMap::new();
            anyhow::ensure!(
                paper_scene::hlsl::varying_slots(&vert, &frag, &combos) == count / 4,
                "varying prefix"
            );
            let mut vertex = translate(&vert, Stage::Vertex, &combos);
            let mut fragment = translate(&frag, Stage::Fragment, &combos);
            unify_uniforms(&mut vertex, &mut fragment);
            let pair = paper_scene::hlsl::rewrite(&vert, &frag, &combos, &[], &[]);
            let quad = renderer.create_quad_buffer_ndc_d3d()?;
            let pipeline = renderer.create_effect_pipeline_for(
                &vertex,
                &fragment,
                Some(&pair),
                "bounded-varying",
                EffectPipelineSpec::quad(ash::vk::Format::R8G8B8A8_UNORM, SceneBlend::Copy, None),
            )?;
            anyhow::ensure!(pipeline.hlsl, "HLSL fallback");
            renderer.run_effect_pass(
                &pipeline,
                &quad,
                EffectDraw { target: &target, preserve: false, final_vertices: false },
                &[],
                &[],
            )?;
            let (width, height, pixels) = renderer.read_scene_target(&target)?;
            anyhow::ensure!((width, height, pixels.len()) == (4, 1, 16), "readback shape");
            let expected = if count == 32 { 112_u8 } else { 120_u8 };
            for pixel in pixels.chunks_exact(4) {
                anyhow::ensure!(
                    pixel[0].abs_diff(expected) <= 1 && pixel[1..] == [0, 0, 255],
                    "bounded pixel {pixel:?}, count {count}"
                );
            }
        }
        Ok(())
    })();
    renderer.destroy_scene_target(target);
    result
}

static REAL_IDLE: std::sync::OnceLock<ash::vk::PFN_vkDeviceWaitIdle> = std::sync::OnceLock::new();
static COUNTED_IDLES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

unsafe extern "system" fn counted_idle(device: ash::vk::Device) -> ash::vk::Result {
    COUNTED_IDLES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    unsafe { REAL_IDLE.get().expect("real idle")(device) }
}

fn counted_device(shared: &crate::shared::SharedDevice) -> ash::Device {
    let lookup = shared.instance.fp_v1_0().get_device_proc_addr;
    let handle = shared.device.handle();
    unsafe {
        ash::Device::load_with(
            |name| {
                let real = lookup(handle, name.as_ptr());
                if name == c"vkDeviceWaitIdle" {
                    REAL_IDLE.get_or_init(|| std::mem::transmute(real.expect("vkDeviceWaitIdle")));
                    return counted_idle as *const std::ffi::c_void;
                }
                real.map_or(std::ptr::null(), |function| function as *const std::ffi::c_void)
            },
            handle,
        )
    }
}

fn counted_renderer(shared: &crate::shared::SharedDevice) -> anyhow::Result<super::Renderer> {
    super::Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            counted_device(shared),
            shared.gfx_family,
            shared.queue,
        ),
        4,
        4,
    )
}

#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn owned_destroy_skips_idle() -> anyhow::Result<()> {
    use super::Renderer;

    let shared = crate::shared::test_device();
    let mut renderer = counted_renderer(&shared)?;
    let effects = Renderer::effect_live_allocations();
    let targets = Renderer::scene_target_live_allocations();
    let target = renderer.create_scene_target(4, 4)?;
    let quad = renderer.create_quad_buffer_ndc()?;
    let buffer = renderer.create_dyn_buffer(64, ash::vk::BufferUsageFlags::VERTEX_BUFFER)?;
    crate::shared::submit(
        &renderer.device,
        shared.queue,
        shared.gfx_family,
        &[],
        ash::vk::Fence::null(),
    )?;
    let idles = COUNTED_IDLES.load(std::sync::atomic::Ordering::SeqCst);
    renderer.destroy_quad_buffer(quad);
    renderer.destroy_dyn_buffer(buffer);
    renderer.destroy_scene_target(target);
    assert_eq!(COUNTED_IDLES.load(std::sync::atomic::Ordering::SeqCst), idles);
    assert_eq!(Renderer::effect_live_allocations(), effects);
    assert_eq!(Renderer::scene_target_live_allocations(), targets);
    Ok(())
}

#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn unsubmitted_fence_skips_wait() -> anyhow::Result<()> {
    let shared = crate::shared::test_device();
    let renderer = counted_renderer(&shared)?;
    let quad = renderer.create_quad_buffer_ndc()?;
    unsafe { renderer.device.reset_fences(&[renderer.fence])? };
    renderer.destroy_quad_buffer(quad);
    renderer.wait_render()?;
    Ok(())
}
