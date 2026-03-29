//! Stress test: 2,000 state machines, each transitioning multiple times per frame.
//! Each transition triggers a bulk stats update — simulating real game work.
//!
//! Run with tracy:
//!   cargo run -p bevy_gearbox_schedule --example schedule_stress --release --features trace_tracy

use bevy::prelude::*;
use bevy_gearbox_schedule::*;

const NUM_MACHINES: usize = 2_000;
const TRANSITIONS_PER_FRAME: usize = 3;

#[derive(Component)]
struct PingPong {
    a: Entity,
    b: Entity,
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

        // A -> B and B -> A, both AlwaysEdge with a guard.
        // We clear one guard per frame to trigger that direction.
        let mut guards_ab = Guards::default();
        guards_ab.add("blocked");
        commands.spawn((Source(a), Target(b), AlwaysEdge, guards_ab));

        let mut guards_ba = Guards::default();
        guards_ba.add("blocked");
        commands.spawn((Source(b), Target(a), AlwaysEdge, guards_ba));

        commands.entity(machine).insert((
            Machine::new(),
            InitialState(a),
            PingPong { a, b },
            Stats::default(),
        ));
    }
}

/// Every frame, write multiple transition messages per machine.
/// Odd transitions go A->B, even go B->A, so we ping-pong rapidly.
fn trigger_transitions(
    q_machines: Query<(Entity, &Machine, &PingPong)>,
    mut writer: MessageWriter<TransitionMessage>,
) {
    for (machine_entity, machine, pp) in &q_machines {
        let mut in_a = machine.is_active(&pp.a);
        for _ in 0..TRANSITIONS_PER_FRAME {
            if in_a {
                writer.write(TransitionMessage {
                    machine: machine_entity,
                    source: pp.a,
                    target: pp.b,
                    edge: None,
                });
            } else {
                writer.write(TransitionMessage {
                    machine: machine_entity,
                    source: pp.b,
                    target: pp.a,
                    edge: None,
                });
            }
            in_a = !in_a;
        }
    }
}

/// Runs in EntryPhase inside the GearboxSchedule — bulk updates stats
/// for all machines based on their new active state.
fn update_stats(mut q_machines: Query<(&Machine, &PingPong, &mut Stats)>) {
    for (machine, pp, mut stats) in &mut q_machines {
        stats.transition_count += 1;
        if machine.is_active(&pp.a) {
            stats.speed = 10.0 + (stats.transition_count as f32 * 0.1).sin();
            stats.armor = 2.0;
            stats.damage = 5.0 + (stats.transition_count as f32 * 0.3).cos();
        } else {
            stats.speed = 3.0;
            stats.armor = 15.0 + (stats.transition_count as f32 * 0.2).sin();
            stats.damage = 12.0 + (stats.transition_count as f32 * 0.15).cos();
        }
    }
}

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, GearboxSchedulePlugin))
        .add_systems(Startup, setup)
        .add_systems(Update, trigger_transitions.before(GearboxSet))
        .add_systems(
            GearboxSchedule,
            update_stats.in_set(GearboxPhase::EntryPhase),
        )
        .run();
}
