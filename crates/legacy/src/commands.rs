use std::time::Duration;

use bevy::prelude::*;
use bevy::ecs::hierarchy::{ChildSpawner, ChildSpawnerCommands};
use bevy::ecs::world::EntityWorldMut;

use crate::{
    InitialState, StateMachine, SubstateOf,
    guards::{Guards, GuardProvider},
    transitions::{AlwaysEdge, Delay, EventEdge, Source, Target, TransitionEvent},
};

// ---------------------------------------------------------------------------
// spawn_substate
// ---------------------------------------------------------------------------

/// Extension trait for spawning substates with less boilerplate.
pub trait SpawnSubstate {
    type Out<'a> where Self: 'a;

    /// Spawn a substate of `parent`. Inserts `SubstateOf(parent)` automatically.
    fn spawn_substate<B: Bundle>(&mut self, parent: Entity, bundle: B) -> Self::Out<'_>;
}

impl SpawnSubstate for ChildSpawnerCommands<'_> {
    type Out<'a> = EntityCommands<'a> where Self: 'a;

    fn spawn_substate<B: Bundle>(&mut self, parent: Entity, bundle: B) -> EntityCommands<'_> {
        self.spawn((SubstateOf(parent), bundle))
    }
}

impl SpawnSubstate for ChildSpawner<'_> {
    type Out<'a> = EntityWorldMut<'a> where Self: 'a;

    fn spawn_substate<B: Bundle>(&mut self, parent: Entity, bundle: B) -> EntityWorldMut<'_> {
        self.spawn((SubstateOf(parent), bundle))
    }
}

impl SpawnSubstate for Commands<'_, '_> {
    type Out<'a> = EntityCommands<'a> where Self: 'a;

    fn spawn_substate<B: Bundle>(&mut self, parent: Entity, bundle: B) -> EntityCommands<'_> {
        self.spawn((SubstateOf(parent), bundle))
    }
}

// ---------------------------------------------------------------------------
// spawn_transition / spawn_transition_always
// ---------------------------------------------------------------------------

/// Extension trait for spawning transitions.
///
/// ```ignore
/// // Event edge
/// parent.spawn_transition::<StartInvoke>(ready, invoke);
///
/// // Always edge
/// parent.spawn_transition_always(invoke, ready);
/// ```
pub trait SpawnTransition {
    type Out<'a> where Self: 'a;

    /// Spawn an event-driven transition from `source` to `target`.
    fn spawn_transition<E: TransitionEvent>(&mut self, source: Entity, target: Entity) -> Self::Out<'_>;

    /// Spawn an always-on transition from `source` to `target`.
    fn spawn_transition_always(&mut self, source: Entity, target: Entity) -> Self::Out<'_>;
}

impl SpawnTransition for ChildSpawnerCommands<'_> {
    type Out<'a> = EntityCommands<'a> where Self: 'a;

    fn spawn_transition<E: TransitionEvent>(&mut self, source: Entity, target: Entity) -> EntityCommands<'_> {
        self.spawn((Source(source), Target(target), EventEdge::<E>::default()))
    }

    fn spawn_transition_always(&mut self, source: Entity, target: Entity) -> EntityCommands<'_> {
        self.spawn((Source(source), Target(target), AlwaysEdge))
    }
}

impl SpawnTransition for ChildSpawner<'_> {
    type Out<'a> = EntityWorldMut<'a> where Self: 'a;

    fn spawn_transition<E: TransitionEvent>(&mut self, source: Entity, target: Entity) -> EntityWorldMut<'_> {
        self.spawn((Source(source), Target(target), EventEdge::<E>::default()))
    }

    fn spawn_transition_always(&mut self, source: Entity, target: Entity) -> EntityWorldMut<'_> {
        self.spawn((Source(source), Target(target), AlwaysEdge))
    }
}

impl SpawnTransition for Commands<'_, '_> {
    type Out<'a> = EntityCommands<'a> where Self: 'a;

    fn spawn_transition<E: TransitionEvent>(&mut self, source: Entity, target: Entity) -> EntityCommands<'_> {
        self.spawn((Source(source), Target(target), EventEdge::<E>::default()))
    }

    fn spawn_transition_always(&mut self, source: Entity, target: Entity) -> EntityCommands<'_> {
        self.spawn((Source(source), Target(target), AlwaysEdge))
    }
}

// ---------------------------------------------------------------------------
// build_transition_always — callback builder for guarded transitions
//
// Holdover while we wait for bsn. Will be deprecated once bsn lands and
// state machines can be declared declaratively.
// ---------------------------------------------------------------------------

/// Collects guard names and deferred inserts, then spawns everything at once.
pub struct TransitionBuilder {
    guards: Vec<String>,
    deferred: Vec<Box<dyn FnOnce(&mut EntityCommands) + Send + Sync>>,
}

impl TransitionBuilder {
    fn new() -> Self {
        Self {
            guards: Vec::new(),
            deferred: Vec::new(),
        }
    }

    /// Add a guard provider. Its guard name is collected and the component
    /// will be inserted alongside a merged `Guards` in a single spawn.
    pub fn init_guard<G: GuardProvider>(&mut self, guard: G) -> &mut Self {
        self.guards.push(G::guard_name().to_string());
        self.deferred.push(Box::new(move |ec| { ec.insert(guard); }));
        self
    }

