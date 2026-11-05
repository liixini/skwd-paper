use crate::shader::{
    DEFINE_PREFIX, PACKED_COMPONENTS, Sampler, Stage, UNDEF_PREFIX, Uniform, UniformKind,
    VaryingPack,
};
use anyhow::{Result, anyhow};
use std::collections::BTreeMap;
use std::fmt::Write;

mod audio;
mod bounds;
mod spirv;

const HLSL_MAT4X3: &str = "float4x3";
const PACKED_VARYING_TYPES: [(&str, u32); 3] = [("float", 1), ("vec2", 2), ("vec3", 3)];
const VARYING_FIELD_TYPES: [(&str, u32); 3] = [("float", 1), ("vec2", 2), ("vec3", 3)];
const HLSL_RETURN_OUT: &str = "return OUT;";
const HLSL_FRAGMENT_OUT: &str = "PS_OUTPUT OUT = (PS_OUTPUT)0;";
const HLSL_PACKED_FIELDS: &str = "skwd_packed_fields";

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
#define SKWD_SAMPLER(s) s ## SamplerState
#define SKWD_COMPARE_SAMPLER(s) s ## SamplerComparisonState
#define DECLARE_SAMPLER2D_PARAMETER(s) Texture2D s, SamplerState SKWD_SAMPLER(s)
#define MAKE_SAMPLER2D_ARGUMENT(s) s, SKWD_SAMPLER(s)
#define texSample2D(s, u) s.Sample(SKWD_SAMPLER(s), u)
#define texSample2DLod(s, u, m) s.SampleLevel(SKWD_SAMPLER(s), u, m)
#define texSample2DCompare(s, u, d) s.SampleCmpLevelZero(SKWD_COMPARE_SAMPLER(s), u, d)
#define texLoad2D(s, u, r) s.Load(int3((u) * (r), 0))
#define texSample3D(s, u) s.Sample(SKWD_SAMPLER(s), u)
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

fn packed_varying_array(decl: &Decl) -> Option<VaryingPack> {
    let width =
        PACKED_VARYING_TYPES.iter().find(|(ty, _)| *ty == decl.ty).map(|(_, width)| *width)?;
    let count: u32 = decl.array.trim_matches(['[', ']']).trim().parse().ok()?;
    (count * width > PACKED_COMPONENTS).then_some(VaryingPack { width, count })
}

fn declaration_slots(decl: &Decl) -> u32 {
    packed_varying_array(decl).map_or_else(
        || decl.array.trim_matches(['[', ']']).trim().parse::<u32>().unwrap_or(1).max(1),
        |pack| pack.slots(),
    )
}

struct FieldPack {
    base: u32,
    pack: VaryingPack,
    fields: BTreeMap<String, (u32, u32)>,
}

impl FieldPack {
    fn of(varyings: &[Decl]) -> Option<Self> {
        let raw: u32 = varyings.iter().map(declaration_slots).sum();
        if raw <= MAX_VARYING_SLOTS {
            return None;
        }
        let mut base = 0;
        let mut count = 0;
        let mut fields = BTreeMap::new();
        for decl in varyings {
            let width = VARYING_FIELD_TYPES
                .iter()
                .find(|(ty, _)| decl.array.is_empty() && *ty == decl.ty)
                .map(|(_, width)| *width);
            if let Some(width) = width {
                fields.insert(decl.name.clone(), (count, width));
                count += width;
            } else {
                base += declaration_slots(decl);
            }
        }
        (count > 0).then_some(Self { base, pack: VaryingPack { width: 1, count }, fields })
    }

    fn copies(&self, out: &mut String, decl: &Decl, stage: Stage) {
        let Some(&(start, width)) = self.fields.get(&decl.name) else { return };
        for component in 0..width {
            let (slot, lane) = self.pack.element(start + component);
            let local = if width == 1 {
                decl.name.clone()
            } else {
                let (_, field) = self.pack.element(component);
                format!("{}.{field}", decl.name)
            };
            let io = format!("{HLSL_PACKED_FIELDS}[{slot}].{lane}");
            let statement = if stage == Stage::Vertex {
                format!("\tOUT.{io} = {local};")
            } else {
                format!("\t{local} = IN.{io};")
            };
            let _ = writeln!(out, "{statement}");
        }
    }
}

