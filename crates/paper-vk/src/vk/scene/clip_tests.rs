use super::*;
use crate::vk::SceneIndices;
use paper_scene::puppet::masks::{Mask, Masks};

const SIZE: u32 = 8;
const FRONT: [u16; 6] = [0, 2, 1, 1, 2, 3];
const BACK: [u16; 6] = [0, 1, 2, 2, 1, 3];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255; 4];
const PALETTE: [[u8; 4]; 4] = [RED, GREEN, BLUE, WHITE];
const CLEAR: [u8; 4] = [0; 4];
const FULL: [f32; 4] = [-0.5, -0.5, 0.5, 0.5];
const LEFT: [f32; 4] = [-0.5, -0.5, 0.0, 0.5];
const TOP: [f32; 4] = [-0.5, -0.5, 0.5, 0.0];
const BAND: [f32; 4] = [-0.25, -0.5, 0.5, 0.5];

fn headless() -> Result<Renderer> {
    let shared = crate::shared::test_device();
    let mut renderer = Renderer::new_shared_headless(
        (
            shared.entry.clone(),
            shared.instance.clone(),
            shared.phys,
            shared.device.clone(),
            shared.gfx_family,
            shared.queue,
        ),
        SIZE,
        SIZE,
    )?;
    renderer.ensure_scene_pipelines()?;
    renderer.ensure_scene_pool(4)?;
    Ok(renderer)
}

fn quads(rects: &[[f32; 4]], corners: [u16; 6]) -> (Vec<[f32; 5]>, Vec<u16>) {
    let texel = 1.0 / rects.len() as f32;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (index, [left, top, right, bottom]) in rects.iter().copied().enumerate() {
        let u = texel * (index as f32 + 0.5);
        let first = vertices.len() as u16;
        for (x, y) in [(left, top), (right, top), (left, bottom), (right, bottom)] {
            vertices.push([x, y, u, y + 0.5, 1.0]);
        }
        indices.extend(corners.map(|corner| first + corner));
    }
    (vertices, indices)
}

fn mask(flags: u32, clipped: u32, base: u32) -> Mask {
    Mask { texture: String::new(), flags, clipped: vec![clipped], base: vec![base] }
}

struct Case {
    rects: Vec<[f32; 4]>,
    records: Vec<Mask>,
    samples: vk::SampleCountFlags,
    painted: [u8; 2],
    planes: u32,
    corners: [u16; 6],
}

impl Case {
    fn new(rects: Vec<[f32; 4]>, records: Vec<Mask>) -> Self {
        Self {
            rects,
            records,
            samples: vk::SampleCountFlags::TYPE_1,
            painted: [u8::MAX; 2],
            planes: SIZE,
            corners: FRONT,
        }
    }
}

fn render(renderer: &mut Renderer, case: Case) -> Result<Vec<[u8; 4]>> {
    let (vertices, indices) = quads(&case.rects, case.corners);
    let spans = (0..case.rects.len()).map(|index| index * 6..index * 6 + 6).collect();
    let order: Vec<_> = (0..case.rects.len()).collect();
    renderer.ensure_clip_pipelines(case.samples)?;
    renderer.scene_pass_append_for(vk::Format::R8G8B8A8_UNORM, case.samples)?;
    let target = renderer.create_scene_target_opts(super::super::SceneTargetSpec {
        depth_attachment: false,
        extent: vk::Extent2D { width: SIZE, height: SIZE },
        format: vk::Format::R8G8B8A8_UNORM,
        repeat: false,
        samples: case.samples,
    })?;
    let colors: Vec<u8> = PALETTE.into_iter().take(case.rects.len()).flatten().collect();
    let albedo = renderer.create_scene_texture(case.rects.len() as u32, 1, &colors)?;
    let rows = case.painted.map(|value| [value, value, value, u8::MAX]).concat();
    let painted = renderer.create_scene_texture(1, 2, &rows)?;
    let nested = case.records.len() > 1;
    let extent = vk::Extent2D { width: case.planes, height: case.planes };
    let planes = renderer.create_clip_planes(extent, nested)?;
    let sources = case
        .records
        .iter()
        .map(|record| ClipSource {
            set: painted.set,
            additive: record.additive(),
            inverted: record.inverted(),
        })
        .collect();
    let plan = Masks { records: case.records, spans }.plan(&order, &indices);
    let mut mesh =
        renderer.create_scene_mesh(vertices.iter().copied(), SceneIndices::U16(&plan.indices))?;
    mesh.set_clip(PuppetClip::new(plan, sources, &planes));
    let positions: Vec<_> = vertices.iter().map(|[x, y, ..]| [*x, -*y, 0.0]).collect();
    renderer.update_scene_mesh(&mesh, &positions, &[], (1.0, 1.0))?;
    renderer.render_scene_mesh(&target, &albedo, &mesh)?;
    let (_, _, pixels) = renderer.read_scene_target(&target)?;
    renderer.destroy_clip_planes(planes);
    renderer.destroy_scene_texture(albedo);
    renderer.destroy_scene_texture(painted);
    renderer.destroy_scene_target(target);
    Ok(pixels.chunks_exact(4).map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]]).collect())
}

