use std::time::Duration;

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::prelude::*;

use crate::components::*;
use crate::messages::{GearboxMessage, MessageEdge};

/// Extension trait to write [`Message`] types from [`Commands`].
pub trait WriteMessageExt {
    fn write_message<M: Message + Send + 'static>(&mut self, msg: M);
}

impl WriteMessageExt for Commands<'_, '_> {
    fn write_message<M: Message + Send + 'static>(&mut self, msg: M) {
        self.queue(move |world: &mut World| {
            world.write_message(msg);
        });
    }
}

/// Extension trait for spawning substates with less boilerplate.
pub trait SpawnSubstate {
    type Out<'a>
    where
        Self: 'a;
    fn spawn_substate<B: Bundle>(&mut self, parent: Entity, bundle: B) -> Self::Out<'_>;
}

impl SpawnSubstate for Commands<'_, '_> {
    type Out<'a>
        = EntityCommands<'a>
    where
        Self: 'a;
    fn spawn_substate<B: Bundle>(&mut self, parent: Entity, bundle: B) -> EntityCommands<'_> {
        self.spawn((SubstateOf(parent), bundle))
    }
}

impl SpawnSubstate for ChildSpawnerCommands<'_> {
    type Out<'a>
        = EntityCommands<'a>
    where
        Self: 'a;
    fn spawn_substate<B: Bundle>(&mut self, parent: Entity, bundle: B) -> EntityCommands<'_> {
        self.spawn((SubstateOf(parent), bundle))
    }
}

/// Extension trait for spawning transitions.
pub trait SpawnTransition {
    type Out<'a>
    where
        Self: 'a;
    fn spawn_transition<M: GearboxMessage>(
        &mut self,
        source: Entity,
        target: Entity,
    ) -> Self::Out<'_>;
    fn spawn_transition_always(&mut self, source: Entity, target: Entity) -> Self::Out<'_>;
}

impl SpawnTransition for Commands<'_, '_> {
    type Out<'a>
        = EntityCommands<'a>
    where
        Self: 'a;

    fn spawn_transition<M: GearboxMessage>(
        &mut self,
        source: Entity,
        target: Entity,
    ) -> EntityCommands<'_> {
        self.spawn((Source(source), Target(target), MessageEdge::<M>::default()))
    }

    fn spawn_transition_always(&mut self, source: Entity, target: Entity) -> EntityCommands<'_> {
        self.spawn((Source(source), Target(target), AlwaysEdge))
    }
}

impl SpawnTransition for ChildSpawnerCommands<'_> {
    type Out<'a>
        = EntityCommands<'a>
    where
        Self: 'a;

    fn spawn_transition<M: GearboxMessage>(
        &mut self,
        source: Entity,
        target: Entity,
    ) -> EntityCommands<'_> {
        self.spawn((Source(source), Target(target), MessageEdge::<M>::default()))
    }

    fn spawn_transition_always(&mut self, source: Entity, target: Entity) -> EntityCommands<'_> {
        self.spawn((Source(source), Target(target), AlwaysEdge))
    }
}

/// Builder for guarded always-transitions.
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

    pub fn init_guard<G: GuardProvider>(&mut self, guard: G) -> &mut Self {
        self.guards.push(G::guard_name().to_string());
        self.deferred.push(Box::new(move |ec| {
            ec.insert(guard);
        }));
        self
    }

    pub fn insert<B: Bundle>(&mut self, bundle: B) -> &mut Self {
        self.deferred.push(Box::new(move |ec| {
            ec.insert(bundle);
        }));
        self
    }

    pub fn with_delay(&mut self, duration: Duration) -> &mut Self {
        self.deferred.push(Box::new(move |ec| {
            ec.insert(Delay { duration });
        }));
        self
    }

    pub fn with_name(&mut self, name: impl Into<String> + Send + Sync + 'static) -> &mut Self {
        self.deferred.push(Box::new(move |ec| {
            ec.insert(Name::new(name.into()));
        }));
        self
    }
}

/// Extension trait for building guarded always-transitions.
pub trait BuildTransition {
    type Out<'a>
    where
        Self: 'a;
    fn build_transition_always(
        &mut self,
        source: Entity,
        target: Entity,
        configure: impl FnOnce(&mut TransitionBuilder),
    ) -> Self::Out<'_>;
}

impl BuildTransition for Commands<'_, '_> {
    type Out<'a>
        = EntityCommands<'a>
    where
        Self: 'a;

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
            self.spawn((
                Source(source),
                Target(target),
                AlwaysEdge,
                Guards::init(builder.guards),
            ))
        };
        for f in builder.deferred {
            f(&mut ec);
        }
        ec
    }
}

impl BuildTransition for ChildSpawnerCommands<'_> {
    type Out<'a>
        = EntityCommands<'a>
    where
        Self: 'a;

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
            self.spawn((
                Source(source),
                Target(target),
                AlwaysEdge,
                Guards::init(builder.guards),
            ))
        };
        for f in builder.deferred {
            f(&mut ec);
        }
        ec
    }
}

/// Extension methods for transition entities.
pub trait TransitionExt {
    fn with_delay(&mut self, duration: Duration) -> &mut Self;
    fn with_name(&mut self, name: impl Into<String>) -> &mut Self;
}

impl TransitionExt for EntityCommands<'_> {
    fn with_delay(&mut self, duration: Duration) -> &mut Self {
        self.insert(Delay { duration })
    }
    fn with_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.insert(Name::new(name.into()))
    }
}

/// Extension for initializing a state machine on an existing entity.
pub trait InitStateMachine {
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
