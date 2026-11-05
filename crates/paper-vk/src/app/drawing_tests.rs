use crate::vk::{self, Renderer, Src};
use paper_control::DrawingInk;

const FIXTURE_SECONDS: f32 = 6.5;
const COST_FRAMES: usize = 120;
const COST_STEP: f32 = 1e-4;

#[test]
#[ignore = "requires a Vulkan device"]
fn drawing_gpu_frames() {
    let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut renderer = crate::shared::renderer(&shared, 64, 64).unwrap();
    renderer.ensure_scene_pool(4).unwrap();
    rejects_bad_textures(&mut renderer);
    let old = renderer.create_scene_texture(64, 64, &[17, 23, 31, 255].repeat(64 * 64)).unwrap();
    let new = renderer.create_scene_texture(64, 64, &[93, 147, 211, 255].repeat(64 * 64)).unwrap();
    let packed = renderer.create_scene_texture(64, 64, &ink_pixels()).unwrap();
    let plain_strokes = lines(&renderer, DrawingInk::Plain);
    let rainbow_strokes = lines(&renderer, DrawingInk::Rainbow);
    let mut export = renderer.create_export_image_opts(64, 64, false).unwrap();
    let target = if renderer.direct_render() {
        renderer.create_export_rt(&export).unwrap()
    } else {
        renderer.create_render_target(64, 64).unwrap()
    };
    let readback = renderer.create_readback_buf(64 * 64 * 4).unwrap();
    let plain = paper_control::DrawingColours::default();
    let shader = paper_shaders::effect_index(paper_shaders::DRAWING_SHADER).unwrap();
    let mut render = |progress, colours, strokes: &vk::DrawingStrokes| {
        let drawing =
            Src::Drawing { photo: new.view, ink: packed.view, strokes, focus: [0.5, 0.5], colours };
        renderer
            .render_effect_to(
                &target,
                &mut export,
                &Src::Rgba(old.view),
                [1.0, 1.0, 0.0, 0.0],
                &drawing,
                [1.0, 1.0, 0.0, 0.0],
                progress,
                shader,
            )
            .unwrap();
        renderer.read_export_to(&export, &readback, 64, 64).unwrap();
        unsafe { std::slice::from_raw_parts(readback.ptr, 64 * 64 * 4).to_vec() }
    };
    assert_eq!(render(0.0, plain, &plain_strokes), [31, 23, 17, 255].repeat(64 * 64));
    assert_eq!(render(1.0, plain, &plain_strokes), [211, 147, 93, 255].repeat(64 * 64));
    let first = (8 + 8 * 64) * 4;
    let last = (48 + 8 * 64) * 4;
    let early = render(0.3, plain, &plain_strokes);
    assert!(early[first] < 40, "drawn ink");
    assert!(early[last] > 220, "undrawn paper");
    let late = render(0.62, plain, &plain_strokes);
    assert!(late[last] < 40, "late ink");
    let chalk = paper_control::DrawingColours { paper: [20, 24, 22], ink: [250, 250, 245] };
    let chalked = render(0.63999, chalk, &plain_strokes);
    for (row, colour) in [(0, chalk.ink), (4, chalk.paper)] {
        let [r, g, b] = colour;
        let at = (8 + row * 64) * 4;
        for (value, expected) in chalked[at..at + 3].iter().zip([b, g, r]) {
            assert!(value.abs_diff(expected) <= 1, "chalk row {row}");
        }
    }
    let built = render(0.63999, plain, &plain_strokes);
    let finish_start = render(0.64, plain, &plain_strokes);
    let pouring = render(0.8, plain, &plain_strokes);
    assert!(
        pouring != finish_start && pouring != render(1.0, plain, &plain_strokes),
        "liquid reveal"
    );
    assert!(built[(8 + 3 * 64) * 4].abs_diff(200) <= 1, "fine ink alpha");
    let difference: u64 =
        built.iter().zip(&finish_start).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
    assert!(difference < built.len() as u64, "finish boundary");
    let rainbow = render(0.62, plain, &rainbow_strokes);
    let spread = |pixel: &[u8]| pixel[..3].iter().max().unwrap() - pixel[..3].iter().min().unwrap();
    assert!(spread(&late[last..last + 4]) <= 1, "plain ink is neutral");
    assert!(spread(&rainbow[last..last + 4]) > 40, "rainbow ink is coloured");
    let paper = (8 + 5 * 64) * 4;
    assert_eq!(rainbow[paper..paper + 3], late[paper..paper + 3], "rainbow paper");
    renderer.wait_render().unwrap();
}

