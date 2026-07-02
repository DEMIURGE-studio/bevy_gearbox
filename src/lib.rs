pub use bevy_gearbox_core::*;

// Derive macros
pub use bevy_gearbox_macros::GearboxMessage;

// Attribute macros
pub use bevy_gearbox_macros::state_component;
pub use bevy_gearbox_macros::state_bridge;


/// Common imports: the core prelude plus the gearbox derive/attribute macros.
///
/// `use bevy_gearbox::prelude::*;` brings the state-machine components, the
/// `GearboxMessage` trait, and the `#[derive(GearboxMessage)]` macro into scope.
pub mod prelude {
    pub use bevy_gearbox_core::prelude::*;

    pub use bevy_gearbox_macros::{state_bridge, state_component, GearboxMessage};
}

pub mod core {
    pub use bevy_gearbox_core::*;
}

#[cfg(feature = "server")]
pub mod server {
    pub use bevy_gearbox_protocol::server::*;
}