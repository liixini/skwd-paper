const TWO_OVER_PI: f64 = f64::from_bits(0x3fe4_5f30_6dc9_c883);
const PI_HI: f64 = f64::from_bits(0x3ff9_21fb_5440_0000);
const PI_LO: f64 = f64::from_bits(0x3dd0_b461_1a62_6331);
const PI_MID: f64 = f64::from_bits(0x3dd0_b461_1a60_0000);
const PI_TAIL: f64 = f64::from_bits(0x3ba3_198a_2e03_7073);
const SIXTH: f64 = f64::from_bits(0x3fc5_5555_5555_5555);
const SIN_A: f64 = f64::from_bits(0x3ec7_1de3_a556_c734);
const SIN_B: f64 = f64::from_bits(0xbf2a_01a0_1a01_a01a);
const SIN_C: f64 = f64::from_bits(0x3f81_1111_1111_1111);
const SIN_D: f64 = f64::from_bits(0xbfc5_5555_5555_5555);
const COS_A: f64 = f64::from_bits(0xbe92_7e4f_b778_9f5c);
const COS_B: f64 = f64::from_bits(0x3efa_01a0_1a01_a019);
const COS_C: f64 = f64::from_bits(0xbf56_c16c_16c1_6c16);
const COS_D: f64 = f64::from_bits(0x3fa5_5555_5555_5555);
const PI_HALF: f64 = f64::from_bits(0x3ff9_21fb_5444_2d18);
const PI_HALF_HI: f64 = f64::from_bits(0x3ff9_21fb_5000_0000);
const PI_HALF_LO: f64 = f64::from_bits(0x3e51_10b4_6000_0000);
const PI_HALF_TAIL: f64 = f64::from_bits(0x3c91_a626_3314_5c06);
const TWO_OVER_PI_TABLE: [u8; 160] = [
    0xe0, 0xf1, 0x1b, 0xc1, 0x0c, 0x58, 0x21, 0x74, 0x35, 0x7e, 0xc4, 0x7e, 0xed, 0xaf, 0xa9, 0x4b,
    0x4a, 0x29, 0xde, 0xe7, 0x1c, 0xf4, 0xec, 0xc5, 0x97, 0xaf, 0x1f, 0xeb, 0x9e, 0xd4, 0xb5, 0xa8,
    0x7f, 0x79, 0x9a, 0xfd, 0x18, 0x3d, 0xdd, 0x26, 0x2c, 0x9f, 0x3c, 0xfb, 0xd9, 0xb4, 0x7d, 0xb4,
    0x29, 0x68, 0x2d, 0x46, 0xbc, 0xbc, 0x3f, 0x60, 0x16, 0x78, 0xff, 0x5f, 0xe2, 0x7f, 0xec, 0xa0,
    0xe4, 0xf7, 0x2e, 0x7e, 0x11, 0x72, 0xd2, 0xe7, 0x4c, 0x0d, 0xe6, 0x58, 0x47, 0xe6, 0x04, 0xf9,
    0x7d, 0xd1, 0x9a, 0xc0, 0x71, 0xa6, 0x13, 0x12, 0xed, 0xba, 0xd4, 0xd7, 0x08, 0xa2, 0xfb, 0x9c,
    0xa6, 0xc4, 0x72, 0xac, 0x77, 0xf8, 0x73, 0x48, 0x46, 0x27, 0xa8, 0xbb, 0x24, 0x19, 0x80, 0x4b,
    0x37, 0x09, 0xe9, 0xb8, 0x91, 0xdc, 0x86, 0x15, 0xef, 0x7a, 0xaf, 0x8e, 0x45, 0xf9, 0x07, 0x41,
    0x0e, 0xf1, 0x64, 0x56, 0x8a, 0x6d, 0x03, 0x77, 0xd3, 0xd4, 0x47, 0x5f, 0x9d, 0xf0, 0xa7, 0x54,
    0x10, 0x39, 0xb9, 0x0d, 0xe6, 0x8b, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

const SIGN: u64 = 1 << 63;
const FRACTION: u64 = (1 << 52) - 1;
const HIDDEN: u64 = 1 << 52;
const EXPONENT_SHIFT: u32 = 52;
const EXPONENT_BIAS: i32 = 1023;
const REDUCTION_OFFSET: usize = 134;
const BYTE_EXPONENT_SHIFT: u32 = 3;
const REDUCTION_SHIFT: u32 = 54;
const SSE_LIMIT: [u32; 2] = [0x4b74_2400, 0x48f4_2400];
const FMA_LIMIT: [u32; 2] = [0x4b80_0456, 0x4f49_0fdb];
const DIRECT_LIMIT: u32 = 0x3f49_0fdb;
const TINY_LIMIT: u32 = 0x3900_0000;
const CUBIC_LIMIT: u32 = 0x3c00_0000;
const CANCEL_BITS: u32 = 15;
const SMALL_EXPONENT: u32 = 0x3f2;
const TINY_EXPONENT: u32 = 0x3de;
const QUADRANT_MASK: u32 = 3;
const SPLIT_MASK: u64 = 0xffff_ffff_f800_0000;
const LOW_SHIFT: u32 = 12;

fn polynomial(value: f64, cosine: bool, fma: bool) -> f64 {
    let square = value * value;
    if cosine {
        if fma {
            let chain = square.mul_add(COS_A, COS_B).mul_add(square, COS_C).mul_add(square, COS_D);
            return chain.mul_add(square * square, 1.0 - square * 0.5);
        }
        let first = COS_A * square + COS_B;
        let second = COS_C * square + COS_D;
        return (-0.5 * square + 1.0) + (first * (square * square) + second) * (square * square);
    }
    if fma {
        let chain = square.mul_add(SIN_A, SIN_B).mul_add(square, SIN_C).mul_add(square, SIN_D);
        return chain.mul_add(value * square, value);
    }
    let first = SIN_A * square + SIN_B;
    let second = SIN_C * square + SIN_D;
    value + (first * (square * square) + second) * (value * square)
}

fn exponent(value: f64) -> u32 {
    const MASK: u64 = 0x7ff;
    ((value.to_bits() >> EXPONENT_SHIFT) & MASK) as u32
}

fn large(value: f64, cosine: bool, fma: bool) -> (f64, u32) {
    const BYTE_MASK: u32 = 7;
    let exponent = exponent(value) - EXPONENT_BIAS as u32;
    let offset = REDUCTION_OFFSET - (exponent >> BYTE_EXPONENT_SHIFT) as usize;
    let significand = (value.to_bits() & FRACTION) | HIDDEN;
    let words: [u64; 3] = std::array::from_fn(|slot| {
        let start = offset + slot * size_of::<u64>();
        u64::from_le_bytes(TWO_OVER_PI_TABLE[start..start + size_of::<u64>()].try_into().unwrap())
    });
    let product = u128::from(words[0]) * u128::from(significand);
    let mut low = product as u64;
    let product = u128::from(words[1]) * u128::from(significand) + (product >> u64::BITS);
    let mut mid = product as u64;
    let mut high = ((product >> u64::BITS) as u64).wrapping_add(words[2].wrapping_mul(significand));
    let shift = REDUCTION_SHIFT - (exponent & BYTE_MASK);
    let carry = (high >> (shift - 1)) & 1;
    let quadrant = ((high >> shift) + carry) as u32 & QUADRANT_MASK;
    let sign = if carry == 0 { 0 } else { SIGN };
    if carry != 0 {
        high = !high;
        mid = !mid;
        low = !low;
    }
    high &= (1 << shift) - 1;
    let mut power = -(shift as i32);
    if high == 0 {
        high = mid;
        mid = low;
        low = 0;
        power -= u64::BITS as i32;
    }
    let top = u64::BITS - 1 - high.leading_zeros();
    power += top as i32;
    let align = top as i32 - EXPONENT_SHIFT as i32;
    if align > 0 {
        let prior = high;
        high = high.wrapping_shr(align as u32);
        mid = mid.wrapping_shr(align as u32) | prior.wrapping_shl(u64::BITS - align as u32);
    } else if align < 0 {
        let count = (-align) as u32;
        let prior = mid;
        high = high.wrapping_shl(count) | prior.wrapping_shr(u64::BITS - count);
        mid = mid.wrapping_shl(count) | low.wrapping_shr(u64::BITS - count);
    }
    let biased = (power + EXPONENT_BIAS) as u64;
    let high_bits = (high & !HIDDEN) | sign | (biased << EXPONENT_SHIFT);
    let fraction = f64::from_bits(high_bits);
    let reduced = if fma && cosine {
        let count = mid.leading_zeros() + 1;
        let low_bits = (mid.wrapping_shl(count) >> LOW_SHIFT)
            | sign
            | ((biased - u64::from(count + EXPONENT_SHIFT)) << EXPONENT_SHIFT);
        let fraction_low = f64::from_bits(low_bits);
        let split = f64::from_bits(high_bits & SPLIT_MASK);
        let leftover = fraction - split;
        let lead = fraction * PI_HALF;
        let tail = split * PI_HALF_HI - lead;
        let tail = leftover.mul_add(PI_HALF_HI, tail);
        let tail = split.mul_add(PI_HALF_LO, tail);
        let tail = leftover.mul_add(PI_HALF_LO, tail);
        let low_product = fraction.mul_add(PI_HALF_TAIL, fraction_low * PI_HALF);
        lead + (tail + low_product)
    } else {
        fraction * PI_HALF
    };
    (reduced, quadrant)
}

pub(super) fn evaluate(input: f32, cosine: bool, fma: bool) -> f32 {
    let magnitude = input.to_bits() & !((SIGN >> u32::BITS) as u32);
    let value = f64::from(input);
    if magnitude <= DIRECT_LIMIT {
        if !cosine && magnitude < TINY_LIMIT {
            return input;
        }
        if fma && cosine && magnitude < TINY_LIMIT {
            return 1.0;
        }
        if magnitude < CUBIC_LIMIT && (fma || !cosine) {
            return if fma {
                if cosine {
                    (-(value * 0.5)).mul_add(value, 1.0)
                } else {
                    (-((value * value) * value)).mul_add(SIXTH, value)
                }
            } else {
                value - ((value * value) * value) * SIXTH
            } as f32;
        }
        return polynomial(value, cosine, fma) as f32;
    }
    let limit = if fma { FMA_LIMIT } else { SSE_LIMIT };
    let absolute = value.abs();
    let (reduced, quadrant, old_exp) = if magnitude < limit[usize::from(cosine)] {
        let quadrant =
            if fma { absolute.mul_add(TWO_OVER_PI, 0.5) } else { absolute * TWO_OVER_PI + 0.5 }
                as u32;
        let mut leading = if fma {
            (-f64::from(quadrant)).mul_add(PI_HI, absolute)
        } else {
            absolute - f64::from(quadrant) * PI_HI
        };
        let mut reduced = leading - f64::from(quadrant) * PI_LO;
        let old_exp = exponent(reduced);
        if !fma && exponent(absolute) - old_exp > CANCEL_BITS {
            let correction = f64::from(quadrant) * PI_MID;
            let tail = f64::from(quadrant) * PI_TAIL;
            let prior = leading;
            leading -= correction;
            let tail = tail - ((prior - leading) - correction);
            reduced = leading - tail;
        }
        (reduced, quadrant, old_exp)
    } else {
        let (reduced, quadrant) = large(absolute, cosine, fma);
        (reduced, quadrant, SMALL_EXPONENT)
    };
    let reduced_cosine = ((quadrant & 1) != 0) != cosine;
    let result = if !fma && old_exp < SMALL_EXPONENT {
        if old_exp <= TINY_EXPONENT {
            if reduced_cosine { 1.0 } else { reduced }
        } else if reduced_cosine {
            1.0 - (reduced * reduced) * 0.5
        } else {
            reduced - (SIXTH * reduced) * (reduced * reduced)
        }
    } else {
        polynomial(reduced, reduced_cosine, fma)
    };
    let negative = if cosine {
        ((quadrant + 1) & 2) != 0
    } else {
        (((quadrant >> 1) & 1) != 0) != (value < 0.0)
    };
    if negative {
        if fma { f64::from_bits(result.to_bits() ^ SIGN) as f32 } else { (0.0 - result) as f32 }
    } else {
        result as f32
    }
}
