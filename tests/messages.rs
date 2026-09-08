//! Tests for message-driven transitions:
//! - Basic MessageEdge<M> triggering
//! - Wrong message type does not fire
//! - Custom MessageValidator filtering
//! - Deeper states have priority over ancestors
//! - Multiple message types on the same machine
//! - Parallel regions each fire independently per message

use bevy::prelude::*;
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

#[derive(Message, Clone, Reflect)]
struct Attack {
    target: Entity,
}

impl GearboxMessage for Attack {
    type Validator = AcceptAll;
    fn target(&self) -> Entity {
        self.target
    }
}

#[derive(Message, Clone, Reflect)]
struct Dodge {
    target: Entity,
}

impl GearboxMessage for Dodge {
    type Validator = AcceptAll;
    fn target(&self) -> Entity {
        self.target
    }
}

/// A MessageEdge<Attack> should fire when an Attack message is sent.
#[test]
fn message_triggers_transition() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_transition::<Attack>();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let idle = world.spawn(SubstateOf(machine)).id();
    let hit = world.spawn(SubstateOf(machine)).id();

    world.spawn((Source(idle), Target(hit), MessageEdge::<Attack>::default()));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(idle)));

    app.update();
    assert!(
        app.world()
            .get::<StateMachine>(machine)
            .unwrap()
            .is_active(&idle)
    );

    app.world_mut().write_message(Attack { target: machine });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&hit));
    assert!(!state.active_leaves.contains(&idle));
}

/// Sending a Dodge message should NOT fire an Attack-only edge.
#[test]
fn wrong_message_type_does_not_fire() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_transition::<Attack>();
    app.register_transition::<Dodge>();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let idle = world.spawn(SubstateOf(machine)).id();
    let hit = world.spawn(SubstateOf(machine)).id();

    world.spawn((Source(idle), Target(hit), MessageEdge::<Attack>::default()));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(idle)));

    app.update();

    app.world_mut().write_message(Dodge { target: machine });
    app.update();

    assert!(
        app.world()
            .get::<StateMachine>(machine)
            .unwrap()
            .is_active(&idle),
        "Dodge should not trigger an Attack edge"
    );
}

/// A custom MessageValidator can filter messages per-edge.
#[test]
fn message_with_custom_validator() {
    #[derive(Message, Clone, Reflect)]
    struct TypedAttack {
        machine: Entity,
        damage: u32,
    }

    #[derive(Default, Clone)]
    struct HighDamageOnly;

    impl MessageValidator<TypedAttack> for HighDamageOnly {
        fn matches(&self, msg: &TypedAttack) -> bool {
            msg.damage >= 50
        }
    }

    impl GearboxMessage for TypedAttack {
        type Validator = HighDamageOnly;
        fn target(&self) -> Entity {
            self.machine
        }
    }

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_transition::<TypedAttack>();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let idle = world.spawn(SubstateOf(machine)).id();
    let staggered = world.spawn(SubstateOf(machine)).id();

    world.spawn((
        Source(idle),
        Target(staggered),
        MessageEdge::<TypedAttack>::new(Some(HighDamageOnly)),
    ));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(idle)));

    app.update();

    // Low damage — validator rejects
    app.world_mut().write_message(TypedAttack {
        machine,
        damage: 10,
    });
    app.update();
    assert!(
        app.world()
            .get::<StateMachine>(machine)
            .unwrap()
            .is_active(&idle)
    );

    // High damage — validator accepts
    app.world_mut().write_message(TypedAttack {
        machine,
        damage: 100,
    });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&staggered),
        "Should transition on high damage, got leaves: {:?}",
        state.active_leaves
    );
}

