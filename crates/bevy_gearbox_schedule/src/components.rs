use std::time::Duration;

use bevy::platform::collections::HashSet;
use bevy::prelude::*;

/// Marks an entity as a state machine root and tracks active states.
#[derive(Component, Default, Debug)]
pub struct Machine {
    pub active: HashSet<Entity>,
    pub active_leaves: HashSet<Entity>,
}

impl Machine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self, entity: &Entity) -> bool {
        self.active.contains(entity)
    }
}

/// Which child state to enter by default when a parent state is entered.
#[derive(Component)]
pub struct InitialState(pub Entity);

/// Relationship: this state is a substate of another.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
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
#[derive(Component)]
pub struct Target(pub Entity);

/// Marker: this edge fires automatically when its source is active.
#[derive(Component)]
pub struct AlwaysEdge;

/// Whether a transition is External (default, exits/re-enters the LCA) or
/// Internal (stays within the source state, no exit/re-enter of the LCA).
#[derive(Component, Default, Clone, Copy, Debug)]
pub enum EdgeKind {
    #[default]
    External,
    Internal,
}

/// Guard conditions that block a transition. Edge fires only when empty.
#[derive(Component, Default, Debug)]
pub struct Guards {
    pub guards: HashSet<String>,
}

impl Guards {
    pub fn new() -> Self {
        Self { guards: HashSet::new() }
    }

    pub fn init(guards: impl IntoIterator<Item = impl Guard>) -> Self {
        Self {
            guards: guards.into_iter().map(|g| g.name()).collect(),
        }
    }

    pub fn check(&self) -> bool {
        self.guards.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        self.guards.is_empty()
    }

    pub fn has_guard(&self, guard: impl Guard) -> bool {
        self.guards.contains(&guard.name())
    }

    pub fn add(&mut self, guard: impl Into<String>) {
        self.guards.insert(guard.into());
    }

    pub fn add_guard(&mut self, guard: impl Guard) {
        self.guards.insert(guard.name());
    }

    pub fn remove(&mut self, guard: &str) {
        self.guards.remove(guard);
    }

    pub fn remove_guard(&mut self, guard: impl Guard) {
        self.guards.remove(&guard.name());
    }
}

/// Trait for types that can identify a guard by name.
pub trait Guard {
    fn name(&self) -> String;
}

impl Guard for String {
    fn name(&self) -> String { self.clone() }
}

impl Guard for &str {
    fn name(&self) -> String { self.to_string() }
}

/// A component that acts as a guard provider. When inserted on a transition
/// edge, it manages a named guard in the [`Guards`] set.
pub trait GuardProvider: Bundle {
    fn guard_name() -> &'static str;
}

/// Delayed transition: fire after `duration` elapses while the source is active.
#[derive(Component)]
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
