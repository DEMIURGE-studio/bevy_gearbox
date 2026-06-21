//! Schedule-based state machine resolution.
//!
//! Uses a dedicated [`GearboxSchedule`] that runs in a loop:
//!
//! ```text
//! GearboxSchedule (loops until no work done):
//!   reset_pending_count
//!   TransitionPhase  <- resolve_transitions (skips blocked, inserts/removes Active)
//!   apply_deferred
//!   ExitPhase        <- user systems reacting to RemovedComponents<Active>
//!   EntryPhase       <- user systems reacting to Added<Active>
//!   GaugeSync        <- (gauge feature) sync WriteBack + AttributeDerived
//!   apply_deferred
//!   EdgeDetectPhase  <- check_always_edges, message_edge_listener, emit_terminal_done
//!   apply_deferred
//!   BlockerPhase     <- user blocker systems (MessageMutator<TransitionMessage>)
//!   collect_blocked  <- populates BlockedEdges resource
//!   apply_deferred
//!   SideEffectPhase  <- user side-effect systems (read Matched<M>, check BlockedEdges)
//! ```
//!
//! After the schedule converges, [`EnterState`] / [`ExitState`] entity events
//! are triggered for observer-based consumers.
//!
//! This is analogous to how Avian runs a physics schedule multiple times per frame.

pub mod commands;
pub mod components;
pub mod delay;
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
// Re-exports — preserve original public API
// ---------------------------------------------------------------------------

// Re-export `inventory` so the attribute macros can submit registrations
#[doc(hidden)]
pub use inventory;

pub use commands::{
    BuildEntityEvent, BuildTransition, GearboxCommandsExt, InitStateMachine, SpawnSubstate,
    SpawnTransition, TransitionBuilder, TransitionExt,
};
pub use components::{
    Active, AlwaysEdge, Delay, EdgeKind, EdgeTimer, InitialState, ResetEdge, ResetScope, Source,
    StateMachine, SubstateOf, Substates, Target, TerminalState, Transitions,
};
pub use history::{History, HistoryState};
pub use messages::{
    emit_terminal_done, message_edge_listener, AcceptAll, Done, GearboxMessage, Matched,
    MessageEdge, MessageValidator,
};
pub use registration::{
    bridge_to_bevy_state, gearbox_auto_register_plugin, replay_deferred_messages, DeferEvent,
    InstalledStateBridges, InstalledStateComponents, InstalledTransitions, RegistrationAppExt,
    StateBridgeInstaller, StateInstaller, TransitionInstaller,
};
pub use resolve::{BlockedEdges, EnterState, ExitState, TransitionMessage};
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

/// System sets within [`GearboxSchedule`], executed in order each iteration.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GearboxPhase {
    /// Internal: detects eligible edges (always-edges on `Changed<Active>`,
    /// message edges, terminal-done) and writes [`TransitionMessage`]s +
    /// [`Matched`](crate::messages::Matched) messages.
    EdgeDetectPhase,
    /// User blocker systems run here. Use
    /// [`MessageMutator<TransitionMessage>`] to set `blocked = true` on
    /// transitions that should not be applied.
    BlockerPhase,
    /// User side-effect systems run here. Read
    /// [`Matched<M>`](crate::messages::Matched) and check
    /// [`BlockedEdges`](crate::resolve::BlockedEdges) to skip blocked
    /// transitions.
    SideEffectPhase,
    /// Internal: reads surviving (non-blocked) transition messages, updates
    /// [`StateMachine`], and inserts/removes [`Active`] components.
    TransitionPhase,
    /// User systems that react to states being exited.
    /// Query `RemovedComponents<Active>` to detect exits.
    ExitPhase,
    /// User systems that react to states being entered.
    /// Query `Added<Active>` to detect entries.
    EntryPhase,
    /// Syncs gauge [`WriteBack`](bevy_gauge::prelude::WriteBack) and
    /// [`AttributeDerived`](bevy_gauge::prelude::AttributeDerived) components
    /// so that derived values are current before edge checks.
    #[cfg(feature = "gauge")]
    GaugeSync,
    /// Deprecated: use [`EdgeDetectPhase`](Self::EdgeDetectPhase) instead.
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
        writer.write(TransitionMessage {
            machine: entity,
            source: entity,
            target: initial.map(|i| i.0).unwrap_or(entity),
            edge: None,
            blocked: false,
        });
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

        app.add_message::<TransitionMessage>()
            .init_resource::<PendingCount>()
            .init_resource::<resolve::BlockedEdges>()
            .init_resource::<IterationCap>();

        let mut schedule = Schedule::new(GearboxSchedule);
        // TransitionPhase runs FIRST so that init messages (from
        // enqueue_machine_init) and delay-timer messages (from
        // tick_delay_timers) are resolved before EdgeDetect runs.
        // EdgeDetect then proposes new transitions from the newly-active
        // states, which pass through Blocker → SideEffect and are resolved
        // in the NEXT iteration's TransitionPhase.
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

        // NOTE: `register_transition::<Done>` must come AFTER `add_schedule`.
        // `register_transition` calls `add_systems(GearboxSchedule, ..)`,
        // which lazily creates a `GearboxSchedule` entry if none exists. If
        // called before `add_schedule`, that lazy entry gets clobbered by
        // `add_schedule`'s `insert` — losing the Done listener — while the
        // dedup resource still thinks the registration succeeded.
        app.register_transition::<Done>();

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
                // Resolve init/delay/previous-iteration edge messages.
                resolve::resolve_transitions.in_set(GearboxPhase::TransitionPhase),
                // Flush Active insert/remove so Exit/Entry phases see changes.
                ApplyDeferred
                    .after(GearboxPhase::TransitionPhase)
                    .before(GearboxPhase::ExitPhase),
                delay::cancel_delay_timers.in_set(GearboxPhase::ExitPhase),
                delay::start_delay_timers.in_set(GearboxPhase::EntryPhase),
                // Flush deferred commands from Exit/Entry (e.g.
                // StateComponent insert/remove) so they are visible to
                // EdgeDetect (Changed<Active>).
                ApplyDeferred
                    .after(GearboxPhase::EntryPhase)
                    .before(GearboxPhase::EdgeDetectPhase),
                // Edge detection: propose new transitions.
                messages::emit_terminal_done
                    .in_set(GearboxPhase::EdgeDetectPhase)
                    .before(resolve::check_always_edges),
                resolve::check_always_edges.in_set(GearboxPhase::EdgeDetectPhase),
                // Flush so blocker systems see edge-detect commands.
                ApplyDeferred
                    .after(GearboxPhase::EdgeDetectPhase)
                    .before(GearboxPhase::BlockerPhase),
                // After blockers, collect which edges were blocked.
                resolve::collect_blocked_edges
                    .after(GearboxPhase::BlockerPhase)
                    .before(GearboxPhase::SideEffectPhase),
                // Flush before side effects.
                ApplyDeferred
                    .after(GearboxPhase::BlockerPhase)
                    .before(GearboxPhase::SideEffectPhase),
            ),
        );

        // Outer driver: detect new machines, tick delay timers (so their
        // transition messages are in the buffer before the loop runs), run
        // the loop, then flush entity events for observer users.
        //
        // Ticking delays before the loop means a delay that finishes this
        // frame gets its transition applied in the same frame, and any
        // cascade it triggers resolves in the same frame too. Ticking after
        // the loop (the old layout) would leak a one-frame latency on every
        // delayed transition.
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
        app.add_systems(
            outer,
            resolve::flush_state_events
                .in_set(GearboxSet)
                .after(run_gearbox_schedule),
        );
    }
}
