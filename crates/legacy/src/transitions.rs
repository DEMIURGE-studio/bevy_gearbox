use std::marker::PhantomData;
use std::time::Duration;

use bevy::platform::collections::HashSet;
use bevy::prelude::*;

use crate::state_component::Reset;
use crate::{guards::Guards, EnterState, ExitState, StateMachine, SubstateOf, Transition};
use crate::{InitialState, Substates};

/// Outbound transitions from a source state. Order defines priority (first match wins).
#[derive(Component, Default, Debug, PartialEq, Eq, Reflect)]
#[relationship_target(relationship = Source, linked_spawn)]
#[reflect(Component, FromWorld, Default)]
pub struct Transitions(Vec<Entity>);

impl<'a> IntoIterator for &'a Transitions {
    type Item = <Self::IntoIter as Iterator>::Item;

    type IntoIter = std::slice::Iter<'a, Entity>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl Transitions {
    pub fn new() -> Self {
        Self(Vec::new())
    }
}

#[derive(Component, Clone, PartialEq, Eq, Debug, Reflect)]
#[relationship(relationship_target = Transitions)]
#[reflect(Component, PartialEq, Debug, FromWorld, Clone)]
pub struct Source(#[entities] pub Entity);

impl FromWorld for Source {
    #[inline(always)]
    fn from_world(_world: &mut World) -> Self {
        Source(Entity::PLACEHOLDER)
    }
}

/// Incoming edge list for a target state (inverse of `Target`).
#[derive(Component, Default, Debug, PartialEq, Eq, Reflect)]
#[relationship_target(relationship = Target, linked_spawn)]
#[reflect(Component, FromWorld, Default)]
pub struct TargetedBy(Vec<Entity>);

impl<'a> IntoIterator for &'a TargetedBy {
    type Item = <Self::IntoIter as Iterator>::Item;

    type IntoIter = std::slice::Iter<'a, Entity>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl TargetedBy {
    pub fn new() -> Self {
        Self(Vec::new())
    }
}

/// Target for an edge transition.
#[derive(Component, Clone, PartialEq, Eq, Debug, Reflect)]
#[relationship(relationship_target = TargetedBy)]
#[reflect(Component, PartialEq, Debug, FromWorld, Clone)]
pub struct Target(#[entities] pub Entity);

impl FromWorld for Target {
    #[inline(always)]
    fn from_world(_world: &mut World) -> Self {
        Target(Entity::PLACEHOLDER)
    }
}

/// Whether the transition should be treated as External (default) or Internal.
#[derive(Component, Reflect, Default, Clone, Copy, Debug)]
#[reflect(Component, Default)]
pub enum EdgeKind {
    #[default]
    External,
    Internal,
}

/// Marker for a transition that should fire on entering the source state (no event).
#[derive(Component, Reflect, Default, Debug)]
#[reflect(Component, Default)]
#[require(EdgeKind)]
pub struct AlwaysEdge;

/// Delayed transition configuration: fire after `duration` has elapsed while the source is active.
#[derive(Component, Reflect, Default)]
#[reflect(Component, Default)]
pub struct Delay {
    pub duration: Duration,
}

impl Delay {
    #[inline]
    pub fn new(duration: Duration) -> Self {
        Self { duration }
    }

    pub fn from_f32(duration: f32) -> Self {
        Self {
            duration: Duration::from_secs_f32(duration),
        }
    }
}

#[derive(Component)]
pub struct EdgeTimer(pub Timer);

/// Pending event stored on an edge awaiting its After timer
#[derive(Component)]
pub struct PendingEvent<E: EntityEvent + Clone> {
    pub event: E,
}

/// Marker event to represent absence of a payload
#[derive(EntityEvent, Reflect, Clone)]
#[reflect(Default)]
pub struct NoEvent(Entity);

impl Default for NoEvent {
    fn default() -> Self {
        Self(Entity::PLACEHOLDER)
    }
}

/// Derive macro for simple events that don't need phase-specific payloads
pub use bevy_gearbox_legacy_macros::SimpleTransition;

fn cleanup_edge_timer_and_pending<E: EntityEvent + Clone + 'static>(
    commands: &mut Commands,
    edge: Entity,
) {
    commands
        .entity(edge)
        .try_remove::<EdgeTimer>()
        .try_remove::<PendingEvent<E>>();
}

/// Trait implemented by events that can provide phase-specific payloads
/// for the transition lifecycle. Methods default to returning None.
pub trait TransitionEvent: EntityEvent + Clone + 'static {
    type ExitEvent: EntityEvent + Clone;
    type EdgeEvent: EntityEvent + Clone;
    type EntryEvent: EntityEvent + Clone;
    type Validator: EventValidator<Self> + Reflect + Default + Clone;

    /// Map this trigger into an optional Exit event. Called once per transition before exits.
    /// source: the state that initiated the transition; machine: the state machine root entity
    fn to_exit_event(
        &self,
        _exiting: Entity,
        _entering: Entity,
        _edge: Entity,
    ) -> Option<Self::ExitEvent> {
        None
    }

    /// Map this trigger into an optional Edge event. Called during the transition actions phase.
    /// edge: the transition edge entity
    fn to_edge_event(&self, _edge: Entity) -> Option<Self::EdgeEvent> {
        None
    }

    /// Map this trigger into an optional Entry event. Called once after entries, on the entering super-state.
    /// entering: the entering super-state entity; exiting: the exiting super-state entity; edge: transition edge
    fn to_entry_event(
        &self,
        _entering: Entity,
        _exiting: Entity,
        _edge: Entity,
    ) -> Option<Self::EntryEvent> {
        None
    }
}

