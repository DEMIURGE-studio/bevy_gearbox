use bevy::{prelude::*, state::state::*};
use crate::{EnterState, StateMachine};

/// Bridge a Gearbox chart's EnterState events to Bevy `States`,
/// setting `NextState<S>` when a chart node carrying `S` is entered.
pub trait AppBevyStateBridgeExt {
    fn add_state_bridge<S>(&mut self) -> &mut Self
    where
        S: States + FreelyMutableState + Component + Clone + 'static;
}

impl AppBevyStateBridgeExt for App {
    fn add_state_bridge<S>(&mut self) -> &mut Self
    where
        S: States + FreelyMutableState + Component + Clone + 'static,
    {
        self.add_observer(bridge_chart_to_bevy_state::<S>)
    }
}

fn bridge_chart_to_bevy_state<S: States + FreelyMutableState + Component + Clone + 'static>(
    trigger: Trigger<EnterState>,
    mut next: ResMut<NextState<S>>,
    state_q: Query<&S>,
) {
    let target = trigger.target();
    if let Ok(s) = state_q.get(target) {
        next.set(s.clone());
    }
}

/// Commands helper to emit a transition event to a specific chart root, located by a marker `M`.
pub trait GearboxCommandsExt {
    fn emit_to_chart<M>(&mut self, event: impl Event + Clone + Send + Sync + 'static)
    where
        M: Component + 'static;

    fn emit_to_unique_chart(&mut self, event: impl Event + Clone + Send + Sync + 'static);
}

impl<'w, 's> GearboxCommandsExt for Commands<'w, 's> {
    fn emit_to_chart<M>(&mut self, event: impl Event + Clone + Send + Sync + 'static)
    where
        M: Component + 'static,
    {
        self.queue(move |world: &mut World| {
            let mut q = world.query_filtered::<Entity, With<M>>();
            if let Ok(root) = q.single(world) {
                world.commands().trigger_targets(event, root);
            }
        });
    }

    fn emit_to_unique_chart(&mut self, event: impl Event + Clone + Send + Sync + 'static) {
        self.queue(move |world: &mut World| {
            let mut q = world.query_filtered::<Entity, With<StateMachine>>();
            if let Ok(root) = q.single(world) {
                world.commands().trigger_targets(event, root);
            }
        });
    }
}