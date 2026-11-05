use super::evaluate_scene;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/fixtures/admission").join(name)
}

#[test]
fn operator_reduction_reported() {
    let report = evaluate_scene(&fixture("unsupported-operator")).unwrap();
    assert_eq!(
        report["loader_reductions"][0],
        "Sparks: particle particles/root.json: operator 1 (unknownmotion) unsupported"
    );
    assert_eq!(report["strict_loader_rejects"], true);
}

#[test]
fn atlas_reduction_reported() {
    let report = evaluate_scene(&fixture("invalid-atlas-page")).unwrap();
    assert_eq!(report["loader_reductions"][0], "Atlas: invalid atlas frame texture sprite");
    assert_eq!(report["texture_formats"][0]["raw_id"], 0);
    assert_eq!(report["texture_formats"][0]["outcome"], "rejected: invalid atlas frame");
}

#[test]
fn texture_format_rejected() {
    let report = evaluate_scene(&fixture("unknown-format")).unwrap();
    assert_eq!(report["texture_formats"][0]["raw_id"], 99);
    assert_eq!(report["texture_formats"][0]["outcome"], "rejected: undecodable");
    assert_eq!(report["capability_gaps"][0], "unknown-texture-formats");
}

#[test]
fn free_image_override_decodes() {
    let report = evaluate_scene(&fixture("free-image-valid")).unwrap();
    assert_eq!(report["texture_formats"][0]["raw_id"], 99);
    assert_eq!(report["texture_formats"][0]["effective_route"], "free-image");
    assert_eq!(report["texture_formats"][0]["outcome"], "decoded");
    assert_eq!(report["strict_loader_rejects"], false);
    assert_eq!(report["capability_gaps"], serde_json::json!([]));
}

#[test]
fn corrupt_free_image_rejected() {
    let report = evaluate_scene(&fixture("free-image-corrupt")).unwrap();
    assert_eq!(report["texture_formats"][0]["raw_id"], 99);
    assert_eq!(report["texture_formats"][0]["effective_route"], "free-image");
    assert_eq!(report["texture_formats"][0]["outcome"], "rejected: undecodable");
    assert_eq!(report["strict_loader_rejects"], true);
}

#[test]
fn rotated_atlas_decodes() {
    let report = evaluate_scene(&fixture("rotated-atlas")).unwrap();
    assert_eq!(report["strict_loader_rejects"], false);
    assert!(
        report["texture_formats"].as_array().unwrap().iter().all(|row| row["outcome"] == "decoded")
    );
}

#[test]
fn budget_requires_renderer() {
    let report = evaluate_scene(&fixture("effect-budget")).unwrap();
    assert_eq!(report["effect_budget"]["status"], "runtime-check-required");
    assert_eq!(report["effect_budget"]["outcome"], "unmeasured");
    assert_eq!(report["effect_budget"]["reason"], "effect-chain-budget");
}

#[test]
fn decode_failure_precedes_atlas() {
    let mut bytes = std::fs::read(fixture("invalid-atlas-page").join("scene.pkg")).unwrap();
    let tag = b"TEXI0001\0";
    let offset = bytes.windows(tag.len()).position(|window| window == tag).unwrap() + tag.len();
    bytes[offset..offset + 4].copy_from_slice(&99_i32.to_le_bytes());
    assert_eq!(
        crate::model::load_texture_bytes(
            &bytes[bytes.windows(9).position(|window| window == b"TEXV0005\0").unwrap()..]
        )
        .err(),
        Some("undecodable")
    );
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("scene.pkg"), bytes).unwrap();
    let report = evaluate_scene(directory.path()).unwrap();
    assert_eq!(report["texture_formats"][0]["outcome"], "rejected: undecodable");
    assert_eq!(report["loader_reductions"][0], "Atlas: undecodable texture sprite");
}

#[test]
fn preset_resolves_parent() {
    let library = tempfile::tempdir().unwrap();
    let parent = library.path().join("1");
    let preset = library.path().join("2");
    std::fs::create_dir(&parent).unwrap();
    std::fs::create_dir(&preset).unwrap();
    std::fs::copy(fixture("free-image-valid").join("scene.pkg"), parent.join("scene.pkg")).unwrap();
    std::fs::write(parent.join("project.json"), r#"{"type":"scene","file":"scene.json"}"#).unwrap();
    std::fs::write(preset.join("project.json"), r#"{"dependency":"1","preset":{}}"#).unwrap();
    let report = evaluate_scene(&preset).unwrap();
    assert_eq!(report["package"], parent.join("scene.pkg").to_str().unwrap());
    assert_eq!(report["texture_formats"][0]["outcome"], "decoded");
}

#[test]
fn unrelated_package_rejected() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::copy(
        fixture("free-image-valid").join("scene.pkg"),
        directory.path().join("other.pkg"),
    )
    .unwrap();
    assert!(evaluate_scene(directory.path()).is_err());
}

#[test]
fn packaged_video_runtime() {
    use crate::model::tests::{clip_tex, oversized_tex, packaged_video};
    let length = crate::tex::MAX_TEX_PAYLOAD_BYTES as u64 + 1;
    let directory = tempfile::tempdir().unwrap();
    for (tex, extra) in
        [(clip_tex(crate::tex::FLAG_IS_VIDEO, false), 0), (oversized_tex(length), length)]
    {
        let pkg = packaged_video(directory.path(), &tex, extra);
        let outcome = super::texture_outcome(&pkg, &pkg.entries()[0]).outcome;
        assert_eq!(outcome, super::OUTCOME_RUNTIME_VIDEO);
    }
}
