//! Guarded transitions: ordered candidates, first passing guard wins.
//!
//! A guard is a marker component on an edge plus a system in `BlockerPhase`
//! that vetoes candidates carrying it. Every matching edge along the active
//! leaf's ancestor chain is proposed; the lowest-ranked survivor (deepest
//! state, then `Transitions` order) is applied. A guardless edge last in the
//! list is the fallback.

use std::time::Duration;

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy::time::TimeUpdateStrategy;
use bevy_gearbox::prelude::*;
use bevy_gearbox::{GearboxPlugin, Matched};

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Hit {
    #[gearbox(target)]
    machine: Entity,
}

/// Guard marker: edges carrying it are always vetoed.
#[derive(Component, Default, Clone)]
struct Never;

/// Guard marker: edges carrying it pass only while `Allow` is true.
#[derive(Component, Default, Clone)]
struct OnlyIfAllowed;

#[derive(Resource, Default)]
struct Allow(bool);

fn veto_guards(
    mut candidates: MessageMutator<TransitionMessage>,
    q_never: Query<(), With<Never>>,
    q_allowed: Query<(), With<OnlyIfAllowed>>,
    allow: Res<Allow>,
) {
    for c in candidates.read() {
        let Some(edge) = c.edge else { continue };
        if q_never.contains(edge) || (q_allowed.contains(edge) && !allow.0) {
            c.blocked = true;
        }
    }
}

/// Records which `Matched<Hit>` edges reached the side-effect phase unblocked.
#[derive(Resource, Default)]
struct Applied(Vec<Entity>);

fn record_side_effects(
    mut reader: MessageReader<Matched<Hit>>,
    blocked: Res<BlockedEdges>,
    mut applied: ResMut<Applied>,
) {
    for m in reader.read() {
        if !blocked.is_blocked(m.edge) {
            applied.0.push(m.edge);
        }
    }
}

fn make_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::scene::ScenePlugin, GearboxPlugin::default()))
        .init_resource::<Allow>()
        .init_resource::<Applied>()
        .add_systems(GearboxSchedule, veto_guards.in_set(GearboxPhase::BlockerPhase))
        .add_systems(GearboxSchedule, record_side_effects.in_set(GearboxPhase::SideEffectPhase));
    app
}

fn named(world: &mut World, name: &str) -> Entity {
    let mut q = world.query::<(Entity, &Name)>();
    q.iter(world)
        .find(|(_, n)| n.as_str() == name)
        .map(|(e, _)| e)
        .unwrap_or_else(|| panic!("no entity named {name}"))
}

fn machine(world: &mut World) -> Entity {
    let mut q = world.query_filtered::<Entity, With<StateMachine>>();
    q.single(world).unwrap()
}

fn is_active(world: &mut World, name: &str) -> bool {
    let e = named(world, name);
    world.get::<Active>(e).is_some()
}

fn send_hit(app: &mut App) {
    let m = machine(app.world_mut());
    app.world_mut().write_message(Hit { machine: m });
    app.update();
}

/// Three candidates on one state, in `Transitions` order: the first is vetoed,
/// the second passes, the third is the guardless fallback.
#[test]
fn first_passing_candidate_wins_in_transitions_order() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Idle)
        Substates [
            #Idle Transitions [
                (#ToA Target(#A) MessageEdge::<Hit> Never),
                (#ToB Target(#B) MessageEdge::<Hit>),
                (#ToC Target(#C) MessageEdge::<Hit>),
            ],
            #A, #B, #C,
        ]
    });
    app.update();
    assert!(is_active(app.world_mut(), "Idle"));

    send_hit(&mut app);
    assert!(is_active(app.world_mut(), "B"), "second candidate should win");
    assert!(!is_active(app.world_mut(), "A"));
    assert!(!is_active(app.world_mut(), "C"));
}

/// Every candidate vetoed: nothing happens and the machine stays put.
#[test]
fn all_candidates_vetoed_leaves_state_unchanged() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Idle)
        Substates [
            #Idle Transitions [
                (Target(#A) MessageEdge::<Hit> Never),
                (Target(#B) MessageEdge::<Hit> Never),
            ],
            #A, #B,
        ]
    });
    app.update();
    send_hit(&mut app);
    assert!(is_active(app.world_mut(), "Idle"));
}

/// A guardless edge last in the list is the fallback.
#[test]
fn guardless_last_edge_is_the_otherwise() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Idle)
        Substates [
            #Idle Transitions [
                (Target(#Special) MessageEdge::<Hit> OnlyIfAllowed),
                (Target(#Normal) MessageEdge::<Hit>),
            ],
            #Special, #Normal Transitions [ (Target(#Idle) MessageEdge::<Hit>) ],
        ]
    });
    app.update();

    send_hit(&mut app);
    assert!(is_active(app.world_mut(), "Normal"), "guard fails: fallback wins");

    send_hit(&mut app); // Normal -> Idle
    assert!(is_active(app.world_mut(), "Idle"));

    app.world_mut().resource_mut::<Allow>().0 = true;
    send_hit(&mut app);
    assert!(is_active(app.world_mut(), "Special"), "guard passes: first edge wins");
}

