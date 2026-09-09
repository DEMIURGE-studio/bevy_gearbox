//! Statecharts for Bevy, built out of ordinary entities.
//!
//! A machine is an entity hierarchy: the root carries [`StateMachine`], every
//! state is an entity linked to its parent by [`SubstateOf`] / [`Substates`],
//! and every transition is an entity linked to its source by [`Source`] /
//! [`Transitions`] with a [`Target`]. Author a whole chart as one `bsn!` scene:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use bevy::scene::prelude::{bsn, CommandsSceneExt};
//! use bevy_gearbox::prelude::*;
//!
//! #[derive(Message, Clone, Reflect, GearboxMessage)]
//! struct Fire {
//!     #[gearbox(target)]
//!     machine: Entity,
//! }
//!
//! fn spawn(mut commands: Commands) {
//!     commands.spawn_scene(bsn! {
//!         StateMachine InitialState(#Ready)
//!         Substates [
//!             #Ready Transitions [ (Target(#Cooldown) MessageEdge::<Fire>) ],
//!             #Cooldown Transitions [ (Target(#Ready) AlwaysEdge Delay::from_secs_f32(0.8)) ],
//!         ]
//!     });
//! }
//!
//! App::new()
//!     .add_plugins((DefaultPlugins, GearboxPlugin::default()))
//!     .add_systems(Startup, spawn)
//!     .run();
//! ```
//!
//! Transitions fire in response to Bevy messages ([`GearboxMessage`]), on entry
//! ([`AlwaysEdge`]), after a [`Delay`], or when a [`TerminalState`] finishes.
//! Guards are marker components on edges plus systems in
//! [`GearboxPhase::BlockerPhase`] ([`InState`] and [`NotInState`] are built
//! in); several edges for one trigger are tried in [`Transitions`] order and
//! the first survivor wins. React to state changes
//! with `Added<Active>` queries, [`EnterState`] / [`ExitState`] observers, or a
//! [`StateComponent`] that mirrors a state onto the machine root.
//!
//! Resolution runs in [`GearboxSchedule`], looped each frame until no work
//! remains, so message-driven cascades settle within the frame they start.
//!
//! The [guide](https://github.com/DEMIURGE-studio/bevy_gearbox/blob/master/DOCS.md)
//! walks through every feature with a worked example. The `server` feature
//! adds [`server::ServerPlugin`], which lets the visual editor connect to a
//! running game; it is off by default because it pulls in an HTTP server.

extern crate self as bevy_gearbox;

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

/// The core crate, re-exported whole for paths that want to be explicit.
pub mod core {
    pub use bevy_gearbox_core::*;
}

/// Editor server: exposes running machines over the Bevy Remote Protocol.
#[cfg(feature = "server")]
pub mod server {
    pub use bevy_gearbox_protocol::server::*;
}
