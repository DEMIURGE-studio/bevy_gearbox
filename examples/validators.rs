//! How do I filter a message by its payload without a guard system?
//!
//! A three-digit vault. One message type, `Press { key }`, feeds every edge;
//! each edge carries a validator that accepts only the digit it wants.
//!
//! ```text
//! Vault (StateMachine, initial = Locked)
//! ├── Locked --Press(1)-->   One
//! ├── One    --Press(2)-->   Two      --Press(other)--> Locked
//! ├── Two    --Press(3)-->   Open     --Press(other)--> Locked
//! └── Open   --Press(any)--> Locked
//! ```
//!
//! A validator is a value stored on the edge (`MessageEdge::<Press>::new(..)`)
//! that sees only the message. An edge whose validator rejects the message is
//! not proposed at all, so the catch-all edge after it in `Transitions` order
//! is what fires for a wrong digit. Validators are for filtering on the
//! message alone; a condition that needs world access is a guard (see
//! `guarded_transitions`).
//!
//! Type **1 2 3** to open the vault; any wrong digit locks it again.
//!
//! ```sh
//! cargo run --example validators
//! # or, to let the gearbox editor connect on 127.0.0.1:15703:
//! cargo run --example validators --features server
//! ```

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

/// A digit key press. `#[gearbox(validator = Digit)]` makes `Digit` the
/// per-edge filter type for this message.
#[derive(Message, Clone, Reflect, GearboxMessage)]
#[gearbox(validator = Digit)]
struct Press {
    #[gearbox(target)]
    machine: Entity,
    digit: u8,
}

/// The validator: `Any` accepts every press, `Only(n)` just that digit.
#[derive(Default, Clone)]
enum Digit {
    #[default]
    Any,
    Only(u8),
}

impl MessageValidator<Press> for Digit {
    fn matches(&self, press: &Press) -> bool {
        match self {
            Digit::Any => true,
            Digit::Only(d) => press.digit == *d,
        }
    }
}

/// Marks the vault door sprite.
#[derive(Component)]
struct Door;
/// Marks the progress label.
#[derive(Component)]
struct Progress;

const LOCKED: Color = Color::srgb(0.6, 0.25, 0.25);
const PARTIAL: Color = Color::srgb(0.7, 0.6, 0.2);
const OPEN: Color = Color::srgb(0.3, 0.8, 0.4);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GearboxPlugin::default())
        .add_plugins(editor_server)
        .add_systems(Startup, setup)
        .add_systems(Update, digits.before(GearboxSet))
        .add_systems(Update, show_progress.after(GearboxSet))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text2d::new("The code is 1 2 3. Type digits; a wrong one locks the vault."),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 200.0, 0.0),
    ));
    commands.spawn((
        Door,
        Sprite::from_color(LOCKED, Vec2::new(160.0, 200.0)),
        Transform::default(),
    ));
    commands.spawn((
        Progress,
        Text2d::new("_ _ _"),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, -150.0, 0.0),
    ));

    // `MessageEdge::<Press>::new(Some(..))` sets the edge's validator. A bare
    // `MessageEdge::<Press>` has none and accepts every `Press`. Order matters:
    // the digit edge comes first, the catch-all is the fallback.
    commands.spawn_scene(bsn! {
        #Vault
            StateMachineId("vault")
            StateMachine InitialState(#Locked)
        Substates [
            #Locked Transitions [
                (Target(#One) MessageEdge::<Press>::new(Some(Digit::Only(1)))),
            ],
            #One Transitions [
                (Target(#Two)    MessageEdge::<Press>::new(Some(Digit::Only(2)))),
                (Target(#Locked) MessageEdge::<Press>),
            ],
            #Two Transitions [
                (Target(#Open)   MessageEdge::<Press>::new(Some(Digit::Only(3)))),
                (Target(#Locked) MessageEdge::<Press>),
            ],
            #Open Transitions [
                (Target(#Locked) MessageEdge::<Press>),
            ],
        ]
    });
}

/// Every digit key writes the same message type; the edges tell them apart.
fn digits(
    keys: Res<ButtonInput<KeyCode>>,
    machine: Single<Entity, With<StateMachine>>,
    mut writer: MessageWriter<Press>,
) {
    const DIGITS: [(KeyCode, u8); 10] = [
        (KeyCode::Digit0, 0), (KeyCode::Digit1, 1), (KeyCode::Digit2, 2), (KeyCode::Digit3, 3),
        (KeyCode::Digit4, 4), (KeyCode::Digit5, 5), (KeyCode::Digit6, 6), (KeyCode::Digit7, 7),
        (KeyCode::Digit8, 8), (KeyCode::Digit9, 9),
    ];
    for (key, digit) in DIGITS {
        if keys.just_pressed(key) {
            writer.write(Press { machine: *machine, digit });
        }
    }
}

/// Paint the door and the progress line from whichever state was entered.
fn show_progress(
    q_entered: Query<&Name, Added<Active>>,
    mut door: Single<&mut Sprite, With<Door>>,
    mut progress: Single<&mut Text2d, With<Progress>>,
) {
    for name in &q_entered {
        let (text, color) = match name.as_str() {
            "Locked" => ("_ _ _", LOCKED),
            "One" => ("1 _ _", PARTIAL),
            "Two" => ("1 2 _", PARTIAL),
            "Open" => ("1 2 3  OPEN", OPEN),
            _ => continue,
        };
        progress.0 = text.into();
        door.color = color;
    }
}

/// With `--features server`, lets the gearbox editor connect at `127.0.0.1:15703`.
fn editor_server(app: &mut App) {
    #[cfg(feature = "server")]
    app.add_plugins(bevy_gearbox::server::ServerPlugin::default());
    #[cfg(not(feature = "server"))]
    let _ = app;
}
