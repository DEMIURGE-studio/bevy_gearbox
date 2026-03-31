
pub use crate::{
    // Events
    EnterState,
    ExitState,
    ResetRegion,
    Transition,
    EdgeTraversed,
    state_component::Reset,
    // Components
    SubstateOf,
    Substates,
    StateMachine,
    transitions::DeferEvent,
    guards::Guards,
    history::HistoryState,
    InitialState,
    state_component::StateComponent,
    state_component::StateInactiveComponent,
    transitions::Delay,
    // Enums
    history::History,
    // Traits
    guards::Guard,
    guards::GuardProvider,
    // Systems
    get_all_leaf_states,
    state_component::state_component_enter,
    state_component::state_component_exit,
    state_component::state_inactive_component_enter,
    state_component::state_inactive_component_exit,
    transitions::Transitions,
    transitions::Source,
    transitions::Target,
    transitions::AlwaysEdge,
    transitions::EdgeKind,
    transitions::EventEdge,
    transitions::replay_deferred_event,
    transitions::TransitionEvent,
    transitions::NoEvent,
    transitions::AcceptAll,
    // Commands extensions
    commands::SpawnSubstate,
    commands::SpawnTransition,
    commands::BuildTransition,
    commands::TransitionExt,
    commands::InitStateMachine,
    // Bevy state integration
    bevy_state::GearboxCommandsExt,
    // Derive macros
    SimpleTransition,
};

pub use bevy_gearbox_legacy_macros::transition_event;
pub use bevy_gearbox_legacy_macros::state_component;
pub use bevy_gearbox_legacy_macros::state_bridge;
pub use bevy_gearbox_legacy_macros::gearbox_param;

pub use crate::parameter::{
    // Parameter components
    FloatParam,
    IntParam,
    BoolParam,
    // Parameter binding traits
    FloatParamBinding,
    IntParamBinding,
    BoolParamBinding,
    // Sync systems
    sync_float_param,
    sync_int_param,
    sync_bool_param,
    // Guard components and appliers
    FloatInRange,
    apply_float_param_guards,
    IntInRange,
    apply_int_param_guards,
    BoolEquals,
    apply_bool_param_guards,
};