//! Tests for shallow and deep history behavior:
//! - First entry uses InitialState (no history yet)
//! - Shallow history restores the immediate child, then drills down
//! - Deep history restores the exact leaf configuration
//! - ResetEdge clears history before re-entry

use bevy::prelude::*;
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

/// On first entry, a state with History should use InitialState (there is no
/// saved history yet).
#[test]
fn first_entry_uses_initial_state() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let p = world.spawn((SubstateOf(machine), History::Shallow)).id();
    let a = world.spawn(SubstateOf(p)).id();
    let _b = world.spawn(SubstateOf(p)).id();

    world.entity_mut(p).insert(InitialState(a));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(p)));

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&a),
        "First entry should use InitialState (A)"
    );
}

/// Shallow history: re-entering a parent restores the immediate child that
/// was active when last exited, then drills down from there.
///
/// ```text
/// machine
///   P (History::Shallow, InitialState=A)
///     A
///     B
///   D
/// ```
///
/// Sequence: init P/A → A→B → P→D → D→P  ⇒  should restore B.
#[test]
fn shallow_history_restores_immediate_child() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let p = world.spawn((SubstateOf(machine), History::Shallow)).id();
    let a = world.spawn(SubstateOf(p)).id();
    let b = world.spawn(SubstateOf(p)).id();
    let d = world.spawn(SubstateOf(machine)).id();

    world.entity_mut(p).insert(InitialState(a));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(p)));

    // Init → P/A
    app.update();
    assert!(
        app.world()
            .get::<StateMachine>(machine)
            .unwrap()
            .active_leaves
            .contains(&a)
    );

    // A → B
    app.world_mut().write_message(TransitionMessage::new(machine, a, b, None));
    app.update();

    // P → D (saves shallow history: B)
    app.world_mut().write_message(TransitionMessage::new(machine, p, d, None));
    app.update();

    // D → P (shallow history restores B)
    app.world_mut().write_message(TransitionMessage::new(machine, d, p, None));
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&b),
        "Shallow history should restore B, got leaves: {:?}",
        state.active_leaves
    );
    assert!(!state.active_leaves.contains(&a));
}

/// Shallow history restores the immediate child, then continues drilling via
/// InitialState.
///
/// ```text
/// machine
///   P (History::Shallow, InitialState=Q)
///     Q (InitialState=X)
///       X
///       Y
///     R (InitialState=Z)
///       Z
///   D
/// ```
///
/// Sequence: init P/Q/X → Q→R (now in R/Z) → P→D → D→P
/// Shallow history saves R (the immediate child of P). Re-entry drills R→Z.
#[test]
fn shallow_history_drills_down_from_restored_child() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let p = world.spawn((SubstateOf(machine), History::Shallow)).id();
    let q = world.spawn(SubstateOf(p)).id();
    let _x = world.spawn(SubstateOf(q)).id();
    let _y = world.spawn(SubstateOf(q)).id();
    let r = world.spawn(SubstateOf(p)).id();
    let z = world.spawn(SubstateOf(r)).id();
    let d = world.spawn(SubstateOf(machine)).id();

    world.entity_mut(q).insert(InitialState(_x));
    world.entity_mut(r).insert(InitialState(z));
    world.entity_mut(p).insert(InitialState(q));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(p)));

    // Init → P/Q/X
    app.update();

    // Q → R (now in P/R/Z)
    app.world_mut().write_message(TransitionMessage::new(machine, q, r, None));
    app.update();
    assert!(
        app.world()
            .get::<StateMachine>(machine)
            .unwrap()
            .active_leaves
            .contains(&z)
    );

    // P → D (saves shallow history: R was the active child of P)
    app.world_mut().write_message(TransitionMessage::new(machine, p, d, None));
    app.update();

    // D → P (restore R, drill to Z)
    app.world_mut().write_message(TransitionMessage::new(machine, d, p, None));
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&z),
        "Should restore R then drill to Z, got leaves: {:?}",
        state.active_leaves
    );
}

/// Deep history restores the exact leaf configuration, skipping InitialState
/// entirely.
///
/// ```text
/// machine
///   P (History::Deep, InitialState=Q)
///     Q (InitialState=X)
///       X
///       Y
///   D
/// ```
///
/// Sequence: init P/Q/X → X→Y → P→D → D→P  ⇒  should restore Y directly.
#[test]
fn deep_history_restores_exact_leaves() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let p = world.spawn((SubstateOf(machine), History::Deep)).id();
    let q = world.spawn(SubstateOf(p)).id();
    let x = world.spawn(SubstateOf(q)).id();
    let y = world.spawn(SubstateOf(q)).id();
    let d = world.spawn(SubstateOf(machine)).id();

    world.entity_mut(q).insert(InitialState(x));
    world.entity_mut(p).insert(InitialState(q));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(p)));

    // Init → P/Q/X
    app.update();

    // X → Y
    app.world_mut().write_message(TransitionMessage::new(machine, x, y, None));
    app.update();

    // P → D
    app.world_mut().write_message(TransitionMessage::new(machine, p, d, None));
    app.update();

    // D → P (deep history restores Y)
    app.world_mut().write_message(TransitionMessage::new(machine, d, p, None));
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&y),
        "Deep history should restore Y directly, got leaves: {:?}",
        state.active_leaves
    );
    assert!(!state.active_leaves.contains(&x));
}

/// ResetEdge(Target) clears all history under the target, so re-entry falls
/// back to InitialState.
#[test]
fn reset_edge_clears_history_before_entry() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let p = world.spawn((SubstateOf(machine), History::Deep)).id();
    let q = world.spawn(SubstateOf(p)).id();
    let x = world.spawn(SubstateOf(q)).id();
    let y = world.spawn(SubstateOf(q)).id();
    let d = world.spawn(SubstateOf(machine)).id();

    world.entity_mut(q).insert(InitialState(x));
    world.entity_mut(p).insert(InitialState(q));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(p)));

    let reset_edge = world
        .spawn((Source(d), Target(p), ResetEdge(ResetScope::Target)))
        .id();

    // Init → P/Q/X
    app.update();

    // X → Y
    app.world_mut().write_message(TransitionMessage::new(machine, x, y, None));
    app.update();

    // P → D (saves deep history: Y)
    app.world_mut().write_message(TransitionMessage::new(machine, p, d, None));
    app.update();

    // D → P via reset edge (clears history → falls back to InitialState X)
    app.world_mut().write_message(TransitionMessage::new(machine, d, p, Some(reset_edge)));
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&x),
        "Reset should clear history, restoring to InitialState X, got leaves: {:?}",
        state.active_leaves
    );
    assert!(!state.active_leaves.contains(&y));
}
