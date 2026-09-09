//! How do I run independent regions, and make one depend on another?
//!
//! Two parallel regions driven by keyboard input - authored as a `bsn!` scene
//! and run as a real Bevy app:
//!
//! ```text
//! Character (StateMachine, no InitialState -> parallel root: both regions run)
//! ├── Posture (initial = Standing):  <C> Crouch / <S> Stand
//! └── Weapon  (initial = Holstered): <D> Draw / <H> Holster   (both only while Standing)
//! ```
//!
//! The root has no `InitialState`, so it's a *parallel* parent: both regions are
//! active at once and transition independently - drawing your weapon doesn't
//! change your posture. The one link between them is a guard: the `Draw` and
//! `Holster` edges carry `InState(#Standing)`, so both are vetoed while
//! crouching. Each leaf
//! carries an `on(..)` entry observer that writes its region's on-screen label.
//!
//! ```sh
//! cargo run --example parallel_regions
//! # or, to let the gearbox editor connect on 127.0.0.1:15703:
//! cargo run --example parallel_regions --features server
//! ```

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
struct Stand {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Draw {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Holster {
    #[gearbox(target)]
    machine: Entity,
}

/// Marks the on-screen label for the posture region.
#[derive(Component)]
struct PostureText;
/// Marks the on-screen label for the weapon region.
#[derive(Component)]
struct WeaponText;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GearboxPlugin::default())
        .add_plugins(editor_server)
        .add_systems(Startup, setup)
        .add_systems(Update, input.before(GearboxSet))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text2d::new("<C> crouch  <S> stand   <D> draw  <H> holster  (weapon changes need standing)"),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 200.0, 0.0),
    ));
    commands.spawn((
        PostureText,
        Text2d::new(""),
        TextColor(Color::srgb(0.6, 0.8, 1.0)),
        Transform::from_xyz(0.0, 40.0, 0.0),
    ));
    commands.spawn((
        WeaponText,
        Text2d::new(""),
        TextColor(Color::srgb(1.0, 0.8, 0.6)),
        Transform::from_xyz(0.0, -40.0, 0.0),
    ));

    commands.spawn_scene(bsn! {
        // No `InitialState` on the root -> PARALLEL parent: every region runs.
        #Character
            StateMachineId("character")
            StateMachine
        Substates [
            // A region with an `InitialState` is sequential: one child active.
            #Posture InitialState(#Standing) Substates [
                #Standing
                    on(label::<PostureText>("Posture: Standing"))
                    Transitions [ (Target(#Crouching) MessageEdge::<Crouch>) ],
                #Crouching
                    on(label::<PostureText>("Posture: Crouching"))
                    Transitions [ (Target(#Standing) MessageEdge::<Stand>) ],
            ],
            #Weapon InitialState(#Holstered) Substates [
                #Holstered
                    on(label::<WeaponText>("Weapon: Holstered"))
                    // A built-in guard: taken only while the posture region is in Standing.
                    Transitions [ (Target(#Drawn) MessageEdge::<Draw> InState(#Standing)) ],
                #Drawn
                    on(label::<WeaponText>("Weapon: Drawn"))
                    Transitions [ (Target(#Holstered) MessageEdge::<Holster> InState(#Standing)) ],
            ],
        ]
    });
}

/// Each key drives one region; the other is untouched.
fn input(
    keys: Res<ButtonInput<KeyCode>>,
    machine: Single<Entity, With<StateMachine>>,
    mut crouch: MessageWriter<Crouch>,
    mut stand: MessageWriter<Stand>,
    mut draw: MessageWriter<Draw>,
    mut holster: MessageWriter<Holster>,
) {
    let m = *machine;
    if keys.just_pressed(KeyCode::KeyC) {
        crouch.write(Crouch { machine: m });
    }
    if keys.just_pressed(KeyCode::KeyS) {
        stand.write(Stand { machine: m });
    }
    if keys.just_pressed(KeyCode::KeyD) {
        draw.write(Draw { machine: m });
    }
    if keys.just_pressed(KeyCode::KeyH) {
        holster.write(Holster { machine: m });
    }
}

/// Entry action: write `text` into the label marked by `L`.
fn label<L: Component>(text: &'static str) -> impl Fn(On<EnterState>, Single<&mut Text2d, With<L>>) + Clone {
    move |_enter, mut label| label.0 = text.into()
}

/// With `--features server`, lets the gearbox editor connect at `127.0.0.1:15703`.
fn editor_server(app: &mut App) {
    #[cfg(feature = "server")]
    app.add_plugins(bevy_gearbox::server::ServerPlugin::default());
    #[cfg(not(feature = "server"))]
    let _ = app;
}
