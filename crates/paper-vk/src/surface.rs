use std::sync::OnceLock;

pub(crate) fn effects() -> (f32, f32) {
    static EFFECTS: OnceLock<(f32, f32)> = OnceLock::new();
    *EFFECTS.get_or_init(|| {
        let value = |key| {
            std::env::var(key).ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0).min(100) as f32
        };
        (value("SKWD_PAPER_BLUR"), value("SKWD_PAPER_DIM") / 100.0)
    })
}

pub(crate) fn needs_shader() -> bool {
    let (blur, dim) = effects();
    blur > 0.0 || dim > 0.0
}
