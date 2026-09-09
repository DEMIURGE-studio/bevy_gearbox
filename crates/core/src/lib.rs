//! Schedule-based state machine resolution.
//!
//! Uses a dedicated [`GearboxSchedule`] that runs in a loop:
//!
//! ```text
//! GearboxSchedule (loops until no work done):
//!   reset_pending_count
//!   TransitionPhase  <- resolve_transitions (skips blocked, inserts/removes Active)
//!   apply_deferred
//!   ExitPhase        <- fire_exit_events (ExitState, deepest first),
//!                       user systems reacting to RemovedComponents<Active>
//!   apply_deferred   <- ExitState observers run
//!   EntryPhase       <- fire_enter_events (EnterState, shallowest first),
//!                       user systems reacting to Added<Active>
//!   GaugeSync        <- (gauge feature) sync WriteBack + AttributeDerived
//!   apply_deferred   <- EnterState observers run
//!   EdgeDetectPhase  <- emit_terminal_done, check_always_edges,
//!                       propose_delayed_transitions, message_edge_listener::<M>
//!                       (each proposes candidate TransitionMessages)
//!   apply_deferred
//!   BlockerPhase     <- check_state_guards and user guard systems veto
//!                       candidates (MessageMutator<TransitionMessage>)
//!   select_transitions <- keeps the best surviving candidate per group, fills BlockedEdges
//!   apply_deferred
//!   SideEffectPhase  <- user side-effect systems (read Matched<M>, check BlockedEdges)
//! ```
//!
//! Guards follow the statechart model: every edge that matches a trigger along
//! the active leaf's ancestor chain is a candidate, ranked deeper-state-first
//! and then by [`Transitions`] order; blockers veto, and the first survivor is
//! applied in the next iteration's `TransitionPhase`.
//!
//! [`EnterState`] / [`ExitState`] entity events fire inside the loop, so a
//! state passed through within one frame gets both, in statechart order.
//!
//! This is analogous to how Avian runs a physics schedule multiple times per frame.

pub mod commands;
pub mod components;
pub mod delay;
pub mod guards;
pub mod helpers;
pub mod history;
pub mod messages;
pub mod prelude;
pub mod registration;
pub mod resolve;
pub mod state_component;

#[cfg(feature = "gauge")]
pub mod gauge;

use bevy::ecs::intern::Interned;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;

use helpers::{compute_active_from_leaves, get_all_leaf_states};
use resolve::PendingCount;

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

// Used by the code generated in `bevy_gearbox_macros_impl`.
#[doc(hidden)]
pub use inventory;
#[doc(hidden)]
pub use bevy as __bevy;

#[allow(deprecated)]
pub use commands::{
    BuildEntityEvent, BuildTransition, GearboxCommandsExt, InitStateMachine, SpawnSubstate,
    SpawnTransition, TransitionBuilder, TransitionExt,
};
pub use components::{
    Active, AlwaysEdge, Delay, EdgeKind, EdgeTimer, InState, InitialState, NotInState,
    ResetEdge, ResetScope, Source, StateMachine, StateMachineId, SubstateOf, Substates,
    Target, TerminalState, Transitions,
};
pub use guards::check_state_guards;
pub use history::{History, HistoryState};
pub use messages::{
    emit_terminal_done, message_edge_listener, AcceptAll, Done, GearboxMessage, Matched,
    MessageEdge, MessageValidator,
};
pub use registration::{
    bridge_to_bevy_state, InstalledStateBridges, InstalledStateComponents, InstalledTransitions, RegistrationAppExt,
    StateBridgeInstaller, StateInstaller, TransitionInstaller,
};
pub use resolve::{BlockedEdges, CandidateGroups, EnterState, ExitState, TransitionMessage};
pub use state_component::{
    state_component_enter, state_component_exit, state_inactive_component_enter,
    state_inactive_component_exit, StateComponent, StateInactiveComponent,
};

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

