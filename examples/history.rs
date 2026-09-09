//! How do I resume where I left off?
//!
//! Give the state a `History` component. When `Playing` is exited for
//! `Paused` it remembers its active substates; re-entering it restores them
//! instead of following `InitialState`. `History::Deep` restores the exact
//! leaf (the level *and* whether you were at its boss); change it to
//! `History::Shallow` and resuming lands on the remembered level's `Start`.
//! A `ResetEdge` forgets the history, so `Restart` begins at `Level1` again.
//!
//! ```text
//! Game (StateMachine, initial = Playing)
//! ├── Playing (History::Deep, initial = Level1)  --Pause (Esc)--> Paused
//! │   ├── Level1 (initial = Start): Start --Boss (B)--> Boss    --Next (N)--> Level2
//! │   ├── Level2 (initial = Start): Start --Boss (B)--> Boss    --Next (N)--> Level3
//! │   └── Level3 (initial = Start): Start --Boss (B)--> Boss
//! └── Paused  --Resume (Esc)--> Playing                           history restores the leaf
//!             --Restart (R)--> Playing  ResetEdge(ResetScope::Target)  history is cleared
//! ```
//!
//! ```sh
//! cargo run --example history
//! # or, to let the gearbox editor connect on 127.0.0.1:15703:
//! cargo run --example history --features server
//! ```

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Next {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct ToBoss {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Pause {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Resume {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Restart {
    #[gearbox(target)]
    machine: Entity,
}

/// Marks the box for level `n`.
#[derive(Component)]
struct LevelBox(u8);
/// Marks the label under the boxes.
#[derive(Component)]
struct StatusText;

const DIM: Color = Color::srgb(0.25, 0.25, 0.3);
const START: Color = Color::srgb(0.4, 0.8, 1.0);
const BOSS: Color = Color::srgb(1.0, 0.4, 0.4);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GearboxPlugin::default())
        .add_plugins(editor_server)
        .add_systems(Startup, setup)
        .add_systems(Update, input.before(GearboxSet))
        .add_systems(Update, draw.after(GearboxSet))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text2d::new("<N> next level   <B> to the boss   <Esc> pause / resume   <R> restart"),
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
            #Playing InitialState(#Level1) History::Deep
                Transitions [ (Target(#Paused) MessageEdge::<Pause>) ]
                Substates [
                    #Level1 InitialState(#Start1)
                        Transitions [ (Target(#Level2) MessageEdge::<Next>) ]
                        Substates [
                            #Start1 Transitions [ (Target(#Boss1) MessageEdge::<ToBoss>) ],
                            #Boss1,
                        ],
                    #Level2 InitialState(#Start2)
                        Transitions [ (Target(#Level3) MessageEdge::<Next>) ]
                        Substates [
                            #Start2 Transitions [ (Target(#Boss2) MessageEdge::<ToBoss>) ],
                            #Boss2,
                        ],
                    #Level3 InitialState(#Start3)
                        Substates [
                            #Start3 Transitions [ (Target(#Boss3) MessageEdge::<ToBoss>) ],
                            #Boss3,
                        ],
                ],
            #Paused Transitions [
                // Re-entering Playing restores its history.
                (Target(#Playing) MessageEdge::<Resume>),
                // ResetEdge clears the history under the target first.
                (Target(#Playing) MessageEdge::<Restart> ResetEdge(ResetScope::Target)),
            ],
        ]
    });
}

fn input(
    keys: Res<ButtonInput<KeyCode>>,
    machine: Single<Entity, With<StateMachine>>,
    q_active: Query<&Name, With<Active>>,
    mut next: MessageWriter<Next>,
    mut boss: MessageWriter<ToBoss>,
    mut pause: MessageWriter<Pause>,
    mut resume: MessageWriter<Resume>,
    mut restart: MessageWriter<Restart>,
) {
    let m = *machine;
    let paused = q_active.iter().any(|n| n.as_str() == "Paused");
    if keys.just_pressed(KeyCode::KeyN) {
        next.write(Next { machine: m });
    }
    if keys.just_pressed(KeyCode::KeyB) {
        boss.write(ToBoss { machine: m });
    }
    if keys.just_pressed(KeyCode::Escape) {
        if paused {
            resume.write(Resume { machine: m });
        } else {
            pause.write(Pause { machine: m });
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        restart.write(Restart { machine: m });
    }
}

/// Light the active level (red at its boss) and say whether we're paused.
fn draw(
    q_active: Query<&Name, With<Active>>,
    mut boxes: Query<(&LevelBox, &mut Sprite)>,
    mut text: Single<&mut Text2d, With<StatusText>>,
) {
    let active = |s: &str| q_active.iter().any(|n| n.as_str() == s);
    for (level, mut sprite) in &mut boxes {
        let n = level.0;
        sprite.color = if active(&format!("Boss{n}")) {
            BOSS
        } else if active(&format!("Start{n}")) {
            START
        } else {
            DIM
        };
    }
    text.0 = if active("Paused") {
        "PAUSED  (Esc resumes where you were, R restarts at Level 1)".into()
    } else {
        "Playing".into()
    };
}

/// With `--features server`, lets the gearbox editor connect at `127.0.0.1:15703`.
fn editor_server(app: &mut App) {
    #[cfg(feature = "server")]
    app.add_plugins(bevy_gearbox::server::ServerPlugin::default());
    #[cfg(not(feature = "server"))]
    let _ = app;
}