    /// Queue an additional component insert on the transition entity.
    pub fn insert<B: Bundle>(&mut self, bundle: B) -> &mut Self {
        self.deferred.push(Box::new(move |ec| { ec.insert(bundle); }));
        self
    }

    /// Add a delay before the transition fires.
    pub fn with_delay(&mut self, duration: Duration) -> &mut Self {
        self.deferred.push(Box::new(move |ec| { ec.insert(Delay { duration }); }));
        self
    }

    /// Set a name on the transition entity.
    pub fn with_name(&mut self, name: impl Into<String> + Send + Sync + 'static) -> &mut Self {
        self.deferred.push(Box::new(move |ec| { ec.insert(Name::new(name.into())); }));
        self
    }
}

/// Extension trait for building guarded always-transitions with a callback.
///
/// Holdover while we wait for bsn. Will be deprecated once bsn lands.
///
/// ```ignore
/// parent.build_transition_always(entity, done, |t| {
///     t.init_guard(requires! { "ProjectileLife <= 0" })
///      .insert(RequiresStatsOf(entity));
/// });
/// ```
pub trait BuildTransition {
    type Out<'a> where Self: 'a;

    /// Build an always-transition with a callback that configures guards and components.
    fn build_transition_always(
        &mut self,
        source: Entity,
        target: Entity,
        configure: impl FnOnce(&mut TransitionBuilder),
    ) -> Self::Out<'_>;
}

impl BuildTransition for ChildSpawnerCommands<'_> {
    type Out<'a> = EntityCommands<'a> where Self: 'a;

    fn build_transition_always(
        &mut self,
        source: Entity,
        target: Entity,
        configure: impl FnOnce(&mut TransitionBuilder),
    ) -> EntityCommands<'_> {
        let mut builder = TransitionBuilder::new();
        configure(&mut builder);

        let mut ec = if builder.guards.is_empty() {
            self.spawn((Source(source), Target(target), AlwaysEdge))
        } else {
            self.spawn((Source(source), Target(target), AlwaysEdge, Guards::init(builder.guards)))
        };

        for f in builder.deferred {
            f(&mut ec);
        }
        ec
    }
}

// Note: ChildSpawner (RelatedSpawner) operates on &mut World and doesn't have
// a commands() method. The deferred closures expect EntityCommands, so we skip
// this impl. Use ChildSpawnerCommands (via EntityCommands::with_children) instead,
// which is the common path for templates.

impl BuildTransition for Commands<'_, '_> {
    type Out<'a> = EntityCommands<'a> where Self: 'a;

    fn build_transition_always(
        &mut self,
        source: Entity,
        target: Entity,
        configure: impl FnOnce(&mut TransitionBuilder),
    ) -> EntityCommands<'_> {
        let mut builder = TransitionBuilder::new();
        configure(&mut builder);

        let mut ec = if builder.guards.is_empty() {
            self.spawn((Source(source), Target(target), AlwaysEdge))
        } else {
            self.spawn((Source(source), Target(target), AlwaysEdge, Guards::init(builder.guards)))
        };

        for f in builder.deferred {
            f(&mut ec);
        }
        ec
    }
}

// ---------------------------------------------------------------------------
// TransitionExt — simple builder methods (no guard support)
// ---------------------------------------------------------------------------

/// Extension methods for configuring transition entities after spawning.
///
/// For transitions with guards, use [`BuildTransition::build_transition_always`] instead.
pub trait TransitionExt {
    /// Add a delay before the transition fires.
    fn with_delay(&mut self, duration: Duration) -> &mut Self;

    /// Set a name on the transition entity.
    fn with_name(&mut self, name: impl Into<String>) -> &mut Self;
}

impl TransitionExt for EntityWorldMut<'_> {
    fn with_delay(&mut self, duration: Duration) -> &mut Self {
        self.insert(Delay { duration })
    }

    fn with_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.insert(Name::new(name.into()))
    }
}

impl TransitionExt for EntityCommands<'_> {
    fn with_delay(&mut self, duration: Duration) -> &mut Self {
        self.insert(Delay { duration })
    }

    fn with_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.insert(Name::new(name.into()))
    }
}

// ---------------------------------------------------------------------------
// init_state_machine
// ---------------------------------------------------------------------------

/// Extension for initializing a state machine on an existing entity.
pub trait InitStateMachine {
    /// Insert `StateMachine` and optionally `InitialState` on this entity.
    ///
    /// Pass `None` for state machines with parallel zones and no single initial state.
    fn init_state_machine(&mut self, initial_state: impl Into<Option<Entity>>) -> &mut Self;
}

impl InitStateMachine for EntityCommands<'_> {
    fn init_state_machine(&mut self, initial_state: impl Into<Option<Entity>>) -> &mut Self {
        if let Some(state) = initial_state.into() {
            self.insert((StateMachine::new(), InitialState(state)))
        } else {
            self.insert(StateMachine::new())
        }
    }
}

impl InitStateMachine for EntityWorldMut<'_> {
    fn init_state_machine(&mut self, initial_state: impl Into<Option<Entity>>) -> &mut Self {
        if let Some(state) = initial_state.into() {
            self.insert((StateMachine::new(), InitialState(state)))
        } else {
            self.insert(StateMachine::new())
        }
    }
}
