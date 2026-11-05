use crate::matrix::Mat4;

pub(super) fn inverse(matrix: &Mat4) -> Option<Mat4> {
    let [xx, xy, xz, xw, yx, yy, yz, yw, zx, zy, zz, zw, wx, wy, wz, ww] = *matrix;
    let [
        minor00,
        minor01,
        minor02,
        minor03,
        minor04,
        minor05,
        minor06,
        minor07,
        minor08,
        minor09,
        minor10,
        minor11,
        minor12,
        minor13,
        minor14,
        minor15,
        minor16,
        minor17,
    ] = [
        (zz * ww) - (wz * zw),
        (ww * yz) - (wz * yw),
        (zw * yz) - (zz * yw),
        (ww * zy) - (zw * wy),
        (ww * yy) - (yw * wy),
        (zw * yy) - (yw * zy),
        (wz * zy) - (zz * wy),
        (wz * yy) - (yz * wy),
        (zz * yy) - (yz * zy),
        (ww * zx) - (zw * wx),
        (ww * yx) - (yw * wx),
        (zw * yx) - (yw * zx),
        (wz * zx) - (zz * wx),
        (wz * yx) - (yz * wx),
        (zz * yx) - (yz * zx),
        (wy * zx) - (zy * wx),
        (wy * yx) - (yy * wx),
        (zy * yx) - (yy * zx),
    ];
    let [cofactor0, cofactor1, cofactor2, cofactor3] = [
        ((yy * minor00) - (yz * minor03)) + (yw * minor06),
        ((yx * minor00) - (yz * minor09)) + (yw * minor12),
        ((yx * minor03) - (yy * minor09)) + (yw * minor15),
        ((yx * minor06) - (yy * minor12)) + (yz * minor15),
    ];
    let determinant =
        ((-cofactor3 * xw) + (cofactor2 * xz)) + ((-cofactor1 * xy) + (cofactor0 * xx));
    if determinant == 0.0 || !determinant.is_finite() {
        return None;
    }
    let reciprocal = 1.0 / determinant;
    let out = [
        reciprocal * cofactor0,
        reciprocal * -(((xy * minor00) - (xz * minor03)) + (xw * minor06)),
        reciprocal * (((xy * minor01) - (xz * minor04)) + (xw * minor07)),
        reciprocal * -(((xy * minor02) - (xz * minor05)) + (xw * minor08)),
        reciprocal * -cofactor1,
        reciprocal * (((xx * minor00) - (xz * minor09)) + (xw * minor12)),
        reciprocal * -(((xx * minor01) - (xz * minor10)) + (xw * minor13)),
        reciprocal * (((xx * minor02) - (xz * minor11)) + (xw * minor14)),
        reciprocal * cofactor2,
        reciprocal * -(((xx * minor03) - (xy * minor09)) + (xw * minor15)),
        reciprocal * (((xx * minor04) - (xy * minor10)) + (xw * minor16)),
        reciprocal * -(((xx * minor05) - (xy * minor11)) + (minor17 * xw)),
        reciprocal * -cofactor3,
        reciprocal * (((xx * minor06) - (xy * minor12)) + (xz * minor15)),
        reciprocal * -(((xx * minor07) - (xy * minor13)) + (xz * minor16)),
        reciprocal * (((xx * minor08) - (xy * minor14)) + (minor17 * xz)),
    ];
    out.iter().all(|value| value.is_finite()).then_some(out)
}