#[test]
#[ignore = "requires a Vulkan device and SKWD_DRAWING_FIXTURE"]
fn drawing_fixture_frames() {
    let fixture = Fixture::open();
    let (width, height) = fixture.size;
    let mut renderer = crate::shared::renderer(&fixture.shared, width, height).unwrap();
    renderer.ensure_scene_pool(4).unwrap();
    let assets = fixture.assets(&mut renderer);
    let mut export = renderer.create_export_image_opts(width, height, false).unwrap();
    let target = renderer.create_render_target(width, height).unwrap();
    let destination = std::path::PathBuf::from(std::env::var("SKWD_DRAWING_FRAMES").unwrap());
    std::fs::create_dir_all(&destination).unwrap();
    for progress in [0.0, 0.079, 0.08, 0.22, 0.36, 0.5, 0.6399, 0.64, 0.75, 0.9, 1.0] {
        assets.render(&mut renderer, &target, &mut export, progress);
        let pixels =
            super::super::dmabuf_helpers::read_rgba_export(&mut renderer, &export, fixture.size)
                .unwrap();
        if progress == paper_shaders::DRAWING_BUILD_END
            && (width, height) == (fixture.manifest.width, fixture.manifest.height)
            && fixture.drawing.ink == DrawingInk::Plain
        {
            assert_native_ink(pixels.as_raw(), &fixture.ink);
        }
        pixels.save(destination.join(format!("{progress:.4}.png"))).unwrap();
    }
    renderer.wait_render().unwrap();
}