/// Validator for event E used to accept/reject an event for a specific edge
pub trait EventValidator<E>: Send + Sync + 'static + Reflect {
    fn matches(&self, event: &E) -> bool;
}

/// Default validator that accepts all events
#[derive(Reflect, Default, Clone)]
#[reflect(Default)]
pub struct AcceptAll;

impl<E> EventValidator<E> for AcceptAll {
    #[inline]
    fn matches(&self, _event: &E) -> bool {
        true
    }
}

/// A typed phase payload holder built from a TransitionEvent
#[derive(Clone, Default)]
pub struct PhaseEvents<
    Exit: EntityEvent + Clone = NoEvent,
    Edge: EntityEvent + Clone = NoEvent,
    Entry: EntityEvent + Clone = NoEvent,
> {
    pub exit: Option<Exit>,
    pub edge: Option<Edge>,
    pub entry: Option<Entry>,
}

/// Phase callbacks the transition observer will invoke at microsteps
pub trait PhasePayload: Clone + Send + Sync + 'static {
    fn on_exit(
        &self,
        _commands: &mut Commands,
        _source: Entity,
        _children: &Query<&Substates>,
        _state_machine: &StateMachine,
    ) {
    }
    fn on_edge(
        &self,
        _commands: &mut Commands,
        _edge: Entity,
        _children: &Query<&Substates>,
        _state_machine: &StateMachine,
    ) {
    }
    fn on_entry(
        &self,
        _commands: &mut Commands,
        _target: Entity,
        _children: &Query<&Substates>,
        _state_machine: &StateMachine,
    ) {
    }
}

impl PhasePayload for () {}

impl<Exit, Edge, Entry> PhasePayload for PhaseEvents<Exit, Edge, Entry>
where
    Exit: EntityEvent + Clone,
    Edge: EntityEvent + Clone,
    Entry: EntityEvent + Clone,
    for<'a> <Exit as Event>::Trigger<'a>: Default,
    for<'a> <Edge as Event>::Trigger<'a>: Default,
    for<'a> <Entry as Event>::Trigger<'a>: Default,
{
    fn on_exit(
        &self,
        commands: &mut Commands,
        _source: Entity,
        _children: &Query<&Substates>,
        _state_machine: &StateMachine,
    ) {
        if let Some(ev) = self.exit.clone() {
            commands.trigger(ev);
        }
    }

    fn on_edge(
        &self,
        commands: &mut Commands,
        _edge: Entity,
        _children: &Query<&Substates>,
        _state_machine: &StateMachine,
    ) {
        if let Some(ev) = self.edge.clone() {
            commands.trigger(ev);
        }
    }

    fn on_entry(
        &self,
        commands: &mut Commands,
        _target: Entity,
        _children: &Query<&Substates>,
        _state_machine: &StateMachine,
    ) {
        if let Some(ev) = self.entry.clone() {
            commands.trigger(ev);
        }
    }
}

