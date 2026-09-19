use super::*;

struct Fixture {
    _directory: tempfile::TempDir,
    assets: Assets,
    fonts: PathBuf,
}

fn fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let steamapps = directory.path().join("steamapps");
    let root = steamapps.join("common/wallpaper_engine/assets");
    let fonts = steamapps.join("compatdata/431960/pfx/drive_c/windows/Fonts");
    std::fs::create_dir_all(root.join("shaders")).unwrap();
    std::fs::create_dir_all(root.join("fonts")).unwrap();
    std::fs::create_dir_all(&fonts).unwrap();
    Fixture { _directory: directory, assets: Assets::discover(root.to_str()), fonts }
}

fn font(family: &str, full: &str) -> Vec<u8> {
    let mut records = Vec::new();
    let mut strings = Vec::new();
    for (id, value) in [(1u16, family), (4, full)] {
        let encoded: Vec<_> = value.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for number in [3, 1, 0x0409, id, encoded.len() as u16, strings.len() as u16] {
            records.extend_from_slice(&number.to_be_bytes());
        }
        strings.extend(encoded);
    }
    let mut table = Vec::new();
    for number in [0u16, 2, 30] {
        table.extend_from_slice(&number.to_be_bytes());
    }
    table.extend(records);
    table.extend(strings);
    let mut bytes = vec![0, 1, 0, 0, 0, 1, 0, 16, 0, 0, 0, 0];
    bytes.extend_from_slice(b"name");
    for number in [0u32, 28, table.len() as u32] {
        bytes.extend_from_slice(&number.to_be_bytes());
    }
    bytes.extend(table);
    bytes
}

#[test]
fn exact_prefix_family_and_full_name_keep_the_selected_font() {
    let fixture = fixture();
    let requested = font("Skwd Fixture", "Skwd Fixture Regular");
    let fallback = font("Arial", "Arial");
    std::fs::write(fixture.fonts.join("requested.ttf"), &requested).unwrap();
    std::fs::write(fixture.fonts.join("arial.ttf"), &fallback).unwrap();
    for family in ["skwd fixture", "SkwdFixtureRegular"] {
        assert_eq!(Cache::default().resolve(&fixture.assets, family), Some(requested.clone()));
    }
    assert_eq!(
        Cache::default().resolve(&fixture.assets, "skwd_missing_font_family"),
        Some(fallback)
    );
}

#[test]
fn cached_requests_share_the_assets_lifetime_and_new_assets_refresh_fonts() {
    let fixture = fixture();
    let requested = font("Skwd Fixture", "Skwd Fixture");
    let fallback = font("Noto Fixture", "Noto Fixture");
    std::fs::write(fixture.fonts.join("requested.ttf"), &requested).unwrap();
    std::fs::write(fixture.assets.root().unwrap().join(super::super::FALLBACK_FONT), &fallback)
        .unwrap();
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[])).unwrap();
    assert_eq!(
        super::super::font_bytes(&package, &fixture.assets, "systemfont_Skwd Fixture"),
        Some(requested.clone())
    );
    std::fs::remove_file(fixture.fonts.join("requested.ttf")).unwrap();
    assert_eq!(
        super::super::font_bytes(&package, &fixture.assets, "systemfont_skwdfixture"),
        Some(requested)
    );
    let refreshed = Assets::discover(fixture.assets.root().unwrap().to_str());
    assert_eq!(
        super::super::font_bytes(&package, &refreshed, "systemfont_Skwd Fixture"),
        Some(fallback)
    );
}

#[test]
fn packaged_fonts_remain_authoritative_and_fontconfig_substitutions_are_rejected() {
    let fixture = fixture();
    let embedded = font("Embedded", "Embedded");
    let fallback = font("Arial", "Arial");
    std::fs::write(fixture.fonts.join("arial.ttf"), &fallback).unwrap();
    let package =
        crate::pkg::Package::parse(crate::tests::build_pkg(&[("fonts/chosen.ttf", &embedded)]))
            .unwrap();
    assert_eq!(
        super::super::font_bytes(&package, &fixture.assets, "fonts/chosen.ttf"),
        Some(embedded)
    );
    assert!(host_font("skwd_missing_font_family").is_none());
}

#[test]
#[ignore = "requires Wallpaper Engine assets, its Proton Arial font, and host Noto Sans"]
fn installed_fonts_resolve_by_real_names_and_render_with_the_prefix_fallback() {
    let installed = Assets::discover(None);
    let noto = installed.read_bytes(super::super::FALLBACK_FONT).unwrap();
    let arial = prefix_fonts(installed.root())
        .into_iter()
        .find(|font| font.names.contains("arial"))
        .and_then(|font| read(&font.path))
        .unwrap();
    let fixture = fixture();
    std::fs::write(fixture.fonts.join("noto.ttf"), &noto).unwrap();
    std::fs::write(fixture.fonts.join("arial.ttf"), &arial).unwrap();
    let mut cache = Cache::default();
    assert_eq!(cache.resolve(&fixture.assets, "Noto Sans"), Some(noto));
    let resolved = cache.resolve(&fixture.assets, "skwd_missing_font_family").unwrap();
    assert_eq!(resolved, arial);
    assert!(super::super::Face::parse(&resolved, 64.0).unwrap().measure("12:34").is_some());
    let host = host_font("Noto Sans").unwrap();
    assert!(names(&host).unwrap().0.contains("notosans"));
}
