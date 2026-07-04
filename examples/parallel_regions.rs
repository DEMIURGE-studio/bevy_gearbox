//! Two parallel regions driven by keyboard input - authored as a `bsn!` scene
//! and run as a real Bevy app:
//!
//! ```text
//! Character (StateMachine, no InitialState -> parallel root: both regions run)
//! ├── Posture (initial = Standing):  <C> Crouch / <S> Stand
//! └── Weapon  (initial = Holstered): <D> Draw   / <H> Holster
//! ```
//!
//! The root has no `InitialState`, so it's a *parallel* parent: both regions are
//! active at once and transition independently - drawing your weapon doesn't
//! change your posture. On-screen text mirrors each region's current state.
//!
//! ```sh
//! cargo run --example parallel_regions
//! ```

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::server::{ServerPlugin, StateMachineId};
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
        .add_plugins(ServerPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, input.before(GearboxSet))
        .add_systems(Update, update_region_labels.after(GearboxSet))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text2d::new("<C> crouch  <S> stand   <D> draw  <H> holster"),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 200.0, 0.0),
    ));
    commands.spawn((
        PostureText,
        Text2d::new("Posture: Standing"),
        TextColor(Color::srgb(0.6, 0.8, 1.0)),
        Transform::from_xyz(0.0, 40.0, 0.0),
    ));
    commands.spawn((
        WeaponText,
        Text2d::new("Weapon: Holstered"),
        TextColor(Color::srgb(1.0, 0.8, 0.6)),
        Transform::from_xyz(0.0, -40.0, 0.0),
    ));

    commands.spawn_scene(bsn! {
        // No `InitialState` on the root -> PARALLEL parent: every region runs.
        #Character
            template(|_| Ok(StateMachineId::new("character")))
            StateMachine
        Substates [
            // A region with an `InitialState` is sequential: one child active.
            #Posture InitialState(#Standing) Substates [
                #Standing Transitions [
                    (Target(#Crouching) MessageEdge::<Crouch>::default())
                ],
                #Crouching Transitions [
                    (Target(#Standing) MessageEdge::<Stand>::default())
                ],
            ],
            #Weapon InitialState(#Holstered) Substates [
                #Holstered Transitions [
                    (Target(#Drawn) MessageEdge::<Draw>::default())
                ],
                #Drawn Transitions [
                    (Target(#Holstered) MessageEdge::<Holster>::default())
                ],
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

/// Mirror each region's active leaf into its on-screen label (keyed off `Name`).
fn update_region_labels(
    q_entered: Query<&Name, Added<Active>>,
    mut q_posture: Query<&mut Text2d, (With<PostureText>, Without<WeaponText>)>,
    mut q_weapon: Query<&mut Text2d, (With<WeaponText>, Without<PostureText>)>,
) {
    for name in &q_entered {
        match name.as_str() {
            "Standing" | "Crouching" => {
                if let Ok(mut text) = q_posture.single_mut() {
                    text.0 = format!("Posture: {}", name.as_str());
                }
            }
            "Holstered" | "Drawn" => {
                if let Ok(mut text) = q_weapon.single_mut() {
                    text.0 = format!("Weapon: {}", name.as_str());
                }
            }
            _ => {}
        }
    }
}