fn validate_edge_basic(
    edge: Entity,
    q_guards: &Query<&Guards>,
    q_target: &Query<&Target>,
    q_substate_of: &Query<&SubstateOf>,
) -> bool {
    // Check guards if present
    if let Ok(guards) = q_guards.get(edge) {
        if !guards.check() {
            return false;
        }
    }
    // Must have valid target
    if let Ok(Target(target)) = q_target.get(edge) {
        // Consider the edge valid only if the target state still exists.
        // We treat targets as states; states (other than machine root) have SubstateOf.
        q_substate_of.get(*target).is_ok()
    } else {
        false
    }
}

/// Generic edge firing logic for TransitionEvent
fn try_fire_first_matching_edge_generic<E: TransitionEvent + Clone>(
    source: Entity,
    event: &E,
    q_transitions: &Query<&Transitions>,
    q_listener: &Query<&EventEdge<E>>,
    q_edge_target: &Query<&Target>,
    q_guards: &Query<&Guards>,
    q_substate_of: &Query<&SubstateOf>,
    q_defer: &mut Query<&mut DeferEvent<E>>,
    q_sm: &Query<&StateMachine>,
    q_delay: &Query<&Delay>,
    q_timer: &mut Query<&mut EdgeTimer>,
    commands: &mut Commands,
) -> bool {
    // Check if this state should defer this event type
    if let Ok(mut defer_event) = q_defer.get_mut(source) {
        let root = q_substate_of.root_ancestor(source);
        if let Ok(sm) = q_sm.get(root) {
            if sm.is_active(&source) {
                defer_event.defer_event(event.clone());
                return false;
            }
        }
    }

    let Ok(transitions) = q_transitions.get(source) else {
        return false;
    };

    for edge in transitions.into_iter().copied() {
        let Ok(listener) = q_listener.get(edge) else {
            continue;
        };

        // Validate edge (guards and target) - skip if invalid
        if !validate_edge_basic(edge, q_guards, q_edge_target, q_substate_of) {
            continue;
        }

        // If validator present on the edge, require match
        if let Some(v) = &listener.validator {
            if !v.matches(event) {
                continue;
            }
        }

        // If edge is delayed, schedule timer and store pending event
        if let Ok(after) = q_delay.get(edge) {
            if let Ok(mut timer) = q_timer.get_mut(edge) {
                timer.0.set_duration(after.duration);
                timer.0.reset();
            } else {
                commands
                    .entity(edge)
                    .insert(EdgeTimer(Timer::new(after.duration, TimerMode::Once)));
            }
            commands.entity(edge).insert(PendingEvent::<E> {
                event: event.clone(),
            });
            return true;
        }

        // Build typed phase events with full context
        let target = q_edge_target.get(edge).ok().map(|t| t.0).unwrap_or(source);
        let payload = PhaseEvents {
            exit: event.to_exit_event(source, target, edge),
            edge: event.to_edge_event(edge),
            entry: event.to_entry_event(target, source, edge),
        };
        let root = q_substate_of.root_ancestor(source);
        commands.trigger(Transition {
            machine: root,
            source,
            edge,
            payload,
        });
        return true;
    }
    false
}

/// Attach this to a transition entity to react to a specific event `E`.
#[derive(Reflect, Component)]
#[reflect(Component, Default)]
#[require(EdgeKind)]
pub struct EventEdge<E: TransitionEvent> {
    #[reflect(ignore)]
    _marker: PhantomData<E>,
    /// Optional per-edge validator; when None, all events of type E are accepted
    pub validator: Option<<E as TransitionEvent>::Validator>,
}

impl<E: TransitionEvent> Default for EventEdge<E> {
    fn default() -> Self {
        Self {
            _marker: PhantomData,
            validator: None,
        }
    }
}

impl<E: TransitionEvent> EventEdge<E> {
    #[inline]
    pub fn new(validator: Option<<E as TransitionEvent>::Validator>) -> Self {
        Self {
            _marker: PhantomData,
            validator,
        }
    }
}

/// A component that can be added to states to an event of a specific type.
/// Event of type `E` that arrive while this state is active will be stored
/// and replayed when the state is exited.
#[derive(Component)]
pub struct DeferEvent<E: EntityEvent> {
    pub deferred: Option<E>,
}

impl<E: EntityEvent> Default for DeferEvent<E> {
    fn default() -> Self {
        Self { deferred: None }
    }
}