/// System sets within [`GearboxSchedule`], declared in the order they run
/// each iteration.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GearboxPhase {
    /// Internal: applies the surviving (non-blocked) transition messages,
    /// updates [`StateMachine`], and inserts/removes [`Active`] components.
    TransitionPhase,
    /// User systems that react to states being exited.
    /// Query `RemovedComponents<Active>` to detect exits. [`ExitState`]
    /// entity events are triggered here and their observers run before
    /// `EntryPhase`.
    ExitPhase,
    /// User systems that react to states being entered.
    /// Query `Added<Active>` to detect entries. [`EnterState`] entity events
    /// are triggered here and their observers run before `EdgeDetectPhase`.
    EntryPhase,
    /// Syncs gauge [`WriteBack`](bevy_gauge::prelude::WriteBack) and
    /// [`AttributeDerived`](bevy_gauge::prelude::AttributeDerived) components
    /// so that derived values are current before edge detection.
    #[cfg(feature = "gauge")]
    GaugeSync,
    /// Internal: proposes candidate [`TransitionMessage`]s (always-edges on
    /// `Changed<Active>`, elapsed delays, message edges, terminal-done) and
    /// writes [`Matched`] messages.
    EdgeDetectPhase,
    /// Guard systems run here: the built-in [`check_state_guards`] and your
    /// own. Use [`MessageMutator<TransitionMessage>`] to set `blocked = true`
    /// on candidates that should not be applied; a vetoed candidate falls
    /// through to the next one in its group.
    BlockerPhase,
    /// User side-effect systems run here. Read
    /// [`Matched<M>`](crate::messages::Matched) and check
    /// [`BlockedEdges`] to skip blocked
    /// transitions.
    SideEffectPhase,
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

/// Detect newly-added StateMachine components and write initialization messages.
///
/// Machines with `InitialState` are sequential roots: the init transition
/// targets the initial child and `get_all_leaf_states` drills from there.
///
/// Machines without `InitialState` are parallel roots: the init transition
/// self-targets the machine entity, and `get_all_leaf_states` walks all
/// children (since the entity has `Substates` but no `InitialState`, it's
/// treated as a parallel parent). A machine with neither `InitialState` nor
/// children is a trivial single-state machine — itself is the only leaf.
fn enqueue_machine_init(
    q_new_machines: Query<(Entity, Option<&InitialState>), Added<StateMachine>>,
    mut writer: MessageWriter<TransitionMessage>,
) {
    for (entity, initial) in &q_new_machines {
        writer.write(TransitionMessage::new(
            entity,
            entity,
            initial.map(|i| i.0).unwrap_or(entity),
            None,
        ));
    }
}

