use super::*;
use crate::puppet::draw_order::SECTION_BYTES;

pub(crate) fn record(texture: &str, flags: u32, clipped: &[u32], base: &[u32]) -> Vec<u8> {
    let mut bytes = 7_u64.to_le_bytes().to_vec();
    bytes.extend(texture.as_bytes());
    bytes.push(0);
    bytes.extend(flags.to_le_bytes());
    for list in [clipped, base] {
        bytes.extend((list.len() as u32).to_le_bytes());
        bytes.extend(list.iter().flat_map(|word| word.to_le_bytes()));
    }
    bytes
}

fn tail(records: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![0, 1];
    bytes.extend((SECTION_BYTES as u32 * 2).to_le_bytes());
    for (first, count) in [(0_u32, 3_u32), (3, 3)] {
        bytes.extend([0_u32, 0, first, count].iter().flat_map(|word| word.to_le_bytes()));
    }
    bytes.extend((records.len() as u32).to_le_bytes());
    bytes.extend(records.iter().flatten());
    bytes
}

#[test]
fn records_parse() {
    let bytes = tail(&[record("masks/eye", 1, &[2], &[0, 1])]);
    let masks = parse(&bytes, 0, crate::puppet::MAGIC_23).unwrap();
    let expected =
        Mask { texture: "masks/eye".into(), flags: 1, clipped: vec![2], base: vec![0, 1] };
    assert_eq!(masks.records, [expected]);
    assert_eq!(masks.spans, [0..3, 3..6]);
    assert_eq!(parse(&bytes, 0, crate::puppet::MAGIC_21).unwrap(), Masks::default());
}

#[test]
fn truncated_records_fail() {
    let bytes = tail(&[record("masks/eye", 0, &[2], &[0, 1])]);
    for length in [bytes.len() - 1, bytes.len() - size_of::<u32>() * 2, 7, 40] {
        assert!(parse(&bytes[..length], 0, crate::puppet::MAGIC_23).is_err(), "{length}");
    }
}

fn mask(flags: u32, clipped: &[u32], base: &[u32]) -> Mask {
    Mask { texture: String::new(), flags, clipped: clipped.to_vec(), base: base.to_vec() }
}

fn masks(records: Vec<Mask>, spans: &[Range<usize>]) -> Masks {
    Masks { records, spans: spans.to_vec() }
}

fn clip(mask: usize, base: Range<u32>, clipped: Range<u32>) -> Command {
    Command::Clip(Clip { mask, base, clipped, ancestors: Vec::new() })
}

#[test]
fn clip_follows_base() {
    let indices: Vec<u16> = (0..12).collect();
    let set = masks(vec![mask(0, &[2], &[1])], &[0..3, 3..6, 6..9, 9..12]);
    let plan = set.plan(&[0, 1, 2, 3], &indices);
    assert_eq!(plan.commands, [Command::Draw(0..6), clip(0, 6..9, 9..12), Command::Draw(12..15)]);
    let expected: Vec<u16> = [0..6, 3..6, 6..9, 9..12].into_iter().flatten().collect();
    assert_eq!(plan.indices, expected);
}

#[test]
fn span_covers_between() {
    let indices: Vec<u16> = (0..4).collect();
    let set = masks(vec![mask(0, &[1], &[3, 2])], &[0..1, 1..2, 2..3, 3..4]);
    let plan = set.plan(&[3, 0, 1, 2], &indices);
    assert_eq!(plan.commands, [Command::Draw(0..3), clip(0, 3..6, 6..7)]);
    assert_eq!(plan.indices, [3, 0, 2, 3, 0, 2, 1]);
}

#[test]
fn nested_clip_waits() {
    let indices: Vec<u16> = (0..4).collect();
    let set = masks(vec![mask(0, &[1], &[0]), mask(0, &[2], &[1])], &[0..1, 1..2, 2..3, 3..4]);
    let plan = set.plan(&[0, 1, 2, 3], &indices);
    let nested =
        Command::Clip(Clip { mask: 1, base: 3..4, clipped: 4..5, ancestors: vec![(0, 1..2)] });
    assert_eq!(
        plan.commands,
        [Command::Draw(0..1), clip(0, 1..2, 2..3), nested, Command::Draw(5..6)]
    );
    assert_eq!(plan.indices, [0, 0, 1, 1, 2, 3]);
}

#[test]
fn unsupported_masks() {
    let spans = [0..1, 1..2];
    assert!(masks(vec![mask(3, &[0], &[1])], &spans).supported(2));
    let cases = [
        (vec![mask(4, &[0], &[1])], 2),
        (vec![mask(0, &[0], &[2])], 2),
        (vec![mask(0, &[0], &[1]), mask(0, &[1], &[0])], 2),
        (vec![mask(0, &[0], &[1])], 1),
        (vec![mask(0, &[0], &[1])], 3),
    ];
    for (records, indices) in cases {
        let set = masks(records, &spans);
        assert!(!set.supported(indices), "{set:?}");
    }
}

#[test]
fn overlapping_spans_rejected() {
    assert!(!masks(vec![mask(0, &[0], &[1])], &[0..2, 1..2]).supported(2));
}

#[test]
fn oversized_plan_unmasked() {
    let indices: Vec<u16> = (0..4).collect();
    let set = masks(vec![mask(0, &[1], &[0])], &[0..4, 0..4]);
    let plan = set.plan(&[0, 1], &indices);
    assert_eq!(plan.commands, [Command::Draw(0..8)]);
}

#[test]
fn two_ancestor_chain() {
    let indices: Vec<u16> = (0..4).collect();
    let records = vec![mask(0, &[1], &[0]), mask(0, &[2], &[1]), mask(0, &[3], &[2])];
    let set = masks(records, &[0..1, 1..2, 2..3, 3..4]);
    let plan = set.plan(&[0, 1, 2, 3], &indices);
    let chain =
        |mask, base, clipped, ancestors| Command::Clip(Clip { mask, base, clipped, ancestors });
    assert_eq!(
        plan.commands,
        [
            Command::Draw(0..1),
            clip(0, 1..2, 2..3),
            chain(1, 3..4, 4..5, vec![(0, 1..2)]),
            chain(2, 5..6, 6..7, vec![(1, 3..4), (0, 1..2)]),
        ]
    );
    assert_eq!(plan.indices, [0, 0, 1, 1, 2, 2, 3]);
}

#[test]
fn child_before_ancestor() {
    let indices: Vec<u16> = (0..3).collect();
    let set = masks(vec![mask(0, &[1], &[2]), mask(0, &[0], &[1])], &[0..1, 1..2, 2..3]);
    let plan = set.plan(&[0, 1, 2], &indices);
    assert_eq!(plan.commands, [Command::Draw(0..1), clip(1, 1..2, 2..3), clip(0, 3..4, 4..5)]);
}
