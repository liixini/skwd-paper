use super::{ID_FIELD, OBJECTS_FIELD, PARENT_FIELD, VISIBLE_FIELD, id_of};
use serde_json::Value;
use std::collections::BTreeSet;

const SCRIPT_FIELD: &str = "script";
const ANIMATION_FIELD: &str = "animation";
const VISIBLE_TOKEN: &str = "visible";
const PARENT_MEMBER: &str = ".parent";
const ASSIGN: char = '=';
const COMPARISON: &str = "==";

pub(super) struct HiddenLayers {
    writes: bool,
    dynamic: BTreeSet<usize>,
}

impl HiddenLayers {
    pub fn of(scene: &Value) -> Self {
        let objects =
            scene.get(OBJECTS_FIELD).and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        let changes = |object: &Value| {
            object.get(VISIBLE_FIELD).is_some_and(|visible| {
                visible.get(SCRIPT_FIELD).is_some() || visible.get(ANIMATION_FIELD).is_some()
            })
        };
        let parent = |object: &Value| {
            let id = object.get(PARENT_FIELD).and_then(id_of)?;
            objects.iter().position(|node| node.get(ID_FIELD).and_then(id_of).as_ref() == Some(&id))
        };
        let dynamic = (0..objects.len())
            .filter(|&index| {
                let mut seen = BTreeSet::new();
                let mut next = Some(index);
                while let Some(at) = next.filter(|at| seen.insert(*at)) {
                    if changes(&objects[at]) {
                        return true;
                    }
                    next = parent(&objects[at]);
                }
                false
            })
            .collect();
        Self { writes: writes_visibility(scene), dynamic }
    }

    pub fn deferrable(&self, index: usize) -> bool {
        !self.writes && !self.dynamic.contains(&index)
    }
}

fn writes_visibility(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, child)| {
            (key == SCRIPT_FIELD && child.as_str().is_some_and(assigns_visibility))
                || writes_visibility(child)
        }),
        Value::Array(values) => values.iter().any(writes_visibility),
        _ => false,
    }
}

fn assigns_visibility(source: &str) -> bool {
    let compared =
        |at: usize, token: &str| source[at + token.len()..].trim_start().starts_with(COMPARISON);
    let reparents = source.match_indices(PARENT_MEMBER).any(|(at, member)| {
        let mut rest = source[at + member.len()..].trim_start().chars();
        rest.next() == Some(ASSIGN) && rest.next() != Some(ASSIGN)
    });
    reparents || source.match_indices(VISIBLE_TOKEN).any(|(at, token)| !compared(at, token))
}

#[cfg(test)]
#[path = "hidden_tests.rs"]
mod tests;
