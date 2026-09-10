use std::collections::BTreeMap;

use crate::effects::EffectBind;

/// Ping and pong are the implicit full-size targets; `Fbo` indexes the effect's FBO list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Ping,
    Pong,
    Fbo(usize),
}

impl Target {
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Ping => 0,
            Self::Pong => 1,
            Self::Fbo(index) => index + 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassAccess {
    pub reads: Vec<Target>,
    pub write: Target,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TargetLifetime {
    pub first_read: Option<usize>,
    pub last_read: Option<usize>,
    pub first_write: Option<usize>,
    pub last_write: Option<usize>,
    /// Sampled before this frame writes it, so it cannot be shared between layers.
    pub loop_carried: bool,
    /// Sampled by the layer-composition pass after every effect layer.
    pub final_output: bool,
}

impl TargetLifetime {
    #[must_use]
    pub fn scratch_eligible(self) -> bool {
        !self.loop_carried && !self.final_output
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetPlan {
    pub passes: Vec<PassAccess>,
    pub lifetimes: Vec<TargetLifetime>,
    pub output: Target,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetPlanError {
    MetadataLength { targets: usize, binds: usize, owners: usize },
    ReadWriteAlias { pass: usize, target: Target },
}

#[must_use]
pub fn resolve_fbo_index<'a>(
    names: impl IntoIterator<Item = &'a str>,
    name: &str,
) -> Option<usize> {
    resolve_fbo_among(names.into_iter().enumerate(), name)
}

pub fn resolve_fbo_scoped<'a>(
    names: impl IntoIterator<Item = &'a str>,
    fbo_owners: &[Option<usize>],
    owner: usize,
    name: &str,
) -> Option<usize> {
    let candidates = names
        .into_iter()
        .enumerate()
        .filter(|(index, _)| fbo_owners.get(*index).copied().flatten().is_none_or(|o| o == owner));
    resolve_fbo_among(candidates, name)
}

fn resolve_fbo_among<'a>(
    candidates: impl IntoIterator<Item = (usize, &'a str)>,
    name: &str,
) -> Option<usize> {
    let mut longest: Option<(usize, usize)> = None;
    for (index, candidate) in candidates {
        if candidate == name {
            return Some(index);
        }
        let Some(suffix) = name.strip_prefix(candidate).and_then(|suffix| suffix.strip_prefix('_'))
        else {
            continue;
        };
        if suffix.is_empty()
            || !suffix.split('_').all(|part| !part.is_empty())
            || !suffix
                .rsplit('_')
                .next()
                .is_some_and(|tail| tail.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        if longest.is_none_or(|(_, length)| candidate.len() > length) {
            longest = Some((index, candidate.len()));
        }
    }
    longest.map(|(index, _)| index)
}

fn fbo_target(
    names: &[String],
    fbo_owners: &[Option<usize>],
    owner: usize,
    remap: &[usize],
    name: &str,
) -> Option<Target> {
    resolve_fbo_scoped(names.iter().map(String::as_str), fbo_owners, owner, name)
        .map(|logical| Target::Fbo(remap[logical]))
}

fn note_read(lifetime: &mut TargetLifetime, step: usize) {
    lifetime.first_read.get_or_insert(step);
    lifetime.last_read = Some(step);
    if lifetime.first_write.is_none() {
        lifetime.loop_carried = true;
    }
}

fn note_write(lifetime: &mut TargetLifetime, step: usize) {
    lifetime.first_write.get_or_insert(step);
    lifetime.last_write = Some(step);
}

/// Replays the renderer's target routing without touching Vulkan resources.
pub fn plan_targets(
    fbo_names: &[String],
    targets: &[Option<String>],
    binds: &[Vec<(usize, EffectBind)>],
    owners: &[usize],
) -> Result<TargetPlan, TargetPlanError> {
    let fbo_owners = vec![None; fbo_names.len()];
    plan_targets_with(fbo_names, &fbo_owners, &[], targets, binds, owners)
}

pub fn plan_targets_with(
    fbo_names: &[String],
    fbo_owners: &[Option<usize>],
    swaps: &[(Option<usize>, usize, usize)],
    targets: &[Option<String>],
    binds: &[Vec<(usize, EffectBind)>],
    owners: &[usize],
) -> Result<TargetPlan, TargetPlanError> {
    if targets.len() != binds.len() || targets.len() != owners.len() {
        return Err(TargetPlanError::MetadataLength {
            targets: targets.len(),
            binds: binds.len(),
            owners: owners.len(),
        });
    }

    let mut lifetimes = vec![TargetLifetime::default(); fbo_names.len() + 2];
    note_write(&mut lifetimes[Target::Pong.index()], 0);
    let mut remap: Vec<usize> = (0..fbo_names.len()).collect();
    let apply_swaps =
        |after: Option<usize>, remap: &mut Vec<usize>, lifetimes: &mut Vec<TargetLifetime>| {
            for &(when, a, b) in swaps {
                if when == after && a < remap.len() && b < remap.len() && a != b {
                    remap.swap(a, b);
                    for logical in [a, b] {
                        lifetimes[Target::Fbo(remap[logical]).index()].loop_carried = true;
                    }
                }
            }
        };
    apply_swaps(None, &mut remap, &mut lifetimes);

    let mut accesses = Vec::with_capacity(targets.len());
    let mut previous = Target::Pong;
    let mut source = Target::Pong;
    let mut flip = true;
    let mut current_owner = owners.first().copied().unwrap_or(0);

    for (pass, ((target_name, pass_binds), owner)) in
        targets.iter().zip(binds).zip(owners).enumerate()
    {
        if *owner != current_owner {
            current_owner = *owner;
            previous = source;
        }

        // An unresolved name leaves the renderer's existing descriptor input in place.
        let mut target_reads = BTreeMap::from([(0usize, source)]);
        for (slot, binding) in pass_binds {
            let bound = match binding {
                EffectBind::Previous => Some(previous),
                EffectBind::Named(name) => fbo_target(fbo_names, fbo_owners, *owner, &remap, name),
                EffectBind::LayerComposite { .. }
                | EffectBind::SceneSoFar
                | EffectBind::SceneUnderLayer => None,
            };
            if let Some(bound) = bound {
                target_reads.insert(*slot, bound);
            }
        }
        let mut reads: Vec<Target> = target_reads.into_values().collect();
        reads.sort_unstable_by_key(|target| target.index());
        reads.dedup();

        let named = target_name
            .as_deref()
            .and_then(|name| fbo_target(fbo_names, fbo_owners, *owner, &remap, name));
        let write = if let Some(named) = named {
            named
        } else {
            let candidate = if flip { Target::Ping } else { Target::Pong };
            if reads.contains(&candidate) {
                flip = !flip;
                if flip { Target::Ping } else { Target::Pong }
            } else {
                candidate
            }
        };
        if reads.contains(&write) {
            return Err(TargetPlanError::ReadWriteAlias { pass, target: write });
        }

        let step = pass + 1;
        for read in &reads {
            note_read(&mut lifetimes[read.index()], step);
        }
        note_write(&mut lifetimes[write.index()], step);
        accesses.push(PassAccess { reads, write });
        source = write;
        if named.is_none() {
            flip = !flip;
        }
        apply_swaps(Some(pass), &mut remap, &mut lifetimes);
    }

    let compose_step = targets.len() + 1;
    note_read(&mut lifetimes[source.index()], compose_step);
    lifetimes[source.index()].final_output = true;

    Ok(TargetPlan { passes: accesses, lifetimes, output: source })
}

#[cfg(test)]
mod tests;
