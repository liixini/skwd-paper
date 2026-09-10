use super::*;

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

fn named(slot: usize, name: &str) -> (usize, EffectBind) {
    (slot, EffectBind::Named(name.to_string()))
}

#[test]
fn ping_pong_scratch_lifetime() {
    let plan = plan_targets(&[], &[None, None], &[vec![], vec![]], &[0, 0]).unwrap();

    assert_eq!(
        plan.passes,
        vec![
            PassAccess { reads: vec![Target::Pong], write: Target::Ping },
            PassAccess { reads: vec![Target::Ping], write: Target::Pong },
        ]
    );
    assert_eq!(plan.output, Target::Pong);
    assert!(plan.lifetimes[Target::Pong.index()].final_output);
    assert!(!plan.lifetimes[Target::Pong.index()].scratch_eligible());
    assert!(plan.lifetimes[Target::Ping.index()].scratch_eligible());
}

#[test]
fn named_fbo_scratch() {
    let plan = plan_targets(
        &strings(&["blur"]),
        &[Some("blur".into()), None],
        &[vec![], vec![named(1, "blur")]],
        &[0, 0],
    )
    .unwrap();

    let blur = plan.lifetimes[Target::Fbo(0).index()];
    assert_eq!(blur.first_write, Some(1));
    assert_eq!(blur.first_read, Some(2));
    assert!(!blur.loop_carried);
    assert!(blur.scratch_eligible());
    assert_eq!(plan.output, Target::Ping);
}

#[test]
fn wallpaper_engine_numeric_suffix_resolves_declared_fbo() {
    let names = strings(&["_rt_FullCompoBuffer1", "_rt_FullCompoBuffer1_19"]);

    assert_eq!(
        resolve_fbo_index(names.iter().map(String::as_str), "_rt_FullCompoBuffer1_19_41"),
        Some(1)
    );
    assert_eq!(
        resolve_fbo_index(names.iter().map(String::as_str), "_rt_FullCompoBuffer1_custom"),
        None
    );
}

#[test]
fn exact_fbo_name_wins_over_numeric_suffix_compatibility() {
    let names = strings(&["history", "history_7"]);

    assert_eq!(resolve_fbo_index(names.iter().map(String::as_str), "history_7"), Some(1));
}

#[test]
fn named_fbo_loop_carried() {
    let plan = plan_targets(
        &strings(&["history"]),
        &[None, Some("history".into())],
        &[vec![named(1, "history")], vec![]],
        &[0, 0],
    )
    .unwrap();

    let history = plan.lifetimes[Target::Fbo(0).index()];
    assert_eq!(history.first_read, Some(1));
    assert_eq!(history.first_write, Some(2));
    assert!(history.loop_carried);
    assert!(!history.scratch_eligible());
}

#[test]
fn previous_binds_prior_output() {
    let plan = plan_targets(
        &strings(&["first", "second"]),
        &[Some("first".into()), Some("second".into())],
        &[vec![], vec![(3, EffectBind::Previous)]],
        &[0, 1],
    )
    .unwrap();

    assert_eq!(plan.passes[0].write, Target::Fbo(0));
    assert!(plan.passes[1].reads.contains(&Target::Fbo(0)));
    assert_eq!(plan.passes[1].write, Target::Fbo(1));
    assert_eq!(plan.output, Target::Fbo(1));
}

#[test]
fn slot_zero_replacement() {
    let plan =
        plan_targets(&strings(&["replacement"]), &[None], &[vec![named(0, "replacement")]], &[0])
            .unwrap();

    assert_eq!(plan.passes[0].reads, vec![Target::Fbo(0)]);
    assert_eq!(plan.passes[0].write, Target::Ping);
}

#[test]
fn named_fbo_feedback_rejected() {
    let error = plan_targets(
        &strings(&["history"]),
        &[Some("history".into())],
        &[vec![named(1, "history")]],
        &[0],
    )
    .unwrap_err();

    assert_eq!(error, TargetPlanError::ReadWriteAlias { pass: 0, target: Target::Fbo(0) });
}

#[test]
fn ping_pong_feedback_rejected() {
    let error = plan_targets(
        &[],
        &[None, None, None],
        &[vec![], vec![], vec![(1, EffectBind::Previous)]],
        &[0, 1, 1],
    )
    .unwrap_err();

    assert_eq!(error, TargetPlanError::ReadWriteAlias { pass: 2, target: Target::Pong });
}

