use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;

use crate::components::*;
use crate::helpers::*;
use crate::history::*;

// ---------------------------------------------------------------------------
// TransitionMessage / PendingCount / CandidateGroups
// ---------------------------------------------------------------------------

/// A pending transition, proposed by edge detection (or by machine init and
/// elapsed delays) and applied by [`resolve_transitions`].
///
/// Several candidates can compete for one trigger. Every edge along the
/// active leaf's ancestor chain that matches a message is proposed with the
/// same `group` and its own `rank`. Blocker systems veto candidates by setting
/// `blocked`; [`select_transitions`] then keeps the lowest-ranked survivor in
/// each group and blocks the rest. Guards are therefore "ordered candidates,
/// first passing guard wins", and a guardless edge placed last in
/// [`Transitions`] is the fallback.
#[derive(Message, Debug, Clone)]
pub struct TransitionMessage {
    pub machine: Entity,
    pub source: Entity,
    pub target: Entity,
    /// The edge entity that proposed this transition, if any. Used for
    /// [`EdgeKind`], [`ResetEdge`], and [`BlockedEdges`] bookkeeping.
    pub edge: Option<Entity>,
    /// Set to `true` by blocker systems in
    /// [`BlockerPhase`](crate::GearboxPhase::BlockerPhase) to veto this
    /// candidate. [`select_transitions`] also sets it on candidates that lost
    /// to a better-ranked one in the same group.
    pub blocked: bool,
    /// Candidates competing for the same trigger in the same region share a
    /// group. Group `0` is reserved for standalone transitions that compete
    /// with nothing (see [`TransitionMessage::new`]).
    pub group: u64,
    /// Priority within the group, lowest wins: (depth rank, index in the
    /// source's [`Transitions`]). Deeper states rank first.
    pub rank: (u32, u32),
}

impl TransitionMessage {
    /// A standalone transition that competes with no other candidate.
    pub fn new(machine: Entity, source: Entity, target: Entity, edge: Option<Entity>) -> Self {
        Self {
            machine,
            source,
            target,
            edge,
            blocked: false,
            group: 0,
            rank: (0, 0),
        }
    }
}

/// Tracks how much work was done during the current schedule iteration.
/// The outer loop checks this after each iteration to decide whether to
/// run another one. Reset by [`reset_pending_count`] at the top of each
/// iteration. Incremented by edge detection systems that write
/// [`TransitionMessage`]s and by [`resolve_transitions`] for each
/// non-blocked transition it processes.
#[derive(Resource, Default)]
pub struct PendingCount(pub usize);

/// Hands out [`TransitionMessage::group`] ids to edge-detection systems.
#[derive(Resource, Default)]
pub struct CandidateGroups(u64);

