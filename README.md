# bevy_gearbox

Statecharts for Bevy, designed for ECS-first workflows and data-driven tools. Pairs well with the visual editor [`bevy_gearbox_editor`](https://github.com/DEMIURGE-studio/bevy_gearbox_editor).

## Overview

bevy_gearbox provides a state machine runtime modeled after XState’s core ideas (LCA-based transitions, entry/exit/transition phases, eventless transitions), adapted to Bevy’s ECS. It’s not a perfect logical match to XState; it prioritizes Bevy ergonomics, serialization, and editor integration.

## Why use bevy_gearbox

- ECS-first design: states, transitions, and hierarchy are plain components and entities.
- Deterministic state changes: exit → transition actions → entry; LCA-based pathing.
- Data-driven: author machines in scenes; integrate with tools out of the box.
- Editor ecosystem: pairs with [`bevy_gearbox_editor`](https://github.com/DEMIURGE-studio/bevy_gearbox_editor) for rapid iteration and live statechart inspection.

## Core concepts

- Logical hierarchy (independent of scene graph): `StateChildOf` / `StateChildren`.
- Activation semantics:
  - Root remains active for the machine lifetime.
  - Entering a state enters all ancestors down to the leaf.
  - Self-transitions are external by default; use `EdgeKind::Internal` to suppress re-entry.
- Transition entities:
  - `Source(Entity)`, `Target(Entity)`, `EdgeKind`.
  - Event-driven transitions via `EventEdge<E>` components on edges.
  - Eventless transitions: `AlwaysEdge`.
  - Delayed transitions: `After { duration }` with cancellation on exit.
- Guards:
  - ECS-managed guard failure sets with per-edge checks.
  - Composition support planned via a small `Logic` tree.
- Actions envelope:
  - `TransitionActions` emitted between exits and entries.
  - Author “assign/raise/send/cancel” patterns as normal systems.

## Quick start

1) Add the plugin and your event observers

```rust
app.add_plugins(GearboxPlugin)
   .add_observer(edge_event_listener::<YourEvent>);
```

2) Send events to the machine root; the listener routes to active leaves internally

```rust
commands.trigger_targets(YourEvent, machine_root);
```

3) Author via code or scenes

- Spawn states and edges with `Source`, `Target`, `EdgeKind`, `EventEdge<E>`, `AlwaysEdge`, `After`.
- Or load a scene authored with [`bevy_gearbox_editor`](https://github.com/DEMIURGE-studio/bevy_gearbox_editor).

## Compatibility

| Crate               | Version | Bevy |
|---------------------|---------|------|
| bevy_gearbox        | 0.3.4   | 0.16 |
| bevy_gearbox_editor | 0.3.4   | 0.16 |

## License

Dual-licensed under MIT or Apache-2.0, at your option.
