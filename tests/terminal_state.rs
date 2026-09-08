//! Tests for `TerminalState` and the `Done` message flow. `Done` can reach
//! a parent's `MessageEdge<Done>` from three places:
//!
//! 1. Internally, from `emit_terminal_done` when a `TerminalState` gains
//!    `Active`; delivered within the same frame's schedule loop.
//! 2. Externally, from a user system running in `Update` after `GearboxSet`
//!    (a counter that writes `Done` when it exhausts); picked up by the next
//!    frame's schedule.
//! 3. The same, in a deeply nested topology: root → Invoking → Repeater →
//!    {Idle, Fire}, with a delayed Fire → Repeater bounce.
//!
//! They pin the schedule ordering of `message_edge_listener`,
//! `tick_delay_timers`, and the `Done` path.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

fn make_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    // Deterministic clock so delay tests can control when timers fire.
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(16)));
    app
}

/// Baseline: a `TerminalState` child of `Working` should emit `Done` when
/// entered, and `Working -[MessageEdge<Done>]-> Finished` should fire in the
/// same `app.update()` call (the edge-check phase runs inside the per-frame
/// loop).
///
/// ```text
/// machine
/// ├── Working (InitialState of machine)
/// │   └── Complete (InitialState of Working, TerminalState)
/// └── Finished
/// ```
#[test]
fn terminal_state_emits_done_and_parent_transitions_same_frame() {
    let mut app = make_app();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let working = world.spawn(SubstateOf(machine)).id();
    let complete = world.spawn((SubstateOf(working), TerminalState)).id();
    let finished = world.spawn(SubstateOf(machine)).id();

    world.spawn((
        Source(working),
        Target(finished),
        MessageEdge::<Done>::default(),
    ));

    world.entity_mut(working).insert(InitialState(complete));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(working)));

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&finished),
        "Machine should have reached Finished after terminal Done fired, \
         got leaves: {:?}",
        state.active_leaves
    );
    assert!(!state.active_leaves.contains(&complete));
    assert!(!state.active_leaves.contains(&working));
}

/// Control test: a CUSTOM message type (not `Done`) written from outside the
/// schedule between `app.update()` calls. If this passes but the `Done`
/// variant below fails, the bug is specific to `Done` registration. If this
/// also fails, the issue is general to the schedule-phase listener path.
#[test]
fn external_custom_message_fires_transition_next_frame() {
    #[derive(Message, Clone, Reflect)]
    struct Finish {
        machine: Entity,
    }
    impl GearboxMessage for Finish {
        type Validator = AcceptAll;
        fn target(&self) -> Entity {
            self.machine
        }
    }

    let mut app = make_app();
    app.register_transition::<Finish>();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let invoking = world.spawn(SubstateOf(machine)).id();
    let cooldown = world.spawn(SubstateOf(machine)).id();

    world.spawn((
        Source(invoking),
        Target(cooldown),
        MessageEdge::<Finish>::default(),
    ));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(invoking)));

    app.update();
    assert!(app
        .world()
        .get::<StateMachine>(machine)
        .unwrap()
        .is_active(&invoking));

    app.world_mut().write_message(Finish { machine: invoking });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.is_active(&cooldown),
        "Custom message Finish should fire Invoking → Cooldown, got leaves: {:?}",
        state.active_leaves
    );
}

