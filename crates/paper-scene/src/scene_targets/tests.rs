use super::*;

fn foreign(slot: usize, layer: &str, buffer: CompositeBuffer) -> (usize, EffectBind) {
    (slot, EffectBind::LayerComposite { layer: layer.into(), buffer })
}

fn node<'a>(
    id: &'a str,
    scene_order: usize,
    local_targets: &'a [String],
    binds: &'a [Vec<(usize, EffectBind)>],
    dynamic: bool,
) -> LayerTargetNode<'a> {
    LayerTargetNode {
        id,
        scene_order,
        local_targets,
        binds,
        dynamic,
        prefix_dynamic: false,
        passthrough: false,
    }
}

#[test]
fn foreign_reference_orders_completed_producer_before_consumer() {
    let producer_binds = [vec![]];
    let consumer_binds = [vec![foreign(2, "producer", CompositeBuffer::B)]];
    let nodes = [
        node("consumer", 0, &[], &consumer_binds, false),
        node("producer", 1, &[], &producer_binds, true),
    ];

    let plan = plan_scene_targets(&nodes, &[]).unwrap();
    assert_eq!(plan.order, [1, 0]);
    assert_eq!(plan.sampled[1], BTreeSet::from([CompositeBuffer::B]));
    assert!(plan.dynamic[0]);
    assert!(plan.retain[1]);
}

#[test]
fn full_framebuffer_waits_for_every_lower_effect_layer() {
    let plain = [vec![]];
    let full = [vec![(3, EffectBind::SceneSoFar)]];
    let nodes = [
        node("low-a", 0, &[], &plain, false),
        node("low-b", 2, &[], &plain, true),
        node("consumer", 3, &[], &full, false),
        node("higher", 4, &[], &plain, false),
    ];

    let plan = plan_scene_targets(&nodes, &[]).unwrap();
    assert_eq!(plan.order, [0, 1, 2, 3]);
    assert!(plan.snapshots[2]);
    assert!(plan.dynamic[2]);
    assert_eq!(plan.shadow_targets, 1);
    assert!(plan.dependencies.contains(&Dependency {
        producer: 0,
        consumer: 2,
        kind: DependencyKind::ScenePrefix,
    }));
    assert!(plan.dependencies.contains(&Dependency {
        producer: 1,
        consumer: 2,
        kind: DependencyKind::ScenePrefix,
    }));
    assert!(!plan.dependencies.iter().any(|edge| edge.producer == 3));
}

#[test]
fn all_scene_reads_share_one_bounded_shadow_target() {
    let full_a = [vec![(1, EffectBind::SceneSoFar)]];
    let full_b = [vec![(2, EffectBind::SceneSoFar)]];
    let nodes = [node("first", 0, &[], &full_a, false), node("second", 1, &[], &full_b, false)];

    let plan = plan_scene_targets(&nodes, &[]).unwrap();
    assert_eq!(plan.shadow_targets, 1);
    assert_eq!(plan.snapshots, [true, true]);
}

#[test]
fn scene_prefix_and_reverse_foreign_reference_cycle_is_rejected() {
    let lower = [vec![foreign(1, "upper", CompositeBuffer::A)]];
    let upper = [vec![(1, EffectBind::SceneSoFar)]];
    let nodes = [node("lower", 0, &[], &lower, false), node("upper", 1, &[], &upper, false)];

    assert_eq!(
        plan_scene_targets(&nodes, &[]),
        Err(SceneTargetPlanError::Cycle { layers: vec![0, 1] })
    );
}

