use bevy::prelude::*;
use std::marker::PhantomData;
use crate::{guards::Guards, transitions::Source, SubstateOf};

/// A strongly-typed float parameter stored on an entity.
/// The marker type `P` identifies the parameter (e.g., `Speed`).
#[derive(Component)]
pub struct FloatParam<P> {
    value: f32,
    _marker: PhantomData<P>,
}

impl<P> Default for FloatParam<P> {
    fn default() -> Self { Self { value: 0.0, _marker: PhantomData } }
}

impl<P> FloatParam<P> {
    #[inline]
    pub fn get(&self) -> f32 { self.value }
    #[inline]
    pub fn set(&mut self, value: f32) { self.value = value; }
}

/// A strongly-typed integer parameter.
#[derive(Component)]
pub struct IntParam<P> {
    value: i32,
    _marker: PhantomData<P>,
}

impl<P> Default for IntParam<P> {
    fn default() -> Self { Self { value: 0, _marker: PhantomData } }
}

impl<P> IntParam<P> {
    #[inline]
    pub fn get(&self) -> i32 { self.value }
    #[inline]
    pub fn set(&mut self, value: i32) { self.value = value; }
}

/// A strongly-typed boolean parameter.
#[derive(Component)]
pub struct BoolParam<P> {
    value: bool,
    _marker: PhantomData<P>,
}

impl<P> Default for BoolParam<P> {
    fn default() -> Self { Self { value: false, _marker: PhantomData } }
}

impl<P> BoolParam<P> {
    #[inline]
    pub fn get(&self) -> bool { self.value }
    #[inline]
    pub fn set(&mut self, value: bool) { self.value = value; }
}

/// Implement this on the marker type `P` to bind a source component `T` to a float param.
pub trait FloatParamBinding<T: Component> {
    fn extract(source: &T) -> f32;
}

/// Generic sync system: copies values from source component `T` into `FloatParam<P>`.
pub fn sync_float_param<T, P>(
    mut q: Query<(&T, &mut FloatParam<P>)>,
)
where
    T: Component,
    P: FloatParamBinding<T> + Send + Sync + 'static,
{
    for (src, mut param) in &mut q {
        param.set(P::extract(src));
    }
}

/// Float range condition with optional hysteresis margin.
#[derive(Component, Clone, Copy)]
pub struct FloatInRange<P> {
    pub min: f32,
    pub max: f32,
    pub hysteresis: f32,
    _marker: PhantomData<P>,
}

impl<P> FloatInRange<P> {
    pub fn new(min: f32, max: f32, hysteresis: f32) -> Self {
        Self { min, max, hysteresis, _marker: PhantomData }
    }
}

fn guard_key_for_float<P>() -> String { format!("FloatInRange::<{}>", std::any::type_name::<P>()) }

/// Update Guards on edges with FloatInRange<P> based on the current FloatParam<P> value.
/// Works seamlessly with AlwaysEdge and EventEdge since both consult Guards.
pub fn apply_float_param_guards<P: Send + Sync + 'static>(
    q_edges: Query<(Entity, &Source, &FloatInRange<P>)>,
    q_params: Query<&FloatParam<P>>,
    q_substate_of: Query<&SubstateOf>,
    mut q_guards: Query<&mut Guards>,
    mut commands: Commands,
) {
    let key = guard_key_for_float::<P>();
    for (edge, Source(source), range) in &q_edges {
        let root = q_substate_of.root_ancestor(*source);
        // Determine desired presence of this guard without mutating existing component
        let desired_blocked = match q_params.get(root) {
            Ok(param) => {
                let v = param.get();
                !(v + range.hysteresis >= range.min && v - range.hysteresis <= range.max)
            }
            Err(_) => true, // missing param => block
        };

        // Read current presence (if any) without triggering change detection
        let current_has = q_guards
            .get(edge)
            .ok()
            .map(|g| g.has_guard(key.as_str()))
            .unwrap_or(false);

        // Only mutate when the membership actually changes
        if desired_blocked != current_has {
            if let Ok(mut g) = q_guards.get_mut(edge) {
                if desired_blocked { g.add_guard(key.as_str()); }
                else { g.remove_guard(key.as_str()); }
            } else if desired_blocked {
                // Insert only when we actually need to block
                commands.entity(edge).insert(Guards::init([key.as_str()]));
            }
        }
    }
}

/// Immediately seed a blocking guard when a FloatInRange<P> is added to an edge.
/// This ensures Always edges won't fire before the first guard application pass.
pub fn init_float_param_guard_on_add<P: Send + Sync + 'static>(
    add: On<Add, FloatInRange<P>>,
    mut q_guards: Query<&mut Guards>,
    mut commands: Commands,
){
    let edge = add.event().entity;
    let key = guard_key_for_float::<P>();
    if let Ok(mut g) = q_guards.get_mut(edge) {
        g.add_guard(key.as_str());
    } else {
        commands.entity(edge).insert(Guards::init([key.as_str()]));
    }
}

/// Implement this on the marker type `P` to bind a source component `T` to an int param.
pub trait IntParamBinding<T: Component> {
    fn extract(source: &T) -> i32;
}

/// Generic sync system: copies values from source component `T` into `IntParam<P>`.
pub fn sync_int_param<T, P>(
    mut q: Query<(&T, &mut IntParam<P>)>,
)
where
    T: Component,
    P: IntParamBinding<T> + Send + Sync + 'static,
{
    for (src, mut param) in &mut q {
        param.set(P::extract(src));
    }
}

