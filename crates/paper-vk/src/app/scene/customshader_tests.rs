use super::effect_lifetime_tests::{center, package_files, texture_levels};
use super::*;
use paper_scene::model::Properties;

#[test]
#[ignore = "requires Vulkan"]
fn customshader_swap_rollback() {
    let texture =
        texture_levels(&[paper_scene::tex::MipLevel { width: 1, height: 1, data: vec![255; 4] }]);
    let vertex = b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}";
    let package = package_files(&[
        ("scene.json", br#"{"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},"objects":[{"id":1,"image":"models/custom.json","origin":"32 32 0","size":"64 64"}]}"#),
        ("models/custom.json", br#"{"material":"materials/custom.json"}"#),
        ("materials/probe.tex", &texture),
        ("materials/custom.json", br#"{"passes":[{"shader":{"value":"red","user":"style"},"textures":["probe"]}]}"#),
        ("shaders/red.vert", vertex),
        ("shaders/green.vert", vertex),
        ("shaders/broken.vert", vertex),
        ("shaders/red.frag", b"void main(){gl_FragColor=vec4(1.0,0.0,0.0,1.0);}"),
        ("shaders/green.frag", b"void main(){gl_FragColor=vec4(0.0,1.0,0.0,1.0);}"),
        ("shaders/broken.frag", b"void main(){ invalid syntax }"),
    ]);
    let project = serde_json::json!({"general":{"properties":{"style":{"type":"customshader","value":0,
        "options":[{"value":0,"shader":"red"},{"value":1,"shader":"green"},{"value":2,"shader":"broken"}]}}}});
    let shared = crate::shared::test_device();
    let baseline =
        (vk::Renderer::scene_target_live_allocations(), vk::Renderer::effect_live_allocations());
    let candidate = |selection| {
        let assets = paper_scene::effects::Assets::discover(Some("/nonexistent"))
            .with_properties(Properties::from([("style".into(), vec![selection])]))
            .with_custom_shaders(&project)?;
        let mut model = paper_scene::model::load_with(&package, &assets)?;
        build_group(&shared, &mut model, false, &[(64, 64)], FillMode::Fit)
    };
    let mut group = candidate(0.0).unwrap();
    assert_eq!(center(&mut group), [255, 0, 0, 255]);
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("project.json"), project.to_string()).unwrap();
    std::fs::write(root.path().join("scene.pkg"), b"fixture").unwrap();
    let directory = root.path().to_str().unwrap();
    let mut source = properties::PropertySource::new(directory, &Properties::new());
    let mut ctl = crate::ctl::Ctl::start_opts("", true, 100, false, false);
    let unchanged = Properties::from([("style".into(), vec![0.0])]);
    let allocations =
        (vk::Renderer::scene_target_live_allocations(), vk::Renderer::effect_live_allocations());
    assert!(group.update_properties(&mut source, directory, &unchanged, &mut ctl));
    assert_eq!(center(&mut group), [255, 0, 0, 255]);
    assert_eq!(
        (vk::Renderer::scene_target_live_allocations(), vk::Renderer::effect_live_allocations()),
        allocations
    );

    for (selection, expected) in [(1.0, [0, 255, 0, 255]), (0.0, [255, 0, 0, 255])] {
        let live = (
            vk::Renderer::scene_target_live_allocations(),
            vk::Renderer::effect_live_allocations(),
        );
        assert!(candidate(2.0).is_err());
        assert!(candidate(9.0).is_err());
        assert_eq!(
            (
                vk::Renderer::scene_target_live_allocations(),
                vk::Renderer::effect_live_allocations()
            ),
            live
        );
        let old_pixels = center(&mut group);
        assert!(old_pixels == [255, 0, 0, 255] || old_pixels == [0, 255, 0, 255]);
        let next = candidate(selection).unwrap();
        let old = std::mem::replace(&mut group, next);
        old.destroy();
        assert_eq!(center(&mut group), expected);
        assert_eq!(
            (
                vk::Renderer::scene_target_live_allocations(),
                vk::Renderer::effect_live_allocations()
            ),
            live
        );
    }
    group.destroy();
    assert_eq!(
        (vk::Renderer::scene_target_live_allocations(), vk::Renderer::effect_live_allocations()),
        baseline
    );
}

#[test]
fn customshader_update_routing() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("scene.pkg"), b"fixture").unwrap();
    std::fs::write(root.path().join("project.json"), br#"{"general":{"properties":{"style":{"type":"customshader","value":0,"options":[{"value":0,"shader":"red"},{"value":1,"shader":"green"}]}}}}"#).unwrap();
    let directory = root.path().to_str().unwrap();
    let source = properties::PropertySource::new(directory, &Properties::new());
    use paper_scene::effects::customshader::Change;
    assert!(
        source.change(directory, &Properties::from([("style".into(), vec![0.0])]))
            == Change::Unchanged
    );
    assert!(
        source.change(directory, &Properties::from([("style".into(), vec![1.0])]))
            == Change::Reload
    );
}
