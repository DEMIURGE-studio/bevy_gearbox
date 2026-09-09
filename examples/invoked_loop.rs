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
//! Entry actions are `on(..)` observers attached to the state entities inside
//! the scene: each state recolors the orb when entered, and `Invoking` also
//! launches a projectile. Close the window to quit.
//!
//! ```sh
//! cargo run --example invoked_loop
//! # or, to let the gearbox editor connect on 127.0.0.1:15703:
//! cargo run --example invoked_loop --features server
//! ```

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

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
        .add_plugins(GearboxPlugin::default())
        .add_plugins(editor_server)
        .add_systems(Startup, setup)
        .add_systems(Update, fire_on_space.before(GearboxSet))
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

    // The state machine. `StateMachineId` lets the editor identify (and save) it.
    // `on(..)` attaches an `EnterState` observer to the state: its entry action.
    commands.spawn_scene(bsn! {
        #Ability
            StateMachineId("ability")
            StateMachine InitialState(#Ready)
        Substates [
            #Ready
                on(recolor(READY))
                Transitions [ (Target(#Invoking) MessageEdge::<Fire>) ],
            #Invoking
                on(recolor(INVOKING))
                on(launch_projectile)
                Transitions [ (Target(#Cooldown) AlwaysEdge Delay::from_secs_f32(0.2)) ],
            #Cooldown
                on(recolor(COOLDOWN))
                Transitions [ (Target(#Ready) AlwaysEdge Delay::from_secs_f32(0.8)) ],
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

/// Entry action: paint the orb with this state's color.
fn recolor(color: Color) -> impl Fn(On<EnterState>, Single<&mut Sprite, With<Orb>>) + Clone {
    move |_enter, mut orb| orb.color = color
}

/// Entry action for `Invoking`: launch a projectile from the orb.
fn launch_projectile(_enter: On<EnterState>, mut commands: Commands) {
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

/// With `--features server`, lets the gearbox editor connect at `127.0.0.1:15703`.
fn editor_server(app: &mut App) {
    #[cfg(feature = "server")]
    app.add_plugins(bevy_gearbox::server::ServerPlugin::default());
    #[cfg(not(feature = "server"))]
    let _ = app;
}
