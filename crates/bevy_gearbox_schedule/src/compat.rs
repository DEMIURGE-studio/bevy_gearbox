/// Compatibility module re-exports for code using core-era module paths.
pub mod guards {
    pub use crate::components::{Guard, GuardProvider, Guards};
}

/// Compatibility re-exports for `bevy_gearbox::transitions::*`.
pub mod transitions {
    pub use crate::components::{
        AlwaysEdge, Delay, EdgeKind, Guards, Source, Target, Transitions,
        ResetEdge, ResetScope,
    };
    pub use crate::messages::{
        AcceptAll, GearboxMessage, MessageEdge, MessageValidator,
    };
    pub use crate::registration::DeferEvent;

    /// Shim: `TransitionEvent` is now [`GearboxMessage`](crate::messages::GearboxMessage).
    /// This alias allows old code to compile while migrating.
    pub use crate::messages::GearboxMessage as TransitionEvent;

    /// Shim: `EventEdge<E>` is now [`MessageEdge<M>`](crate::messages::MessageEdge).
    pub use crate::messages::MessageEdge as EventEdge;

    /// Shim: `EventValidator` is now [`MessageValidator`](crate::messages::MessageValidator).
    pub use crate::messages::MessageValidator as EventValidator;

    /// Placeholder for code that referenced `NoEvent` (phase sub-events are
    /// gone in the schedule version).
    #[derive(Clone, Default)]
    pub struct NoEvent;
}

/// Compatibility prelude matching core's `prelude` module.
pub mod prelude {
    pub use crate::components::{
        SubstateOf, Substates, Machine, InitialState,
        Source, Target, Transitions, AlwaysEdge, EdgeKind,
        Guards, Delay,
        Guard, GuardProvider,
        ResetEdge, ResetScope,
    };
    pub use crate::state_component::{
        StateComponent, StateInactiveComponent,
        state_component_enter, state_component_exit,
        state_inactive_component_enter, state_inactive_component_exit,
    };
    pub use crate::history::{History, HistoryState};
    pub use crate::registration::DeferEvent;
    pub use crate::messages::{
        GearboxMessage, MessageValidator, AcceptAll, MessageEdge,
    };
    pub use crate::commands::{
        SpawnSubstate, SpawnTransition, BuildTransition,
        TransitionExt, InitStateMachine, WriteMessageExt,
    };
    pub use crate::{GearboxSchedule, GearboxPhase, GearboxSet};
    pub use crate::resolve::{
        TransitionMessage, TransitionLog, TransitionRecord,
        FrameTransitionLog,
    };
    pub use crate::registration::RegistrationAppExt;
    pub use crate::parameters::{
        FloatParam, IntParam, BoolParam,
        FloatParamBinding, IntParamBinding, BoolParamBinding,
        FloatInRange, IntInRange, BoolEquals,
        sync_float_param, sync_int_param, sync_bool_param,
        apply_float_param_guards, apply_int_param_guards, apply_bool_param_guards,
    };
    pub use crate::NoEvent;

    // Compat aliases
    pub use crate::components::Machine as StateMachine;
    pub use crate::messages::MessageEdge as EventEdge;
    pub use crate::messages::GearboxMessage as TransitionEvent;
    pub use crate::messages::MessageValidator as EventValidator;
}

/// Compat: `SimpleTransition` doesn't exist as a derive in the schedule
/// version. Re-export the trait alias so `use bevy_gearbox::SimpleTransition`
/// resolves. Users will need to switch from `#[derive(SimpleTransition)]` to
/// a manual `impl GearboxMessage`.
pub use crate::messages::GearboxMessage as SimpleTransition;

/// Compat: core called it `StateMachine`, schedule calls it `Machine`.
pub use crate::components::Machine as StateMachine;

/// Compat alias for the plugin.
pub use crate::GearboxSchedulePlugin as GearboxPlugin;
