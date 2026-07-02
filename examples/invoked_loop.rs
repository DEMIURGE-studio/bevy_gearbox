//! A player-fired ability with a cooldown - authored as a `bsn!` scene and run
//! as a real Bevy app. Press **Space** to fire: the ability enters `Invoking`
//! (which launches a projectile), then `Cooldown`, then returns to `Ready`.
//! Firing is gated - Space does nothing until the cooldown elapses.
//!
//! ```text
//! Ability (StateMachine, initial = Ready)
//! ├── Ready     --Fire (Space)-->       Invoking
//! ├── Invoking  --always, after 0.2s--> Cooldown   (launches a projectile on enter)
//! └── Cooldown  --always, after 0.8s--> Ready
//! ```
//!
//! The orb's color reflects the current state (green = Ready, yellow = Invoking,
//! red = Cooldown). The gearbox editor can attach while this runs - the example
//! serves the editor protocol on `127.0.0.1:15703`; open the editor and connect.
//! Close the window to quit.
//!
//! ```sh
//! cargo run --example invoked_loop
//! ```

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::server::{ServerPlugin, StateMachineId};
use bevy_gearbox::{gearbox_auto_register_plugin, GearboxPlugin};

/// Fires the ability: `Ready -> Invoking`. Addressed to the machine root.
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Fire {
    #[gearbox(target)]
    machine: Entity,
}

/// Marks the ability "orb" whose color tracks the current state.
#[derive(Component)]
struct Orb;

/// A projectile launched when the ability invokes.
#[derive(Component)]
struct Projectile;

const ORB_POS: Vec2 = Vec2::new(-300.0, 0.0);
const READY: Color = Color::srgb(0.3, 0.9, 0.4);
const INVOKING: Color = Color::srgb(1.0, 0.9, 0.2);
const COOLDOWN: Color = Color::srgb(0.9, 0.3, 0.3);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins((GearboxPlugin::default(), gearbox_auto_register_plugin))
        // Lets the gearbox editor connect at 127.0.0.1:15703. Remove this line
        // to run without the editor server.
        .add_plugins(ServerPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, fire_on_space.before(GearboxSet))
        .add_systems(
            Update,
            (recolor_orb, launch_on_invoke).after(GearboxSet),
        )
        .add_systems(Update, move_projectiles)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text2d::new("Press <Space> to fire"),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 220.0, 0.0),
    ));

    // The ability "orb"; its color reflects the current state.
    commands.spawn((
        Orb,
        Sprite {
            color: READY,
            custom_size: Some(Vec2::splat(64.0)),
            ..default()
        },
        Transform::from_translation(ORB_POS.extend(0.0)),
    ));

    // The state machine. The root carries a name and an editor id so the editor
    // can identify (and save) it.
    commands.spawn_scene(bsn! {
        #Ability
            template(|_| Ok(StateMachineId::new("ability")))
            StateMachine InitialState(#Ready)
        Substates [
            #Ready Transitions [
                (Target(#Invoking) MessageEdge::<Fire>::default())
            ],
            #Invoking Transitions [
                (Target(#Cooldown) AlwaysEdge Delay::from_secs_f32(0.2))
            ],
            #Cooldown Transitions [
                (Target(#Ready) AlwaysEdge Delay::from_secs_f32(0.8))
            ],
        ]
    });
}

/// Press Space to write a `Fire` message. If the machine isn't in `Ready` (no
/// active `Fire` edge), the message is simply ignored - that's the cooldown gate.
fn fire_on_space(
    keys: Res<ButtonInput<KeyCode>>,
    machine: Single<Entity, With<StateMachine>>,
    mut writer: MessageWriter<Fire>,
) {
    if keys.just_pressed(KeyCode::Space) {
        writer.write(Fire { machine: *machine });
    }
}

/// Recolor the orb whenever the ability enters a new state (keyed off the
/// state's `Name`).
fn recolor_orb(
    q_entered: Query<&Name, Added<Active>>,
    mut q_orb: Query<&mut Sprite, With<Orb>>,
) {
    for name in &q_entered {
        let color = match name.as_str() {
            "Ready" => READY,
            "Invoking" => INVOKING,
            "Cooldown" => COOLDOWN,
            _ => continue,
        };
        if let Ok(mut sprite) = q_orb.single_mut() {
            sprite.color = color;
        }
    }
}

/// When `Invoking` is entered, launch a projectile from the orb.
fn launch_on_invoke(q_entered: Query<&Name, Added<Active>>, mut commands: Commands) {
    for name in &q_entered {
        if name.as_str() == "Invoking" {
            commands.spawn((
                Projectile,
                Sprite {
                    color: INVOKING,
                    custom_size: Some(Vec2::new(28.0, 10.0)),
                    ..default()
                },
                Transform::from_translation(ORB_POS.extend(0.0)),
            ));
        }
    }
}

fn move_projectiles(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Transform), With<Projectile>>,
) {
    for (entity, mut transform) in &mut q {
        transform.translation.x += 700.0 * time.delta_secs();
        if transform.translation.x > 400.0 {
            commands.entity(entity).despawn();
        }
    }
}
