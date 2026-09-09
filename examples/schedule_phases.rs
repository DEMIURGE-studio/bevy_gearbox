//! How do I run logic inside the resolution loop, or on exit?
//!
//! Add systems to `GearboxSchedule` in one of its phases. The schedule loops
//! until the chart settles, and each iteration runs `ExitPhase`, `EntryPhase`
//! and `SideEffectPhase`, so a system there sees every step of a cascade, even
//! states entered and left within one frame. This turret also runs the whole
//! chart in `FixedUpdate` (`GearboxPlugin::default().schedule(FixedUpdate)`).
//!
//! ```text
//! Turret (StateMachine, initial = Idle)
//! ├── Idle    --Spot (click)-->            Aiming     SideEffectPhase: turn toward the click
//! ├── Aiming  --always, after 0.5s-->      Firing     EntryPhase: spawn the beam
//! └── Firing  --always, after 0.4s-->      Idle       ExitPhase:  remove the beam
//! ```
//!
//! - `spawn_beam` runs in `EntryPhase` on `Added<Active>`.
//! - `remove_beam` runs in `ExitPhase` on `RemovedComponents<Active>`.
//! - `aim` runs in `SideEffectPhase` and reads the click position from
//!   `Matched<Spot>`, the message that caused the transition.
//! - `Aiming` also has an `on(ExitState)` observer, the callback flavour of
//!   the same hook, which resets the barrel colour.
//!
//! Click anywhere to spot a target.
//!
//! ```sh
//! cargo run --example schedule_phases
//! # or, to let the gearbox editor connect on 127.0.0.1:15703:
//! cargo run --example schedule_phases --features server
//! ```

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::{GearboxPlugin, Matched};

/// A target was spotted at `at` (world position).
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Spot {
    #[gearbox(target)]
    machine: Entity,
    at: Vec2,
}

/// Marks the barrel sprite.
#[derive(Component)]
struct Barrel;
/// Marks the beam spawned while `Firing` is active.
#[derive(Component)]
struct Beam;

const BARREL_IDLE: Color = Color::srgb(0.6, 0.6, 0.7);
const BARREL_AIMING: Color = Color::srgb(1.0, 0.8, 0.3);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        // The chart resolves at the fixed timestep, like physics would.
        .add_plugins(GearboxPlugin::default().schedule(FixedUpdate))
        .add_plugins(editor_server)
        .add_systems(Startup, setup)
        .add_systems(Update, spot_on_click)
        .add_systems(
            GearboxSchedule,
            (
                remove_beam.in_set(GearboxPhase::ExitPhase),
                spawn_beam.in_set(GearboxPhase::EntryPhase),
                aim.in_set(GearboxPhase::SideEffectPhase),
            ),
        )
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text2d::new("Click anywhere: the turret aims, then fires"),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 200.0, 0.0),
    ));
    commands.spawn(Sprite::from_color(Color::srgb(0.3, 0.3, 0.35), Vec2::splat(40.0)));
    commands.spawn((
        Barrel,
        Sprite::from_color(BARREL_IDLE, Vec2::new(60.0, 12.0)),
        Transform::from_xyz(0.0, 0.0, 1.0),
    ));

    commands.spawn_scene(bsn! {
        #Turret
            StateMachineId("turret")
            StateMachine InitialState(#Idle)
        Substates [
            #Idle Transitions [ (Target(#Aiming) MessageEdge::<Spot>) ],
            #Aiming
                on(|_exit: On<ExitState>, mut barrel: Single<&mut Sprite, With<Barrel>>| {
                    barrel.color = BARREL_IDLE;
                })
                Transitions [ (Target(#Firing) AlwaysEdge Delay::from_secs_f32(0.5)) ],
            #Firing Transitions [ (Target(#Idle) AlwaysEdge Delay::from_secs_f32(0.4)) ],
        ]
    });
}

/// Turn a click into a `Spot` at the cursor's world position.
fn spot_on_click(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform)>,
    machine: Single<Entity, With<StateMachine>>,
    mut writer: MessageWriter<Spot>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let (camera, camera_transform) = *camera;
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if let Ok(at) = camera.viewport_to_world_2d(camera_transform, cursor) {
        writer.write(Spot { machine: *machine, at });
    }
}

/// SideEffectPhase: the transition's own message is available as `Matched`.
/// Turn the barrel toward the click for the `Spot` that was actually taken.
fn aim(
    mut matched: MessageReader<Matched<Spot>>,
    blocked: Res<BlockedEdges>,
    mut barrel: Single<(&mut Transform, &mut Sprite), With<Barrel>>,
) {
    for m in matched.read() {
        if blocked.is_blocked(m.edge) {
            continue;
        }
        let (transform, sprite) = &mut *barrel;
        let angle = m.message.at.to_angle();
        transform.rotation = Quat::from_rotation_z(angle);
        transform.translation = (Vec2::from_angle(angle) * 30.0).extend(1.0);
        sprite.color = BARREL_AIMING;
    }
}

/// EntryPhase: `Firing` was just entered, put a beam along the barrel.
fn spawn_beam(
    q_entered: Query<&Name, Added<Active>>,
    barrel: Single<&Transform, With<Barrel>>,
    mut commands: Commands,
) {
    if q_entered.iter().any(|n| n.as_str() == "Firing") {
        let dir = barrel.rotation * Vec3::X;
        commands.spawn((
            Beam,
            Sprite::from_color(Color::srgb(1.0, 0.3, 0.3), Vec2::new(600.0, 4.0)),
            Transform::from_translation(dir * 330.0).with_rotation(barrel.rotation),
        ));
    }
}

/// ExitPhase: `Firing` was just exited, take the beam down.
fn remove_beam(
    mut removed: RemovedComponents<Active>,
    q_name: Query<&Name>,
    beams: Query<Entity, With<Beam>>,
    mut commands: Commands,
) {
    for state in removed.read() {
        if q_name.get(state).is_ok_and(|n| n.as_str() == "Firing") {
            for beam in &beams {
                commands.entity(beam).despawn();
            }
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
