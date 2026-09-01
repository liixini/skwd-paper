#[test]
fn fill_mode_reexport() {
    fn accepts_geometry_mode(_: paper_geom::FillMode) {}

    let default = paper_control::FillMode::default();
    accepts_geometry_mode(default);
    assert_eq!(default, paper_control::FillMode::Fill);
    assert_eq!(serde_json::to_string(&paper_control::FillMode::Fit).unwrap(), "\"fit\"");
    assert_eq!(
        serde_json::from_str::<paper_control::FillMode>("\"span\"").unwrap(),
        paper_control::FillMode::Span
    );
}
