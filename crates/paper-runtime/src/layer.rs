#[macro_export]
macro_rules! impl_layer_registry {
    ($app:ty) => {
        impl ::smithay_client_toolkit::registry::ProvidesRegistryState for $app {
            fn registry(&mut self) -> &mut ::smithay_client_toolkit::registry::RegistryState {
                &mut self.registry_state
            }
            ::smithay_client_toolkit::registry_handlers![
                ::smithay_client_toolkit::output::OutputState
            ];
        }
        ::smithay_client_toolkit::delegate_compositor!($app);
        ::smithay_client_toolkit::delegate_output!($app);
        ::smithay_client_toolkit::delegate_registry!($app);
    };
}

#[macro_export]
macro_rules! impl_empty_dispatch {
    ($app:ty, $($proto:ty),+ $(,)?) => {
        $(
            impl ::wayland_client::Dispatch<$proto, ()> for $app {
                fn event(
                    _: &mut Self,
                    _: &$proto,
                    _: <$proto as ::wayland_client::Proxy>::Event,
                    (): &(),
                    _: &::wayland_client::Connection,
                    _: &::wayland_client::QueueHandle<Self>,
                ) {
                }
            }
        )+
    };
}

#[macro_export]
macro_rules! layer_surface_defaults {
    ($layer:expr) => {{
        use ::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
            Anchor, KeyboardInteractivity,
        };
        $layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
        $layer.set_exclusive_zone(-1);
        $layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        $layer.set_size(0, 0);
    }};
}
