use anyhow::{Result, anyhow};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

pub(crate) const GLSL_UINT: &str = "uint";
const GLSL_MAT4X3: &str = "mat4x3";
const SAMPLER_SOURCE: &str = "samplers";
const SAMPLER_UNIFORM: &str = "uniform sampler";
const SHADER_ENTRY: &str = "main";
const LINE_BREAK: &str = "\n";
const LINE_COMMENT: &str = "//";
const UNIFORM_DECL: &str = "uniform ";
const PRECISION_QUALIFIERS: [&str; 3] = ["lowp", "mediump", "highp"];
const MATERIAL_NOTE_FIELD: &str = "material";
const COMPONENTS_NOTE_FIELD: &str = "components";
const COMBO_NOTE_FIELD: &str = "combo";
const STD140_ARRAY_ALIGNMENT: usize = 16;
pub(crate) const DEFINE_PREFIX: &str = "#define ";
pub(crate) const UNDEF_PREFIX: &str = "#undef ";
const UNDEF_DIRECTIVE: &str = "undef";
const UNRESOLVED_UNIFORM_ARRAY: &str = "#error unresolved uniform array size\n";

pub const PRELUDE: &str = r"#version 450
#define CASTU(x) (uint(x))
#define CASTI(x) (int(x))
#define CASTF(x) (float(x))
#define CAST4U(x) (uvec4(x))
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
#define texSample2D(s, uv) texture(s, vec2(uv))
#define texSample2DLod(s, uv, l) textureLod(s, vec2(uv), l)
#define texSample2DGrad(s, uv, dx, dy) textureGrad(s, vec2(uv), dx, dy)
#define texSample2DCompare(s, uv, depth) vec4(texture(s, vec3(uv, depth)))
#define DECLARE_SAMPLER2D_PARAMETER(s) sampler2D s
#define MAKE_SAMPLER2D_ARGUMENT(s) s
#define mul(v, m) ((m) * (v))
vec2 mix(vec2 before, vec2 after, bool select) { return select ? after : before; }
";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Vertex,
    Geometry,
    Fragment,
}

mod geometry;
pub use geometry::{interfaces as geometry_interfaces, translate_geometry};

#[derive(Clone, Debug, PartialEq)]
pub enum UniformKind {
    Float,
    Vec2,
    Vec3,
    Vec4,
    Mat3,
    Mat4,
    Mat4x3,
    Int,
    Uint,
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
            GLSL_MAT4X3 => Self::Mat4x3,
            "int" | "bool" => Self::Int,
            GLSL_UINT => Self::Uint,
            _ => return None,
        })
    }

    pub fn std140_size(&self) -> usize {
        match self {
            Self::Float | Self::Int | Self::Uint => 4,
            Self::Vec2 => 8,
            Self::Vec3 => 12,
            Self::Vec4 => 16,
            Self::Mat3 => 48,
            Self::Mat4 | Self::Mat4x3 => 64,
        }
    }

    pub fn std140_align(&self) -> usize {
        match self {
            Self::Float | Self::Int | Self::Uint => 4,
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
            Self::Mat4x3 => GLSL_MAT4X3,
            Self::Int => "int",
            Self::Uint => GLSL_UINT,
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
        if self.name.contains('[') {
            let stride =
                self.kind.std140_size().div_ceil(STD140_ARRAY_ALIGNMENT) * STD140_ARRAY_ALIGNMENT;
            return stride * self.count;
        }
        self.kind.std140_size()
    }

    #[must_use]
    pub fn std140_align(&self) -> usize {
        if self.name.contains('[') { STD140_ARRAY_ALIGNMENT } else { self.kind.std140_align() }
    }
}

