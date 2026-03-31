use bevy::ecs::component::Mutable;
use bevy::prelude::*;

use crate::components::Active;

/// When added to a state entity, inserts `T` on the machine root when this
/// state is entered and removes it when this state is exited.
#[derive(Component, Clone)]
pub struct StateComponent<T: Component + Clone>(pub T);

/// When added to a state entity, removes `T` from the machine root when this
/// state is entered and restores the stored clone when this state is exited.
#[derive(Component, Clone)]
pub struct StateInactiveComponent<T: Component + Clone>(pub T);

/// Insert `T` on the machine root when a state with `StateComponent<T>` is entered.
pub fn state_component_enter<T: Component<Mutability = Mutable> + Clone>(
    q_entered: Query<(&Active, &StateComponent<T>), Added<Active>>,
    mut commands: Commands,
) {
    for (active, sc) in &q_entered {
        commands.entity(active.machine).insert(sc.0.clone());
    }
}

/// Remove `T` from the machine root when a state with `StateComponent<T>` is exited.
pub fn state_component_exit<T: Component + Clone>(
    mut removed: RemovedComponents<Active>,
    q_state_comp: Query<(), With<StateComponent<T>>>,
    q_substate_of: Query<&crate::components::SubstateOf>,
    mut commands: Commands,
) {
    for entity in removed.read() {
        if !q_state_comp.contains(entity) {
            continue;
        }
        let machine = q_substate_of.root_ancestor(entity);
        commands.entity(machine).try_remove::<T>();
    }
}

/// Remove `T` from the machine root when a state with `StateInactiveComponent<T>` is entered.
pub fn state_inactive_component_enter<T: Component + Clone>(
    q_entered: Query<&Active, (Added<Active>, With<StateInactiveComponent<T>>)>,
    mut commands: Commands,
) {
    for active in &q_entered {
        commands.entity(active.machine).try_remove::<T>();
    }
}

/// Restore `T` on the machine root when a state with `StateInactiveComponent<T>` is exited.
pub fn state_inactive_component_exit<T: Component + Clone>(
    mut removed: RemovedComponents<Active>,
    q_inactive: Query<&StateInactiveComponent<T>>,
    q_substate_of: Query<&crate::components::SubstateOf>,
    mut commands: Commands,
) {
    for entity in removed.read() {
        let Ok(ic) = q_inactive.get(entity) else {
            continue;
        };
        let machine = q_substate_of.root_ancestor(entity);
        commands.entity(machine).insert(ic.0.clone());
    }
}
