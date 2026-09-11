pub mod audit;
pub mod capability;
pub mod dynamic_text;
pub mod effect_lifetime;
pub mod effects;
pub mod hlsl;
pub mod json;
pub mod model;
pub mod mouse;
pub mod particles;
pub mod pkg;
pub mod puppet;
mod read;
pub mod scene;
pub mod scene_targets;
pub mod shader;
pub mod sound;
pub mod tex;
pub mod text;

#[cfg(test)]
mod corpus_tests;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
