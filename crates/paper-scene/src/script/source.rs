use serde_json::Value;
use sha2::{Digest, Sha256};

pub(super) struct Binding {
    pub path: String,
    pub source: String,
    pub layer: Option<usize>,
    pub properties: Value,
}

pub(super) fn collect(
    value: &mut Value,
    path: &str,
    out: &mut Vec<Binding>,
    props: &crate::model::Properties,
) {
    match value {
        Value::Object(map)
            if map.get("script").and_then(Value::as_str).is_some_and(|s| !s.trim().is_empty()) =>
        {
            let initial = effective(&Value::Object(map.clone()), props);
            let source = map.remove("script").and_then(|v| v.as_str().map(str::to_owned)).unwrap();
            let layer =
                path.strip_prefix("/objects/").and_then(|s| s.split('/').next()?.parse().ok());
            let properties =
                map.remove("scriptproperties").unwrap_or_else(|| serde_json::json!({}));
            out.push(Binding { path: path.to_owned(), source, layer, properties });
            *value = initial;
        }
        Value::Object(map) => {
            for (key, child) in map {
                collect(
                    child,
                    &format!("{path}/{}", key.replace('~', "~0").replace('/', "~1")),
                    out,
                    props,
                );
            }
        }
        Value::Array(values) => {
            for (i, child) in values.iter_mut().enumerate() {
                collect(child, &format!("{path}/{i}"), out, props);
            }
        }
        _ => {}
    }
}

pub(super) fn present(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            map.get("script").and_then(Value::as_str).is_some_and(|s| !s.trim().is_empty())
                || map.values().any(present)
        }
        Value::Array(values) => values.iter().any(present),
        _ => false,
    }
}

pub(super) fn needs_runtime(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(map)
            if map.get("script").and_then(Value::as_str).is_some_and(|s| !s.trim().is_empty()) =>
        {
            let source = map["script"].as_str().unwrap();
            let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
            key != "text"
                || ![
                    "a586126a95a01a70b7de65487e04996a3618fe9842e20567588ae05872bfc249",
                    "6772b9454e51c96bf5555084cc04be0fa187a3114329286002ac216d46c0741b",
                    "ba025ba3c7843f0edd71998e4accda28a9c4026e7ae857f0c81efb8f3df3ceaf",
                ]
                .contains(&hash.as_str())
        }
        Value::Object(map) => map.iter().any(|(key, v)| needs_runtime(v, key)),
        Value::Array(values) => values.iter().any(|v| needs_runtime(v, key)),
        _ => false,
    }
}

pub(super) fn effective(value: &Value, props: &crate::model::Properties) -> Value {
    let base = value.get("value").unwrap_or(&Value::Null);
    if value.get("user").is_none() {
        return base.clone();
    }
    if let Some(numbers) = crate::effects::bound_value(value, props) {
        if numbers.len() == 1 {
            return if base.is_boolean() {
                Value::Bool(numbers[0] != 0.0)
            } else {
                Value::from(numbers[0])
            };
        }
        return serde_json::json!(numbers);
    }
    base.clone()
}

pub(super) fn resolve_wrappers(value: &mut Value, props: &crate::model::Properties) {
    match value {
        Value::Object(map) if map.contains_key("value") && map.contains_key("user") => {
            *value = effective(value, props);
        }
        Value::Object(map) => {
            for child in map.values_mut() {
                resolve_wrappers(child, props);
            }
        }
        Value::Array(values) => {
            for child in values {
                resolve_wrappers(child, props);
            }
        }
        _ => {}
    }
}
