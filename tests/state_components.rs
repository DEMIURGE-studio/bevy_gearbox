//! Tests for StateComponent and StateInactiveComponent behavior:
//! - StateComponent<T> inserts T on machine root when state is entered
//! - StateComponent<T> removes T from machine root when state is exited
//! - StateInactiveComponent<T> removes T on enter and restores on exit
//! - Multiple components on different states
//! - Components on nested states

use bevy::prelude::*;
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

#[derive(Component, Clone, Debug, PartialEq)]
struct Speed(f32);

#[derive(Component, Clone, Debug, PartialEq)]
struct Jumping;

#[derive(Component, Clone, Debug, PartialEq)]
struct CanMove;

/// StateComponent<T> should insert T on the machine root entity when entered.
#[test]
fn state_component_inserted_on_enter() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_state_component::<Speed>();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let a = world
        .spawn((SubstateOf(machine), StateComponent(Speed(5.0))))
        .id();

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(a)));

    app.update();

    assert_eq!(
        app.world().get::<Speed>(machine).map(|s| s.0),
        Some(5.0),
        "Speed should be on machine root after entering A"
    );
}

/// StateComponent<T> should remove T from the machine root when the state is
/// exited.
#[test]
fn state_component_removed_on_exit() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_state_component::<Speed>();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let a = world
        .spawn((SubstateOf(machine), StateComponent(Speed(5.0))))
        .id();
    let b = world.spawn(SubstateOf(machine)).id();

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(a)));

    app.update();
    assert!(app.world().get::<Speed>(machine).is_some());

    // Transition A -> B
    app.world_mut().write_message(TransitionMessage {
        machine,
        source: a,
        target: b,
        edge: None,
        blocked: false,
    });
    app.update();

    assert!(
        app.world().get::<Speed>(machine).is_none(),
        "Speed should be removed when A is exited"
    );
}

/// StateInactiveComponent<T> removes T from the machine root while the state
/// is active and restores it when the state is exited.
#[test]
fn state_inactive_component_removed_on_enter_restored_on_exit() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_state_component::<CanMove>();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let a = world
        .spawn((SubstateOf(machine), StateInactiveComponent(CanMove)))
        .id();
    let b = world.spawn(SubstateOf(machine)).id();

    // Machine starts with CanMove present
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(a), CanMove));

    app.update();

    // While A is active, CanMove should be removed
    assert!(
        app.world().get::<CanMove>(machine).is_none(),
        "CanMove should be removed while A (StateInactiveComponent) is active"
    );

    // Transition A -> B
    app.world_mut().write_message(TransitionMessage {
        machine,
        source: a,
        target: b,
        edge: None,
        blocked: false,
    });
    app.update();

    // After exiting A, CanMove should be restored
    assert!(
        app.world().get::<CanMove>(machine).is_some(),
        "CanMove should be restored after exiting A"
    );
}

/// Different states can each have their own StateComponent. Transitioning
/// between them should swap the component on the machine root.
#[test]
fn different_state_components_swap_on_transition() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_state_component::<Speed>();
    app.register_state_component::<Jumping>();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let running = world
        .spawn((SubstateOf(machine), StateComponent(Speed(3.0))))
        .id();
    let jumping = world
        .spawn((SubstateOf(machine), StateComponent(Jumping)))
        .id();

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(running)));

    app.update();
    assert!(app.world().get::<Speed>(machine).is_some());
    assert!(app.world().get::<Jumping>(machine).is_none());

    // Transition running -> jumping
    app.world_mut().write_message(TransitionMessage {
        machine,
        source: running,
        target: jumping,
        edge: None,
        blocked: false,
    });
    app.update();

    assert!(
        app.world().get::<Speed>(machine).is_none(),
        "Speed should be removed after leaving running state"
    );
    assert!(
        app.world().get::<Jumping>(machine).is_some(),
        "Jumping should be present after entering jumping state"
    );
}

/// Root-sourced transition exits a state with children: the parent state's
/// StateComponent must be removed even when the active leaf is a child
/// substate, not the parent itself.
///
/// Hierarchy:
///   Machine (root)
///   ├── A  [StateComponent(Speed)]
///   │   └── A_child  (InitialState of A — active leaf when A is active)
///   └── B
///
/// Transition: root → B (an any-state edge sourced at the root).
/// Expected: Speed is removed because A is exited. Every state between the
/// exited leaf and the transition source, A included, must lose `Active` so
/// `state_component_exit` fires for it.
#[test]
fn root_sourced_transition_removes_state_component_from_intermediate_parent() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_state_component::<Speed>();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();

    let a = world
        .spawn((SubstateOf(machine), StateComponent(Speed(5.0))))
        .id();
    let a_child = world.spawn(SubstateOf(a)).id();
    let b = world.spawn(SubstateOf(machine)).id();

    world.entity_mut(a).insert(InitialState(a_child));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(a)));

    app.update();

    // A is active (via a_child as leaf), Speed is on the root.
    assert_eq!(
        app.world().get::<Speed>(machine).map(|s| s.0),
        Some(5.0),
        "Speed should be on machine root after entering A"
    );

    // Root-sourced transition: machine → B (simulates an any-state edge).
    app.world_mut().write_message(TransitionMessage {
        machine,
        source: machine,
        target: b,
        edge: None,
        blocked: false,
    });
    app.update();

    assert!(
        app.world().get::<Speed>(machine).is_none(),
        "Speed should be removed when A is exited via a root-sourced transition"
    );
}

/// A StateComponent on a deeply nested state should still apply to the
/// machine root entity (not the intermediate parent).
#[test]
fn state_component_on_nested_state_applies_to_root() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_state_component::<Speed>();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let parent = world.spawn(SubstateOf(machine)).id();
    let child = world
        .spawn((SubstateOf(parent), StateComponent(Speed(1.0))))
        .id();

    world.entity_mut(parent).insert(InitialState(child));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(parent)));

    app.update();

    assert_eq!(
        app.world().get::<Speed>(machine).map(|s| s.0),
        Some(1.0),
        "Speed should be on machine root even though the state is nested"
    );
    assert!(
        app.world().get::<Speed>(parent).is_none(),
        "Speed should NOT be on intermediate parent"
    );
}
