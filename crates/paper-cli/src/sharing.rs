use paper_control::{Assignment, FillMode, RendererPolicy};

use super::{ALL_OUTPUT, OUTPUT_SEPARATOR, playback::Control, static_transition};

type Group = (Assignment, String, Vec<Assignment>);

pub(super) fn remaining(worker: &crate::backend::Worker, outputs: &[String]) -> Vec<Assignment> {
    worker
        .assignments()
        .iter()
        .filter_map(|assignment| {
            let mut assignment = assignment.clone();
            assignment.outputs.retain(|output| outputs.contains(output));
            assignment.transition = None;
            (!assignment.outputs.is_empty()).then_some(assignment)
        })
        .collect()
}

pub(super) fn groups(
    assignments: Vec<(Assignment, String)>,
    policy: Option<&RendererPolicy>,
    pause: &Control,
    duck: &Control,
) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for (assignment, output) in assignments {
        if policy.and_then(|policy| policy.shared_outputs) != Some(true)
            || assignment.outputs == [ALL_OUTPUT]
            || assignment.fill_mode == FillMode::Span
        {
            groups.push((assignment, output, Vec::new()));
            continue;
        }
        if let Some((group, target, members)) = groups.iter_mut().find(|(group, _, _)| {
            compatible(group, &assignment)
                && pause.outputs(&group.outputs) == pause.outputs(&assignment.outputs)
                && duck.outputs(&group.outputs) == duck.outputs(&assignment.outputs)
        }) {
            if !static_transition(group) && static_transition(&assignment) {
                group.transition.clone_from(&assignment.transition);
            }
            if !assignment.mute {
                group.mute = false;
                group.volume = assignment.volume;
            }
            group.outputs.extend(assignment.outputs.iter().cloned());
            *target = group.outputs.join(OUTPUT_SEPARATOR);
            members.push(assignment);
        } else {
            groups.push((assignment.clone(), output, vec![assignment]));
        }
    }
    groups
}

fn compatible(left: &Assignment, right: &Assignment) -> bool {
    left.outputs != [ALL_OUTPUT]
        && left.source == right.source
        && left.fill_mode == right.fill_mode
        && (!matches!(left.fill_mode, FillMode::Fit | FillMode::Center)
            || left.background == right.background)
        && left.system_audio.unwrap_or(true) == right.system_audio.unwrap_or(true)
        && left.volume == right.volume
        && left.layer == right.layer
        && ((static_transition(left) && static_transition(right)) || same_transition(left, right))
}

fn same_transition(left: &Assignment, right: &Assignment) -> bool {
    let without_source = |assignment: &Assignment| {
        assignment.transition.clone().map(|mut transition| {
            let has_source = transition.from.take().is_some();
            (transition, has_source)
        })
    };
    without_source(left) == without_source(right)
}

#[cfg(test)]
#[path = "sharing_tests.rs"]
mod tests;
