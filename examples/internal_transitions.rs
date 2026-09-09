//! How do I react to a message without leaving the state?
//!
//! With an internal self-loop. `Playing` has two edges back to itself: the
//! `Coin` edge is `EdgeKind::Internal`, so the current level stays active and
//! `Playing`'s entry action does not run again; the `Restart` edge is external
//! (the default), so `Playing` is exited and re-entered, its entry action runs
//! again and the level goes back to `InitialState`.
//!
//! ```text
//! Game (StateMachine, initial = Playing)
//! └── Playing (initial = Level1)  --Coin (C), internal-->  Playing   stay put, score a coin
//!                                 --Restart (R)-------->  Playing   re-enter: run 2, Level1
//!     ├── Level1  --Next (N)--> Level2
//!     ├── Level2  --Next (N)--> Level3
//!     └── Level3
//! ```
//!
//! The coin itself is scored from `Matched<Coin>` in `SideEffectPhase`, the
//! place for logic that belongs to a transition rather than to a state.
//!
//! ```sh
//! cargo run --example internal_transitions
//! # or, to let the gearbox editor connect on 127.0.0.1:15703:
//! cargo run --example internal_transitions --features server
//! ```

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::{GearboxPlugin, Matched};

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
struct Restart {
    #[gearbox(target)]
    machine: Entity,
}

/// How many times `Playing` has been entered, and coins collected this run.
#[derive(Resource, Default)]
struct Score {
    runs: u32,
    coins: u32,
}

/// Marks the level box for level `n`.
#[derive(Component)]
struct LevelBox(u8);
/// Marks the status label.
#[derive(Component)]
struct StatusText;

const DIM: Color = Color::srgb(0.25, 0.25, 0.3);
const LIT: Color = Color::srgb(0.4, 0.8, 1.0);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GearboxPlugin::default())
        .add_plugins(editor_server)
        .init_resource::<Score>()
        .add_systems(Startup, setup)
        .add_systems(Update, input.before(GearboxSet))
        .add_systems(Update, (light_level, show_status).after(GearboxSet))
        .add_systems(GearboxSchedule, score_coins.in_set(GearboxPhase::SideEffectPhase))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text2d::new("<C> coin (internal: stay)   <N> next level   <R> restart (external: re-enter)"),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 200.0, 0.0),
    ));
    for n in 1..=3u8 {
        commands.spawn((
            LevelBox(n),
            Sprite::from_color(DIM, Vec2::splat(90.0)),
            Transform::from_xyz((n as f32 - 2.0) * 130.0, 20.0, 0.0),
        ));
        commands.spawn((
            Text2d::new(format!("Level {n}")),
            TextColor(Color::WHITE),
            Transform::from_xyz((n as f32 - 2.0) * 130.0, 20.0, 1.0),
        ));
    }
    commands.spawn((
        StatusText,
        Text2d::new(""),
        TextColor(Color::srgb(0.9, 0.85, 0.5)),
        Transform::from_xyz(0.0, -120.0, 0.0),
    ));

    commands.spawn_scene(bsn! {
        #Game
            StateMachineId("game")
            StateMachine InitialState(#Playing)
        Substates [
            #Playing InitialState(#Level1)
                on(new_run)
                Transitions [
                    // Internal: no exit, no entry, the active level is kept.
                    (Target(#Playing) MessageEdge::<Coin> EdgeKind::Internal),
                    // External (the default): Playing is exited and re-entered.
                    (Target(#Playing) MessageEdge::<Restart>),
                ]
                Substates [
                    #Level1 Transitions [ (Target(#Level2) MessageEdge::<Next>) ],
                    #Level2 Transitions [ (Target(#Level3) MessageEdge::<Next>) ],
                    #Level3,
                ],
        ]
    });
}

/// Entry action on `Playing`: a new run starts. Not triggered by the internal
/// `Coin` edge.
fn new_run(_enter: On<EnterState>, mut score: ResMut<Score>) {
    score.runs += 1;
    score.coins = 0;
}

/// Transition side effect: one coin per `Coin` transition actually taken.
fn score_coins(
    mut matched: MessageReader<Matched<Coin>>,
    blocked: Res<BlockedEdges>,
    mut score: ResMut<Score>,
) {
    for m in matched.read() {
        if !blocked.is_blocked(m.edge) {
            score.coins += 1;
        }
    }
}

fn input(
    keys: Res<ButtonInput<KeyCode>>,
    machine: Single<Entity, With<StateMachine>>,
    mut coin: MessageWriter<Coin>,
    mut next: MessageWriter<Next>,
    mut restart: MessageWriter<Restart>,
) {
    let m = *machine;
    if keys.just_pressed(KeyCode::KeyC) {
        coin.write(Coin { machine: m });
    }
    if keys.just_pressed(KeyCode::KeyN) {
        next.write(Next { machine: m });
    }
    if keys.just_pressed(KeyCode::KeyR) {
        restart.write(Restart { machine: m });
    }
}

fn light_level(q_active: Query<&Name, With<Active>>, mut boxes: Query<(&LevelBox, &mut Sprite)>) {
    for (level, mut sprite) in &mut boxes {
        let name = format!("Level{}", level.0);
        let lit = q_active.iter().any(|n| n.as_str() == name);
        sprite.color = if lit { LIT } else { DIM };
    }
}

fn show_status(score: Res<Score>, mut text: Single<&mut Text2d, With<StatusText>>) {
    text.0 = format!("Run {}   Coins {}", score.runs, score.coins);
}

/// With `--features server`, lets the gearbox editor connect at `127.0.0.1:15703`.
fn editor_server(app: &mut App) {
    #[cfg(feature = "server")]
    app.add_plugins(bevy_gearbox::server::ServerPlugin::default());
    #[cfg(not(feature = "server"))]
    let _ = app;
}