/// External writer: a `Done` message written from outside the schedule
/// (a user system in `Update` after `GearboxSet`) must fire the parent's
/// `Done` edge on the next update.
///
/// ```text
/// machine
/// ├── Invoking (InitialState of machine)
/// └── Cooldown
/// ```
///
/// The test manually writes `Done::new(invoking)` from a system in `Update`
/// after `GearboxSet`, then calls `app.update()` again and expects the
/// machine to be in `Cooldown`.
#[test]
fn external_done_writer_fires_parent_transition_next_frame() {
    let mut app = make_app();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let invoking = world.spawn(SubstateOf(machine)).id();
    let cooldown = world.spawn(SubstateOf(machine)).id();

    world.spawn((
        Source(invoking),
        Target(cooldown),
        MessageEdge::<Done>::default(),
    ));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(invoking)));

    // Frame 1: initialise — machine enters Invoking.
    app.update();
    assert!(
        app.world()
            .get::<StateMachine>(machine)
            .unwrap()
            .active_leaves
            .contains(&invoking),
        "Should have initialised into Invoking"
    );

    // Simulate the repeater: an external system writes Done to the parent
    // state from outside the schedule. We do this directly on the world
    // rather than via a Bevy system — the buffering semantics are the same.
    app.world_mut().write_message(Done::new(invoking));

    // Frame 2: the next schedule run should read the buffered Done and
    // fire Invoking → Cooldown.
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&cooldown),
        "Done written externally should trigger Invoking → Cooldown next \
         frame, got leaves: {:?}",
        state.active_leaves
    );
    assert!(!state.active_leaves.contains(&invoking));
}

/// External writer with a DEEP leaf: the active leaf is nested three levels
/// below `Invoking`, mirroring the repeater topology (Invoking →
/// Repeater → Idle). `message_edge_listener<Done>` must walk up from the
/// leaf to reach `Invoking` and find the `Done` edge.
///
/// ```text
/// machine
/// ├── Invoking
/// │   └── Repeater
/// │       ├── Idle (InitialState)
/// │       └── Fire
/// └── Cooldown
/// ```
#[test]
fn external_done_writer_walks_up_from_deep_leaf() {
    let mut app = make_app();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let invoking = world.spawn(SubstateOf(machine)).id();
    let repeater = world.spawn(SubstateOf(invoking)).id();
    let idle = world.spawn(SubstateOf(repeater)).id();
    let _fire = world.spawn(SubstateOf(repeater)).id();
    let cooldown = world.spawn(SubstateOf(machine)).id();

    world.spawn((
        Source(invoking),
        Target(cooldown),
        MessageEdge::<Done>::default(),
    ));

    world.entity_mut(repeater).insert(InitialState(idle));
    world.entity_mut(invoking).insert(InitialState(repeater));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(invoking)));

    app.update();

    // Active leaf should be Idle; Invoking, Repeater, Idle all active.
    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&idle));
    assert!(state.is_active(&invoking));
    assert!(state.is_active(&repeater));

    // External writer fires Done targeting Invoking (mirrors the repeater
    // writing `Done::new(parent)` where parent = Invoking).
    app.world_mut().write_message(Done::new(invoking));

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&cooldown),
        "Done from deep leaf ancestor walk should transition to Cooldown, \
         got leaves: {:?}",
        state.active_leaves
    );
    assert!(!state.active_leaves.contains(&idle));
    assert!(!state.is_active(&invoking));
    assert!(!state.is_active(&repeater));
}

/// Delayed bounce cycle: `A -(always, delay)-> B -(always, delay)-> A`. Over
/// many frames with time advancing, this should cycle back and forth. This
/// directly exercises the `tick_delay_timers` position (before the schedule
/// loop vs. after) that the user flagged as suspicious.
#[test]
fn delayed_bounce_cycle_progresses() {
    let mut app = make_app();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let a = world.spawn(SubstateOf(machine)).id();
    let b = world.spawn(SubstateOf(machine)).id();

    world.spawn((
        Source(a),
        Target(b),
        AlwaysEdge,
        Delay::new(Duration::from_millis(50)),
    ));
    world.spawn((
        Source(b),
        Target(a),
        AlwaysEdge,
        Delay::new(Duration::from_millis(50)),
    ));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(a)));

    // Initial update: enter A, start its delay timer.
    app.update();
    assert!(
        app.world()
            .get::<StateMachine>(machine)
            .unwrap()
            .is_active(&a),
        "Should start in A"
    );

    // Pump frames at 16ms each. After ~4 frames (~64ms) we should be in B;
    // after another ~4 frames (~128ms) we should be back in A.
    for _ in 0..5 {
        app.update();
    }
    assert!(
        app.world()
            .get::<StateMachine>(machine)
            .unwrap()
            .is_active(&b),
        "After ~80ms we should have bounced into B, got active: {:?}",
        app.world().get::<StateMachine>(machine).unwrap().active
    );

    for _ in 0..5 {
        app.update();
    }
    assert!(
        app.world()
            .get::<StateMachine>(machine)
            .unwrap()
            .is_active(&a),
        "After another ~80ms we should have bounced back into A"
    );
}

