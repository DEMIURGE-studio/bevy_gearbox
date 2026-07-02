//! Tests for parallel vs non-parallel region behavior:
//! - Parallel parents (no InitialState) activate all children
//! - Non-parallel parents (with InitialState) activate only one child
//! - Transitions within a region preserve sibling regions
//! - Exiting a parallel parent exits all children
//! - Nested parallel and sequential regions

use bevy::prelude::*;
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

/// A parent with no InitialState is a parallel region — all children become
/// active leaves.
#[test]
fn parallel_region_activates_all_children() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let parallel = world.spawn(SubstateOf(machine)).id();
    let a = world.spawn(SubstateOf(parallel)).id();
    let b = world.spawn(SubstateOf(parallel)).id();
    let c = world.spawn(SubstateOf(parallel)).id();

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(parallel)));

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&a));
    assert!(state.active_leaves.contains(&b));
    assert!(state.active_leaves.contains(&c));
    assert_eq!(state.active_leaves.len(), 3);
    assert!(state.active.contains(&parallel));
}

/// A parent WITH InitialState is a non-parallel region — only the initial
/// child is active.
#[test]
fn non_parallel_region_uses_initial_state_only() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let parent = world.spawn(SubstateOf(machine)).id();
    let a = world.spawn(SubstateOf(parent)).id();
    let _b = world.spawn(SubstateOf(parent)).id();
    let _c = world.spawn(SubstateOf(parent)).id();

    world.entity_mut(parent).insert(InitialState(a));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(parent)));

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&a));
    assert_eq!(state.active_leaves.len(), 1);
}

/// Transitioning within one parallel region must not affect sibling regions.
///
/// ```text
/// machine -> parallel (no init)
///   region_a (init=a1): a1, a2
///   region_b (init=b1): b1, b2
/// ```
///
/// Transition a1 -> a2 should leave b1 untouched.
#[test]
fn transition_within_parallel_region_preserves_siblings() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let parallel = world.spawn(SubstateOf(machine)).id();

    let region_a = world.spawn(SubstateOf(parallel)).id();
    let a1 = world.spawn(SubstateOf(region_a)).id();
    let a2 = world.spawn(SubstateOf(region_a)).id();

    let region_b = world.spawn(SubstateOf(parallel)).id();
    let b1 = world.spawn(SubstateOf(region_b)).id();
    let _b2 = world.spawn(SubstateOf(region_b)).id();

    world.entity_mut(region_a).insert(InitialState(a1));
    world.entity_mut(region_b).insert(InitialState(b1));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(parallel)));

    app.update();
    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&a1));
    assert!(state.active_leaves.contains(&b1));

    // Transition within region A only
    app.world_mut().write_message(TransitionMessage {
        machine,
        source: a1,
        target: a2,
        edge: None,
        blocked: false,
    });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&a2), "a2 should be active");
    assert!(!state.active_leaves.contains(&a1), "a1 should be exited");
    assert!(
        state.active_leaves.contains(&b1),
        "b1 should be unaffected by transition in region A"
    );
}

/// Transitioning out of a parallel parent exits ALL sibling leaves.
#[test]
fn exiting_parallel_parent_exits_all_children() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let parallel = world.spawn(SubstateOf(machine)).id();
    let a = world.spawn(SubstateOf(parallel)).id();
    let b = world.spawn(SubstateOf(parallel)).id();
    let c = world.spawn(SubstateOf(parallel)).id();
    let d = world.spawn(SubstateOf(machine)).id();

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(parallel)));

    app.update();
    assert_eq!(
        app.world()
            .get::<StateMachine>(machine)
            .unwrap()
            .active_leaves
            .len(),
        3
    );

    // Transition parallel -> d
    app.world_mut().write_message(TransitionMessage {
        machine,
        source: parallel,
        target: d,
        edge: None,
        blocked: false,
    });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&d));
    assert_eq!(state.active_leaves.len(), 1);
    assert!(!state.active.contains(&parallel));
    assert!(!state.active.contains(&a));
    assert!(!state.active.contains(&b));
    assert!(!state.active.contains(&c));
}

