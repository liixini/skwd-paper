use super::source;
use crate::effects::property_dependencies as dependencies;
use crate::model::Properties;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

pub(super) struct PropertyBindings {
    defaults: Properties,
    declarations: Map<String, Value>,
    supported: BTreeSet<String>,
    values: Vec<(String, Value, BTreeSet<String>)>,
    scripts: Vec<(usize, Value, BTreeSet<String>)>,
}

impl PropertyBindings {
    pub fn new(scene: &Value, project: &Value, scripts: &[source::Binding]) -> Self {
        let mut values = Vec::new();
        collect(scene, "", &mut values);
        let mut supported = BTreeSet::new();
        let mut blocked = BTreeSet::new();
        for (path, _, names) in &values {
            let destination =
                if supported_path(scene, path) { &mut supported } else { &mut blocked };
            destination.extend(names.iter().cloned());
        }
        let scripts = scripts
            .iter()
            .enumerate()
            .filter_map(|(index, binding)| {
                let names = dependencies(&binding.properties);
                if binding.source.matches("createScriptProperties").count() != 1 {
                    blocked.extend(names.iter().cloned());
                }
                (!names.is_empty()).then(|| (index, binding.properties.clone(), names))
            })
            .collect();
        supported.retain(|name| !blocked.contains(name));
        values.retain(|(path, _, _)| !path.contains("/scriptproperties/"));
        Self {
            defaults: crate::effects::parse_properties(project),
            declarations: project
                .pointer("/general/properties")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            supported,
            values,
            scripts,
        }
    }

    pub fn restrict_package(&mut self, package: &crate::pkg::Package) {
        for entry in package.entries() {
            if entry.path == "scene.json" || !entry.path.ends_with(".json") {
                continue;
            }
            let Ok(value) = crate::json::parse(package.read(entry)) else {
                self.supported.clear();
                return;
            };
            for name in dependencies(&value) {
                self.supported.remove(&name);
            }
        }
    }

    pub fn restrict_assets(&mut self, dependencies: Option<&BTreeSet<String>>) {
        if let Some(dependencies) = dependencies {
            self.supported.retain(|name| !dependencies.contains(name));
        } else {
            self.supported.clear();
        }
    }

    pub fn restrict_hidden_layers(&mut self, scene: &Value, layers: &[String], callbacks: bool) {
        if layers.is_empty() {
            return;
        }
        if callbacks {
            self.supported.clear();
            return;
        }
        let Some(objects) = scene["objects"].as_array() else {
            self.supported.clear();
            return;
        };
        let mut ancestors = BTreeSet::new();
        for id in layers {
            let Some(mut index) = objects.iter().position(|node| {
                node.get("id").is_some_and(|value| value.to_string().trim_matches('"') == id)
            }) else {
                self.supported.clear();
                return;
            };
            for _ in 0..objects.len() {
                if !ancestors.insert(index) {
                    break;
                }
                let Some(parent) = objects[index].get("parent") else { break };
                let Some(next) = objects.iter().position(|node| node.get("id") == Some(parent))
                else {
                    break;
                };
                index = next;
            }
        }
        for index in ancestors {
            let path = format!("/objects/{index}/visible");
            for (_, _, names) in self.values.iter().filter(|(key, _, _)| *key == path) {
                self.supported.retain(|name| !names.contains(name));
            }
        }
    }

    pub fn desired(&self, overrides: &Properties) -> Properties {
        let mut desired = self.defaults.clone();
        desired.extend(overrides.clone());
        desired
    }

    pub fn user_properties(&self, props: &Properties) -> Value {
        let mut values = self.declarations.clone();
        for (key, numbers) in props {
            let name = values
                .keys()
                .find(|name| name.eq_ignore_ascii_case(key))
                .cloned()
                .unwrap_or_else(|| key.clone());
            let entry = values.entry(name).or_insert_with(|| json!({}));
            entry["value"] = if entry["type"] == "bool" {
                Value::Bool(numbers.first().is_some_and(|number| *number != 0.0))
            } else if numbers.len() == 1 {
                Value::from(numbers[0])
            } else {
                json!(numbers)
            };
        }
        Value::Object(values)
    }

    pub fn update(&self, current: &Properties, desired: &Properties) -> Option<Value> {
        let changed: BTreeSet<_> = current
            .keys()
            .chain(desired.keys())
            .filter(|name| current.get(*name) != desired.get(*name))
            .cloned()
            .collect();
        if !changed.is_subset(&self.supported) {
            return None;
        }
        let values: Vec<_> = self
            .values
            .iter()
            .filter(|(_, _, names)| !names.is_disjoint(&changed))
            .map(|(path, value, _)| (path, source::effective(value, desired)))
            .collect();
        let scripts: Vec<_> = self
            .scripts
            .iter()
            .filter(|(_, _, names)| !names.is_disjoint(&changed))
            .map(|(index, value, _)| {
                let mut resolved = value.clone();
                source::resolve_wrappers(&mut resolved, desired);
                (index, resolved)
            })
            .collect();
        let properties = self.user_properties(desired);
        let changed_values: Map<_, _> = properties
            .as_object()?
            .iter()
            .filter(|(name, _)| changed.contains(&name.to_ascii_lowercase()))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        Some(
            json!({"values": values, "scripts": scripts, "properties": properties, "changed": changed_values}),
        )
    }
}

fn collect(value: &Value, path: &str, out: &mut Vec<(String, Value, BTreeSet<String>)>) {
    match value {
        Value::Object(map) => {
            if map.contains_key("user") {
                out.push((path.to_owned(), value.clone(), dependencies(value)));
            }
            for (key, child) in map {
                if key == "value" && map.contains_key("user") {
                    continue;
                }
                let key = key.replace('~', "~0").replace('/', "~1");
                collect(child, &format!("{path}/{key}"), out);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect(child, &format!("{path}/{index}"), out);
            }
        }
        _ => {}
    }
}

fn supported_path(scene: &Value, path: &str) -> bool {
    let parts: Vec<_> = path.split('/').skip(1).collect();
    let ["objects", index, tail @ ..] = parts.as_slice() else { return false };
    let Some(object) = index.parse::<usize>().ok().and_then(|index| scene["objects"].get(index))
    else {
        return false;
    };
    if object.get("particle").is_some() || object.get("sound").is_some() {
        return false;
    }
    match tail {
        ["visible"] => !has_nonvisual_descendant(scene, object),
        ["alpha" | "color"]
        | ["text", "scriptproperties", ..]
        | ["effects", _, "passes", _, "constantshadervalues", _] => true,
        _ => false,
    }
}

fn has_nonvisual_descendant(scene: &Value, parent: &Value) -> bool {
    let Some(objects) = scene["objects"].as_array() else { return true };
    objects
        .iter()
        .filter(|object| object.get("particle").is_some() || object.get("sound").is_some())
        .any(|object| {
            let mut next = object.get("parent");
            for _ in 0..objects.len() {
                let Some(id) = next else { return false };
                if parent.get("id") == Some(id) {
                    return true;
                }
                next = objects
                    .iter()
                    .find(|candidate| candidate.get("id") == Some(id))
                    .and_then(|object| object.get("parent"));
            }
            true
        })
}

#[cfg(test)]
#[path = "properties_tests.rs"]
mod tests;
