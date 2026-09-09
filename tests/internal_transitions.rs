//! `EdgeKind::Internal`: the source state is not exited. A self-loop keeps
//! its active children; an edge to a descendant swaps the active child
//! without re-entering the parent. An external self-loop (the default)
//! exits and re-enters, resetting to `InitialState`.

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Coin {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Next {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Skip {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Restart {
    #[gearbox(target)]
    machine: Entity,
}

/// How many times `Playing` was entered.
#[derive(Resource, Default)]
struct Entries(u32);

fn count_entry(_enter: On<EnterState>, mut entries: ResMut<Entries>) {
    entries.0 += 1;
}

fn make_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::scene::ScenePlugin, GearboxPlugin::default()))
        .init_resource::<Entries>();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Playing)
        Substates [
            #Playing InitialState(#Level1)
                on(count_entry)
                Transitions [
                    (Target(#Playing) MessageEdge::<Coin> EdgeKind::Internal),
                    (Target(#Level3)  MessageEdge::<Skip> EdgeKind::Internal),
                    (Target(#Playing) MessageEdge::<Restart>),
                ]
                Substates [
                    #Level1 Transitions [ (Target(#Level2) MessageEdge::<Next>) ],
                    #Level2 Transitions [ (Target(#Level3) MessageEdge::<Next>) ],
                    #Level3,
                ],
        ]
    });
    app.update();
    app
}

fn active_names(world: &mut World) -> Vec<String> {
    let mut q = world.query_filtered::<&Name, With<Active>>();
    let mut names: Vec<String> = q.iter(world).map(|n| n.to_string()).collect();
    names.sort();
    names
}

fn send<M: Message>(app: &mut App, make: impl Fn(Entity) -> M) {
    let mut q = app.world_mut().query_filtered::<Entity, With<StateMachine>>();
    let machine = q.single(app.world()).unwrap();
    app.world_mut().write_message(make(machine));
    app.update();
}

#[test]
fn internal_self_loop_keeps_the_active_child() {
    let mut app = make_app();
    send(&mut app, |machine| Next { machine });
    assert_eq!(active_names(app.world_mut()), ["Level2", "Playing"]);

    send(&mut app, |machine| Coin { machine });
    assert_eq!(active_names(app.world_mut()), ["Level2", "Playing"], "Level1 must not be re-entered");
    assert_eq!(app.world().resource::<Entries>().0, 1, "Playing was not re-entered");
}

#[test]
fn internal_edge_to_a_descendant_swaps_the_child_without_reentering_the_parent() {
    let mut app = make_app();
    send(&mut app, |machine| Skip { machine });
    assert_eq!(active_names(app.world_mut()), ["Level3", "Playing"]);
    assert_eq!(app.world().resource::<Entries>().0, 1);
}

#[test]
fn external_self_loop_reenters_and_resets_to_initial() {
    let mut app = make_app();
    send(&mut app, |machine| Next { machine });
    send(&mut app, |machine| Restart { machine });
    assert_eq!(active_names(app.world_mut()), ["Level1", "Playing"]);
    assert_eq!(app.world().resource::<Entries>().0, 2, "Playing was exited and entered again");
}
