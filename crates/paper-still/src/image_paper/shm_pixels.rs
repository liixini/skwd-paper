use wayland_client::protocol::wl_shm;

pub(super) fn choose_shm_format(formats: &[wl_shm::Format]) -> wl_shm::Format {
    if formats.contains(&wl_shm::Format::Abgr8888) {
        wl_shm::Format::Abgr8888
    } else {
        wl_shm::Format::Argb8888
    }
}

pub(super) fn pack_pixels(canvas: &mut [u8], rgba: &[u8], format: wl_shm::Format) {
    if format == wl_shm::Format::Abgr8888 {
        canvas.copy_from_slice(rgba);
        return;
    }
    for (destination, source) in canvas.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
        destination[0] = source[2];
        destination[1] = source[1];
        destination[2] = source[0];
        destination[3] = source[3];
    }
}