#[test]
fn missing_and_ambiguous_targets_are_explicit() {
    let missing = [vec![foreign(1, "gone", CompositeBuffer::A)]];
    let nodes = [node("consumer", 0, &[], &missing, false)];
    assert_eq!(
        plan_scene_targets(&nodes, &[]),
        Err(SceneTargetPlanError::MissingLayer { consumer: 0, layer: "gone".into() })
    );

    let duplicate_binds = [vec![]];
    let consumer = [vec![foreign(1, "same", CompositeBuffer::A)]];
    let nodes = [
        node("same", 0, &[], &duplicate_binds, false),
        node("same", 1, &[], &duplicate_binds, false),
        node("consumer", 2, &[], &consumer, false),
    ];
    assert_eq!(
        plan_scene_targets(&nodes, &[]),
        Err(SceneTargetPlanError::AmbiguousLayer { consumer: 2, layer: "same".into() })
    );
}

#[test]
fn undeclared_local_target_is_not_silently_sampled() {
    let binds = [vec![(1, EffectBind::Named("_rt_missing".into()))]];
    let nodes = [node("layer", 0, &[], &binds, false)];

    assert_eq!(
        plan_scene_targets(&nodes, &[]),
        Err(SceneTargetPlanError::MissingLocalTarget { consumer: 0, target: "_rt_missing".into() })
    );
}

#[test]
fn uniquified_local_target_resolves_only_with_numeric_suffix() {
    let local = ["_rt_FullCompoBuffer1".to_string()];
    let binds = [vec![(1, EffectBind::Named("_rt_FullCompoBuffer1_19_41".into()))]];
    let nodes = [node("19", 0, &local, &binds, false)];

    assert!(plan_scene_targets(&nodes, &[]).is_ok());

    let invalid = [vec![(1, EffectBind::Named("_rt_FullCompoBuffer1_custom".into()))]];
    let nodes = [node("19", 0, &local, &invalid, false)];
    assert_eq!(
        plan_scene_targets(&nodes, &[]),
        Err(SceneTargetPlanError::MissingLocalTarget {
            consumer: 0,
            target: "_rt_FullCompoBuffer1_custom".into(),
        })
    );
}

#[test]
fn non_effect_prefix_animation_keeps_scene_snapshot_consumer_live() {
    let full = [vec![(1, EffectBind::SceneSoFar)]];
    let mut consumer = node("consumer", 2, &[], &full, false);
    consumer.prefix_dynamic = true;

    let plan = plan_scene_targets(&[consumer], &[]).unwrap();
    assert!(plan.dynamic[0]);
}

#[test]
fn passive_image_layer_is_a_valid_completed_target() {
    let binds = [vec![foreign(1, "plain", CompositeBuffer::A)]];
    let nodes = [node("consumer", 1, &[], &binds, false)];
    let passive = [PassiveLayerTarget { id: "plain", dynamic: true }];

    let plan = plan_scene_targets(&nodes, &passive).unwrap();
    assert_eq!(plan.order, [0]);
    assert!(plan.dependencies.is_empty());
    assert!(plan.dynamic[0]);
}

#[test]
fn passthrough_layer_snapshots_the_scene_prefix() {
    let binds: [Vec<(usize, EffectBind)>; 1] = [vec![]];
    let mut passthrough = node("compose", 1, &[], &binds, false);
    passthrough.passthrough = true;
    let nodes = [node("bg", 0, &[], &binds, false), passthrough];

    let plan = plan_scene_targets(&nodes, &[]).unwrap();

    assert_eq!(plan.snapshots, [false, true]);
    assert_eq!(plan.shadow_targets, 1);
    assert_eq!(
        plan.dependencies,
        [Dependency { producer: 0, consumer: 1, kind: DependencyKind::ScenePrefix }]
    );
    assert_eq!(plan.order, [0, 1]);
}

#[test]
fn scene_under_layer_binds_snapshot_the_prefix() {
    let binds: [Vec<(usize, EffectBind)>; 1] = [vec![(4, EffectBind::SceneUnderLayer)]];
    let none: [Vec<(usize, EffectBind)>; 1] = [vec![]];
    let nodes = [node("bg", 0, &[], &none, false), node("blend", 1, &[], &binds, false)];

    let plan = plan_scene_targets(&nodes, &[]).unwrap();

    assert_eq!(plan.snapshots, [false, true]);
    assert_eq!(plan.shadow_targets, 1);
}
