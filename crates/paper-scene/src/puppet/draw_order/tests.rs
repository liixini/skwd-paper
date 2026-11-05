use super::*;
use crate::puppet::{Animation, alpha};

const SECTIONS: [Section; 3] = [
    Section { bone: 0, bias: 0, first: 0, count: 3 },
    Section { bone: 1, bias: 0, first: 3, count: 3 },
    Section { bone: 2, bias: 0, first: 6, count: 3 },
];

fn clip(frames: [[f32; 3]; 2]) -> Animation {
    let mut animation =
        crate::puppet::parse(&crate::puppet::tests::fixture()).unwrap().animations.swap_remove(0);
    animation.id = 1;
    animation.fps = 1.0;
    animation.duration = 2;
    let tracks = (0..3).map(|bone| Some(vec![frames[0][bone], frames[1][bone], 0.0])).collect();
    animation.draw_order = alpha::Clip { tracks };
    animation
}

fn mesh(clips: Vec<Animation>) -> Mesh {
    let mut mesh = crate::puppet::parse(&crate::puppet::tests::fixture()).unwrap();
    mesh.animations = clips;
    mesh.indices = (0..9).collect();
    mesh.draw_order = Some(Layout { sections: SECTIONS.to_vec(), rest: vec![0, 100, 200] });
    mesh
}

fn layer(id: u32, blend: f32, additive: bool) -> AnimationLayer {
    AnimationLayer { id, rate: 1.0, blend, additive, visible: true }
}

fn resorted(
    sorter: &mut Sorter,
    mesh: &Mesh,
    samples: &[(AnimationLayer, f32)],
) -> Option<Vec<u8>> {
    sorter.update(mesh, samples).then(|| sorter.bytes(&mesh.indices))
}

fn first_indices(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks_exact(size_of::<u16>() * 3)
        .map(|run| u16::from_ne_bytes([run[0], run[1]]))
        .collect()
}

#[test]
fn track_moves_section() {
    let mesh = mesh(vec![clip([[300.0, 0.0, 0.0], [0.0, 0.0, 0.0]])]);
    let mut sorter = Sorter::of(&mesh).unwrap();
    let bytes = resorted(&mut sorter, &mesh, &[(layer(1, 1.0, false), 0.0)]).unwrap();
    assert_eq!(first_indices(&bytes), [3, 6, 0]);
}

#[test]
fn sorted_writes_nothing() {
    let mesh = mesh(vec![clip([[0.0, 1.0, 2.0], [300.0, 0.0, 0.0]])]);
    let mut sorter = Sorter::of(&mesh).unwrap();
    assert!(resorted(&mut sorter, &mesh, &[(layer(1, 1.0, false), 0.0)]).is_none());
    assert!(resorted(&mut sorter, &mesh, &[(layer(1, 1.0, false), 0.0)]).is_none());
}

#[test]
fn ties_keep_previous() {
    let mesh = mesh(vec![clip([[300.0, 0.0, 0.0], [100.0, 0.0, -100.0]])]);
    let mut sorter = Sorter::of(&mesh).unwrap();
    let samples = [(layer(1, 1.0, false), 0.0)];
    assert_eq!(first_indices(&resorted(&mut sorter, &mesh, &samples).unwrap()), [3, 6, 0]);
    let samples = [(layer(1, 1.0, false), 1.0)];
    assert!(resorted(&mut sorter, &mesh, &samples).is_none());
    assert_eq!(sorter.slots[0].section, SECTIONS[1]);
}

#[test]
fn hidden_layers_keep_order() {
    let mut mesh = mesh(vec![clip([[300.0, 0.0, 0.0], [0.0, 0.0, 0.0]])]);
    mesh.draw_order.as_mut().unwrap().rest = vec![200, 100, 0];
    let mut sorter = Sorter::of(&mesh).unwrap();
    let mut hidden = layer(1, 1.0, false);
    hidden.visible = false;
    assert!(resorted(&mut sorter, &mesh, &[(hidden, 0.0)]).is_none());
    assert_eq!(sorter.slots[0].section, SECTIONS[0]);
}

#[test]
fn disabled_tracks_stay_rest() {
    let mut animation = clip([[300.0, 0.0, 0.0], [0.0, 0.0, 0.0]]);
    animation.draw_order.tracks[0] = None;
    let mesh = mesh(vec![animation]);
    let mut sorter = Sorter::of(&mesh).unwrap();
    let samples = [(layer(1, 1.0, false), 0.0)];
    assert!(resorted(&mut sorter, &mesh, &samples).is_none());
}

#[test]
fn blend_follows_engine() {
    let cases = [
        (layer(1, 0.25, false), 10.0, 50.0, 0.0, 20.0),
        (layer(1, 1.0, false), 10.0, 50.0, 0.0, 50.0),
        (layer(1, 0.5, true), 10.0, 50.0, 20.0, 25.0),
        (layer(1, 1.0, true), 10.0, 50.0, -100.0, 50.0),
        (layer(1, 1.0, true), 10.0, 5.0, 50.0, 5.0),
    ];
    for (layer, current, value, rest, expected) in cases {
        assert_eq!(blended(current, value, rest, layer), expected, "{layer:?}");
    }
}

