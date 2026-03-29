pub use bevy_gearbox_schedule::*;

// Core is still available for consumers that need it during migration.
pub mod core {
    pub use bevy_gearbox_core::*;
}

#[cfg(feature = "server")]
pub mod server {
    pub use bevy_gearbox_protocol::server::*;
}