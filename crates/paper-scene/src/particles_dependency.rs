use serde_json::Value;

pub(super) const DEPENDENCIES: &str = "dependencies";
const TYPE: &str = "type";
const ID: &str = "id";
const INDEX: &str = "index";
const MASK: &str = "mask";
const IMAGE: &str = "emitterimage";

pub(super) enum Kind {
    Image,
    Model,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Image => IMAGE,
            Self::Model => super::model_collision::NAME,
        }
    }
}

pub(super) struct Dependency<'a> {
    pub source: u32,
    pub ordinal: usize,
    pub mask: &'a str,
}

pub(super) fn parse(object: &Value, kind: Kind) -> impl Iterator<Item = Dependency<'_>> {
    let name = kind.name();
    object
        .get(DEPENDENCIES)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(move |entry| entry.get(TYPE).and_then(Value::as_str) == Some(name))
        .filter_map(|entry| {
            Some(Dependency {
                source: u32::try_from(entry.get(ID)?.as_u64()?).ok()?,
                ordinal: usize::try_from(entry.get(INDEX)?.as_u64()?).ok()?,
                mask: entry.get(MASK).and_then(Value::as_str).unwrap_or_default(),
            })
        })
}
