use super::{instructions, names_globals, normalize};

fn fixture(vector: bool) -> Vec<u32> {
    let mut words = vec![0x0723_0203, 0x0001_0500, 0, 20, 0];
    words.extend([6 << 16 | 0xb, 1, 0x4c53_4c47, 0x6474_732e, 0x3035_342e, 0]);
    words.extend([3 << 16 | 0x16, 2, 32]);
    let ty = if vector {
        words.extend([4 << 16 | 0x17, 3, 2, 2]);
        3
    } else {
        2
    };
    words.extend([5 << 16 | 0x85, ty, 6, 4, 5]);
    words.extend([6 << 16 | 0xc, ty, 7, 1, 8, 6]);
    words.extend([5 << 16 | 0x83, ty, 8, 6, 7]);
    words
}

#[test]
fn fuses_fractional_product() {
    for vector in [false, true] {
        let input = fixture(vector);
        let output = normalize(input.clone()).unwrap();
        let ty = if vector { 3 } else { 2 };
        assert_eq!(output[3], 21);
        assert_eq!(
            &output[output.len() - 12..],
            &[4 << 16 | 0x7f, ty, 20, 7, 8 << 16 | 0xc, ty, 8, 1, 50, 4, 5, 20]
        );
        assert_eq!(&output[5..input.len() - 5], &input[5..input.len() - 5]);
        assert_eq!(normalize(output.clone()).unwrap(), output);
    }
}

#[test]
fn preserves_precise_products() {
    for id in [6, 7, 8] {
        let mut input = fixture(true);
        input.splice(5..5, [3 << 16 | 0x47, id, 42]);
        assert_eq!(normalize(input.clone()).unwrap(), input);
    }
    let mut input = fixture(true);
    input.splice(5..5, [3 << 16 | 0x4a, 10, 6]);
    assert_eq!(normalize(input.clone()).unwrap(), input);
}

#[test]
fn rejects_other_patterns() {
    let input = fixture(false);
    for (index, replacement) in [(13, 64), (16, 9), (22, 9), (24, 9), (28, 5)] {
        let mut changed = input.clone();
        changed[index] = replacement;
        assert_eq!(normalize(changed.clone()).unwrap(), changed, "index {index}");
    }
}

#[test]
fn rejects_broken_modules() {
    for words in [vec![], vec![0x0723_0203, 0, 0, 1, 0, 0], vec![0x0723_0203, 0, 0, 1, 0, 2 << 16]]
    {
        assert!(normalize(words).is_err());
    }
    for bound in [0, 8, u32::MAX] {
        let mut input = fixture(false);
        input[3] = bound;
        assert!(normalize(input).is_err());
    }
}

#[test]
fn rejects_broken_instructions() {
    for tail in [vec![0], vec![2 << 16 | 0x5], vec![u32::MAX]] {
        let mut words = vec![0x0723_0203, 0x0001_0500, 0, 20, 0];
        assert!(instructions(&words).is_ok());
        words.extend(tail);
        assert_eq!(instructions(&words).unwrap_err().to_string(), "invalid spirv instruction");
        assert!(normalize(words.clone()).is_err());
        assert!(names_globals(&words).is_err());
    }
    let mut words = fixture(false);
    words.pop();
    assert_eq!(instructions(&words).unwrap_err().to_string(), "invalid spirv instruction");
    assert!(normalize(words).is_err());
}

#[test]
fn reads_global_names() {
    let mut input = fixture(false);
    assert!(!names_globals(&input).unwrap());
    input.extend([5 << 16 | 0x5, 9, 0x6f6c_4724, 0x736c_6162, 0]);
    assert!(names_globals(&input).unwrap());
    input.pop();
    assert!(names_globals(&input).is_err());
}

#[test]
fn preserves_empty_module() {
    let input = vec![0x0723_0203, 0x0001_0500, 0, 1, 0];
    assert_eq!(normalize(input.clone()).unwrap(), input);
    assert!(instructions(&input).unwrap().is_empty());
}
