use super::*;
use crate::model::{Attachments, Properties};
use serde_json::json;

const SUN: SunView = SunView { eye: [0.0; 3], forward: [0.0, 0.0, -1.0], flat: false };

#[test]
fn point_caster_depth() {
    for radius in [600.0, 2000.0] {
        let scene = json!({"objects":[{"id":1,"light":"lpoint","radius":radius}]});
        let lights = super::super::lights(&scene, &Properties::new(), &Attachments::new());
        let light = &lights[0];
        let receiver = light.shadow_views(ShadowView::Receiver, &SUN);
        let caster = light.shadow_views(ShadowView::Caster, &SUN);
        for (mut receiver, caster) in receiver.zip(caster) {
            assert_eq!(receiver[14], (1.0 / (radius - 1.0)) * radius);
            receiver[14] -= 0.00333;
            assert_eq!(receiver, caster);
        }
    }
}

#[test]
fn native_projection_rounding() {
    let scene = json!({"objects":[{"id":1,"light":"lpoint","radius":2000}]});
    let lights = super::super::lights(&scene, &Properties::new(), &Attachments::new());
    assert_eq!(lights[0].shadow_parameters()[1], 1.000_500_3);
}

#[test]
fn native_shadow_arrays() {
    let scene = json!({"objects":[
        {"id":1,"light":"lpoint","castshadow":true,"visible":false},
        {"id":2,"light":"lpoint","castshadow":false},
        {"id":3,"light":"lpoint","castshadow":true,"radius":600},
        {"id":4,"light":"lspot","castshadow":true,"radius":600}
    ]});
    let lights = super::super::lights(&scene, &Properties::new(), &Attachments::new());
    let layout = ShadowLayout::new(&lights).unwrap();
    let values = layout.uniforms(&lights, &SUN);
    assert_eq!(&values[POINT_PROJECTION][..4], &[1.0 / 599.0, 600.0 / 599.0, -1.0, 0.0]);
    assert_eq!(&values[POINT_PROJECTION][4..8], &[0.0; 4]);
    assert_eq!(&values[POINT_TRANSFORM][..4], &[0.5, 0.0, 0.5, 0.5]);
    assert_eq!(&values[SPOT_TRANSFORM][..4], &[0.0, 0.5, 0.5, 0.5]);
    assert!(values[SPOT_PROJECTION][..16].iter().any(|value| *value != 0.0));
    assert!(values[SPOT_PROJECTION][16..].iter().all(|value| *value == 0.0));
}

#[test]
fn spot_projection_axes() {
    for (scale, length) in [(1.0, 0.998_308_1), (3.0, 0.998_858_33)] {
        let scene = json!({"objects":[{"id":1,"light":"lspot","radius":2000,
            "castshadow":true,"angles":"0 1.5707963267948966 0",
            "scale":format!("{scale} 5 7")}]});
        let lights = super::super::lights(&scene, &Properties::new(), &Attachments::new());
        let light = &lights[0];
        let mut receiver = light.shadow_views(ShadowView::Receiver, &SUN).next().unwrap();
        assert!((receiver[11].abs() - length).abs() < f32::EPSILON);
        let layout = ShadowLayout::new(&lights).unwrap();
        assert_eq!(&layout.uniforms(&lights, &SUN)[SPOT_PROJECTION][..16], &receiver);
        receiver[14] -= 0.0005;
        assert_eq!(light.shadow_views(ShadowView::Caster, &SUN).next().unwrap(), receiver);
    }
}
