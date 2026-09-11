use serde_json::Value;

pub(super) fn literal(source: &str) -> Option<Value> {
    let source = source.trim();
    if let Some(quote @ ('\'' | '"')) = source.chars().next() {
        let rest = &source[1..];
        return Some(Value::String(rest[..rest.find(quote)?].to_owned()));
    }
    match source.split(|ch: char| !ch.is_ascii_alphanumeric()).next()? {
        "true" => Some(Value::Bool(true)),
        "false" => Some(Value::Bool(false)),
        _ => None,
    }
}

pub(super) fn setting(script: &str, properties: Option<&Value>, name: &str) -> Option<Value> {
    if let Some(value) = properties.and_then(|props| props.get(name)) {
        return Some(value.clone());
    }
    for line in script.lines().map(str::trim) {
        let Some(declaration) =
            ["let ", "var ", "const "].iter().find_map(|prefix| line.strip_prefix(prefix))
        else {
            continue;
        };
        if let Some((key, value)) = declaration.split_once('=')
            && key.trim() == name
        {
            return literal(value);
        }
    }
    for block in script.split(".add").skip(1) {
        let Some((_, fields)) = block.split_once('{') else { continue };
        let fields = fields.split('}').next()?;
        let Some((_, field_name)) = fields.split_once("name:") else { continue };
        if literal(field_name)?.as_str() == Some(name) {
            return literal(fields.split_once("value:")?.1);
        }
    }
    None
}
