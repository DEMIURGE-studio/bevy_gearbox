extern crate self as bevy_gearbox;

use bevy::{prelude::*, reflect::Reflect};
use bevy::platform::collections::HashSet;

// Re-exports
use crate::{guards::Guards, history::{History, HistoryState}};
pub use registration::RegistrationAppExt;
pub use bevy_gearbox_legacy_macros::SimpleTransition;
pub use bevy_gearbox_legacy_macros::transition_event;
pub use bevy_gearbox_legacy_macros::state_component;
pub use bevy_gearbox_legacy_macros::state_bridge;
pub use transitions::{TransitionEvent, NoEvent, AcceptAll};
pub use inventory;

pub mod commands;
pub mod guards;
pub mod history;
pub mod prelude;
pub mod parameter;
pub mod state_component;
pub mod transitions;
pub mod bevy_state;
pub mod registration;

/// The main plugin for `bevy_gearbox`. Registers events and adds the core systems.
pub struct GearboxPlugin;

impl Plugin for GearboxPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(transition_observer::<()> )
            .add_observer(initialize_state_machine)
            .add_observer(reset_state_region)
            .add_observer(transitions::always_edge_listener)
            .add_observer(transitions::start_after_on_enter)
            .add_observer(transitions::cancel_after_on_exit)
            .add_observer(transitions::reset_on_transition_actions);

        app.register_type::<InitialState>()
            .register_type::<StateMachine>()
            .register_type::<History>()
            .register_type::<HistoryState>()
            .register_type::<Substates>()
            .register_type::<SubstateOf>()
            .register_type::<StateMachineId>()
            .register_type::<Guards>()
            .register_type::<EnterState>()
            .register_type::<ExitState>()
            .register_type::<ResetRegion>()
            .register_type::<EdgeTraversed>()
            .register_type::<transitions::Delay>()
            .register_type::<transitions::Source>()
            .register_type::<transitions::Transitions>()
            .register_type::<transitions::Target>()
            .register_type::<transitions::TargetedBy>()
            .register_type::<transitions::AlwaysEdge>()
            .register_type::<transitions::EdgeKind>()
            .register_type::<transitions::NoEvent>()
            .register_type::<transitions::AcceptAll>()
            .register_type::<transitions::ResetEdge>()
            .register_type::<transitions::ResetScope>()
            .register_type::<state_component::Reset>();

        app.add_systems(Update, (
            transitions::check_always_on_guards_changed,
            transitions::tick_after_system,
        ));

        // Auto-register items discovered via inventory (transitions, states, params)
        app.run_auto_installers();
    }
}

// State-specific hierarchy relationships
#[derive(Component, Default, Debug, PartialEq, Eq, Reflect)]
#[relationship_target(relationship = SubstateOf, linked_spawn)]
#[reflect(Component, FromWorld, Default)]
pub struct Substates(Vec<Entity>);

impl<'a> IntoIterator for &'a Substates {
    type Item = <Self::IntoIter as Iterator>::Item;

    type IntoIter = std::slice::Iter<'a, Entity>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl Substates {
    pub fn new() -> Self {
        Self(Vec::new())
    }
}

#[derive(Component, Clone, PartialEq, Eq, Debug, Reflect)]
#[relationship(relationship_target = Substates)]
#[reflect(Component, PartialEq, Debug, FromWorld, Clone)]
pub struct SubstateOf(#[entities] pub Entity);

impl FromWorld for SubstateOf {
    #[inline(always)]
    fn from_world(_world: &mut World) -> Self {
        SubstateOf(Entity::PLACEHOLDER)
    }
}

/// An event that triggers a state transition in a machine.
/// Prefer using `edge` to reference a transition entity. The legacy `connection`
/// field is retained for initialization and backward compatibility.
#[derive(EntityEvent)]
pub struct Transition<T = ()> where T: Clone + Send + Sync + 'static {
    #[event_target]
    pub machine: Entity,
    /// The state that triggered this transition. This is used to determine the scope
    /// of the transition, especially in parallel state machines.
    pub source: Entity,
    /// The transition edge entity that defines the target and kind.
    pub edge: Entity,
    /// Optional typed payload that can be used by the transition observer
    pub payload: T,
}

