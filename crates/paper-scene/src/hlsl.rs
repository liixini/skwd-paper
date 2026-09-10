use crate::shader::{Sampler, Stage, Uniform, UniformKind};
use anyhow::{Result, anyhow};
use std::collections::BTreeMap;
use std::fmt::Write;

pub const PRELUDE: &str = "#define HLSL 1
#define HLSL_SM40 1
#define vec2 float2
#define vec3 float3
#define vec4 float4
#define ivec2 int2
#define ivec3 int3
#define ivec4 int4
#define uvec4 uint4
#define mat4 float4x4
#define mat4x3 float4x3
#define mat3 float3x3
#define mat2 float2x2
#define mix lerp
#define mod(x, y) ((x)-(y)*floor((x)/(y)))
#define CASTI(x) ((int)(x))
#define CASTU(x) ((uint)(x))
#define CASTF(x) ((float)(x))
#define CAST2(x) ((float2)(x))
#define CAST3(x) ((float3)(x))
#define CAST4U(x) ((uint4)(x))
#define CAST4(x) ((float4)(x))
#define CAST3X3(x) ((float3x3)(x))
#define texSample2D(s, u) s.Sample(s ## SamplerState, u)
#define texSample2DLod(s, u, m) s.SampleLevel(s ## SamplerState, u, m)
#define texSample2DCompare(s, u, d) s.SampleCmpLevelZero(s ## SamplerComparisonState, u, d)
#define texLoad2D(s, u, r) s.Load(int3((u) * (r), 0))
#define texSample3D(s, u) s.Sample(s ## SamplerState, u)
#define texSample2DBackBuffer(s, u, r) texSample2D(s, (u))
#define fract frac
#define inversesqrt rsqrt
#define dFdx ddx
#define dFdy ddy
";

pub const TEXTURE_BINDING_SHIFT: u32 = 1;
pub const SAMPLER_BINDING_SHIFT: u32 = 17;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HlslPair {
    pub vertex: String,
    pub fragment: String,
}

struct Decl {
    ty: String,
    name: String,
    array: String,
}

fn semantic(name: &str, next: &mut u32) -> String {
    let fixed = match name {
        "a_Position" => Some("POSITION"),
        "a_TexCoord" => Some("TEXCOORD0"),
        "a_Normal" => Some("NORMAL"),
        "a_Tangent4" => Some("TANGENT"),
        "a_Color" => Some("COLOR"),
        "a_BlendIndices" => Some("BLENDINDICES"),
        "a_BlendWeights" => Some("BLENDWEIGHT"),
        _ => None,
    };
    if let Some(fixed) = fixed {
        return fixed.to_string();
    }
    *next += 1;
    format!("TEXCOORD{next}")
}

fn split_decl(rest: &str) -> Option<Decl> {
    let decl = rest.trim_end_matches(';').trim();
    let mut words = decl.split_whitespace();
    let ty = words.next()?.to_string();
    let name_raw = words.next()?;
    let (name, array) = match name_raw.find('[') {
        Some(at) => (name_raw[..at].to_string(), name_raw[at..].to_string()),
        None => (name_raw.to_string(), String::new()),
    };
    if name.is_empty() || !name.bytes().all(word_char) {
        return None;
    }
    Some(Decl { ty, name, array })
}

fn packed_scalar_array(decl: &Decl) -> Option<u32> {
    if decl.ty != "float" {
        return None;
    }
    let count: u32 = decl.array.trim_matches(['[', ']']).trim().parse().ok()?;
    (count > 4).then_some(count.div_ceil(4))
}

