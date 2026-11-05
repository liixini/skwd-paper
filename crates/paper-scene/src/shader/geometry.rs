use super::{Stage, Translated, VaryingSlot, translate};
use anyhow::{Result, anyhow};
use std::collections::BTreeMap;
use std::fmt::Write;

const INPUT: &str = "in ";
const OUTPUT: &str = "out ";
const POSITION: &str = "gl_Position";
const MAX_VERTICES: &str = "[maxvertexcount(";
const MAX_VERTICES_END: &str = ")]";
const EMIT: &str = "OUT.Append(";
const VS_OUTPUT_BEGIN: &str = "struct VS_OUTPUT {\n    vec4 position;\n";
const PS_INPUT_BEGIN: &str = "};\nstruct PS_INPUT {\n    vec4 position;\n";
const EMITTER_BEGIN: &str =
    "};\nVS_OUTPUT IN[1];\nvoid skwd_emit(PS_INPUT value) {\n    gl_Position = value.position;\n";
const EMITTER_END: &str = "    EmitVertex();\n}\n";
const GEOMETRY_MAIN: &str = "void main() {";
const INPUT_BEGIN: &str = "void main() {\n    IN[0].position = gl_in[0].gl_Position;\n";
const SOURCE_POSITION_MEMBER: &str = ".gl_Position";
const OUTPUT_POSITION_MEMBER: &str = ".position";
const EMIT_REPLACEMENT: &str = "skwd_emit(";
const LOCATION_BEGIN: &str = "layout(location = ";
const INPUT_LOCATION: &str = ") in ";
const OUTPUT_LOCATION: &str = ") out ";
const INPUT_VARIABLE: &str = " skwd_in_";
const OUTPUT_VARIABLE: &str = " skwd_out_";
const INPUT_ARRAY: &str = "[];";
const OUTPUT_TAIL: &str = ";";
const SPRITESHEET: &str = "SPRITESHEET";
const VECTOR3: &str = "vec3 ";
const MATRIX_PRODUCT: &str = "mul(";
const LOOP: &str = "for";
const BLOCK_COMMENT: &str = "/*";
const BLOCK_END: &str = "*/";
const TRAIL_POSITION: &str = "ComputeParticleTrailTangents(IN[0].position,";
const TRAIL_POSITION_XYZ: &str = "ComputeParticleTrailTangents(IN[0].position.xyz,";
const UV_SWIZZLES: [(&str, &str); 3] = [
    ("uvOffsets.zyzy", "uvOffsets.zy"),
    ("uvOffsets.xzxz", "uvOffsets.xz"),
    ("uvOffsets.xyxy", "uvOffsets.xy"),
];

#[derive(Clone)]
struct Field {
    ty: String,
    name: String,
}

fn field(line: &str, direction: &str) -> Option<Field> {
    let mut words = line.trim().strip_prefix(direction)?.trim_end_matches(';').split_whitespace();
    let ty = words.next()?.to_string();
    let name = words.next()?.to_string();
    (words.next().is_none()).then_some(Field { ty, name })
}

fn fields(source: &str, direction: &str) -> Vec<Field> {
    source.lines().filter_map(|line| field(line, direction)).collect()
}

fn explicit_products(source: &str, output: &[Field]) -> String {
    let mut result = String::with_capacity(source.len());
    for line in source.lines() {
        let replacement = line.split_once('=').and_then(|(left, right)| {
            let expression = right.trim().strip_suffix(';')?;
            if !expression.starts_with(MATRIX_PRODUCT) {
                return None;
            }
            let member = left.trim().rsplit_once('.').map(|(_, name)| name);
            let vector = left.trim_start().starts_with(VECTOR3)
                || output
                    .iter()
                    .any(|field| field.ty == VECTOR3.trim() && member == Some(field.name.as_str()));
            vector.then(|| format!("{left}= vec3({expression});"))
        });
        result.push_str(replacement.as_deref().unwrap_or(line));
        result.push('\n');
    }
    result
}

fn loop_scopes(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let mut scopes = Vec::new();
    let mut loop_header = false;
    let mut comment = false;
    for line in source.lines() {
        let code = line.split(super::LINE_COMMENT).next().unwrap_or_default();
        let trimmed = code.trim();
        if !comment
            && trimmed.strip_prefix(LOOP).is_some_and(|rest| {
                rest.trim_start().starts_with('(')
                    && (trimmed.ends_with(')') || trimmed.ends_with('{'))
            })
        {
            loop_header = true;
        } else if !trimmed.is_empty() && !trimmed.starts_with('{') {
            loop_header = false;
        }
        for (offset, character) in line.char_indices() {
            if offset < code.len() {
                if line[offset..].starts_with(BLOCK_COMMENT) {
                    comment = true;
                }
                let in_comment = comment;
                if line[offset..].starts_with(BLOCK_END) {
                    comment = false;
                }
                if in_comment {
                    result.push(character);
                    continue;
                }
                match character {
                    '{' => {
                        scopes.push(loop_header);
                        if std::mem::take(&mut loop_header) {
                            result.push('{');
                        }
                    }
                    '}' => {
                        if scopes.pop() == Some(true) {
                            result.push('}');
                        }
                    }
                    _ => {}
                }
            }
            result.push(character);
        }
        result.push('\n');
    }
    result
}

