//! Benchmark: observer-based state machine resolution (bevy_gearbox).
//!
//! Equivalent scenario to schedule_bench: N state machines, each with
//! states A -> B -> C via AlwaysEdge. We measure steady-state resolution.

use bevy::prelude::*;
use bevy_gearbox::{GearboxPlugin, prelude::*};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

/// Spawns `n` state machines using the observer-based bevy_gearbox.
/// Returns (app, vec of (machine_root, a_state, edge_c_to_a) tuples).
fn setup_app(n: usize) -> (App, Vec<(Entity, Entity, Entity)>) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GearboxPlugin));

    let mut machines = Vec::with_capacity(n);

    let world = app.world_mut();
    for _ in 0..n {
        let machine = world.spawn_empty().id();
        let a = world.spawn(SubstateOf(machine)).id();
        let b = world.spawn(SubstateOf(machine)).id();
        let c = world.spawn(SubstateOf(machine)).id();

        // A -> B (AlwaysEdge)
        world.spawn((Source(a), Target(b), AlwaysEdge));
        // B -> C (AlwaysEdge)
        world.spawn((Source(b), Target(c), AlwaysEdge));
        // C -> A edge (not AlwaysEdge — we trigger this manually)
        let edge_c_to_a = world.spawn((Source(c), Target(a))).id();

        world
            .entity_mut(machine)
            .insert((StateMachine::new(), InitialState(a)));

        machines.push((machine, a, edge_c_to_a));
    }

    // Run one frame to initialize all machines — they should chain A -> B -> C
    app.update();

    // Verify
    for &(machine, _, _) in &machines {
        let sm = app.world().get::<StateMachine>(machine).unwrap();
        assert!(
            !sm.active_leaves.is_empty(),
            "Machine should have active leaves after init"
        );
    }

    (app, machines)
}

fn bench_resolve(c: &mut Criterion) {
    let mut group = c.benchmark_group("observer_resolve");

    for n in [10, 100, 1_000, 100_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let (mut app, machines) = setup_app(n);

            b.iter(|| {
                // Trigger C -> A on every machine, forcing re-resolution through A -> B -> C
                {
                    let mut commands = app.world_mut().commands();
                    for &(machine, _a, edge_c_to_a) in &machines {
                        // Get the current leaf (should be C) to use as source
                        commands.trigger(Transition {
                            machine,
                            source: machine, // source = machine root triggers from wherever active
                            edge: edge_c_to_a,
                            payload: (),
                        });
                    }
                }

                app.update();
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_resolve);
criterion_main!(benches);
