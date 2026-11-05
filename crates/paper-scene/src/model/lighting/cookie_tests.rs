use super::*;

fn spot(id: &str, cookie: &str, visible: bool) -> Light {
    Light {
        id: id.into(),
        origin: [0.0; 3],
        forward: [1.0, 0.0, 0.0],
        right: [0.0, 1.0, 0.0],
        up: [0.0, 0.0, 1.0],
        shadow_axes: super::super::shadow::spot_axes(&crate::matrix::model_matrix(
            [0.0; 3], [1.0; 3], [0.0; 3],
        )),
        color: [1.0; 3],
        radius: 1.0,
        source_size: 0.0,
        exponent: 1.0,
        sun: None,
        cone: Some([1.0; 2]),
        shadow: false,
        cookie: Some(cookie.into()),
        visible,
        volume: None,
    }
}

fn package(files: &[(&str, &[u8])]) -> Package {
    Package::parse(crate::tests::build_pkg(files)).unwrap()
}

fn tile(color: [u8; CHANNELS]) -> Vec<u8> {
    crate::tests::build_tex("TEXB0002", 0, 0, &color.repeat(16), false, None)
}

#[test]
fn cookies_pack_and_reuse() {
    let red = tile([255, 0, 0, 255]);
    let green = tile([0, 255, 0, 255]);
    let pkg = package(&[("materials/red.tex", &red), ("materials/green.tex", &green)]);
    let assets = Assets::discover(Some(""));
    let lights =
        [spot("red", "red", true), spot("green", "green", true), spot("same", "red", true)];
    let cookies = load(&pkg, &assets, &lights).unwrap().unwrap();
    let uniforms = cookies.layout.uniforms(&lights);
    let uv = &uniforms[UNIFORM];
    assert_eq!(&uv[..CHANNELS], &uv[CHANNELS * 2..CHANNELS * 3]);
    assert_eq!(&uv[..CHANNELS], &uv[CHANNELS..CHANNELS * 2]);
    assert!(cookies.layout.packed());
    let pixels = &cookies.texture.pixels.levels[0].data;
    let sample = |slot: usize| {
        let offset = slot * CHANNELS;
        let x = ((uv[offset] + uv[offset + 2] * 0.5) * cookies.texture.width as f32) as usize;
        let y = ((uv[offset + 1] + uv[offset + 3] * 0.5) * cookies.texture.height as f32) as usize;
        let index = (y * cookies.texture.width as usize + x) * CHANNELS;
        &pixels[index..index + CHANNELS]
    };
    assert_eq!(sample(0), &[255, 0, 0, 255]);
    assert_eq!(sample(1), &[255, 0, 0, 255]);
    assert_eq!(sample(2), sample(0));
    let mut switched = lights.clone();
    switched[2].visible = false;
    let changed = cookies.layout.uniforms(&switched);
    assert_ne!(&changed[UNIFORM][..CHANNELS], &uv[..CHANNELS]);
    assert_eq!(&changed[UNIFORM][..CHANNELS], &changed[UNIFORM][CHANNELS..CHANNELS * 2]);
}

#[test]
fn cookies_fallback_and_visibility() {
    let white = tile([255; CHANNELS]);
    let pkg = package(&[("materials/cookie/flashlight1.tex", &white)]);
    let assets = Assets::discover(Some(""));
    let lights = [spot("hidden", "missing", false), spot("shown", "missing", true)];
    let cookies = load(&pkg, &assets, &lights).unwrap().unwrap();
    assert_eq!(cookies.texture.width, 4);
    assert!(!cookies.layout.packed());
    let values = &cookies.layout.uniforms(&lights)[UNIFORM];
    assert!(values[2] > 0.0);
    assert_eq!(&values[CHANNELS..], &[0.0; CHANNELS]);
    let mut disabled = lights.clone();
    disabled[1].cookie = None;
    assert_eq!(cookies.layout.uniforms(&disabled)[UNIFORM], [0.0; CHANNELS * 2]);
    let empty = tempfile::tempdir().unwrap();
    std::fs::create_dir(empty.path().join("shaders")).unwrap();
    let isolated = Assets::discover(empty.path().to_str());
    assert!(load(&package(&[]), &isolated, &lights).is_err());
}

#[test]
fn cookies_compact_visible_spots() {
    let white = tile([255; CHANNELS]);
    let pkg = package(&[("materials/cookie/flashlight1.tex", &white)]);
    let assets = Assets::discover(Some(""));
    let mut lights = vec![spot("hidden", FALLBACK, false)];
    lights.extend((0..CAPACITY).map(|index| spot(&index.to_string(), FALLBACK, true)));
    let cookies = load(&pkg, &assets, &lights).unwrap().unwrap();
    let values = &cookies.layout.uniforms(&lights)[UNIFORM];
    assert_eq!(values.len(), CAPACITY * CHANNELS);
    assert!(values[(CAPACITY - 1) * CHANNELS + 2] > 0.0);
}

#[test]
fn cookies_reject_oversize() {
    let mut tiles = [Tile { width: MAX_EDGE, height: 1, pixels: Vec::new(), x: 0, y: 0 }];
    assert!(pack(&mut tiles).is_err());
}

#[test]
fn compressed_cookie_pixels() {
    let encoded =
        crate::tests::build_tex("TEXB0002", tex::TexFormat::Dxt1.raw(), 0, &[0; 8], false, None);
    let pkg = package(&[("materials/compressed.tex", &encoded)]);
    let assets = Assets::discover(Some(""));
    let original = load_texture_or_asset(&pkg, &assets, "compressed").unwrap();
    let cookies = load(&pkg, &assets, &[spot("one", "compressed", true)]).unwrap().unwrap();
    assert!(!cookies.layout.packed());
    assert_eq!(cookies.texture.pixels.format, original.pixels.format);
    assert_eq!(cookies.texture.pixels.levels[0].data, original.pixels.levels[0].data);
}
