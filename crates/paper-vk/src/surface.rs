use std::sync::{
    OnceLock,
    atomic::{AtomicU32, Ordering},
};

fn state() -> &'static AtomicU32 {
    static EFFECTS: OnceLock<AtomicU32> = OnceLock::new();
    EFFECTS.get_or_init(|| {
        let value =
            |key| std::env::var(key).ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0).min(100);
        AtomicU32::new(value("SKWD_PAPER_BLUR") | (value("SKWD_PAPER_DIM") << 16))
    })
}

pub(crate) fn set(surface: &paper_control::SurfacePolicy) {
    state().store(surface.blur.min(100) | (surface.dim.min(100) << 16), Ordering::Relaxed);
}

pub(crate) fn effects() -> (f32, f32) {
    let value = state().load(Ordering::Relaxed);
    ((value & 0xffff) as f32, (value >> 16) as f32 / 100.0)
}

pub(crate) fn needs_shader() -> bool {
    static DYNAMIC: OnceLock<bool> = OnceLock::new();
    let (blur, dim) = effects();
    *DYNAMIC.get_or_init(|| std::env::var("SKWD_PAPER_DYNAMIC_EFFECTS").as_deref() == Ok("1"))
        || blur > 0.0
        || dim > 0.0
}
