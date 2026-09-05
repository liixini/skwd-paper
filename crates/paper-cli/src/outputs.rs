use anyhow::{Context, Result};
use serde::Serialize;
use smithay_client_toolkit::{
    delegate_output, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
};
use wayland_client::{Connection, QueueHandle, globals::registry_queue_init, protocol::wl_output};

#[derive(Serialize)]
pub(crate) struct Output {
    name: String,
    description: Option<String>,
    position: (i32, i32),
    logical_position: Option<(i32, i32)>,
    logical_size: Option<(i32, i32)>,
    size: Option<(i32, i32)>,
    refresh_millihz: Option<i32>,
    scale: i32,
    transform: String,
}

struct State {
    registry: RegistryState,
    outputs: OutputState,
}

pub(crate) fn query() -> Result<Vec<Output>> {
    let connection =
        Connection::connect_to_env().context("connect to Wayland for output discovery")?;
    let (globals, mut queue) = registry_queue_init::<State>(&connection)?;
    let handle = queue.handle();
    let mut state = State {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &handle),
    };
    queue.roundtrip(&mut state).context("read Wayland output information")?;
    queue.roundtrip(&mut state).context("read logical output dimensions")?;
    let mut outputs = state
        .outputs
        .outputs()
        .filter_map(|output| {
            let info = state.outputs.info(&output)?;
            let mode = info.modes.iter().find(|mode| mode.current);
            Some(Output {
                name: info.name?,
                description: info.description,
                position: info.location,
                logical_position: info.logical_position,
                logical_size: info.logical_size,
                size: mode.map(|mode| mode.dimensions),
                refresh_millihz: mode.map(|mode| mode.refresh_rate),
                scale: info.scale_factor,
                transform: format!("{:?}", info.transform),
            })
        })
        .collect::<Vec<_>>();
    outputs.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(outputs)
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers![OutputState];
}

delegate_output!(State);
delegate_registry!(State);
