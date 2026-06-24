//! Tests for core transition resolution mechanics:
//! - AlwaysEdge chains resolving across multiple schedule iterations
//! - Manual TransitionMessages combining with AlwaysEdge chains
//! - Stale source handling
//! - Hierarchical drill-down on initialization
//! - Internal vs external edge kinds

use bevy::prelude::*;
use bevy_gearbox::prelude::*;
use bevy_gearbox::{GearboxPlugin, IterationCap};

/// A -> B -> C -> D via AlwaysEdges should all resolve in a single `update()`.
#[test]
fn always_edge_chain_resolves_in_single_update() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let a = world.spawn(SubstateOf(machine)).id();
    let b = world.spawn(SubstateOf(machine)).id();
    let c = world.spawn(SubstateOf(machine)).id();
    let d = world.spawn(SubstateOf(machine)).id();

    world.spawn((Source(a), Target(b), AlwaysEdge));
    world.spawn((Source(b), Target(c), AlwaysEdge));
    world.spawn((Source(c), Target(d), AlwaysEdge));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(a)));

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&d), "Should reach D in one update");
    assert_eq!(state.active_leaves.len(), 1);
    assert!(!state.active.contains(&a));
    assert!(!state.active.contains(&b));
    assert!(!state.active.contains(&c));
}

/// A manual TransitionMessage followed by an AlwaysEdge should chain within
/// a single `update()`.
#[test]
fn manual_transition_chains_with_always_edge() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let a = world.spawn(SubstateOf(machine)).id();
    let b = world.spawn(SubstateOf(machine)).id();
    let c = world.spawn(SubstateOf(machine)).id();

    // B -> C fires automatically once B is entered
    world.spawn((Source(b), Target(c), AlwaysEdge));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(a)));

    app.update();
    assert!(app.world().get::<StateMachine>(machine).unwrap().is_active(&a));

    // Manually push A -> B; the AlwaysEdge should carry on to C
    app.world_mut().write_message(TransitionMessage {
        machine,
        source: a,
        target: b,
        edge: None,
        blocked: false,
    });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&c),
        "Should chain through B to C, got leaves: {:?}",
        state.active_leaves
    );
    assert!(!state.active_leaves.contains(&b));
}

/// A TransitionMessage whose source is no longer active should be silently
/// dropped.
#[test]
fn transition_from_inactive_source_is_ignored() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let a = world.spawn(SubstateOf(machine)).id();
    let b = world.spawn(SubstateOf(machine)).id();
    let c = world.spawn(SubstateOf(machine)).id();

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(a)));

    app.update();

    // b is not active — this message should be ignored
    app.world_mut().write_message(TransitionMessage {
        machine,
        source: b,
        target: c,
        edge: None,
        blocked: false,
    });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&a), "Should still be in A");
    assert!(!state.active_leaves.contains(&c));
}

/// Self-transition exits and re-enters the state, causing EntryPhase to run
/// again.
#[test]
fn self_transition_exits_and_reenters() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    #[derive(Resource, Default)]
    struct EntryCount(u32);

    #[derive(Component)]
    struct Marker;

    fn count_entries(
        q_machine: Query<&StateMachine, With<Marker>>,
        mut count: ResMut<EntryCount>,
    ) {
        for machine in &q_machine {
            if !machine.active_leaves.is_empty() {
                count.0 += 1;
            }
        }
    }

    app.init_resource::<EntryCount>();
    app.add_systems(
        GearboxSchedule,
        count_entries.in_set(GearboxPhase::EntryPhase),
    );

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let a = world.spawn(SubstateOf(machine)).id();

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(a), Marker));

    app.update();
    let count_after_init = app.world().resource::<EntryCount>().0;

    app.world_mut().write_message(TransitionMessage {
        machine,
        source: a,
        target: a,
        edge: None,
        blocked: false,
    });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&a));

    let count_after_self = app.world().resource::<EntryCount>().0;
    assert!(
        count_after_self > count_after_init,
        "EntryPhase should have run again for the self-transition \
         (init={count_after_init}, after={count_after_self})"
    );
}