fn pack_indices(text: &str, name: &str) -> String {
    let pattern = format!("{name}[");
    let mut out = String::with_capacity(text.len() + 64);
    let mut rest = text;
    while let Some(at) = rest.find(&pattern) {
        let preceded_by_word = rest[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.');
        let start = at + pattern.len();
        let mut depth = 1usize;
        let mut end = None;
        for (offset, ch) in rest[start..].char_indices() {
            match ch {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(start + offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            break;
        };
        if preceded_by_word && !name.contains('.') {
            out.push_str(&rest[..=end]);
            rest = &rest[end + 1..];
            continue;
        }
        let index = rest[start..end].trim();
        out.push_str(&rest[..at]);
        let _ = write!(out, "{name}[int({index}) / 4][int({index}) % 4]");
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn strip_declarations(source: &str) -> (String, Vec<Decl>, Vec<Decl>) {
    let mut body = String::with_capacity(source.len());
    let mut attributes = Vec::new();
    let mut varyings = Vec::new();
    for line in source.lines() {
        let code = line.trim_start().split("//").next().unwrap_or("").trim();
        if code.starts_with("#version") || code.starts_with("uniform ") {
            continue;
        }
        if let Some(rest) = code.strip_prefix("attribute ") {
            if let Some(decl) = split_decl(rest) {
                attributes.push(decl);
            }
            continue;
        }
        if let Some(rest) = code.strip_prefix("varying ") {
            if let Some(decl) = split_decl(rest) {
                varyings.push(decl);
            }
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    (body, attributes, varyings)
}

fn replace_word(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + 16);
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx..].starts_with(from.as_bytes()) {
            let before_ok = idx == 0 || !word_char(bytes[idx - 1]);
            let after = idx + from.len();
            let after_ok = after >= bytes.len() || !word_char(bytes[after]);
            if before_ok && after_ok && !(idx > 0 && bytes[idx - 1] == b'.') {
                out.push_str(to);
                idx = after;
                continue;
            }
        }
        let width = crate::shader::utf8_width(bytes[idx]);
        out.push_str(&text[idx..idx + width]);
        idx += width;
    }
    out
}

fn word_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn rewrite_main(body: &str, stage: Stage) -> String {
    let signature = match stage {
        Stage::Vertex => "VS_OUTPUT main(VS_INPUT IN) {\n\tVS_OUTPUT OUT = (VS_OUTPUT)0;",
        Stage::Fragment => "PS_OUTPUT main(VS_OUTPUT IN) {\n\tPS_OUTPUT OUT = (PS_OUTPUT)0;",
    };
    let Some(at) = body.find("void main") else {
        return body.to_string();
    };
    let Some(open) = body[at..].find('{').map(|off| at + off) else {
        return body.to_string();
    };
    let mut depth = 0i32;
    let mut close = open;
    for (offset, byte) in body[open..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    close = open + offset;
                    break;
                }
            }
            _ => {}
        }
    }
    let inner = replace_return(&body[open + 1..close]);
    let mut out = String::with_capacity(body.len() + 64);
    out.push_str(&body[..at]);
    out.push_str(signature);
    out.push_str(&inner);
    out.push_str("\n\treturn OUT;\n");
    out.push_str(&body[close..]);
    out
}

fn replace_return(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let bytes = inner.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx..].starts_with(b"return")
            && (idx == 0 || !word_char(bytes[idx - 1]))
            && bytes[idx + 6..].iter().copied().find(|b| !b.is_ascii_whitespace()) == Some(b';')
        {
            let semi = idx + 6 + bytes[idx + 6..].iter().position(|b| *b == b';').unwrap_or(0);
            out.push_str("return OUT;");
            idx = semi + 1;
            continue;
        }
        let width = crate::shader::utf8_width(bytes[idx]);
        out.push_str(&inner[idx..idx + width]);
        idx += width;
    }
    out
}

fn static_const(body: &str) -> String {
    let mut out = String::with_capacity(body.len() + 32);
    let mut depth = 0i32;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if depth == 0 && trimmed.starts_with("const ") {
            out.push_str(&line[..line.len() - trimmed.len()]);
            out.push_str("static ");
            out.push_str(trimmed);
        } else {
            out.push_str(line);
        }
        out.push('\n');
        let code = line.split("//").next().unwrap_or("");
        for ch in code.chars() {
            match ch {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
    }
    out
}

fn names_globals(words: &[u32]) -> bool {
    let mut at = 5;
    while at < words.len() {
        let count = (words[at] >> 16) as usize;
        if count == 0 {
            return false;
        }
        if words[at] & 0xffff == 5 && count > 2 {
            let bytes: Vec<u8> = words[at + 2..(at + count).min(words.len())]
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect();
            let name = bytes.split(|b| *b == 0).next().unwrap_or(&[]);
            if name == b"$Globals" {
                return true;
            }
        }
        at += count;
    }
    false
}

fn hlsl_type(kind: &UniformKind) -> &'static str {
    match kind {
        UniformKind::Float => "float",
        UniformKind::Vec2 => "float2",
        UniformKind::Vec3 => "float3",
        UniformKind::Vec4 => "float4",
        UniformKind::Mat3 => "float3x3",
        UniformKind::Mat4 => "float4x4",
        UniformKind::Int => "int",
    }
}

