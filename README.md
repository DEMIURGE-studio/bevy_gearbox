<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./assets/logo_light.png">
    <source media="(prefers-color-scheme: light)" srcset="./assets/logo_dark.png">
    <img width="600" src="./assets/logo_dark.png" alt="bevy_gearbox">
  </picture>
</p>

**Gearbox** is a statechart library for the [Bevy](https://bevyengine.org/) game engine.

[![MIT/Apache 2.0](https://img.shields.io/badge/license-MIT%2FApache-blue.svg)](#license)
[![crates.io](https://img.shields.io/crates/v/bevy_gearbox?label=crates.io)](https://crates.io/crates/bevy_gearbox)
[![docs.rs](https://img.shields.io/docsrs/bevy_gearbox?label=docs.rs)](https://docs.rs/bevy_gearbox)

---

## Why gearbox

State machines are useful everywhere in games - AI behavior, ability lifecycles, UI flows, animation controllers. But state machines in an ECS are a hard problem. Gearbox solves this by representing state machines as regular entity hierarchies. States are entities. Transitions are entities. Everything lives in the ECS.

- **Pure ECS.** States and transitions are entities with components. Query for active states with `Added<Active>`, or expose a state on the machine root with a `StateComponent`.
- **Statechart semantics.** Hierarchy, parallel regions, history, guards, delays and terminal states follow the same rules as [XState](https://stately.ai/docs/xstate) and SCXML.
- **Parallel by construction.** Resolution runs in a dedicated schedule, so your guards and side effects are ordinary Bevy systems and run in parallel like any other.
- **Message-driven.** Trigger transitions by writing Bevy messages. Read the matched payload in a side-effect phase to apply damage, spend a resource, or fire the next message.
- **Data-driven.** Author a whole chart as one `bsn!` scene. Save and load it as a Bevy scene, or edit it while the game runs.
- **Visual editor.** Build, edit, and monitor state machines in a running game over the Bevy Remote Protocol.

<p align="center">
  <img width="600" src="assets/editor_demo.webp">
</p>

Read the [guide](DOCS.md) for a tutorial on how to build a statechart start to finish.

## Getting started

```rust
use bevy::prelude::*;
use bevy_gearbox::GearboxPlugin;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, GearboxPlugin::default()))
        // Optional plugin for connecting the editor to your game
        .add_plugins(editor_server)
        .run();
}
```

## Building a state machine

```rust
use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;

fn spawn_machine(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        StateMachine InitialState(#Ready)
        Substates [
            #Ready Transitions [
                (Target(#Active) MessageEdge::<Activate>)
            ],
            #Active Transitions [
                (Target(#Ready) AlwaysEdge Delay::from_secs_f32(0.5))
            ],
        ]
    });
}
```

States nest under `Substates [ .. ]`, edges under `Transitions [ .. ]`, and
`#Name` references resolve to sibling states in the scene. A parent with an
`InitialState` is sequential; a parent without one is a parallel region.

### Triggering transitions

Define a message with `#[derive(GearboxMessage)]`, marking the entity it's
addressed to with `#[gearbox(target)]` (the message listener walks `SubstateOf`
from there to find the machine root):

```rust
use bevy::prelude::*;
use bevy_gearbox::prelude::*;

#[derive(Message, Clone, Reflect, GearboxMessage)]
struct Activate {
    #[gearbox(target)]
    machine: Entity,
}

// Write it from any system.
fn input_system(mut writer: MessageWriter<Activate>, machine: Single<Entity, With<StateMachine>>) {
    writer.write(Activate { machine: *machine });
}
```

The derive registers the message type through `inventory`, and `GearboxPlugin`
installs every derived message's listener on build, so a derived message needs
no extra wiring. Generic message types can't be auto-registered; call
`app.register_transition::<M>()` for those.

### Guarded transitions

A `Transitions` with multiple of the same edge acts as a branching transition. The statechart will attempt each edge in order. This means that you should carefully order your transitions. In the following example, note that the more strict transition (alive -> dead) is before the less strict transition (alive -> hurt).

```rust
#[derive(Component, Default, Clone)]
struct HpIsZero;

#Alive Transitions [
    (Target(#Dead) MessageEdge::<Attacked> HpIsZero),  // only if the guard passes
    (Target(#Hurt) MessageEdge::<Attacked>),           // otherwise
]

// A guard is a system in BlockerPhase that vetoes candidates carrying the marker.
fn hp_is_zero(mut candidates: MessageMutator<TransitionMessage>, q_guard: Query<(), With<HpIsZero>>, q_hp: Query<&Hitpoints>) {
    for c in candidates.read() {
        if c.edge.is_some_and(|e| q_guard.contains(e)) && q_hp.get(c.machine).is_ok_and(|hp| hp.current > 0.0) {
            c.blocked = true;
        }
    }
}
```

### State components

State components are inserted into the statechart root when the state that has the state component is entered. The component is removed from the root when the state is exited. You can use this to selectively expose states to the ECS.

```rust
use bevy_gearbox::prelude::*;

#[state_component]
#[derive(Component, Clone, Default)]
struct Walking;

// On the state, in the scene:
#Walking StateComponent::<Walking>

// While that state is active the root carries `Walking`, so
// `Query<&mut Velocity, With<Walking>>` finds walking characters.
fn while_walking(Query<&mut Velocity, With<Walking>>) { ... }
```

### Reacting to state changes

Attach entry actions in the scene, or query `Active` from systems:

```rust
// In the scene: an EnterState observer on the state entity.
#Invoking on(|_: On<EnterState>, mut commands: Commands| { /* launch a projectile */ })

// From a system, ordered after GearboxSet:
fn on_enter(q_entered: Query<(Entity, &Active), Added<Active>>) {
    for (state, active) in &q_entered {
        // `state` was just entered; `active.machine` is the machine root.
    }
}
```

## Features

- Hierarchical states (nested state machines / statecharts)
- Parallel regions
- Shallow and deep history
- Message-driven transitions with per-edge validators
- Guarded transitions: ordered candidates, first passing guard wins, guardless fallback
- Always-edges (automatic transitions on entry) and delayed edges (timer-based), both guardable
- Terminal states that emit `Done` to their parent
- Side effects with payloads via `Matched<M>`, skipped for vetoed transitions
- State components (auto insert/remove on the machine root)
- Entry/exit observers (`EnterState` / `ExitState`) and `Added<Active>` queries
- Reset edges (clear history under a subtree on transition)
- Internal vs external transitions
- Bridge to Bevy `States` (`#[state_bridge]`)
- Optional [bevy_gauge](https://crates.io/crates/bevy_gauge) integration: `Delay` driven by an attribute (`gauge` feature)
- Optional editor server (`server` feature, off by default) for the visual editor

## Examples

- [`examples/invoked_loop.rs`](examples/invoked_loop.rs) - a fire-and-cooldown ability with entry actions.
- [`examples/parallel_regions.rs`](examples/parallel_regions.rs) - posture and weapon regions driven by keyboard.
- [`examples/guarded_transitions.rs`](examples/guarded_transitions.rs) - lethal / heavy / otherwise hits through guards and a `Matched<M>` side effect.

Run any of them with `--features server` and the editor can connect to it.

## Scenes and serialization

Author with `bsn!`. Bevy 0.19 has no `.bsn` asset format yet; a chart the editor
saves is written as a Bevy `DynamicScene` (`.scn.ron`) and reloaded through
`bevy_world_serialization`. `Substates` and `Transitions` are rebuilt from each
child's `SubstateOf` / `Source` on load, in file order, and that order is the
priority order for guarded edges.

## Version Table

| Bevy | Gearbox |
| ---- | ------- |
| 0.19 | 0.8     |
| 0.19 | 0.7     |
| 0.18 | 0.6     |
| 0.18 | 0.5     |
| 0.17 | 0.4     |

## Contributing

Feel free to open issues or create pull requests if you encounter any problems.

Ask us on the [Bevy Discord](https://discord.com/invite/bevy) server's [Gearbox topic](https://discord.com/channels/691052431525675048/1379511828949762048) in `#ecosystem-crates` for larger changes or other things if you feel like so!

## License

Dual-licensed under MIT ([LICENSE-MIT](LICENSE-MIT)) or Apache 2.0 ([LICENSE-APACHE](LICENSE-APACHE)).
