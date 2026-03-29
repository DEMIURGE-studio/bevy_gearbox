//! Schedule-based state machine resolution.
//!
//! Instead of observers that resolve transitions recursively in one shot,
//! this crate uses a dedicated [`GearboxSchedule`] that runs in a loop:
//!
//! 1. Systems read pending [`TransitionMessage`]s via [`MessageReader`]
//! 2. They compute exits/entries, update [`Machine`], and populate [`TransitionLog`]
//! 3. User systems in [`GearboxPhase::ExitPhase`] / [`GearboxPhase::EntryPhase`] react
//! 4. [`check_always_edges`] may produce *new* messages (e.g. AlwaysEdge becoming eligible)
//! 5. The schedule runs again until no new messages are produced or [`IterationCap`] is hit
//!
//! ```text
//! GearboxSchedule (loops until converged):
//!   ClearPhase      <- reset TransitionLog
//!   TransitionPhase <- resolve_transitions (internal)
//!   ExitPhase       <- user systems reacting to exited states
//!   EntryPhase      <- user systems reacting to entered states
//!   EdgeCheckPhase  <- check_always_edges (internal)
//! ```
//!
//! Timer ticking, parameter sync, and other per-frame work belongs in
//! [`Update`] ordered relative to [`GearboxSet`].
//!
//! This is analogous to how Avian runs a physics schedule multiple times per frame.

pub mod components;
pub mod history;
pub mod state_component;
pub mod resolve;
pub mod messages;
pub mod parameters;
pub mod delay;
pub mod commands;
pub mod registration;
pub mod compat;
pub mod helpers;

use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;

use resolve::PendingCount;

// ---------------------------------------------------------------------------
// Re-exports — preserve original public API
// ---------------------------------------------------------------------------

pub use components::{
    Machine, InitialState, SubstateOf, Substates, Source, Transitions, Target,
    AlwaysEdge, EdgeKind, Guards, Guard, GuardProvider, Delay, EdgeTimer,
    ResetEdge, ResetScope,
};
pub use history::{History, HistoryState};
pub use state_component::{
    StateComponent, StateInactiveComponent,
    state_component_enter, state_component_exit,
    state_inactive_component_enter, state_inactive_component_exit,
};
pub use resolve::{
    TransitionRecord, TransitionLog, TransitionMessage,
    FrameTransitionLog,
};
pub use messages::{
    GearboxMessage, MessageValidator, AcceptAll, MessageEdge, Matched,
    SideEffect, produce_side_effects, message_edge_listener,
};
pub use parameters::{
    FloatParam, IntParam, BoolParam,
    FloatParamBinding, IntParamBinding, BoolParamBinding,
    sync_float_param, sync_int_param, sync_bool_param,
    FloatInRange, IntInRange, BoolEquals,
    apply_float_param_guards, apply_int_param_guards, apply_bool_param_guards,
    init_float_param_guard_on_add, init_int_param_guard_on_add, init_bool_param_guard_on_add,
};
pub use commands::{
    SpawnSubstate, SpawnTransition, BuildTransition, TransitionBuilder,
    TransitionExt, InitStateMachine, WriteMessageExt,
};
pub use registration::{
    InstalledTransitions, InstalledStateComponents,
    InstalledFloatParams, InstalledIntParams, InstalledBoolParams,
    InstalledFloatParamBindings, InstalledIntParamBindings, InstalledBoolParamBindings,
    InstalledStateBridges,
    RegistrationAppExt, gearbox_auto_register_plugin,
    TransitionInstaller, StateInstaller,
    FloatParamInstaller, IntParamInstaller, BoolParamInstaller,
    FloatParamBindingInstaller, IntParamBindingInstaller, BoolParamBindingInstaller,
    StateBridgeInstaller,
    DeferEvent, replay_deferred_messages, bridge_to_bevy_state,
};
pub use compat::{SimpleTransition, StateMachine, GearboxPlugin};
pub use compat::guards;
pub use compat::transitions;
pub use compat::prelude;

/// Placeholder for code that referenced `NoEvent`.
#[derive(Clone, Default)]
pub struct NoEvent;

// ---------------------------------------------------------------------------
// Schedule, sets & phases
// ---------------------------------------------------------------------------

/// The schedule that resolves state machine transitions. Runs N times per
/// frame inside [`run_gearbox_schedule`].
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GearboxSchedule;

