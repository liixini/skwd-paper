use super::*;

fn sample() -> Vec<DrawingStroke> {
    vec![
        DrawingStroke {
            start: 0.0,
            end: 0.25,
            width: 7.0,
            hatch: false,
            points: vec![[1.0, 2.0], [3.5, 4.0], [8.0, 2.0]],
        },
        DrawingStroke {
            start: 0.5,
            end: 1.0,
            width: 24.0,
            hatch: true,
            points: vec![[1.6, 14.4], [30.4, -4.4]],
        },
    ]
}

#[test]
fn strokes_round_trip() {
    let strokes = sample();
    assert_eq!(DrawingStroke::decode(&DrawingStroke::encode(&strokes)).unwrap(), strokes);
}

#[test]
fn strokes_wire_shape() {
    let bytes = DrawingStroke::encode(&sample()[1..]);
    let words: Vec<u32> = bytes[4..]
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
        .collect();
    assert_eq!(&bytes[..4], b"SKDS");
    assert_eq!(words[..7], [1, 1, 0.5_f32.to_bits(), 1.0_f32.to_bits(), 24.0_f32.to_bits(), 1, 2]);
    assert_eq!(words.len(), 11);
}

#[test]
fn rejects_bad_strokes() {
    let good = DrawingStroke::encode(&sample());
    let mut cases = vec![good[..good.len() - 1].to_vec(), [good.clone(), vec![0; 4]].concat()];
    let mut magic = good.clone();
    magic[0] = b'X';
    cases.push(magic);
    for change in [
        |stroke: &mut DrawingStroke| stroke.start = 1.5,
        |stroke: &mut DrawingStroke| stroke.end = -0.1,
        |stroke: &mut DrawingStroke| stroke.width = 0.0,
        |stroke: &mut DrawingStroke| stroke.width = f32::NAN,
        |stroke: &mut DrawingStroke| stroke.points.truncate(1),
        |stroke: &mut DrawingStroke| stroke.points[0][1] = f32::INFINITY,
    ] {
        let mut strokes = sample();
        change(&mut strokes[0]);
        cases.push(DrawingStroke::encode(&strokes));
    }
    let mut kind = good;
    kind[4 + 4 * 5..4 + 4 * 6].copy_from_slice(&7_u32.to_le_bytes());
    cases.push(kind);
    let mut version = DrawingStroke::encode(&sample());
    version[4..8].copy_from_slice(&2_u32.to_le_bytes());
    cases.push(version);
    for case in cases {
        assert!(DrawingStroke::decode(&case).is_err());
    }
}

#[test]
fn rejects_huge_counts() {
    let mut bytes = b"SKDS".to_vec();
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(DrawingStroke::decode(&bytes).is_err());
}

#[test]
fn rejects_short_bodies() {
    let mut bytes = DrawingStroke::encode(&sample());
    bytes[8..12].copy_from_slice(&3_u32.to_le_bytes());
    assert!(DrawingStroke::decode(&bytes).is_err());
}

#[test]
fn rejects_segment_floods() {
    let points = vec![[0.0, 0.0]; MAX_SEGMENTS + 2];
    let flood = DrawingStroke { start: 0.0, end: 1.0, width: 1.0, hatch: false, points };
    assert!(DrawingStroke::decode(&DrawingStroke::encode(&[flood])).is_err());
}
