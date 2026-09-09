//! `EnterState` / `ExitState` fire inside the schedule loop, in statechart
//! order: descendants exit before ancestors, ancestors enter before
//! descendants, and a state passed through within one frame gets both.

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Go {
    #[gearbox(target)]
    machine: Entity,
}

/// Every event in the order the observers saw it, as "enter Name" / "exit Name".
#[derive(Resource, Default)]
struct Log(Vec<String>);

fn record_enter(enter: On<EnterState>, q_name: Query<&Name>, mut log: ResMut<Log>) {
    log.0.push(format!("enter {}", q_name.get(enter.state).unwrap()));
}

fn record_exit(exit: On<ExitState>, q_name: Query<&Name>, mut log: ResMut<Log>) {
    log.0.push(format!("exit {}", q_name.get(exit.state).unwrap()));
}

fn make_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::scene::ScenePlugin, GearboxPlugin::default()))
        .init_resource::<Log>();
    app
}

fn send_go(app: &mut App) {
    let mut q = app.world_mut().query_filtered::<Entity, With<StateMachine>>();
    let machine = q.single(app.world()).unwrap();
    app.world_mut().write_message(Go { machine });
    app.update();
}

fn take_log(app: &mut App) -> Vec<String> {
    std::mem::take(&mut app.world_mut().resource_mut::<Log>().0)
}

/// Idle -[Go]-> Mid -[always]-> End: Mid is entered and exited in one frame.
#[test]
fn transient_state_gets_enter_and_exit_in_one_frame() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Idle)
        Substates [
            #Idle on(record_enter) on(record_exit) Transitions [ (Target(#Mid) MessageEdge::<Go>) ],
            #Mid on(record_enter) on(record_exit) Transitions [ (Target(#End) AlwaysEdge) ],
            #End on(record_enter) on(record_exit),
        ]
    });
    app.update();
    assert_eq!(take_log(&mut app), ["enter Idle"]);

    send_go(&mut app);
    assert_eq!(
        take_log(&mut app),
        ["exit Idle", "enter Mid", "exit Mid", "enter End"]
    );
}

/// Idle -[Go]-> Outer { Inner } -[Go]-> Idle: parents enter first, children
/// exit first.
#[test]
fn events_follow_statechart_order() {
    let mut app = make_app();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Idle)
        Substates [
            #Idle on(record_enter) on(record_exit) Transitions [ (Target(#Outer) MessageEdge::<Go>) ],
            #Outer on(record_enter) on(record_exit)
                InitialState(#Inner)
                Transitions [ (Target(#Idle) MessageEdge::<Go>) ]
                Substates [
                    #Inner on(record_enter) on(record_exit),
                ],
        ]
    });
    app.update();
    take_log(&mut app);

    send_go(&mut app);
    assert_eq!(take_log(&mut app), ["exit Idle", "enter Outer", "enter Inner"]);

    send_go(&mut app);
    assert_eq!(take_log(&mut app), ["exit Inner", "exit Outer", "enter Idle"]);
}

/// An `ExitState` observer's commands are applied before `EntryPhase`, and an
/// `EnterState` observer's before `EdgeDetectPhase`: a marker inserted from
/// the exit observer is visible to the entry observer in the same iteration.
#[test]
fn exit_observers_run_before_entry_observers() {
    #[derive(Component)]
    struct LeftIdle;

    #[derive(Resource, Default)]
    struct SawMarker(bool);

    let mut app = make_app();
    app.init_resource::<SawMarker>();
    app.world_mut().commands().spawn_scene(bsn! {
        StateMachine InitialState(#Idle)
        Substates [
            #Idle
                on(|exit: On<ExitState>, mut commands: Commands| { commands.entity(exit.machine).insert(LeftIdle); })
                Transitions [ (Target(#Run) MessageEdge::<Go>) ],
            #Run
                on(|enter: On<EnterState>, q: Query<(), With<LeftIdle>>, mut saw: ResMut<SawMarker>| {
                    saw.0 = q.contains(enter.machine);
                }),
        ]
    });
    app.update();
    send_go(&mut app);
    assert!(app.world().resource::<SawMarker>().0);
}
