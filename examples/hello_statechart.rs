//! How do I build and run a chart?
//!
//! A light switch: two states, one message, and a system that reads the
//! result. Authored as a `bsn!` scene and run as a real Bevy app.
//!
//! ```text
//! Light (StateMachine, initial = Off)
//! ├── Off --Toggle (Space)--> On
//! └── On  --Toggle (Space)--> Off
//! ```
//!
//! `Toggle` is an ordinary Bevy message. The input system writes it before
//! `GearboxSet`; gearbox resolves the transition; the bulb system runs after
//! `GearboxSet` and repaints whenever a state gains `Active`. No observers,
//! no callbacks: states are entities and `Active` is a component.
//!
//! Press **Space** to toggle. Close the window to quit.
//!
//! ```sh
//! cargo run --example hello_statechart
//! # or, to let the gearbox editor connect on 127.0.0.1:15703:
//! cargo run --example hello_statechart --features server
//! ```

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

/// The message that drives the chart. `#[gearbox(target)]` names the entity
/// it is addressed to: the machine root.
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Toggle {
    #[gearbox(target)]
    machine: Entity,
}

/// Marks the bulb sprite.
#[derive(Component)]
struct Bulb;

const OFF: Color = Color::srgb(0.25, 0.25, 0.3);
const ON: Color = Color::srgb(1.0, 0.9, 0.3);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GearboxPlugin::default())
        .add_plugins(editor_server)
        .add_systems(Startup, setup)
        .add_systems(Update, toggle_on_space.before(GearboxSet))
        .add_systems(Update, paint_bulb.after(GearboxSet))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text2d::new("Press <Space> to toggle the light"),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 200.0, 0.0),
    ));
    commands.spawn((
        Bulb,
        Sprite::from_color(OFF, Vec2::splat(120.0)),
        Transform::default(),
    ));

    // The whole chart. `#Name` both names the entity and lets other parts of
    // the scene refer to it; `Substates [ .. ]` nests states under the root,
    // `Transitions [ .. ]` hangs edges off a state.
    commands.spawn_scene(bsn! {
        #Light
            StateMachineId("light")
            StateMachine InitialState(#Off)
        Substates [
            #Off Transitions [ (Target(#On) MessageEdge::<Toggle>) ],
            #On  Transitions [ (Target(#Off) MessageEdge::<Toggle>) ],
        ]
    });
}

/// Write a `Toggle` to the machine. Runs before `GearboxSet` so the transition
/// resolves this frame.
fn toggle_on_space(
    keys: Res<ButtonInput<KeyCode>>,
    machine: Single<Entity, With<StateMachine>>,
    mut writer: MessageWriter<Toggle>,
) {
    if keys.just_pressed(KeyCode::Space) {
        writer.write(Toggle { machine: *machine });
    }
}

/// React to a state being entered: every state that gained `Active` this
/// frame shows up in `Added<Active>`. Runs after `GearboxSet`.
fn paint_bulb(
    q_entered: Query<&Name, Added<Active>>,
    mut bulb: Single<&mut Sprite, With<Bulb>>,
) {
    for name in &q_entered {
        match name.as_str() {
            "On" => bulb.color = ON,
            "Off" => bulb.color = OFF,
            _ => {}
        }
    }
}

/// With `--features server`, lets the gearbox editor connect at `127.0.0.1:15703`.
fn editor_server(app: &mut App) {
    #[cfg(feature = "server")]
    app.add_plugins(bevy_gearbox::server::ServerPlugin::default());
    #[cfg(not(feature = "server"))]
    let _ = app;
}
