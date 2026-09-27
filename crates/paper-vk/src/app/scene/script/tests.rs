use super::effect_objects;
use serde_json::json;

#[test]
fn effect_objects_preserve_first_match_and_refresh_after_scene_changes() {
    let mut scene = json!({"objects":[
        {"id":1,"alpha":0.25},
        {"id":1,"alpha":0.75},
        {"id":"quoted\"id","alpha":0.5}
    ]});
    let objects = effect_objects(&scene);
    assert_eq!(objects["1"]["alpha"], 0.25);
    assert_eq!(objects[r#"quoted\"id"#]["alpha"], 0.5);
    drop(objects);
    scene["objects"][0]["id"] = json!(2);
    scene["objects"][0]["alpha"] = json!(1);
    scene["objects"].as_array_mut().unwrap().swap(0, 1);
    let objects = effect_objects(&scene);
    assert_eq!(objects["1"]["alpha"], 0.75);
    assert_eq!(objects["2"]["alpha"], 1);
}