#[test]
#[ignore = "requires a Vulkan device and SKWD_DRAWING_FIXTURE"]
fn drawing_fixture_cost() {
    let fixture = Fixture::open();
    let (width, height) = fixture.size;
    let mut renderer = crate::shared::renderer(&fixture.shared, width, height).unwrap();
    renderer.ensure_scene_pool(4).unwrap();
    let assets = fixture.assets(&mut renderer);
    let mut export = renderer.create_export_image_opts(width, height, false).unwrap();
    let target = renderer.create_render_target(width, height).unwrap();
    for (label, from) in [("drawing", 0.6), ("finish", 0.8)] {
        let mut times: Vec<f64> = (0..COST_FRAMES)
            .map(|frame| {
                let started = std::time::Instant::now();
                assets.render(&mut renderer, &target, &mut export, from + frame as f32 * COST_STEP);
                renderer.wait_render().unwrap();
                started.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        times.sort_by(f64::total_cmp);
        println!(
            "drawing cost {label} {width}x{height} {:?} {} segments: median {:.3} ms, p95 {:.3} ms",
            fixture.drawing.ink,
            fixture.strokes.iter().map(|stroke| stroke.points.len() - 1).sum::<usize>(),
            times[times.len() / 2],
            times[times.len() * 95 / 100]
        );
    }
}

struct Fixture {
    shared: crate::shared::SharedDevice,
    manifest: paper_control::DrawingManifest,
    drawing: paper_control::DrawingTransition,
    ink: std::path::PathBuf,
    strokes: Vec<paper_control::DrawingStroke>,
    size: (u32, u32),
}

struct Assets {
    photo: vk::SceneTexture,
    ink: vk::SceneTexture,
    strokes: vk::DrawingStrokes,
    focus: [f32; 2],
    colours: paper_control::DrawingColours,
}

impl Fixture {
    fn open() -> Self {
        let artifact = std::env::var("SKWD_DRAWING_FIXTURE").unwrap();
        let manifest: paper_control::DrawingManifest =
            serde_json::from_slice(&std::fs::read(&artifact).unwrap()).unwrap();
        let ink = match std::env::var("SKWD_DRAWING_INK").as_deref() {
            Ok("rainbow") => DrawingInk::Rainbow,
            _ => DrawingInk::Plain,
        };
        let drawing = paper_control::DrawingTransition {
            artifact: artifact.into(),
            colours: paper_control::DrawingColours::default(),
            ink,
        };
        let (_, ink, strokes) = drawing.load(&manifest.source.path).unwrap();
        let size = std::env::var("SKWD_DRAWING_SIZE").ok().map_or(
            (manifest.width, manifest.height),
            |size| {
                let (width, height) = size.split_once('x').unwrap();
                (width.parse().unwrap(), height.parse().unwrap())
            },
        );
        Self {
            shared: crate::shared::create(std::ptr::null_mut()).unwrap(),
            strokes: paper_control::DrawingStroke::read(&strokes).unwrap(),
            manifest,
            drawing,
            ink,
            size,
        }
    }

    fn assets(&self, renderer: &mut Renderer) -> Assets {
        let (width, height) = self.size;
        let photo = image::open(&self.manifest.source.path)
            .unwrap()
            .resize_exact(width, height, image::imageops::FilterType::Triangle)
            .into_rgba8();
        let extent = ash::vk::Extent2D {
            width: self.manifest.timing_width,
            height: self.manifest.timing_height,
        };
        let spec = vk::StrokeSpec { extent, ink: self.drawing.ink, seconds: FIXTURE_SECONDS };
        let size = (self.manifest.width, self.manifest.height);
        Assets {
            photo: renderer.create_scene_texture(width, height, photo.as_raw()).unwrap(),
            ink: super::load_texture(renderer, &self.ink, size).unwrap(),
            strokes: renderer.create_drawing_strokes(&self.strokes, spec).unwrap(),
            focus: self.manifest.focus,
            colours: self.drawing.colours,
        }
    }
}

impl Assets {
    fn render(
        &self,
        renderer: &mut Renderer,
        target: &vk::RenderTarget,
        export: &mut vk::ExportImage,
        progress: f32,
    ) {
        let drawing = Src::Drawing {
            photo: self.photo.view,
            ink: self.ink.view,
            strokes: &self.strokes,
            focus: self.focus,
            colours: self.colours,
        };
        renderer
            .render_effect_to(
                target,
                export,
                &Src::Rgba(self.photo.view),
                [1.0, 1.0, 0.0, 0.0],
                &drawing,
                [1.0, 1.0, 0.0, 0.0],
                progress,
                paper_shaders::effect_index(paper_shaders::DRAWING_SHADER).unwrap(),
            )
            .unwrap();
    }
}

fn lines(renderer: &Renderer, ink: DrawingInk) -> vk::DrawingStrokes {
    let strokes: Vec<_> = (0..8)
        .map(|row| {
            let y = row as f32 * 8.0 + 1.5;
            paper_control::DrawingStroke {
                start: 0.0,
                end: 1.0,
                width: 7.0,
                hatch: false,
                points: vec![[0.0, y], [64.0, y]],
            }
        })
        .collect();
    let extent = ash::vk::Extent2D { width: 64, height: 64 };
    let spec = vk::StrokeSpec { extent, ink, seconds: FIXTURE_SECONDS };
    renderer.create_drawing_strokes(&strokes, spec).unwrap()
}

fn rejects_bad_textures(renderer: &mut Renderer) {
    let corrupt = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(corrupt.path(), b"invalid PNG").unwrap();
    assert!(super::load_texture(renderer, corrupt.path(), (64, 64)).is_err());
    assert!(
        super::load_texture(renderer, &corrupt.path().with_extension("missing"), (64, 64)).is_err()
    );
}

fn assert_native_ink(pixels: &[u8], ink: &std::path::Path) {
    let ink = image::open(ink).unwrap().into_rgba8();
    for (actual, mark) in pixels.chunks_exact(4).zip(ink.pixels()) {
        for (value, paper) in actual.iter().zip(paper_control::DRAWING_PAPER.map(u16::from)) {
            let expected = ((paper * u16::from(u8::MAX - mark[0]) + 127) / 255) as u8;
            assert!(value.abs_diff(expected) <= 1, "native ink coverage");
        }
    }
}

fn ink_pixels() -> Vec<u8> {
    (0..64)
        .flat_map(|y| {
            (0_u32..64).flat_map(move |x| {
                [
                    match y % 8 {
                        0..=2 => 255,
                        3 => 32,
                        _ => 0,
                    },
                    if x < 32 { 32 } else { 220 },
                    (x * 4) as u8,
                    0,
                ]
            })
        })
        .collect::<Vec<_>>()
}

#[test]
#[ignore = "requires a Vulkan device"]
fn drawing_styled_photo() {
    let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut renderer = crate::shared::renderer(&shared, 64, 64).unwrap();
    renderer.ensure_scene_pool(16).unwrap();
    let raw = image::RgbaImage::from_fn(64, 64, |x, y| {
        image::Rgba(if (x / 8 + y / 8) % 2 == 0 { [220, 90, 30, 255] } else { [20, 170, 230, 255] })
    });
    let old = styled_photo(&mut renderer, &raw, 11);
    let new = styled_photo(&mut renderer, &raw, 33);
    assert_ne!(old, new);
    assert_ne!(old, raw);
    crate::surface::set(&paper_control::SurfacePolicy {
        namespace: "test".into(),
        blur: 0,
        dim: 0,
    });
    let old_texture = renderer.create_scene_texture(64, 64, old.as_raw()).unwrap();
    let texture = renderer.create_scene_texture(64, 64, raw.as_raw()).unwrap();
    let mut photo =
        super::super::dmabuf_helpers::RgbaStillSource { texture, uvs: vec![[1.0, 1.0, 0.0, 0.0]] };
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("photo.ppm");
    paper_runtime::snapshot::write_rgba(&path, 64, 64, new.as_raw()).unwrap();
    super::replace_photo(&mut renderer, &mut photo, &path, &[(64, 64)]).unwrap();
    let ink = renderer.create_scene_texture(64, 64, &ink_pixels()).unwrap();
    let strokes = lines(&renderer, DrawingInk::Plain);
    let mut export = renderer.create_export_image_opts(64, 64, false).unwrap();
    let target = renderer.create_render_target(64, 64).unwrap();
    for (progress, expected) in [(0.0, &old), (1.0, &new)] {
        renderer
            .render_effect_to(
                &target,
                &mut export,
                &Src::Rgba(old_texture.view),
                [1.0, 1.0, 0.0, 0.0],
                &Src::Drawing {
                    photo: photo.texture.view,
                    ink: ink.view,
                    strokes: &strokes,
                    focus: [0.5, 0.5],
                    colours: paper_control::DrawingColours::default(),
                },
                photo.uvs[0],
                progress,
                paper_shaders::effect_index(paper_shaders::DRAWING_SHADER).unwrap(),
            )
            .unwrap();
        let actual =
            super::super::dmabuf_helpers::read_rgba_export(&mut renderer, &export, (64, 64))
                .unwrap();
        assert_eq!(&actual, expected, "endpoint {progress}");
    }
    renderer.render_to(&target, &mut export, &Src::Rgba(photo.texture.view), photo.uvs[0]).unwrap();
    assert_eq!(
        super::super::dmabuf_helpers::read_rgba_export(&mut renderer, &export, (64, 64)).unwrap(),
        new
    );
    drop(target);
    renderer.destroy_scene_texture(old_texture);
    renderer.destroy_scene_texture(photo.texture);
    renderer.destroy_scene_texture(ink);
}

fn styled_photo(renderer: &mut Renderer, raw: &image::RgbaImage, blur: u32) -> image::RgbaImage {
    crate::surface::set(&paper_control::SurfacePolicy { namespace: "test".into(), blur, dim: 44 });
    let texture = renderer.create_scene_texture(64, 64, raw.as_raw()).unwrap();
    let target = renderer.create_render_target(64, 64).unwrap();
    let mut export = renderer.create_export_image_opts(64, 64, false).unwrap();
    renderer
        .render_to(&target, &mut export, &Src::Rgba(texture.view), [1.0, 1.0, 0.0, 0.0])
        .unwrap();
    let result =
        super::super::dmabuf_helpers::read_rgba_export(renderer, &export, (64, 64)).unwrap();
    drop(target);
    renderer.destroy_scene_texture(texture);
    result
}

#[test]
fn drawing_names_agree() {
    assert_eq!(paper_shaders::DRAWING_SHADER, paper_control::DRAWING_EFFECT);
}
