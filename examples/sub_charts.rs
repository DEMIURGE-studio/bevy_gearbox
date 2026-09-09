//! How do I nest a sub-chart and continue when it finishes?
//!
//! Mark the sub-chart's last state `TerminalState`. Entering it sends a
//! `Done` message to the parent, and a `MessageEdge::<Done>` on the parent
//! moves on. A parallel state is done when every region has reached a
//! terminal state: here `Gather` waits for both wood and stone.
//!
//! ```text
//! Quest (StateMachine, initial = Gather)
//! ├── Gather (parallel)                          --Done--> Build     when both regions are done
//! │   ├── Wood  (initial = Chopping): Chopping --Chop (W)--> WoodReady  (terminal)
//! │   └── Stone (initial = Mining):  Mining   --Mine (S)--> StoneReady (terminal)
//! ├── Build (initial = Hammering)                --Done--> Complete
//! │   └── Hammering --always, after 1.5s--> Built (terminal)
//! └── Complete
//! ```
//!
//! `Done` is addressed to the state that finished, so `Build`'s edge cannot be
//! fired by the wood region finishing, and `Gather`'s cannot fire until the
//! stone is ready too.
//!
//! ```sh
//! cargo run --example sub_charts
//! # or, to let the gearbox editor connect on 127.0.0.1:15703:
//! cargo run --example sub_charts --features server
//! ```

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Chop {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Mine {
    #[gearbox(target)]
    machine: Entity,
}

/// One line of the quest log, keyed by the state names it watches.
#[derive(Component)]
struct Row {
    working: &'static str,
    done: &'static str,
    label: &'static str,
}

const WAITING: Color = Color::srgb(0.5, 0.5, 0.55);
const WORKING: Color = Color::srgb(1.0, 0.85, 0.4);
const DONE: Color = Color::srgb(0.4, 0.9, 0.5);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GearboxPlugin::default())
        .add_plugins(editor_server)
        .add_systems(Startup, setup)
        .add_systems(Update, input.before(GearboxSet))
        .add_systems(Update, draw_rows.after(GearboxSet))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text2d::new("<W> chop wood   <S> mine stone   (building starts once both are ready)"),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 200.0, 0.0),
    ));
    let rows = [
        ("Chopping", "WoodReady", "Wood"),
        ("Mining", "StoneReady", "Stone"),
        ("Hammering", "Built", "Build"),
        ("", "Complete", "Quest"),
    ];
    for (i, (working, done, label)) in rows.into_iter().enumerate() {
        commands.spawn((
            Row { working, done, label },
            Text2d::new(""),
            TextColor(WAITING),
            Transform::from_xyz(0.0, 80.0 - 50.0 * i as f32, 0.0),
        ));
    }

    commands.spawn_scene(bsn! {
        #Quest
            StateMachineId("quest")
            StateMachine InitialState(#Gather)
        Substates [
            // No InitialState: a parallel state. Its Done fires when every
            // region is in a terminal state.
            #Gather
                Transitions [ (Target(#Build) MessageEdge::<Done>) ]
                Substates [
                    #Wood InitialState(#Chopping) Substates [
                        #Chopping Transitions [ (Target(#WoodReady) MessageEdge::<Chop>) ],
                        #WoodReady TerminalState,
                    ],
                    #Stone InitialState(#Mining) Substates [
                        #Mining Transitions [ (Target(#StoneReady) MessageEdge::<Mine>) ],
                        #StoneReady TerminalState,
                    ],
                ],
            // A sequential sub-chart: Done when its terminal child is entered.
            #Build InitialState(#Hammering)
                Transitions [ (Target(#Complete) MessageEdge::<Done>) ]
                Substates [
                    #Hammering Transitions [ (Target(#Built) AlwaysEdge Delay::from_secs_f32(1.5)) ],
                    #Built TerminalState,
                ],
            #Complete,
        ]
    });
}

fn input(
    keys: Res<ButtonInput<KeyCode>>,
    machine: Single<Entity, With<StateMachine>>,
    mut chop: MessageWriter<Chop>,
    mut mine: MessageWriter<Mine>,
) {
    if keys.just_pressed(KeyCode::KeyW) {
        chop.write(Chop { machine: *machine });
    }
    if keys.just_pressed(KeyCode::KeyS) {
        mine.write(Mine { machine: *machine });
    }
}

/// Each row reads the chart: waiting, working, or done.
fn draw_rows(q_active: Query<&Name, With<Active>>, mut rows: Query<(&Row, &mut Text2d, &mut TextColor)>) {
    let active = |s: &str| !s.is_empty() && q_active.iter().any(|n| n.as_str() == s);
    for (row, mut text, mut color) in &mut rows {
        let (status, c) = if active(row.done) {
            ("done", DONE)
        } else if active(row.working) {
            ("in progress", WORKING)
        } else {
            ("waiting", WAITING)
        };
        text.0 = format!("{}: {status}", row.label);
        color.0 = c;
    }
}

/// With `--features server`, lets the gearbox editor connect at `127.0.0.1:15703`.
fn editor_server(app: &mut App) {
    #[cfg(feature = "server")]
    app.add_plugins(bevy_gearbox::server::ServerPlugin::default());
    #[cfg(not(feature = "server"))]
    let _ = app;
}
