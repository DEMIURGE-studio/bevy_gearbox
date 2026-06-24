use std::time::Duration;

use bevy::platform::collections::HashSet;
use bevy::prelude::*;

/// Marks an entity as a state machine root and tracks active states.
#[derive(Component, Default, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct StateMachine {
    pub active: HashSet<Entity>,
    pub active_leaves: HashSet<Entity>,
}

impl StateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self, entity: &Entity) -> bool {
        self.active.contains(entity)
    }
}

/// Marker inserted on state entities that are currently active.
///
/// Use `Added<Active>` to detect newly entered states and
/// `RemovedComponents<Active>` to detect exits.
#[derive(Component, Debug, Clone, Copy)]
pub struct Active {
    /// The state machine root entity this state belongs to.
    pub machine: Entity,
}

/// Which child state to enter by default when a parent state is entered.
#[derive(Component, FromTemplate)]
pub struct InitialState(pub Entity);

/// Relationship: this state is a substate of another.
#[derive(Component, Clone, PartialEq, Eq, Debug, Reflect, FromTemplate)]
#[reflect(Component)]
#[relationship(relationship_target = Substates)]
pub struct SubstateOf(#[entities] pub Entity);

impl FromWorld for SubstateOf {
    fn from_world(_world: &mut World) -> Self {
        SubstateOf(Entity::PLACEHOLDER)
    }
}

/// Relationship target: children substates.
#[derive(Component, Default, Debug, PartialEq, Eq)]
#[relationship_target(relationship = SubstateOf, linked_spawn)]
pub struct Substates(Vec<Entity>);

impl<'a> IntoIterator for &'a Substates {
    type Item = &'a Entity;
    type IntoIter = std::slice::Iter<'a, Entity>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Source state of a transition edge.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[relationship(relationship_target = Transitions)]
pub struct Source(#[entities] pub Entity);

impl FromWorld for Source {
    fn from_world(_world: &mut World) -> Self {
        Source(Entity::PLACEHOLDER)
    }
}

/// Outbound edges from a state.
#[derive(Component, Default, Debug, PartialEq, Eq)]
#[relationship_target(relationship = Source, linked_spawn)]
pub struct Transitions(Vec<Entity>);

impl<'a> IntoIterator for &'a Transitions {
    type Item = &'a Entity;
    type IntoIter = std::slice::Iter<'a, Entity>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Target state of a transition edge.
#[derive(Component, FromTemplate)]
pub struct Target(pub Entity);

/// Marker: this edge fires automatically when its source is active.
#[derive(Component, Default, Clone, Reflect)]
#[reflect(Component)]
pub struct AlwaysEdge;

/// Whether a transition is External (default, exits/re-enters the LCA) or
/// Internal (stays within the source state, no exit/re-enter of the LCA).
#[derive(Component, Default, Clone, Copy, Debug, Reflect, FromTemplate)]
#[reflect(Component)]
pub enum EdgeKind {
    #[default]
    External,
    Internal,
}

/// Delayed transition: fire after `duration` elapses while the source is active.
#[derive(Component, Default, Clone)]
pub struct Delay {
    pub duration: Duration,
}

impl Delay {
    pub fn new(duration: Duration) -> Self {
        Self { duration }
    }

    pub fn from_secs_f32(secs: f32) -> Self {
        Self {
            duration: Duration::from_secs_f32(secs),
        }
    }
}

/// Active timer for a delayed edge. Created when the source state is entered,
/// removed when exited. Ticked in [`Update`] after [`GearboxSet`].
#[derive(Component)]
pub struct EdgeTimer(pub Timer);

/// Marks a state as terminal (XState "final state"). When entered, a [`Done`]
/// message is emitted targeting the parent state (via [`SubstateOf`]). The
/// parent can then transition out via a `MessageEdge<Done>`.
#[derive(Component, Default, Clone, Reflect)]
#[reflect(Component)]
pub struct TerminalState;

/// A single arm of a [`BranchTransition`]. Each arm has a target state
/// and an associated entity for condition data.
#[derive(Clone, Debug)]
pub struct BranchArm {
    /// The destination state if this arm is taken.
    pub target: Entity,
    /// An entity associated with this arm (for condition components, etc.).
    pub guard: Entity,
}

/// Replaces the single [`Target`] on an edge with conditional branching.
/// Arms are evaluated in order; the first with passing guards wins.
/// If no arm passes, the `otherwise` target is used.
///
/// Works with both [`AlwaysEdge`] and [`MessageEdge`].
#[derive(Component, Debug)]
pub struct BranchTransition {
    pub arms: Vec<BranchArm>,
    pub otherwise: Entity,
}

/// Marker to request reset of subtree(s) when an edge fires.
#[derive(Component, Default)]
pub struct ResetEdge(pub ResetScope);

/// Which side of the transition to reset.
#[derive(Default, Clone, Copy, Debug)]
pub enum ResetScope {
    #[default]
    Source,
    Target,
    Both,
}
