//! How do I share one transition across several states?
//!
//! Put the edge on their common ancestor. A message that no active leaf
//! handles is offered to the leaf's ancestors, so `Alive`'s single `Damage`
//! edge covers every state nested under it. Edges stay between siblings:
//! `Respawn` targets `Alive`, and `Alive`'s `InitialState` chain does the
//! rest, landing on `Grounded` and then `Idle`.
//!
//! ```text
//! Character (StateMachine, initial = Alive)
//! ├── Alive (initial = Grounded)      --Damage (X)-->  Dead      one edge for every descendant
//! │   ├── Grounded (initial = Idle)   --Jump (Space)--> Airborne
//! │   │   ├── Idle     --Walk (W)--> Walking
//! │   │   └── Walking  --Walk (W)--> Idle
//! │   └── Airborne  --always, after 0.6s--> Grounded
//! └── Dead  --Respawn (R)--> Alive     InitialState chain: Grounded, then Idle
//! ```
//!
//! The label shows the full active path, root to leaf, read straight from the
//! `StateMachine` component and the `SubstateOf` relationship.
//!
//! ```sh
//! cargo run --example hierarchy
//! # or, to let the gearbox editor connect on 127.0.0.1:15703:
//! cargo run --example hierarchy --features server
//! ```

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Walk {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Jump {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Damage {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Respawn {
    #[gearbox(target)]
    machine: Entity,
}

/// Marks the character sprite.
#[derive(Component)]
struct Figure;
/// Marks the active-path label.
#[derive(Component)]
struct PathText;

const IDLE: Color = Color::srgb(0.6, 0.6, 0.7);
const WALKING: Color = Color::srgb(0.4, 0.7, 1.0);
const AIRBORNE: Color = Color::srgb(1.0, 0.9, 0.3);
const DEAD: Color = Color::srgb(0.8, 0.2, 0.2);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GearboxPlugin::default())
        .add_plugins(editor_server)
        .add_systems(Startup, setup)
        .add_systems(Update, input.before(GearboxSet))
        .add_systems(Update, (draw_figure, show_path).after(GearboxSet))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text2d::new("<W> walk/stop   <Space> jump   <X> take damage   <R> respawn"),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 200.0, 0.0),
    ));
    commands.spawn((
        Figure,
        Sprite::from_color(IDLE, Vec2::new(50.0, 80.0)),
        Transform::from_xyz(0.0, -40.0, 0.0),
    ));
    commands.spawn((
        PathText,
        Text2d::new(""),
        TextColor(Color::srgb(0.7, 0.9, 0.7)),
        Transform::from_xyz(0.0, -160.0, 0.0),
    ));

    commands.spawn_scene(bsn! {
        #Character
            StateMachineId("character")
            StateMachine InitialState(#Alive)
        Substates [
            // `Damage` is handled here, once, for Idle, Walking and Airborne alike.
            #Alive InitialState(#Grounded)
                Transitions [ (Target(#Dead) MessageEdge::<Damage>) ]
                Substates [
                    #Grounded InitialState(#Idle)
                        Transitions [ (Target(#Airborne) MessageEdge::<Jump>) ]
                        Substates [
                            #Idle    Transitions [ (Target(#Walking) MessageEdge::<Walk>) ],
                            #Walking Transitions [ (Target(#Idle)    MessageEdge::<Walk>) ],
                        ],
                    #Airborne Transitions [
                        (Target(#Grounded) AlwaysEdge Delay::from_secs_f32(0.6))
                    ],
                ],
            // Back to the sibling; Alive's InitialState chain picks Grounded, then Idle.
            #Dead Transitions [ (Target(#Alive) MessageEdge::<Respawn>) ],
        ]
    });
}

fn input(
    keys: Res<ButtonInput<KeyCode>>,
    machine: Single<Entity, With<StateMachine>>,
    mut walk: MessageWriter<Walk>,
    mut jump: MessageWriter<Jump>,
    mut damage: MessageWriter<Damage>,
    mut respawn: MessageWriter<Respawn>,
) {
    let m = *machine;
    if keys.just_pressed(KeyCode::KeyW) {
        walk.write(Walk { machine: m });
    }
    if keys.just_pressed(KeyCode::Space) {
        jump.write(Jump { machine: m });
    }
    if keys.just_pressed(KeyCode::KeyX) {
        damage.write(Damage { machine: m });
    }
    if keys.just_pressed(KeyCode::KeyR) {
        respawn.write(Respawn { machine: m });
    }
}

/// Color and height follow the active leaf; walking slides the figure.
fn draw_figure(
    time: Res<Time>,
    q_active: Query<&Name, With<Active>>,
    mut figure: Single<(&mut Sprite, &mut Transform), With<Figure>>,
) {
    let active = |s: &str| q_active.iter().any(|n| n.as_str() == s);
    let (sprite, transform) = &mut *figure;
    sprite.color = if active("Dead") {
        DEAD
    } else if active("Airborne") {
        AIRBORNE
    } else if active("Walking") {
        WALKING
    } else {
        IDLE
    };
    transform.translation.y = if active("Airborne") { 40.0 } else { -40.0 };
    if active("Walking") {
        transform.translation.x += 120.0 * time.delta_secs();
        if transform.translation.x > 300.0 {
            transform.translation.x = -300.0;
        }
    }
}

/// The active configuration is a path from the root to the leaf: walk
/// `SubstateOf` upward from the leaf recorded in `StateMachine`.
fn show_path(
    machine: Single<&StateMachine>,
    q_name: Query<&Name>,
    q_parent: Query<&SubstateOf>,
    mut text: Single<&mut Text2d, With<PathText>>,
) {
    let Some(&leaf) = machine.active_leaves.iter().next() else {
        return;
    };
    let mut path: Vec<&str> = std::iter::once(leaf)
        .chain(q_parent.iter_ancestors(leaf))
        .filter_map(|e| q_name.get(e).ok().map(|n| n.as_str()))
        .collect();
    path.reverse();
    text.0 = path.join("  >  ");
}

/// With `--features server`, lets the gearbox editor connect at `127.0.0.1:15703`.
fn editor_server(app: &mut App) {
    #[cfg(feature = "server")]
    app.add_plugins(bevy_gearbox::server::ServerPlugin::default());
    #[cfg(not(feature = "server"))]
    let _ = app;
}
