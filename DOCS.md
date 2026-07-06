## Your first statechart

Gearbox state machines are entity hierarchies: **states are entities**,
**transitions are entities**, and everything lives in the ECS. You author them
as [`bsn`] scenes which spawns the whole tree atomically. 

This guide builds a small character chart step by step:

```text
Character (StateMachine, initial = Alive)
├── Alive (initial = Standing)
│   ├── Standing  ──Jump──> Jumping
│   └── Jumping   ──Land──> Standing
└── Dead
```

`Standing` and `Jumping` are substates of `Alive`: the character can only jump
or stand while alive. `Alive` and `Dead` are substates of the root.

### Building the chart

Author the machine as one `bsn!` scene. States nest under `Substates [ ... ]`,
edges under `Transitions [ ... ]`, and `#Name` references resolve to sibling
states within the scene. The relationship blocks set the `SubstateOf` and
`Source` back-references for you.

The root entity is also a state entity, so it can carry your gameplay
components right alongside `StateMachine`:

```rust
use bevy::prelude::*;
use bevy::scene::prelude::{bsn, CommandsSceneExt};
use bevy_gearbox::prelude::*;

fn spawn_character(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        #Character
            Player
            Collider::capsule(1.0, 0.5)
            Hitpoints { max: 100.0, current: 100.0 }
            StateMachine InitialState(#Alive)
        Substates [
            #Alive InitialState(#Standing) Substates [
                #Standing Transitions [
                    (Target(#Jumping) MessageEdge::<Jump>)
                ],
                #Jumping Transitions [
                    (Target(#Standing) MessageEdge::<Land>)
                ],
            ],
            #Dead,
        ]
    });
}
```

To attach a machine to an entity you already spawned, use `apply_scene`
instead of `spawn_scene` - the scene's root patches onto the existing entity:

```rust
commands.entity(player).apply_scene(bsn! {
    StateMachine InitialState(#Alive)
    Substates [ /* ... */ ]
});
```

### Triggering transitions

Transitions fire in response to **messages**. Define one with
`#[derive(GearboxMessage)]`, marking the entity it's addressed to with
`#[gearbox(target)]`. The message listener walks `SubstateOf` from that entity
to find the machine root, so you can address either the root or any substate:

```rust
#[derive(Message, Clone, Reflect, GearboxMessage)]
pub struct Jump {
    #[gearbox(target)]
    pub target: Entity,
}

#[derive(Message, Clone, Reflect, GearboxMessage)]
pub struct Land {
    #[gearbox(target)]
    pub target: Entity,
}
```

Deriving `GearboxMessage` also auto-registers the type through `inventory`.

```rust
app.add_plugins(GearboxPlugin::default());
```

Generic message types can't be auto-registered through `inventory`. Register
those explicitly with `app.register_transition::<Jump>()`.

Then write messages from any system - an input system fires `Jump`, a physics
system fires `Land`:

```rust
fn jump_input(
    input: Res<ButtonInput<KeyCode>>,
    q_players: Query<Entity, With<Player>>,
    mut writer: MessageWriter<Jump>,
) {
    if input.just_pressed(KeyCode::Space) {
        for player in &q_players {
            writer.write(Jump { target: player });
        }
    }
}
```

If two edges on the active branch match the same message, the deeper (leaf)
state wins. In parallel regions, each region consumes the message independently.

#### Filtering with a validator

By default every message of the right type matches. To filter per edge, supply
a custom validator:

```rust
#[derive(Message, Clone, Reflect, GearboxMessage)]
#[gearbox(validator = HighDamageOnly)]
pub struct Attacked {
    #[gearbox(target)]
    pub target: Entity,
    pub amount: f32,
}

#[derive(Default, Clone)]
pub struct HighDamageOnly;

impl MessageValidator<Attacked> for HighDamageOnly {
    fn matches(&self, msg: &Attacked) -> bool {
        msg.amount >= 50.0
    }
}
```

### Querying active states with `StateComponent`

The machine changes state internally, but from the outside you need a way to
tell what state it's in - for instance, to make your physics act on jumping
characters. A `StateComponent` clones its payload onto the machine **root**
while its state is active, and removes it when the state exits.

Because `StateComponent` isn't `Default`, insert it through a `template`
closure:

```rust
#[derive(Clone, Component)]
pub struct Jumping;

// In the scene, on the #Jumping state:
#Jumping
    template(|_| Ok(StateComponent(Jumping)))
    Transitions [ (Target(#Standing) MessageEdge::<Land>) ]
```

Now, while `Jumping` is active, the root carries a `Jumping` component, so a
plain query finds jumping characters:

```rust
fn falling_system(mut q_jumping: Query<&mut Velocity, With<Jumping>>) {
    for mut velocity in &mut q_jumping {
        // apply gravity to airborne characters
    }
}
```

> `StateInactiveComponent` is the inverse: it attaches its payload to the root
> while the state is **inactive**, removing it once the state becomes active.

