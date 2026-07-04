use std::any::TypeId;

use bevy::ecs::component::Mutable;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;

use crate::messages::*;
use crate::state_component::*;
use crate::{GearboxPhase, GearboxSchedule, GearboxSet};

// ---------------------------------------------------------------------------
// Dedup resources
// ---------------------------------------------------------------------------

/// Deduplication resource for registered message types.
#[derive(Resource, Default)]
pub struct InstalledTransitions(pub HashSet<TypeId>);

/// Deduplication resource for registered state components.
#[derive(Resource, Default)]
pub struct InstalledStateComponents(pub HashSet<TypeId>);

#[derive(Resource, Default)]
pub struct InstalledStateBridges(pub HashSet<TypeId>);

// ---------------------------------------------------------------------------
// RegistrationAppExt
// ---------------------------------------------------------------------------

/// Extension trait for registering message-driven transitions, state
/// components, and bridges with the gearbox schedule.
pub trait RegistrationAppExt {
    fn register_transition<M: GearboxMessage>(&mut self) -> &mut Self;
    fn register_state_component<T: Component<Mutability = Mutable> + Clone + 'static>(&mut self) -> &mut Self;
    fn register_state_bridge<S: States + bevy::state::state::FreelyMutableState + Default + Component + Clone + 'static>(&mut self) -> &mut Self;
}

/// Helper macro for dedup boilerplate.
macro_rules! dedup {
    ($app:expr, $res:ty, $key:expr) => {{
        if !$app.world().contains_resource::<$res>() {
            $app.insert_resource(<$res>::default());
        }
        let mut installed = $app.world_mut().resource_mut::<$res>();
        let already = !installed.0.insert($key);
        drop(installed);
        already
    }};
}

impl RegistrationAppExt for App {
    fn register_transition<M: GearboxMessage>(&mut self) -> &mut Self {
        if dedup!(self, InstalledTransitions, TypeId::of::<M>()) { return self; }
        self.add_message::<M>();
        self.add_message::<Matched<M>>();
        self.register_type::<MessageEdge<M>>();
        // Edge-detect listeners run inside the per-frame schedule loop so that
        // messages fired the same frame a machine is spawned are processed
        // after the machine's initial state has been activated.
        self.add_systems(
            GearboxSchedule,
            message_edge_listener::<M>.in_set(GearboxPhase::EdgeDetectPhase),
        );
        self
    }

    fn register_state_component<T: Component<Mutability = Mutable> + Clone + 'static>(&mut self) -> &mut Self {
        if dedup!(self, InstalledStateComponents, TypeId::of::<T>()) { return self; }
        self.add_systems(
            GearboxSchedule,
            (
                state_component_enter::<T>.in_set(GearboxPhase::EntryPhase),
                state_component_exit::<T>.in_set(GearboxPhase::ExitPhase),
                state_inactive_component_enter::<T>.in_set(GearboxPhase::EntryPhase),
                state_inactive_component_exit::<T>.in_set(GearboxPhase::ExitPhase),
            ),
        );
        self
    }

    fn register_state_bridge<S: States + bevy::state::state::FreelyMutableState + Default + Component + Clone + 'static>(&mut self) -> &mut Self {
        if dedup!(self, InstalledStateBridges, TypeId::of::<S>()) { return self; }
        self.add_systems(Update, bridge_to_bevy_state::<S>.after(GearboxSet));
        self
    }
}

/// Run every inventory-submitted registration: derived [`GearboxMessage`]
/// transitions, `#[state_component]` types, and `#[state_bridge]` types.
pub(crate) fn run_auto_installers(app: &mut App) {
    for i in inventory::iter::<TransitionInstaller> { (i.install)(app); }
    for i in inventory::iter::<StateInstaller> { (i.install)(app); }
    for i in inventory::iter::<StateBridgeInstaller> { (i.install)(app); }
}

// ---------------------------------------------------------------------------
// Bevy state bridge
// ---------------------------------------------------------------------------

/// Sync gearbox state into Bevy's `NextState<S>` when a state with component
/// `S` is entered. Runs in [`Update`] after [`GearboxSet`].
pub fn bridge_to_bevy_state<S: States + bevy::state::state::FreelyMutableState + Component + Clone + 'static>(
    q_entered: Query<(Entity, &S), Added<crate::components::Active>>,
    mut next: ResMut<NextState<S>>,
) {
    for (_entity, s) in &q_entered {
        next.set(s.clone());
    }
}

// ---------------------------------------------------------------------------
// Inventory auto-installers
// ---------------------------------------------------------------------------

pub struct TransitionInstaller { pub install: fn(&mut App) }
inventory::collect!(TransitionInstaller);

pub struct StateInstaller { pub install: fn(&mut App) }
inventory::collect!(StateInstaller);

pub struct StateBridgeInstaller { pub install: fn(&mut App) }
inventory::collect!(StateBridgeInstaller);

// ---------------------------------------------------------------------------
// Standalone register_* wrappers (used as fn pointers by inventory macros)
// ---------------------------------------------------------------------------

pub fn register_transition<M: GearboxMessage>(app: &mut App) {
    app.register_transition::<M>();
}

pub fn register_state_component<T: Component<Mutability = Mutable> + Clone + 'static>(app: &mut App) {
    app.register_state_component::<T>();
}

pub fn register_state_bridge<S: States + bevy::state::state::FreelyMutableState + Default + Component + Clone + 'static>(app: &mut App) {
    app.register_state_bridge::<S>();
}

// ---------------------------------------------------------------------------
// DeferEvent
// ---------------------------------------------------------------------------

/// Add to a state to defer messages of type `M` while that state is active.
/// When the state exits, the deferred message is replayed.
#[derive(Component)]
pub struct DeferEvent<M: GearboxMessage> {
    pub deferred: Option<M>,
}

impl<M: GearboxMessage> Default for DeferEvent<M> {
    fn default() -> Self { Self { deferred: None } }
}

impl<M: GearboxMessage> DeferEvent<M> {
    pub fn new() -> Self { Self::default() }

    pub fn defer_event(&mut self, msg: M) {
        self.deferred = Some(msg);
    }

    pub fn take_deferred(&mut self) -> Option<M> {
        self.deferred.take()
    }
}

/// Replay deferred messages when their host state exits.
/// Runs in [`Update`] after [`GearboxSet`] — replayed messages are picked up next frame.
pub fn replay_deferred_messages<M: GearboxMessage>(
    mut removed: RemovedComponents<crate::components::Active>,
    mut q_defer: Query<&mut DeferEvent<M>>,
    mut writer: MessageWriter<M>,
) {
    for state in removed.read() {
        if let Ok(mut defer) = q_defer.get_mut(state) {
            if let Some(msg) = defer.take_deferred() {
                writer.write(msg);
            }
        }
    }
}
