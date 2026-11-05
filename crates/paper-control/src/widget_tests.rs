use super::*;

const WIRE: &str = r#"{"widgets":[{"face":{"kind":"clock","style":"aperture-ring"},"depth":"front","pin":{"anchor":[0.5,0.25],"offset":[-12.0,8.0]}}],"outlines":["/cache/drawing/manifest.json"],"palette":{"text":[236,235,231],"accent":[90,110,200],"shade":[18,19,22]},"font":"/usr/share/skwd/RobotoCondensed-Bold.ttf","labels":{"days":["MON","TUE","WED","THU","FRI","SAT","SUN"],"day_names":["MONDAY","TUESDAY","WEDNESDAY","THURSDAY","FRIDAY","SATURDAY","SUNDAY"],"months":["JAN","FEB","MAR","APR","MAY","JUN","JUL","AUG","SEP","OCT","NOV","DEC"],"month_names":["JANUARY","FEBRUARY","MARCH","APRIL","MAY","JUNE","JULY","AUGUST","SEPTEMBER","OCTOBER","NOVEMBER","DECEMBER"]}}"#;

fn scene() -> WidgetScene {
    serde_json::from_str(WIRE).unwrap()
}

#[test]
fn wire_round_trip() {
    let scene = scene();
    assert_eq!(scene.widgets[0].face, Face::Clock(ClockStyle::ApertureRing));
    assert_eq!(scene.widgets[0].depth, Depth::Front);
    assert_eq!(serde_json::to_string(&scene).unwrap(), WIRE);
}

#[test]
fn depth_defaults_behind() {
    let wire = WIRE.replace(r#""depth":"front","#, "");
    let scene: WidgetScene = serde_json::from_str(&wire).unwrap();
    assert_eq!(scene.widgets[0].depth, Depth::Behind);
}

#[test]
fn rejects_bad_pins() {
    let mut scene = scene();
    scene.widgets[0].pin.anchor = [1.5, 0.0];
    assert_eq!(scene.validate(), Err(ValidationError::InvalidWidgetPin));
    scene.widgets[0].pin = Pin { anchor: [0.0, 0.0], offset: [f32::NAN, 0.0] };
    assert_eq!(scene.validate(), Err(ValidationError::InvalidWidgetPin));
}

#[test]
fn rejects_relative_paths() {
    let mut scene = scene();
    scene.font = Some(PathBuf::from("fonts/a.ttf"));
    assert_eq!(scene.validate(), Err(ValidationError::RelativeWidgetPath));
}

#[test]
fn caps_widget_count() {
    let mut scene = scene();
    scene.widgets = vec![scene.widgets[0].clone(); MAX_WIDGETS + 1];
    assert_eq!(scene.validate(), Err(ValidationError::TooManyWidgets(MAX_WIDGETS + 1)));
}

#[test]
fn caps_outline_count() {
    let mut scene = scene();
    scene.outlines = vec![PathBuf::from("/cache/manifest.json"); MAX_OUTLINES + 1];
    assert_eq!(scene.validate(), Err(ValidationError::TooManyOutlines(MAX_OUTLINES + 1)));
}

#[test]
fn height_is_optional() {
    assert_eq!(scene().widgets[0].height, None);
    let wire = WIRE.replace(r#""depth":"front","#, r#""depth":"front","height":0.6,"#);
    let mut scene: WidgetScene = serde_json::from_str(&wire).unwrap();
    assert_eq!(scene.widgets[0].height, Some(0.6));
    assert_eq!(scene.validate(), Ok(()));
    scene.widgets[0].height = Some(1.5);
    assert_eq!(scene.validate(), Err(ValidationError::InvalidWidgetHeight));
}

#[test]
fn font_and_output_optional() {
    let wire = WIRE.replace(r#""font":"/usr/share/skwd/RobotoCondensed-Bold.ttf","#, "");
    let mut scene: WidgetScene = serde_json::from_str(&wire).unwrap();
    assert_eq!(scene.font, None);
    assert_eq!(scene.widgets[0].output, None);
    scene.widgets[0].output = Some("DP-1".into());
    let back: WidgetScene = serde_json::from_str(&serde_json::to_string(&scene).unwrap()).unwrap();
    assert_eq!(back.widgets[0].output.as_deref(), Some("DP-1"));
}

#[test]
fn output_filter() {
    let mut widget = scene().widgets[0].clone();
    assert!(widget.shown_on("DP-1") && widget.shown_on("HDMI-A-1"));
    widget.output = Some("DP-1".into());
    assert!(widget.shown_on("DP-1"));
    assert!(!widget.shown_on("HDMI-A-1"));
}
