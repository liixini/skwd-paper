use super::*;

#[test]
fn changed_inputs_rejected() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("shader.frag");
    std::fs::write(&path, b"first shader")?;
    let capture = Capture::start()?;
    read(&path)?;
    let initial = capture.finish()?;
    std::fs::write(&path, b"other shader")?;
    let capture = Capture::start()?;
    read(&path)?;
    assert!(!initial.matches(&capture.finish()?));
    Ok(())
}

#[test]
fn changed_defaults_rejected() -> Result<()> {
    let path = Path::new("dependency-project");
    let capture = Capture::start()?;
    project(path, &serde_json::json!({"general": {"properties": {"color": {"value": "1 0 0"}}}}));
    let initial = capture.finish()?;
    let capture = Capture::start()?;
    project(path, &serde_json::json!({"general": {"properties": {"color": {"value": "0 1 0"}}}}));
    assert!(!initial.matches(&capture.finish()?));
    Ok(())
}

#[test]
fn mixed_source_rejected() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("scene.pkg");
    std::fs::write(&path, b"stable package")?;
    let capture = Capture::start()?;
    file(&path, b"first package");
    file(&path, b"other package");
    assert!(capture.finish().is_err());
    let capture = Capture::start()?;
    file(&path, b"stable package");
    assert!(capture.finish().is_ok());
    Ok(())
}

#[test]
fn dependency_files_track_changes() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("project.json");
    std::fs::write(&path, b"{\"value\":1}")?;
    let directories = [directory.path().to_path_buf()];
    let capture = Capture::start()?;
    project_files(&directories)?;
    let initial = capture.finish()?;
    let capture = Capture::start()?;
    project_files(&directories)?;
    assert!(initial.matches(&capture.finish()?));
    std::fs::write(&path, b"{\"value\":2}")?;
    let capture = Capture::start()?;
    project_files(&directories)?;
    assert!(!initial.matches(&capture.finish()?));
    Ok(())
}

#[test]
fn loose_assets_track_content() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let shaders = directory.path().join("shaders");
    std::fs::create_dir(&shaders)?;
    let path = shaders.join("probe.vert");
    std::fs::write(&path, b"first shader")?;
    let assets = crate::effects::Assets::discover(directory.path().to_str());
    let capture = Capture::start()?;
    assert_eq!(assets.read("shaders/probe.vert").unwrap(), "first shader");
    let initial = capture.finish()?;
    let capture = Capture::start()?;
    assert_eq!(assets.read("shaders/probe.vert").unwrap(), "first shader");
    assert!(initial.matches(&capture.finish()?));
    std::fs::write(&path, b"other shader")?;
    let capture = Capture::start()?;
    assert_eq!(assets.read("shaders/probe.vert").unwrap(), "other shader");
    assert!(!initial.matches(&capture.finish()?));
    Ok(())
}

#[test]
#[ignore = "requires pinned SKWD_WE_RESIZE_PROJECT fixture"]
fn repeated_models_keep_identity() -> Result<()> {
    let directory = std::path::PathBuf::from(std::env::var("SKWD_WE_RESIZE_PROJECT")?);
    let load = || -> Result<Identity> {
        let capture = Capture::start()?;
        let path = paper_control::we_project::Project::scene_package_at(&directory)?;
        let package = crate::pkg::Package::open(&path)?;
        let _ = crate::model::load_from_dir(&package, &directory)?;
        capture.finish()
    };
    let initial = load()?;
    for _ in 0..3 {
        assert!(initial.matches(&load()?));
    }
    Ok(())
}

#[test]
#[ignore = "requires pinned SKWD_WE_RESIZE_PROJECT fixture"]
fn changed_package_rejected() -> Result<()> {
    let original = std::path::PathBuf::from(std::env::var("SKWD_WE_RESIZE_PROJECT")?);
    let path = paper_control::we_project::Project::scene_package_at(&original)?;
    let directory = tempfile::tempdir()?;
    let target = directory.path().join("scene.pkg");
    std::fs::copy(path, &target)?;
    let capture = Capture::start()?;
    let _ = crate::pkg::Package::open(&target)?;
    let initial = capture.finish()?;
    let mut bytes = std::fs::read(&target)?;
    bytes.push(0);
    std::fs::write(&target, bytes)?;
    let capture = Capture::start()?;
    let _ = crate::pkg::Package::open(&target)?;
    assert!(!initial.matches(&capture.finish()?));
    Ok(())
}