/// A descendant -> ancestor self-transition (e.g. a repeater's `Fire -> Repeater`
/// bounce) must re-fire `Changed<Active>` on the shared ancestor so that
/// `Changed<Active>`-driven logic — like the diesel repeater tick — runs again.
#[test]
fn descendant_to_ancestor_self_transition_refires_changed_active() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    #[derive(Resource, Default)]
    struct PChanged(u32);

    #[derive(Component)]
    struct PMarker;

    fn count_p_changed(
        changed: Query<(), (Changed<Active>, With<PMarker>)>,
        mut count: ResMut<PChanged>,
    ) {
        count.0 += changed.iter().count() as u32;
    }

    app.init_resource::<PChanged>();
    app.add_systems(
        GearboxSchedule,
        count_p_changed.in_set(GearboxPhase::EntryPhase),
    );

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    // P is a compound ancestor with children A (initial) and B.
    let p = world.spawn((SubstateOf(machine), PMarker)).id();
    let a = world.spawn(SubstateOf(p)).id();
    let b = world.spawn(SubstateOf(p)).id();

    world.entity_mut(p).insert(InitialState(a));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(p)));

    // Init: machine drills P -> A. P gains Active once (Changed fires once).
    app.update();
    assert!(app.world().get::<StateMachine>(machine).unwrap().is_active(&a));
    assert_eq!(app.world().resource::<PChanged>().0, 1, "P Active changed once on init");

    // Move to B (sibling transition under P; P stays active and is NOT the
    // target, so its Active is untouched -> no Changed for P).
    app.world_mut().write_message(TransitionMessage {
        machine,
        source: a,
        target: b,
        edge: None,
        blocked: false,
    });
    app.update();
    assert!(app.world().get::<StateMachine>(machine).unwrap().is_active(&b));
    let before_bounce = app.world().resource::<PChanged>().0;
    assert_eq!(before_bounce, 1, "A -> B must NOT re-fire Changed<Active> on the ancestor P");

    // The bounce: descendant B -> ancestor P (external, edge = None). P is the
    // target, so its Active is re-inserted to fire Changed<Active>; the machine
    // then drills back to initial A.
    app.world_mut().write_message(TransitionMessage {
        machine,
        source: b,
        target: p,
        edge: None,
        blocked: false,
    });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(
        state.active_leaves.contains(&a),
        "B -> P should re-enter P and drill back to its initial child A"
    );
    let after_bounce = app.world().resource::<PChanged>().0;
    assert!(
        after_bounce > before_bounce,
        "descendant -> ancestor transition must re-fire Changed<Active> on P \
         (changed before={before_bounce}, after={after_bounce})"
    );
}

/// An external transition must re-enter the WHOLE subtree under the target, not
/// just the target itself: a state that stays active because the target
/// re-drilled back into it must still re-fire `Changed<Active>`.
///
/// Chain `P > M > L` (each the initial of its parent), settled in `L`. An
/// external `L -> M` transition re-enters `M`, whose initial drills back to `L`.
/// `L` never leaves the active set, but it IS re-entered, so it must re-signal.
/// Before the subtree-re-entry fix, only `M` (the target) re-signaled and `L`
/// was silently skipped.
#[test]
fn external_reentry_resignals_whole_subtree() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    #[derive(Resource, Default)]
    struct LChanged(u32);

    #[derive(Component)]
    struct LMarker;

    fn count_l_changed(
        changed: Query<(), (Changed<Active>, With<LMarker>)>,
        mut count: ResMut<LChanged>,
    ) {
        count.0 += changed.iter().count() as u32;
    }

    app.init_resource::<LChanged>();
    app.add_systems(
        GearboxSchedule,
        count_l_changed.in_set(GearboxPhase::EntryPhase),
    );

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let p = world.spawn(SubstateOf(machine)).id();
    let m = world.spawn(SubstateOf(p)).id();
    let l = world.spawn((SubstateOf(m), LMarker)).id();

    world.entity_mut(m).insert(InitialState(l));
    world.entity_mut(p).insert(InitialState(m));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(p)));

    app.update();
    assert!(app.world().get::<StateMachine>(machine).unwrap().is_active(&l));
    assert_eq!(app.world().resource::<LChanged>().0, 1, "L Active changed once on init");

    // External L -> M (edge = None). M re-drills to its initial L; L stays active
    // but must be re-entered as part of M's subtree.
    app.world_mut().write_message(TransitionMessage {
        machine,
        source: l,
        target: m,
        edge: None,
        blocked: false,
    });
    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&l), "L should still be active after re-entry");
    assert_eq!(
        app.world().resource::<LChanged>().0,
        2,
        "L must re-fire Changed<Active> when re-entered as part of the M subtree"
    );
}