/// System set in [`Update`] that contains the gearbox schedule runner.
/// Use this for ordering user systems relative to gearbox resolution:
///
/// ```rust,ignore
/// app.add_systems(Update, my_trigger_system.before(GearboxSet));
/// ```
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GearboxSet;

/// System sets within [`GearboxSchedule`], executed in order each iteration.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GearboxPhase {
    /// Internal: clears [`TransitionLog`] for this iteration.
    ClearPhase,
    /// Internal: reads transition messages and updates [`Machine`].
    TransitionPhase,
    /// User systems that react to states being exited. Read [`TransitionLog`]
    /// to see which states were exited this iteration.
    ExitPhase,
    /// User systems that react to states being entered. Read [`TransitionLog`]
    /// to see which states were entered this iteration.
    EntryPhase,
    /// Internal: checks AlwaysEdge eligibility and writes new messages.
    EdgeCheckPhase,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of iterations the schedule will run per frame.
/// If hit, a warning is logged — this likely indicates a transition loop.
#[derive(Resource)]
pub struct IterationCap(pub u32);

impl Default for IterationCap {
    fn default() -> Self {
        Self(32)
    }
}

// ---------------------------------------------------------------------------
// Initialization system (runs in Update, writes init messages)
// ---------------------------------------------------------------------------

/// Detect newly-added Machine components and write initialization messages.
fn enqueue_machine_init(
    q_new_machines: Query<(Entity, &InitialState), Added<Machine>>,
    mut writer: MessageWriter<TransitionMessage>,
) {
    for (entity, initial) in &q_new_machines {
        writer.write(TransitionMessage {
            machine: entity,
            source: entity,
            target: initial.0,
            edge: None,
        });
    }
}

// ---------------------------------------------------------------------------
// Schedule runner (the "substep loop")
// ---------------------------------------------------------------------------

