use bevy::platform::collections::HashSet;
use bevy::prelude::*;

/// Enables history behavior for a state. When a state with this component is
/// exited and later re-entered, it restores previously active substates
/// instead of following [`InitialState`](crate::InitialState).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum History {
    /// Remember only the direct children that were active when last exited.
    /// On re-entry, restore those children and follow normal drill-down from there.
    #[default]
    Shallow,
    /// Remember the entire set of active leaves under this state.
    /// On re-entry, restore the exact leaf configuration.
    Deep,
}

/// Stores the previously active states for history restoration.
/// Automatically managed by [`resolve_transitions`](crate::resolve::resolve_transitions).
#[derive(Component, Default, Debug)]
pub struct HistoryState(pub HashSet<Entity>);