fn strip_declarations(
    source: &str,
    combos: &BTreeMap<String, i64>,
) -> (String, Vec<Decl>, Vec<Decl>) {
    let mut symbols = combos.clone();
    let mut body = String::with_capacity(source.len());
    let mut attributes = Vec::new();
    let mut varyings = Vec::new();
    for line in source.lines() {
        let code = line.trim_start().split("//").next().unwrap_or("").trim();
        if let Some(rest) = code.strip_prefix(DEFINE_PREFIX) {
            let mut words = rest.split_whitespace();
            if let (Some(name), Some(Ok(value))) = (words.next(), words.next().map(str::parse)) {
                symbols.insert(name.to_string(), value);
            }
        }
        if let Some(name) = code.strip_prefix(UNDEF_PREFIX) {
            symbols.remove(name.trim());
        }
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
            if let Some(mut decl) = split_decl(rest) {
                let token = decl.array.trim_matches(['[', ']']);
                if let Some(size) = symbols.get(token) {
                    decl.array = format!("[{size}]");
                }
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
        Stage::Geometry => return body.to_string(),
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

fn hlsl_type(kind: &UniformKind) -> &'static str {
    match kind {
        UniformKind::Float => "float",
        UniformKind::Vec2 => "float2",
        UniformKind::Vec3 => "float3",
        UniformKind::Vec4 => "float4",
        UniformKind::Mat3 => "float3x3",
        UniformKind::Mat4 => "float4x4",
        UniformKind::Mat4x3 => HLSL_MAT4X3,
        UniformKind::Int => "int",
        UniformKind::Uint => crate::shader::GLSL_UINT,
    }
}

const COMPARISON_STATE: &str = "SamplerComparisonState";
const SAMPLER_STATE: &str = "SamplerState";

pub(crate) fn reads_texture(source: &str, name: &str) -> bool {
    source
        .lines()
        .filter(|line| !line.contains(SAMPLER_STATE) && !line.contains(COMPARISON_STATE))
        .any(|line| crate::shader::words_of(line).iter().any(|word| word == name))
}

fn declare_resources(out: &mut String, uniforms: &[Uniform], samplers: &[Sampler]) {
    if !uniforms.is_empty() {
        out.push_str("cbuffer SkwdParams : register(b0) {\n");
        for uniform in uniforms {
            let array = if uniform.count > 1 || uniform.name.contains('[') {
                format!("[{}]", uniform.count)
            } else {
                String::new()
            };
            let name = uniform.name.split('[').next().unwrap_or(&uniform.name);
            let _ = writeln!(out, "\t{} {name}{array};", hlsl_type(&uniform.kind));
        }
        out.push_str("};\n");
    }
    for sampler in samplers {
        let state = if sampler.comparison { COMPARISON_STATE } else { SAMPLER_STATE };
        let _ = writeln!(
            out,
            "Texture2D {0} : register(t{1}); {state} {0}{state} : register(s{1});",
            sampler.name, sampler.index
        );
    }
}

pub const MAX_VARYING_SLOTS: u32 = 32;

#[must_use]
pub fn varying_slots(vertex: &str, fragment: &str, combos: &BTreeMap<String, i64>) -> u32 {
    let vertex = crate::shader::rename_authored_mod(vertex);
    let fragment = crate::shader::rename_authored_mod(fragment);
    let (vertex_body, _, mut varyings) = strip_declarations(&vertex, combos);
    let (fragment_body, _, fragment_varyings) = strip_declarations(&fragment, combos);
    if bounds::supported_source(&vertex) && bounds::supported_source(&fragment) {
        bounds::shrink(&vertex_body, &fragment_body, combos, &mut varyings, &fragment_varyings);
    }
    FieldPack::of(&varyings).map_or_else(
        || varyings.iter().map(declaration_slots).sum(),
        |fields| fields.base + fields.pack.slots(),
    )
}

#[must_use]
pub fn fragment_indexes_varying_arrays_dynamically(
    fragment: &str,
    combos: &BTreeMap<String, i64>,
) -> bool {
    let (body, _, varyings) = strip_declarations(fragment, combos);
    let scalar =
        |decl: &&Decl| !decl.array.is_empty() && matches!(decl.ty.as_str(), "float" | "int");
    varyings.iter().filter(scalar).filter(|decl| packed_varying_array(decl).is_none()).any(|decl| {
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
    let vertex = crate::shader::rename_authored_mod(vertex);
    let fragment = crate::shader::rename_authored_mod(fragment);
    let (vert_body, attributes, mut varyings) = strip_declarations(&vertex, combos);
    let (frag_body, _, fragment_varyings) = strip_declarations(&fragment, combos);
    if bounds::supported_source(&vertex) && bounds::supported_source(&fragment) {
        bounds::shrink(&vert_body, &frag_body, combos, &mut varyings, &fragment_varyings);
    }
    let fields = FieldPack::of(&varyings);
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
        if fields.as_ref().is_some_and(|fields| fields.fields.contains_key(&decl.name)) {
            continue;
        }
        if let Some(pack) = packed_varying_array(decl) {
            let _ = writeln!(
                header,
                "\tfloat4 skwd_io_{}[{}] : TEXCOORD{slot};",
                decl.name,
                pack.slots()
            );
            slot += pack.slots();
        } else {
            let _ = writeln!(header, "\t{} {}{} : TEXCOORD{slot};", decl.ty, decl.name, decl.array);
            slot += declaration_slots(decl);
        }
    }
    if let Some(fields) = &fields {
        let _ = writeln!(
            header,
            "\tfloat4 {HLSL_PACKED_FIELDS}[{}] : TEXCOORD{};",
            fields.pack.slots(),
            fields.base
        );
    }
    header.push_str("};\nstruct PS_OUTPUT { float4 gl_FragColor : SV_TARGET; };\n");
    for decl in &varyings {
        if packed_varying_array(decl).is_some()
            || fields.as_ref().is_some_and(|fields| fields.fields.contains_key(&decl.name))
        {
            let _ = writeln!(header, "static {} {}{};", decl.ty, decl.name, decl.array);
        }
    }

    let mut vert = audio::flatten(&static_const(&vert_body), uniforms);
    for decl in &attributes {
        vert = replace_word(&vert, &decl.name, &format!("IN.{}", decl.name));
    }
    for decl in &varyings {
        if packed_varying_array(decl).is_none()
            && !fields.as_ref().is_some_and(|fields| fields.fields.contains_key(&decl.name))
        {
            vert = replace_word(&vert, &decl.name, &format!("OUT.{}", decl.name));
        }
    }
    vert = replace_word(&vert, "gl_Position", "OUT.gl_Position");
    vert = rewrite_main(&vert, Stage::Vertex);
    let mut output_copies = String::new();
    for decl in &varyings {
        if let Some(pack) = packed_varying_array(decl) {
            for (io, local) in pack.bindings(&decl.name) {
                let _ = writeln!(output_copies, "\tOUT.{io} = {local};");
            }
        }
        if let Some(fields) = &fields {
            fields.copies(&mut output_copies, decl, Stage::Vertex);
        }
    }
    if !output_copies.is_empty() {
        vert =
            vert.replace(HLSL_RETURN_OUT, &format!("{{\n{output_copies}\t{HLSL_RETURN_OUT}\n\t}}"));
    }

    let mut frag = audio::flatten(&static_const(&frag_body), uniforms);
    let mut input_copies = String::new();
    for decl in &varyings {
        if let Some(fields) = &fields
            && fields.fields.contains_key(&decl.name)
        {
            fields.copies(&mut input_copies, decl, Stage::Fragment);
        } else if decl.array.is_empty() {
            frag = replace_word(&frag, &decl.name, &format!("IN.{}", decl.name));
        } else if let Some(pack) = packed_varying_array(decl) {
            for (io, local) in pack.bindings(&decl.name) {
                let _ = writeln!(input_copies, "\t{local} = IN.{io};");
            }
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
        let marker = HLSL_FRAGMENT_OUT;
        if let Some(at) = frag.find(marker) {
            let end = at + marker.len();
            if frag[end..].starts_with('\n') {
                frag.insert_str(end + 1, &input_copies);
            } else {
                frag.insert_str(end, &format!("\n{input_copies}"));
            }
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
    const GEOMETRY_CACHE_TAG: &str = "hgeom";
    const GEOMETRY_PROFILE: &str = "gs_6_0";
    if std::env::var("SKWD_PAPER_SHADER_BACKEND").as_deref() == Ok("glsl") {
        return Err(anyhow!("hlsl backend disabled"));
    }
    let tag = match stage {
        Stage::Vertex => "hvert",
        Stage::Geometry => GEOMETRY_CACHE_TAG,
        Stage::Fragment => "hfrag",
    };
    if let Some(words) = crate::shader::cached_spirv(source, tag) {
        return spirv::normalize(words);
    }
    let words = DXC.with(|cell| {
        let dxc = cell.get_or_init(load_dxc).as_ref().ok_or_else(|| anyhow!("dxc unavailable"))?;
        let compiler = dxc.create_compiler().map_err(|err| anyhow!("dxc compiler: {err}"))?;
        let library = dxc.create_library().map_err(|err| anyhow!("dxc library: {err}"))?;
        let blob = library
            .create_blob_with_encoding_from_str(source)
            .map_err(|err| anyhow!("dxc blob: {err}"))?;
        let profile = match stage {
            Stage::Vertex => "vs_6_0",
            Stage::Geometry => GEOMETRY_PROFILE,
            Stage::Fragment => "ps_6_0",
        };
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
                if spirv::names_globals(&words)? {
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
    spirv::normalize(words)
}

#[cfg(test)]
mod tests;