/// Deeper (leaf-level) edges have priority over ancestor edges for the same
/// message type.
#[test]
fn deeper_state_has_priority() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_transition::<Attack>();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let parent = world.spawn(SubstateOf(machine)).id();
    let child = world.spawn(SubstateOf(parent)).id();
    let child_target = world.spawn(SubstateOf(parent)).id();
    let parent_target = world.spawn(SubstateOf(machine)).id();

    // Leaf-level edge (higher priority)
    world.spawn((
        Source(child),
        Target(child_target),
        MessageEdge::<Attack>::default(),
    ));
    // Parent-level edge (lower priority)
    world.spawn((
        Source(parent),
        Target(parent_target),
        MessageEdge::<Attack>::default(),
    ));

    world.entity_mut(parent).insert(InitialState(child));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(parent)));

    app.update();

    app.world_mut().write_message(Attack { target: machine });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&child_target),
        "Deeper state should win"
    );
    assert!(!state.active_leaves.contains(&parent_target));
}

/// Two different message types can coexist on the same machine, each routing
/// to different targets.
#[test]
fn multiple_message_types_on_same_machine() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_transition::<Attack>();
    app.register_transition::<Dodge>();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let idle = world.spawn(SubstateOf(machine)).id();
    let hit = world.spawn(SubstateOf(machine)).id();
    let dodging = world.spawn(SubstateOf(machine)).id();

    world.spawn((Source(idle), Target(hit), MessageEdge::<Attack>::default()));
    world.spawn((
        Source(idle),
        Target(dodging),
        MessageEdge::<Dodge>::default(),
    ));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(idle)));

    app.update();

    // Send Dodge — should go to dodging, not hit
    app.world_mut().write_message(Dodge { target: machine });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&dodging));
    assert!(!state.active_leaves.contains(&hit));
}

/// In parallel regions, each region should independently consume the same
/// message type.
///
/// ```text
/// machine -> P (parallel)
///   A --[Attack]--> A2
///   B --[Attack]--> B2
/// ```
#[test]
fn parallel_regions_each_fire_on_same_message() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_transition::<Attack>();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let p = world.spawn(SubstateOf(machine)).id();
    let a = world.spawn(SubstateOf(p)).id();
    let a2 = world.spawn(SubstateOf(p)).id();
    let b = world.spawn(SubstateOf(p)).id();
    let b2 = world.spawn(SubstateOf(p)).id();

    world.spawn((Source(a), Target(a2), MessageEdge::<Attack>::default()));
    world.spawn((Source(b), Target(b2), MessageEdge::<Attack>::default()));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(p)));

    app.update();
    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&a));
    assert!(state.active_leaves.contains(&b));

    app.world_mut().write_message(Attack { target: machine });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&a2),
        "Region A should transition, got leaves: {:?}",
        state.active_leaves
    );
    assert!(
        state.active_leaves.contains(&b2),
        "Region B should transition, got leaves: {:?}",
        state.active_leaves
    );
}

/// A message written the same frame a `StateMachine` is spawned must still
/// fire its transition. `message_edge_listener` runs inside the
/// `GearboxSchedule` loop, so the first iteration resolves the init
/// transition and a later iteration sees the populated machine.
#[test]
fn message_in_same_frame_as_spawn_fires_transition() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_transition::<Attack>();

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let idle = world.spawn(SubstateOf(machine)).id();
    let hit = world.spawn(SubstateOf(machine)).id();

    world.spawn((Source(idle), Target(hit), MessageEdge::<Attack>::default()));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(idle)));

    // Critically: write the message BEFORE any `app.update()` call so the
    // machine is still "newly added" when the message arrives.
    world.write_message(Attack { target: machine });

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&hit),
        "Transition should have fired in the same frame the machine was \
         spawned, got leaves: {:?}",
        state.active_leaves
    );
    assert!(!state.active_leaves.contains(&idle));
}

/// Double registration of the same message type should be idempotent.
#[test]
fn register_transition_dedup() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.register_transition::<Attack>();
    app.register_transition::<Attack>(); // should not panic or double-register

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let a = world.spawn(SubstateOf(machine)).id();
    let b = world.spawn(SubstateOf(machine)).id();

    world.spawn((Source(a), Target(b), MessageEdge::<Attack>::default()));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(a)));

    app.update();
    app.world_mut().write_message(Attack { target: machine });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&b));
}