/// Detect substates added at runtime under an already-active parallel parent
/// and activate them.
fn activate_added_substates(
    q_newly_attached: Query<(Entity, &SubstateOf), Added<SubstateOf>>,
    q_substate_of: Query<&SubstateOf>,
    q_initial: Query<&InitialState>,
    q_substates: Query<&Substates>,
    q_history: Query<&History>,
    q_history_state: Query<&mut HistoryState>,
    mut q_machine: Query<&mut StateMachine>,
    q_active: Query<(), With<Active>>,
    mut commands: Commands,
) {
    for (child, SubstateOf(parent)) in &q_newly_attached {
        let parent = *parent;

        // Find the machine root by walking SubstateOf ancestors. If the
        // parent (or any ancestor) isn't part of a StateMachine, skip.
        let machine_entity = q_substate_of.root_ancestor(parent);
        let Ok(mut machine) = q_machine.get_mut(machine_entity) else {
            continue;
        };

        // Parent must already be active.
        if !machine.active.contains(&parent) {
            continue;
        }

        // Sequential parents don't auto-activate new children.
        if q_initial.contains(parent) {
            continue;
        }

        // If parent was a childless leaf before this add, it's in
        // `active_leaves` — remove it since it now has a child.
        machine.active_leaves.remove(&parent);

        // Drill into the new subtree to find its leaves.
        let new_leaves = get_all_leaf_states(
            child,
            &q_initial,
            &q_substates,
            &q_history,
            &q_history_state,
        );
        for leaf in &new_leaves {
            machine.active_leaves.insert(*leaf);
        }

        // Recompute `active` from the updated leaf set.
        machine.active = compute_active_from_leaves(&machine.active_leaves, &q_substate_of);
        machine.active.insert(machine_entity);

        // Insert Active on states that don't yet carry it.
        for &state in &machine.active {
            if q_active.get(state).is_err() {
                commands.entity(state).insert(Active {
                    machine: machine_entity,
                });
            }
        }
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

    warn!("GearboxSchedule hit iteration cap ({cap}). Possible transition loop!");
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// State machine plugin. By default the driver systems (machine init,
/// delay timers, schedule runner) are added to [`Update`]. Use
/// [`schedule`](Self::schedule) to run them in a different schedule
/// (e.g. `FixedPreUpdate` for deterministic simulation).
pub struct GearboxPlugin {
    outer_schedule: Interned<dyn ScheduleLabel>,
}

impl Default for GearboxPlugin {
    fn default() -> Self {
        Self {
            outer_schedule: Update.intern(),
        }
    }
}

impl GearboxPlugin {
    /// Set the schedule where the driver systems run. The inner
    /// `GearboxSchedule` is unaffected — only the systems that detect
    /// new machines, tick delay timers, and invoke the schedule loop
    /// are moved.
    pub fn schedule(mut self, schedule: impl ScheduleLabel) -> Self {
        self.outer_schedule = schedule.intern();
        self
    }
}

impl Plugin for GearboxPlugin {
    fn build(&self, app: &mut App) {
        let outer = self.outer_schedule;

        // Explicit registration so scene serialization works for apps that
        // build Bevy without `reflect_auto_register`.
        app.register_type::<StateMachine>()
            .register_type::<StateMachineId>()
            .register_type::<Active>()
            .register_type::<InitialState>()
            .register_type::<SubstateOf>()
            .register_type::<Substates>()
            .register_type::<Source>()
            .register_type::<Transitions>()
            .register_type::<Target>()
            .register_type::<AlwaysEdge>()
            .register_type::<EdgeKind>()
            .register_type::<Delay>()
            .register_type::<TerminalState>()
            .register_type::<ResetEdge>()
            .register_type::<InState>()
            .register_type::<NotInState>()
            .register_type::<History>();

        app.add_message::<TransitionMessage>()
            .init_resource::<PendingCount>()
            .init_resource::<resolve::CandidateGroups>()
            .init_resource::<resolve::BlockedEdges>()
            .init_resource::<delay::ElapsedDelays>()
            .init_resource::<IterationCap>();

        let mut schedule = Schedule::new(GearboxSchedule);
        // TransitionPhase runs first so that init messages (from
        // enqueue_machine_init) are resolved before EdgeDetect runs.
        // EdgeDetect then proposes candidate transitions from the newly
        // active states (and from delays that elapsed this frame), which pass
        // through Blocker → select → SideEffect and are resolved in the next
        // iteration's TransitionPhase.
        #[cfg(not(feature = "gauge"))]
        schedule.configure_sets(
            (
                GearboxPhase::TransitionPhase,
                GearboxPhase::ExitPhase,
                GearboxPhase::EntryPhase,
                GearboxPhase::EdgeDetectPhase,
                GearboxPhase::BlockerPhase,
                GearboxPhase::SideEffectPhase,
            )
                .chain(),
        );
        #[cfg(feature = "gauge")]
        schedule.configure_sets(
            (
                GearboxPhase::TransitionPhase,
                GearboxPhase::ExitPhase,
                GearboxPhase::EntryPhase,
                GearboxPhase::GaugeSync,
                GearboxPhase::EdgeDetectPhase,
                GearboxPhase::BlockerPhase,
                GearboxPhase::SideEffectPhase,
            )
                .chain(),
        );
        app.add_schedule(schedule);

        // `register_transition::<Done>` must come after `add_schedule`: it calls
        // `add_systems(GearboxSchedule, ..)`, which would otherwise lazily create
        // a schedule entry that `add_schedule` then replaces, dropping the listener.
        app.register_transition::<Done>();

        // Install everything registered via inventory (derived GearboxMessage,
        // #[state_component], #[state_bridge]). Must run after `add_schedule`
        // so the GearboxSchedule entry these listeners target already exists.
        crate::registration::run_auto_installers(app);

        #[cfg(feature = "gauge")]
        {
            bevy_gauge::derived::add_gauge_sync_to_schedule(app, GearboxSchedule);
            app.configure_sets(
                GearboxSchedule,
                (
                    bevy_gauge::prelude::WriteBackSet.in_set(GearboxPhase::GaugeSync),
                    bevy_gauge::prelude::AttributeDerivedSet.in_set(GearboxPhase::GaugeSync),
                ),
            );
        }

        app.add_systems(
            GearboxSchedule,
            (
                // Reset work counter at the start of each iteration.
                resolve::reset_pending_count.before(GearboxPhase::TransitionPhase),
                // Resolve init messages and the previous iteration's winners.
                resolve::resolve_transitions.in_set(GearboxPhase::TransitionPhase),
                // Flush Active insert/remove so Exit/Entry phases see changes.
                ApplyDeferred
                    .after(GearboxPhase::TransitionPhase)
                    .before(GearboxPhase::ExitPhase),
                resolve::fire_exit_events.in_set(GearboxPhase::ExitPhase),
                delay::cancel_delay_timers.in_set(GearboxPhase::ExitPhase),
                // Run ExitState observers (and apply exit-phase commands)
                // before anything reacts to entries.
                ApplyDeferred
                    .after(GearboxPhase::ExitPhase)
                    .before(GearboxPhase::EntryPhase),
                resolve::fire_enter_events.in_set(GearboxPhase::EntryPhase),
                delay::start_delay_timers.in_set(GearboxPhase::EntryPhase),
                // Run EnterState observers and flush entry-phase commands
                // (e.g. StateComponent inserts) so they are visible to
                // EdgeDetect (Changed<Active>).
                ApplyDeferred
                    .after(GearboxPhase::EntryPhase)
                    .before(GearboxPhase::EdgeDetectPhase),
                // Edge detection: propose candidate transitions.
                messages::emit_terminal_done
                    .in_set(GearboxPhase::EdgeDetectPhase)
                    .before(resolve::check_always_edges),
                resolve::check_always_edges.in_set(GearboxPhase::EdgeDetectPhase),
                delay::propose_delayed_transitions.in_set(GearboxPhase::EdgeDetectPhase),
                // Flush so blocker systems see edge-detect commands.
                ApplyDeferred
                    .after(GearboxPhase::EdgeDetectPhase)
                    .before(GearboxPhase::BlockerPhase),
                guards::check_state_guards.in_set(GearboxPhase::BlockerPhase),
                // After blockers, pick one winner per candidate group and
                // record every blocked edge.
                resolve::select_transitions
                    .after(GearboxPhase::BlockerPhase)
                    .before(GearboxPhase::SideEffectPhase),
                // Flush before side effects.
                ApplyDeferred
                    .after(GearboxPhase::BlockerPhase)
                    .before(GearboxPhase::SideEffectPhase),
            ),
        );

        // Outer driver: detect new machines, tick delay timers (so elapsed
        // delays are proposed in the loop's first iteration), then run the
        // loop. A delay that finishes this frame is applied this frame, along
        // with any cascade it starts.
        app.add_systems(
            outer,
            (
                enqueue_machine_init,
                activate_added_substates,
                delay::tick_delay_timers,
                run_gearbox_schedule,
            )
                .chain()
                .in_set(GearboxSet),
        );
    }
}