#[derive(EntityEvent, Reflect)]
pub struct EdgeTraversed { #[event_target] pub target: Entity }

/// A component that specifies the initial substate for a state.
/// When a state is entered, the machine will recursively drill down through `InitialState`
/// components to find the leaf state(s) to activate.
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct InitialState(#[entities] pub Entity);

/// A component on the state machine's root entity that tracks the current active states.
/// - `active` contains all active states (root, ancestors, and leaves)
/// - `active_leaves` contains only the active leaf states
#[derive(Component, Reflect, Default)]
#[reflect(Component, Default)]
pub struct StateMachine {
    #[entities]
    pub active: HashSet<Entity>,
    #[entities]
    pub active_leaves: HashSet<Entity>,
}

impl StateMachine {
    pub fn new() -> Self {
        Self { active: HashSet::new(), active_leaves: HashSet::new() }
    }

    #[inline]
    pub fn insert(&mut self, entity: Entity) {
        self.active.insert(entity);
    }

    #[inline]
    pub fn insert_leaf(&mut self, entity: Entity) {
        self.active_leaves.insert(entity);
    }

    #[inline]
    pub fn is_active(&self, entity: &Entity) -> bool {
        self.active.contains(entity)
    }

    #[inline]
    pub fn is_leaf_active(&self, entity: &Entity) -> bool {
        self.active_leaves.contains(entity)
    }
}

/// Stable identifier for a state machine. Stored on the root state machine entity and
/// serialized as part of scene assets. Editors can use this to associate sidecar files
/// (e.g., by resolving `<id>.sm.ron`).
#[derive(Component, Reflect, Default, Clone)]
#[reflect(Component, Default, Clone)]
pub struct StateMachineId(pub String);

impl StateMachineId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// An event that is triggered on a state entity when it is being entered.
#[derive(EntityEvent, Reflect)]
pub struct EnterState { #[event_target] pub target: Entity, pub state_machine: Entity }

/// An event that is triggered on a state entity when it is being exited.
#[derive(EntityEvent, Reflect)]
pub struct ExitState { #[event_target] pub target: Entity, pub state_machine: Entity }

/// Event to reset a state machine: clear Active flags under the root and reinitialize
#[derive(EntityEvent, Reflect)]
pub struct ResetRegion { #[event_target] pub target: Entity }

impl ResetRegion {
    pub fn new(entity: Entity) -> Self { Self { target: entity } }
}

/// The core system that observes `Transition` events and orchestrates the state change.
/// It calculates the exit and entry paths, sends `ExitState` and `EnterState` events
/// to the appropriate states, and updates the machine's `CurrentState`.
/// Also handles history state saving and restoration.
fn transition_observer<T: transitions::PhasePayload>(
    transition: On<Transition<T>>,
    mut q_sm: Query<&mut StateMachine>,
    q_children: Query<&Substates>,
    q_initial_state: Query<&InitialState>,
    q_history: Query<&History>,
    mut q_history_state: Query<&mut HistoryState>,
    q_substate_of: Query<&SubstateOf>,
    q_edge_target: Query<&transitions::Target>,
    q_kind: Query<&transitions::EdgeKind>,
    mut commands: Commands,
) {
    let state_machine = transition.event().machine;
    let source_state = transition.event().source;
    // Resolve target: prefer Target on the edge; otherwise treat the edge itself
    // as the super state to start from (useful for root init where initial is on the state).
    let new_super_state = match q_edge_target.get(transition.event().edge) {
        Ok(edge_target) => edge_target.0,
        Err(_) => transition.event().edge,
    };

    let Ok(mut current_state) = q_sm.get_mut(state_machine) else {
        return;
    };

    // Handle initialization case where there are no current active states
    if current_state.active_leaves.is_empty() {
        // Build path from target up to (but excluding) the machine root
        let mut path_to_target: Vec<Entity> = vec![new_super_state];
        path_to_target.extend(
            q_substate_of
                .iter_ancestors(new_super_state)
                .take_while(|&ancestor| ancestor != state_machine),
        );

        // Enter ancestors parent→child down to the target
        for entity in path_to_target.iter().rev() {
            commands.trigger(EnterState { target: *entity, state_machine });
        }

        let new_leaf_states = get_all_leaf_states(
            new_super_state,
            state_machine,
            &q_initial_state,
            &q_children,
            &q_history,
            &q_history_state,
            &q_substate_of,
            &mut commands,
        );
        current_state.active_leaves.extend(new_leaf_states);
        // Derive full active set from leaves
        current_state.active = compute_active_from_leaves(&current_state.active_leaves, &q_substate_of);
        // Ensure the root state machine entity is always considered active
        current_state.active.insert(state_machine);
        return;
    }

    // Determine whether the source is a parallel parent (implicit: has children and lacks InitialState)
    let source_is_parallel = q_initial_state.get(source_state).is_err() && q_children.get(source_state).is_ok();

    // Exit/enter computation diverges for parallel parents
    let (states_to_exit_vec, states_to_enter_vec) = if source_is_parallel {
        // 1) Exit: all active leaves under the parallel source up to (and including) the source
        let mut ordered_exits: Vec<Entity> = Vec::new();
        let mut seen: HashSet<Entity> = HashSet::new();
        for &leaf in current_state.active_leaves.iter() {
            // Consider only leaves that are descendants of the parallel source
            let is_descendant = leaf == source_state
                || q_substate_of.iter_ancestors(leaf).any(|a| a == source_state);
            if !is_descendant { continue; }

            // Exit path from leaf up to source_state (inclusive)
            let path = get_path_to_root(leaf, &q_substate_of);
            if let Some(pos) = path.iter().position(|&e| e == source_state) {
                let slice = &path[..=pos]; // includes source_state
                for &e in slice {
                    if seen.insert(e) { ordered_exits.push(e); }
                }
            }
        }

        // 2) Enter: compute LCA between source_state and new_super_state
        let exit_path_from_source = get_path_to_root(source_state, &q_substate_of);
        let enter_path = get_path_to_root(new_super_state, &q_substate_of);

        let mut lca_depth = exit_path_from_source
            .iter()
            .rev()
            .zip(enter_path.iter().rev())
            .take_while(|(a, b)| a == b)
            .count();

        let lca_entity = if lca_depth > 0 { Some(exit_path_from_source[exit_path_from_source.len() - lca_depth]) } else { None };

        let is_internal = matches!(q_kind.get(transition.event().edge), Ok(transitions::EdgeKind::Internal));
        if !is_internal {
            // If source is the LCA, default external re-enters the source
            if lca_entity == Some(source_state) {
                lca_depth = lca_depth.saturating_sub(1);
            }
        }

        let states_to_enter = enter_path[..enter_path.len() - lca_depth].to_vec();
        (ordered_exits, states_to_enter)
    } else {
        // Non-parallel source: may still have multiple active descendant leaves if there are
        // deeper parallel regions underneath. Exit ALL active descendant leaves under `source_state`.
        let mut descendant_leaves: Vec<Entity> = current_state
            .active_leaves
            .iter()
            .copied()
            .filter(|leaf| {
                *leaf == source_state
                    || q_substate_of
                        .iter_ancestors(*leaf)
                        .any(|ancestor| ancestor == source_state)
            })
            .collect();

        if descendant_leaves.is_empty() {
            // This transition is not coming from any of the currently active states.
            return;
        }

        let enter_path = get_path_to_root(new_super_state, &q_substate_of);
        let is_internal = matches!(q_kind.get(transition.event().edge), Ok(transitions::EdgeKind::Internal));

        // Build ordered exits by walking each leaf up to (but not including) the LCA with the target path
        let mut ordered_exits: Vec<Entity> = Vec::new();
        let mut seen: HashSet<Entity> = HashSet::new();
        let mut min_lca_depth: Option<usize> = None;

        for leaf in descendant_leaves.drain(..) {
            let exit_path = get_path_to_root(leaf, &q_substate_of);
            let mut lca_depth = exit_path
                .iter()
                .rev()
                .zip(enter_path.iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            let lca_entity = if lca_depth > 0 { Some(exit_path[exit_path.len() - lca_depth]) } else { None };

            if !is_internal {
                if new_super_state == leaf {
                    lca_depth = lca_depth.saturating_sub(1);
                } else if lca_entity == Some(source_state) {
                    lca_depth = lca_depth.saturating_sub(1);
                }
            }

            // Track minimal lca_depth across all leaves to compute entry path later
            min_lca_depth = Some(match min_lca_depth {
                Some(min) => min.min(lca_depth),
                None => lca_depth,
            });

            // Exit from leaf up to (but not including) the LCA portion
            let upto = exit_path.len() - lca_depth;
            for &e in &exit_path[..upto] {
                if seen.insert(e) {
                    ordered_exits.push(e);
                }
            }
        }

        let lca_depth_final = min_lca_depth.unwrap_or(0);
        let states_to_enter = enter_path[..enter_path.len() - lca_depth_final].to_vec();
        (ordered_exits, states_to_enter)
    };

    // Invoke typed Exit payload once at the start (root + source)
    transition.event().payload.on_exit(&mut commands, source_state, &q_children, &current_state);
    for entity in states_to_exit_vec.iter() {
        // Save history if this state has history behavior
        if let Ok(history) = q_history.get(*entity) {
            let states_to_save = match history {
                History::Shallow => {
                    // For shallow history, save the immediate child of `entity` on the path
                    // to each active leaf descendant (handles both normal and parallel parents).
                    let mut saved: HashSet<Entity> = HashSet::new();
                    for &leaf in current_state.active_leaves.iter() {
                        // Track the previous node while walking ancestors; when we hit `entity`,
                        // `prev` is the immediate child under `entity`.
                        let mut prev = leaf;
                        for ancestor in q_substate_of.iter_ancestors(leaf) {
                            if ancestor == *entity {
                                saved.insert(prev);
                                break;
                            }
                            prev = ancestor;
                        }
                    }
                    saved
                }
                History::Deep => {
                    // For deep history, save all active descendant leaves
                    current_state.active_leaves.iter()
                        .filter(|&&state| {
                            state == *entity || q_substate_of
                                .iter_ancestors(state)
                                 .any(|ancestor| ancestor == *entity)
                        })
                        .copied()
                        .collect()
                }
            };
            
            // Insert or update the history state
            if let Ok(mut existing_history) = q_history_state.get_mut(*entity) {
                existing_history.0 = states_to_save;
            } else {
                commands.entity(*entity).insert(HistoryState(states_to_save));
            }
        }
        
        commands.trigger(ExitState { target: *entity, state_machine });
    }

    // Update the current state set
    // For parallel parents we potentially exited multiple leaves; remove any leaves we exited.
    for exited in states_to_exit_vec.iter() {
        // Only remove if it was a leaf previously
        current_state.active_leaves.remove(exited);
    }

    // Transition actions phase (between exits and entries)
    commands.trigger(EdgeTraversed { target: transition.event().edge });
    transition.event().payload.on_edge(&mut commands, transition.event().edge, &q_children, &current_state);
    // Invoke typed Effect payload if present
    // Note: we avoid trait bounds here; user code can downcast payload if desired via helper

    // Enter from parent to child
    for entity in states_to_enter_vec.iter().rev() {
        commands.trigger(EnterState { target: *entity, state_machine });
    }

    // Now, from the entered super state, drill down to the new leaf states.
    let new_leaf_states = get_all_leaf_states(
        new_super_state,
        state_machine,
        &q_initial_state,
        &q_children,
        &q_history,
        &q_history_state,
        &q_substate_of,
        &mut commands,
    );
    current_state.active_leaves.extend(new_leaf_states);
    // Invoke typed Entry payload
    transition.event().payload.on_entry(&mut commands, new_super_state, &q_children, &current_state);
    // Derive full active set from leaves
    current_state.active = compute_active_from_leaves(&current_state.active_leaves, &q_substate_of);
    // Ensure the root state machine entity is always considered active
    current_state.active.insert(state_machine);
}

fn get_path_to_root(start_entity: Entity, q_substate_of: &Query<&SubstateOf>) -> Vec<Entity> {
    let mut path = vec![start_entity];
    path.extend(q_substate_of.iter_ancestors(start_entity));
    path
}

pub fn get_all_leaf_states(
    start_node: Entity,
    state_machine: Entity,
    q_initial_state: &Query<&InitialState>,
    q_children: &Query<&Substates>,
    q_history: &Query<&History>,
    q_history_state: &Query<&mut HistoryState>,
    q_substate_of: &Query<&SubstateOf>,
    commands: &mut Commands,
) -> HashSet<Entity> {
    let mut leaves = HashSet::new();
    let mut stack = vec![start_node];

    while let Some(entity) = stack.pop() {
        let mut found_next = false;

        // 1) History takes precedence (works for both parallel and non-parallel parents)
        if let (Ok(history), Ok(history_state)) = (q_history.get(entity), q_history_state.get(entity)) {
            found_next = true;
            match history {
                History::Shallow => {
                    for &saved_state in &history_state.0 {
                        commands.trigger(EnterState { target: saved_state, state_machine });
                        stack.push(saved_state);
                    }
                }
                History::Deep => {
                    for &saved_state in &history_state.0 {
                        let mut path_to_substate = vec![saved_state];
                        path_to_substate.extend(
                            q_substate_of
                                .iter_ancestors(saved_state)
                                .take_while(|&ancestor| ancestor != entity),
                        );
                        for e in path_to_substate.iter().rev() {
                            commands.trigger(EnterState { target: *e, state_machine });
                        }
                        leaves.insert(saved_state);
                    }
                    continue;
                }
            }
        }
        // 2) If it's a parallel state (without history), explore all children regions.
        else if q_initial_state.get(entity).is_err() {
            if let Ok(children) = q_children.get(entity) {
                found_next = true;
                for &child in children {
                    commands.trigger(EnterState { target: child, state_machine });
                    stack.push(child);
                }
            }
        }
        // 3) If it has a single initial state, explore that.
        else if let Ok(initial_state) = q_initial_state.get(entity) {
            found_next = true;

            // To enter a deeply nested initial state, we must first enter all of its parents
            // that are descendants of the current state (`entity`).
            let mut path_to_substate = vec![initial_state.0];
            path_to_substate.extend(
                q_substate_of
                    .iter_ancestors(initial_state.0)
                    .take_while(|&ancestor| ancestor != entity),
            );

            // Enter from parent to child
            for e in path_to_substate.iter().rev() {
                commands.trigger(EnterState { target: *e, state_machine });
            }

            stack.push(initial_state.0);
        }

        // Otherwise, it's a leaf state.
        if !found_next {
            leaves.insert(entity);
        }
    }
    leaves
}

fn compute_active_from_leaves(
    leaves: &HashSet<Entity>,
    q_substate_of: &Query<&SubstateOf>,
) -> HashSet<Entity> {
    let mut active: HashSet<Entity> = HashSet::new();
    for &leaf in leaves.iter() {
        active.insert(leaf);
        for ancestor in q_substate_of.iter_ancestors(leaf) {
            active.insert(ancestor);
        }
    }
    active
}

/// Triggers the InitializeMachine event when AbilityMachine component is added.
fn initialize_state_machine(
    add: On<Add, StateMachine>,
    mut commands: Commands,
) {
    let target = add.event().entity;
    // Always attempt to initialize: root-as-leaf, parallel, or parent with InitialState
    commands.trigger(Transition { machine: target, source: target, edge: target, payload: () });
}

/// Resets a machine by clearing Active components under the root and re-inserting StateMachine
fn reset_state_region(
    reset_region: On<ResetRegion>,
    mut commands: Commands,
) {
    let root = reset_region.target;

    commands.entity(root).try_remove::<StateMachine>().insert(StateMachine::new());
}