impl<E: EntityEvent> DeferEvent<E> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn defer_event(&mut self, event: E) {
        self.deferred = Some(event);
    }

    pub fn take_deferred(&mut self) -> Option<E> {
        std::mem::take(&mut self.deferred)
    }
}

/// Marker to request reset of subtree(s) during TransitionActions phase
#[derive(Component, Reflect, Default)]
#[reflect(Component, Default)]
pub struct ResetEdge(pub ResetScope);

#[derive(Reflect, Default, Clone, Copy)]
pub enum ResetScope {
    #[default]
    Source,
    Target,
    Both,
}

/// On EnterState(source), evaluate AlwaysEdge transitions listed in `Transitions(source)` in order.
/// Respects After components - transitions with After will be handled by the timer system instead.
pub fn always_edge_listener(
    enter_state: On<EnterState>,
    q_transitions: Query<&Transitions>,
    q_always: Query<(), With<AlwaysEdge>>,
    q_edge_target: Query<&Target>,
    q_guards: Query<&Guards>,
    q_delay: Query<&Delay>,
    q_substate_of: Query<&SubstateOf>,
    mut commands: Commands,
) {
    let source = enter_state.target;
    let Ok(transitions) = q_transitions.get(source) else {
        return;
    };

    // Evaluate in order; fire the first allowed transition
    for edge in transitions.into_iter().copied() {
        if q_always.get(edge).is_err() {
            continue;
        }

        // Skip transitions with After component - let the timer system handle them
        if q_delay.get(edge).is_ok() {
            continue;
        }

        // Validate edge (guards and target)
        if !validate_edge_basic(edge, &q_guards, &q_edge_target, &q_substate_of) {
            continue;
        }

        // Fire transition
        let root = q_substate_of.root_ancestor(source);
        commands.trigger(Transition {
            machine: root,
            source,
            edge,
            payload: (),
        });
        break;
    }
}

/// Helper function to find the parallel region root for a given state.
/// Returns the state itself if it's not under a parallel region.
fn find_parallel_region_root(
    state: Entity,
    q_substate_of: &Query<&SubstateOf>,
    q_initial_state: &Query<&InitialState>,
    q_children: &Query<&Substates>,
) -> Entity {
    // Walk up the hierarchy to find if we're under a parallel state
    let mut previous_ancestor = state;
    for ancestor in q_substate_of.iter_ancestors(state) {
        // Implicit parallel: ancestor has children and lacks InitialState
        let has_direct_child = q_children
            .get(ancestor)
            .ok()
            .map(|c| c.into_iter().next().is_some())
            .unwrap_or(false);
        let is_parallel = !q_initial_state.contains(ancestor) && has_direct_child;
        if is_parallel {
            return previous_ancestor;
        }
        previous_ancestor = ancestor;
    }

    // Not under a parallel state, return the state itself
    state
}

/// On event `E`, scan `Transitions` for a matching edge with `EventEdge<E>`, in priority order.
pub(crate) fn edge_event_listener<E: TransitionEvent + Clone>(
    transition_event: On<E>,
    q_transitions: Query<&Transitions>,
    q_listener: Query<&EventEdge<E>>,
    q_edge_target: Query<&Target>,
    q_guards: Query<&Guards>,
    q_substate_of: Query<&SubstateOf>,
    q_sm: Query<&StateMachine>,
    mut q_defer: Query<&mut DeferEvent<E>>,
    q_initial_state: Query<&InitialState>,
    q_children: Query<&Substates>,
    q_delay: Query<&Delay>,
    mut q_timer: Query<&mut EdgeTimer>,
    mut commands: Commands,
) {
    let event = transition_event.event();
    let machine_root = transition_event.event().event_target();

    // If the event target is a machine root, try leaves/branches first (statechart-like), then fall back to root
    if let Ok(current) = q_sm.get(machine_root) {
        let mut visited: HashSet<Entity> = HashSet::new();
        let mut fired_regions: HashSet<Entity> = HashSet::new();

        // Leaves-first: attempt to fire along each active branch (one per parallel region)
        for &leaf in current.active_leaves.iter() {
            let region_root =
                find_parallel_region_root(leaf, &q_substate_of, &q_initial_state, &q_children);
            if fired_regions.contains(&region_root) {
                continue;
            }

            if try_fire_first_matching_edge_on_branch(
                leaf,
                event,
                machine_root,
                &q_transitions,
                &q_listener,
                &q_edge_target,
                &q_guards,
                &q_substate_of,
                &mut q_defer,
                &q_sm,
                &q_delay,
                &mut q_timer,
                &mut visited,
                &mut commands,
            ) {
                fired_regions.insert(region_root);
            }
        }

        // If no branch consumed the event, fall back to root-level transitions
        if fired_regions.is_empty() {
            let _ = try_fire_first_matching_edge(
                machine_root,
                event,
                &q_transitions,
                &q_listener,
                &q_edge_target,
                &q_guards,
                &q_substate_of,
                &mut q_defer,
                &q_sm,
                &q_delay,
                &mut q_timer,
                &mut commands,
            );
        }
        return;
    }

    // Otherwise, evaluate on the targeted state directly
    try_fire_first_matching_edge(
        machine_root,
        event,
        &q_transitions,
        &q_listener,
        &q_edge_target,
        &q_guards,
        &q_substate_of,
        &mut q_defer,
        &q_sm,
        &q_delay,
        &mut q_timer,
        &mut commands,
    );
}