fn at(pixels: &[[u8; 4]], x: u32, y: u32) -> [u8; 4] {
    pixels[(y * SIZE + x) as usize]
}

fn quadrants(pixels: &[[u8; 4]]) -> [[u8; 4]; 4] {
    let (near, far, bottom) = (SIZE / 4, SIZE * 3 / 4, SIZE - 1);
    [at(pixels, near, 0), at(pixels, far, 0), at(pixels, near, bottom), at(pixels, far, bottom)]
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn clipped_inside_base() -> Result<()> {
    let mut renderer = headless()?;
    for samples in [vk::SampleCountFlags::TYPE_1, vk::SampleCountFlags::TYPE_4] {
        let top_only = [u8::MAX, 0];
        let plain =
            Case { samples, painted: top_only, ..Case::new(vec![LEFT, FULL], vec![mask(0, 1, 0)]) };
        assert_eq!(quadrants(&render(&mut renderer, plain)?), [GREEN, CLEAR, RED, CLEAR]);
        let inverted =
            Case { samples, painted: top_only, ..Case::new(vec![LEFT, FULL], vec![mask(2, 1, 0)]) };
        assert_eq!(quadrants(&render(&mut renderer, inverted)?), [RED, GREEN, GREEN, GREEN]);
        let culled =
            Case { samples, corners: BACK, ..Case::new(vec![LEFT, FULL], vec![mask(0, 1, 0)]) };
        assert_eq!(quadrants(&render(&mut renderer, culled)?), [RED, CLEAR, RED, CLEAR]);
    }
    Ok(())
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn nested_masks_multiply() -> Result<()> {
    let live = Renderer::scene_target_live_allocations();
    let mut renderer = headless()?;
    let case = Case::new(vec![LEFT, TOP, FULL], vec![mask(0, 1, 0), mask(0, 2, 1)]);
    assert_eq!(quadrants(&render(&mut renderer, case)?), [BLUE, CLEAR, RED, CLEAR]);
    let chain = vec![mask(0, 1, 0), mask(0, 2, 1), mask(0, 3, 2)];
    let pixels = render(&mut renderer, Case::new(vec![LEFT, TOP, BAND, FULL], chain))?;
    let row = |y| [1, 3, 6].map(|x| at(&pixels, x, y));
    assert_eq!(row(1), [GREEN, WHITE, CLEAR]);
    assert_eq!(row(6), [RED, RED, CLEAR]);
    assert_eq!(Renderer::scene_target_live_allocations(), live);
    Ok(())
}

#[cfg(feature = "shared-device")]
#[test]
#[ignore = "requires isolated Vulkan qualification"]
fn planes_bound_targets() -> Result<()> {
    let mut renderer = headless()?;
    let larger = Case { planes: SIZE * 2, ..Case::new(vec![LEFT, FULL], vec![mask(0, 1, 0)]) };
    assert_eq!(quadrants(&render(&mut renderer, larger)?), [GREEN, CLEAR, GREEN, CLEAR]);
    let smaller = Case { planes: SIZE / 2, ..Case::new(vec![LEFT, FULL], vec![mask(0, 1, 0)]) };
    assert_eq!(quadrants(&render(&mut renderer, smaller)?), [RED, CLEAR, RED, CLEAR]);
    Ok(())
}
