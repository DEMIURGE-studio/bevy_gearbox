//! Reflected type paths used to address components over BRP (`world.*`
//! queries and `+watch` component filters).

pub const NAME: &str = "bevy_ecs::name::Name";

// State machine components from bevy_gearbox_core.
pub const STATE_MACHINE: &str = "bevy_gearbox_core::components::StateMachine";
pub const STATE_CHILDREN: &str = "bevy_gearbox_core::components::Substates";
pub const TRANSITIONS: &str = "bevy_gearbox_core::components::Transitions";
pub const TARGET: &str = "bevy_gearbox_core::components::Target";
pub const ALWAYS_EDGE: &str = "bevy_gearbox_core::components::AlwaysEdge";
pub const DELAY: &str = "bevy_gearbox_core::components::Delay";
pub const EDGE_KIND: &str = "bevy_gearbox_core::components::EdgeKind";
pub const INITIAL_STATE: &str = "bevy_gearbox_core::components::InitialState";
pub const STATE_MACHINE_ID: &str = "bevy_gearbox_core::components::StateMachineId";

// Substring used to detect generic message edge component types like ...MessageEdge<...>
pub const MESSAGE_EDGE_SUBSTR: &str = "MessageEdge";