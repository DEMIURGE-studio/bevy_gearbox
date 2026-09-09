//! `InState` / `NotInState`: built-in guards that veto an edge unless another
//! state is (or is not) active, the usual way to coordinate parallel regions.

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Crouch {
    #[gearbox(target)]
    machine: Entity,
}

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Draw {
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

fn is_active(world: &mut World, name: &str) -> bool {
    let e = named(world, name);
    world.get::<Active>(e).is_some()
}

fn send<M: Message>(app: &mut App, make: impl Fn(Entity) -> M) {
    let mut q = app.world_mut().query_filtered::<Entity, With<StateMachine>>();
    let machine = q.single(app.world()).unwrap();
    app.world_mut().write_message(make(machine));
    app.update();
}

/// Posture: Standing -[Crouch]-> Crouching. Weapon: Holstered -[Draw]-> Drawn,
/// guarded by `InState(#Standing)`.
#[test]
fn in_state_guard_requires_the_named_state_to_be_active() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine
        Substates [
            #Posture InitialState(#Standing) Substates [
                #Standing Transitions [ (Target(#Crouching) MessageEdge::<Crouch>) ],
                #Crouching,
            ],
            #Weapon InitialState(#Holstered) Substates [
                #Holstered Transitions [ (Target(#Drawn) MessageEdge::<Draw> InState(#Standing)) ],
                #Drawn,
            ],
        ]
    });
    app.update();

    send(&mut app, |machine| Crouch { machine });
    send(&mut app, |machine| Draw { machine });
    assert!(is_active(app.world_mut(), "Holstered"), "crouching: the draw is vetoed");

    // Fresh machine, still standing.
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine
        Substates [
            #Posture InitialState(#Standing) Substates [
                #Standing Transitions [ (Target(#Crouching) MessageEdge::<Crouch>) ],
                #Crouching,
            ],
            #Weapon InitialState(#Holstered) Substates [
                #Holstered Transitions [ (Target(#Drawn) MessageEdge::<Draw> InState(#Standing)) ],
                #Drawn,
            ],
        ]
    });
    app.update();
    send(&mut app, |machine| Draw { machine });
    assert!(is_active(app.world_mut(), "Drawn"), "standing: the draw goes through");
}

/// `NotInState` is the complement, and a vetoed edge falls through to the
/// next candidate in `Transitions` order.
#[test]
fn not_in_state_guard_and_fallthrough() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine
        Substates [
            #Posture InitialState(#Standing) Substates [
                #Standing Transitions [ (Target(#Crouching) MessageEdge::<Crouch>) ],
                #Crouching,
            ],
            #Weapon InitialState(#Holstered) Substates [
                #Holstered Transitions [
                    (Target(#Drawn) MessageEdge::<Draw> NotInState(#Crouching)),
                    (Target(#Fumbled) MessageEdge::<Draw>),
                ],
                #Drawn,
                #Fumbled,
            ],
        ]
    });
    app.update();

    send(&mut app, |machine| Crouch { machine });
    send(&mut app, |machine| Draw { machine });
    assert!(is_active(app.world_mut(), "Fumbled"), "crouching: the guarded edge is vetoed, the fallback wins");
}