#[derive(Clone, Debug)]
pub struct Sampler {
    pub comparison: bool,
    pub material: Option<String>,
    pub name: String,
    pub index: u32,
    pub default: Option<String>,
    pub combo: Option<String>,
    pub components: Vec<Option<String>>,
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

#[derive(serde::Serialize)]
pub(crate) struct MaterialBinding {
    pub uniform: String,
    pub value: Option<Vec<f32>>,
    pub width: usize,
}

pub(crate) fn material_bindings(source: &str) -> BTreeMap<String, MaterialBinding> {
    let mut bindings = BTreeMap::new();
    for line in source.lines() {
        let code = line.trim_start().split(LINE_COMMENT).next().unwrap_or("").trim();
        let Some(rest) = code.strip_prefix(UNIFORM_DECL) else { continue };
        let mut words = rest.trim_end_matches(';').split_whitespace();
        let (Some(kind), Some(name)) = (words.next(), words.next()) else { continue };
        let Some(kind) = UniformKind::parse(kind) else { continue };
        let name = name.trim_end_matches(';').trim();
        let Some(note) = annotation_default(line) else { continue };
        let Some(material) = note.get(MATERIAL_NOTE_FIELD).and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        bindings.insert(
            material.to_string(),
            MaterialBinding {
                uniform: name.to_string(),
                value: default_values(&kind, Some(&note)),
                width: kind.std140_size() / size_of::<f32>(),
            },
        );
    }
    bindings
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
        UniformKind::Float | UniformKind::Int | UniformKind::Uint => 1,
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

pub const COMMON_H_FALLBACK: &str = r"#define M_PI 3.14159265359
#define M_PI_HALF 1.57079632679
#define M_PI_2 6.28318530718
#define SQRT_2 1.41421356237
#define SQRT_3 1.73205080756
vec3 hsv2rgb(vec3 c) {
    vec4 k = vec4(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    vec3 p = abs(frac(c.xxx + k.xyz) * 6.0 - k.www);
    return c.z * mix(k.xxx, saturate(p - k.xxx), c.y);
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
";

const COMMON_H_OVERLOADS: &str = r"vec2 rotateVec2(vec4 v, float r) {
    return rotateVec2(v.xy, r);
}
float greyscale(vec4 color) {
    return greyscale(color.xyz);
}
";

const LIGHTING_V1: &str = include_str!("shader/lighting.glsl");
pub const SHADOW_TEXTURE: &str = "SHADOW_TEXTURE";
const SHADOW_SAMPLER: &str = "g_Texture6";
const SHADOW_BEGIN: &str = "#if SKWD_LIGHT_SHADOWS\n";
const SHADOW_END: &str = "#endif\n";
pub const SHADOW_SAMPLE: &str = include_str!("shader/shadow.glsl");

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
        if let Some(module) = trimmed.strip_prefix("#require") {
            if module.trim() == "LightingV1" {
                out.push_str(SHADOW_BEGIN);
                out.push_str(&SHADOW_SAMPLE.replace(SHADOW_TEXTURE, SHADOW_SAMPLER));
                out.push_str(SHADOW_END);
                out.push_str(LIGHTING_V1);
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("#include") {
            let name = rest.trim().trim_matches(['"', '<', '>', ' ']);
            match load(name) {
                Some(text) => {
                    out.push_str(&resolve_includes(&text, load, depth + 1)?);
                    out.push('\n');
                }
                None if name == "common.h" => {
                    out.push_str(COMMON_H_FALLBACK);
                }
                None => {
                    let _ = writeln!(out, "// missing include {name}");
                }
            }
            if name == "common.h" {
                out.push_str(COMMON_H_OVERLOADS);
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

pub(crate) fn rename_authored_mod(source: &str) -> String {
    const MOD: &str = "mod";
    const AUTHORED_MOD: &str = "skwd_authored_mod";
    let declared = source.lines().any(|line| {
        let code = line.split(LINE_COMMENT).next().unwrap_or_default();
        code.split([';', '{', '}']).any(|statement| {
            let Some((head, _)) = statement.split_once('(') else { return false };
            let mut words = head.split_whitespace().rev();
            words.next() == Some(MOD) && words.next().and_then(UniformKind::parse).is_some()
        })
    });
    if declared { rename_word(source, MOD, AUTHORED_MOD) } else { source.to_owned() }
}

fn is_word_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[must_use]
pub fn utf8_width(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
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
        let width = utf8_width(bytes[idx]);
        out.push_str(&text[idx..idx + width]);
        idx += width;
    }
    out
}

fn conditional_symbols(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start().split("//").next().unwrap_or("");
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
            let rest = line.trim_start().strip_prefix(DEFINE_PREFIX)?;
            rest.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .find(|word| !word.is_empty())
                .map(String::from)
        })
        .collect()
}

fn replace_array_size(decl: &str, count: u32) -> String {
    match (decl.find('['), decl.find(']')) {
        (Some(open), Some(close)) if close > open => {
            format!("{}[{count}]{}", &decl[..open], &decl[close + 1..])
        }
        _ => decl.to_string(),
    }
}

fn array_size(decl: &str, combos: &BTreeMap<String, i64>) -> Option<u32> {
    let open = decl.find('[')?;
    let close = open + decl[open..].find(']')?;
    let text = decl[open + 1..close].trim();
    if let Ok(count) = text.parse::<u32>() {
        return Some(count.max(1));
    }
    let value = combos.get(text).copied()?;
    u32::try_from(value).ok().map(|count| count.max(1))
}

fn uniform_array_size(
    decl: &str,
    combos: &BTreeMap<String, i64>,
    macros: &BTreeMap<String, Option<String>>,
) -> Option<u32> {
    let open = decl.find('[')?;
    let close = open + decl[open..].find(']')?;
    let mut text = decl[open + 1..close].trim();
    let mut seen = Vec::new();
    loop {
        if let Ok(count) = text.parse::<u32>() {
            return (count > 0).then_some(count);
        }
        if text.is_empty() || !text.bytes().all(is_word_char) || seen.contains(&text) {
            return None;
        }
        seen.push(text);
        match macros.get(text) {
            Some(Some(value)) => text = value.trim(),
            Some(None) => return None,
            None => {
                let count = u32::try_from(*combos.get(text)?).ok()?;
                return (count > 0).then_some(count);
            }
        }
    }
}

fn uniform_decl(uniform: &Uniform) -> String {
    if uniform.name.contains('[') {
        replace_array_size(&uniform.name, uniform.count as u32)
    } else {
        uniform.name.clone()
    }
}

fn varying_locations(decl: &str, combos: &BTreeMap<String, i64>) -> u32 {
    if decl.contains('[') { array_size(decl, combos).unwrap_or(1).max(1) } else { 1 }
}

fn array_varying(decl: &str, combos: &BTreeMap<String, i64>) -> Option<(u32, u32)> {
    let count = array_size(decl, combos)?;
    let width = vector_width(decl.split_whitespace().next().unwrap_or(""));
    (width > 0 && width < PACKED_COMPONENTS).then_some((width, count))
}

fn packed_locations(decl: &str, combos: &BTreeMap<String, i64>) -> u32 {
    match array_varying(decl, combos) {
        Some((width, count)) => VaryingPack { width, count }.slots(),
        None => varying_locations(decl, combos),
    }
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
            let width = utf8_width(bytes[idx]);
            out.push_str(&source[idx..idx + width]);
            idx += width;
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

fn cast_conditions(source: &str) -> String {
    const BLOCK_START: &[u8] = b"/*";
    const BLOCK_END: &[u8] = b"*/";
    const IF_KEYWORD: &[u8] = b"if";
    const CAST_START: &str = "bool(";
    const CAST_END: &str = "))";

    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut idx = 0;
    let mut line_start = 0;
    while idx < bytes.len() {
        if bytes[idx..].starts_with(LINE_COMMENT.as_bytes()) {
            let end = bytes[idx..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |end| idx + end);
            out.push_str(&source[idx..end]);
            idx = end;
            continue;
        }
        if bytes[idx..].starts_with(BLOCK_START) {
            let end = bytes[idx + BLOCK_START.len()..]
                .windows(BLOCK_END.len())
                .position(|pair| pair == BLOCK_END)
                .map_or(bytes.len(), |end| idx + end + BLOCK_START.len() + BLOCK_END.len());
            out.push_str(&source[idx..end]);
            line_start = source[..end].rfind('\n').map_or(line_start, |at| at + 1);
            idx = end;
            continue;
        }
        if bytes[idx..].starts_with(IF_KEYWORD)
            && (idx == 0 || !is_word_char(bytes[idx - 1]))
            && bytes.get(idx + IF_KEYWORD.len()).is_none_or(|byte| !is_word_char(*byte))
            && !source[line_start..idx].trim_start().starts_with('#')
        {
            let open =
                (idx + IF_KEYWORD.len()..bytes.len()).find(|at| !bytes[*at].is_ascii_whitespace());
            if let Some(open) = open
                && bytes[open] == b'('
            {
                let mut depth = 1;
                let mut end = open + 1;
                while end < bytes.len() && depth > 0 {
                    match bytes[end] {
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    end += 1;
                }
                if depth == 0 {
                    out.push_str(&source[idx..=open]);
                    out.push_str(CAST_START);
                    out.push_str(&source[open + 1..end - 1]);
                    out.push_str(CAST_END);
                    idx = end;
                    continue;
                }
            }
        }
        let width = utf8_width(bytes[idx]);
        out.push_str(&source[idx..idx + width]);
        if bytes[idx] == b'\n' {
            line_start = idx + 1;
        }
        idx += width;
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

struct Expr<'a> {
    text: &'a [u8],
    pos: usize,
    symbols: &'a dyn Fn(&str) -> Option<i64>,
}

impl Expr<'_> {
    fn skip(&mut self) {
        while self.pos < self.text.len() && self.text[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self, token: &str) -> bool {
        self.skip();
        self.text[self.pos..].starts_with(token.as_bytes())
    }

    fn eat(&mut self, token: &str) -> bool {
        if self.peek(token) {
            self.pos += token.len();
            return true;
        }
        false
    }

    fn word(&mut self) -> Option<String> {
        self.skip();
        let start = self.pos;
        while self.pos < self.text.len() && is_word_char(self.text[self.pos]) {
            self.pos += 1;
        }
        (self.pos > start)
            .then(|| String::from_utf8_lossy(&self.text[start..self.pos]).into_owned())
    }

    fn primary(&mut self) -> i64 {
        if self.eat("!") {
            return i64::from(self.primary() == 0);
        }
        if self.eat("(") {
            let value = self.or();
            self.eat(")");
            return value;
        }
        let Some(word) = self.word() else {
            return 0;
        };
        if word == "defined" {
            let paren = self.eat("(");
            let name = self.word().unwrap_or_default();
            if paren {
                self.eat(")");
            }
            return i64::from((self.symbols)(&name).is_some());
        }
        if word.bytes().next().is_some_and(|b| b.is_ascii_digit()) {
            return word.parse().unwrap_or(0);
        }
        (self.symbols)(&word).unwrap_or(0)
    }

    fn compare(&mut self) -> i64 {
        let mut left = self.primary();
        loop {
            let (token, result): (&str, fn(i64, i64) -> bool) = if self.peek("==") {
                ("==", |a, b| a == b)
            } else if self.peek("!=") {
                ("!=", |a, b| a != b)
            } else if self.peek("<=") {
                ("<=", |a, b| a <= b)
            } else if self.peek(">=") {
                (">=", |a, b| a >= b)
            } else if self.peek("<") {
                ("<", |a, b| a < b)
            } else if self.peek(">") {
                (">", |a, b| a > b)
            } else {
                return left;
            };
            self.eat(token);
            let right = self.primary();
            left = i64::from(result(left, right));
        }
    }

    fn and(&mut self) -> i64 {
        let mut left = self.compare();
        while self.eat("&&") {
            let right = self.compare();
            left = i64::from(left != 0 && right != 0);
        }
        left
    }

    fn or(&mut self) -> i64 {
        let mut left = self.and();
        while self.eat("||") {
            let right = self.and();
            left = i64::from(left != 0 || right != 0);
        }
        left
    }
}

fn evaluate(expr: &str, symbols: &dyn Fn(&str) -> Option<i64>) -> bool {
    let mut parser = Expr { text: expr.as_bytes(), pos: 0, symbols };
    parser.or() != 0
}

#[must_use]
pub fn preprocess(source: &str, combos: &BTreeMap<String, i64>) -> String {
    let mut defines: BTreeMap<String, i64> = BTreeMap::new();
    let mut undefined = std::collections::BTreeSet::new();
    let mut stack: Vec<(bool, bool)> = Vec::new();
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        let active = stack.iter().all(|(taken, _)| *taken);
        let code = line.trim_start().split("//").next().unwrap_or("").trim_end();
        let directive = code.strip_prefix('#').map(str::trim_start);
        let lookup = |name: &str| {
            (!undefined.contains(name))
                .then(|| defines.get(name).or_else(|| combos.get(name)).copied())
                .flatten()
        };
        if let Some(rest) = directive {
            let end = rest.find(|ch: char| !ch.is_ascii_alphabetic()).unwrap_or(rest.len());
            let (word, arg) = rest.split_at(end);
            let arg = arg.trim_start();
            match word {
                "if" => {
                    let taken = active && evaluate(arg, &lookup);
                    stack.push((taken, taken));
                    continue;
                }
                "ifdef" | "ifndef" => {
                    let name = arg.split_whitespace().next().unwrap_or("");
                    let defined = lookup(name).is_some();
                    let taken = active && (defined == (word == "ifdef"));
                    stack.push((taken, taken));
                    continue;
                }
                "elif" | "else" => {
                    if let Some(last) = stack.len().checked_sub(1) {
                        let parent = stack[..last].iter().all(|(taken, _)| *taken);
                        let top = &mut stack[last];
                        let now = parent && !top.1 && (word == "else" || evaluate(arg, &lookup));
                        top.0 = now;
                        top.1 |= now || word == "else";
                    }
                    continue;
                }
                "endif" => {
                    stack.pop();
                    continue;
                }
                "define" if active => {
                    let mut parts = arg.split_whitespace();
                    if let Some(name) = parts.next() {
                        let name = name.split('(').next().unwrap_or(name);
                        let value =
                            parts.next().and_then(|raw| raw.parse::<i64>().ok()).unwrap_or(1);
                        undefined.remove(name);
                        defines.insert(name.to_string(), value);
                    }
                }
                UNDEF_DIRECTIVE if active => {
                    if let Some(name) = arg.split_whitespace().next() {
                        defines.remove(name);
                        undefined.insert(name.to_string());
                    }
                }
                _ => {}
            }
        }
        if active {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VaryingSlot {
    pub location: u32,
    pub width: u32,
}

fn vector_width(ty: &str) -> u32 {
    match ty {
        "float" => 1,
        "vec2" => 2,
        "vec3" => 3,
        "vec4" => 4,
        _ => 0,
    }
}

fn vector_type(width: u32) -> &'static str {
    ["float", "vec2", "vec3", "vec4"][width.clamp(1, 4) as usize - 1]
}

#[must_use]
pub fn varying_map(
    vertex: &str,
    fragment: &str,
    combos: &BTreeMap<String, i64>,
) -> BTreeMap<String, VaryingSlot> {
    let mut out: BTreeMap<String, VaryingSlot> = BTreeMap::new();
    let mut next = 0u32;
    for source in [vertex, fragment] {
        for line in source.lines() {
            let code = line.trim_start().split("//").next().unwrap_or("").trim();
            let Some(rest) = code.strip_prefix("varying ") else {
                continue;
            };
            let decl = rest.trim_end_matches(';');
            let name = varying_name(decl);
            if name.is_empty() {
                continue;
            }
            let width = if decl.contains('[') {
                0
            } else {
                vector_width(decl.split_whitespace().next().unwrap_or(""))
            };
            if let Some(slot) = out.get_mut(&name) {
                slot.width = slot.width.max(width);
                continue;
            }
            out.insert(name, VaryingSlot { location: next, width });
            next += packed_locations(decl, combos);
        }
    }
    out
}

struct Shadow {
    name: String,
    own: u32,
    width: u32,
    conditions: Vec<(String, bool)>,
}

impl Shadow {
    fn widened(&self) -> String {
        if self.own == self.width {
            return self.name.clone();
        }
        let fill = match (self.width - self.own, self.width) {
            (1, 4) => ", 1.0",
            (1, _) => ", 0.0",
            (2, 4) => ", 0.0, 1.0",
            (2, _) => ", 0.0, 0.0",
            _ => ", 0.0, 0.0, 1.0",
        };
        format!("{}({}{fill})", vector_type(self.width), self.name)
    }

    fn narrowed(&self) -> String {
        let io = format!("skwd_io_{}", self.name);
        if self.own == self.width {
            return io;
        }
        format!("{io}.{}", &"xyzw"[..self.own as usize])
    }

    fn write(&self, out: &mut String, statement: &str) {
        write_conditional(out, &self.conditions, statement);
    }
}

fn write_conditional(out: &mut String, conditions: &[(String, bool)], statement: &str) {
    for (directive, in_else) in conditions {
        out.push_str(directive);
        out.push('\n');
        if *in_else {
            out.push_str("#else\n");
        }
    }
    let _ = writeln!(out, "    {statement}");
    for _ in conditions {
        out.push_str(SHADOW_END);
    }
}

struct PackedArray {
    name: String,
    width: u32,
    count: u32,
    conditions: Vec<(String, bool)>,
}

impl PackedArray {
    fn slots(&self) -> u32 {
        VaryingPack { width: self.width, count: self.count }.slots()
    }

    fn copies(&self, out: &mut String, stage: Stage) {
        let pack = VaryingPack { width: self.width, count: self.count };
        for (io, local) in pack.bindings(&self.name) {
            let statement = if stage == Stage::Vertex {
                format!("{io} = {local};")
            } else {
                format!("{local} = {io};")
            };
            write_conditional(out, &self.conditions, &statement);
        }
    }
}

pub(crate) struct VaryingPack {
    pub width: u32,
    pub count: u32,
}

impl VaryingPack {
    pub fn slots(&self) -> u32 {
        (self.count * self.width).div_ceil(PACKED_COMPONENTS)
    }

    pub fn element(&self, index: u32) -> (u32, &'static str) {
        match self.width {
            1 => (index / PACKED_COMPONENTS, SCALAR_LANES[index as usize % SCALAR_LANES.len()]),
            2 => (
                index / VECTOR_LANES.len() as u32,
                VECTOR_LANES[index as usize % VECTOR_LANES.len()],
            ),
            _ => unreachable!(),
        }
    }

    pub fn bindings(&self, name: &str) -> Vec<(String, String)> {
        let mut bindings = Vec::new();
        for index in 0..self.count {
            let start = index * self.width;
            let end = start + self.width;
            for slot in start / PACKED_COMPONENTS..end.div_ceil(PACKED_COMPONENTS) {
                let offset = slot * PACKED_COMPONENTS;
                let first = start.max(offset);
                let last = end.min(offset + PACKED_COMPONENTS);
                let lanes = &COMPONENT_LANES[(first - offset) as usize..(last - offset) as usize];
                let io = format!("skwd_io_{name}[{slot}].{lanes}");
                let local = if last - first == self.width {
                    format!("{name}[{index}]")
                } else {
                    let lanes = &COMPONENT_LANES[(first - start) as usize..(last - start) as usize];
                    format!("{name}[{index}].{lanes}")
                };
                bindings.push((io, local));
            }
        }
        bindings
    }
}

pub(crate) const PACKED_COMPONENTS: u32 = 4;
const SCALAR_LANES: [&str; 4] = ["x", "y", "z", "w"];
const VECTOR_LANES: [&str; 2] = ["xy", "zw"];
const COMPONENT_LANES: &str = "xyzw";

fn track_condition(stack: &mut Vec<(String, bool)>, code: &str) {
    if code.starts_with("#if") {
        stack.push((code.to_string(), false));
    } else if let Some(rest) = code.strip_prefix("#elif") {
        if let Some(top) = stack.last_mut() {
            *top = (format!("#if{rest}"), false);
        }
    } else if code.starts_with("#else") {
        if let Some(top) = stack.last_mut() {
            top.1 = true;
        }
    } else if code.starts_with("#endif") {
        stack.pop();
    }
}

#[must_use]
fn zero_literal(ty: &str) -> Option<String> {
    let digits = |text: &str| text.chars().filter_map(|c| c.to_digit(10)).collect::<Vec<_>>();
    let value = match ty {
        "float" | "double" => "0.0".to_string(),
        "int" => "0".to_string(),
        "uint" => "0u".to_string(),
        "bool" => "false".to_string(),
        _ => {
            let (inner, kind) = if let Some(rest) = ty.strip_prefix("ivec") {
                ("0", digits(rest))
            } else if let Some(rest) = ty.strip_prefix("uvec") {
                ("0u", digits(rest))
            } else if let Some(rest) = ty.strip_prefix("bvec") {
                ("false", digits(rest))
            } else if let Some(rest) = ty.strip_prefix("vec") {
                ("0.0", digits(rest))
            } else if let Some(rest) = ty.strip_prefix("mat") {
                ("0.0", digits(rest))
            } else {
                return None;
            };
            let count = match kind.as_slice() {
                [rows] if (2..=4).contains(rows) => {
                    if ty.starts_with("mat") {
                        rows * rows
                    } else {
                        *rows
                    }
                }
                [rows, columns] if (2..=4).contains(rows) && (2..=4).contains(columns) => {
                    rows * columns
                }
                _ => return None,
            };
            let parts = std::iter::repeat_n(inner, count as usize).collect::<Vec<_>>().join(", ");
            format!("{ty}({parts})")
        }
    };
    Some(value)
}

fn signature_zero(code: &str) -> Option<String> {
    let open = code.find('(')?;
    code[open..].find(')')?;
    let head = code[..open].trim();
    let mut words = head.split_whitespace();
    let ty = words.next()?;
    let name = words.next()?;
    if words.next().is_some() || name == "main" || name == "skwd_main" {
        return None;
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    zero_literal(ty)
}

#[must_use]
pub fn close_non_void_functions(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + 256);
    let mut depth = 0i32;
    let mut header: Option<String> = None;
    let mut current: Option<String> = None;
    for line in source.lines() {
        let code = line.split("//").next().unwrap_or("");
        let opens = code.matches('{').count() as i32;
        let closes = code.matches('}').count() as i32;
        if depth == 0 {
            if opens > 0 {
                current = signature_zero(code).or_else(|| header.take());
            } else if !code.trim().is_empty() && !code.trim_start().starts_with('#') {
                header = code.trim_end().ends_with(')').then(|| signature_zero(code)).flatten();
            }
        }
        depth += opens - closes;
        if depth <= 0
            && closes > 0
            && let Some(zero) = current.take()
        {
            let at = line.rfind('}').unwrap_or(0);
            out.push_str(&line[..at]);
            let _ = writeln!(out, "return {zero};");
            out.push_str(&line[at..]);
            out.push('\n');
            depth = depth.max(0);
            continue;
        }
        depth = depth.max(0);
        out.push_str(line);
        out.push('\n');
    }
    out
}

pub(crate) fn reads_sampler(source: &str, combos: &BTreeMap<String, i64>, name: &str) -> bool {
    preprocess(source, combos)
        .lines()
        .map(|line| line.split(LINE_COMMENT).next().unwrap_or(""))
        .filter(|line| !line.contains(SAMPLER_UNIFORM))
        .any(|line| words_of(line).iter().any(|word| word == name))
}

pub(crate) fn words_of(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty() && !word.starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_string)
        .collect()
}

const COMPARISON_TYPES: [&str; 2] = ["sampler2DComparison", "sampler2DShadow"];
const SAMPLER_TYPE: &str = "sampler2D";

fn sampler_line(sampler: &Sampler) -> String {
    let kind = if sampler.comparison { COMPARISON_TYPES[1] } else { SAMPLER_TYPE };
    format!("layout(set = 0, binding = {}) uniform {kind} {};", sampler.index + 1, sampler.name)
}

const PRUNE_CACHE_LIMIT: usize = 256;

type PrunedWords = std::rc::Rc<[String]>;

thread_local! {
    static PRUNE_WORDS: std::cell::RefCell<std::collections::HashMap<(bool, u64), (String, PrunedWords)>> =
        std::cell::RefCell::default();
}

fn prune_words(code: &str, language: shaderc::SourceLanguage) -> Option<PrunedWords> {
    let mut hasher = std::hash::DefaultHasher::new();
    std::hash::Hash::hash(code, &mut hasher);
    let key = (language == shaderc::SourceLanguage::HLSL, std::hash::Hasher::finish(&hasher));
    let cached = PRUNE_WORDS.with_borrow(|cache| {
        cache.get(&key).filter(|(source, _)| source == code).map(|(_, words)| words.clone())
    });
    if cached.is_some() {
        return cached;
    }
    let compiler = shaderc::Compiler::new().ok()?;
    let mut options = shaderc::CompileOptions::new().ok()?;
    options.set_source_language(language);
    let preprocessed = compiler
        .preprocess(code, SAMPLER_SOURCE, SHADER_ENTRY, Some(&options))
        .inspect_err(|err| tracing::debug!(%err, "shader: sampler pruning skipped"))
        .ok()?;
    let words: PrunedWords = words_of(&preprocessed.as_text()).into();
    PRUNE_WORDS.with_borrow_mut(|cache| {
        if cache.len() >= PRUNE_CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, (code.to_owned(), words.clone()));
    });
    Some(words)
}

pub(crate) fn omit_unused_samplers(
    translated: &mut Translated,
    combos: &BTreeMap<String, i64>,
    present: &[bool],
    hlsl: Option<&str>,
) {
    let missing: Vec<_> = translated
        .samplers
        .iter()
        .filter(|sampler| {
            sampler.index != 0
                && sampler
                    .combo
                    .as_ref()
                    .is_some_and(|combo| combos.get(combo).copied().unwrap_or(0) == 0)
                && !present.get(sampler.index as usize).copied().unwrap_or(false)
        })
        .map(|sampler| sampler.name.clone())
        .collect();
    if missing.is_empty() {
        return;
    }
    let declarations: Vec<_> = translated.samplers.iter().map(sampler_line).collect();
    let glsl = translated
        .source
        .lines()
        .filter(|line| !declarations.iter().any(|declaration| *line == declaration))
        .collect::<Vec<_>>()
        .join(LINE_BREAK);
    let Some(glsl_words) = prune_words(&glsl, shaderc::SourceLanguage::GLSL) else { return };
    let hlsl_words = if let Some(source) = hlsl {
        let mut code = crate::hlsl::PRELUDE.to_string();
        for (name, value) in combos {
            let _ = writeln!(code, "#define {name} {value}");
        }
        for line in source.lines() {
            let declaration = line.split(LINE_COMMENT).next().unwrap_or("").trim();
            if declaration.starts_with(SAMPLER_UNIFORM) && declaration.ends_with(';') {
                continue;
            }
            if let Some(macro_name) = declaration
                .strip_prefix(DEFINE_PREFIX)
                .and_then(|rest| rest.split(|ch: char| ch == '(' || ch.is_whitespace()).next())
            {
                let _ = writeln!(code, "{UNDEF_PREFIX}{macro_name}");
            }
            code.push_str(line);
            code.push('\n');
        }
        match prune_words(&code, shaderc::SourceLanguage::HLSL) {
            Some(words) => words,
            None => return,
        }
    } else {
        PrunedWords::from([])
    };
    let omitted: Vec<_> = missing
        .into_iter()
        .filter(|name| !glsl_words.contains(name) && !hlsl_words.contains(name))
        .collect();
    if omitted.is_empty() {
        return;
    }
    let omitted_lines: Vec<_> = translated
        .samplers
        .iter()
        .filter(|sampler| omitted.contains(&sampler.name))
        .map(sampler_line)
        .collect();
    translated.samplers.retain(|sampler| !omitted.contains(&sampler.name));
    translated.source = translated
        .source
        .lines()
        .filter(|line| !omitted_lines.iter().any(|declaration| *line == declaration))
        .collect::<Vec<_>>()
        .join(LINE_BREAK);
    translated.source.push('\n');
}

fn dynamic_words(source: &str) -> std::collections::BTreeSet<String> {
    let mut dynamic: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut defines: Vec<(String, String)> = Vec::new();
    for line in source.lines() {
        let code = line.split("//").next().unwrap_or("").trim();
        if let Some(rest) = code.strip_prefix(UNIFORM_DECL) {
            let decl = rest.trim_end_matches(';');
            let name = decl.split(['[', '=']).next().unwrap_or(decl);
            if let Some(word) = name.split_whitespace().last() {
                dynamic.insert(word.to_string());
            }
        } else if let Some(rest) = code.strip_prefix(DEFINE_PREFIX) {
            let rest = rest.trim();
            let split = rest.find(|c: char| c.is_whitespace() || c == '(').unwrap_or(rest.len());
            let (name, body) = rest.split_at(split);
            if !body.starts_with('(') {
                defines.push((name.to_string(), body.to_string()));
            }
        }
    }
    loop {
        let before = dynamic.len();
        for (name, body) in &defines {
            if words_of(body).iter().any(|word| dynamic.contains(word)) {
                dynamic.insert(name.clone());
            }
        }
        if dynamic.len() == before {
            break;
        }
    }
    dynamic
}

#[must_use]
pub fn macro_dynamic_globals(source: &str) -> String {
    let dynamic = dynamic_words(source);
    if dynamic.is_empty() {
        return source.to_string();
    }
    let mut converted: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out = String::with_capacity(source.len() + 128);
    let mut depth = 0i32;
    for line in source.lines() {
        let code = line.split("//").next().unwrap_or("");
        let trimmed = code.trim();
        let mut rewritten = false;
        if depth == 0
            && let Some(rest) = trimmed.strip_prefix("const ")
            && let Some(eq) = rest.find('=')
            && rest.trim_end().ends_with(';')
        {
            let decl = rest[..eq].trim();
            let init = rest[eq + 1..].trim().trim_end_matches(';').trim();
            let mut parts = decl.split_whitespace();
            if let (Some(_), Some(name), None) = (parts.next(), parts.next(), parts.next())
                && !name.contains('[')
                && words_of(init)
                    .iter()
                    .any(|word| dynamic.contains(word) || converted.contains(word))
            {
                let _ = writeln!(out, "#define {name} ({init})");
                converted.insert(name.to_string());
                rewritten = true;
            }
        }
        depth += code.matches('{').count() as i32 - code.matches('}').count() as i32;
        depth = depth.max(0);
        if !rewritten {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[must_use]
pub fn relax_portability(source: &str) -> String {
    close_non_void_functions(&macro_dynamic_globals(source))
}

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
    varyings: Option<&BTreeMap<String, VaryingSlot>>,
) -> Translated {
    let mut source = rename_authored_mod(source);
    for word in RESERVED {
        if source.contains(word) {
            source = rename_word(&source, word, &format!("skwd_{word}"));
        }
    }
    let source = fix_math_calls(&source);
    let source = source.as_str();
    let mut known_macros = defined_symbols(PRELUDE);
    known_macros.extend(combos.keys().cloned());
    let mut array_macros: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut unresolved_uniform = false;
    let mut body = String::new();
    let mut uniforms: Vec<Uniform> = Vec::new();
    let mut samplers: Vec<Sampler> = Vec::new();
    let mut shadows: Vec<Shadow> = Vec::new();
    let mut packed: Vec<PackedArray> = Vec::new();
    let mut conditions: Vec<(String, bool)> = Vec::new();
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
        track_condition(&mut conditions, code);
        if let Some(rest) = code.strip_prefix(UNDEF_PREFIX)
            && let Some(name) = rest.split_whitespace().next()
        {
            array_macros.insert(name.to_string(), None);
        }
        if let Some(rest) = code.strip_prefix(DEFINE_PREFIX) {
            let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if !name.is_empty() {
                let value = (!name.contains('(') && !value.trim().is_empty())
                    .then(|| value.trim().to_string());
                array_macros.insert(name.to_string(), value);
            }
        }
        if let Some(rest) = code.strip_prefix(DEFINE_PREFIX)
            && let Some(name) =
                rest.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_')).next()
            && !name.is_empty()
        {
            if known_macros.iter().any(|known| known == name) {
                let _ = writeln!(body, "#undef {name}");
            } else {
                known_macros.push(name.to_string());
            }
        }

        if let Some(rest) = code.strip_prefix("uniform ") {
            let mut words = rest.trim_end_matches(';').split_whitespace();
            let ty = words.next();
            let ty =
                ty.filter(|word| !PRECISION_QUALIFIERS.contains(word)).or_else(|| words.next());
            let (Some(ty), Some(name_raw)) = (ty, words.next()) else {
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
                    comparison: COMPARISON_TYPES.contains(&ty),
                    material: note
                        .as_ref()
                        .and_then(|note| note.get(MATERIAL_NOTE_FIELD))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    name: name.to_string(),
                    index,
                    combo: note
                        .as_ref()
                        .and_then(|note| note.get(COMBO_NOTE_FIELD))
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                    components: note
                        .as_ref()
                        .and_then(|note| note.get(COMPONENTS_NOTE_FIELD))
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                        .take(crate::tex::COMPONENT_COUNT)
                        .map(|component| {
                            component
                                .get(COMBO_NOTE_FIELD)
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_owned)
                        })
                        .collect(),
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
                    .and_then(|note| note.get(MATERIAL_NOTE_FIELD))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                let count = if name.contains('[') {
                    uniform_array_size(name, combos, &array_macros)
                } else {
                    Some(1)
                };
                unresolved_uniform |= count.is_none();
                uniforms.push(Uniform {
                    name: name.to_string(),
                    kind,
                    default,
                    material,
                    offset: 0,
                    count: count.unwrap_or(1) as usize,
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
            let name = varying_name(decl);
            let own = if decl.contains('[') {
                0
            } else {
                vector_width(decl.split_whitespace().next().unwrap_or(""))
            };
            let slot = varyings.and_then(|map| map.get(&name).copied());
            let location = slot.map_or(varying_loc, |slot| slot.location);
            let width = slot.map_or(own, |slot| slot.width.max(own));
            if own == 0 {
                if let Some((width, count)) = array_varying(decl, combos) {
                    let array = PackedArray { name, width, count, conditions: conditions.clone() };
                    let _ = writeln!(
                        body,
                        "layout(location = {location}) {direction} vec4 skwd_io_{}[{}];",
                        array.name,
                        array.slots()
                    );
                    let _ = writeln!(body, "{} {}[{count}];", vector_type(width), array.name);
                    packed.push(array);
                } else {
                    let count = array_size(decl, combos);
                    let decl = match count {
                        Some(count) => replace_array_size(decl, count),
                        None => decl.to_string(),
                    };
                    let _ = writeln!(body, "layout(location = {location}) {direction} {decl};");
                }
            } else {
                let _ = writeln!(
                    body,
                    "layout(location = {location}) {direction} {} skwd_io_{name};",
                    vector_type(width)
                );
                let _ = writeln!(body, "{} {name};", vector_type(own));
                shadows.push(Shadow { name, own, width, conditions: conditions.clone() });
            }
            varying_loc = location + packed_locations(decl, combos);
            continue;
        }
        if line.contains("gl_FragColor") {
            uses_frag_color = true;
        }
        if (!shadows.is_empty() || !packed.is_empty() || varyings.is_some())
            && let Some(at) = code.find("void main")
            && code[at + 9..].trim_start().starts_with('(')
        {
            let _ = writeln!(body, "{}", line.replacen("main", "skwd_main", 1));
            continue;
        }
        if let Some(rest) = code.strip_prefix("attribute ") {
            let decl = rest.trim_end_matches(';');
            let _ = writeln!(body, "layout(location = {attribute_loc}) in {decl};");
            attribute_loc += 1;
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }

    if let Some(map) = varyings {
        let direction = if stage == Stage::Vertex { "out" } else { "in" };
        for (name, slot) in map {
            if slot.width == 0 || shadows.iter().any(|shadow| shadow.name == *name) {
                continue;
            }
            let ty = vector_type(slot.width);
            let _ = writeln!(
                body,
                "layout(location = {}) {direction} {ty} skwd_io_{name};",
                slot.location
            );
            let _ = writeln!(body, "{ty} {name};");
            shadows.push(Shadow {
                name: name.clone(),
                own: slot.width,
                width: slot.width,
                conditions: Vec::new(),
            });
        }
    }
    let mut offset = 0usize;
    for uniform in &mut uniforms {
        let align = uniform.std140_align();
        offset = offset.div_ceil(align) * align;
        uniform.offset = offset;
        offset += uniform.std140_span();
    }
    let block_size = offset.div_ceil(STD140_ARRAY_ALIGNMENT) * STD140_ARRAY_ALIGNMENT;

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
    if unresolved_uniform {
        out.push_str(UNRESOLVED_UNIFORM_ARRAY);
    }
    if !uniforms.is_empty() {
        out.push_str("layout(std140, set = 0, binding = 0) uniform SkwdParams {\n");
        for uniform in &uniforms {
            let _ = writeln!(out, "    {} {};", uniform.kind.glsl(), uniform_decl(uniform));
        }
        out.push_str("};\n");
    }
    for sampler in &samplers {
        out.push_str(&sampler_line(sampler));
        out.push('\n');
    }
    if stage == Stage::Fragment && uses_frag_color {
        out.push_str("layout(location = 0) out vec4 skwd_FragColor;\n");
        out.push_str("#define gl_FragColor skwd_FragColor\n");
    }
    out.push_str(&cast_conditions(&body));
    if body.contains("void skwd_main") {
        out.push_str("void main() {\n");
        if stage == Stage::Fragment {
            for shadow in &shadows {
                shadow.write(&mut out, &format!("{} = {};", shadow.name, shadow.narrowed()));
            }
            for array in &packed {
                array.copies(&mut out, stage);
            }
        }
        out.push_str("    skwd_main();\n");
        if stage == Stage::Vertex {
            for shadow in &shadows {
                shadow.write(&mut out, &format!("skwd_io_{} = {};", shadow.name, shadow.widened()));
            }
            for array in &packed {
                array.copies(&mut out, stage);
            }
        }
        out.push_str("}\n");
    }

    Translated { source: out, uniforms, samplers, block_size: block_size.max(16) }
}

fn block_text(uniforms: &[Uniform]) -> String {
    if uniforms.is_empty() {
        return String::new();
    }
    let mut out = String::from("layout(std140, set = 0, binding = 0) uniform SkwdParams {\n");
    for uniform in uniforms {
        let _ = writeln!(out, "    {} {};", uniform.kind.glsl(), uniform_decl(uniform));
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
    unify_uniform_stages(&mut [vertex, fragment]);
}

pub fn unify_uniform_stages(stages: &mut [&mut Translated]) {
    let mut union: Vec<Uniform> = Vec::new();
    for stage in stages.iter() {
        for uniform in &stage.uniforms {
            if !union.iter().any(|known| known.name == uniform.name) {
                union.push(uniform.clone());
            }
        }
    }
    let mut offset = 0usize;
    for uniform in &mut union {
        let align = uniform.std140_align();
        offset = offset.div_ceil(align) * align;
        uniform.offset = offset;
        offset += uniform.std140_span();
    }
    let size = offset.div_ceil(STD140_ARRAY_ALIGNMENT) * STD140_ARRAY_ALIGNMENT;
    let block = block_text(&union);
    for stage in stages.iter_mut() {
        stage.source = replace_block(&stage.source, &block);
        stage.uniforms.clone_from(&union);
        stage.block_size = size.max(16);
    }
}

const IDENTITY4: [f32; 16] =
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
const SPLAT_LANES: usize = 4;
const IDENTITY3_STD140: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

pub fn pack_uniforms(uniforms: &[Uniform], values: &BTreeMap<String, Vec<f32>>) -> Vec<u8> {
    pack_with(uniforms, |name| values.get(name).map(Vec::as_slice))
}

pub fn pack_with<'a>(
    uniforms: &'a [Uniform],
    lookup: impl Fn(&str) -> Option<&'a [f32]>,
) -> Vec<u8> {
    let size =
        uniforms.iter().map(|uniform| uniform.offset + uniform.std140_span()).max().unwrap_or(16);
    let mut out = vec![0u8; size.div_ceil(STD140_ARRAY_ALIGNMENT) * STD140_ARRAY_ALIGNMENT];
    for uniform in uniforms {
        let fallback: Option<&[f32]> = match uniform.kind {
            UniformKind::Mat4 => Some(&IDENTITY4),
            UniformKind::Mat3 => Some(&IDENTITY3_STD140),
            _ => None,
        };
        let Some(data) = lookup(&uniform.name).or(uniform.default.as_deref()).or(fallback) else {
            continue;
        };
        let lanes = uniform.kind.std140_size() / 4;
        let stride = if uniform.name.contains('[') {
            uniform.kind.std140_size().div_ceil(STD140_ARRAY_ALIGNMENT) * STD140_ARRAY_ALIGNMENT
        } else {
            0
        };
        let splat: [f32; SPLAT_LANES];
        let data = if data.len() == 1
            && lanes > 1
            && uniform.count == 1
            && !matches!(uniform.kind, UniformKind::Mat3 | UniformKind::Mat4 | UniformKind::Mat4x3)
        {
            splat = [data[0]; SPLAT_LANES];
            &splat[..lanes]
        } else {
            data
        };
        for (idx, value) in data.iter().take(lanes * uniform.count).enumerate() {
            let at = uniform.offset + (idx / lanes) * stride + (idx % lanes) * 4;
            if at + 4 <= out.len() {
                let word = if uniform.kind == UniformKind::Uint {
                    (*value as u32).to_le_bytes()
                } else {
                    value.to_le_bytes()
                };
                out[at..at + 4].copy_from_slice(&word);
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

fn cache_key(source: &str, tag: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in source.as_bytes() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}.{tag}.spv")
}

const MAX_CACHED_SPIRV_BYTES: u64 = 16 * 1024 * 1024;

#[must_use]
pub fn cached_spirv(source: &str, tag: &str) -> Option<Vec<u32>> {
    use std::io::Read;
    let path = spirv_cache_dir().join(cache_key(source, tag));
    let file = std::fs::File::open(&path).ok()?;
    if file.metadata().ok()?.len() > MAX_CACHED_SPIRV_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_CACHED_SPIRV_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.is_empty() || bytes.len() % 4 != 0 || bytes.len() as u64 > MAX_CACHED_SPIRV_BYTES {
        return None;
    }
    Some(
        bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect(),
    )
}

pub fn store_spirv(source: &str, tag: &str, words: &[u32]) {
    let dir = spirv_cache_dir();
    let path = dir.join(cache_key(source, tag));
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
}

pub struct CompilationSession {
    _compiler: shaderc::Compiler,
}

impl CompilationSession {
    pub fn new() -> Result<Self> {
        let compiler = shaderc::Compiler::new().map_err(|err| anyhow!("shaderc init: {err}"))?;
        Ok(Self { _compiler: compiler })
    }
}

pub fn compile(source: &str, stage: Stage, label: &str) -> Result<Vec<u32>> {
    const GEOMETRY_CACHE_TAG: &str = "geom";
    let tag = match stage {
        Stage::Vertex => "vert",
        Stage::Geometry => GEOMETRY_CACHE_TAG,
        Stage::Fragment => "frag",
    };
    if let Some(words) = cached_spirv(source, tag) {
        return Ok(words);
    }
    let compiler = shaderc::Compiler::new().map_err(|err| anyhow!("shaderc init: {err}"))?;
    let mut options =
        shaderc::CompileOptions::new().map_err(|err| anyhow!("shaderc options: {err}"))?;
    options.set_target_env(shaderc::TargetEnv::Vulkan, shaderc::EnvVersion::Vulkan1_2 as u32);
    options.set_optimization_level(shaderc::OptimizationLevel::Performance);
    let kind = match stage {
        Stage::Vertex => shaderc::ShaderKind::Vertex,
        Stage::Geometry => shaderc::ShaderKind::Geometry,
        Stage::Fragment => shaderc::ShaderKind::Fragment,
    };
    let artifact = compiler
        .compile_into_spirv(source, kind, label, "main", Some(&options))
        .map_err(|err| anyhow!("compile {label}: {err}"))?;
    let words = artifact.as_binary().to_vec();
    store_spirv(source, tag, &words);
    Ok(words)
}

#[cfg(test)]
mod tests;

const VERTEX_LAYOUT_PREFIX: &str = "layout(location = ";
const VERTEX_INPUT_DELIMITER: &str = ") in ";

pub struct VertexInput<'a> {
    pub location: u32,
    pub kind: &'a str,
    pub name: &'a str,
}

pub fn vertex_inputs(source: &str) -> Result<Vec<VertexInput<'_>>, String> {
    source
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix(VERTEX_LAYOUT_PREFIX)
                .and_then(|rest| rest.split_once(VERTEX_INPUT_DELIMITER))
        })
        .map(|(location, declaration)| {
            let location = location.parse().map_err(|_| "mesh vertex location invalid")?;
            let mut parts = declaration.trim_end_matches(';').split_whitespace();
            let kind = parts.next().ok_or("mesh vertex declaration incomplete")?;
            let name = parts.next().ok_or("mesh vertex name missing")?;
            Ok(VertexInput { location, kind, name })
        })
        .collect()
}