/// Nested topology: a sequential parent drills into a parallel child.
///
/// ```text
/// machine -> seq (init=parallel) -> parallel (no init) -> [leaf_a, leaf_b]
/// ```
#[test]
fn nested_sequential_then_parallel() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let seq = world.spawn(SubstateOf(machine)).id();
    let parallel = world.spawn(SubstateOf(seq)).id();
    let leaf_a = world.spawn(SubstateOf(parallel)).id();
    let leaf_b = world.spawn(SubstateOf(parallel)).id();

    world.entity_mut(seq).insert(InitialState(parallel));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(seq)));

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&leaf_a));
    assert!(state.active_leaves.contains(&leaf_b));
    assert!(state.active.contains(&seq));
    assert!(state.active.contains(&parallel));
    assert_eq!(state.active_leaves.len(), 2);
}

/// The machine entity itself can be a parallel root: omit `InitialState`
/// from the machine and all of its children become active leaves.
///
/// This is a regression test for the init system: previously
/// `enqueue_machine_init` required `&InitialState` in its query and silently
/// skipped any machine without one, even though a `StateMachine` with no
/// `InitialState` is valid. Self-targeting the init transition resolves the
/// machine entity as a parallel parent through the existing leaf-finder.
#[test]
fn parallel_root_machine_activates_all_children() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let a = world.spawn(SubstateOf(machine)).id();
    let b = world.spawn(SubstateOf(machine)).id();
    let c = world.spawn(SubstateOf(machine)).id();

    // No InitialState on the machine — it's a parallel root.
    world.entity_mut(machine).insert(StateMachine::new());

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&a));
    assert!(state.active_leaves.contains(&b));
    assert!(state.active_leaves.contains(&c));
    assert_eq!(state.active_leaves.len(), 3);
    assert!(state.active.contains(&machine));
}

/// A parallel-root machine whose children are themselves sequential
/// regions: each region's InitialState child should be active, and a
/// transition inside one region should not affect the other.
///
/// ```text
/// machine (no init, parallel root)
/// ├── region_a (init=a1)
/// │   ├── a1
/// │   └── a2
/// └── region_b (init=b1)
///     ├── b1
///     └── b2
/// ```
#[test]
fn parallel_root_machine_with_sequential_subregions() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();

    let region_a = world.spawn(SubstateOf(machine)).id();
    let a1 = world.spawn(SubstateOf(region_a)).id();
    let a2 = world.spawn(SubstateOf(region_a)).id();

    let region_b = world.spawn(SubstateOf(machine)).id();
    let b1 = world.spawn(SubstateOf(region_b)).id();
    let _b2 = world.spawn(SubstateOf(region_b)).id();

    world.entity_mut(region_a).insert(InitialState(a1));
    world.entity_mut(region_b).insert(InitialState(b1));
    // No InitialState on the machine — parallel root.
    world.entity_mut(machine).insert(StateMachine::new());

    app.update();
    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&a1));
    assert!(state.active_leaves.contains(&b1));
    assert_eq!(state.active_leaves.len(), 2);
    assert!(state.active.contains(&machine));
    assert!(state.active.contains(&region_a));
    assert!(state.active.contains(&region_b));

    // Transition within region A only — region B should be untouched.
    app.world_mut().write_message(TransitionMessage {
        machine,
        source: a1,
        target: a2,
        edge: None,
        blocked: false,
    });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&a2), "a2 should be active");
    assert!(!state.active_leaves.contains(&a1), "a1 should be exited");
    assert!(
        state.active_leaves.contains(&b1),
        "b1 should be unaffected by transition in region A"
    );
}