/// Integer range condition with optional hysteresis margin.
#[derive(Component, Clone, Copy)]
pub struct IntInRange<P> {
    pub min: i32,
    pub max: i32,
    pub hysteresis: i32,
    _marker: PhantomData<P>,
}

impl<P> IntInRange<P> {
    pub fn new(min: i32, max: i32, hysteresis: i32) -> Self {
        Self { min, max, hysteresis, _marker: PhantomData }
    }
}

fn guard_key_for_int<P>() -> String { format!("IntInRange::<{}>", std::any::type_name::<P>()) }

/// Update Guards on edges with IntInRange<P> based on the current IntParam<P> value.
pub fn apply_int_param_guards<P: Send + Sync + 'static>(
    q_edges: Query<(Entity, &Source, &IntInRange<P>)>,
    q_params: Query<&IntParam<P>>,
    q_substate_of: Query<&SubstateOf>,
    mut q_guards: Query<&mut Guards>,
    mut commands: Commands,
){
    let key = guard_key_for_int::<P>();
    for (edge, Source(source), range) in &q_edges {
        let root = q_substate_of.root_ancestor(*source);
        let desired_blocked = match q_params.get(root) {
            Ok(param) => {
                let v = param.get();
                // inclusive range with hysteresis margin
                !((v + range.hysteresis) as i64 >= range.min as i64 && (v - range.hysteresis) as i64 <= range.max as i64)
            }
            Err(_) => true,
        };

        let current_has = q_guards
            .get(edge)
            .ok()
            .map(|g| g.has_guard(key.as_str()))
            .unwrap_or(false);

        if desired_blocked != current_has {
            if let Ok(mut g) = q_guards.get_mut(edge) {
                if desired_blocked { g.add_guard(key.as_str()); }
                else { g.remove_guard(key.as_str()); }
            } else if desired_blocked {
                commands.entity(edge).insert(Guards::init([key.as_str()]));
            }
        }
    }
}

/// Immediately seed a blocking guard when an IntInRange<P> is added to an edge.
pub fn init_int_param_guard_on_add<P: Send + Sync + 'static>(
    add: On<Add, IntInRange<P>>,
    mut q_guards: Query<&mut Guards>,
    mut commands: Commands,
){
    let edge = add.event().entity;
    let key = guard_key_for_int::<P>();
    if let Ok(mut g) = q_guards.get_mut(edge) {
        g.add_guard(key.as_str());
    } else {
        commands.entity(edge).insert(Guards::init([key.as_str()]));
    }
}

/// Implement this on the marker type `P` to bind a source component `T` to a bool param.
pub trait BoolParamBinding<T: Component> {
    fn extract(source: &T) -> bool;
}

/// Generic sync system: copies values from source component `T` into `BoolParam<P>`.
pub fn sync_bool_param<T, P>(
    mut q: Query<(&T, &mut BoolParam<P>)>,
)
where
    T: Component,
    P: BoolParamBinding<T> + Send + Sync + 'static,
{
    for (src, mut param) in &mut q {
        param.set(P::extract(src));
    }
}

/// Boolean equality condition.
#[derive(Component, Clone, Copy)]
pub struct BoolEquals<P> {
    pub expected: bool,
    _marker: PhantomData<P>,
}

impl<P> BoolEquals<P> {
    pub fn new(expected: bool) -> Self { Self { expected, _marker: PhantomData } }
}

fn guard_key_for_bool<P>() -> String { format!("BoolEquals::<{}>", std::any::type_name::<P>()) }

/// Update Guards on edges with BoolEquals<P> based on the current BoolParam<P> value.
pub fn apply_bool_param_guards<P: Send + Sync + 'static>(
    q_edges: Query<(Entity, &Source, &BoolEquals<P>)>,
    q_params: Query<&BoolParam<P>>,
    q_substate_of: Query<&SubstateOf>,
    mut q_guards: Query<&mut Guards>,
    mut commands: Commands,
){
    let key = guard_key_for_bool::<P>();
    for (edge, Source(source), eq) in &q_edges {
        let root = q_substate_of.root_ancestor(*source);
        let desired_blocked = match q_params.get(root) {
            Ok(param) => param.get() != eq.expected,
            Err(_) => true,
        };

        let current_has = q_guards
            .get(edge)
            .ok()
            .map(|g| g.has_guard(key.as_str()))
            .unwrap_or(false);

        if desired_blocked != current_has {
            if let Ok(mut g) = q_guards.get_mut(edge) {
                if desired_blocked { g.add_guard(key.as_str()); }
                else { g.remove_guard(key.as_str()); }
            } else if desired_blocked {
                commands.entity(edge).insert(Guards::init([key.as_str()]));
            }
        }
    }
}

/// Immediately seed a blocking guard when a BoolEquals<P> is added to an edge.
pub fn init_bool_param_guard_on_add<P: Send + Sync + 'static>(
    add: On<Add, BoolEquals<P>>,
    mut q_guards: Query<&mut Guards>,
    mut commands: Commands,
){
    let edge = add.event().entity;
    let key = guard_key_for_bool::<P>();
    if let Ok(mut g) = q_guards.get_mut(edge) {
        g.add_guard(key.as_str());
    } else {
        commands.entity(edge).insert(Guards::init([key.as_str()]));
    }
}