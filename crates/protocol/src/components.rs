
// Stable component keys used on the wire (Phase 1 minimal)
pub const NAME: &str = "bevy_ecs::name::Name";

// Phase 1: reflect type-path strings used by BRP world.* queries
// State machine domain types in bevy_gearbox
pub const STATE_MACHINE: &str = "bevy_gearbox_core::StateMachine";
pub const STATE_CHILDREN: &str = "bevy_gearbox_core::Substates";
pub const TRANSITIONS: &str = "bevy_gearbox_core::transitions::Transitions";
pub const TARGET: &str = "bevy_gearbox_core::transitions::Target";
pub const TARGETED_BY: &str = "bevy_gearbox_core::transitions::TargetedBy";
pub const ALWAYS_EDGE: &str = "bevy_gearbox_core::transitions::AlwaysEdge";
pub const DELAY: &str = "bevy_gearbox_core::transitions::Delay";
pub const EDGE_KIND: &str = "bevy_gearbox_core::transitions::EdgeKind";
pub const INITIAL_STATE: &str = "bevy_gearbox_core::InitialState";
pub const STATE_MACHINE_ID: &str = "bevy_gearbox_core::StateMachineId";

// Editor-exposed trackers (from bevy_gearbox::server)
pub const TRANSITION_FEED: &str = "bevy_gearbox_core::server::TransitionFeed";

// Substring used to detect generic event edge component types like ...EventEdge<...>
pub const EVENT_EDGE_SUBSTR: &str = "EventEdge";