use bevy::{ecs::component::Mutable, prelude::*};

use crate::{EnterState, ExitState, SubstateOf};

/// A component that when added to a state entity, will insert the contained component
/// `T` into the state machine's root entity when this state is entered.
#[derive(Component, Reflect, Clone)]
#[reflect(Component)]
pub struct StateComponent<T: Component + Reflect + Clone>(pub T);

/// A component that when added to a state entity, will remove the component type `T`
/// from the state machine's root entity when this state is entered, and restore
/// the stored value when the state is exited.
#[derive(Component, Reflect, Clone)]
#[reflect(Component)]
pub struct StateInactiveComponent<T: Component + Reflect + Clone>(pub T);

/// A generic system that adds a component `T` to the state machine's root entity
/// when a state with `StateComponent<T>` is entered.
pub fn state_component_enter<T: Component<Mutability = Mutable> + Reflect + Clone>(
    enter_state: On<EnterState>,
    q_state_component: Query<&StateComponent<T>>,
    q_substate_of: Query<&SubstateOf>,
    mut commands: Commands,
) {
    let entered_state = enter_state.target;
    let Ok(insert_component) = q_state_component.get(entered_state) else {
        return;
    };

    let root_entity = q_substate_of.root_ancestor(entered_state);

    if root_entity != entered_state {
        commands.entity(root_entity).insert(insert_component.0.clone());
    }
}

/// A generic system that removes a component `T` from the state machine's root entity
/// when a state with `StateComponent<T>` is exited.
pub fn state_component_exit<T: Component + Reflect + Clone>(
    exit_state: On<ExitState>,
    q_state_component: Query<&StateComponent<T>>,
    q_substate_of: Query<&SubstateOf>,
    mut commands: Commands,
) {
    let exited_state = exit_state.target;
    if !q_state_component.contains(exited_state) {
        return;
    };

    let root_entity = q_substate_of.root_ancestor(exited_state);

    if root_entity != exited_state {
        commands.entity(root_entity).try_remove::<T>();
    }
}

/// A generic system that removes a component `T` from the state machine's root entity
/// when a state with `StateInactiveComponent<T>` is entered.
pub fn state_inactive_component_enter<T: Component + Reflect + Clone>(
    enter_state: On<EnterState>,
    q_state_inactive_component: Query<&StateInactiveComponent<T>>,
    q_substate_of: Query<&SubstateOf>,
    mut commands: Commands,
) {
    let entered_state = enter_state.target;
    if !q_state_inactive_component.contains(entered_state) {
        return;
    };

    let root_entity = q_substate_of.root_ancestor(entered_state);

    if root_entity != entered_state {
        commands.entity(root_entity).try_remove::<T>();
    }
}

/// A generic system that restores a component `T` to the state machine's root entity
/// when a state with `StateInactiveComponent<T>` is exited, using the stored clone.
pub fn state_inactive_component_exit<T: Component + Reflect + Clone>(
    exit_state: On<ExitState>,
    q_state_inactive_component: Query<&StateInactiveComponent<T>>,
    q_substate_of: Query<&SubstateOf>,
    mut commands: Commands,
) {
    let exited_state = exit_state.target;
    let Ok(remove_component) = q_state_inactive_component.get(exited_state) else {
        return;
    };

    let root_entity = q_substate_of.root_ancestor(exited_state);

    if root_entity != exited_state {
        commands.entity(root_entity).insert(remove_component.0.clone());
    }
}

/// Event to reset a subtree rooted at the target entity.
#[derive(EntityEvent, Reflect)]
pub struct Reset { #[event_target] pub target: Entity }

impl Reset {
    pub fn new(entity: Entity) -> Self {
        Self { target: entity }
    }
}