/// The leaf's edge normally beats the parent's, but when every leaf candidate
/// is vetoed the parent's edge is taken (SCXML selection).
#[test]
fn vetoed_leaf_falls_through_to_parent_edge() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Parent)
        Substates [
            #Parent InitialState(#Child)
                Transitions [ (Target(#FromParent) MessageEdge::<Hit>) ]
                Substates [
                    #Child Transitions [ (Target(#FromChild) MessageEdge::<Hit> OnlyIfAllowed) ],
                ],
            #FromParent Transitions [ (Target(#Parent) MessageEdge::<Hit>) ],
            #FromChild,
        ]
    });
    app.update();

    send_hit(&mut app);
    assert!(is_active(app.world_mut(), "FromParent"), "leaf vetoed: parent's edge wins");

    send_hit(&mut app); // back to Parent/Child
    assert!(is_active(app.world_mut(), "Child"));

    app.world_mut().resource_mut::<Allow>().0 = true;
    send_hit(&mut app);
    assert!(is_active(app.world_mut(), "FromChild"), "leaf passes: deeper edge wins");
}

/// Only the winning candidate's `Matched<M>` reaches side effects unblocked;
/// losers and vetoed edges appear in `BlockedEdges`.
#[test]
fn only_the_winner_is_unblocked_for_side_effects() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Idle)
        Substates [
            #Idle Transitions [
                (#Vetoed Target(#A) MessageEdge::<Hit> Never),
                (#Winner Target(#B) MessageEdge::<Hit>),
                (#Loser  Target(#C) MessageEdge::<Hit>),
            ],
            #A, #B, #C,
        ]
    });
    app.update();
    send_hit(&mut app);

    let winner = named(app.world_mut(), "Winner");
    let applied = app.world().resource::<Applied>().0.clone();
    assert_eq!(applied, vec![winner], "exactly one Matched reaches side effects unblocked");
}

/// Guarded `AlwaysEdge` list: the first always-edge is vetoed, the second
/// fires, all within the frame the source is entered.
#[test]
fn guarded_always_edges_fall_through() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Start)
        Substates [
            #Start Transitions [
                (Target(#A) AlwaysEdge Never),
                (Target(#B) AlwaysEdge),
            ],
            #A, #B,
        ]
    });
    app.update();
    assert!(is_active(app.world_mut(), "B"));
    assert!(!is_active(app.world_mut(), "A"));
}

/// Guarded delayed always-edges with the same delay: the first is vetoed when
/// the timers elapse, the second fires.
#[test]
fn guarded_delayed_always_edges_fall_through() {
    let mut app = make_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(50)));
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Start)
        Substates [
            #Start Transitions [
                (Target(#A) AlwaysEdge Delay::from_secs_f32(0.1) Never),
                (Target(#B) AlwaysEdge Delay::from_secs_f32(0.1)),
            ],
            #A, #B,
        ]
    });
    app.update(); // spawn + init; timers start
    assert!(is_active(app.world_mut(), "Start"));
    app.update(); // 50ms
    app.update(); // 100ms: timers elapse
    app.update(); // slack for the time strategy's first delta
    assert!(is_active(app.world_mut(), "B"), "second delayed edge should win");
    assert!(!is_active(app.world_mut(), "A"));
}

/// Delayed transitions pass through the blocker phase too.
#[test]
fn delayed_transition_can_be_vetoed() {
    let mut app = make_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(50)));
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Start)
        Substates [
            #Start Transitions [ (Target(#A) AlwaysEdge Delay::from_secs_f32(0.1) Never) ],
            #A,
        ]
    });
    for _ in 0..6 {
        app.update();
    }
    assert!(is_active(app.world_mut(), "Start"), "vetoed delayed edge must not fire");
}

/// Parallel regions: each region resolves its own candidates independently.
#[test]
fn each_parallel_region_selects_independently() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine
        Substates [
            #Left InitialState(#L1) Substates [
                #L1 Transitions [
                    (Target(#L2) MessageEdge::<Hit> Never),
                    (Target(#L3) MessageEdge::<Hit>),
                ],
                #L2, #L3,
            ],
            #Right InitialState(#R1) Substates [
                #R1 Transitions [ (Target(#R2) MessageEdge::<Hit>) ],
                #R2,
            ],
        ]
    });
    app.update();
    send_hit(&mut app);
    assert!(is_active(app.world_mut(), "L3"));
    assert!(is_active(app.world_mut(), "R2"));
}