fn table(records: &[[u32; 4]]) -> Vec<u8> {
    let mut bytes = vec![0, 1];
    bytes.extend((size_of_val(records) as u32).to_le_bytes());
    bytes.extend(records.iter().flatten().flat_map(|word| word.to_le_bytes()));
    bytes
}

fn parsed(records: &[[u32; 4]], rest: &[i32], indices: usize) -> Option<Layout> {
    Layout::parse(&table(records), 0, crate::puppet::MAGIC_23, rest.to_vec(), indices)
}

#[test]
fn table_tiles_indices() {
    let reordered = [[1, 0, 3, 3], [0, 0, 0, 3]];
    assert_eq!(parsed(&reordered, &[0, 7], 6).unwrap().sections.len(), 2);
    let cases: [(&[[u32; 4]], &[i32], usize); 5] = [
        (&reordered, &[0], 6),
        (&reordered, &[], 6),
        (&reordered, &[0, 7], 9),
        (&[[0, 0, 0, 3], [0, 0, 2, 4]], &[0], 6),
        (&[[0, 0, 0, 2], [0, 0, 3, 3]], &[0], 6),
    ];
    for (records, rest, indices) in cases {
        assert!(parsed(records, rest, indices).is_none(), "{records:?} {rest:?} {indices}");
    }
    assert!(Layout::parse(&[1], 0, crate::puppet::MAGIC_23, vec![0], 6).is_none());
}

#[test]
fn stripped_mesh_skips() {
    let mut mesh = mesh(vec![clip([[0.0; 3]; 2])]);
    assert!(Sorter::of(&mesh).is_some());
    mesh.indices = Vec::new();
    assert!(Sorter::of(&mesh).is_none());
}

#[test]
fn clipless_meshes_skip() {
    let mut animation = clip([[0.0; 3]; 2]);
    animation.draw_order = alpha::Clip::default();
    assert!(Sorter::of(&mesh(vec![animation])).is_none());
}

const FIXTURE: &[u8] = include_bytes!("../../fixtures/draw-order.mdl");

#[test]
fn flagged_mesh_parses() {
    let mesh = crate::puppet::parse(FIXTURE).unwrap();
    let layout = mesh.draw_order.as_ref().unwrap();
    assert_eq!(layout.rest, [700]);
    assert_eq!(layout.sections[0], Section { bone: 0, bias: 5, first: 0, count: 1 });
    assert_eq!(layout.sections[1], Section { bone: 0, bias: 0, first: 1, count: 2 });
    assert_eq!(mesh.indices, [0, 1, 2]);
    assert_eq!(mesh.animations[0].draw_order.tracks, [Some(vec![1.0, 2.0, 3.0])]);
}

#[test]
fn unflagged_mesh_ignores() {
    let mut bytes = FIXTURE.to_vec();
    bytes[22..26].copy_from_slice(&0_u32.to_le_bytes());
    let mesh = crate::puppet::parse(&bytes).unwrap();
    assert!(mesh.draw_order.is_none());
    assert!(mesh.animations[0].draw_order.tracks.is_empty());
    assert!(!mesh.skipped_draw_order);
}

#[test]
fn missing_rest_keeps_puppet() {
    let mut bytes = FIXTURE.to_vec();
    let animation = crate::puppet::find(&bytes, b"MDLA0006\0").unwrap();
    bytes[animation - size_of::<i32>() - 1] = 0;
    let mesh = crate::puppet::parse(&bytes).unwrap();
    assert!(mesh.draw_order.is_none() && mesh.skipped_draw_order);
    assert_eq!(mesh.indices, [0, 1, 2]);
    assert!(mesh.animations[0].draw_order.tracks.is_empty());
}

#[test]
fn resort_moves_clip() {
    use crate::puppet::masks::{Clip, Command, Mask, Masks};
    let mut mesh = mesh(vec![clip([[300.0, 0.0, 0.0], [0.0, 0.0, 0.0]])]);
    let record = Mask { texture: String::new(), flags: 0, clipped: vec![1], base: vec![0] };
    let spans = SECTIONS.iter().map(|section| section.first..section.first + section.count);
    mesh.masks = Masks { records: vec![record], spans: spans.collect() };
    let mut sorter = Sorter::of(&mesh).unwrap();
    let cut = |base, clipped| Command::Clip(Clip { mask: 0, base, clipped, ancestors: Vec::new() });
    let rest = mesh.masks.plan(&sorter.sections(), &mesh.indices);
    assert_eq!(rest.commands, [Command::Draw(0..3), cut(3..6, 6..9), Command::Draw(9..12)]);
    assert!(sorter.update(&mesh, &[(layer(1, 1.0, false), 0.0)]));
    assert_eq!(sorter.sections(), [1, 2, 0]);
    let moved = mesh.masks.plan(&sorter.sections(), &mesh.indices);
    assert_eq!(moved.commands, [Command::Draw(0..6), cut(6..9, 9..12)]);
    assert_eq!(moved.indices, [6, 7, 8, 0, 1, 2, 0, 1, 2, 3, 4, 5]);
}
