use bevy::platform::collections::HashSet;
use bevy::prelude::*;

use crate::components::*;
use crate::history::*;

pub(crate) fn path_to_root(start: Entity, q_substate_of: &Query<&SubstateOf>) -> Vec<Entity> {
    let mut path = vec![start];
    path.extend(q_substate_of.iter_ancestors(start));
    path
}

/// Depth-based priority for a candidate transition: deeper states rank first,
/// so a leaf's edge beats its ancestor's for the same trigger.
pub(crate) fn depth_rank(state: Entity, q_substate_of: &Query<&SubstateOf>) -> u32 {
    u32::MAX - q_substate_of.iter_ancestors(state).count() as u32
}

/// A parallel state has substates but no [`InitialState`]: every child is a
/// region and all of them are active together.
pub(crate) fn is_parallel(
    state: Entity,
    q_initial: &Query<&InitialState>,
    q_children: &Query<&Substates>,
) -> bool {
    let has_children = q_children
        .get(state)
        .ok()
        .map(|c| c.into_iter().next().is_some())
        .unwrap_or(false);
    has_children && !q_initial.contains(state)
}

/// `true` if `state` is `ancestor` or sits somewhere below it.
pub(crate) fn is_self_or_descendant(
    state: Entity,
    ancestor: Entity,
    q_substate_of: &Query<&SubstateOf>,
) -> bool {
    state == ancestor || q_substate_of.iter_ancestors(state).any(|a| a == ancestor)
}

/// SCXML "in a final state": an active [`TerminalState`] leaf, a sequential
/// state whose active child is a `TerminalState`, or a parallel state whose
/// every region is in a final state.
pub(crate) fn is_in_final_state(
    state: Entity,
    q_active: &Query<(), With<Active>>,
    q_terminal: &Query<(), With<TerminalState>>,
    q_initial: &Query<&InitialState>,
    q_children: &Query<&Substates>,
) -> bool {
    if !q_active.contains(state) {
        return false;
    }
    let children: Vec<Entity> = q_children
        .get(state)
        .map(|c| c.into_iter().copied().collect())
        .unwrap_or_default();
    if children.is_empty() {
        q_terminal.contains(state)
    } else if q_initial.contains(state) {
        children
            .iter()
            .any(|&c| q_active.contains(c) && q_terminal.contains(c))
    } else {
        children
            .iter()
            .all(|&c| is_in_final_state(c, q_active, q_terminal, q_initial, q_children))
    }
}

/// The root of the parallel region containing `state`: the child of the
/// nearest parallel ancestor. Returns `machine` when no ancestor is parallel,
/// so a sequential machine is a single region.
pub(crate) fn region_root(
    state: Entity,
    machine: Entity,
    q_substate_of: &Query<&SubstateOf>,
    q_initial: &Query<&InitialState>,
    q_children: &Query<&Substates>,
) -> Entity {
    let mut previous = state;
    for ancestor in q_substate_of.iter_ancestors(state) {
        if is_parallel(ancestor, q_initial, q_children) {
            return previous;
        }
        previous = ancestor;
        if ancestor == machine {
            break;
        }
    }
    machine
}

pub(crate) fn get_all_leaf_states(
    start: Entity,
    q_initial: &Query<&InitialState>,
    q_children: &Query<&Substates>,
    q_history: &Query<&History>,
    q_history_state: &Query<&mut HistoryState>,
) -> HashSet<Entity> {
    let mut leaves = HashSet::new();
    let mut stack = vec![start];

    while let Some(entity) = stack.pop() {
        // 1) History takes precedence
        if let (Ok(history), Ok(hs)) = (q_history.get(entity), q_history_state.get(entity))
        {
            if !hs.0.is_empty() {
                match history {
                    History::Shallow => {
                        // Saved states are the immediate children; continue
                        // drilling from them.
                        for &saved in &hs.0 {
                            stack.push(saved);
                        }
                        continue;
                    }
                    History::Deep => {
                        // Saved states are the exact leaves; no further drilling.
                        leaves.extend(&hs.0);
                        continue;
                    }
                }
            }
        }

        // 2) InitialState → drill into that child
        if let Ok(initial) = q_initial.get(entity) {
            stack.push(initial.0);
            continue;
        }

        // 3) Parallel parent (has children, no InitialState) → explore all
        if let Ok(children) = q_children.get(entity) {
            let children_vec: Vec<_> = children.into_iter().copied().collect();
            if !children_vec.is_empty() {
                stack.extend(children_vec);
                continue;
            }
        }

        // 4) Leaf
        leaves.insert(entity);
    }
    leaves
}

pub(crate) fn compute_active_from_leaves(
    leaves: &HashSet<Entity>,
    q_substate_of: &Query<&SubstateOf>,
) -> HashSet<Entity> {
    let mut active = HashSet::new();
    for &leaf in leaves {
        active.insert(leaf);
        for ancestor in q_substate_of.iter_ancestors(leaf) {
            active.insert(ancestor);
        }
    }
    active
}