/// An *internal* transition does NOT re-enter the target subtree, so a
/// stayed-active descendant must NOT re-signal. Same `P > M > L` shape, but the
/// `L -> M` edge is `EdgeKind::Internal`.
#[test]
fn internal_reentry_does_not_resignal_subtree() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    #[derive(Resource, Default)]
    struct LChanged(u32);

    #[derive(Component)]
    struct LMarker;

    fn count_l_changed(
        changed: Query<(), (Changed<Active>, With<LMarker>)>,
        mut count: ResMut<LChanged>,
    ) {
        count.0 += changed.iter().count() as u32;
    }

    app.init_resource::<LChanged>();
    app.add_systems(
        GearboxSchedule,
        count_l_changed.in_set(GearboxPhase::EntryPhase),
    );

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let p = world.spawn(SubstateOf(machine)).id();
    let m = world.spawn(SubstateOf(p)).id();
    let l = world.spawn((SubstateOf(m), LMarker)).id();

    // Internal edge L -> M, fired manually below.
    let edge = world.spawn((Source(l), Target(m), EdgeKind::Internal)).id();

    world.entity_mut(m).insert(InitialState(l));
    world.entity_mut(p).insert(InitialState(m));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(p)));

    app.update();
    let before = app.world().resource::<LChanged>().0;

    app.world_mut().write_message(TransitionMessage {
        machine,
        source: l,
        target: m,
        edge: Some(edge),
        blocked: false,
    });
    app.update();

    assert_eq!(
        app.world().resource::<LChanged>().0,
        before,
        "internal transition must NOT re-enter / re-signal the descendant L"
    );
}

/// Machine -> P (InitialState=Q) -> Q (InitialState=leaf)
/// Initialization should drill all the way down to the leaf.
#[test]
fn hierarchical_drill_down_on_init() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let p = world.spawn(SubstateOf(machine)).id();
    let q = world.spawn(SubstateOf(p)).id();
    let leaf = world.spawn(SubstateOf(q)).id();

    world.entity_mut(q).insert(InitialState(leaf));
    world.entity_mut(p).insert(InitialState(q));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(p)));

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&leaf), "Should drill down to leaf");
    assert!(state.active.contains(&p), "P should be active (ancestor)");
    assert!(state.active.contains(&q), "Q should be active (ancestor)");
    assert_eq!(state.active_leaves.len(), 1);
}

/// An internal edge whose source IS the LCA should not exit/re-enter the LCA.
#[test]
fn internal_edge_preserves_lca() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let p = world.spawn(SubstateOf(machine)).id();
    let a = world.spawn(SubstateOf(p)).id();
    let b = world.spawn(SubstateOf(p)).id();

    world.spawn((Source(a), Target(b), AlwaysEdge, EdgeKind::Internal));

    world.entity_mut(p).insert(InitialState(a));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(p)));

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&b));
    assert!(
        state.active.contains(&p),
        "P (LCA) should remain active with internal edge"
    );
}

/// A transition sourced from a composite (non-leaf) parent should fire when
/// any of its descendant leaves are active.
#[test]
fn transition_from_composite_parent() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let p = world.spawn(SubstateOf(machine)).id();
    let a = world.spawn(SubstateOf(p)).id();
    let _b = world.spawn(SubstateOf(p)).id();
    let d = world.spawn(SubstateOf(machine)).id();

    world.spawn((Source(p), Target(d), AlwaysEdge));

    world.entity_mut(p).insert(InitialState(a));
    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(p)));

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.active_leaves.contains(&d));
    assert!(!state.active.contains(&p));
}

/// A machine with no AlwaysEdges stays in its initial state.
#[test]
fn stable_state_no_extra_iterations() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let a = world.spawn(SubstateOf(machine)).id();
    let b = world.spawn(SubstateOf(machine)).id();

    // Edge without AlwaysEdge marker — should not fire automatically
    world.spawn((Source(a), Target(b)));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(a)));

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.is_active(&a));
    assert!(!state.is_active(&b));
}

/// Active component is inserted on entered states and removed on exited
/// states, with the correct machine reference.
#[test]
fn active_component_tracks_entries_and_exits() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let a = world.spawn(SubstateOf(machine)).id();
    let b = world.spawn(SubstateOf(machine)).id();

    world.spawn((Source(a), Target(b), AlwaysEdge));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(a)));

    app.update();

    assert!(
        app.world().get::<Active>(b).is_some(),
        "B should have Active component"
    );
    assert!(
        app.world().get::<Active>(a).is_none(),
        "A should NOT have Active component (it was exited)"
    );
    let active = app.world().get::<Active>(b).unwrap();
    assert_eq!(active.machine, machine);
}

/// With IterationCap(2), a chain A -> B -> C should only get to B.
#[test]
fn iteration_cap_limits_resolution_depth() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin::default()));
    app.insert_resource(IterationCap(2));

    let world = app.world_mut();
    let machine = world.spawn_empty().id();
    let a = world.spawn(SubstateOf(machine)).id();
    let b = world.spawn(SubstateOf(machine)).id();
    let c = world.spawn(SubstateOf(machine)).id();

    world.spawn((Source(a), Target(b), AlwaysEdge));
    world.spawn((Source(b), Target(c), AlwaysEdge));

    world
        .entity_mut(machine)
        .insert((StateMachine::new(), InitialState(a)));

    app.update();

    let state = app.world().get::<StateMachine>(machine).unwrap();
    assert!(state.is_active(&b), "Cap=2 should only reach B");
    assert!(!state.active_leaves.contains(&c));
}
