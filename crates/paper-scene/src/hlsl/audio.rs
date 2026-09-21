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

pub(super) fn flatten(source: &str, uniforms: &[Uniform]) -> String {
    let mut source = source.to_owned();
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
