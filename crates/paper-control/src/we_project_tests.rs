use super::*;
use serde_json::json;

fn item(root: &Path, id: &str, value: &Value) -> PathBuf {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("project.json"), value.to_string()).unwrap();
    dir
}

#[test]
fn chained_presets_inherit_declarations_and_override_in_order() {
    let root = tempfile::tempdir().unwrap();
    let base = item(
        root.path(),
        "1",
        &json!({"type":"scene","general":{"properties":{"rain":{"type":"bool","value":true,"text":"Rain"},"clock":{"type":"bool","value":true}}}}),
    );
    std::fs::write(base.join("scene.pkg"), b"fixture").unwrap();
    item(root.path(), "2", &json!({"dependency":"1","preset":{"rain":false,"clock":false}}));
    let preset = item(root.path(), "3", &json!({"dependency":2,"preset":{"clock":true}}));
    let resolved = Project::resolve(&preset).unwrap();
    assert_eq!(resolved.scene_package().unwrap(), base.join("scene.pkg"));
    assert_eq!(resolved.directories.len(), 3);
    let properties = resolved.declarations();
    assert_eq!(properties["rain"], json!({"type":"bool","value":false,"text":"Rain"}));
    assert_eq!(properties["clock"]["value"], true);
    assert_eq!(read(&base).unwrap()["general"]["properties"]["rain"]["value"], true);
}

#[test]
fn rejects_missing_cyclic_and_invalid_dependencies() {
    let root = tempfile::tempdir().unwrap();
    let preset = item(root.path(), "1", &json!({"dependency":"2","preset":{}}));
    assert!(Project::resolve(&preset).err().unwrap().to_string().contains("Workshop item 2"));
    item(root.path(), "2", &json!({"dependency":"1","preset":{}}));
    assert!(Project::resolve(&preset).err().unwrap().to_string().contains("cycle"));
    for invalid in
        [json!("../2"), json!("/2"), json!(""), json!(0), json!(-1), json!("18446744073709551616")]
    {
        assert!(dependency(&json!({"dependency":invalid,"preset":{}})).is_err());
    }
    assert!(dependency(&json!({"preset":{}})).is_err());
    assert!(dependency(&json!({"dependency":"1","preset":[]})).is_err());
}

#[test]
fn resolves_library_links_and_bounds_chains() {
    let root = tempfile::tempdir().unwrap();
    let external = root.path().join("external");
    let library = root.path().join("library");
    std::fs::create_dir(&library).unwrap();
    let preset = item(&external, "2", &json!({"dependency":"1","preset":{}}));
    item(&library, "1", &json!({"type":"scene"}));
    std::os::unix::fs::symlink(&preset, library.join("2")).unwrap();
    assert_eq!(Project::resolve(&library.join("2")).unwrap().source, library.join("1"));
    for id in 3..40 {
        item(&external, &id.to_string(), &json!({"dependency":(id+1).to_string(),"preset":{}}));
    }
    assert!(Project::resolve(&external.join("3")).err().unwrap().to_string().contains("too deep"));
}

#[test]
fn reads_bom_and_rejects_non_objects() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("project.json"), b"\xef\xbb\xbf{}").unwrap();
    assert_eq!(read(root.path()).unwrap(), json!({}));
    std::fs::write(root.path().join("project.json"), b"[]").unwrap();
    assert!(read(root.path()).is_err());
}
