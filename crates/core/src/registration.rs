use std::any::TypeId;

use bevy::ecs::component::Mutable;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;

use crate::messages::*;
use crate::parameters::*;
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
pub struct InstalledFloatParams(pub HashSet<TypeId>);

#[derive(Resource, Default)]
pub struct InstalledIntParams(pub HashSet<TypeId>);

#[derive(Resource, Default)]
pub struct InstalledBoolParams(pub HashSet<TypeId>);

#[derive(Resource, Default)]
pub struct InstalledFloatParamBindings(pub HashSet<(TypeId, TypeId)>);

#[derive(Resource, Default)]
pub struct InstalledIntParamBindings(pub HashSet<(TypeId, TypeId)>);

#[derive(Resource, Default)]
pub struct InstalledBoolParamBindings(pub HashSet<(TypeId, TypeId)>);

#[derive(Resource, Default)]
pub struct InstalledStateBridges(pub HashSet<TypeId>);

// ---------------------------------------------------------------------------
// RegistrationAppExt
// ---------------------------------------------------------------------------

/// Extension trait for registering message-driven transitions, state
/// components, parameters, and bridges with the gearbox schedule.
pub trait RegistrationAppExt {
    fn register_transition<M: GearboxMessage>(&mut self) -> &mut Self;
    fn register_side_effect<M: GearboxMessage, S: SideEffect<M>>(&mut self) -> &mut Self;
    fn register_state_component<T: Component<Mutability = Mutable> + Clone + 'static>(&mut self) -> &mut Self;
    fn register_float_param<P: Send + Sync + 'static>(&mut self) -> &mut Self;
    fn register_int_param<P: Send + Sync + 'static>(&mut self) -> &mut Self;
    fn register_bool_param<P: Send + Sync + 'static>(&mut self) -> &mut Self;
    fn register_float_param_binding<T: Component + 'static, P: FloatParamBinding<T> + Send + Sync + 'static>(&mut self) -> &mut Self;
    fn register_int_param_binding<T: Component + 'static, P: IntParamBinding<T> + Send + Sync + 'static>(&mut self) -> &mut Self;
    fn register_state_bridge<S: States + bevy::state::state::FreelyMutableState + Default + Component + Clone + 'static>(&mut self) -> &mut Self;
    fn run_auto_installers(&mut self);
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
        self.add_systems(Update, message_edge_listener::<M>.before(GearboxSet));
        self
    }

    fn register_side_effect<M: GearboxMessage, S: SideEffect<M>>(&mut self) -> &mut Self {
        self.add_message::<S>();
        self.add_systems(
            Update,
            produce_side_effects::<M, S>
                .after(message_edge_listener::<M>)
                .before(GearboxSet),
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

    fn register_float_param<P: Send + Sync + 'static>(&mut self) -> &mut Self {
        if dedup!(self, InstalledFloatParams, TypeId::of::<P>()) { return self; }
        self.add_observer(init_float_param_guard_on_add::<P>);
        self.add_systems(PreUpdate, apply_float_param_guards::<P>);
        self
    }

    fn register_int_param<P: Send + Sync + 'static>(&mut self) -> &mut Self {
        if dedup!(self, InstalledIntParams, TypeId::of::<P>()) { return self; }
        self.add_observer(init_int_param_guard_on_add::<P>);
        self.add_systems(PreUpdate, apply_int_param_guards::<P>);
        self
    }

    fn register_bool_param<P: Send + Sync + 'static>(&mut self) -> &mut Self {
        if dedup!(self, InstalledBoolParams, TypeId::of::<P>()) { return self; }
        self.add_observer(init_bool_param_guard_on_add::<P>);
        self.add_systems(PreUpdate, apply_bool_param_guards::<P>);
        self
    }

    fn register_float_param_binding<T: Component + 'static, P: FloatParamBinding<T> + Send + Sync + 'static>(&mut self) -> &mut Self {
        if dedup!(self, InstalledFloatParamBindings, (TypeId::of::<T>(), TypeId::of::<P>())) { return self; }
        self.add_systems(PreUpdate, sync_float_param::<T, P>.before(apply_float_param_guards::<P>));
        self
    }

    fn register_int_param_binding<T: Component + 'static, P: IntParamBinding<T> + Send + Sync + 'static>(&mut self) -> &mut Self {
        if dedup!(self, InstalledIntParamBindings, (TypeId::of::<T>(), TypeId::of::<P>())) { return self; }
        self.add_systems(PreUpdate, sync_int_param::<T, P>.before(apply_int_param_guards::<P>));
        self
    }

    fn register_state_bridge<S: States + bevy::state::state::FreelyMutableState + Default + Component + Clone + 'static>(&mut self) -> &mut Self {
        if dedup!(self, InstalledStateBridges, TypeId::of::<S>()) { return self; }
        self.add_systems(Update, bridge_to_bevy_state::<S>.after(GearboxSet));
        self
    }

    fn run_auto_installers(&mut self) {
        for i in inventory::iter::<TransitionInstaller> { (i.install)(self); }
        for i in inventory::iter::<StateInstaller> { (i.install)(self); }
        for i in inventory::iter::<FloatParamInstaller> { (i.install)(self); }
        for i in inventory::iter::<IntParamInstaller> { (i.install)(self); }
        for i in inventory::iter::<BoolParamInstaller> { (i.install)(self); }
        for i in inventory::iter::<FloatParamBindingInstaller> { (i.install)(self); }
        for i in inventory::iter::<IntParamBindingInstaller> { (i.install)(self); }
        for i in inventory::iter::<BoolParamBindingInstaller> { (i.install)(self); }
        for i in inventory::iter::<StateBridgeInstaller> { (i.install)(self); }
        for i in inventory::iter::<SideEffectInstaller> { (i.install)(self); }
    }
}

