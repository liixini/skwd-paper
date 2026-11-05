#[test]
#[ignore = "requires Vulkan and native image instance controls"]
fn instance_material_pixels() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_IMAGE_INSTANCE_ROOT").unwrap());
    let dir = root.join("probe");
    let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let shared = crate::shared::test_device();
    let mut group = crate::app::scene::build_group(
        &shared,
        &mut model,
        true,
        &[(1920, 1080)],
        paper_geom::FillMode::Fit,
    )?;
    group.compose(1.0, 1.0 / 60.0)?;
    let (width, height, pixels) = group.read_canvas()?;
    let rendered = image::RgbaImage::from_raw(width, height, pixels).unwrap();
    rendered.save(root.join("paper.png"))?;
    for ((x, y), expected) in [
        ((240, 340), [255, 255, 255]),
        ((720, 340), [255, 0, 0]),
        ((1200, 340), [0, 255, 0]),
        ((1680, 340), [0, 0, 255]),
        ((720, 800), [255, 255, 255]),
        ((1200, 800), [0, 255, 0]),
    ] {
        assert_eq!(rendered.get_pixel(x, y).0[..3], expected, "{x},{y}");
    }
    let native = image::open(std::env::var_os("SKWD_IMAGE_INSTANCE_REFERENCE").unwrap())?.to_rgb8();
    assert_eq!(rendered.dimensions(), native.dimensions());
    assert!(
        rendered
            .pixels()
            .zip(native.pixels())
            .all(|(actual, expected)| actual.0[..3] == expected.0)
    );
    group.destroy();
    Ok(())
}

fn compare_instance_fixture(fixture: &str) -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("SKWD_IMAGE_INSTANCE_ROOT").unwrap());
    let dir = root.join(fixture);
    let package = paper_scene::pkg::Package::open(&dir.join("scene.pkg"))?;
    let mut model = paper_scene::model::load_from_dir(&package, &dir)?;
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let shared = crate::shared::test_device();
    let mut group = crate::app::scene::build_group(
        &shared,
        &mut model,
        true,
        &[(1920, 1080)],
        paper_geom::FillMode::Fill,
    )?;
    group.compose(1.0, 1.0 / 60.0)?;
    let (width, height, pixels) = group.read_canvas()?;
    let rendered = image::RgbaImage::from_raw(width, height, pixels).unwrap();
    rendered.save(root.join(format!("{fixture}-paper.png")))?;
    let native = image::open(root.join(format!("{fixture}-native/at12s.png")))?.to_rgb8();
    assert_eq!(rendered.dimensions(), native.dimensions());
    assert!(
        rendered
            .pixels()
            .zip(native.pixels())
            .all(|(actual, expected)| actual.0[..3] == expected.0),
        "{fixture}"
    );
    group.destroy();
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native image instance controls"]
fn multipass_instance_pixels() -> anyhow::Result<()> {
    for fixture in ["multipass_instance", "multipass_plain", "multipass_tail"] {
        compare_instance_fixture(fixture)?;
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native image instance controls"]
fn instance_scene_input_pixels() -> anyhow::Result<()> {
    for fixture in ["instance_scene_input", "scene_input_tail"] {
        compare_instance_fixture(fixture)?;
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native image instance controls"]
fn unset_material_pixels() -> anyhow::Result<()> {
    for fixture in ["unset_explicit_full", "unset_unset_method_full", "unset_unset_property_full"] {
        compare_instance_fixture(fixture)?;
    }
    Ok(())
}

#[test]
#[ignore = "requires Vulkan and native material binding controls"]
fn authored_material_pixels() -> anyhow::Result<()> {
    for fixture in ["users", "users-instanced", "users-effects", "users-effects-overrides"] {
        compare_instance_fixture(fixture)?;
    }
    Ok(())
}