impl CandidateGroups {
    /// A fresh group id (never `0`).
    pub fn next(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
}

/// Set of edge entities whose [`TransitionMessage`] ended this iteration
/// `blocked`, either vetoed by a blocker system or beaten by a better-ranked
/// candidate in the same group. Populated by [`select_transitions`] after
/// [`BlockerPhase`](crate::GearboxPhase::BlockerPhase). Side-effect systems
/// check this to skip the [`Matched`](crate::messages::Matched) messages of
/// transitions that will not be applied.
#[derive(Resource, Default)]
pub struct BlockedEdges(pub HashSet<Entity>);

impl BlockedEdges {
    /// Returns `true` if the given edge was blocked this iteration.
    pub fn is_blocked(&self, edge: Entity) -> bool {
        self.0.contains(&edge)
    }
}

/// Reset [`PendingCount`] at the start of each schedule iteration.
pub(crate) fn reset_pending_count(mut pending: ResMut<PendingCount>) {
    pending.0 = 0;
}

/// After [`BlockerPhase`](crate::GearboxPhase::BlockerPhase): within each
/// candidate group keep the lowest-ranked unblocked transition and block the
/// others, then record every blocked edge in [`BlockedEdges`].
pub(crate) fn select_transitions(
    mut candidates: MessageMutator<TransitionMessage>,
    mut blocked: ResMut<BlockedEdges>,
) {
    blocked.0.clear();
    let mut msgs: Vec<&mut TransitionMessage> = candidates.read().collect();

    let mut best: HashMap<u64, (u32, u32)> = HashMap::new();
    for msg in msgs.iter() {
        if msg.blocked || msg.group == 0 {
            continue;
        }
        best.entry(msg.group)
            .and_modify(|r| {
                if msg.rank < *r {
                    *r = msg.rank;
                }
            })
            .or_insert(msg.rank);
    }

    for msg in msgs.iter_mut() {
        if !msg.blocked {
            if let Some(winner) = best.get(&msg.group) {
                if msg.rank != *winner {
                    msg.blocked = true;
                }
            }
        }
        if msg.blocked {
            if let Some(edge) = msg.edge {
                blocked.0.insert(edge);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// EnterState / ExitState entity events
// ---------------------------------------------------------------------------

/// Triggered on a state entity when it is entered, inside the schedule loop
/// in [`EntryPhase`](crate::GearboxPhase::EntryPhase). Ancestors are entered
/// before their descendants. Use `On<EnterState>` observers on state
/// entities to react.
///
/// For systems, prefer querying [`Added<Active>`](crate::components::Active)
/// from `EntryPhase` (or from `Update` after `GearboxSet` for the frame's
/// net result).
#[derive(EntityEvent, Clone, Debug)]
pub struct EnterState {
    #[event_target]
    pub state: Entity,
    pub machine: Entity,
}

/// Triggered on a state entity when it is exited, inside the schedule loop in
/// [`ExitPhase`](crate::GearboxPhase::ExitPhase). Descendants are exited
/// before their ancestors. Use `On<ExitState>` observers on state entities to
/// react.
///
/// For systems, prefer `RemovedComponents<Active>` from `ExitPhase` (or from
/// `Update` after `GearboxSet` for the frame's net result).
#[derive(EntityEvent, Clone, Debug)]
pub struct ExitState {
    #[event_target]
    pub state: Entity,
    pub machine: Entity,
}

/// Triggers [`ExitState`] for every state that lost [`Active`] in this
/// iteration's `TransitionPhase`, deepest first. A state passed through
/// within one frame gets its exit event in the iteration that leaves it.
pub(crate) fn fire_exit_events(
    mut removed: RemovedComponents<Active>,
    q_substate_of: Query<&SubstateOf>,
    q_machine: Query<(), With<StateMachine>>,
    mut commands: Commands,
) {
    let mut exited: Vec<(usize, Entity, Entity)> = removed
        .read()
        .filter_map(|state| {
            let machine = q_substate_of.root_ancestor(state);
            q_machine
                .contains(machine)
                .then(|| (q_substate_of.iter_ancestors(state).count(), state, machine))
        })
        .collect();
    exited.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, state, machine) in exited {
        commands.trigger(ExitState { state, machine });
    }
}

/// Triggers [`EnterState`] for every state that gained [`Active`] in this
/// iteration's `TransitionPhase`, shallowest first.
pub(crate) fn fire_enter_events(
    q_entered: Query<(Entity, &Active), Added<Active>>,
    q_substate_of: Query<&SubstateOf>,
    mut commands: Commands,
) {
    let mut entered: Vec<(usize, Entity, Entity)> = q_entered
        .iter()
        .map(|(state, active)| (q_substate_of.iter_ancestors(state).count(), state, active.machine))
        .collect();
    entered.sort_by_key(|e| e.0);
    for (_, state, machine) in entered {
        commands.trigger(EnterState { state, machine });
    }
}

// ---------------------------------------------------------------------------
// Systems (run inside GearboxSchedule)
// ---------------------------------------------------------------------------

/// Resolve all pending transition messages: compute exits, entries, update
/// StateMachine, save history, handle ResetEdge, and insert/remove [`Active`].
///
/// Skips messages marked `blocked` by blocker systems. Increments
/// [`PendingCount`] for each non-blocked transition it processes so the
/// outer loop knows work was done.
pub(crate) fn resolve_transitions(
    mut reader: MessageReader<TransitionMessage>,
    mut q_machine: Query<&mut StateMachine>,
    q_substates: Query<&Substates>,
    q_substate_of: Query<&SubstateOf>,
    q_initial: Query<&InitialState>,
    q_history: Query<&History>,
    mut q_history_state: Query<&mut HistoryState>,
    q_edge_kind: Query<&EdgeKind>,
    q_reset_edge: Query<&ResetEdge>,
    mut pending: ResMut<PendingCount>,
    mut commands: Commands,
) {
    for msg in reader.read() {
        if msg.blocked {
            continue;
        }
        let Ok(mut machine) = q_machine.get_mut(msg.machine) else {
            continue;
        };

        // --- Initialization (no active leaves yet) ---
        if machine.active_leaves.is_empty() {
            let leaves = get_all_leaf_states(
                msg.target,
                &q_initial,
                &q_substates,
                &q_history,
                &q_history_state,
            );
            machine.active_leaves.extend(&leaves);
            machine.active =
                compute_active_from_leaves(&machine.active_leaves, &q_substate_of);
            machine.active.insert(msg.machine);

            // Insert Active on all newly active states
            for &state in &machine.active {
                commands.entity(state).insert(Active { machine: msg.machine });
            }
            pending.0 += 1;
            continue;
        }

        // --- Normal transition ---

        // Skip if the source is no longer active.
        if !machine.active.contains(&msg.source) {
            continue;
        }

        let exit_path = path_to_root(msg.source, &q_substate_of);
        let enter_path = path_to_root(msg.target, &q_substate_of);

        // LCA
        let mut lca_depth = exit_path
            .iter()
            .rev()
            .zip(enter_path.iter().rev())
            .take_while(|(a, b)| a == b)
            .count();

        // EdgeKind::Internal: don't exit/re-enter the LCA itself.
        // EdgeKind::External (default): if source IS the LCA, bump lca_depth
        // down so the source is exited and re-entered.
        let is_internal = msg
            .edge
            .and_then(|e| q_edge_kind.get(e).ok())
            .map(|k| matches!(k, EdgeKind::Internal))
            .unwrap_or(false);

        if !is_internal {
            let lca_entity = if lca_depth > 0 {
                Some(exit_path[exit_path.len() - lca_depth])
            } else {
                None
            };
            if lca_entity == Some(msg.source) {
                lca_depth = lca_depth.saturating_sub(1);
            }
        }

        // Collect the set of ancestors being exited.
        let exit_upto = exit_path.len() - lca_depth;
        let exited_ancestors: HashSet<Entity> =
            exit_path[..exit_upto].iter().copied().collect();

        // Collect exited leaves BEFORE modifying active_leaves (needed for history).
        let exited_leaves: Vec<Entity> = machine
            .active_leaves
            .iter()
            .copied()
            .filter(|leaf| {
                exited_ancestors.contains(leaf)
                    || q_substate_of
                        .iter_ancestors(*leaf)
                        .any(|a| exited_ancestors.contains(&a))
            })
            .collect();

        // Save history for any exited ancestor that has a History component.
        for &ancestor in &exited_ancestors {
            if let Ok(history) = q_history.get(ancestor) {
                let states_to_save = match history {
                    History::Shallow => {
                        let mut saved = HashSet::new();
                        for &leaf in &exited_leaves {
                            let mut prev = leaf;
                            for anc in q_substate_of.iter_ancestors(leaf) {
                                if anc == ancestor {
                                    saved.insert(prev);
                                    break;
                                }
                                prev = anc;
                            }
                        }
                        saved
                    }
                    History::Deep => {
                        exited_leaves
                            .iter()
                            .copied()
                            .filter(|leaf| {
                                *leaf == ancestor
                                    || q_substate_of
                                        .iter_ancestors(*leaf)
                                        .any(|a| a == ancestor)
                            })
                            .collect()
                    }
                };

                if let Ok(mut existing) = q_history_state.get_mut(ancestor) {
                    existing.0 = states_to_save;
                } else {
                    commands
                        .entity(ancestor)
                        .insert(HistoryState(states_to_save));
                }
            }
        }

        // Remove exited leaves.
        for &leaf in &exited_leaves {
            machine.active_leaves.remove(&leaf);
        }

        // Handle ResetEdge: clear history and active state under the reset scope.
        if let Some(edge) = msg.edge {
            if let Ok(reset) = q_reset_edge.get(edge) {
                let reset_roots: Vec<Entity> = match reset.0 {
                    ResetScope::Source => vec![msg.source],
                    ResetScope::Target => vec![msg.target],
                    ResetScope::Both => vec![msg.source, msg.target],
                };
                for &root in &reset_roots {
                    // Clear history under this subtree
                    let mut stack = vec![root];
                    while let Some(e) = stack.pop() {
                        if let Ok(mut hs) = q_history_state.get_mut(e) {
                            hs.0.clear();
                        }
                        if let Ok(children) = q_substates.get(e) {
                            stack.extend(children.into_iter().copied());
                        }
                    }
                    // Remove any remaining active leaves under this root
                    machine.active_leaves.retain(|leaf| {
                        *leaf != root
                            && !q_substate_of
                                .iter_ancestors(*leaf)
                                .any(|a| a == root)
                    });
                }
            }
        }

        // Enter: drill down to leaf from target.
        let new_leaves = get_all_leaf_states(
            msg.target,
            &q_initial,
            &q_substates,
            &q_history,
            &q_history_state,
        );
        machine.active_leaves.extend(new_leaves);

        // Recompute active set.
        let old_active = std::mem::take(&mut machine.active);
        machine.active =
            compute_active_from_leaves(&machine.active_leaves, &q_substate_of);
        machine.active.insert(msg.machine);

        // Build the full exited set: every state that was active before but
        // is no longer active. This catches intermediate states (e.g. a parent
        // with StateComponent) that sit between the exited leaves and the
        // transition source.
        let exited_all: Vec<Entity> = old_active
            .iter()
            .copied()
            .filter(|e| !machine.active.contains(e))
            .collect();

        // Remove Active from exited states
        for &state in &exited_all {
            commands.entity(state).remove::<Active>();
        }

        for &state in &machine.active {
            if !old_active.contains(&state) || exited_all.contains(&state) {
                // New or re-entered (exited then re-added): triggers Added<Active>.
                commands.entity(state).insert(Active { machine: msg.machine });
            } else if state == msg.target
                || (!is_internal
                    && q_substate_of
                        .iter_ancestors(state)
                        .any(|a| a == msg.target))
            {
                commands.entity(state).insert(Active { machine: msg.machine });
            }
        }

        pending.0 += 1;
    }
}

/// Propose every undelayed [`AlwaysEdge`] on states that were just entered or
/// re-entered (`Changed<Active>`). Candidates within one parallel region share
/// a group, ranked deeper-state-first and then by [`Transitions`] order, so
/// exactly one always-edge per region survives [`select_transitions`] (the
/// XState `always: [ .. ]` list). Fires once per state entry.
///
/// Increments [`PendingCount`] for each candidate it queues.
pub(crate) fn check_always_edges(
    mut writer: MessageWriter<TransitionMessage>,
    mut pending: ResMut<PendingCount>,
    mut groups: ResMut<CandidateGroups>,
    q_active: Query<(Entity, &Active), Changed<Active>>,
    q_transitions: Query<&Transitions>,
    q_always: Query<(), With<AlwaysEdge>>,
    q_target: Query<&Target>,
    q_source: Query<&Source>,
    q_delay: Query<(), With<Delay>>,
    q_substate_of: Query<&SubstateOf>,
    q_initial: Query<&InitialState>,
    q_children: Query<&Substates>,
) {
    let mut region_groups: HashMap<(Entity, Entity), u64> = HashMap::new();

    for (state, active) in &q_active {
        let Ok(transitions) = q_transitions.get(state) else {
            continue;
        };
        let machine = active.machine;
        let mut group = None;
        let depth = depth_rank(state, &q_substate_of);

        for (index, &edge) in transitions.into_iter().enumerate() {
            if !q_always.contains(edge) || q_delay.contains(edge) {
                continue;
            }
            let Ok(target) = q_target.get(edge) else {
                continue;
            };
            let group = *group.get_or_insert_with(|| {
                let region = region_root(state, machine, &q_substate_of, &q_initial, &q_children);
                *region_groups
                    .entry((machine, region))
                    .or_insert_with(|| groups.next())
            });
            let source = q_source.get(edge).map(|s| s.0).unwrap_or(state);

            writer.write(TransitionMessage {
                machine,
                source,
                target: target.0,
                edge: Some(edge),
                blocked: false,
                group,
                rank: (depth, index as u32),
            });
            pending.0 += 1;
        }
    }
}
