use super::super::{EffectGeometry, PUPPET_VERTEX_BYTES, mesh_attributes};

#[test]
fn puppet_input_contract() {
    let source = "layout(location = 5) in vec3 a_Position;\nlayout(location = 3) in vec3 a_Normal;\nlayout(location = 1) in vec4 a_Tangent4;\nlayout(location = 4) in uvec4 a_BlendIndices;\nlayout(location = 2) in vec4 a_BlendWeights;\nlayout(location = 0) in vec2 a_TexCoord;\nlayout(location = 0) out vec2 v_TexCoord;\n";
    let attrs = mesh_attributes(source, EffectGeometry::Puppet).unwrap();
    assert_eq!(PUPPET_VERTEX_BYTES, 80);
    assert_eq!(attrs.iter().map(|attr| attr.location).collect::<Vec<_>>(), [5, 3, 1, 4, 2, 0]);
    assert_eq!(attrs.iter().map(|attr| attr.offset).collect::<Vec<_>>(), [0, 12, 24, 40, 56, 72]);
    assert!(attrs.iter().all(|attr| attr.binding == 0));
    assert_eq!(attrs[3].format.as_raw(), ash::vk::Format::R32G32B32A32_UINT.as_raw());
    assert_eq!(attrs[4].format.as_raw(), ash::vk::Format::R32G32B32A32_SFLOAT.as_raw());
    for invalid in [source.replace("uvec4", "vec4"), source.replace("a_BlendWeights", "a_Unknown")]
    {
        assert!(mesh_attributes(&invalid, EffectGeometry::Puppet).is_err());
    }
}

#[test]
fn translated_puppet_inputs() {
    use paper_scene::shader::{Stage, UniformKind, translate};
    use std::collections::BTreeMap;

    let source = "uniform mat4x3 g_Bones[2];\nattribute vec3 a_Position;\nattribute uvec4 a_BlendIndices;\nattribute vec4 a_BlendWeights;\nattribute vec2 a_TexCoord;\nvoid main() { gl_Position = vec4(mul(vec4(a_Position, 1.0), g_Bones[a_BlendIndices.x]) * a_BlendWeights.x, 1.0); }\n";
    let vertex = translate(source, Stage::Vertex, &BTreeMap::new());
    let attrs = mesh_attributes(&vertex.source, EffectGeometry::Puppet).unwrap();
    assert_eq!(attrs.iter().map(|attr| attr.offset).collect::<Vec<_>>(), [0, 40, 56, 72]);
    assert_eq!(attrs[1].format.as_raw(), ash::vk::Format::R32G32B32A32_UINT.as_raw());
    let bone = &vertex.uniforms[0];
    assert_eq!(
        (bone.name.as_str(), &bone.kind, bone.count),
        ("g_Bones[2]", &UniformKind::Mat4x3, 2)
    );
    assert_eq!(vertex.block_size, 128);
}

#[cfg(feature = "shared-device")]
mod gpu {
    use crate::vk::{
        self, EffectCoordinates, EffectDepth, EffectGeometry, EffectPipelineSpec, Renderer,
    };
    use anyhow::{Result, ensure};
    use paper_scene::effects::{EffectPass, FrameClock, PassBlend, PassMeta};
    use paper_scene::puppet::{AnimationLayer, BonePose};
    use paper_scene::shader::{Stage, UniformKind, translate, unify_uniforms};
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;

    const VERTEX: &str = "uniform float g_Before;\nuniform mat4x3 g_Bones[2];\nuniform float g_After;\nuniform int g_Channel;\nattribute vec3 a_Position;\nattribute vec3 a_Normal;\nattribute vec4 a_Tangent4;\nattribute uvec4 a_BlendIndices;\nattribute vec4 a_BlendWeights;\nattribute vec2 a_TexCoord;\nvarying vec4 v_Bytes;\nvoid main() {\n float value = g_Before + g_After;\n if (g_Channel < 3) {\n vec3 posed = mul(vec4(a_Position, 1.0), g_Bones[a_BlendIndices.x] * a_BlendWeights.x + g_Bones[a_BlendIndices.y] * a_BlendWeights.y + g_Bones[a_BlendIndices.z] * a_BlendWeights.z + g_Bones[a_BlendIndices.w] * a_BlendWeights.w);\n value = posed[g_Channel];\n }\n else if (g_Channel < 6) value = a_Position[g_Channel - 3];\n else if (g_Channel < 9) value = a_Normal[g_Channel - 6];\n else if (g_Channel < 13) value = a_Tangent4[g_Channel - 9];\n else if (g_Channel < 17) value = a_BlendWeights[g_Channel - 13];\n uint bits = floatBitsToUint(value);\n if (g_Channel >= 17 && g_Channel < 21) bits = a_BlendIndices[g_Channel - 17];\n v_Bytes = vec4(bits & 255u, (bits >> 8u) & 255u, (bits >> 16u) & 255u, (bits >> 24u) & 255u) / 255.0;\n gl_Position = vec4(a_TexCoord, 0.0, 1.0);\n}\n";
    const FRAGMENT: &str = "varying vec4 v_Bytes;\nvoid main() { gl_FragColor = v_Bytes; }\n";

