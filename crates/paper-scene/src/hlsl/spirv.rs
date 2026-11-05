use anyhow::{Result, anyhow};
use std::collections::{BTreeMap, BTreeSet};

const MAGIC: u32 = 0x0723_0203;
const HEADER_WORDS: usize = 5;
const BOUND_WORD: usize = 3;
const VERSION_WORD: usize = 1;
const SCHEMA_WORD: usize = 4;
const MIN_VERSION: u32 = 0x0001_0000;
const MAX_VERSION: u32 = 0x0001_0600;
const VERSION_MASK: u32 = 0xff;
const WORD_COUNT_SHIFT: u32 = 16;
const OPCODE_MASK: u32 = 0xffff;
const OP_NAME: u32 = 5;
const OP_IMPORT: u32 = 11;
const OP_EXT_INST: u32 = 12;
const OP_TYPE_FLOAT: u32 = 22;
const OP_TYPE_VECTOR: u32 = 23;
const OP_DECORATE: u32 = 71;
const OP_GROUP_DECORATE: u32 = 74;
const OP_GROUP_MEMBER_DECORATE: u32 = 75;
const OP_NEGATE: u32 = 127;
const OP_SUBTRACT: u32 = 131;
const OP_MULTIPLY: u32 = 133;
const NO_CONTRACTION: u32 = 42;
const FLOAT_WIDTH: u32 = 32;
const VECTOR_LANES: std::ops::RangeInclusive<u32> = 2..=4;
const FLOOR: u32 = 8;
const FMA: u32 = 50;
const GLSL_IMPORT: &[u8] = b"GLSL.std.450\0";
const GLOBALS_NAME: &[u8] = b"$Globals\0";
const NEGATE_WORDS: u32 = 4;
const FMA_WORDS: u32 = 8;

fn instructions(words: &[u32]) -> Result<Vec<&[u32]>> {
    if words.len() < HEADER_WORDS
        || words[0] != MAGIC
        || words[BOUND_WORD] == 0
        || words[SCHEMA_WORD] != 0
        || !(MIN_VERSION..=MAX_VERSION).contains(&words[VERSION_WORD])
        || words[VERSION_WORD] & VERSION_MASK != 0
    {
        return Err(anyhow!("invalid spirv header"));
    }
    let mut tail = &words[HEADER_WORDS..];
    let mut decoded = Vec::new();
    while let Some(header) = tail.first() {
        let count = (header >> WORD_COUNT_SHIFT) as usize;
        if count == 0 || count > tail.len() {
            return Err(anyhow!("invalid spirv instruction"));
        }
        let (instruction, rest) = tail.split_at(count);
        decoded.push(instruction);
        tail = rest;
    }
    Ok(decoded)
}

fn string_matches(words: &[u32], expected: &[u8]) -> bool {
    words
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .take(expected.len())
        .eq(expected.iter().copied())
}

pub(super) fn names_globals(words: &[u32]) -> Result<bool> {
    Ok(instructions(words)?.iter().any(|instruction| {
        matches!(instruction, [header, _, name @ ..] if header & OPCODE_MASK == OP_NAME && string_matches(name, GLOBALS_NAME))
    }))
}

pub(super) fn normalize(words: Vec<u32>) -> Result<Vec<u32>> {
    let decoded = instructions(&words)?;
    let mut floats = BTreeSet::new();
    let mut imports = BTreeSet::new();
    let mut precise = BTreeSet::new();
    let mut products = BTreeMap::new();
    let mut floors = BTreeMap::new();
    for instruction in &decoded {
        match *instruction {
            [header, _, ..]
                if matches!(header & OPCODE_MASK, OP_GROUP_DECORATE | OP_GROUP_MEMBER_DECORATE) =>
            {
                return Ok(words);
            }
            [header, id, name @ ..]
                if header & OPCODE_MASK == OP_IMPORT && string_matches(name, GLSL_IMPORT) =>
            {
                imports.insert(*id);
            }
            [header, id, FLOAT_WIDTH] if header & OPCODE_MASK == OP_TYPE_FLOAT => {
                floats.insert(*id);
            }
            [header, id, scalar, lanes]
                if header & OPCODE_MASK == OP_TYPE_VECTOR
                    && floats.contains(scalar)
                    && VECTOR_LANES.contains(lanes) =>
            {
                floats.insert(*id);
            }
            [header, id, NO_CONTRACTION] if header & OPCODE_MASK == OP_DECORATE => {
                precise.insert(*id);
            }
            [header, ty, id, left, right] if header & OPCODE_MASK == OP_MULTIPLY => {
                products.insert(*id, (*ty, *left, *right));
            }
            [header, ty, id, import, FLOOR, product]
                if header & OPCODE_MASK == OP_EXT_INST && imports.contains(import) =>
            {
                floors.insert(*id, (*ty, *import, *product));
            }
            _ => {}
        }
    }
    let mut out = words[..HEADER_WORDS].to_vec();
    let mut bound = words[BOUND_WORD];
    for instruction in decoded {
        if let [header, ty, id, product, floor] = instruction
            && header & OPCODE_MASK == OP_SUBTRACT
            && floats.contains(ty)
            && !precise.contains(id)
            && !precise.contains(product)
            && !precise.contains(floor)
            && let Some(&(product_ty, left, right)) = products.get(product)
            && let Some(&(floor_ty, import, floor_product)) = floors.get(floor)
            && product_ty == *ty
            && floor_ty == *ty
            && floor_product == *product
        {
            if [*ty, *id, *product, *floor, import, left, right]
                .iter()
                .any(|id| *id == 0 || *id >= words[BOUND_WORD])
            {
                return Err(anyhow!("invalid spirv id bound"));
            }
            let next = bound.checked_add(1).ok_or_else(|| anyhow!("spirv bound overflow"))?;
            out.extend_from_slice(&[
                NEGATE_WORDS << WORD_COUNT_SHIFT | OP_NEGATE,
                *ty,
                bound,
                *floor,
            ]);
            out.extend_from_slice(&[
                FMA_WORDS << WORD_COUNT_SHIFT | OP_EXT_INST,
                *ty,
                *id,
                import,
                FMA,
                left,
                right,
                bound,
            ]);
            bound = next;
        } else {
            out.extend_from_slice(instruction);
        }
    }
    out[BOUND_WORD] = bound;
    Ok(out)
}

#[cfg(test)]
mod tests;
