//! How do I drive Bevy `States` from a chart?
//!
//! Put the Bevy state value on the chart's state entity and mark the enum
//! `#[state_bridge]`. Whenever a state carrying a `Screen` value is entered,
//! gearbox sets `NextState<Screen>`, so `OnEnter`, `OnExit`, `in_state` and
//! `DespawnOnExit` all work as usual. The chart owns the transitions; the
//! rest of the app never writes `NextState` itself.
//!
//! ```text
//! Flow (StateMachine, initial = Menu)
//! ├── Menu     Screen::Menu     --Start (Space)-->          Loading
//! ├── Loading  Screen::Loading  --always, after 1.0s-->     Playing
//! └── Playing  Screen::Playing  --Quit (Esc)-->             Menu
//! ```
//!
//! ```sh
//! cargo run --example state_bridge
//! # or, to let the gearbox editor connect on 127.0.0.1:15703:
//! cargo run --example state_bridge --features server
//! ```

use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::GearboxPlugin;

/// The Bevy state. It is also a component, placed on the chart's states.
#[state_bridge]
#[derive(States, Component, Clone, Copy, Default, Debug, Hash, PartialEq, Eq, Reflect, FromTemplate)]
enum Screen {
    #[default]
    Menu,
    Loading,
    Playing,
}

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Start {
    #[gearbox(target)]
    machine: Entity,
}
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Quit {
    #[gearbox(target)]
    machine: Entity,
}

/// Marks the spinning square on the Playing screen.
#[derive(Component)]
struct Spinner;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GearboxPlugin::default())
        .add_plugins(editor_server)
        .init_state::<Screen>()
        .add_systems(Startup, setup)
        .add_systems(Update, input.before(GearboxSet))
        // Plain Bevy state hooks from here on.
        .add_systems(OnEnter(Screen::Menu), menu_screen)
        .add_systems(OnEnter(Screen::Loading), loading_screen)
        .add_systems(OnEnter(Screen::Playing), playing_screen)
        .add_systems(Update, spin.run_if(in_state(Screen::Playing)))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn_scene(bsn! {
        #Flow
            StateMachineId("flow")
            StateMachine InitialState(#Menu)
        Substates [
            #Menu    Screen::Menu    Transitions [ (Target(#Loading) MessageEdge::<Start>) ],
            #Loading Screen::Loading Transitions [ (Target(#Playing) AlwaysEdge Delay::from_secs_f32(1.0)) ],
            #Playing Screen::Playing Transitions [ (Target(#Menu) MessageEdge::<Quit>) ],
        ]
    });
}

fn input(
    keys: Res<ButtonInput<KeyCode>>,
    machine: Single<Entity, With<StateMachine>>,
    mut start: MessageWriter<Start>,
    mut quit: MessageWriter<Quit>,
) {
    if keys.just_pressed(KeyCode::Space) {
        start.write(Start { machine: *machine });
    }
    if keys.just_pressed(KeyCode::Escape) {
        quit.write(Quit { machine: *machine });
    }
}

/// Each screen's entities are scoped to its state with `DespawnOnExit`.
fn menu_screen(mut commands: Commands) {
    commands.spawn((
        DespawnOnExit(Screen::Menu),
        Text2d::new("MENU\n\nPress <Space> to start"),
        TextColor(Color::WHITE),
    ));
}

fn loading_screen(mut commands: Commands) {
    commands.spawn((
        DespawnOnExit(Screen::Loading),
        Text2d::new("Loading..."),
        TextColor(Color::srgb(0.9, 0.85, 0.5)),
    ));
}

fn playing_screen(mut commands: Commands) {
    commands.spawn((
        DespawnOnExit(Screen::Playing),
        Text2d::new("PLAYING   <Esc> back to the menu"),
        TextColor(Color::srgb(0.6, 0.9, 0.6)),
        Transform::from_xyz(0.0, 150.0, 0.0),
    ));
    commands.spawn((
        DespawnOnExit(Screen::Playing),
        Spinner,
        Sprite::from_color(Color::srgb(0.4, 0.8, 1.0), Vec2::splat(80.0)),
    ));
}

fn spin(time: Res<Time>, mut spinner: Single<&mut Transform, With<Spinner>>) {
    spinner.rotate_z(2.0 * time.delta_secs());
}

/// With `--features server`, lets the gearbox editor connect at `127.0.0.1:15703`.
fn editor_server(app: &mut App) {
    #[cfg(feature = "server")]
    app.add_plugins(bevy_gearbox::server::ServerPlugin::default());
    #[cfg(not(feature = "server"))]
    let _ = app;
}
