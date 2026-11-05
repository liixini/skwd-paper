use paper_geom::Background;
use std::sync::OnceLock;

pub fn worker_background() -> Background {
    static VALUE: OnceLock<Background> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("SKWD_PAPER_BACKGROUND")
            .ok()
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default()
    })
}
