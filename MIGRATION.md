# Migrating to Gearbox 0.6

Gearbox 0.6 replaces the observer-based resolution engine with a schedule-based one. State machines now resolve via a parallelized schedule, which scales significantly better. The public API has changed to reflect this.

## Transition events → Messages

Events that trigger state machine transitions are now Bevy **messages** instead of entity events.

```rust
// Before
#[derive(SimpleTransition, EntityEvent, Clone)]
struct Attack {
    #[event_target]
    target: Entity,
}

// After (derive — recommended)
#[gearbox_message]
struct Attack {
    machine: Entity,
    damage: f32,
}

// After (manual — use when you need a custom validator)
#[derive(Message, Clone)]
struct Attack {
    machine: Entity,
    damage: f32,
}

impl GearboxMessage for Attack {
    type Validator = DamageThresholdValidator;
    fn machine(&self) -> Entity { self.machine }
}

// Manual impls require explicit registration
app.register_transition::<Attack>();
```

The `SimpleTransition` derive is replaced by the `#[gearbox_message]` attribute macro. The struct must have a `machine: Entity` field. The macro adds `#[derive(Message, Clone)]`, implements `GearboxMessage` with `AcceptAll`, and auto-registers the transition via inventory.

For custom validators, implement `GearboxMessage` manually and use `#[transition_message]` on the struct to get auto-registration.

## Transition edges

```rust
// Before
commands.spawn((Source(a), Target(b), EventEdge::<Attack>::default()));

// After
commands.spawn((Source(a), Target(b), MessageEdge::<Attack>::default()));

// Or use the helper
commands.spawn_transition::<Attack>(a, b);
```

## Triggering transitions

```rust
// Before
commands.trigger(Attack { target: machine_entity });

// After
writer.write(Attack { machine: machine_entity });
```

Use `MessageWriter<Attack>` as a system parameter instead of `Commands::trigger()`.

## Reacting to state changes

Active states now have an `Active` component inserted on the state entity. Use Bevy's built-in change detection to react to state changes:

```rust
// Before
fn on_enter(enter: On<EnterState>, query: Query<&MyComponent>) {
    let machine = enter.state_machine;
    // ...
}
commands.entity(my_state).observe(on_enter);

// After (recommended — query-based, parallelizable)
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

// Combine with other components to scope to specific state types
fn on_pae_activate(
    q_entered: Query<&Active, (Added<Active>, With<StateComponent<MyMarker>>)>,
) {
    for active in &q_entered {
        // Only fires when a state with StateComponent<MyMarker> is entered
    }
}

app.add_systems(Update, on_enter.after(GearboxSet));
```

`Active { machine: Entity }` is inserted on every state entity that is currently active, and removed when the state is exited. Inside the `GearboxSchedule`, commands are flushed between `TransitionPhase` and `ExitPhase`, so `Added<Active>` and `RemovedComponents<Active>` are visible in `EntryPhase` / `ExitPhase` systems.

Observers still work for `EnterState` / `ExitState` — these are triggered as entity events after the schedule converges:

```rust
// After (observer — still supported, fires after convergence)
fn on_enter(enter: On<EnterState>, query: Query<&MyComponent>) {
    let machine = enter.machine;
    let state = enter.state;
    // ...
}
commands.entity(my_state).observe(on_enter);
```

## TransitionEvent trait → GearboxMessage trait

The `TransitionEvent` trait with phase sub-events (`ExitEvent`, `EdgeEvent`, `EntryEvent`) is gone. If you used `to_entry_event()` to fire side effects during transitions, use the `SideEffect` trait instead.

```rust
// Before
impl TransitionEvent for StartInvoke {
    type ExitEvent = NoEvent;
    type EdgeEvent = NoEvent;
    type EntryEvent = GoOff;
    type Validator = AcceptAll;

    fn to_entry_event(&self, entering: Entity, _: Entity, _: Entity) -> Option<GoOff> {
        Some(GoOff::new(entering, self.targets.clone()))
    }
}

// After (simple case — use the derive)
#[gearbox_message]
struct StartInvoke {
    machine: Entity,
    targets: Vec<Entity>,
}

// After (with side effects — derive handles the transition, #[side_effect] handles the rest)
#[gearbox_message]
struct StartInvoke {
    machine: Entity,
    targets: Vec<Entity>,
}

#[side_effect]
impl SideEffect<StartInvoke> for GoOff {
    fn produce(matched: &Matched<StartInvoke>) -> Option<Self> {
        Some(GoOff::new(matched.target, matched.message.targets.clone()))
    }
}

// Both are auto-registered via inventory — no manual calls needed
```

## Registration

`register_transition` still exists but now registers a message listener system instead of an observer.

```rust
// Before
app.register_transition::<Attack>();

// After (with derive — automatic via inventory, no manual call needed)
#[gearbox_message]
struct Attack { machine: Entity }

// After (manual — still works)
app.register_transition::<Attack>();
```

All macros use inventory for auto-registration. No manual `register_*` calls are needed when using them:

| Macro | Registers |
|---|---|
| `#[gearbox_message]` | Transition message + listener |
| `#[transition_message]` | Transition message + listener (for manual impls) |
| `#[side_effect]` | Side effect producer system |
| `#[state_component]` | State component enter/exit systems |
| `#[gearbox_param(...)]` | Parameter guard + optional sync binding |
| `#[state_bridge]` | Bevy `States` bridge |

## System ordering

Systems that read state machine results should run after `GearboxSet`:

```rust
app.add_systems(Update, my_system.after(GearboxSet));
```

## Quick reference

| 0.5                                        | 0.6                                                               |
| ------------------------------------------ | ----------------------------------------------------------------- |
| `#[derive(SimpleTransition, EntityEvent)]` | `#[gearbox_message]`                       |
| `EventEdge::<E>`                           | `MessageEdge::<M>`                                                |
| `commands.trigger(event)`                  | `writer.write(message)`                                           |
| `On<EnterState>` (`state_machine` field)   | `Query<&Active, Added<Active>>` or `On<EnterState>` (`machine` field) |
| `On<ExitState>` (`state_machine` field)    | `RemovedComponents<Active>` or `On<ExitState>` (`machine` field)      |
| `TransitionEvent::to_entry_event()`        | `impl SideEffect<M> for S`                                        |
| `NoEvent`                                  | (not needed)                                                      |
| `EventValidator`                           | `MessageValidator`                                                |
| `#[transition_event]` (attribute)          | `#[transition_message]` (attribute, for manual impls)              |
| `app.register_side_effect::<M, S>()`      | `#[side_effect]` on `impl SideEffect<M> for S`                    |
| `#[state_component]`                       | `#[state_component]` (unchanged)                                  |
| `#[gearbox_param(...)]`                    | `#[gearbox_param(...)]` (unchanged)                               |
| `#[state_bridge]`                          | `#[state_bridge]` (unchanged)                                     |