#[test]
fn metadata_length_mismatch() {
    assert_eq!(
        plan_targets(&[], &[None], &[], &[0]),
        Err(TargetPlanError::MetadataLength { targets: 1, binds: 0, owners: 1 })
    );
}

#[test]
fn fbo_names_tolerate_layer_uniquifier_suffixes() {
    let names = ["_rt_FullCompoBuffer1", "_rt_FullCompoBuffer2"];
    assert_eq!(resolve_fbo_index(names, "_rt_FullCompoBuffer1_fullscreen_90"), Some(0));
    assert_eq!(resolve_fbo_index(names, "_rt_FullCompoBuffer2_12"), Some(1));
    assert_eq!(resolve_fbo_index(names, "_rt_FullCompoBuffer1_fullscreen"), None);
    assert_eq!(resolve_fbo_index(names, "_rt_FullCompoBuffer12"), None);
}

#[test]
fn swap_marks_both_physical_buffers_loop_carried_and_remaps_later_reads() {
    let names = strings(&["_rt_V1", "_rt_V2"]);
    let plan = plan_targets_with(
        &names,
        &[None, None],
        &[(Some(0), 0, 1)],
        &[Some("_rt_V2".into()), None],
        &[vec![named(1, "_rt_V1")], vec![named(1, "_rt_V1")]],
        &[0, 0],
    )
    .unwrap();
    assert_eq!(plan.passes[0].write, Target::Fbo(1));
    assert!(plan.passes[0].reads.contains(&Target::Fbo(0)));
    assert!(plan.passes[1].reads.contains(&Target::Fbo(1)), "{:?}", plan.passes[1]);
    assert!(plan.lifetimes[Target::Fbo(0).index()].loop_carried);
    assert!(plan.lifetimes[Target::Fbo(1).index()].loop_carried);
    assert!(!plan.lifetimes[Target::Fbo(0).index()].scratch_eligible());
    assert!(!plan.lifetimes[Target::Fbo(1).index()].scratch_eligible());
}

#[test]
fn unique_fbos_resolve_within_their_owning_effect() {
    let names = strings(&["_rt_H", "_rt_S", "_rt_H"]);
    let owners = [Some(0), None, Some(1)];
    assert_eq!(
        resolve_fbo_scoped(names.iter().map(String::as_str), &owners, 1, "_rt_H_19_41"),
        Some(2)
    );
    assert_eq!(resolve_fbo_scoped(names.iter().map(String::as_str), &owners, 0, "_rt_H"), Some(0));
    assert_eq!(resolve_fbo_scoped(names.iter().map(String::as_str), &owners, 1, "_rt_S"), Some(1));
    assert_eq!(resolve_fbo_scoped(names.iter().map(String::as_str), &owners, 2, "_rt_H"), None);
    let plan = plan_targets_with(
        &names,
        &owners,
        &[],
        &[Some("_rt_H".into()), Some("_rt_H".into()), None],
        &[vec![], vec![named(1, "_rt_S")], vec![named(1, "_rt_H_19_41")]],
        &[0, 1, 1],
    )
    .unwrap();
    assert_eq!(plan.passes[0].write, Target::Fbo(0));
    assert_eq!(plan.passes[1].write, Target::Fbo(2));
    assert!(plan.passes[1].reads.contains(&Target::Fbo(1)));
    assert!(plan.passes[2].reads.contains(&Target::Fbo(2)), "{:?}", plan.passes[2]);
    assert!(!plan.passes[2].reads.contains(&Target::Fbo(0)), "{:?}", plan.passes[2]);
}

#[test]
fn swap_before_the_first_pass_applies_every_frame_start() {
    let names = strings(&["_rt_A", "_rt_B"]);
    let plan = plan_targets_with(
        &names,
        &[None, None],
        &[(None, 0, 1)],
        &[Some("_rt_A".into())],
        &[vec![]],
        &[0],
    )
    .unwrap();
    assert_eq!(plan.passes[0].write, Target::Fbo(1));
    assert!(plan.lifetimes[Target::Fbo(0).index()].loop_carried);
    assert!(plan.lifetimes[Target::Fbo(1).index()].loop_carried);
}
