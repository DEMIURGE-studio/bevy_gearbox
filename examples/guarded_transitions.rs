//! Guarded transitions: several edges for one message, tried in order, first
//! passing guard wins. Authored as a `bsn!` scene and run as a real Bevy app.
//!
//! ```text
//! Character (StateMachine, initial = Alive)
//! ├── Alive      --Hit--> Dead       if the hit is lethal      (guard: Lethal)
//! │              --Hit--> Staggered  if the hit is heavy       (guard: Heavy)
//! │              --Hit--> Hurt       otherwise                 (no guard)
//! ├── Hurt       --always, after 0.4s--> Alive
//! ├── Staggered  --always, after 1.0s--> Alive
//! └── Dead       --Revive--> Alive
//! ```
//!
//! There is no "branch" node. `Alive` simply has three `MessageEdge::<Hit>`
//! edges in its `Transitions [ .. ]` list. Every one of them is proposed when a
//! `Hit` arrives; the guard systems veto the ones whose condition fails; the
//! first survivor in list order is taken. The guardless `Hurt` edge is last,
//! so it is the fallback.
//!
//! A guard is a marker component on the edge (`Lethal`, `Heavy`) plus a system
//! in `GearboxPhase::BlockerPhase`. Guards here read the hit's damage from the
//! `Matched<Hit>` payload and the character's `Hitpoints`, the same
//! `(context, event)` pair an XState guard sees.
//!
//! Press **H** for a light hit (10), **J** for a heavy hit (35), **R** to
//! revive. Close the window to quit.
//!
//! ```sh
//! cargo run --example guarded_transitions
//! ```

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;
use bevy_gearbox::server::{ServerPlugin, StateMachineId};
use bevy_gearbox::{GearboxPlugin, Matched};

/// A hit on the character. `damage` is read by the guards and applied by the
/// side-effect system.
#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Hit {
    #[gearbox(target)]
    machine: Entity,
    damage: f32,
}

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Revive {
    #[gearbox(target)]
    machine: Entity,
}

/// Guard marker: the edge is taken only if the hit would bring hitpoints to zero.
#[derive(Component, Default, Clone)]
struct Lethal;

/// Guard marker: the edge is taken only if the hit deals at least `HEAVY` damage.
#[derive(Component, Default, Clone)]
struct Heavy;

const HEAVY: f32 = 30.0;

#[derive(Component, Default, Clone)]
struct Hitpoints {
    current: f32,
    max: f32,
}

/// Marks the on-screen status label.
#[derive(Component)]
struct StatusText;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GearboxPlugin::default())
        .add_plugins(ServerPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, input.before(GearboxSet))
        .add_systems(Update, update_label.after(GearboxSet))
        // Guards veto candidates; the damage side effect applies to the winner.
        .add_systems(GearboxSchedule, hit_guards.in_set(GearboxPhase::BlockerPhase))
        .add_systems(GearboxSchedule, apply_damage.in_set(GearboxPhase::SideEffectPhase))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text2d::new("<H> light hit (10)   <J> heavy hit (35)   <R> revive"),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 200.0, 0.0),
    ));
    commands.spawn((
        StatusText,
        Text2d::new(""),
        TextColor(Color::srgb(0.6, 0.9, 0.6)),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    commands.spawn_scene(bsn! {
        #Character
            template(|_| Ok(StateMachineId::new("character")))
            Hitpoints { current: 100.0, max: 100.0 }
            StateMachine InitialState(#Alive)
        Substates [
            // Three candidates for the same message, in priority order.
            // The guardless edge is last: it is the fallback.
            #Alive Transitions [
                (Target(#Dead)      MessageEdge::<Hit> Lethal),
                (Target(#Staggered) MessageEdge::<Hit> Heavy),
                (Target(#Hurt)      MessageEdge::<Hit>),
            ],
            #Hurt Transitions [
                (Target(#Alive) AlwaysEdge Delay::from_secs_f32(0.4))
            ],
            #Staggered Transitions [
                (Target(#Alive) AlwaysEdge Delay::from_secs_f32(1.0))
            ],
            #Dead Transitions [
                (Target(#Alive) MessageEdge::<Revive>)
            ],
        ]
    });
}

/// The guards. Every `Hit` candidate arrives here with its `Matched<Hit>`
/// payload already written, so the damage of the hit and the character's
/// current hitpoints are both available. Anything vetoed falls through to the
/// next edge in `Transitions` order.
fn hit_guards(
    mut candidates: MessageMutator<TransitionMessage>,
    mut matched: MessageReader<Matched<Hit>>,
    q_lethal: Query<(), With<Lethal>>,
    q_heavy: Query<(), With<Heavy>>,
    q_hp: Query<&Hitpoints>,
) {
    let damage_by_edge: HashMap<Entity, f32> =
        matched.read().map(|m| (m.edge, m.message.damage)).collect();

    for c in candidates.read() {
        let Some(edge) = c.edge else { continue };
        let Some(&damage) = damage_by_edge.get(&edge) else { continue };
        let Ok(hp) = q_hp.get(c.machine) else { continue };

        if q_lethal.contains(edge) && hp.current > damage {
            c.blocked = true;
        }
        if q_heavy.contains(edge) && damage < HEAVY {
            c.blocked = true;
        }
    }
}

/// Apply the hit's damage for the winning candidate only. Losers and vetoed
/// edges are in `BlockedEdges`.
fn apply_damage(
    mut matched: MessageReader<Matched<Hit>>,
    blocked: Res<BlockedEdges>,
    mut q_hp: Query<&mut Hitpoints>,
) {
    for m in matched.read() {
        if blocked.is_blocked(m.edge) {
            continue;
        }
        if let Ok(mut hp) = q_hp.get_mut(m.machine) {
            hp.current = (hp.current - m.message.damage).max(0.0);
        }
    }
}

fn input(
    keys: Res<ButtonInput<KeyCode>>,
    machine: Single<Entity, With<StateMachine>>,
    mut hits: MessageWriter<Hit>,
    mut revives: MessageWriter<Revive>,
    mut q_hp: Query<&mut Hitpoints>,
) {
    let m = *machine;
    if keys.just_pressed(KeyCode::KeyH) {
        hits.write(Hit { machine: m, damage: 10.0 });
    }
    if keys.just_pressed(KeyCode::KeyJ) {
        hits.write(Hit { machine: m, damage: 35.0 });
    }
    if keys.just_pressed(KeyCode::KeyR) {
        if let Ok(mut hp) = q_hp.get_mut(m) {
            hp.current = hp.max;
        }
        revives.write(Revive { machine: m });
    }
}

/// Show the active state and hitpoints (keyed off the entered state's `Name`).
fn update_label(
    q_entered: Query<&Name, Added<Active>>,
    q_hp: Query<&Hitpoints, With<StateMachine>>,
    mut q_text: Query<&mut Text2d, With<StatusText>>,
    mut current: Local<String>,
) {
    for name in &q_entered {
        match name.as_str() {
            "Alive" | "Hurt" | "Staggered" | "Dead" => *current = name.to_string(),
            _ => {}
        }
    }
    let (Ok(hp), Ok(mut text)) = (q_hp.single(), q_text.single_mut()) else {
        return;
    };
    text.0 = format!("{}   HP {:.0}/{:.0}", *current, hp.current, hp.max);
}
