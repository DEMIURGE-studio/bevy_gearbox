use bevy::{prelude::*, state::state::FreelyMutableState};
use crate::EnterState;

pub fn bridge_chart_to_bevy_state<S: States + FreelyMutableState + Component + Clone + 'static>(
    enter_state: On<EnterState>,
    mut next: ResMut<NextState<S>>,
    q_state: Query<&S>,
) {
    let target = enter_state.target;
    if let Ok(s) = q_state.get(target) {
        next.set(s.clone());
    }
}

/// Commands helper to emit a transition event to a specific chart root, located by a marker `M`.
pub trait GearboxCommandsExt {
    /// Build and emit an EntityEvent using the resolved chart root `Entity` for marker `M`.
    /// Usage: `commands.emit_to_chart::<AppState>(|root| MyEvent::new(root))`.
    fn emit_to_chart<M>(&mut self, make: impl BuildEntityEvent + Send + 'static)
    where
        M: Component + 'static;
}

impl<'w, 's> GearboxCommandsExt for Commands<'w, 's> {
    fn emit_to_chart<M>(&mut self, make: impl BuildEntityEvent + Send + 'static)
    where
        M: Component + 'static,
    {
        self.queue(move |world: &mut World| {
            let mut q = world.query_filtered::<Entity, With<M>>();
            if let Ok(root) = q.single(world) {
                make.trigger_into_world(world, root);
            }
        });
    }
}

/// Helper trait to infer the event type from the closure and trigger it into the world.
pub trait BuildEntityEvent {
    fn trigger_into_world(self, world: &mut World, root: Entity);
}

impl<F, E> BuildEntityEvent for F
where
    F: FnOnce(Entity) -> E + Send + 'static,
    E: EntityEvent + Clone + Send + Sync + 'static,
    for<'a> <E as Event>::Trigger<'a>: Default,
{
    fn trigger_into_world(self, world: &mut World, root: Entity) {
        let event = self(root);
        world.commands().trigger(event);
    }
}