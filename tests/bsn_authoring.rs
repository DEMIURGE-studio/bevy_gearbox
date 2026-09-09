//! Every authorable gearbox component, written in `bsn!` and run through the
//! plugin. These tests pin the authoring surface: if a component stops lowering
//! in `bsn!`, or lowers but does not behave, something here fails.

use std::time::Duration;

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy::time::TimeUpdateStrategy;
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Go {
    #[gearbox(target)]
    machine: Entity,
}

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Back {
    #[gearbox(target)]
    machine: Entity,
}

#[state_component]
#[derive(Component, Clone, Default)]
struct Walking;

#[state_component]
#[derive(Component, Clone, Default, PartialEq, Debug)]
struct Speed(f32);

#[state_component]
#[derive(Component, Clone, Default)]
struct CanMove;

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

fn send<M: GearboxMessage>(app: &mut App, make: impl Fn(Entity) -> M) {
    let m = machine(app.world_mut());
    app.world_mut().write_message(make(m));
    app.update();
}

/// `StateComponent::<T>` lowers bare (via `Default`) and with a payload
/// (`StateComponent::<T>(value)`), and lands on the machine root.
#[test]
fn state_components_lower_in_bsn() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Idle)
        Substates [
            #Idle StateInactiveComponent::<CanMove> Transitions [ (Target(#Run) MessageEdge::<Go>) ],
            #Run StateComponent::<Walking> StateComponent::<Speed>(Speed(7.5))
                 Transitions [ (Target(#Idle) MessageEdge::<Back>) ],
        ]
    });
    app.update();
    let root = machine(app.world_mut());
    assert!(app.world().get::<Walking>(root).is_none());
    assert!(app.world().get::<CanMove>(root).is_none(), "inactive-component payload removed while Idle is active");

    send(&mut app, |m| Go { machine: m });
    assert!(app.world().get::<Walking>(root).is_some());
    assert_eq!(app.world().get::<Speed>(root), Some(&Speed(7.5)));
    assert!(app.world().get::<CanMove>(root).is_some(), "restored once Idle is exited");

    send(&mut app, |m| Back { machine: m });
    assert!(app.world().get::<Walking>(root).is_none());
    assert!(app.world().get::<Speed>(root).is_none());
}

/// `History::Shallow` restores the last active child instead of `InitialState`.
#[test]
fn history_lowers_and_restores() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Menu)
        Substates [
            #Menu History::Shallow InitialState(#Main)
                Transitions [ (Target(#Game) MessageEdge::<Go>) ]
                Substates [
                    #Main Transitions [ (Target(#Options) MessageEdge::<Back>) ],
                    #Options,
                ],
            #Game Transitions [ (Target(#Menu) MessageEdge::<Back>) ],
        ]
    });
    app.update();
    assert!(is_active(app.world_mut(), "Main"));

    send(&mut app, |m| Back { machine: m }); // Main -> Options
    assert!(is_active(app.world_mut(), "Options"));
    send(&mut app, |m| Go { machine: m }); // Menu -> Game
    assert!(is_active(app.world_mut(), "Game"));
    send(&mut app, |m| Back { machine: m }); // Game -> Menu, history restores Options
    assert!(is_active(app.world_mut(), "Options"), "shallow history restored the last child");
}

/// `ResetEdge(ResetScope::Target)` clears the target's history so re-entry
/// follows `InitialState` again.
#[test]
fn reset_edge_lowers_and_clears_history() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Menu)
        Substates [
            #Menu History::Shallow InitialState(#Main)
                Transitions [ (Target(#Game) MessageEdge::<Go>) ]
                Substates [
                    #Main Transitions [ (Target(#Options) MessageEdge::<Back>) ],
                    #Options,
                ],
            #Game Transitions [ (Target(#Menu) MessageEdge::<Back> ResetEdge(ResetScope::Target)) ],
        ]
    });
    app.update();
    send(&mut app, |m| Back { machine: m }); // Main -> Options
    send(&mut app, |m| Go { machine: m }); // -> Game
    send(&mut app, |m| Back { machine: m }); // -> Menu with reset
    assert!(is_active(app.world_mut(), "Main"), "reset edge discarded the saved history");
}

/// `TerminalState` emits `Done` to its parent; the parent's `MessageEdge::<Done>`
/// fires in the same frame.
#[test]
fn terminal_state_and_done_lower() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Work)
        Substates [
            #Work InitialState(#Step)
                Transitions [ (Target(#Finished) MessageEdge::<Done>) ]
                Substates [
                    #Step Transitions [ (Target(#End) MessageEdge::<Go>) ],
                    #End TerminalState,
                ],
            #Finished,
        ]
    });
    app.update();
    send(&mut app, |m| Go { machine: m });
    assert!(is_active(app.world_mut(), "Finished"), "Done from the terminal child fired the parent's edge");
}

/// A `Delay` on a `MessageEdge` starts on the message and fires when it elapses.
#[test]
fn delayed_message_edge_lowers() {
    let mut app = make_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(50)));
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Idle)
        Substates [
            #Idle Transitions [ (Target(#Run) MessageEdge::<Go> Delay::from_secs_f32(0.1)) ],
            #Run,
        ]
    });
    app.update();
    send(&mut app, |m| Go { machine: m });
    assert!(is_active(app.world_mut(), "Idle"), "not yet: delay pending");
    for _ in 0..4 {
        app.update();
    }
    assert!(is_active(app.world_mut(), "Run"));
}

/// A root without `InitialState` is a parallel machine; `Source(#X)` authors an
/// edge outside a `Transitions` block; `StateMachineId("..")` lowers on the root.
#[test]
fn parallel_root_explicit_source_and_machine_id_lower() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachineId("character")
        StateMachine
        Substates [
            #Posture InitialState(#Standing) Substates [ #Standing, #Crouching ],
            #Weapon InitialState(#Holstered) Substates [ #Holstered, #Drawn ],
            (Source(#Standing) Target(#Crouching) MessageEdge::<Go>),
        ]
    });
    app.update();
    let root = machine(app.world_mut());
    assert_eq!(app.world().get::<StateMachineId>(root), Some(&StateMachineId("character".into())));
    assert!(is_active(app.world_mut(), "Standing"));
    assert!(is_active(app.world_mut(), "Holstered"));

    send(&mut app, |m| Go { machine: m });
    assert!(is_active(app.world_mut(), "Crouching"), "explicit Source edge fired");
    assert!(is_active(app.world_mut(), "Holstered"), "other region untouched");
}

/// `on(..)` attaches an `EnterState` observer to a state entity from inside the scene.
#[test]
fn enter_state_observer_lowers() {
    #[derive(Resource, Default)]
    struct Entered(u32);

    let mut app = make_app();
    app.init_resource::<Entered>();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Idle)
        Substates [
            #Idle Transitions [ (Target(#Run) MessageEdge::<Go>) ],
            #Run on(|_: On<EnterState>, mut entered: ResMut<Entered>| { entered.0 += 1; }),
        ]
    });
    app.update();
    assert_eq!(app.world().resource::<Entered>().0, 0);
    send(&mut app, |m| Go { machine: m });
    assert_eq!(app.world().resource::<Entered>().0, 1);
}
