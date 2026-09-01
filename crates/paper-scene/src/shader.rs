use anyhow::{Result, anyhow};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

pub const PRELUDE: &str = r"#version 450
#ifndef M_PI
#define M_PI 3.14159265359
#endif
#ifndef M_PI_HALF
#define M_PI_HALF 1.57079632679
#endif
#ifndef M_PI_2
#define M_PI_2 6.28318530718
#endif
#ifndef SQRT_2
#define SQRT_2 1.41421356237
#endif
#ifndef SQRT_3
#define SQRT_3 1.73205080756
#endif
#define CAST2(x) (vec2(x))
#define CAST3(x) (vec3(x))
#define CAST4(x) (vec4(x))
#define CAST3X3(x) (mat3(x))
#define float2 vec2
#define float3 vec3
#define float4 vec4
#define int2 ivec2
#define int3 ivec3
#define int4 ivec4
#define frac(x) fract(x)
#define lerp(a, b, t) mix(a, b, t)
#define saturate(x) clamp(x, 0.0, 1.0)
#define atan2(y, x) atan(y, x)
#define fmod(x, y) ((x) - (y) * trunc((x) / (y)))
#define log10(x) (log2(x) * 0.301029995663981)
#define ddx(x) dFdx(x)
#define ddy(x) dFdy(x)
#define texSample2D(s, uv) texture(s, uv)
#define texSample2DLod(s, uv, l) textureLod(s, uv, l)
#define texSample2DGrad(s, uv, dx, dy) textureGrad(s, uv, dx, dy)
#define mul(v, m) ((m) * (v))
vec3 hsv2rgb(vec3 c) {
    vec4 k = vec4(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    vec3 p = abs(fract(c.xxx + k.xyz) * 6.0 - k.www);
    return c.z * mix(k.xxx, clamp(p - k.xxx, 0.0, 1.0), c.y);
}
vec3 rgb2hsv(vec3 c) {
    vec4 k = vec4(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    vec4 p = c.g < c.b ? vec4(c.bg, k.wz) : vec4(c.gb, k.xy);
    vec4 q = c.r < p.x ? vec4(p.xyw, c.r) : vec4(c.r, p.yzx);
    float d = q.x - min(q.w, q.y);
    return vec3(abs(q.z + (q.w - q.y) / (6.0 * d + 1e-10)), d / (q.x + 1e-10), q.x);
}
vec2 rotateVec2(vec2 v, float r) {
    float s = sin(r);
    float c = cos(r);
    return vec2(v.x * c - v.y * s, v.x * s + v.y * c);
}
float greyscale(vec3 color) {
    return dot(color, vec3(0.299, 0.587, 0.114));
}
float greyscale(vec4 color) {
    return dot(color.rgb, vec3(0.299, 0.587, 0.114));
}
";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Vertex,
    Fragment,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UniformKind {
    Float,
    Vec2,
    Vec3,
    Vec4,
    Mat3,
    Mat4,
    Int,
}

impl UniformKind {
    fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "float" => Self::Float,
            "vec2" => Self::Vec2,
            "vec3" => Self::Vec3,
            "vec4" => Self::Vec4,
            "mat3" => Self::Mat3,
            "mat4" => Self::Mat4,
            "int" | "bool" => Self::Int,
            _ => return None,
        })
    }

    pub fn std140_size(&self) -> usize {
        match self {
            Self::Float | Self::Int => 4,
            Self::Vec2 => 8,
            Self::Vec3 | Self::Vec4 => 16,
            Self::Mat3 => 48,
            Self::Mat4 => 64,
        }
    }

    pub fn std140_align(&self) -> usize {
        match self {
            Self::Float | Self::Int => 4,
            Self::Vec2 => 8,
            _ => 16,
        }
    }

    pub fn glsl(&self) -> &'static str {
        match self {
            Self::Float => "float",
            Self::Vec2 => "vec2",
            Self::Vec3 => "vec3",
            Self::Vec4 => "vec4",
            Self::Mat3 => "mat3",
            Self::Mat4 => "mat4",
            Self::Int => "int",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Uniform {
    pub name: String,
    pub kind: UniformKind,
    pub default: Option<Vec<f32>>,
    pub material: Option<String>,
    pub offset: usize,
    pub count: usize,
}

impl Uniform {
    #[must_use]
    pub fn std140_span(&self) -> usize {
        if self.count > 1 {
            let stride = self.kind.std140_size().div_ceil(16) * 16;
            return stride * self.count;
        }
        self.kind.std140_size()
    }

    #[must_use]
    pub fn std140_align(&self) -> usize {
        if self.count > 1 { 16 } else { self.kind.std140_align() }
    }
}

fn array_count(name: &str) -> usize {
    let Some(open) = name.find('[') else {
        return 1;
    };
    let Some(close) = name[open..].find(']') else {
        return 1;
    };
    name[open + 1..open + close].trim().parse::<usize>().unwrap_or(1).max(1)
}

#[derive(Clone, Debug)]
pub struct Sampler {
    pub name: String,
    pub index: u32,
    pub default: Option<String>,
    pub combo: Option<String>,
}

pub struct Translated {
    pub source: String,
    pub uniforms: Vec<Uniform>,
    pub samplers: Vec<Sampler>,
    pub block_size: usize,
}

fn annotation_default(line: &str) -> Option<serde_json::Value> {
    let comment = line.split("//").nth(1)?.trim();
    if !comment.starts_with('{') {
        return None;
    }
    crate::json::parse(comment.as_bytes()).ok()
}

fn default_values(kind: &UniformKind, note: Option<&serde_json::Value>) -> Option<Vec<f32>> {
    let raw = note?.get("default")?;
    let mut out = Vec::new();
    match raw {
        serde_json::Value::Number(num) => out.push(num.as_f64()? as f32),
        serde_json::Value::String(text) => {
            for part in text.split_whitespace() {
                out.push(part.parse().ok()?);
            }
        }
        serde_json::Value::Bool(flag) => out.push(f32::from(u8::from(*flag))),
        _ => return None,
    }
    let want = match kind {
        UniformKind::Float | UniformKind::Int => 1,
        UniformKind::Vec2 => 2,
        UniformKind::Vec3 => 3,
        UniformKind::Vec4 => 4,
        _ => return None,
    };
    while out.len() < want {
        let fill = *out.last().unwrap_or(&0.0);
        out.push(fill);
    }
    out.truncate(want);
    Some(out)
}

pub const PROVIDED_INCLUDES: [&str; 1] = ["common.h"];

pub fn resolve_includes(
    source: &str,
    load: &dyn Fn(&str) -> Option<String>,
    depth: usize,
) -> Result<String> {
    if depth > 8 {
        return Err(anyhow!("shader include depth exceeded"));
    }
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("#include") {
            let name = rest.trim().trim_matches(['"', '<', '>', ' ']);
            if PROVIDED_INCLUDES.contains(&name) {
                continue;
            }
            match load(name) {
                Some(text) => {
                    out.push_str(&resolve_includes(&text, load, depth + 1)?);
                    out.push('\n');
                }
                None => {
                    let _ = writeln!(out, "// missing include {name}");
                }
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

const RESERVED: [&str; 8] =
    ["sample", "filter", "input", "output", "active", "partition", "resource", "common"];

fn is_word_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn rename_word(text: &str, from: &str, to: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx..].starts_with(from.as_bytes()) {
            let before_ok = idx == 0 || !is_word_char(bytes[idx - 1]);
            let after = idx + from.len();
            let after_ok = after >= bytes.len() || !is_word_char(bytes[after]);
            if before_ok && after_ok {
                out.push_str(to);
                idx = after;
                continue;
            }
        }
        out.push(bytes[idx] as char);
        idx += 1;
    }
    out
}

fn conditional_symbols(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let rest = if let Some(rest) = trimmed.strip_prefix("#if ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("#elif ") {
            rest
        } else {
            continue;
        };
        let mut word = String::new();
        let mut skip_next = false;
        for ch in rest.chars().chain(std::iter::once(' ')) {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                word.push(ch);
                continue;
            }
            if !word.is_empty() {
                let taken = std::mem::take(&mut word);
                if taken == "defined" {
                    skip_next = true;
                } else if skip_next {
                    skip_next = false;
                } else if !taken.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
                    out.push(taken);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn defined_symbols(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix("#define ")?;
            rest.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .find(|word| !word.is_empty())
                .map(String::from)
        })
        .collect()
}

fn varying_locations(decl: &str) -> u32 {
    let Some(open) = decl.find('[') else {
        return 1;
    };
    let Some(close) = decl[open..].find(']') else {
        return 1;
    };
    decl[open + 1..open + close].trim().parse::<u32>().unwrap_or(1).max(1)
}

const PROMOTE_FNS: [&str; 11] = [
    "max",
    "min",
    "clamp",
    "mix",
    "pow",
    "step",
    "smoothstep",
    "mod",
    "atan",
    "reflect",
    "refract",
];

fn is_numeric_literal(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && trimmed.parse::<f64>().is_ok()
        && trimmed
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.' || byte == b'-' || byte == b'+')
}

fn split_args(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut bracket = 0i32;
    let mut current = String::new();
    for ch in inner.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            '[' => bracket += 1,
            ']' => bracket -= 1,
            ',' if depth == 0 && bracket == 0 => {
                out.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    if !current.trim().is_empty() || !out.is_empty() {
        out.push(current);
    }
    out
}

fn fix_math_calls(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut idx = 0;
    while idx < bytes.len() {
        let matched = PROMOTE_FNS.iter().find(|name| {
            bytes[idx..].starts_with(name.as_bytes())
                && bytes.get(idx + name.len()) == Some(&b'(')
                && (idx == 0 || !is_word_char(bytes[idx - 1]))
        });
        let Some(name) = matched else {
            out.push(bytes[idx] as char);
            idx += 1;
            continue;
        };
        let mut cursor = idx + name.len() + 1;
        let mut depth = 0i32;
        let start = cursor;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'(' => depth += 1,
                b')' if depth == 0 => break,
                b')' => depth -= 1,
                _ => {}
            }
            cursor += 1;
        }
        if cursor >= bytes.len() {
            out.push_str(&source[idx..]);
            break;
        }
        let inner = &source[start..cursor];
        let mut args = split_args(inner);
        for arg in &mut args {
            if is_numeric_literal(arg) && !arg.contains('.') {
                let lead: String = arg.chars().take_while(|ch| ch.is_whitespace()).collect();
                *arg = format!("{lead}{}.0", arg.trim());
            }
        }
        if (*name == "max" || *name == "min")
            && args.len() == 2
            && is_numeric_literal(&args[0])
            && !is_numeric_literal(&args[1])
        {
            args.swap(0, 1);
        }
        out.push_str(name);
        out.push('(');
        out.push_str(&args.join(","));
        out.push(')');
        idx = cursor + 1;
    }
    out
}

fn varying_name(decl: &str) -> String {
    decl.trim_end_matches(';')
        .split_whitespace()
        .last()
        .unwrap_or("")
        .split('[')
        .next()
        .unwrap_or("")
        .to_string()
}

fn varying_widths(source: &str) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    for line in source.lines() {
        let Some(rest) = line.trim_start().strip_prefix("varying ") else {
            continue;
        };
        let decl = rest.trim_end_matches(';');
        if decl.contains('[') {
            continue;
        }
        let width = match decl.split_whitespace().next() {
            Some("float") => 1,
            Some("vec2") => 2,
            Some("vec3") => 3,
            Some("vec4") => 4,
            _ => continue,
        };
        out.insert(varying_name(decl), width);
    }
    out
}

