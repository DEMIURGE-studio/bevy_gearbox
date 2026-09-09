//! Tests for multiple independent state machines in the same world:
//! - Machines have independent state
//! - Messages are scoped to their target machine
//! - Machines can have different topologies
//! - Simultaneous AlwaysEdge transitions on different machines

use bevy::prelude::*;
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

#[derive(Message, Clone, Reflect)]
struct Trigger {
    machine: Entity,
}

impl GearboxMessage for Trigger {
    type Validator = AcceptAll;
    fn target(&self) -> Entity {
        self.machine
    }
}

/// Two machines in the same world should have fully independent state.
#[test]
fn two_machines_independent_state() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();

    let m1 = world.spawn_empty().id();
    let m1_a = world.spawn(SubstateOf(m1)).id();
    let m1_b = world.spawn(SubstateOf(m1)).id();
    world
        .entity_mut(m1)
        .insert((StateMachine::new(), InitialState(m1_a)));

    let m2 = world.spawn_empty().id();
    let m2_x = world.spawn(SubstateOf(m2)).id();
    let _m2_y = world.spawn(SubstateOf(m2)).id();
    world
        .entity_mut(m2)
        .insert((StateMachine::new(), InitialState(m2_x)));

    app.update();

    assert!(app.world().get::<StateMachine>(m1).unwrap().is_active(&m1_a));
    assert!(app.world().get::<StateMachine>(m2).unwrap().is_active(&m2_x));

    // Transition only machine 1
    app.world_mut().write_message(TransitionMessage::new(m1, m1_a, m1_b, None));
    app.update();

    assert!(
        app.world()
            .get::<StateMachine>(m1)
            .unwrap()
            .active_leaves
            .contains(&m1_b)
    );
    assert!(
        app.world()
            .get::<StateMachine>(m2)
            .unwrap()
            .active_leaves
            .contains(&m2_x),
        "Machine 2 should be unaffected"
    );
}

/// A message addressed to machine 1 should not trigger edges on machine 2.
#[test]
fn message_only_affects_target_machine() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_transition::<Trigger>();

    let world = app.world_mut();

    let m1 = world.spawn_empty().id();
    let m1_a = world.spawn(SubstateOf(m1)).id();
    let m1_b = world.spawn(SubstateOf(m1)).id();
    world.spawn((
        Source(m1_a),
        Target(m1_b),
        MessageEdge::<Trigger>::default(),
    ));
    world
        .entity_mut(m1)
        .insert((StateMachine::new(), InitialState(m1_a)));

    let m2 = world.spawn_empty().id();
    let m2_a = world.spawn(SubstateOf(m2)).id();
    let m2_b = world.spawn(SubstateOf(m2)).id();
    world.spawn((
        Source(m2_a),
        Target(m2_b),
        MessageEdge::<Trigger>::default(),
    ));
    world
        .entity_mut(m2)
        .insert((StateMachine::new(), InitialState(m2_a)));

    app.update();

    // Send trigger only for machine 1
    app.world_mut().write_message(Trigger { machine: m1 });
    app.update();

    assert!(
        app.world()
            .get::<StateMachine>(m1)
            .unwrap()
            .active_leaves
            .contains(&m1_b)
    );
    assert!(
        app.world()
            .get::<StateMachine>(m2)
            .unwrap()
            .active_leaves
            .contains(&m2_a),
        "Machine 2 should not react to machine 1's trigger"
    );
}

/// Machines can have completely different topologies (flat, parallel, deep)
/// and coexist without interference.
#[test]
fn machines_with_different_topologies() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();

    // Machine 1: simple flat
    let m1 = world.spawn_empty().id();
    let m1_a = world.spawn(SubstateOf(m1)).id();
    world
        .entity_mut(m1)
        .insert((StateMachine::new(), InitialState(m1_a)));

    // Machine 2: parallel region
    let m2 = world.spawn_empty().id();
    let m2_parallel = world.spawn(SubstateOf(m2)).id();
    let m2_leaf_a = world.spawn(SubstateOf(m2_parallel)).id();
    let m2_leaf_b = world.spawn(SubstateOf(m2_parallel)).id();
    world
        .entity_mut(m2)
        .insert((StateMachine::new(), InitialState(m2_parallel)));

    // Machine 3: deep hierarchy
    let m3 = world.spawn_empty().id();
    let m3_p = world.spawn(SubstateOf(m3)).id();
    let m3_q = world.spawn(SubstateOf(m3_p)).id();
    let m3_leaf = world.spawn(SubstateOf(m3_q)).id();
    world.entity_mut(m3_q).insert(InitialState(m3_leaf));
    world.entity_mut(m3_p).insert(InitialState(m3_q));
    world
        .entity_mut(m3)
        .insert((StateMachine::new(), InitialState(m3_p)));

    app.update();

    // Machine 1: one leaf
    let s1 = app.world().get::<StateMachine>(m1).unwrap();
    assert!(s1.active_leaves.contains(&m1_a));
    assert_eq!(s1.active_leaves.len(), 1);

    // Machine 2: parallel, two leaves
    let s2 = app.world().get::<StateMachine>(m2).unwrap();
    assert!(s2.active_leaves.contains(&m2_leaf_a));
    assert!(s2.active_leaves.contains(&m2_leaf_b));
    assert_eq!(s2.active_leaves.len(), 2);

    // Machine 3: drilled to leaf
    let s3 = app.world().get::<StateMachine>(m3).unwrap();
    assert!(s3.active_leaves.contains(&m3_leaf));
    assert_eq!(s3.active_leaves.len(), 1);
    assert!(s3.active.contains(&m3_p));
    assert!(s3.active.contains(&m3_q));
}

/// AlwaysEdge transitions on multiple machines should all resolve within the
/// same `update()`.
#[test]
fn simultaneous_always_edge_transitions() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();

    let m1 = world.spawn_empty().id();
    let m1_a = world.spawn(SubstateOf(m1)).id();
    let m1_b = world.spawn(SubstateOf(m1)).id();
    world.spawn((Source(m1_a), Target(m1_b), AlwaysEdge));
    world
        .entity_mut(m1)
        .insert((StateMachine::new(), InitialState(m1_a)));

    let m2 = world.spawn_empty().id();
    let m2_a = world.spawn(SubstateOf(m2)).id();
    let m2_b = world.spawn(SubstateOf(m2)).id();
    world.spawn((Source(m2_a), Target(m2_b), AlwaysEdge));
    world
        .entity_mut(m2)
        .insert((StateMachine::new(), InitialState(m2_a)));

    app.update();

    assert!(
        app.world()
            .get::<StateMachine>(m1)
            .unwrap()
            .active_leaves
            .contains(&m1_b)
    );
    assert!(
        app.world()
            .get::<StateMachine>(m2)
            .unwrap()
            .active_leaves
            .contains(&m2_b)
    );
}
