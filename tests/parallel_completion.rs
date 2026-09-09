//! A parallel state completes when every region has reached a `TerminalState`.
//! `Done` is then addressed to the parallel state, so its own
//! `MessageEdge<Done>` can transition out. Nested parallel states cascade;
//! a sequential ancestor does not (it needs its own terminal child).

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Knock {
    #[gearbox(target)]
    machine: Entity,
}

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Drop {
    #[gearbox(target)]
    machine: Entity,
}

fn make_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::scene::ScenePlugin, GearboxPlugin::default()));
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

fn send<M: Message>(app: &mut App, make: impl Fn(Entity) -> M) {
    let m = machine(app.world_mut());
    app.world_mut().write_message(make(m));
    app.update();
}

/// ```text
/// machine
/// ├── Fight (parallel) -[Done]-> Over
/// │   ├── Posture: Standing -[Knock]-> Down (terminal)
/// │   └── Weapon:  Armed -[Drop]-> Dropped (terminal)
/// └── Over
/// ```
#[test]
fn parallel_state_completes_only_when_every_region_is_done() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Fight)
        Substates [
            #Fight
                Transitions [ (Target(#Over) MessageEdge::<Done>) ]
                Substates [
                    #Posture InitialState(#Standing) Substates [
                        #Standing Transitions [ (Target(#Down) MessageEdge::<Knock>) ],
                        #Down TerminalState,
                    ],
                    #Weapon InitialState(#Armed) Substates [
                        #Armed Transitions [ (Target(#Dropped) MessageEdge::<Drop>) ],
                        #Dropped TerminalState,
                    ],
                ],
            #Over,
        ]
    });
    app.update();
    assert!(is_active(app.world_mut(), "Standing"));
    assert!(is_active(app.world_mut(), "Armed"));

    send(&mut app, |machine| Knock { machine });
    assert!(is_active(app.world_mut(), "Down"));
    assert!(is_active(app.world_mut(), "Fight"), "one finished region must not complete the parallel state");
    assert!(!is_active(app.world_mut(), "Over"));

    send(&mut app, |machine| Drop { machine });
    assert!(is_active(app.world_mut(), "Over"), "both regions done: Fight's Done edge should fire");
    assert!(!is_active(app.world_mut(), "Fight"));
    assert!(!is_active(app.world_mut(), "Down"));
    assert!(!is_active(app.world_mut(), "Dropped"));
}

/// ```text
/// machine
/// ├── Outer (parallel) -[Done]-> Over
/// │   ├── Inner (parallel)
/// │   │   ├── Posture: Standing -[Knock]-> Down (terminal)
/// │   │   └── Weapon:  Armed -[Drop]-> Dropped (terminal)
/// │   └── Side: Finished (terminal)
/// └── Over
/// ```
#[test]
fn nested_parallel_completion_cascades_upward() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Outer)
        Substates [
            #Outer
                Transitions [ (Target(#Over) MessageEdge::<Done>) ]
                Substates [
                    #Inner Substates [
                        #Posture InitialState(#Standing) Substates [
                            #Standing Transitions [ (Target(#Down) MessageEdge::<Knock>) ],
                            #Down TerminalState,
                        ],
                        #Weapon InitialState(#Armed) Substates [
                            #Armed Transitions [ (Target(#Dropped) MessageEdge::<Drop>) ],
                            #Dropped TerminalState,
                        ],
                    ],
                    #Side InitialState(#Finished) Substates [
                        #Finished TerminalState,
                    ],
                ],
            #Over,
        ]
    });
    app.update();
    assert!(is_active(app.world_mut(), "Outer"), "Side is done but Inner is not");

    send(&mut app, |machine| Knock { machine });
    assert!(is_active(app.world_mut(), "Outer"));

    send(&mut app, |machine| Drop { machine });
    assert!(is_active(app.world_mut(), "Over"), "Inner completed, so Outer completed too");
}

/// ```text
/// machine
/// ├── Round (sequential) -[Done]-> Over
/// │   └── Fight (parallel) -[Done]-> Rest
/// │   │   ├── Posture: Standing -[Knock]-> Down (terminal)
/// │   │   └── Weapon:  Armed -[Drop]-> Dropped (terminal)
/// │   └── Rest
/// └── Over
/// ```
#[test]
fn completed_parallel_does_not_complete_a_sequential_parent() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Round)
        Substates [
            #Round
                InitialState(#Fight)
                Transitions [ (Target(#Over) MessageEdge::<Done>) ]
                Substates [
                    #Fight
                        Transitions [ (Target(#Rest) MessageEdge::<Done>) ]
                        Substates [
                            #Posture InitialState(#Standing) Substates [
                                #Standing Transitions [ (Target(#Down) MessageEdge::<Knock>) ],
                                #Down TerminalState,
                            ],
                            #Weapon InitialState(#Armed) Substates [
                                #Armed Transitions [ (Target(#Dropped) MessageEdge::<Drop>) ],
                                #Dropped TerminalState,
                            ],
                        ],
                    #Rest,
                ],
            #Over,
        ]
    });
    app.update();

    send(&mut app, |machine| Knock { machine });
    send(&mut app, |machine| Drop { machine });
    assert!(is_active(app.world_mut(), "Rest"), "Fight's own Done edge fires");
    assert!(is_active(app.world_mut(), "Round"), "Round has no terminal child, so it is not done");
    assert!(!is_active(app.world_mut(), "Over"));
}
