use std::{cell::Cell, sync::OnceLock};

thread_local! {
    static EFFECTS: Cell<u32> = Cell::new({
        let value =
            |key| std::env::var(key).ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0).min(100);
        value("SKWD_PAPER_BLUR") | (value("SKWD_PAPER_DIM") << 16)
    });
}

pub(crate) fn set(surface: &paper_control::SurfacePolicy) {
    EFFECTS.set(surface.blur.min(100) | (surface.dim.min(100) << 16));
}

pub(crate) fn effects() -> (f32, f32) {
    let value = EFFECTS.get();
    ((value & 0xffff) as f32, (value >> 16) as f32 / 100.0)
}

pub(crate) fn needs_shader() -> bool {
    static DYNAMIC: OnceLock<bool> = OnceLock::new();
    let (blur, dim) = effects();
    *DYNAMIC.get_or_init(|| std::env::var("SKWD_PAPER_DYNAMIC_EFFECTS").as_deref() == Ok("1"))
        || blur > 0.0
        || dim > 0.0
}

#[cfg(test)]
mod tests;