fn try_fire_first_matching_edge<E: TransitionEvent + Clone>(
    source: Entity,
    event: &E,
    q_transitions: &Query<&Transitions>,
    q_listener: &Query<&EventEdge<E>>,
    q_edge_target: &Query<&Target>,
    q_guards: &Query<&Guards>,
    q_substate_of: &Query<&SubstateOf>,
    q_defer: &mut Query<&mut DeferEvent<E>>,
    q_sm: &Query<&StateMachine>,
    q_delay: &Query<&Delay>,
    q_timer: &mut Query<&mut EdgeTimer>,
    commands: &mut Commands,
) -> bool {
    try_fire_first_matching_edge_generic(
        source,
        event,
        q_transitions,
        q_listener,
        q_edge_target,
        q_guards,
        q_substate_of,
        q_defer,
        q_sm,
        q_delay,
        q_timer,
        commands,
    )
}

fn try_fire_first_matching_edge_on_branch<E: EntityEvent + Clone + TransitionEvent>(
    start: Entity,
    event: &E,
    machine_root: Entity,
    q_transitions: &Query<&Transitions>,
    q_listener: &Query<&EventEdge<E>>,
    q_edge_target: &Query<&Target>,
    q_guards: &Query<&Guards>,
    q_substate_of: &Query<&SubstateOf>,
    q_defer: &mut Query<&mut DeferEvent<E>>,
    q_sm: &Query<&StateMachine>,
    q_delay: &Query<&Delay>,
    q_timer: &mut Query<&mut EdgeTimer>,
    visited: &mut HashSet<Entity>,
    commands: &mut Commands,
) -> bool {
    // Walk from leaf up to (but not beyond) the machine root
    let mut current = Some(start);
    while let Some(state) = current {
        // Skip states already checked across other branches
        if !visited.insert(state) {
            if state == machine_root {
                break;
            }
            current = q_substate_of.get(state).ok().map(|rel| rel.0);
            continue;
        }
        if try_fire_first_matching_edge(
            state,
            event,
            q_transitions,
            q_listener,
            q_edge_target,
            q_guards,
            q_substate_of,
            q_defer,
            q_sm,
            q_delay,
            q_timer,
            commands,
        ) {
            return true;
        }
        if state == machine_root {
            break;
        }
        current = q_substate_of.get(state).ok().map(|rel| rel.0);
    }
    false
}

/// When guards on an Always edge change while its source state is active, re-check and fire if now allowed.
pub fn check_always_on_guards_changed(
    q_guards_changed: Query<
        (Entity, &Guards, &Source, Has<Target>),
        (Changed<Guards>, With<AlwaysEdge>),
    >,
    q_transitions: Query<&Transitions>,
    q_substate_of: Query<&SubstateOf>,
    q_sm: Query<&StateMachine>,
    q_delay: Query<&Delay>,
    mut commands: Commands,
) {
    for (edge, guards, source, edge_target) in q_guards_changed.iter() {
        let source = source.0;

        let root = q_substate_of.root_ancestor(source);
        let Ok(sm) = q_sm.get(root) else {
            continue;
        };
        if !sm.is_active(&source) {
            continue;
        }

        // Only consider Always edges whose guard set changed to passing
        if !guards.check() {
            continue;
        }

        // Ensure this edge is actually listed on the source's transitions (priority set)
        let Ok(transitions) = q_transitions.get(source) else {
            continue;
        };
        if !transitions.into_iter().any(|&e| e == edge) {
            continue;
        }

        // Ensure edge has a valid target; then fire (or arm timer if delayed)
        if !edge_target {
            continue;
        }
        let root = q_substate_of.root_ancestor(source);
        if q_delay.get(edge).is_ok() {
            let after = q_delay.get(edge).unwrap();
            commands
                .entity(edge)
                .insert(EdgeTimer(Timer::new(after.duration, TimerMode::Once)));
        } else {
            commands.trigger(Transition {
                machine: root,
                source,
                edge,
                payload: (),
            });
        }
    }
}