fn declare_resources(out: &mut String, uniforms: &[Uniform], samplers: &[Sampler]) {
    if !uniforms.is_empty() {
        out.push_str("cbuffer SkwdParams : register(b0) {\n");
        for uniform in uniforms {
            let array =
                if uniform.count > 1 { format!("[{}]", uniform.count) } else { String::new() };
            let name = uniform.name.split('[').next().unwrap_or(&uniform.name);
            let _ = writeln!(out, "\t{} {name}{array};", hlsl_type(&uniform.kind));
        }
        out.push_str("};\n");
    }
    for sampler in samplers {
        let _ = writeln!(
            out,
            "Texture2D {0} : register(t{1}); SamplerState {0}SamplerState : register(s{1});",
            sampler.name, sampler.index
        );
    }
}

pub const MAX_VARYING_SLOTS: u32 = 32;

#[must_use]
pub fn varying_slots(vertex: &str) -> u32 {
    let (_, _, varyings) = strip_declarations(vertex);
    varyings
        .iter()
        .map(|decl| {
            packed_scalar_array(decl).unwrap_or_else(|| {
                decl.array.trim_matches(['[', ']']).trim().parse::<u32>().unwrap_or(1).max(1)
            })
        })
        .sum()
}

#[must_use]
pub fn fragment_indexes_varying_arrays_dynamically(fragment: &str) -> bool {
    let (body, _, varyings) = strip_declarations(fragment);
    let scalar =
        |decl: &&Decl| !decl.array.is_empty() && matches!(decl.ty.as_str(), "float" | "int");
    varyings.iter().filter(scalar).any(|decl| {
        let pattern = format!("{}[", decl.name);
        let mut rest = body.as_str();
        while let Some(at) = rest.find(&pattern) {
            let after = &rest[at + pattern.len()..];
            let index: String = after.chars().take_while(|c| *c != ']').collect();
            let literal =
                !index.trim().is_empty() && index.trim().chars().all(|c| c.is_ascii_digit());
            if !literal {
                return true;
            }
            rest = &rest[at + pattern.len()..];
        }
        false
    })
}

pub fn rewrite(
    vertex: &str,
    fragment: &str,
    combos: &BTreeMap<String, i64>,
    uniforms: &[Uniform],
    samplers: &[Sampler],
) -> HlslPair {
    let (vert_body, attributes, varyings) = strip_declarations(vertex);
    let (frag_body, _, _) = strip_declarations(fragment);
    let mut header = String::from(PRELUDE);
    for (key, value) in combos {
        let _ = writeln!(header, "#define {key} {value}");
    }
    declare_resources(&mut header, uniforms, samplers);
    header.push_str("struct VS_INPUT {\n");
    let mut next_semantic = 0u32;
    for (location, decl) in attributes.iter().enumerate() {
        let _ = writeln!(
            header,
            "\t[[vk::location({location})]] {} {}{} : {};",
            decl.ty,
            decl.name,
            decl.array,
            semantic(&decl.name, &mut next_semantic)
        );
    }
    header.push_str("};\nstruct VS_OUTPUT {\n\tfloat4 gl_Position : SV_POSITION;\n");
    let mut slot = 0u32;
    for decl in &varyings {
        if let Some(packed) = packed_scalar_array(decl) {
            let _ = writeln!(header, "\tfloat4 {}[{packed}] : TEXCOORD{slot};", decl.name);
            slot += packed;
        } else {
            let _ = writeln!(header, "\t{} {}{} : TEXCOORD{slot};", decl.ty, decl.name, decl.array);
            slot += decl.array.trim_matches(['[', ']']).parse::<u32>().unwrap_or(1).max(1);
        }
    }
    header.push_str("};\nstruct PS_OUTPUT { float4 gl_FragColor : SV_TARGET; };\n");

    let mut vert = static_const(&vert_body);
    for decl in &attributes {
        vert = replace_word(&vert, &decl.name, &format!("IN.{}", decl.name));
    }
    for decl in &varyings {
        vert = replace_word(&vert, &decl.name, &format!("OUT.{}", decl.name));
        if packed_scalar_array(decl).is_some() {
            vert = pack_indices(&vert, &format!("OUT.{}", decl.name));
        }
    }
    vert = replace_word(&vert, "gl_Position", "OUT.gl_Position");
    vert = rewrite_main(&vert, Stage::Vertex);

    let mut frag = static_const(&frag_body);
    let mut input_copies = String::new();
    for decl in &varyings {
        if decl.array.is_empty() {
            frag = replace_word(&frag, &decl.name, &format!("IN.{}", decl.name));
        } else if let Some(packed) = packed_scalar_array(decl) {
            let _ = writeln!(input_copies, "\tfloat4 {}[{packed}] = IN.{};", decl.name, decl.name);
            frag = pack_indices(&frag, &decl.name);
        } else {
            let _ = writeln!(
                input_copies,
                "\t{} {}{} = IN.{};",
                decl.ty, decl.name, decl.array, decl.name
            );
        }
    }
    frag = replace_word(&frag, "gl_FragColor", "OUT.gl_FragColor");
    frag = rewrite_main(&frag, Stage::Fragment);
    if !input_copies.is_empty() {
        let marker = "PS_OUTPUT OUT = (PS_OUTPUT)0;\n";
        if let Some(at) = frag.find(marker) {
            frag.insert_str(at + marker.len(), &input_copies);
        }
    }

    HlslPair { vertex: format!("{header}{vert}"), fragment: format!("{header}{frag}") }
}

