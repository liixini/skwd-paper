use super::dmabuf_present::run_shared_dmabuf_with_commit_ready;
use super::model::StartFade;
use super::readiness::signal_ready;
use crate::{ctl, shared, wayland};
use anyhow::{Context, Result, anyhow};
use paper_control::{MultiVideoEntry, PaperCommand};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) fn parse_manifest(spec: &str) -> Vec<MultiVideoEntry> {
    serde_json::from_str::<Vec<MultiVideoEntry>>(spec)
        .unwrap_or_default()
        .into_iter()
        .map(MultiVideoEntry::normalized)
        .filter(|entry| !entry.output.is_empty() && !entry.video.is_empty())
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
struct Group {
    video: String,
    outputs: Vec<String>,
    transition_from: Option<String>,
    mute: bool,
    volume: u32,
    with_audio: bool,
}

fn group_entries(entries: &[MultiVideoEntry]) -> Vec<Group> {
    let mut entries = entries.to_vec();
    entries.sort_by(|lhs, rhs| lhs.output.cmp(&rhs.output));

    // One source has one audio stream even when a startup transition splits it across groups.
    let mut audio = BTreeMap::<String, (bool, u32)>::new();
    for entry in &entries {
        let state = audio.entry(entry.video.clone()).or_insert((true, entry.volume));
        if !entry.mute && state.0 {
            *state = (false, entry.volume);
        }
    }

    let mut groups = Vec::<Group>::new();
    for entry in entries {
        let transition_from = entry.transition_from.filter(|from| from != &entry.video);
        if let Some(group) = groups
            .iter_mut()
            .find(|group| group.video == entry.video && group.transition_from == transition_from)
        {
            group.outputs.push(entry.output);
            continue;
        }
        let (mute, volume) = audio[&entry.video];
        groups.push(Group {
            video: entry.video,
            outputs: vec![entry.output],
            transition_from,
            mute,
            volume,
            with_audio: false,
        });
    }
    let mut audio_owners = HashSet::new();
    for group in &mut groups {
        group.with_audio = audio_owners.insert(group.video.clone());
    }
    groups
}

fn routed_to(outputs: &[String], command: &PaperCommand) -> bool {
    command
        .outputs
        .as_ref()
        .is_none_or(|wanted| wanted.iter().any(|name| outputs.iter().any(|output| output == name)))
}

pub(super) fn run_multi(
    entries: &[MultiVideoEntry],
    layer: Option<&str>,
    idle_secs: u32,
    shader: Option<&str>,
    duration_ms: u64,
) -> Result<()> {
    let sd = Arc::new(shared::create(std::ptr::null_mut()).context("shared device")?);
    let groups = group_entries(entries);
    tracing::info!(
        outputs = entries.len(),
        groups = groups.len(),
        "skwd-wall-vk: consolidated video VkDevice up"
    );

    let mut fanout = Vec::new();
    let mut ctls: Vec<ctl::Ctl> = Vec::new();
    let with_stdin = ctl::stdin_enabled();
    for group in &groups {
        let (tx, rx) = std::sync::mpsc::channel();
        let wake_pipe = if with_stdin { paper_runtime::wake::make_pipe() } else { None };
        let wake_owner = wake_pipe.clone();
        fanout.push((group.outputs.clone(), tx, wake_pipe));
        ctls.push(ctl::Ctl::from_channel(
            rx,
            wake_owner,
            &group.video,
            group.mute,
            group.volume,
            group.with_audio,
        ));
    }
    if with_stdin {
        paper_control::spawn_stdin_line_reader("skwd-wall-vk", move |line| {
            match serde_json::from_str::<PaperCommand>(line) {
                Ok(cmd) => {
                    for (outputs, tx, wake_pipe) in &fanout {
                        if !routed_to(outputs, &cmd) {
                            continue;
                        }
                        if tx.send(cmd.clone()).is_ok()
                            && let Some(wake) = wake_pipe
                        {
                            wake.poke();
                        }
                    }
                }
                Err(err) => tracing::info!("skwd-wall-vk: bad command ({err}): {line}"),
            }
        });
    }

    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    let mut handles = Vec::new();
    for (group, group_ctl) in groups.into_iter().zip(ctls) {
        let sd = Arc::clone(&sd);
        let ready = ready_tx.clone();
        let layer = layer.map(String::from);
        let shader = shader.map(String::from);
        handles.push(std::thread::spawn(move || -> Result<()> {
            let spec = group.outputs.join(",");
            let mut target =
                wayland::setup(&spec, layer.as_deref()).context("multi wayland setup")?;
            target.set_content_type(
                wayland_protocols::wp::content_type::v1::client::wp_content_type_v1::Type::Video,
            );
            for surf in &target.app.surfaces {
                tracing::info!(
                    w = surf.width,
                    h = surf.height,
                    output = %surf.name,
                    video = %group.video,
                    "vk consolidated surface"
                );
            }
            if idle_secs > 0 {
                target.arm_idle(idle_secs);
            }
            let start_fade = group.transition_from.map(|from| StartFade {
                from,
                shader,
                duration_ms,
                overlay: false,
                held: false,
            });
            let result = run_shared_dmabuf_with_commit_ready(
                &mut target,
                &group.video,
                group.mute,
                group.volume,
                start_fade,
                &sd,
                Some(group_ctl),
                &move || {
                    let _ = ready.send(());
                },
            );
            if let Err(err) = &result {
                tracing::error!("skwd-wall-vk: consolidated group [{spec}] failed: {err:#}");
            }
            result
        }));
    }
    drop(ready_tx);

    let total = handles.len();
    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut up = 0usize;
    while up < total {
        let left = deadline.saturating_duration_since(Instant::now());
        match ready_rx.recv_timeout(left) {
            Ok(()) => up += 1,
            Err(_) => break,
        }
    }
    if up == total {
        tracing::info!("skwd-wall-vk: consolidated video ready ({up}/{total} groups)");
        signal_ready();
    } else {
        tracing::error!("skwd-wall-vk: consolidated video incomplete ({up}/{total} groups)");
    }

    let mut first_err: Option<anyhow::Error> = None;
    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                first_err.get_or_insert(err);
            }
            Err(_) => {
                first_err.get_or_insert(anyhow!("multi group thread panicked"));
            }
        }
    }
    match first_err {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests;