fn interface(header: &mut String, slot: u32, field: &Field, input: bool) {
    let (direction, variable, tail) = if input {
        (INPUT_LOCATION, INPUT_VARIABLE, INPUT_ARRAY)
    } else {
        (OUTPUT_LOCATION, OUTPUT_VARIABLE, OUTPUT_TAIL)
    };
    header.push_str(LOCATION_BEGIN);
    header.push_str(&slot.to_string());
    header.push_str(direction);
    header.push_str(&field.ty);
    header.push_str(variable);
    header.push_str(&field.name);
    header.push_str(tail);
    header.push('\n');
}

#[must_use]
pub fn interfaces(source: &str) -> (String, String) {
    let varying = |direction| {
        let mut out = String::new();
        for field in fields(source, direction).into_iter().filter(|field| field.name != POSITION) {
            let _ = writeln!(out, "varying {} {};", field.ty, field.name);
        }
        out
    };
    (varying(INPUT), varying(OUTPUT))
}

pub fn translate_geometry(
    source: &str,
    combos: &BTreeMap<String, i64>,
    inputs: &BTreeMap<String, VaryingSlot>,
    outputs: &BTreeMap<String, VaryingSlot>,
) -> Result<Translated> {
    let input = fields(source, INPUT);
    let output = fields(source, OUTPUT);
    let mut body = String::new();
    let mut vertices = None;
    for line in source.lines() {
        let trimmed = line.trim();
        if field(line, INPUT).is_some() || field(line, OUTPUT).is_some() {
            continue;
        }
        if let Some(value) =
            trimmed.strip_prefix(MAX_VERTICES).and_then(|text| text.strip_suffix(MAX_VERTICES_END))
        {
            vertices = Some(value.to_string());
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    let vertices = vertices.ok_or_else(|| anyhow!("geometry vertex count missing"))?;
    let mut header =
        format!("layout(points) in;\nlayout(triangle_strip, max_vertices = {vertices}) out;\n");
    for field in &input {
        if field.name == POSITION {
            continue;
        }
        let slot = inputs
            .get(&field.name)
            .ok_or_else(|| anyhow!("geometry input {} missing", field.name))?;
        interface(&mut header, slot.location, field, true);
    }
    for field in &output {
        if field.name == POSITION {
            continue;
        }
        let slot = outputs
            .get(&field.name)
            .ok_or_else(|| anyhow!("geometry output {} missing", field.name))?;
        interface(&mut header, slot.location, field, false);
    }
    header.push_str(VS_OUTPUT_BEGIN);
    for field in input.iter().filter(|field| field.name != POSITION) {
        let _ = writeln!(header, "    {} {};", field.ty, field.name);
    }
    header.push_str(PS_INPUT_BEGIN);
    for field in output.iter().filter(|field| field.name != POSITION) {
        let _ = writeln!(header, "    {} {};", field.ty, field.name);
    }
    header.push_str(EMITTER_BEGIN);
    for field in output.iter().filter(|field| field.name != POSITION) {
        let _ = writeln!(header, "    skwd_out_{} = value.{};", field.name, field.name);
    }
    header.push_str(EMITTER_END);
    let mut init = String::from(INPUT_BEGIN);
    for field in input.iter().filter(|field| field.name != POSITION) {
        let _ = writeln!(init, "    IN[0].{} = skwd_in_{}[0];", field.name, field.name);
    }
    let mut body = body
        .replace(SOURCE_POSITION_MEMBER, OUTPUT_POSITION_MEMBER)
        .replace(EMIT, EMIT_REPLACEMENT)
        .replace(TRAIL_POSITION, TRAIL_POSITION_XYZ);
    if combos.get(SPRITESHEET).copied().unwrap_or(0) == 0 {
        for (wide, narrow) in UV_SWIZZLES {
            body = body.replace(wide, narrow);
        }
    }
    let body = loop_scopes(&explicit_products(&body, &output)).replacen(GEOMETRY_MAIN, &init, 1);
    if !body.contains(&init) {
        return Err(anyhow!("geometry entry missing"));
    }
    Ok(translate(&(header + &body), Stage::Geometry, combos))
}
