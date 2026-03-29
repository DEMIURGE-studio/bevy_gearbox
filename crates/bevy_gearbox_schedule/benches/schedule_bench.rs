//! Benchmark: schedule-based state machine resolution.
//!
//! Scenario: N state machines, each with states A -> B -> C via AlwaysEdge.
//! We measure steady-state: machines are already initialized into C,
//! then we trigger A on every machine and measure the time to resolve
//! through A -> B -> C for all machines in a single frame.

use bevy::prelude::*;
use bevy_gearbox_schedule::*;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

/// Spawns `n` state machines, each with A -[AlwaysEdge]-> B -[AlwaysEdge]-> C.
/// Returns (app, vec of (machine, a, b, c) tuples).
fn setup_app(n: usize) -> (App, Vec<(Entity, Entity, Entity, Entity)>) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxSchedulePlugin));

    let mut machines = Vec::with_capacity(n);

    let world = app.world_mut();
    for _ in 0..n {
        let machine = world.spawn_empty().id();
        let a = world.spawn(SubstateOf(machine)).id();
        let b = world.spawn(SubstateOf(machine)).id();
        let c = world.spawn(SubstateOf(machine)).id();

        world.spawn((Source(a), Target(b), AlwaysEdge));
        world.spawn((Source(b), Target(c), AlwaysEdge));

        world
            .entity_mut(machine)
            .insert((Machine::new(), InitialState(a)));

        machines.push((machine, a, b, c));
    }

    // Run one frame to initialize all machines into C
    app.update();

    // Verify all machines settled into C
    for &(machine, _, _, c) in &machines {
        let state = app.world().get::<Machine>(machine).unwrap();
        assert!(
            state.is_active(&c),
            "Machine should be in state C after init"
        );
    }

    (app, machines)
}

fn bench_resolve(c: &mut Criterion) {
    let mut group = c.benchmark_group("schedule_resolve");

    for n in [10, 100, 1_000, 100_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let (mut app, machines) = setup_app(n);

            b.iter(|| {
                // Trigger all machines: write a message to go from current leaf -> A
                // (forcing re-resolution through A -> B -> C)
                {
                    let world = app.world_mut();
                    for &(machine, a, _, c) in &machines {
                        world.write_message(TransitionMessage {
                            machine,
                            source: c,
                            target: a,
                            edge: None,
                        });
                    }
                }

                // Resolve everything in one frame
                app.update();
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_resolve);
criterion_main!(benches);