/// Two parallel leaves both have AlwaysEdges that would exit the parallel
/// region. Only the first should fire; the second's source becomes stale.
#[test]
fn stale_source_skipped_after_parallel_exit() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let p = world.spawn(SubstateOf(machine)).id();
    let a = world.spawn(SubstateOf(p)).id();
    let b = world.spawn(SubstateOf(p)).id();
    let d = world.spawn(SubstateOf(machine)).id();
    let e = world.spawn(SubstateOf(machine)).id();

    // Both leaves try to exit the parallel region
    world.spawn((Source(a), Target(d), AlwaysEdge));
    world.spawn((Source(b), Target(e), AlwaysEdge));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(p)));

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    // Exactly one of (d, e) should be active — whichever parallel leaf's
    // AlwaysEdge fired first. The other is stale-skipped because its
    // source was exited when the parallel region was left.
    let d_active = state.active_leaves.contains(&d);
    let e_active = state.active_leaves.contains(&e);
    assert!(
        d_active ^ e_active,
        "exactly one of d/e should be active, got d={d_active} e={e_active}"
    );
    assert!(!state.active_leaves.contains(&a));
    assert!(!state.active_leaves.contains(&b));
}

/// A substate added at runtime under an already-active parallel parent
/// should join `active_leaves` automatically. This is the dynamic-
/// composition path (e.g. equipment installing ability sub-charts on a
/// running character).
#[test]
fn runtime_substate_added_under_active_parallel_parent_activates() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let machine = app.world_mut().spawn_empty().id();
    let parallel = app.world_mut().spawn(SubstateOf(machine)).id();
    app.world_mut()
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(parallel)));

    app.update();

    // Initially `parallel` has no children, so it's treated as a leaf.
    {
        let state = app.world().get::<StateMachine>(machine).unwrap();
        assert!(state.active_leaves.contains(&parallel));
        assert_eq!(state.active_leaves.len(), 1);
    }

    // Add a child to the already-active parallel parent at runtime.
    let new_child = app.world_mut().spawn(SubstateOf(parallel)).id();

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&new_child),
        "runtime-added child should join active_leaves",
    );
    assert!(
        !state.active_leaves.contains(&parallel),
        "parallel parent should no longer be a leaf",
    );
    assert!(
        state.active.contains(&new_child),
        "runtime-added child should be in active set",
    );
    assert!(
        app.world().get::<Active>(new_child).is_some(),
        "runtime-added child should have Active component",
    );
}

/// A runtime-added substate with its own `InitialState` sequential child
/// should activate the initial state, not the new substate itself.
#[test]
fn runtime_substate_with_initial_state_activates_initial_leaf() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let machine = app.world_mut().spawn_empty().id();
    let parallel = app.world_mut().spawn(SubstateOf(machine)).id();
    app.world_mut()
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(parallel)));

    app.update();

    // Runtime-build an ability sub-chart with its own initial state.
    let ability = app.world_mut().spawn(SubstateOf(parallel)).id();
    let idle = app.world_mut().spawn(SubstateOf(ability)).id();
    let _fire = app.world_mut().spawn(SubstateOf(ability)).id();
    app.world_mut()
        .entity_mut(ability)
        .insert(InitialState(idle));

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&idle),
        "InitialState child should be the active leaf",
    );
    assert_eq!(
        state.active_leaves.len(),
        1,
        "only the initial state should be a leaf (not fire, not ability itself)",
    );
    assert!(state.active.contains(&ability));
    assert!(state.active.contains(&idle));
}

/// Adding a substate under an active sequential (non-parallel) parent should
/// NOT auto-activate it. Sequential parents route through `InitialState` or
/// explicit transitions.
#[test]
fn runtime_substate_under_sequential_parent_does_not_auto_activate() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let machine = app.world_mut().spawn_empty().id();
    let seq = app.world_mut().spawn(SubstateOf(machine)).id();
    let a = app.world_mut().spawn(SubstateOf(seq)).id();
    app.world_mut().entity_mut(seq).insert(InitialState(a));
    app.world_mut()
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(seq)));

    app.update();

    assert_eq!(
        app.world()
            .get::<StateMachine>(machine)
            .unwrap()
            .active_leaves
            .len(),
        1,
    );

    // Add a second child to the sequential parent at runtime.
    let b = app.world_mut().spawn(SubstateOf(seq)).id();
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&a), "a should remain active");
    assert!(
        !state.active_leaves.contains(&b),
        "b should NOT be active under a sequential parent",
    );
    assert!(app.world().get::<Active>(b).is_none());
}
