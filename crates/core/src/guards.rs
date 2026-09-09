//! Built-in guards. A guard is a component on an edge plus a system in
//! [`BlockerPhase`](crate::GearboxPhase::BlockerPhase) that vetoes the
//! candidates carrying it; see [`TransitionMessage`].

use bevy::prelude::*;

use crate::components::{Active, InState, NotInState};
use crate::resolve::TransitionMessage;

/// Vetoes candidates whose edge carries an [`InState`] naming an inactive
/// state, or a [`NotInState`] naming an active one. The configuration checked
/// is the one the transition would leave, as with XState's `stateIn`.
pub fn check_state_guards(
    mut candidates: MessageMutator<TransitionMessage>,
    q_in: Query<&InState>,
    q_not_in: Query<&NotInState>,
    q_active: Query<(), With<Active>>,
) {
    for c in candidates.read() {
        let Some(edge) = c.edge else {
            continue;
        };
        if q_in.get(edge).is_ok_and(|g| !q_active.contains(g.0))
            || q_not_in.get(edge).is_ok_and(|g| q_active.contains(g.0))
        {
            c.blocked = true;
        }
    }
}
