use std::fmt::Write;

use super::super::{EffectPass, PassBlend};
use crate::hlsl::HlslPair;
use crate::shader::{Sampler, Translated};
use anyhow::{Context, Result, ensure};

const ALBEDO: &str = "albedo";
const UV: [&str; 2] = ["a_TexCoord", "a_TexCoordVec4"];
const GLSL_ENTRY: &str = "void main(";
const GLSL_SHADOW_ENTRY: &str = "void skwd_shadow_vertex(";
const HLSL_ENTRY: &str = "VS_OUTPUT main(VS_INPUT IN)";
const HLSL_SHADOW_ENTRY: &str = "VS_OUTPUT skwd_shadow_vertex(VS_INPUT IN)";
const GLSL_HEADER: &str = "#version 450\n";
const GLSL_PUSH: &str =
    "layout(push_constant) uniform ShadowProjection { mat4 skwd_ShadowProjection; };\n";
const HLSL_PUSH: &str = "struct ShadowProjection { float4x4 matrix; };\n[[vk::push_constant]] ShadowProjection skwd_ShadowProjection;\n";
const GLSL_OUTPUT: &str = "layout(location=0) out vec2 skwd_ShadowUv;\n";
const GLSL_MAIN: &str = "void main(){gl_Position=skwd_ShadowProjection*vec4(a_Position,1.0);";
const SKIN_INDICES: &str = "a_BlendIndices";
const SKIN_WEIGHTS: &str = "a_BlendWeights";
const GLSL_SKIN_MAIN: &str = "void main(){vec3 skinned=(g_Bones[a_BlendIndices.x]*a_BlendWeights.x+g_Bones[a_BlendIndices.y]*a_BlendWeights.y+g_Bones[a_BlendIndices.z]*a_BlendWeights.z+g_Bones[a_BlendIndices.w]*a_BlendWeights.w)*vec4(a_Position,1.0);gl_Position=skwd_ShadowProjection*vec4(skinned,1.0);";
const HLSL_SKIN_MAIN: &str = "};\nShadowOutput main(VS_INPUT IN){ShadowOutput OUT;float3 skinned=mul(float4(IN.a_Position,1.0),g_Bones[IN.a_BlendIndices.x]*IN.a_BlendWeights.x+g_Bones[IN.a_BlendIndices.y]*IN.a_BlendWeights.y+g_Bones[IN.a_BlendIndices.z]*IN.a_BlendWeights.z+g_Bones[IN.a_BlendIndices.w]*IN.a_BlendWeights.w);OUT.position=mul(float4(skinned,1.0),skwd_ShadowProjection.matrix);";
const GLSL_END: &str = "}\n";
const GLSL_OPAQUE: &str = "#version 450\nvoid main(){}";
const HLSL_OPAQUE: &str = "void main(){}";
const OUTPUT_LAYOUT: &str = "layout(location";
const OUTPUT_MARKERS: [&str; 2] = [") out ", ") flat out "];
const HLSL_OUTPUT: &str = "struct ShadowOutput {float4 position:SV_POSITION;";
const HLSL_UV: &str = "float2 uv:TEXCOORD0;";
const HLSL_MAIN: &str = "};\nShadowOutput main(VS_INPUT IN){ShadowOutput OUT;OUT.position=mul(float4(IN.a_Position,1.0),skwd_ShadowProjection.matrix);";
const HLSL_END: &str = "return OUT;}\n";
const ENTRY_ERROR: &str = "shadow vertex entry missing";
const UV_ERROR: &str = "shadow albedo coordinates missing";

pub struct Sources {
    pub vertex: Translated,
    pub fragment: Translated,
    pub hlsl: Option<HlslPair>,
}

pub(super) fn albedo(pass: &EffectPass) -> Option<&Sampler> {
    if pass.blend != PassBlend::AlphaCoverage {
        return None;
    }
    pass.vertex
        .samplers
        .iter()
        .chain(&pass.fragment.samplers)
        .find(|sampler| sampler.material.as_deref() == Some(ALBEDO))
}

pub fn sources(pass: &EffectPass) -> Result<Sources> {
    let inputs = crate::shader::vertex_inputs(&pass.vertex.source).map_err(anyhow::Error::msg)?;
    let skinned = inputs.iter().any(|input| input.name == SKIN_INDICES);
    ensure!(
        !skinned || inputs.iter().any(|input| input.name == SKIN_WEIGHTS),
        "shadow skin weights missing"
    );
    let alpha = albedo(pass);
    let uv = alpha
        .map(|_| {
            UV.iter()
                .find(|name| pass.vertex.source.contains(&format!(" {name};")))
                .copied()
                .context(UV_ERROR)
        })
        .transpose()?;
    let mut source = String::new();
    for line in pass.vertex.source.lines() {
        let private = line
            .trim_start()
            .starts_with(OUTPUT_LAYOUT)
            .then(|| OUTPUT_MARKERS.iter().find_map(|marker| line.split_once(marker)))
            .flatten();
        source.push_str(private.map_or(line, |(_, declaration)| declaration));
        source.push('\n');
    }
    ensure!(source.contains(GLSL_ENTRY), ENTRY_ERROR);
    source = source.replacen(GLSL_ENTRY, GLSL_SHADOW_ENTRY, 1);
    source.push_str(GLSL_PUSH);
    if uv.is_some() {
        source.push_str(GLSL_OUTPUT);
    }
    source.push_str(if skinned { GLSL_SKIN_MAIN } else { GLSL_MAIN });
    if let Some(uv) = uv {
        write!(source, "skwd_ShadowUv={uv}.xy;")?;
    }
    source.push_str(GLSL_END);
    let fragment_source = alpha.map_or_else(|| GLSL_OPAQUE.to_owned(), |sampler| format!(
        "{GLSL_HEADER}layout(set=0,binding={}) uniform sampler2D {};\nlayout(location=0) in vec2 skwd_ShadowUv;\nvoid main(){{if(texture({},skwd_ShadowUv).a<0.5)discard;}}",
        sampler.index+1,sampler.name,sampler.name
    ));
    let hlsl = pass.hlsl.as_ref().map(|pair| {
        let mut vertex = pair.vertex.replacen(HLSL_ENTRY,HLSL_SHADOW_ENTRY,1);
        vertex.push_str(HLSL_PUSH);
        vertex.push_str(HLSL_OUTPUT);
        if uv.is_some() { vertex.push_str(HLSL_UV); }
        vertex.push_str(if skinned { HLSL_SKIN_MAIN } else { HLSL_MAIN });
        if let Some(uv) = uv { write!(vertex, "OUT.uv=IN.{uv}.xy;").expect("shadow shader string"); }
        vertex.push_str(HLSL_END);
        let fragment = alpha.map_or_else(|| HLSL_OPAQUE.to_owned(), |sampler| format!(
            "Texture2D {}:register(t{});SamplerState shadowSampler:register(s{});\nvoid main(float2 uv:TEXCOORD0){{clip({}.Sample(shadowSampler,uv).a-0.5);}}",
            sampler.name,sampler.index,sampler.index,sampler.name
        ));
        HlslPair { vertex, fragment }
    });
    Ok(Sources {
        vertex: Translated {
            source,
            uniforms: pass.vertex.uniforms.clone(),
            samplers: pass.vertex.samplers.clone(),
            block_size: pass.vertex.block_size,
        },
        fragment: Translated {
            source: fragment_source,
            uniforms: Vec::new(),
            samplers: alpha.cloned().into_iter().collect(),
            block_size: 0,
        },
        hlsl,
    })
}
