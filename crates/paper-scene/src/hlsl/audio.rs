use crate::shader::{Uniform, UniformKind};
use std::fmt::Write;

fn subscript(text: &str) -> Option<(&str, &str)> {
    let tail = text.trim_start().strip_prefix('[')?;
    let mut depth = 1usize;
    for (at, ch) in tail.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some((tail[..at].trim(), &tail[at + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

fn sized_aliases(source: &str, uniforms: &[Uniform]) -> String {
    const FLOAT_DECL: &str = "float ";
    const INFERRED_ARRAY: &str = "[]";
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        let declaration = line
            .trim_start()
            .strip_prefix(FLOAT_DECL)
            .and_then(|line| line.split_once('='))
            .and_then(|(name, source)| {
                let name = name.trim().strip_suffix(INFERRED_ARRAY)?.trim();
                let source = source.trim().trim_end_matches(';').trim();
                let uniform = uniforms.iter().find(|uniform| {
                    uniform.name.split('[').next() == Some(source) && uniform.count > 1
                })?;
                Some((name, source, uniform.count))
            });
        if let Some((name, source, count)) = declaration {
            let _ = writeln!(out, "{FLOAT_DECL}{name}[{count}] = {source};");
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

pub(super) fn flatten(source: &str, uniforms: &[Uniform]) -> String {
    let mut source = sized_aliases(source, uniforms);
    for uniform in uniforms {
        let name = uniform.name.split('[').next().unwrap_or(&uniform.name);
        if uniform.kind != UniformKind::Float
            || ![16, 32, 64].into_iter().any(|count| {
                uniform.count == count
                    && ["Left", "Right"]
                        .into_iter()
                        .any(|side| name == format!("g_AudioSpectrum{count}{side}"))
            })
        {
            continue;
        }
        let mut out = String::with_capacity(source.len());
        let mut rest = source.as_str();
        while let Some(at) = rest.find(name) {
            let end = at + name.len();
            let boundary = rest[..at]
                .bytes()
                .next_back()
                .is_none_or(|byte| !super::word_char(byte) && byte != b'.');
            let indices = subscript(&rest[end..])
                .and_then(|(row, tail)| subscript(tail).map(|(column, tail)| (row, column, tail)));
            if let Some((row, column, tail)) = indices.filter(|_| boundary) {
                out.push_str(&rest[..at]);
                let _ = write!(out, "{name}[int({row}) * 4 + int({column})]");
                rest = tail;
            } else {
                out.push_str(&rest[..end]);
                rest = &rest[end..];
            }
        }
        out.push_str(rest);
        source = out;
    }
    source
}
