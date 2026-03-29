use bevy::ecs::component::Mutable;
use bevy::prelude::*;

use crate::resolve::TransitionLog;

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
    log: Res<TransitionLog>,
    q_state_comp: Query<&StateComponent<T>>,
    mut commands: Commands,
) {
    for (machine, state) in log.all_entered() {
        let Ok(sc) = q_state_comp.get(state) else {
            continue;
        };
        if machine != state {
            commands.entity(machine).insert(sc.0.clone());
        }
    }
}

/// Remove `T` from the machine root when a state with `StateComponent<T>` is exited.
pub fn state_component_exit<T: Component + Clone>(
    log: Res<TransitionLog>,
    q_state_comp: Query<(), With<StateComponent<T>>>,
    mut commands: Commands,
) {
    for (machine, state) in log.all_exited() {
        if !q_state_comp.contains(state) {
            continue;
        }
        if machine != state {
            commands.entity(machine).try_remove::<T>();
        }
    }
}

/// Remove `T` from the machine root when a state with `StateInactiveComponent<T>` is entered.
pub fn state_inactive_component_enter<T: Component + Clone>(
    log: Res<TransitionLog>,
    q_inactive: Query<(), With<StateInactiveComponent<T>>>,
    mut commands: Commands,
) {
    for (machine, state) in log.all_entered() {
        if !q_inactive.contains(state) {
            continue;
        }
        if machine != state {
            commands.entity(machine).try_remove::<T>();
        }
    }
}

/// Restore `T` on the machine root when a state with `StateInactiveComponent<T>` is exited.
pub fn state_inactive_component_exit<T: Component + Clone>(
    log: Res<TransitionLog>,
    q_inactive: Query<&StateInactiveComponent<T>>,
    mut commands: Commands,
) {
    for (machine, state) in log.all_exited() {
        let Ok(ic) = q_inactive.get(state) else {
            continue;
        };
        if machine != state {
            commands.entity(machine).insert(ic.0.clone());
        }
    }
}