/// Repeater scenario: a counter system (running in `Update` after
/// `GearboxSet`) watches for
/// `Changed<Active>` on the `Repeater` state. On the first N "re-entries"
/// (triggered by the Fire → Repeater delayed bounce), it does nothing; on
/// the Nth entry with `remaining == 0` it writes `Done` to `Invoking`.
///
/// Expected behaviour: the machine cycles Idle → Fire → Repeater (bounce) N
/// times and then transitions Invoking → Cooldown.
#[test]
fn repeater_topology_with_external_done_writer_exhausts_and_exits() {
    #[derive(Component, Default)]
    struct TestRepeater {
        remaining: u32,
    }

    #[derive(Message, Clone, Reflect)]
    struct FireIt {
        machine: Entity,
    }
    impl GearboxMessage for FireIt {
        type Validator = AcceptAll;
        fn target(&self) -> Entity {
            self.machine
        }
    }

    fn repeater_driver(
        mut q_changed: Query<
            (Entity, Ref<Active>, &mut TestRepeater, &SubstateOf),
            Changed<Active>,
        >,
        mut fire_writer: MessageWriter<FireIt>,
        mut done_writer: MessageWriter<Done>,
    ) {
        for (entity, active_ref, mut rep, parent) in &mut q_changed {
            if active_ref.is_added() {
                // Initial entry: fire once.
                fire_writer.write(FireIt { machine: entity });
            } else if rep.remaining > 0 {
                // Re-entry from the bounce: fire and decrement.
                rep.remaining -= 1;
                fire_writer.write(FireIt { machine: entity });
            } else {
                // Exhausted — tell the parent we're done.
                done_writer.write(Done::new(parent.0));
            }
        }
    }

    let mut app = make_app();
    app.register_transition::<FireIt>();
    app.add_systems(Update, repeater_driver.after(GearboxSet));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let invoking = world.spawn(SubstateOf(machine)).id();
    let repeater = world
        .spawn((SubstateOf(invoking), TestRepeater { remaining: 2 }))
        .id();
    let idle = world.spawn(SubstateOf(repeater)).id();
    let fire = world.spawn(SubstateOf(repeater)).id();
    let cooldown = world.spawn(SubstateOf(machine)).id();

    // Idle → Fire on FireIt
    world.spawn((
        Source(idle),
        Target(fire),
        MessageEdge::<FireIt>::default(),
    ));
    // Fire → Repeater (bounce) after a short delay
    world.spawn((
        Source(fire),
        Target(repeater),
        AlwaysEdge,
        Delay::new(Duration::from_millis(30)),
    ));
    // Invoking → Cooldown on Done (emitted by the driver)
    world.spawn((
        Source(invoking),
        Target(cooldown),
        MessageEdge::<Done>::default(),
    ));

    world.entity_mut(repeater).insert(InitialState(idle));
    world.entity_mut(invoking).insert(InitialState(repeater));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(invoking)));

    // Pump enough frames for 3 cycles of 30ms each (initial + 2 re-entries).
    // At 16ms per frame, that's ~12 frames minimum; give a generous budget.
    for _ in 0..30 {
        app.update();
        if app
            .world()
            .get::<StateMachine>(machine)
            .map(|s| s.is_active(&cooldown))
            .unwrap_or(false)
        {
            break;
        }
    }

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.is_active(&cooldown),
        "Machine should have reached Cooldown after repeater exhausted, \
         got active: {:?}",
        state.active
    );
    assert!(!state.is_active(&invoking));
    assert!(!state.is_active(&repeater));
}
