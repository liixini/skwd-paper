use super::Assets;
use crate::model::Properties;
use anyhow::{Result, anyhow, ensure};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Component, Path};

const KIND: &str = "customshader";
const TYPE: &str = "type";
const OPTIONS: &str = "options";
const SHADER: &str = "shader";
const USER: &str = "user";
const NAME: &str = "name";
const MAX_CHOICES: usize = 256;
const MAX_SHADER_NAME_BYTES: usize = 256;

#[derive(Deserialize)]
struct Choice {
    value: f32,
    shader: String,
}

fn selected(project: &Value, properties: &Properties) -> Result<BTreeMap<String, String>> {
    let mut selected = BTreeMap::new();
    let Some(declarations) = project
        .get(super::GENERAL_FIELD)
        .and_then(|general| general.get(super::PROPERTIES_FIELD))
        .and_then(Value::as_object)
    else {
        return Ok(selected);
    };
    for (name, entry) in declarations {
        if entry.get(TYPE).and_then(Value::as_str) != Some(KIND) {
            continue;
        }
        let options = entry
            .get(OPTIONS)
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("customshader {name} options missing"))?;
        ensure!(options.len() <= MAX_CHOICES, "customshader {name} has too many options");
        let choices: Vec<Choice> = serde_json::from_value(Value::Array(options.clone()))?;
        ensure!(!choices.is_empty(), "customshader {name} options empty");
        for (index, choice) in choices.iter().enumerate() {
            ensure!(
                choice.value.is_finite()
                    && !choices[..index].iter().any(|other| other.value == choice.value),
                "customshader {name} option values must be unique and finite"
            );
            ensure!(
                !choice.shader.is_empty()
                    && choice.shader.len() <= MAX_SHADER_NAME_BYTES
                    && !choice.shader.contains('\\')
                    && Path::new(&choice.shader)
                        .components()
                        .all(|part| matches!(part, Component::Normal(_))),
                "customshader {name} shader path must be relative"
            );
        }
        let name = name.to_ascii_lowercase();
        let value = properties
            .get(&name)
            .filter(|values| values.len() == 1)
            .and_then(|values| values.first())
            .ok_or_else(|| anyhow!("customshader {name} selection missing"))?;
        let choice = choices
            .into_iter()
            .find(|choice| choice.value == *value)
            .ok_or_else(|| anyhow!("customshader {name} selection not declared"))?;
        ensure!(selected.insert(name, choice.shader).is_none(), "duplicate customshader property");
    }
    Ok(selected)
}

#[derive(PartialEq, Eq)]
pub enum Change {
    Unchanged,
    Uniforms,
    Reload,
}

pub fn change(project: &Value, before: &Properties, after: &Properties) -> Change {
    let defaults = super::parse_properties(project);
    let effective = |overrides: &Properties| {
        let mut properties = defaults.clone();
        properties.extend(overrides.clone());
        properties
    };
    let before = effective(before);
    let after = effective(after);
    match (selected(project, &before), selected(project, &after)) {
        (Ok(old), Ok(new)) if old == new => {
            if !old.is_empty() && before == after {
                Change::Unchanged
            } else {
                Change::Uniforms
            }
        }
        _ => Change::Reload,
    }
}

pub(crate) fn resolve<'a>(pass: &'a Value, assets: &'a Assets) -> Option<&'a str> {
    let shader = pass.get(SHADER)?;
    if let Some(name) = shader.as_str() {
        return Some(name);
    }
    let user = shader.get(USER)?;
    let name = user.as_str().or_else(|| user.get(NAME)?.as_str())?;
    assets.custom_shaders.get(&name.to_ascii_lowercase()).map(String::as_str)
}

impl Assets {
    pub fn with_custom_shaders(mut self, project: &Value) -> Result<Self> {
        self.custom_shaders = selected(project, &self.properties)?;
        Ok(self)
    }

    pub(crate) fn has_custom_shaders(&self) -> bool {
        !self.custom_shaders.is_empty()
    }
}

#[cfg(test)]
#[path = "customshader_tests.rs"]
mod tests;
