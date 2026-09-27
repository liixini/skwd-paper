use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Background {
    #[serde(default)]
    pub color: [u8; 3],
    #[serde(default)]
    pub blur: bool,
}

impl Background {
    pub fn worker() -> Self {
        static VALUE: OnceLock<Background> = OnceLock::new();
        *VALUE.get_or_init(|| {
            std::env::var("SKWD_PAPER_BACKGROUND")
                .ok()
                .and_then(|value| serde_json::from_str(&value).ok())
                .unwrap_or_default()
        })
    }

    pub fn rgba(self) -> [u8; 4] {
        [self.color[0], self.color[1], self.color[2], 255]
    }
}
