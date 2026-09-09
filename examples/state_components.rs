//! How do I query "which characters are walking" from a normal system?
//!
//! Mirror the state onto the machine root with a state component. While a
//! state carrying `StateComponent::<T>` is active, the root has `T`; when it
//! is exited, `T` is removed. Ordinary systems then filter on `With<T>` or
//! read `T`'s data, with no knowledge of the chart.
//!
//! ```text
//! Mover (StateMachine, initial = Standing, carries Controllable)
//! ├── Standing   --Move (W)--> Walking
//! ├── Walking    StateComponent::<Moving>  StateComponent::<Speed>(Speed(120.0))
//! │              --Sprint (Shift)--> Sprinting   --Move (W)--> Standing
//! ├── Sprinting  StateComponent::<Moving>  StateComponent::<Speed>(Speed(320.0))
//! │              --Move (W)--> Standing
//! └── Stunned    StateInactiveComponent::<Controllable>   --always, after 1.2s--> Standing
//!     (reached from any state: the Stun (X) edge is on the root)
//! ```
//!
//! Three forms, each with its own consumer:
//! - `StateComponent::<Moving>`: a marker. `bob` animates every root `With<Moving>`.
//! - `StateComponent::<Speed>(Speed(..))`: a payload. `slide` reads `&Speed`.
//! - `StateInactiveComponent::<Controllable>`: removed *while the state is
//!   active* and restored on exit. `input` requires `With<Controllable>`, so
//!   keys do nothing while stunned.
//!
//! ```sh
//! cargo run --example state_components
//! # or, to let the gearbox editor connect on 127.0.0.1:15703:
//! cargo run --example state_components --features server
//! ```

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Move {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Sprint {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Stun {
    #[gearbox(target)]
    machine: Entity,
}

/// On the root while Walking or Sprinting.
#[state_component]
#[derive(Component, Clone, Default)]
struct Moving;

/// On the root while Walking or Sprinting, with the state's own value.
#[state_component]
#[derive(Component, Clone, Default)]
struct Speed(f32);

/// On the root except while Stunned.
#[state_component]
#[derive(Component, Clone, Default)]
struct Controllable;

/// Marks the sprite that shows the mover.
#[derive(Component)]
struct Dot;
/// Marks the status label.
#[derive(Component)]
struct StatusText;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GearboxPlugin::default())
        .add_plugins(editor_server)
        .add_systems(Startup, setup)
        .add_systems(Update, input.before(GearboxSet))
        .add_systems(Update, (slide, bob, show_status).after(GearboxSet))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text2d::new("<W> walk / stop   <Shift> sprint   <X> stun (input is ignored while stunned)"),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 200.0, 0.0),
    ));
    commands.spawn((
        Dot,
        Sprite::from_color(Color::srgb(0.4, 0.8, 1.0), Vec2::splat(40.0)),
        Transform::from_xyz(-300.0, 0.0, 0.0),
    ));
    commands.spawn((
        StatusText,
        Text2d::new(""),
        TextColor(Color::srgb(0.9, 0.85, 0.5)),
        Transform::from_xyz(0.0, -120.0, 0.0),
    ));

    commands.spawn_scene(bsn! {
        #Mover
            StateMachineId("mover")
            Controllable
            StateMachine InitialState(#Standing)
            // On the root, so it applies from every state.
            Transitions [ (Target(#Stunned) MessageEdge::<Stun>) ]
        Substates [
            #Standing Transitions [ (Target(#Walking) MessageEdge::<Move>) ],
            #Walking
                StateComponent::<Moving>
                StateComponent::<Speed>(Speed(120.0))
                Transitions [
                    (Target(#Sprinting) MessageEdge::<Sprint>),
                    (Target(#Standing)  MessageEdge::<Move>),
                ],
            #Sprinting
                StateComponent::<Moving>
                StateComponent::<Speed>(Speed(320.0))
                Transitions [ (Target(#Standing) MessageEdge::<Move>) ],
            #Stunned
                StateInactiveComponent::<Controllable>
                Transitions [ (Target(#Standing) AlwaysEdge Delay::from_secs_f32(1.2)) ],
        ]
    });
}

/// Only a controllable machine takes input. While `Stunned` is active the
/// root has no `Controllable`, so the query is empty and keys are ignored.
fn input(
    keys: Res<ButtonInput<KeyCode>>,
    machine: Query<Entity, (With<StateMachine>, With<Controllable>)>,
    mut mv: MessageWriter<Move>,
    mut sprint: MessageWriter<Sprint>,
    mut stun: MessageWriter<Stun>,
) {
    let Ok(m) = machine.single() else {
        return;
    };
    if keys.just_pressed(KeyCode::KeyW) {
        mv.write(Move { machine: m });
    }
    if keys.just_pressed(KeyCode::ShiftLeft) {
        sprint.write(Sprint { machine: m });
    }
    if keys.just_pressed(KeyCode::KeyX) {
        stun.write(Stun { machine: m });
    }
}

/// Reads the state's payload: the root carries `Speed` only while moving.
fn slide(time: Res<Time>, speed: Query<&Speed, With<StateMachine>>, mut dot: Single<&mut Transform, With<Dot>>) {
    let Ok(speed) = speed.single() else {
        return;
    };
    dot.translation.x += speed.0 * time.delta_secs();
    if dot.translation.x > 320.0 {
        dot.translation.x = -320.0;
    }
}

/// Filters on the marker: bobs while moving, rests otherwise.
fn bob(time: Res<Time>, moving: Query<(), (With<StateMachine>, With<Moving>)>, mut dot: Single<&mut Transform, With<Dot>>) {
    dot.translation.y = if moving.is_empty() { 0.0 } else { (time.elapsed_secs() * 12.0).sin() * 8.0 };
}

fn show_status(
    root: Single<(Option<&Speed>, Has<Moving>, Has<Controllable>), With<StateMachine>>,
    mut text: Single<&mut Text2d, With<StatusText>>,
) {
    let (speed, moving, controllable) = *root;
    text.0 = format!(
        "Moving: {moving}   Speed: {}   Controllable: {controllable}",
        speed.map(|s| s.0.to_string()).unwrap_or_else(|| "none".into())
    );
}

/// With `--features server`, lets the gearbox editor connect at `127.0.0.1:15703`.
fn editor_server(app: &mut App) {
    #[cfg(feature = "server")]
    app.add_plugins(bevy_gearbox::server::ServerPlugin::default());
    #[cfg(not(feature = "server"))]
    let _ = app;
}
