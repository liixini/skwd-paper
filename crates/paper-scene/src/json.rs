use anyhow::{Context, Result};
use serde_json::Value;

pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;

pub fn parse(raw: &[u8]) -> Result<Value> {
    if raw.len() > MAX_JSON_BYTES {
        anyhow::bail!("json is {} bytes; limit is {MAX_JSON_BYTES}", raw.len());
    }
    let text = raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(raw);
    match serde_json::from_slice(text) {
        Ok(value) => Ok(value),
        Err(strict) => {
            let relaxed = relax(&String::from_utf8_lossy(text));
            serde_json::from_str(&relaxed)
                .with_context(|| format!("lenient parse failed (strict error: {strict})"))
        }
    }
}

fn relax(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut idx = 0;
    while idx < bytes.len() {
        let byte = bytes[idx];
        if in_string {
            out.push(byte as char);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            idx += 1;
            continue;
        }
        match byte {
            b'"' => {
                in_string = true;
                out.push('"');
                idx += 1;
            }
            b'/' if idx + 1 < bytes.len() && bytes[idx + 1] == b'/' => {
                while idx < bytes.len() && bytes[idx] != b'\n' {
                    idx += 1;
                }
            }
            b'/' if idx + 1 < bytes.len() && bytes[idx + 1] == b'*' => {
                idx += 2;
                while idx + 1 < bytes.len() && !(bytes[idx] == b'*' && bytes[idx + 1] == b'/') {
                    idx += 1;
                }
                idx = (idx + 2).min(bytes.len());
            }
            b',' => {
                let mut peek = idx + 1;
                while peek < bytes.len() && bytes[peek].is_ascii_whitespace() {
                    peek += 1;
                }
                if peek < bytes.len() && (bytes[peek] == b'}' || bytes[peek] == b']') {
                    idx += 1;
                } else {
                    out.push(',');
                    idx += 1;
                }
            }
            other => {
                out.push(other as char);
                idx += 1;
            }
        }
    }
    out
}