/// Runs [`GearboxSchedule`] in a loop until no new messages are produced or cap is hit.
fn run_gearbox_schedule(world: &mut World) {
    let cap = world
        .get_resource::<IterationCap>()
        .map(|c| c.0)
        .unwrap_or(32);

    for iteration in 0..cap {
        world.run_schedule(GearboxSchedule);

        let produced = world
            .get_resource::<PendingCount>()
            .map(|p| p.0)
            .unwrap_or(0);

        if produced == 0 {
            if iteration > 0 {
                debug!(
                    "GearboxSchedule converged after {} iteration(s)",
                    iteration + 1
                );
            }
            return;
        }
    }

    warn!(
        "GearboxSchedule hit iteration cap ({cap}). Possible transition loop!"
    );
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct GearboxSchedulePlugin;

impl Plugin for GearboxSchedulePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<TransitionMessage>()
            .init_resource::<PendingCount>()
            .init_resource::<IterationCap>()
            .init_resource::<TransitionLog>()
            .init_resource::<FrameTransitionLog>();

        let mut schedule = Schedule::new(GearboxSchedule);
        schedule.configure_sets((
            GearboxPhase::ClearPhase,
            GearboxPhase::TransitionPhase,
            GearboxPhase::ExitPhase,
            GearboxPhase::EntryPhase,
            GearboxPhase::EdgeCheckPhase,
        ).chain());
        app.add_schedule(schedule);

        app.add_systems(
            GearboxSchedule,
            (
                resolve::clear_transition_log.in_set(GearboxPhase::ClearPhase),
                resolve::resolve_transitions.in_set(GearboxPhase::TransitionPhase),
                delay::cancel_delay_timers.in_set(GearboxPhase::ExitPhase),
                delay::start_delay_timers.in_set(GearboxPhase::EntryPhase),
                resolve::check_always_edges.in_set(GearboxPhase::EdgeCheckPhase),
                resolve::accumulate_frame_log.in_set(GearboxPhase::EdgeCheckPhase),
            ),
        );

        // Outer driver: clear frame log, detect new machines, run loop, tick timers
        app.add_systems(
            Update,
            (
                resolve::clear_frame_log,
                enqueue_machine_init,
                run_gearbox_schedule,
                delay::tick_delay_timers,
            )
                .chain()
                .in_set(GearboxSet),
        );
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use super::*;

    /// Simple chain: A -[AlwaysEdge]-> B -[AlwaysEdge]-> C
    /// After one frame, the machine should be in state C.
    #[test]
    fn chain_resolves_to_leaf() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let a = world.spawn(SubstateOf(machine)).id();
        let b = world.spawn(SubstateOf(machine)).id();
        let c = world.spawn(SubstateOf(machine)).id();

        world.spawn((Source(a), Target(b), AlwaysEdge));
        world.spawn((Source(b), Target(c), AlwaysEdge));

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(a)));

        app.update();

        let machine_state = app.world().get::<Machine>(machine).unwrap();
        assert!(
            machine_state.is_active(&c),
            "C should be active, got active: {:?}",
            machine_state.active
        );
        assert!(
            machine_state.active_leaves.contains(&c),
            "C should be the only leaf, got leaves: {:?}",
            machine_state.active_leaves
        );
        assert!(!machine_state.active_leaves.contains(&a));
        assert!(!machine_state.active_leaves.contains(&b));
    }

    /// If we cap at 2 iterations, a chain of A -> B -> C should only get to B.
    #[test]
    fn cap_limits_resolution() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));
        app.insert_resource(IterationCap(2));

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let a = world.spawn(SubstateOf(machine)).id();
        let b = world.spawn(SubstateOf(machine)).id();
        let c = world.spawn(SubstateOf(machine)).id();

        world.spawn((Source(a), Target(b), AlwaysEdge));
        world.spawn((Source(b), Target(c), AlwaysEdge));

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(a)));

        app.update();

        let machine_state = app.world().get::<Machine>(machine).unwrap();
        assert!(machine_state.is_active(&b));
        assert!(!machine_state.active_leaves.contains(&c));
    }

    /// A machine with no AlwaysEdges stays put.
    #[test]
    fn stable_state_no_extra_iterations() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let a = world.spawn(SubstateOf(machine)).id();
        let b = world.spawn(SubstateOf(machine)).id();

        world.spawn((Source(a), Target(b)));

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(a)));

        app.update();

        let machine_state = app.world().get::<Machine>(machine).unwrap();
        assert!(machine_state.is_active(&a));
        assert!(!machine_state.is_active(&b));
    }

    /// External trigger: write a transition message directly.
    #[test]
    fn external_trigger() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let a = world.spawn(SubstateOf(machine)).id();
        let b = world.spawn(SubstateOf(machine)).id();

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(a)));

        app.update();
        assert!(app.world().get::<Machine>(machine).unwrap().is_active(&a));

        app.world_mut().write_message(TransitionMessage {
            machine,
            source: a,
            target: b,
            edge: None,
        });

        app.update();
        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(state.is_active(&b));
        assert!(!state.active_leaves.contains(&a));
    }

    // -----------------------------------------------------------------------
    // Parallel / edge-case tests
    // -----------------------------------------------------------------------

    /// Transitioning out of a parallel region must exit ALL sibling leaves.
    #[test]
    fn parallel_exit_cleans_up_sibling_leaves() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let p = world.spawn(SubstateOf(machine)).id();
        let a = world.spawn(SubstateOf(p)).id();
        let b = world.spawn(SubstateOf(p)).id();
        let c = world.spawn(SubstateOf(p)).id();
        let d = world.spawn(SubstateOf(machine)).id();

        world.spawn((Source(a), Target(d), AlwaysEdge));

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(p)));

        app.update();

        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(state.active_leaves.contains(&d));
        assert!(!state.active_leaves.contains(&a));
        assert!(!state.active_leaves.contains(&b));
        assert!(!state.active_leaves.contains(&c));
        assert!(!state.active.contains(&p));
    }

    /// If two parallel leaves both have eligible edges and one transition
    /// invalidates the other's source, the second should be skipped.
    #[test]
    fn stale_source_skipped_after_parallel_exit() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let p = world.spawn(SubstateOf(machine)).id();
        let a = world.spawn(SubstateOf(p)).id();
        let b = world.spawn(SubstateOf(p)).id();
        let d = world.spawn(SubstateOf(machine)).id();
        let e = world.spawn(SubstateOf(machine)).id();

        world.spawn((Source(a), Target(d), AlwaysEdge));
        world.spawn((Source(b), Target(e), AlwaysEdge));

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(p)));

        app.update();

        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(state.active_leaves.contains(&d));
        assert!(!state.active_leaves.contains(&a));
        assert!(!state.active_leaves.contains(&b));
        assert!(!state.active_leaves.contains(&e));
    }

    /// Self-transition exits and re-enters.
    #[test]
    fn self_transition_exits_and_reenters() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));

        #[derive(Resource, Default)]
        struct EntryCount(u32);

        #[derive(Component)]
        struct Marker;

        fn count_entries(
            q_machine: Query<&Machine, With<Marker>>,
            mut count: ResMut<EntryCount>,
        ) {
            for machine in &q_machine {
                if !machine.active_leaves.is_empty() {
                    count.0 += 1;
                }
            }
        }

        app.init_resource::<EntryCount>();
        app.add_systems(
            GearboxSchedule,
            count_entries.in_set(GearboxPhase::EntryPhase),
        );

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let a = world.spawn(SubstateOf(machine)).id();

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(a), Marker));

        app.update();
        let count_after_init = app.world().resource::<EntryCount>().0;

        app.world_mut().write_message(TransitionMessage {
            machine,
            source: a,
            target: a,
            edge: None,
        });
        app.update();

        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(state.active_leaves.contains(&a));

        let count_after_self = app.world().resource::<EntryCount>().0;
        assert!(
            count_after_self > count_after_init,
            "EntryPhase should have run again for the self-transition \
             (init={count_after_init}, after={count_after_self})"
        );
    }

    /// A transition sourced from a composite (non-leaf) parent should fire.
    #[test]
    fn transition_from_composite_parent() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let p = world.spawn(SubstateOf(machine)).id();
        let a = world.spawn(SubstateOf(p)).id();
        let _b = world.spawn(SubstateOf(p)).id();
        let d = world.spawn(SubstateOf(machine)).id();

        world.spawn((Source(p), Target(d), AlwaysEdge));

        world.entity_mut(p).insert(InitialState(a));
        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(p)));

        app.update();

        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(state.active_leaves.contains(&d));
        assert!(!state.active_leaves.contains(&a));
    }

    /// User system in EdgeCheckPhase clears a guard, enabling an AlwaysEdge
    /// in the same iteration cycle.
    #[test]
    fn user_system_clears_guard_mid_resolution() {
        #[derive(Component)]
        struct GuardedEdgeMarker;

        fn clear_guard_when_active(
            q_machine: Query<&Machine>,
            mut q_guards: Query<&mut Guards, With<GuardedEdgeMarker>>,
        ) {
            for machine in &q_machine {
                if !machine.active_leaves.is_empty() {
                    for mut guards in &mut q_guards {
                        guards.remove("blocked");
                    }
                }
            }
        }

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));
        // Run before edge check so the guard is cleared in time
        app.add_systems(
            GearboxSchedule,
            clear_guard_when_active.in_set(GearboxPhase::EntryPhase),
        );

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let a = world.spawn(SubstateOf(machine)).id();
        let b = world.spawn(SubstateOf(machine)).id();

        let mut initial_guards = Guards::default();
        initial_guards.add("blocked");
        world.spawn((
            Source(a),
            Target(b),
            AlwaysEdge,
            initial_guards,
            GuardedEdgeMarker,
        ));

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(a)));

        app.update();

        let machine_state = app.world().get::<Machine>(machine).unwrap();
        assert!(
            machine_state.is_active(&b),
            "B should be active — user system cleared the guard mid-resolution. \
             Got active: {:?}",
            machine_state.active
        );
    }

    // -----------------------------------------------------------------------
    // TransitionLog tests
    // -----------------------------------------------------------------------

    /// TransitionLog records exited and entered states.
    #[test]
    fn transition_log_records_entries_and_exits() {
        #[derive(Resource, Default)]
        struct Captured {
            entered: Vec<(Entity, Entity)>,
            exited: Vec<(Entity, Entity)>,
        }

        fn capture_log(log: Res<TransitionLog>, mut captured: ResMut<Captured>) {
            captured.entered.extend(log.all_entered());
            captured.exited.extend(log.all_exited());
        }

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));
        app.init_resource::<Captured>();
        app.add_systems(
            GearboxSchedule,
            capture_log.in_set(GearboxPhase::EntryPhase),
        );

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let a = world.spawn(SubstateOf(machine)).id();
        let b = world.spawn(SubstateOf(machine)).id();

        world.spawn((Source(a), Target(b), AlwaysEdge));

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(a)));

        app.update();

        let captured = app.world().resource::<Captured>();
        // Init enters A, then A->B exits A and enters B
        assert!(
            captured.entered.iter().any(|&(m, s)| m == machine && s == b),
            "B should appear in entered, got: {:?}",
            captured.entered
        );
        assert!(
            captured.exited.iter().any(|&(m, s)| m == machine && s == a),
            "A should appear in exited, got: {:?}",
            captured.exited
        );
    }

    // -----------------------------------------------------------------------
    // History tests
    // -----------------------------------------------------------------------

    /// Shallow history: re-entering a parent restores the immediate child
    /// that was active when it was last exited.
    ///
    /// ```text
    /// Machine
    /// +-- P (History::Shallow, InitialState=A)
    /// |   +-- A
    /// |   +-- B
    /// +-- D
    /// ```
    ///
    /// Start in P/A, transition A->B, then P->D, then D->P.
    /// P should restore to B (not A).
    #[test]
    fn shallow_history_restores_child() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let p = world.spawn((SubstateOf(machine), History::Shallow)).id();
        let a = world.spawn(SubstateOf(p)).id();
        let b = world.spawn(SubstateOf(p)).id();
        let d = world.spawn(SubstateOf(machine)).id();

        world.entity_mut(p).insert(InitialState(a));
        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(p)));

        // Init -> P/A
        app.update();
        assert!(app.world().get::<Machine>(machine).unwrap().active_leaves.contains(&a));

        // A -> B
        app.world_mut().write_message(TransitionMessage {
            machine,
            source: a,
            target: b,
            edge: None,
        });
        app.update();
        assert!(app.world().get::<Machine>(machine).unwrap().active_leaves.contains(&b));

        // P -> D (exits P, saves history: B was the active child)
        app.world_mut().write_message(TransitionMessage {
            machine,
            source: p,
            target: d,
            edge: None,
        });
        app.update();
        assert!(app.world().get::<Machine>(machine).unwrap().active_leaves.contains(&d));

        // D -> P (re-enters P; shallow history should restore B, not A)
        app.world_mut().write_message(TransitionMessage {
            machine,
            source: d,
            target: p,
            edge: None,
        });
        app.update();

        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(
            state.active_leaves.contains(&b),
            "Shallow history should restore B, got leaves: {:?}",
            state.active_leaves
        );
        assert!(!state.active_leaves.contains(&a));
    }

    /// Deep history: re-entering a parent restores the exact leaf
    /// configuration that was active when it was last exited.
    ///
    /// ```text
    /// Machine
    /// +-- P (History::Deep, InitialState=Q)
    /// |   +-- Q (InitialState=X)
    /// |       +-- X
    /// |       +-- Y
    /// +-- D
    /// ```
    ///
    /// Start in P/Q/X, transition X->Y, then P->D, then D->P.
    /// Deep history should restore directly to Y (skipping Q's InitialState).
    #[test]
    fn deep_history_restores_exact_leaves() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let p = world.spawn((SubstateOf(machine), History::Deep)).id();
        let q = world.spawn(SubstateOf(p)).id();
        let x = world.spawn(SubstateOf(q)).id();
        let y = world.spawn(SubstateOf(q)).id();
        let d = world.spawn(SubstateOf(machine)).id();

        world.entity_mut(q).insert(InitialState(x));
        world.entity_mut(p).insert(InitialState(q));
        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(p)));

        // Init -> P/Q/X
        app.update();
        assert!(app.world().get::<Machine>(machine).unwrap().active_leaves.contains(&x));

        // X -> Y
        app.world_mut().write_message(TransitionMessage {
            machine,
            source: x,
            target: y,
            edge: None,
        });
        app.update();
        assert!(app.world().get::<Machine>(machine).unwrap().active_leaves.contains(&y));

        // P -> D
        app.world_mut().write_message(TransitionMessage {
            machine,
            source: p,
            target: d,
            edge: None,
        });
        app.update();
        assert!(app.world().get::<Machine>(machine).unwrap().active_leaves.contains(&d));

        // D -> P (deep history should restore Y directly)
        app.world_mut().write_message(TransitionMessage {
            machine,
            source: d,
            target: p,
            edge: None,
        });
        app.update();

        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(
            state.active_leaves.contains(&y),
            "Deep history should restore Y, got leaves: {:?}",
            state.active_leaves
        );
        assert!(!state.active_leaves.contains(&x));
    }

    // -----------------------------------------------------------------------
    // State component tests
    // -----------------------------------------------------------------------

    /// StateComponent<T> inserts T on enter and removes on exit.
    #[test]
    fn state_component_insert_and_remove() {
        #[derive(Component, Clone, Debug, PartialEq)]
        struct Speed(f32);

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));
        app.add_systems(
            GearboxSchedule,
            (
                state_component_enter::<Speed>.in_set(GearboxPhase::EntryPhase),
                state_component_exit::<Speed>.in_set(GearboxPhase::ExitPhase),
            ),
        );

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let a = world.spawn((SubstateOf(machine), StateComponent(Speed(10.0)))).id();
        let b = world.spawn(SubstateOf(machine)).id();

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(a)));

        // Init -> A: Speed should be inserted on machine root
        app.update();
        assert_eq!(
            app.world().get::<Speed>(machine).map(|s| s.0),
            Some(10.0),
            "Speed should be on machine root after entering A"
        );

        // A -> B: Speed should be removed
        app.world_mut().write_message(TransitionMessage {
            machine,
            source: a,
            target: b,
            edge: None,
        });
        app.update();
        assert!(
            app.world().get::<Speed>(machine).is_none(),
            "Speed should be removed after exiting A"
        );
    }

    // -----------------------------------------------------------------------
    // Message-driven transition tests
    // -----------------------------------------------------------------------

    /// Helper message type for tests.
    #[derive(Message, Clone)]
    struct TestMsg {
        machine: Entity,
    }

    impl GearboxMessage for TestMsg {
        type Validator = AcceptAll;
        fn machine(&self) -> Entity {
            self.machine
        }
    }

    /// Basic message-driven transition: A --[TestMsg]--> B
    #[test]
    fn message_edge_triggers_transition() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));
        app.register_transition::<TestMsg>();

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let a = world.spawn(SubstateOf(machine)).id();
        let b = world.spawn(SubstateOf(machine)).id();

        // Edge A -> B triggered by TestMsg
        world.spawn((Source(a), Target(b), MessageEdge::<TestMsg>::default()));

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(a)));

        app.update();
        assert!(app.world().get::<Machine>(machine).unwrap().is_active(&a));

        // Send the message
        app.world_mut().write_message(TestMsg { machine });
        app.update();

        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(
            state.active_leaves.contains(&b),
            "B should be active after TestMsg, got leaves: {:?}",
            state.active_leaves
        );
        assert!(!state.active_leaves.contains(&a));
    }

    /// Message with a guarded edge should not fire until the guard is cleared.
    #[test]
    fn message_edge_respects_guards() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));
        app.register_transition::<TestMsg>();

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let a = world.spawn(SubstateOf(machine)).id();
        let b = world.spawn(SubstateOf(machine)).id();

        let mut guards = Guards::default();
        guards.add("blocked");
        world.spawn((
            Source(a),
            Target(b),
            MessageEdge::<TestMsg>::default(),
            guards,
        ));

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(a)));

        app.update();

        // Send message — should NOT transition because guard is active
        app.world_mut().write_message(TestMsg { machine });
        app.update();
        assert!(
            app.world().get::<Machine>(machine).unwrap().is_active(&a),
            "Should stay in A when guard is active"
        );
    }

    /// Message with a custom validator: only matching messages fire the edge.
    #[test]
    fn message_edge_with_validator() {
        #[derive(Message, Clone)]
        struct TypedMsg {
            machine: Entity,
            kind: u32,
        }

        #[derive(Default, Clone)]
        struct OnlyKind42;

        impl MessageValidator<TypedMsg> for OnlyKind42 {
            fn matches(&self, msg: &TypedMsg) -> bool {
                msg.kind == 42
            }
        }

        impl GearboxMessage for TypedMsg {
            type Validator = OnlyKind42;
            fn machine(&self) -> Entity {
                self.machine
            }
        }

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));
        app.register_transition::<TypedMsg>();

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let a = world.spawn(SubstateOf(machine)).id();
        let b = world.spawn(SubstateOf(machine)).id();

        // Edge with validator that only accepts kind=42
        world.spawn((
            Source(a),
            Target(b),
            MessageEdge::<TypedMsg>::new(Some(OnlyKind42)),
        ));

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(a)));

        app.update();

        // Wrong kind — should not fire
        app.world_mut().write_message(TypedMsg { machine, kind: 1 });
        app.update();
        assert!(app.world().get::<Machine>(machine).unwrap().is_active(&a));

        // Right kind — should fire
        app.world_mut().write_message(TypedMsg { machine, kind: 42 });
        app.update();
        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(
            state.active_leaves.contains(&b),
            "Should transition on kind=42, got leaves: {:?}",
            state.active_leaves
        );
    }

    /// Statechart semantics: deeper states get priority over ancestors.
    ///
    /// ```text
    /// Machine
    /// +-- P (InitialState=A)
    /// |   +-- A (leaf) --[TestMsg]--> B
    /// |   +-- B (leaf)
    /// +-- D
    /// ```
    ///
    /// P also has a TestMsg edge to D. A's edge should fire first (deeper).
    #[test]
    fn message_deeper_state_has_priority() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));
        app.register_transition::<TestMsg>();

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let p = world.spawn(SubstateOf(machine)).id();
        let a = world.spawn(SubstateOf(p)).id();
        let b = world.spawn(SubstateOf(p)).id();
        let d = world.spawn(SubstateOf(machine)).id();

        // A -> B (leaf-level edge, should have priority)
        world.spawn((Source(a), Target(b), MessageEdge::<TestMsg>::default()));
        // P -> D (parent-level edge, should NOT fire because A consumed)
        world.spawn((Source(p), Target(d), MessageEdge::<TestMsg>::default()));

        world.entity_mut(p).insert(InitialState(a));
        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(p)));

        app.update();
        assert!(app.world().get::<Machine>(machine).unwrap().is_active(&a));

        app.world_mut().write_message(TestMsg { machine });
        app.update();

        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(
            state.active_leaves.contains(&b),
            "A->B should fire (deeper state priority), got leaves: {:?}",
            state.active_leaves
        );
        assert!(!state.active_leaves.contains(&d));
    }

    /// Parallel regions each get one transition per message.
    ///
    /// ```text
    /// Machine
    /// +-- P (parallel)
    /// |   +-- A --[TestMsg]--> A2
    /// |   +-- B --[TestMsg]--> B2
    /// ```
    #[test]
    fn message_parallel_regions_each_fire() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));
        app.register_transition::<TestMsg>();

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let p = world.spawn(SubstateOf(machine)).id();
        let a = world.spawn(SubstateOf(p)).id();
        let a2 = world.spawn(SubstateOf(p)).id();
        let b = world.spawn(SubstateOf(p)).id();
        let b2 = world.spawn(SubstateOf(p)).id();

        world.spawn((Source(a), Target(a2), MessageEdge::<TestMsg>::default()));
        world.spawn((Source(b), Target(b2), MessageEdge::<TestMsg>::default()));

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(p)));

        app.update();
        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(state.active_leaves.contains(&a));
        assert!(state.active_leaves.contains(&b));

        app.world_mut().write_message(TestMsg { machine });
        app.update();

        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(
            state.active_leaves.contains(&a2),
            "A->A2 should fire in region A, got leaves: {:?}",
            state.active_leaves
        );
        assert!(
            state.active_leaves.contains(&b2),
            "B->B2 should fire in region B, got leaves: {:?}",
            state.active_leaves
        );
    }

    /// Double registration is idempotent.
    #[test]
    fn register_transition_dedup() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));
        app.register_transition::<TestMsg>();
        app.register_transition::<TestMsg>(); // should not panic or double-register
        // If it doubled, we'd get duplicate TransitionMessages per msg — test indirectly
        // by just verifying the machine works normally.

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let a = world.spawn(SubstateOf(machine)).id();
        let b = world.spawn(SubstateOf(machine)).id();

        world.spawn((Source(a), Target(b), MessageEdge::<TestMsg>::default()));

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(a)));

        app.update();
        app.world_mut().write_message(TestMsg { machine });
        app.update();

        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(state.active_leaves.contains(&b));
    }

    // -----------------------------------------------------------------------
    // EdgeKind tests
    // -----------------------------------------------------------------------

    /// Internal transition: source IS the LCA, so it should NOT be exited
    /// and re-entered.
    #[test]
    fn internal_transition_does_not_exit_source() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let p = world.spawn(SubstateOf(machine)).id();
        let a = world.spawn(SubstateOf(p)).id();
        let b = world.spawn(SubstateOf(p)).id();

        // Internal edge on P: A -> B, P is the LCA and should NOT be exited
        world.spawn((Source(a), Target(b), AlwaysEdge, EdgeKind::Internal));

        world.entity_mut(p).insert(InitialState(a));
        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(p)));

        app.update();

        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(
            state.active_leaves.contains(&b),
            "B should be active, got leaves: {:?}",
            state.active_leaves
        );
        // P should still be active (internal = no exit of LCA)
        assert!(
            state.active.contains(&p),
            "P should still be active (internal transition)"
        );
    }

    // -----------------------------------------------------------------------
    // Delay tests
    // -----------------------------------------------------------------------

    /// A delayed AlwaysEdge should NOT fire in the same frame as entry.
    /// It should create an EdgeTimer on the edge entity, and NOT fire
    /// via check_always_edges.
    #[test]
    fn delayed_always_edge_creates_timer_and_skips_immediate() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let a = world.spawn(SubstateOf(machine)).id();
        let b = world.spawn(SubstateOf(machine)).id();

        let edge = world
            .spawn((
                Source(a),
                Target(b),
                AlwaysEdge,
                Delay::new(Duration::from_secs(5)),
            ))
            .id();

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(a)));

        // First frame: enter A, start timer, but do NOT fire immediately
        app.update();
        assert!(
            app.world().get::<Machine>(machine).unwrap().is_active(&a),
            "Should stay in A — delay not elapsed"
        );
        // EdgeTimer should have been created on the edge entity
        assert!(
            app.world().get::<EdgeTimer>(edge).is_some(),
            "EdgeTimer should exist on the delayed edge after source was entered"
        );
    }

    // -----------------------------------------------------------------------
    // ResetEdge tests
    // -----------------------------------------------------------------------

    /// ResetEdge(Target) clears history under the target before re-entering.
    ///
    /// ```text
    /// Machine
    /// +-- P (History::Deep, InitialState=Q)
    /// |   +-- Q (InitialState=X)
    /// |       +-- X
    /// |       +-- Y
    /// +-- D
    /// ```
    ///
    /// Enter P/Q/X -> move to Y -> exit to D (saves deep history Y) ->
    /// transition D->P with ResetEdge(Target) -> should go to X (history cleared).
    #[test]
    fn reset_edge_clears_history() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));

        let world = app.world_mut();
        let machine = world.spawn_empty().id();
        let p = world.spawn((SubstateOf(machine), History::Deep)).id();
        let q = world.spawn(SubstateOf(p)).id();
        let x = world.spawn(SubstateOf(q)).id();
        let y = world.spawn(SubstateOf(q)).id();
        let d = world.spawn(SubstateOf(machine)).id();

        world.entity_mut(q).insert(InitialState(x));
        world.entity_mut(p).insert(InitialState(q));
        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(p)));

        // Edge D -> P with ResetEdge(Target) (NOT AlwaysEdge — we trigger manually)
        let reset_edge = world
            .spawn((Source(d), Target(p), ResetEdge(ResetScope::Target)))
            .id();

        // Init -> P/Q/X
        app.update();
        assert!(app.world().get::<Machine>(machine).unwrap().active_leaves.contains(&x));

        // X -> Y
        app.world_mut().write_message(TransitionMessage {
            machine,
            source: x,
            target: y,
            edge: None,
        });
        app.update();

        // P -> D (saves deep history: Y)
        app.world_mut().write_message(TransitionMessage {
            machine,
            source: p,
            target: d,
            edge: None,
        });
        app.update();
        assert!(app.world().get::<Machine>(machine).unwrap().active_leaves.contains(&d));

        // D -> P with the ResetEdge — use the edge entity so ResetEdge is checked
        app.world_mut().write_message(TransitionMessage {
            machine,
            source: d,
            target: p,
            edge: Some(reset_edge),
        });
        app.update();

        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(
            state.active_leaves.contains(&x),
            "Reset should clear history, restoring to InitialState (X). Got leaves: {:?}",
            state.active_leaves
        );
        assert!(!state.active_leaves.contains(&y));
    }
}
