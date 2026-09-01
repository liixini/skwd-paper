pub(super) fn ease_out_cubic(progress: f32) -> f32 {
    1.0 - (1.0 - progress.clamp(0.0, 1.0)).powi(3)
}

pub(super) fn compose_tall(canvas: &mut [u8], old: &[u8], new: &[u8], direction_up: bool) {
    let half = old.len();
    if direction_up {
        canvas[..half].copy_from_slice(old);
        canvas[half..].copy_from_slice(new);
    } else {
        canvas[..half].copy_from_slice(new);
        canvas[half..].copy_from_slice(old);
    }
}

pub(super) fn slide_source_y(direction_up: bool, height: u32, eased_progress: f32) -> f64 {
    let span = f64::from(height);
    if direction_up {
        span * f64::from(eased_progress)
    } else {
        span * f64::from(1.0 - eased_progress)
    }
}