    #[test]
    #[ignore = "requires frozen R92 rotatex model, isolated Vulkan and DXC"]
    #[expect(
        clippy::too_many_lines,
        reason = "one frozen palette and vertex GPU roundtrip transaction"
    )]
    fn authored_palette_roundtrip() -> Result<()> {
        let path = std::env::var_os("SKWD_PUPPET_MODEL_FIXTURE").expect("rotatex fixture path");
        let source = std::fs::read(path)?;
        ensure!(
            format!("{:x}", Sha256::digest(&source))
                == "370d315ba0509f98d8ef8c2eeeb553c996aba67e93cb546ea35238742d5f571d",
            "rotatex fixture hash"
        );
        ensure!(source[73..77] == 0x0180_000f_u32.to_le_bytes(), "modern vertex layout");
        let mesh = paper_scene::puppet::parse(&source)?;
        let frame = paper_scene::puppet::skin(
            &mesh,
            &[AnimationLayer { visible: true, id: 211, rate: 1.0, blend: 1.0, additive: false }],
            0.0,
        );
        let [Some(BonePose::Spatial(matrix))] = frame.bones.as_ref() else {
            anyhow::bail!("qualified spatial palette missing");
        };
        let record = &source[81 + 71 * 80..81 + 72 * 80];
        let words: Vec<_> = record
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("word width")))
            .collect();
        ensure!(words[10..14] == [0; 4], "source blend indices");
        ensure!(mesh.vertices[71].position == [2.0, -3.0, 5.0], "source position");
        let mut expected = vec![0x441a_a21f, 0xc3a9_0dbc, 0x4096_6c86];
        expected.extend_from_slice(&words[..10]);
        expected.extend_from_slice(&words[14..18]);
        expected.extend_from_slice(&words[10..14]);
        expected.push(8.0_f32.to_bits());
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
            1,
            1,
        )?;
        renderer.ensure_scene_pipelines()?;
        let target =
            renderer.create_scene_target_fmt(1, 1, false, ash::vk::Format::R8G8B8A8_UNORM)?;
        let result = (|| {
            for hlsl in [false, true] {
                ensure!(!hlsl || paper_scene::hlsl::available(), "DXC unavailable");
                let combos = BTreeMap::new();
                let mut vertex = translate(VERTEX, Stage::Vertex, &combos);
                let mut fragment = translate(FRAGMENT, Stage::Fragment, &combos);
                unify_uniforms(&mut vertex, &mut fragment);
                let pair = hlsl.then(|| {
                    paper_scene::hlsl::rewrite(
                        &VERTEX.replace("floatBitsToUint", "asuint"),
                        FRAGMENT,
                        &combos,
                        &vertex.uniforms,
                        &[],
                    )
                });
                let pass = EffectPass {
                    blend: PassBlend::Copy,
                    property_source: None,
                    user_uniforms: std::collections::BTreeSet::new(),
                    name: "authored-puppet-roundtrip".into(),
                    vertex,
                    geometry: None,
                    fragment,
                    hlsl: pair,
                    textures: Vec::new(),
                    values: BTreeMap::new(),
                    target: None,
                    binds: Vec::new(),
                };
                let bone = pass
                    .vertex
                    .uniforms
                    .iter()
                    .find(|uniform| uniform.name == "g_Bones[2]")
                    .expect("bone declaration");
                assert_eq!(bone.kind, UniformKind::Mat4x3);
                assert_eq!(bone.count, 2);
                assert_eq!(bone.offset % 16, 0);
                let pipeline = renderer.create_effect_pipeline_for(
                    &pass.vertex,
                    &pass.fragment,
                    pass.hlsl.as_ref(),
                    &pass.name,
                    EffectPipelineSpec {
                        depth_attachment: true,
                        samples: ash::vk::SampleCountFlags::TYPE_1,
                        format: ash::vk::Format::R8G8B8A8_UNORM,
                        blend: vk::SceneBlend::Copy,
                        scene: None,
                        geometry: EffectGeometry::Puppet,
                        depth: EffectDepth::default(),
                        coordinates: EffectCoordinates::Viewport,
                    },
                )?;
                ensure!(pipeline.hlsl == hlsl, "shader backend mismatch");
                let indices =
                    renderer.create_dyn_buffer(6, ash::vk::BufferUsageFlags::INDEX_BUFFER)?;
                renderer.write_dyn_buffer(&indices, &[0, 0, 1, 0, 2, 0]);
                let vertices =
                    renderer.create_dyn_buffer(240, ash::vk::BufferUsageFlags::VERTEX_BUFFER)?;
                let mut slot = renderer.create_view_slot()?;
                let observed = (|| {
                    slot.effect = Some(vk::SceneEffect::mesh(
                        &pipeline,
                        vk::MeshGeometry {
                            vertices: vertices.buffer,
                            indices: indices.buffer,
                            index_type: ash::vk::IndexType::UINT16,
                            count: 3,
                        },
                        None,
                    )?);
                    let quad = vk::SceneQuad {
                        rect: [0.0, 0.0, 1.0, 1.0],
                        uv: [0.0, 0.0, 1.0, 1.0],
                        uv_axes: None,
                        uv_padding: false,
                        tint: [1.0; 4],
                        angle: 0.0,
                        projection: None,
                        order_bias: 0,
                        texture: 0,
                        blend: vk::SceneBlend::Copy,
                    };
                    for index in [0_u32, 1, 0x8000_0001] {
                        let mut records = Vec::with_capacity(240);
                        for uv in [[-1.0_f32, -1.0], [3.0, -1.0], [-1.0, 3.0]] {
                            let mut raw = record.to_vec();
                            for lane in 0..4 {
                                raw[40 + lane * 4..44 + lane * 4]
                                    .copy_from_slice(&index.to_le_bytes());
                            }
                            for (axis, value) in uv.into_iter().enumerate() {
                                raw[72 + axis * 4..76 + axis * 4]
                                    .copy_from_slice(&value.to_le_bytes());
                            }
                            records.extend(raw);
                        }
                        renderer.write_dyn_buffer(&vertices, &records);
                        let mut palette = vec![123.0; 32];
                        let slot_index = index.min(1) as usize;
                        palette[slot_index * 16..(slot_index + 1) * 16].copy_from_slice(matrix);
                        for lane in [3, 7, 11, 15] {
                            palette[slot_index * 16 + lane] = 91.0 + lane as f32;
                        }
                        let meta = PassMeta::of(&pass);
                        for (channel, &native) in expected.iter().enumerate() {
                            if index > 1 && !(17..21).contains(&channel) {
                                continue;
                            }
                            let values = BTreeMap::from([
                                ("g_Before".into(), vec![3.0]),
                                ("g_After".into(), vec![5.0]),
                                ("g_Bones[2]".into(), palette.clone()),
                                ("g_Channel".into(), vec![f32::from_bits(channel as u32)]),
                            ]);
                            let bytes = meta.uniform_bytes_with(
                                FrameClock::default(),
                                None,
                                1,
                                1,
                                (1, 1),
                                &[],
                                &values,
                                hlsl,
                            );
                            assert_eq!(
                                &bytes[bone.offset..bone.offset + 128],
                                palette
                                    .iter()
                                    .flat_map(|value| value.to_le_bytes())
                                    .collect::<Vec<_>>()
                            );
                            renderer.write_effect_inputs(&pipeline, &[], &bytes);
                            renderer.render_scene(
                                &target,
                                [0.25; 4],
                                std::slice::from_ref(&quad),
                                std::slice::from_ref(&slot),
                            )?;
                            let (width, height, pixels) = renderer.read_scene_target(&target)?;
                            ensure!((width, height, pixels.len()) == (1, 1, 4), "readback shape");
                            let bits = if (17..21).contains(&channel) { index } else { native };
                            ensure!(
                                pixels == bits.to_le_bytes(),
                                "puppet bytes {pixels:?}, expected {bits:08x}, channel {channel}, index {index}, HLSL {hlsl}"
                            );
                        }
                    }
                    Ok(())
                })();
                renderer.destroy_scene_texture(slot);
                observed?;
            }
            Ok(())
        })();
        renderer.destroy_scene_target(target);
        result
    }
}