fn widen_stage(source: &str, name: &str, from: u32, to: u32) -> String {
    let from_ty = ["float", "vec2", "vec3", "vec4"][from as usize - 1];
    let to_ty = ["float", "vec2", "vec3", "vec4"][to as usize - 1];
    let fill = match to - from {
        1 if to == 4 => ", 1.0",
        1 => ", 0.0",
        2 if to == 4 => ", 0.0, 1.0",
        2 => ", 0.0, 0.0",
        _ => ", 0.0, 0.0, 1.0",
    };
    let mut out = String::with_capacity(source.len() + 64);
    for line in source.lines() {
        let trimmed = line.trim_start();
        let decl = format!("varying {from_ty} {name}");
        if trimmed.starts_with(&decl) && trimmed[decl.len()..].trim_start().starts_with(';') {
            let indent = &line[..line.len() - trimmed.len()];
            let _ = writeln!(out, "{indent}varying {to_ty} {name};");
            continue;
        }
        let assign = format!("{name} =");
        if let Some(rest) = trimmed.strip_prefix(&assign)
            && !rest.starts_with('=')
            && let Some(expr) = rest.strip_suffix(';')
        {
            let indent = &line[..line.len() - trimmed.len()];
            let _ = writeln!(out, "{indent}{name} = {to_ty}({}{fill});", expr.trim());
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[must_use]
pub fn unify_varying_types(vertex: &str, fragment: &str) -> (String, String) {
    let vert_widths = varying_widths(vertex);
    let frag_widths = varying_widths(fragment);
    let mut vert = vertex.to_string();
    let mut frag = fragment.to_string();
    for (name, vert_width) in &vert_widths {
        let Some(frag_width) = frag_widths.get(name) else {
            continue;
        };
        if vert_width < frag_width {
            vert = widen_stage(&vert, name, *vert_width, *frag_width);
        } else if frag_width < vert_width {
            frag = widen_stage(&frag, name, *frag_width, *vert_width);
        }
    }
    (vert, frag)
}

#[must_use]
pub fn varying_map(vertex: &str, fragment: &str) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    let mut next = 0u32;
    for source in [vertex, fragment] {
        for line in source.lines() {
            let code = line.trim_start();
            let Some(rest) = code.strip_prefix("varying ") else {
                continue;
            };
            let decl = rest.trim_end_matches(';');
            let name = varying_name(decl);
            if name.is_empty() || out.contains_key(&name) {
                continue;
            }
            out.insert(name, next);
            next += varying_locations(decl);
        }
    }
    out
}

#[must_use]
pub fn combo_defaults(source: &str) -> BTreeMap<String, i64> {
    let mut out = BTreeMap::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("//").map(str::trim_start) else {
            continue;
        };
        let Some(json) = rest.strip_prefix("[COMBO]").map(str::trim) else {
            continue;
        };
        let Ok(note) = crate::json::parse(json.as_bytes()) else {
            continue;
        };
        let Some(name) = note.get("combo").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let value = note.get("default").and_then(serde_json::Value::as_i64).unwrap_or(0);
        out.entry(name.to_string()).or_insert(value);
    }
    out
}

pub fn translate(source: &str, stage: Stage, combos: &BTreeMap<String, i64>) -> Translated {
    translate_with(source, stage, combos, None)
}

pub fn translate_with(
    source: &str,
    stage: Stage,
    combos: &BTreeMap<String, i64>,
    varyings: Option<&BTreeMap<String, u32>>,
) -> Translated {
    let mut source = source.to_string();
    for word in RESERVED {
        if source.contains(word) {
            source = rename_word(&source, word, &format!("skwd_{word}"));
        }
    }
    let source = fix_math_calls(&source);
    let source = source.as_str();
    let mut body = String::new();
    let mut uniforms: Vec<Uniform> = Vec::new();
    let mut samplers: Vec<Sampler> = Vec::new();
    let mut varying_loc = 0u32;
    let mut attribute_loc = 0u32;
    let mut uses_frag_color = false;

    for raw_line in source.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim_start();
        if trimmed.starts_with("#version") {
            continue;
        }
        let code = trimmed.split("//").next().unwrap_or("").trim();

        if let Some(rest) = code.strip_prefix("uniform ") {
            let mut words = rest.trim_end_matches(';').split_whitespace();
            let (Some(ty), Some(name_raw)) = (words.next(), words.next()) else {
                body.push_str(line);
                body.push('\n');
                continue;
            };
            let name = name_raw.trim_end_matches(';').trim();
            let note = annotation_default(line);
            if ty.starts_with("sampler") {
                let index = name
                    .strip_prefix("g_Texture")
                    .and_then(|digits| digits.parse::<u32>().ok())
                    .unwrap_or(samplers.len() as u32);
                samplers.push(Sampler {
                    name: name.to_string(),
                    index,
                    combo: note
                        .as_ref()
                        .and_then(|note| note.get("combo"))
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                    default: note
                        .as_ref()
                        .and_then(|note| note.get("default"))
                        .and_then(|value| value.as_str())
                        .map(String::from),
                });
                continue;
            }
            if let Some(kind) = UniformKind::parse(ty) {
                let default = default_values(&kind, note.as_ref());
                let material = note
                    .as_ref()
                    .and_then(|note| note.get("material"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                uniforms.push(Uniform {
                    name: name.to_string(),
                    kind,
                    default,
                    material,
                    offset: 0,
                    count: array_count(name),
                });
                continue;
            }
            body.push_str(line);
            body.push('\n');
            continue;
        }

        if let Some(rest) = code.strip_prefix("varying ") {
            let decl = rest.trim_end_matches(';');
            let direction = if stage == Stage::Vertex { "out" } else { "in" };
            let location = varyings
                .and_then(|map| map.get(&varying_name(decl)).copied())
                .unwrap_or(varying_loc);
            let _ = writeln!(body, "layout(location = {location}) {direction} {decl};");
            varying_loc = location + varying_locations(decl);
            continue;
        }
        if let Some(rest) = code.strip_prefix("attribute ") {
            let decl = rest.trim_end_matches(';');
            let _ = writeln!(body, "layout(location = {attribute_loc}) in {decl};");
            attribute_loc += 1;
            continue;
        }
        if line.contains("gl_FragColor") {
            uses_frag_color = true;
        }
        body.push_str(line);
        body.push('\n');
    }

    let mut offset = 0usize;
    for uniform in &mut uniforms {
        let align = uniform.std140_align();
        offset = offset.div_ceil(align) * align;
        uniform.offset = offset;
        offset += uniform.std140_span();
    }
    let block_size = offset.div_ceil(16) * 16;

    let mut out = String::with_capacity(source.len() + PRELUDE.len() + 512);
    out.push_str(PRELUDE);
    for (key, value) in combos {
        let _ = writeln!(out, "#define {key} {value}");
    }
    let declared = defined_symbols(source);
    for symbol in conditional_symbols(source) {
        if !combos.contains_key(&symbol) && !declared.contains(&symbol) {
            let _ = writeln!(out, "#define {symbol} 0");
        }
    }
    if !uniforms.is_empty() {
        out.push_str("layout(std140, set = 0, binding = 0) uniform SkwdParams {\n");
        for uniform in &uniforms {
            let _ = writeln!(out, "    {} {};", uniform.kind.glsl(), uniform.name);
        }
        out.push_str("};\n");
    }
    for sampler in &samplers {
        let _ = writeln!(
            out,
            "layout(set = 0, binding = {}) uniform sampler2D {};",
            sampler.index + 1,
            sampler.name
        );
    }
    if stage == Stage::Fragment && uses_frag_color {
        out.push_str("layout(location = 0) out vec4 skwd_FragColor;\n");
        out.push_str("#define gl_FragColor skwd_FragColor\n");
    }
    out.push_str(&body);

    Translated { source: out, uniforms, samplers, block_size: block_size.max(16) }
}

fn block_text(uniforms: &[Uniform]) -> String {
    if uniforms.is_empty() {
        return String::new();
    }
    let mut out = String::from("layout(std140, set = 0, binding = 0) uniform SkwdParams {\n");
    for uniform in uniforms {
        let _ = writeln!(out, "    {} {};", uniform.kind.glsl(), uniform.name);
    }
    out.push_str("};\n");
    out
}

fn replace_block(source: &str, block: &str) -> String {
    let Some(start) = source.find("layout(std140, set = 0, binding = 0) uniform SkwdParams {")
    else {
        let anchor = source
            .find("layout(set = 0, binding = ")
            .or_else(|| source.find("layout(location = "))
            .or_else(|| source.find("void main"))
            .unwrap_or(source.len());
        let mut out = String::with_capacity(source.len() + block.len());
        out.push_str(&source[..anchor]);
        out.push_str(block);
        out.push_str(&source[anchor..]);
        return out;
    };
    let Some(end) = source[start..].find("};\n").map(|at| start + at + 3) else {
        return source.to_string();
    };
    let mut out = String::with_capacity(source.len() + block.len());
    out.push_str(&source[..start]);
    out.push_str(block);
    out.push_str(&source[end..]);
    out
}

pub fn unify_uniforms(vertex: &mut Translated, fragment: &mut Translated) {
    let mut union: Vec<Uniform> = vertex.uniforms.clone();
    for uniform in &fragment.uniforms {
        if !union.iter().any(|known| known.name == uniform.name) {
            union.push(uniform.clone());
        }
    }
    let mut offset = 0usize;
    for uniform in &mut union {
        let align = uniform.std140_align();
        offset = offset.div_ceil(align) * align;
        uniform.offset = offset;
        offset += uniform.std140_span();
    }
    let size = offset.div_ceil(16) * 16;
    let block = block_text(&union);
    vertex.source = replace_block(&vertex.source, &block);
    fragment.source = replace_block(&fragment.source, &block);
    vertex.uniforms.clone_from(&union);
    fragment.uniforms = union;
    vertex.block_size = size.max(16);
    fragment.block_size = size.max(16);
}

const IDENTITY4: [f32; 16] =
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
const IDENTITY3_STD140: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

pub fn pack_uniforms(uniforms: &[Uniform], values: &BTreeMap<String, Vec<f32>>) -> Vec<u8> {
    let size =
        uniforms.iter().map(|uniform| uniform.offset + uniform.std140_span()).max().unwrap_or(16);
    let mut out = vec![0u8; size.div_ceil(16) * 16];
    let identity4 = IDENTITY4.to_vec();
    let identity3 = IDENTITY3_STD140.to_vec();
    for uniform in uniforms {
        let fallback = match uniform.kind {
            UniformKind::Mat4 => Some(&identity4),
            UniformKind::Mat3 => Some(&identity3),
            _ => None,
        };
        let Some(data) = values.get(&uniform.name).or(uniform.default.as_ref()).or(fallback) else {
            continue;
        };
        let lanes = uniform.kind.std140_size() / 4;
        let stride =
            if uniform.count > 1 { uniform.kind.std140_size().div_ceil(16) * 16 } else { 0 };
        for (idx, value) in data.iter().take(lanes * uniform.count).enumerate() {
            let at = uniform.offset + (idx / lanes) * stride + (idx % lanes) * 4;
            if at + 4 <= out.len() {
                out[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    out
}

pub fn spirv_cache_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_CACHE_HOME").map_or_else(
        |_| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache"),
        std::path::PathBuf::from,
    );
    base.join("skwd-wall/scene-shaders")
}

fn cache_temp_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("{}.{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

fn cache_key(source: &str, stage: Stage) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in source.as_bytes() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    let tag = if stage == Stage::Vertex { "vert" } else { "frag" };
    format!("{hash:016x}.{tag}.spv")
}

pub fn compile(source: &str, stage: Stage, label: &str) -> Result<Vec<u32>> {
    const MAX_CACHED_SPIRV_BYTES: u64 = 16 * 1024 * 1024;
    let dir = spirv_cache_dir();
    let path = dir.join(cache_key(source, stage));
    let cached = std::fs::File::open(&path).ok().and_then(|file| {
        use std::io::Read;
        if file.metadata().ok()?.len() > MAX_CACHED_SPIRV_BYTES {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(MAX_CACHED_SPIRV_BYTES + 1).read_to_end(&mut bytes).ok()?;
        (bytes.len() as u64 <= MAX_CACHED_SPIRV_BYTES).then_some(bytes)
    });
    if let Some(bytes) = cached
        && bytes.len() % 4 == 0
        && !bytes.is_empty()
    {
        return Ok(bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect());
    }
    let compiler = shaderc::Compiler::new().map_err(|err| anyhow!("shaderc init: {err}"))?;
    let mut options =
        shaderc::CompileOptions::new().map_err(|err| anyhow!("shaderc options: {err}"))?;
    options.set_target_env(shaderc::TargetEnv::Vulkan, shaderc::EnvVersion::Vulkan1_2 as u32);
    options.set_optimization_level(shaderc::OptimizationLevel::Performance);
    let kind = match stage {
        Stage::Vertex => shaderc::ShaderKind::Vertex,
        Stage::Fragment => shaderc::ShaderKind::Fragment,
    };
    let artifact = compiler
        .compile_into_spirv(source, kind, label, "main", Some(&options))
        .map_err(|err| anyhow!("compile {label}: {err}"))?;
    let words = artifact.as_binary().to_vec();
    let _ = std::fs::create_dir_all(&dir);
    let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    let tmp = path.with_extension(format!("{}.tmp", cache_temp_suffix()));
    if let Ok(mut file) =
        std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)
    {
        if file.write_all(&bytes).is_ok() && file.sync_all().is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(words)
}
