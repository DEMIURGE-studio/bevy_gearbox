//! Stress test: 2,000 state machines, each transitioning multiple times per frame.
//! Each transition triggers a stats update via observer — simulating real game work.
//!
//! Run with tracy:
//!   cargo run -p bevy_gearbox --example observer_stress --release --features trace_tracy

use bevy::prelude::*;
use bevy_gearbox::{prelude::*, GearboxPlugin};

const NUM_MACHINES: usize = 2_000;
const TRANSITIONS_PER_FRAME: usize = 3;

#[derive(Component)]
struct PingPong {
    a: Entity,
    b: Entity,
    edge_a_to_b: Entity,
    edge_b_to_a: Entity,
}

/// Simulated game state derived from the active state.
#[derive(Component, Default)]
struct Stats {
    speed: f32,
    armor: f32,
    damage: f32,
    transition_count: u64,
}

fn setup(mut commands: Commands) {
    for _ in 0..NUM_MACHINES {
        let machine = commands.spawn_empty().id();
        let a = commands.spawn(SubstateOf(machine)).id();
        let b = commands.spawn(SubstateOf(machine)).id();

        let edge_ab = commands.spawn((Source(a), Target(b))).id();
        let edge_ba = commands.spawn((Source(b), Target(a))).id();

        commands.entity(machine).insert((
            StateMachine::new(),
            InitialState(a),
            PingPong {
                a,
                b,
                edge_a_to_b: edge_ab,
                edge_b_to_a: edge_ba,
            },
            Stats::default(),
        ));
    }
}

/// Every frame, trigger multiple transitions per machine.
fn trigger_transitions(
    q_machines: Query<(Entity, &StateMachine, &PingPong)>,
    mut commands: Commands,
) {
    for (machine_entity, sm, pp) in &q_machines {
        let mut in_a = sm.is_active(&pp.a);
        for _ in 0..TRANSITIONS_PER_FRAME {
            if in_a {
                commands.trigger(Transition {
                    machine: machine_entity,
                    source: pp.a,
                    edge: pp.edge_a_to_b,
                    payload: (),
                });
            } else {
                commands.trigger(Transition {
                    machine: machine_entity,
                    source: pp.b,
                    edge: pp.edge_b_to_a,
                    payload: (),
                });
            }
            in_a = !in_a;
        }
    }
}

/// Observer that updates stats when any state is entered.
fn on_enter_update_stats(
    enter: On<EnterState>,
    mut q_stats: Query<(&StateMachine, &PingPong, &mut Stats)>,
) {
    let machine_entity = enter.event().state_machine;
    let Ok((sm, pp, mut stats)) = q_stats.get_mut(machine_entity) else {
        return;
    };

    stats.transition_count += 1;
    if sm.is_active(&pp.a) {
        stats.speed = 10.0 + (stats.transition_count as f32 * 0.1).sin();
        stats.armor = 2.0;
        stats.damage = 5.0 + (stats.transition_count as f32 * 0.3).cos();
    } else {
        stats.speed = 3.0;
        stats.armor = 15.0 + (stats.transition_count as f32 * 0.2).sin();
        stats.damage = 12.0 + (stats.transition_count as f32 * 0.15).cos();
    }
}

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, GearboxPlugin))
        .add_observer(on_enter_update_stats)
        .add_systems(Startup, setup)
        .add_systems(Update, trigger_transitions)
        .run();
}
