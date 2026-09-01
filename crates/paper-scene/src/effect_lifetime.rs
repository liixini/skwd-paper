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
    let mut longest: Option<(usize, usize)> = None;
    for (index, candidate) in names.into_iter().enumerate() {
        if candidate == name {
            return Some(index);
        }
        let Some(suffix) = name.strip_prefix(candidate).and_then(|suffix| suffix.strip_prefix('_'))
        else {
            continue;
        };
        if suffix.is_empty()
            || !suffix
                .split('_')
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            continue;
        }
        if longest.is_none_or(|(_, length)| candidate.len() > length) {
            longest = Some((index, candidate.len()));
        }
    }
    longest.map(|(index, _)| index)
}

fn fbo_target(names: &[String], name: &str) -> Option<Target> {
    resolve_fbo_index(names.iter().map(String::as_str), name).map(Target::Fbo)
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
    if targets.len() != binds.len() || targets.len() != owners.len() {
        return Err(TargetPlanError::MetadataLength {
            targets: targets.len(),
            binds: binds.len(),
            owners: owners.len(),
        });
    }

    let mut lifetimes = vec![TargetLifetime::default(); fbo_names.len() + 2];
    note_write(&mut lifetimes[Target::Pong.index()], 0);

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
                EffectBind::Named(name) => fbo_target(fbo_names, name),
                EffectBind::LayerComposite { .. } | EffectBind::SceneSoFar => None,
            };
            if let Some(bound) = bound {
                target_reads.insert(*slot, bound);
            }
        }
        let mut reads: Vec<Target> = target_reads.into_values().collect();
        reads.sort_unstable_by_key(|target| target.index());
        reads.dedup();

        let named = target_name.as_deref().and_then(|name| fbo_target(fbo_names, name));
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
    }

    let compose_step = targets.len() + 1;
    note_read(&mut lifetimes[source.index()], compose_step);
    lifetimes[source.index()].final_output = true;

    Ok(TargetPlan { passes: accesses, lifetimes, output: source })
}

#[cfg(test)]
mod tests;
