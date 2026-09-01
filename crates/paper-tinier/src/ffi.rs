use std::ffi::c_int;

#[allow(non_snake_case)]
unsafe extern "C" {
    pub fn I420ToARGB(
        src_y: *const u8,
        src_stride_y: c_int,
        src_u: *const u8,
        src_stride_u: c_int,
        src_v: *const u8,
        src_stride_v: c_int,
        dst_argb: *mut u8,
        dst_stride_argb: c_int,
        width: c_int,
        height: c_int,
    ) -> c_int;

    pub fn H420ToARGB(
        src_y: *const u8,
        src_stride_y: c_int,
        src_u: *const u8,
        src_stride_u: c_int,
        src_v: *const u8,
        src_stride_v: c_int,
        dst_argb: *mut u8,
        dst_stride_argb: c_int,
        width: c_int,
        height: c_int,
    ) -> c_int;
}
