use super::*;
use serde_json::json;

#[test]
fn native_fog_parameters() {
    let scene = json!({"general": {
        "fogdistance": true,
        "fogdistancestart": 10,
        "fogdistanceend": 30,
        "fogdistancestartdensity": 0.2,
        "fogdistanceenddensity": 0.8,
        "fogdistancecolor": "0.2 0.8 0.4",
        "fogheight": true,
        "fogheightstart": 5,
        "fogheightend": 25,
        "fogheightstartdensity": 0.1,
        "fogheightenddensity": 0.9,
        "fogheightcolor": "0.8 0.2 0.6"
    }});
    let config = Config::load(&scene, &Properties::new());
    assert_eq!(config.uniform("g_FogDistanceParams"), Some(vec![10.0, 20.0, 0.2, 0.6]));
    assert_eq!(config.uniform("g_FogHeightParams"), Some(vec![5.0, 20.0, 0.1, 0.9_f32 - 0.1]));
    assert_eq!(config.uniform("g_FogDistanceColor"), Some(vec![0.2, 0.8, 0.4]));
    assert_eq!(config.uniform("g_FogHeightColor"), Some(vec![0.8, 0.2, 0.6]));
}

#[test]
fn native_fog_defaults() {
    let config = Config::default();
    assert_eq!(config.uniform("g_FogDistanceParams"), Some(vec![1.0, 4.0, 0.0, 1.0]));
    assert_eq!(config.uniform("g_FogHeightParams"), Some(vec![1.0, -4.0, 0.0, 1.0]));
    assert_eq!(config.uniform("g_FogDistanceColor"), Some(vec![0.0; 3]));
    assert_eq!(config.uniform("g_FogHeightColor"), Some(vec![0.0; 3]));
}

#[test]
fn material_fog_switches() {
    let scene = json!({"general": {"fogdistance":true, "fogheight":true}});
    let config = Config::load(&scene, &Properties::new());
    for (material, enabled) in [(0, 0), (1, 1)] {
        let mut combos = BTreeMap::from([("FOG".into(), material)]);
        config.combos(&mut combos);
        assert_eq!(combos["FOG_DIST"], enabled);
        assert_eq!(combos["FOG_HEIGHT"], enabled);
    }
    let mut combos = BTreeMap::from([("FOG".into(), 1)]);
    Config::default().combos(&mut combos);
    assert_eq!(combos["FOG_DIST"], 0);
    assert_eq!(combos["FOG_HEIGHT"], 0);
}