/// Function-style plugin to run inventory-based auto-registrations.
pub fn gearbox_auto_register_plugin(app: &mut App) {
    app.run_auto_installers();
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

pub struct FloatParamInstaller { pub install: fn(&mut App) }
inventory::collect!(FloatParamInstaller);

pub struct IntParamInstaller { pub install: fn(&mut App) }
inventory::collect!(IntParamInstaller);

pub struct BoolParamInstaller { pub install: fn(&mut App) }
inventory::collect!(BoolParamInstaller);

pub struct FloatParamBindingInstaller { pub install: fn(&mut App) }
inventory::collect!(FloatParamBindingInstaller);

pub struct IntParamBindingInstaller { pub install: fn(&mut App) }
inventory::collect!(IntParamBindingInstaller);

pub struct BoolParamBindingInstaller { pub install: fn(&mut App) }
inventory::collect!(BoolParamBindingInstaller);

pub struct StateBridgeInstaller { pub install: fn(&mut App) }
inventory::collect!(StateBridgeInstaller);

pub struct SideEffectInstaller { pub install: fn(&mut App) }
inventory::collect!(SideEffectInstaller);

// ---------------------------------------------------------------------------
// Standalone register_* wrappers (used as fn pointers by inventory macros)
// ---------------------------------------------------------------------------

pub fn register_transition<M: GearboxMessage>(app: &mut App) {
    app.register_transition::<M>();
}

pub fn register_state_component<T: Component<Mutability = Mutable> + Clone + 'static>(app: &mut App) {
    app.register_state_component::<T>();
}

pub fn register_float_param<P: Send + Sync + 'static>(app: &mut App) {
    app.register_float_param::<P>();
}

pub fn register_int_param<P: Send + Sync + 'static>(app: &mut App) {
    app.register_int_param::<P>();
}

pub fn register_bool_param<P: Send + Sync + 'static>(app: &mut App) {
    app.register_bool_param::<P>();
}

pub fn register_float_param_binding<T: Component + 'static, P: FloatParamBinding<T> + Send + Sync + 'static>(app: &mut App) {
    app.register_float_param_binding::<T, P>();
}

pub fn register_int_param_binding<T: Component + 'static, P: IntParamBinding<T> + Send + Sync + 'static>(app: &mut App) {
    app.register_int_param_binding::<T, P>();
}

pub fn register_state_bridge<S: States + bevy::state::state::FreelyMutableState + Default + Component + Clone + 'static>(app: &mut App) {
    app.register_state_bridge::<S>();
}

pub fn register_side_effect<M: GearboxMessage, S: SideEffect<M>>(app: &mut App) {
    app.register_side_effect::<M, S>();
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
