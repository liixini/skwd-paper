use crate::effect_lifetime::resolve_fbo_index;
use crate::effects::{CompositeBuffer, EffectBind};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DependencyKind {
    LayerComposite(CompositeBuffer),
    ScenePrefix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dependency {
    pub producer: usize,
    pub consumer: usize,
    pub kind: DependencyKind,
}

pub struct LayerTargetNode<'a> {
    pub id: &'a str,
    pub scene_order: usize,
    pub local_targets: &'a [String],
    pub binds: &'a [Vec<(usize, EffectBind)>],
    pub dynamic: bool,
    pub prefix_dynamic: bool,
}

#[derive(Clone, Copy)]
pub struct PassiveLayerTarget<'a> {
    pub id: &'a str,
    pub dynamic: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneTargetPlan {
    pub order: Vec<usize>,
    pub dependencies: Vec<Dependency>,
    pub snapshots: Vec<bool>,
    pub dynamic: Vec<bool>,
    pub retain: Vec<bool>,
    pub sampled: Vec<BTreeSet<CompositeBuffer>>,
    pub shadow_targets: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneTargetPlanError {
    MissingLayer { consumer: usize, layer: String },
    AmbiguousLayer { consumer: usize, layer: String },
    MissingLocalTarget { consumer: usize, target: String },
    Cycle { layers: Vec<usize> },
}

impl SceneTargetPlanError {
    #[must_use]
    pub fn consumers(&self) -> Vec<usize> {
        match self {
            Self::MissingLayer { consumer, .. }
            | Self::AmbiguousLayer { consumer, .. }
            | Self::MissingLocalTarget { consumer, .. } => vec![*consumer],
            Self::Cycle { layers } => layers.clone(),
        }
    }
}

impl fmt::Display for SceneTargetPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingLayer { consumer, layer } => {
                write!(formatter, "effect layer {consumer} references missing image layer {layer}")
            }
            Self::AmbiguousLayer { consumer, layer } => write!(
                formatter,
                "effect layer {consumer} references duplicate image layer id {layer}"
            ),
            Self::MissingLocalTarget { consumer, target } => write!(
                formatter,
                "effect layer {consumer} references undeclared local target {target}"
            ),
            Self::Cycle { layers } => {
                write!(formatter, "scene render-target dependency cycle across layers {layers:?}")
            }
        }
    }
}

impl std::error::Error for SceneTargetPlanError {}

fn add_dependency(dependencies: &mut Vec<Dependency>, dependency: Dependency) {
    if !dependencies.contains(&dependency) {
        dependencies.push(dependency);
    }
}