/// On EnterState(source), start timers for any After edges.
pub fn start_after_on_enter(
    enter_state: On<EnterState>,
    q_transitions: Query<&Transitions>,
    q_delay: Query<&Delay>,
    q_always: Query<(), With<AlwaysEdge>>,
    mut commands: Commands,
) {
    let source = enter_state.target;
    let Ok(transitions) = q_transitions.get(source) else {
        return;
    };
    for edge in transitions.into_iter().copied() {
        if q_delay.get(edge).is_ok() && q_always.get(edge).is_ok() {
            let after = q_delay.get(edge).unwrap();
            commands
                .entity(edge)
                .insert(EdgeTimer(Timer::new(after.duration, TimerMode::Once)));
        }
    }
}

/// On ExitState(source), cancel timers for any After edges.
pub fn cancel_after_on_exit(
    exit_state: On<crate::ExitState>,
    q_transitions: Query<&Transitions>,
    q_delay: Query<&Delay>,
    mut commands: Commands,
) {
    let source = exit_state.target;
    let Ok(transitions) = q_transitions.get(source) else {
        return;
    };
    for edge in transitions.into_iter().copied() {
        if q_delay.get(edge).is_ok() {
            commands.entity(edge).try_remove::<EdgeTimer>();
        }
    }
}

/// During TransitionActions, if an edge has ResetEdge, emit ResetSubtree for its scope
pub(crate) fn reset_on_transition_actions(
    transition_action: On<crate::EdgeTraversed>,
    q_reset_edge: Query<&ResetEdge>,
    q_edge: Query<(&Source, &Target)>,
    q_children: Query<&crate::Substates>,
    mut commands: Commands,
) {
    let edge = transition_action.target;
    let Ok(reset) = q_reset_edge.get(edge) else {
        return;
    };

    let Ok((Source(source), Target(target))) = q_edge.get(edge) else {
        return;
    };

    let mut entities = vec![];

    match reset.0 {
        ResetScope::Source => {
            entities.push(*source);
            entities.extend(q_children.iter_descendants(*source));
        }
        ResetScope::Target => {
            entities.push(*target);
            entities.extend(q_children.iter_descendants(*target));
        }
        ResetScope::Both => {
            entities.push(*source);
            entities.push(*target);
            entities.extend(q_children.iter_descendants(*source));
            entities.extend(q_children.iter_descendants(*target));
        }
    }

    for entity in entities {
        commands.trigger(Reset::new(entity));
    }
}

/// Tick After timers and fire the first due transition per active source, respecting Transitions order.
pub fn tick_after_system(
    time: Res<Time>,
    q_transitions: Query<(Entity, &Transitions)>,
    mut q_timer: Query<&mut EdgeTimer>,
    q_delay: Query<&Delay>,
    q_always: Query<(), With<AlwaysEdge>>,
    q_guards: Query<&Guards>,
    q_edge_target: Query<&Target>,
    q_substate_of: Query<&SubstateOf>,
    q_sm: Query<&StateMachine>,
    mut commands: Commands,
) {
    for (source, transitions) in q_transitions.iter() {
        let root = q_substate_of.root_ancestor(source);
        let Ok(sm) = q_sm.get(root) else {
            continue;
        };
        if !sm.is_active(&source) {
            continue;
        }
        // Walk edges in priority order; fire first eligible
        for edge in transitions.into_iter().copied() {
            if q_delay.get(edge).is_err() {
                continue;
            }
            if q_always.get(edge).is_err() {
                continue;
            }
            let Ok(mut timer) = q_timer.get_mut(edge) else {
                continue;
            };
            timer.0.tick(time.delta());
            if !timer.0.just_finished() {
                continue;
            }

            // Validate edge (guards and target) before firing
            if !validate_edge_basic(edge, &q_guards, &q_edge_target, &q_substate_of) {
                // Cancel invalid timer
                commands.entity(edge).try_remove::<EdgeTimer>();
                continue;
            }

            // Cancel timer to avoid multiple firings if state persists
            commands.entity(edge).try_remove::<EdgeTimer>();

            // Emit transition to the machine root with empty payload
            let root = q_substate_of.root_ancestor(source);
            commands.trigger(Transition {
                machine: root,
                source,
                edge,
                payload: (),
            });
            break; // only one delayed transition per source per frame
        }
    }
}

