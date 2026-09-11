use super::model::{App, Target};
use wayland_client::protocol::{wl_pointer, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};

#[derive(Clone, Copy, Debug, Default)]
pub struct Mouse {
    pub surface: Option<usize>,
    pub position: [f32; 2],
    pub buttons: [bool; 3],
    pub revision: u64,
}

impl App {
    fn bind_pointer(&mut self, qh: &QueueHandle<Self>) {
        if self.pointer_enabled && self.pointer_capable && self.pointer.is_none() {
            self.pointer = self.seat.as_ref().map(|seat| seat.get_pointer(qh, ()));
        }
    }

    fn pointer_motion(&mut self, x: f64, y: f64) {
        let Some(index) = self.pointer_focus else { return };
        let surface = &self.surfaces[index];
        if !x.is_finite() || !y.is_finite() || surface.width == 0 || surface.height == 0 {
            return;
        }
        self.mouse.surface = Some(index);
        self.mouse.position = [
            (x as f32 / surface.width as f32).clamp(0.0, 1.0),
            (y as f32 / surface.height as f32).clamp(0.0, 1.0),
        ];
        self.mouse.revision = self.mouse.revision.wrapping_add(1);
    }

    fn pointer_leave(&mut self) {
        self.pointer_focus = None;
        self.mouse.buttons = [false; 3];
        self.mouse.revision = self.mouse.revision.wrapping_add(1);
    }
}

impl Target {
    pub fn enable_pointer(&mut self) {
        self.app.pointer_enabled = true;
        self.app.bind_pointer(&self.queue.handle());
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for App {
    fn event(
        state: &mut Self,
        _: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(capabilities) } = event {
            state.pointer_capable = capabilities.contains(wl_seat::Capability::Pointer);
            if !state.pointer_capable {
                if let Some(pointer) = state.pointer.take()
                    && pointer.version() >= 3
                {
                    pointer.release();
                }
                state.pointer_leave();
            }
            state.bind_pointer(qh);
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for App {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter { surface, surface_x, surface_y, .. } => {
                state.pointer_focus = state.surfaces.iter().position(|ui| ui.surface == surface);
                state.pointer_motion(surface_x, surface_y);
            }
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                state.pointer_motion(surface_x, surface_y)
            }
            wl_pointer::Event::Leave { .. } => state.pointer_leave(),
            wl_pointer::Event::Button { button, state: WEnum::Value(button_state), .. } => {
                if state.pointer_focus.is_some() && (0x110..=0x112).contains(&button) {
                    state.mouse.buttons[(button - 0x110) as usize] =
                        button_state == wl_pointer::ButtonState::Pressed;
                    state.mouse.revision = state.mouse.revision.wrapping_add(1);
                }
            }
            _ => {}
        }
    }
}