struct NoIncludes;

impl hassle_rs::DxcIncludeHandler for NoIncludes {
    fn load_source(&mut self, _filename: String) -> Option<String> {
        None
    }
}

thread_local! {
    static DXC: std::cell::OnceCell<Option<hassle_rs::Dxc>> = const { std::cell::OnceCell::new() };
}

fn load_dxc() -> Option<hassle_rs::Dxc> {
    if std::env::var("SKWD_PAPER_SHADER_BACKEND").as_deref() == Ok("glsl") {
        return None;
    }
    let beside_exe = std::env::current_exe().ok().and_then(|exe| {
        let dir = exe.parent()?;
        [dir.join("libdxcompiler.so"), dir.join("../lib/libdxcompiler.so")]
            .into_iter()
            .find(|path| path.is_file())
    });
    let candidates: Vec<Option<std::path::PathBuf>> = match std::env::var_os("SKWD_DXC_PATH") {
        Some(path) => vec![Some(std::path::PathBuf::from(path))],
        None => vec![
            beside_exe,
            std::env::var_os("HOME")
                .map(|home| std::path::PathBuf::from(home).join(".local/lib/libdxcompiler.so")),
            None,
            Some("/usr/lib/libdxcompiler.so".into()),
            Some("/usr/lib64/libdxcompiler.so".into()),
        ],
    };
    for candidate in candidates {
        if let Ok(dxc) = hassle_rs::Dxc::new(candidate) {
            return Some(dxc);
        }
    }
    None
}

#[must_use]
pub fn available() -> bool {
    DXC.with(|cell| cell.get_or_init(load_dxc).is_some())
}

pub fn compile(source: &str, stage: Stage, label: &str) -> Result<Vec<u32>> {
    if std::env::var("SKWD_PAPER_SHADER_BACKEND").as_deref() == Ok("glsl") {
        return Err(anyhow!("hlsl backend disabled"));
    }
    let tag = if stage == Stage::Vertex { "hvert" } else { "hfrag" };
    if let Some(words) = crate::shader::cached_spirv(source, tag) {
        return Ok(words);
    }
    let words = DXC.with(|cell| {
        let dxc = cell.get_or_init(load_dxc).as_ref().ok_or_else(|| anyhow!("dxc unavailable"))?;
        let compiler = dxc.create_compiler().map_err(|err| anyhow!("dxc compiler: {err}"))?;
        let library = dxc.create_library().map_err(|err| anyhow!("dxc library: {err}"))?;
        let blob = library
            .create_blob_with_encoding_from_str(source)
            .map_err(|err| anyhow!("dxc blob: {err}"))?;
        let profile = if stage == Stage::Vertex { "vs_6_0" } else { "ps_6_0" };
        let args = [
            "-spirv",
            "-HV",
            "2018",
            "-fspv-target-env=vulkan1.2",
            "-Zpr",
            "-fvk-use-gl-layout",
            "-fvk-b-shift",
            "0",
            "0",
            "-fvk-t-shift",
            "1",
            "0",
            "-fvk-s-shift",
            "17",
            "0",
            "-Wno-conversion",
        ];
        let result =
            compiler.compile(&blob, label, "main", profile, &args, Some(&mut NoIncludes), &[]);
        match result {
            Ok(result) => {
                let blob = result.get_result().map_err(|err| anyhow!("dxc result: {err}"))?;
                let words = blob.to_vec::<u32>();
                if names_globals(&words) {
                    return Err(anyhow!("dxc {label}: non-static global landed in $Globals"));
                }
                Ok(words)
            }
            Err((result, _)) => {
                let text = result
                    .get_error_buffer()
                    .ok()
                    .and_then(|buffer| library.get_blob_as_string(&buffer.into()).ok())
                    .unwrap_or_default();
                Err(anyhow!("dxc {label}: {text}"))
            }
        }
    })?;
    crate::shader::store_spirv(source, tag, &words);
    Ok(words)
}

#[cfg(test)]
mod tests;