/// Generic system to replay deferred event when a state exits.
pub fn replay_deferred_event<E: EntityEvent + Clone>(
    exit_state: On<ExitState>,
    mut q_defer: Query<&mut DeferEvent<E>>,
    mut commands: Commands,
) where
    for<'a> <E as Event>::Trigger<'a>: Default,
{
    let exited_state = exit_state.target;

    if let Ok(mut defer_event) = q_defer.get_mut(exited_state) {
        if let Some(deferred) = defer_event.take_deferred() {
            commands.trigger(deferred);
        }
    }
}

/// Timer system for event edges with After; fire when due
pub fn tick_after_event_timers<E: TransitionEvent + Clone + 'static>(
    time: Res<Time>,
    mut q_timer: Query<(Entity, &mut EdgeTimer, &PendingEvent<E>, &EventEdge<E>)>,
    q_delay: Query<&Delay>,
    q_guards: Query<&Guards>,
    q_edge_target: Query<&Target>,
    q_edge_source: Query<&Source>,
    q_substate_of: Query<&SubstateOf>,
    q_sm: Query<&StateMachine>,
    mut commands: Commands,
) {
    for (edge, mut timer, pending, listener) in q_timer.iter_mut() {
        // Only consider edges that still have After
        if q_delay.get(edge).is_err() {
            continue;
        }

        // If the source is no longer active, cancel the pending event
        let Ok(Source(source)) = q_edge_source.get(edge) else {
            continue;
        };
        let root = q_substate_of.root_ancestor(*source);
        if let Ok(sm) = q_sm.get(root) {
            if !sm.is_active(source) {
                cleanup_edge_timer_and_pending::<E>(&mut commands, edge);
                continue;
            }
        } else {
            cleanup_edge_timer_and_pending::<E>(&mut commands, edge);
            continue;
        }

        timer.0.tick(time.delta());
        if !timer.0.just_finished() {
            continue;
        }

        // Validate edge (guards and target) before firing
        if !validate_edge_basic(edge, &q_guards, &q_edge_target, &q_substate_of) {
            // Cancel invalid timer/pending
            cleanup_edge_timer_and_pending::<E>(&mut commands, edge);
            continue;
        }

        // If validator present on the edge, require match against pending event
        if let Some(v) = &listener.validator {
            if !v.matches(&pending.event) {
                cleanup_edge_timer_and_pending::<E>(&mut commands, edge);
                continue;
            }
        }

        let target = q_edge_target.get(edge).ok().map(|t| t.0).unwrap_or(*source);
        let payload = PhaseEvents {
            exit: pending.event.to_exit_event(*source, target, edge),
            edge: pending.event.to_edge_event(edge),
            entry: pending.event.to_entry_event(target, *source, edge),
        };

        // Cleanup timer/pending and fire the transition to machine root
        cleanup_edge_timer_and_pending::<E>(&mut commands, edge);
        let root = q_substate_of.root_ancestor(*source);
        commands.trigger(Transition {
            machine: root,
            source: *source,
            edge,
            payload,
        });
    }
}

/// Cancel a pending delayed event for a source when it exits
pub fn cancel_pending_event_on_exit<E: EntityEvent + Clone + 'static>(
    exit_state: On<ExitState>,
    q_transitions: Query<&Transitions>,
    q_pending: Query<&PendingEvent<E>>,
    mut commands: Commands,
) {
    let source = exit_state.target;
    let Ok(transitions) = q_transitions.get(source) else {
        return;
    };
    for &edge in transitions.into_iter() {
        if q_pending.get(edge).is_ok() {
            cleanup_edge_timer_and_pending::<E>(&mut commands, edge);
        }
    }
}
