use super::*;

fn bitmap(
    bytes: &mut [u8],
    width: i32,
    pitch: i32,
    mode: ft::FT_Pixel_Mode,
    levels: i16,
) -> ft::FT_Bitmap {
    ft::FT_Bitmap {
        rows: 1,
        width,
        pitch,
        buffer: bytes.as_mut_ptr(),
        num_grays: levels,
        pixel_mode: mode as i8,
        palette_mode: 0,
        palette: ptr::null_mut(),
    }
}

#[test]
fn packed_alpha_modes() {
    let mut mono = [0b1010_1000, 0b1000_0000];
    let image = bitmap(&mut mono, 9, 2, ft::FT_PIXEL_MODE_MONO, 2);
    assert_eq!(
        unsafe { alpha_bitmap(&image, 9, 1) },
        Some(vec![255, 0, 255, 0, 255, 0, 0, 0, 255])
    );

    let mut gray2 = [0b0001_1011, 0b1100_0000];
    let image = bitmap(&mut gray2, 5, 2, ft::FT_PIXEL_MODE_GRAY2, 4);
    assert_eq!(unsafe { alpha_bitmap(&image, 5, 1) }, Some(vec![0, 85, 170, 255, 255]));

    let mut gray4 = [0x0f, 0x80];
    let image = bitmap(&mut gray4, 3, 2, ft::FT_PIXEL_MODE_GRAY4, 16);
    assert_eq!(unsafe { alpha_bitmap(&image, 3, 1) }, Some(vec![0, 255, 136]));
}

#[test]
fn gray_levels_scale() {
    let mut gray = [0, 1, 2, 3];
    let image = bitmap(&mut gray, 4, 4, ft::FT_PIXEL_MODE_GRAY, 4);
    assert_eq!(unsafe { alpha_bitmap(&image, 4, 1) }, Some(vec![0, 85, 170, 255]));
}

#[test]
fn full_gray_rows() {
    let mut bytes = [1, 2, 3, 99, 4, 5, 6, 99];
    let mut image = bitmap(&mut bytes, 3, 4, ft::FT_PIXEL_MODE_GRAY, 256);
    image.rows = 2;
    assert_eq!(unsafe { alpha_bitmap(&image, 3, 2) }, Some(vec![1, 2, 3, 4, 5, 6]));
}

#[test]
fn negative_pitch_rows() {
    let mut bytes = [4, 5, 6, 0, 1, 2, 3, 0];
    let mut image = bitmap(&mut bytes, 3, -4, ft::FT_PIXEL_MODE_GRAY, 256);
    image.rows = 2;
    image.buffer = unsafe { bytes.as_mut_ptr().add(4) };
    assert_eq!(unsafe { alpha_bitmap(&image, 3, 2) }, Some(vec![1, 2, 3, 4, 5, 6]));
}

#[test]
fn invalid_bitmap_rejected() {
    let mut bytes = [0, 1, 2];
    let image = bitmap(&mut bytes, 3, 2, ft::FT_PIXEL_MODE_GRAY, 256);
    assert_eq!(unsafe { alpha_bitmap(&image, 3, 1) }, None);

    let image = bitmap(&mut bytes, 3, 3, ft::FT_PIXEL_MODE_GRAY, 1);
    assert_eq!(unsafe { alpha_bitmap(&image, 3, 1) }, None);

    let image = bitmap(&mut bytes, 3, 3, ft::FT_PIXEL_MODE_BGRA, 256);
    assert_eq!(unsafe { alpha_bitmap(&image, 3, 1) }, None);
}