### Reacting to enter/exit

There are two ways to run logic on state changes.

**Query change detection** (preferred for systems). Order your system after
`GearboxSet` so it sees this frame's changes:

```rust
fn on_enter(q_entered: Query<(Entity, &Active), Added<Active>>) {
    for (state, active) in &q_entered {
        // `state` was just entered; `active.machine` is the machine root.
    }
}

fn on_exit(mut removed: RemovedComponents<Active>) {
    for state in removed.read() {
        // `state` was just exited.
    }
}

app.add_systems(Update, (on_enter, on_exit).after(GearboxSet));
```

**Observers** (`EnterState` / `ExitState` entity events). These are triggered
on the state entity after the schedule converges, and carry the state and its
machine root:

```rust
fn on_enter_jumping(enter: On<EnterState>, mut q_velocity: Query<&mut Velocity>) {
    // `enter.state` is the entered state; `enter.machine` is the character root.
    if let Ok(mut velocity) = q_velocity.get_mut(enter.machine) {
        velocity.0.y += 5.0; // apply a jump impulse
    }
}

// Attach the observer to the #Jumping state entity, e.g. via a `template`:
#Jumping template(|_| Ok(/* ... */))
```

### Automatic and timed transitions

Not every edge needs a message.

- **`AlwaysEdge`** fires as soon as its source state becomes active. Use it to
  auto-advance a chart with no external trigger.
- Add a **`Delay`** to any edge to fire it after a duration while the source
  stays active - `Delay::from_secs_f32(0.8)` for an 0.8s cooldown.
- A **`TerminalState`** emits a `Done` message addressed to its parent when
  entered, so a `MessageEdge::<Done>` on the parent can transition out once a
  sub-chart finishes.

```rust
#Ready Transitions [
    (Target(#Invoking) AlwaysEdge)                       // fire immediately
],
#Invoking Transitions [
    (Target(#Cooldown) AlwaysEdge Delay::from_secs_f32(0.3))
],
#Cooldown Transitions [
    (Target(#Ready) AlwaysEdge Delay::from_secs_f32(0.8))  // cooldown, then loop
],
```

The `Ready → Invoking → Cooldown → Ready` shape is the invoked-ability pattern.
[`examples/invoked_loop.rs`](examples/invoked_loop.rs) runs a playable version —
press Space to fire, with the cast and cooldown legs paced by `Delay`s. With the
`gauge` feature, a `Delay` can alias a gauge attribute so cooldowns respond to
live stat changes.

### Side effects with payloads

`EnterState` / `ExitState` tell you a state changed, but not *why*. When a
transition should carry data - apply damage, spend a resource - read the
`Matched<M>` message. Gearbox writes one whenever a `MessageEdge<M>` matches,
carrying the original message plus the transition context (`source`, `target`,
`edge`, `machine`).

Run the reader in `GearboxPhase::SideEffectPhase`, and skip transitions that a
blocker vetoed by checking `BlockedEdges`:

```rust
fn apply_damage(
    mut reader: MessageReader<Matched<Attacked>>,
    blocked: Res<BlockedEdges>,
    mut q_hp: Query<&mut Hitpoints>,
) {
    for m in reader.read() {
        if blocked.is_blocked(m.edge) {
            continue; // a blocker rejected this transition
        }
        if let Ok(mut hp) = q_hp.get_mut(m.machine) {
            hp.current -= m.message.amount;
        }
    }
}

app.add_systems(
    GearboxSchedule,
    apply_damage.in_set(GearboxPhase::SideEffectPhase),
);
```

To make a state *accept* `Attacked` without leaving its current substate, give
it an **internal self-loop**. An internal edge (`EdgeKind::Internal`) keeps the
source and its active children intact rather than exiting and re-entering:

```rust
#Alive InitialState(#Standing) Transitions [
    (Target(#Alive) MessageEdge::<Attacked> EdgeKind::Internal)
] Substates [ /* Standing, Jumping ... */ ]
```

Sending `Attacked { target: character, amount }` is safe when the character is
`Dead`: `Dead` has no `Attacked` edge, so no `Matched<Attacked>` is produced and
no damage is applied. Edges are **external** by default; mark them
`EdgeKind::Internal` only when you want to stay within the source state.

### Blocking a transition

To veto a transition before it applies, run a system in
`GearboxPhase::BlockerPhase` that mutates the pending `TransitionMessage` and
sets `blocked = true`. `collect_blocked_edges` then records the edge so
side-effect systems skip it (as shown above).

---

For runnable, end-to-end examples see
[`examples/invoked_loop.rs`](examples/invoked_loop.rs) (a playable fire-and-cooldown
ability) and [`examples/parallel_regions.rs`](examples/parallel_regions.rs)
(parallel regions driven by keyboard input). Both are real windowed apps that
also serve the editor protocol — run one and connect the gearbox editor to it.
