# Changelog

All notable changes to bevy_gearbox. The workspace crates (`bevy_gearbox`,
`bevy_gearbox_core`, `bevy_gearbox_macros`, `bevy_gearbox_macros_impl`,
`bevy_gearbox_protocol`, `bevy_gearbox_editor`) share one version and are
released together.

## Unreleased

### Breaking

- **Guarded transitions replace `BranchTransition`.** `BranchTransition`,
  `BranchArm`, `BranchBuilder` and `SpawnBranch` are removed. A conditional
  transition is now several edges for the same trigger in `Transitions` order,
  each carrying whatever guard marker it needs; a guard is a system in
  `GearboxPhase::BlockerPhase` that vetoes candidates, and the first edge no
  guard vetoes wins. If every edge on the active state is vetoed, the parent's
  edges are tried. See the "Guards" section of the guide.
- **Guards now apply to delayed edges too.** A `Delay` edge whose guard vetoes
  it falls through to the next edge with the same delay, like an XState
  `after: { ms: [ .. ] }` list.
- **`GearboxPhase::EdgeCheckPhase` removed.** Use `EdgeDetectPhase`.
- **`DeferEvent<M>` removed.** It was never wired up. Park messages in a
  resource and rewrite them from a system, or use an internal self-loop edge.
- **The `server` feature is off by default.** It adds an HTTP server and world
  serialization to a game; enable it explicitly to use the editor. The
  `protocol` feature alias is gone; use `server`.
- **`StateMachineId` lives in the prelude now** (moved to `bevy_gearbox_core`;
  `bevy_gearbox::server::StateMachineId` still works). Its reflected type path
  changed, so scenes the editor saved before this release need that entry
  re-pathed.
- `bevy_gearbox_protocol`: the `client` feature gates the HTTP client and Tokio
  runtime; `server` gates the file scanning. Depend with
  `default-features = false` and pick a side. The `BRP_URL` environment
  variable is no longer read; use `GEARBOX_PROTOCOL_URL`.
- **A message addressed to a substate only reaches that subtree.** Edges on
  the addressed state and its descendants are considered; ancestors' edges are
  not. Address the machine root (the usual case) to offer a message to every
  active state. This is what makes `Done` land on the one state that finished.
- **`EnterState` / `ExitState` fire inside the gearbox schedule**, in
  `ExitPhase` and `EntryPhase`, rather than once after it converges. A state
  passed through within one frame now gets both events, exits come
  deepest-first and entries shallowest-first, and exit observers run before
  entry observers. Observers that assumed the frame had settled should move
  their logic to a system after `GearboxSet`.

### Added

- Every authorable component lowers in `bsn!`: `StateComponent::<Walking>`,
  `StateComponent::<Speed>(Speed(7.5))`, `StateMachineId("ability")`,
  `History::Deep`, `ResetEdge(ResetScope::Target)` and `Source(#X)` all work
  directly, with no `template(..)` closures.
- `GearboxPlugin` is exported from the prelude.
- **Parallel completion.** A parallel state is done when every region has
  reached a `TerminalState`: `Done` is addressed to the parallel state so its
  own `MessageEdge<Done>` fires, and nested parallel states cascade. Before,
  one finished region fired the parent's `Done` edge.
- **`InState(#Other)` / `NotInState(#Other)` guards** on an edge, vetoed by a
  built-in blocker while the named state is inactive / active (XState's
  `stateIn`). Authorable in `bsn!` with no Rust.
- `bevy_gearbox_macros_impl`: lets a crate that re-exports gearbox offer
  `#[derive(GearboxMessage)]`, `#[state_component]` and `#[state_bridge]` under
  its own path, so its users need no direct `bevy_gearbox` dependency.
- Editor: "Make Initial" works (new `editor.set_initial_state` RPC), the canvas
  marks the actual initial child instead of guessing the first one, and
  selecting a node highlights its incoming edges again.
- `examples/guarded_transitions.rs`, a playable guards demo.
- Crate-level documentation on every crate, `LICENSE-MIT` and `LICENSE-APACHE`.

### Fixed

- Editor server: saving a chart over an existing `.scn.ron` / `.sm.ron` failed
  on Windows; re-saving now works.
- Editor: "Make Parent" and "Make Parallel" kept the requested child name
  instead of always producing "New State".
- Editor: machines without a `Name` were never listed in the explorer.
- Editor: the control-bus watch task was leaked on disconnect; stream errors
  from the game are now logged instead of dropped.
- Editor: the transition-kind picker's filter box works; a layout sidecar saved
  for a different chart shape is reported on load.

### Changed

- `Substates` and `Transitions` are reflected and saved with a chart, like
  Bevy's `Children`, so a scene round-trips its edge order.

- Several always-edges on one state form an ordered list; one fires per parallel
  region per entry (previously one per machine).
- Examples use `on(..)` entry observers attached in the scene, and run without
  the editor server unless `--features server` is given.

## 0.8.1

Last release before this changelog. See the git history.
