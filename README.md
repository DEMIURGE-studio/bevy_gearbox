<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./assets/logo_light.png">
    <source media="(prefers-color-scheme: light)" srcset="./assets/logo_dark.png">
    <img width="600" src="https://user-images.githubusercontent.com/25423296/163456779-a8556205-d0a5-45e2-ac17-42d089e3c3f8.png">
  </picture>
</p>

**Gearbox** is a state machine/chart library for the [Bevy](https://bevyengine.org/) game engine.

[![MIT/Apache 2.0](https://img.shields.io/badge/license-MIT%2FApache-blue.svg)](#license)
[![crates.io](https://img.shields.io/crates/v/bevy_gearbox?label=crates.io)](https://crates.io/crates/bevy_gearbox)
[![docs.rs](https://img.shields.io/docsrs/bevy_gearbox?label=docs.rs)](https://docs.rs/bevy_gearbox)

---

## Why gearbox

State machines are useful everywhere in games - AI behavior, ability lifecycles, UI flows, animation controllers. But state machines in an ECS are a hard problem. Gearbox solves this by representing state machines as regular entity hierarchies. States are entities. Transitions are entities. Everything lives in the ECS and plays by its rules.

- **Pure ECS.** States and transitions are entities with components. Query for active states with `With<MyComponent>`. No new paradigms.
- **Fully parallelizable.** All transition resolution runs through a parallelized schedule. Thousands of machines per frame.
- **Message-driven.** Trigger transitions by writing Bevy messages. Attach side effects that automatically produce downstream messages when transitions fire.
- **Data-driven.** State machines are entity hierarchies. Spawn them from scenes, build them from assets, edit them at runtime.
- **Visual Editor** (optional). Build, edit, and monitor state machines while your game runs.

<p align="center">
  <img width="600" src="assets/editor_demo.webp">
</p>

## Getting started

```rust
use bevy::prelude::*;
use bevy_gearbox::GearboxPlugin;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, GearboxPlugin))
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
                (Target(#Active) MessageEdge::<Activate>::default())
            ],
            #Active Transitions [
                (Target(#Ready) AlwaysEdge)
            ],
        ]
    });
}
```

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

The derive also auto-registers the message type through `inventory`. Add
`gearbox_auto_register_plugin` to install every derived message's listener, or
call `app.register_transition::<Activate>()` explicitly.

To filter which messages match an edge, supply a custom validator (the default
is `AcceptAll`):

```rust
#[derive(Message, Clone, Reflect, GearboxMessage)]
#[gearbox(validator = MyValidator)]
struct Fire {
    #[gearbox(target)]
    machine: Entity,
}
```

### State components

Automatically insert/remove a component on the machine root based on which
state is active. `StateComponent` isn't `Default`, so insert it through a
`template` closure:

```rust
#Walking template(|_| Ok(StateComponent(Walking)))
// The `Walking` component appears on the machine root while this state is active.
```

### Reacting to state changes

```rust
fn on_enter(q_entered: Query<(Entity, &Active), Added<Active>>) {
    for (state, active) in &q_entered {
        // `state` was just entered, `active.machine` is the state machine root
    }
}

fn on_exit(mut removed: RemovedComponents<Active>) {
    for state in removed.read() {
        // `state` was just exited
    }
}
```

## Features

- Hierarchical states (nested state machines / statecharts)
- Parallel regions
- Shallow and deep history
- Guarded transitions with string-based guard sets
- Delayed transitions (timer-based)
- Always-edges (automatic transitions when conditions are met)
- Parameter-driven guards (float/int/bool ranges with hysteresis)
- Side effects (message-in, message-out on transition)
- State components (auto insert/remove on enter/exit)
- Reset edges (clear subtree state on transition)
- Internal vs external transitions

## Migrating from the builder API

The imperative builder traits (`spawn_substate`, `spawn_transition`,
`init_state_machine`, `build_transition_always`, `spawn_branch`, …) are
**deprecated** and will be removed in the next major release. The
`#[gearbox_message]` / `#[transition_message]` attribute macros have been
**removed** - define messages with `#[derive(GearboxMessage)]`. Author machines
as `bsn!` scenes instead.

This is a restructuring, not a one-to-one swap. The complicated builder 
collapses into a single `bsn` block. 

```rust
// Before - imperative builders:
let ready  = commands.spawn_substate(machine, Name::new("Ready")).id();
let active = commands.spawn_substate(machine, Name::new("Active")).id();
commands.spawn_transition::<Activate>(ready, active);
commands.spawn((Source(active), Target(ready), AlwaysEdge));
commands.entity(machine).init_state_machine(ready);

// After - one bsn! scene:
commands.spawn_scene(bsn! {
    StateMachine InitialState(#Ready)
    Substates [
        #Ready  Transitions [ (Target(#Active) MessageEdge::<Activate>::default()) ],
        #Active Transitions [ (Target(#Ready) AlwaysEdge) ],
    ]
});
```

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

Dual-licensed under MIT ([LICENSE-MIT](/LICENSE-MIT)) or Apache 2.0 ([LICENSE-APACHE](/LICENSE-APACHE)).