pub fn plan_scene_targets(
    nodes: &[LayerTargetNode<'_>],
    passive_layers: &[PassiveLayerTarget<'_>],
) -> Result<SceneTargetPlan, SceneTargetPlanError> {
    #[derive(Clone, Copy)]
    enum Provider {
        Active(usize),
        Passive(bool),
    }

    let mut ids: BTreeMap<&str, Vec<Provider>> = BTreeMap::new();
    for (index, node) in nodes.iter().enumerate() {
        ids.entry(node.id).or_default().push(Provider::Active(index));
    }
    for layer in passive_layers {
        ids.entry(layer.id).or_default().push(Provider::Passive(layer.dynamic));
    }

    let mut snapshots = vec![false; nodes.len()];
    let mut sampled = vec![BTreeSet::new(); nodes.len()];
    let mut passive_dynamic = vec![false; nodes.len()];
    let mut dependencies = Vec::new();
    for (consumer, node) in nodes.iter().enumerate() {
        for (_, binding) in node.binds.iter().flatten() {
            match binding {
                EffectBind::Previous => {}
                EffectBind::Named(target) => {
                    if resolve_fbo_index(node.local_targets.iter().map(String::as_str), target)
                        .is_none()
                    {
                        return Err(SceneTargetPlanError::MissingLocalTarget {
                            consumer,
                            target: target.clone(),
                        });
                    }
                }
                EffectBind::LayerComposite { layer, buffer } => {
                    let Some(producers) = ids.get(layer.as_str()) else {
                        return Err(SceneTargetPlanError::MissingLayer {
                            consumer,
                            layer: layer.clone(),
                        });
                    };
                    if producers.len() != 1 {
                        return Err(SceneTargetPlanError::AmbiguousLayer {
                            consumer,
                            layer: layer.clone(),
                        });
                    }
                    match producers[0] {
                        Provider::Active(producer) => {
                            sampled[producer].insert(*buffer);
                            add_dependency(
                                &mut dependencies,
                                Dependency {
                                    producer,
                                    consumer,
                                    kind: DependencyKind::LayerComposite(*buffer),
                                },
                            );
                        }
                        Provider::Passive(dynamic) => {
                            passive_dynamic[consumer] |= dynamic;
                        }
                    }
                }
                EffectBind::SceneSoFar => snapshots[consumer] = true,
            }
        }
    }

    for (consumer, needs_snapshot) in snapshots.iter().copied().enumerate() {
        if !needs_snapshot {
            continue;
        }
        for (producer, node) in nodes.iter().enumerate() {
            if node.scene_order < nodes[consumer].scene_order {
                add_dependency(
                    &mut dependencies,
                    Dependency { producer, consumer, kind: DependencyKind::ScenePrefix },
                );
            }
        }
    }
    dependencies.sort_by_key(|edge| {
        (
            nodes[edge.consumer].scene_order,
            edge.consumer,
            nodes[edge.producer].scene_order,
            edge.producer,
            match edge.kind {
                DependencyKind::LayerComposite(CompositeBuffer::A) => 0,
                DependencyKind::LayerComposite(CompositeBuffer::B) => 1,
                DependencyKind::ScenePrefix => 2,
            },
        )
    });

    let mut incoming = vec![0usize; nodes.len()];
    let mut outgoing = vec![Vec::new(); nodes.len()];
    let dependency_pairs: BTreeSet<(usize, usize)> =
        dependencies.iter().map(|dependency| (dependency.producer, dependency.consumer)).collect();
    for (producer, consumer) in dependency_pairs {
        incoming[consumer] += 1;
        outgoing[producer].push(consumer);
    }
    for consumers in &mut outgoing {
        consumers.sort_unstable();
        consumers.dedup();
    }

    let mut ready = BTreeSet::new();
    for (index, count) in incoming.iter().copied().enumerate() {
        if count == 0 {
            ready.insert((nodes[index].scene_order, index));
        }
    }
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(&(scene_order, producer)) = ready.first() {
        ready.remove(&(scene_order, producer));
        order.push(producer);
        for &consumer in &outgoing[producer] {
            incoming[consumer] -= 1;
            if incoming[consumer] == 0 {
                ready.insert((nodes[consumer].scene_order, consumer));
            }
        }
    }
    if order.len() != nodes.len() {
        let layers = incoming
            .iter()
            .enumerate()
            .filter_map(|(index, &count)| (count > 0).then_some(index))
            .collect();
        return Err(SceneTargetPlanError::Cycle { layers });
    }

    let mut dynamic: Vec<bool> = nodes
        .iter()
        .zip(&snapshots)
        .enumerate()
        .map(|(index, (node, &snapshot))| {
            node.dynamic || passive_dynamic[index] || (snapshot && node.prefix_dynamic)
        })
        .collect();
    for &producer in &order {
        if dynamic[producer] {
            for &consumer in &outgoing[producer] {
                dynamic[consumer] = true;
            }
        }
    }
    let mut retain = vec![false; nodes.len()];
    for dependency in &dependencies {
        if matches!(dependency.kind, DependencyKind::LayerComposite(_))
            && dynamic[dependency.consumer]
        {
            retain[dependency.producer] = true;
        }
    }

    let shadow_targets = usize::from(snapshots.iter().any(|needed| *needed));
    Ok(SceneTargetPlan { order, dependencies, snapshots, dynamic, retain, sampled, shadow_targets })
}

#[cfg(test)]
mod tests;
