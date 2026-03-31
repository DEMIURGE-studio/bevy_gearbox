use std::marker::PhantomData;

use bevy::platform::collections::HashSet;
use bevy::prelude::*;

use crate::components::*;
use crate::resolve::TransitionMessage;

/// Trait implemented by user message types that can trigger state machine
/// transitions. The schedule version of core's `TransitionEvent`.
///
/// ```rust,ignore
/// #[derive(Message, Clone)]
/// struct Attack {
///     machine: Entity,
///     damage: f32,
/// }
///
/// impl GearboxMessage for Attack {
///     type Validator = AcceptAll;
///     fn machine(&self) -> Entity { self.machine }
/// }
/// ```
pub trait GearboxMessage: Message + Clone + Send + Sync + 'static {
    /// Per-edge validator type. Use [`AcceptAll`] if every edge of this
    /// message type should match unconditionally.
    type Validator: MessageValidator<Self> + Default + Clone + Send + Sync;

    /// Which machine this message is addressed to.
    fn machine(&self) -> Entity;
}

/// Per-edge filter that accepts or rejects a message for a specific edge.
pub trait MessageValidator<M>: Send + Sync + 'static {
    fn matches(&self, message: &M) -> bool;
}

/// Default validator that accepts all messages.
#[derive(Default, Clone, Debug)]
pub struct AcceptAll;

impl<M> MessageValidator<M> for AcceptAll {
    #[inline]
    fn matches(&self, _: &M) -> bool {
        true
    }
}

/// Attach to a transition edge to make it react to messages of type `M`.
///
/// The edge fires when:
/// 1. The source state is active
/// 2. Guards (if any) are passing
/// 3. The validator (if set) accepts the message
#[derive(Component)]
pub struct MessageEdge<M: GearboxMessage> {
    _marker: PhantomData<M>,
    /// Optional per-edge validator. When `None`, all messages of type `M` are accepted.
    pub validator: Option<M::Validator>,
}

impl<M: GearboxMessage> Default for MessageEdge<M> {
    fn default() -> Self {
        Self {
            _marker: PhantomData,
            validator: None,
        }
    }
}

impl<M: GearboxMessage> MessageEdge<M> {
    pub fn new(validator: Option<M::Validator>) -> Self {
        Self {
            _marker: PhantomData,
            validator,
        }
    }
}

// ---------------------------------------------------------------------------
// Matched<M> + SideEffect trait
// ---------------------------------------------------------------------------

/// Written by [`message_edge_listener`] when a message of type `M` successfully
/// matches an edge and produces a [`TransitionMessage`]. Carries the original
/// message along with the transition context.
///
/// Use [`SideEffect`] to transform a `Matched<M>` into a downstream message.
#[derive(Message, Clone, Debug)]
pub struct Matched<M: GearboxMessage> {
    /// The original message that triggered the transition.
    pub message: M,
    /// The machine root entity.
    pub machine: Entity,
    /// The source state of the transition.
    pub source: Entity,
    /// The target state of the transition.
    pub target: Entity,
    /// The edge entity that was matched.
    pub edge: Entity,
}

/// Pure data transform: produce a side-effect message from a matched transition.
///
/// Register via [`RegistrationAppExt::register_side_effect`](crate::registration::RegistrationAppExt::register_side_effect).
///
/// ```rust,ignore
/// impl SideEffect<StartInvoke<Vec3>> for GoOff<Vec3> {
///     fn produce(matched: &Matched<StartInvoke<Vec3>>) -> Option<Self> {
///         Some(GoOff::new(matched.target, matched.message.targets.clone()))
///     }
/// }
/// ```
pub trait SideEffect<Origin: GearboxMessage>: Message + Send + Sync + Sized + 'static {
    fn produce(matched: &Matched<Origin>) -> Option<Self>;
}

/// System that reads [`Matched<M>`] messages and writes side-effect messages.
/// Registered by [`RegistrationAppExt::register_side_effect`](crate::registration::RegistrationAppExt::register_side_effect).
pub fn produce_side_effects<M: GearboxMessage, S: SideEffect<M>>(
    mut reader: MessageReader<Matched<M>>,
    mut writer: MessageWriter<S>,
) {
    for matched in reader.read() {
        if let Some(effect) = S::produce(matched) {
            writer.write(effect);
        }
    }
}

// ---------------------------------------------------------------------------
// message_edge_listener
// ---------------------------------------------------------------------------

/// System that reads incoming messages of type `M`, finds matching edges on
/// active states (leaf-first, walk ancestors), and writes [`TransitionMessage`]s
/// and [`Matched<M>`] messages.
///
/// Runs in [`Update`] before [`GearboxSet`](crate::GearboxSet) so messages are resolved in the
/// same frame they are sent.
pub fn message_edge_listener<M: GearboxMessage>(
    mut reader: MessageReader<M>,
    mut writer: MessageWriter<TransitionMessage>,
    mut matched_writer: MessageWriter<Matched<M>>,
    q_machine: Query<&StateMachine>,
    q_transitions: Query<&Transitions>,
    q_edge: Query<&MessageEdge<M>>,
    q_target: Query<&Target>,
    q_source: Query<&Source>,
    q_guards: Query<&Guards>,
    q_substate_of: Query<&SubstateOf>,
    q_initial: Query<&InitialState>,
    q_children: Query<&Substates>,
) {
    let msgs: Vec<_> = reader.read().cloned().collect();
    for msg in msgs {
        let machine_entity = msg.machine();
        let Ok(machine) = q_machine.get(machine_entity) else {
            continue;
        };

        // Track which parallel region roots have already fired so each
        // region gets at most one transition per message.
        let mut fired_regions: HashSet<Entity> = HashSet::new();
        let mut visited: HashSet<Entity> = HashSet::new();

        // Walk from each active leaf upward through ancestors (statechart
        // semantics: deepest state gets priority).
        for &leaf in &machine.active_leaves {
            let region_root = find_parallel_region_root(
                leaf,
                &q_substate_of,
                &q_initial,
                &q_children,
            );
            if fired_regions.contains(&region_root) {
                continue;
            }

            if try_fire_edge_on_branch(
                leaf,
                machine_entity,
                &msg,
                &q_transitions,
                &q_edge,
                &q_target,
                &q_source,
                &q_guards,
                &q_substate_of,
                machine,
                &mut visited,
                &mut writer,
                &mut matched_writer,
            ) {
                fired_regions.insert(region_root);
            }
        }

        // If no branch consumed the message, try root-level transitions.
        if fired_regions.is_empty() {
            try_fire_edge_at_state(
                machine_entity,
                machine_entity,
                &msg,
                &q_transitions,
                &q_edge,
                &q_target,
                &q_source,
                &q_guards,
                &mut writer,
                &mut matched_writer,
            );
        }
    }
}

/// Walk from `start` up through ancestors, trying each state's edges.
fn try_fire_edge_on_branch<M: GearboxMessage>(
    start: Entity,
    machine_entity: Entity,
    msg: &M,
    q_transitions: &Query<&Transitions>,
    q_edge: &Query<&MessageEdge<M>>,
    q_target: &Query<&Target>,
    q_source: &Query<&Source>,
    q_guards: &Query<&Guards>,
    q_substate_of: &Query<&SubstateOf>,
    machine: &StateMachine,
    visited: &mut HashSet<Entity>,
    writer: &mut MessageWriter<TransitionMessage>,
    matched_writer: &mut MessageWriter<Matched<M>>,
) -> bool {
    let mut current = Some(start);
    while let Some(state) = current {
        if !visited.insert(state) {
            current = q_substate_of.get(state).ok().map(|rel| rel.0);
            continue;
        }
        if state == machine_entity {
            break;
        }
        if !machine.active.contains(&state) {
            break;
        }
        if try_fire_edge_at_state(
            state,
            machine_entity,
            msg,
            q_transitions,
            q_edge,
            q_target,
            q_source,
            q_guards,
            writer,
            matched_writer,
        ) {
            return true;
        }
        current = q_substate_of.get(state).ok().map(|rel| rel.0);
    }
    false
}

/// Check edges at a single state for a matching `MessageEdge<M>`.
fn try_fire_edge_at_state<M: GearboxMessage>(
    state: Entity,
    machine_entity: Entity,
    msg: &M,
    q_transitions: &Query<&Transitions>,
    q_edge: &Query<&MessageEdge<M>>,
    q_target: &Query<&Target>,
    q_source: &Query<&Source>,
    q_guards: &Query<&Guards>,
    writer: &mut MessageWriter<TransitionMessage>,
    matched_writer: &mut MessageWriter<Matched<M>>,
) -> bool {
    let Ok(transitions) = q_transitions.get(state) else {
        return false;
    };
    for &edge in transitions {
        let Ok(me) = q_edge.get(edge) else {
            continue;
        };
        if let Ok(guards) = q_guards.get(edge) {
            if !guards.is_empty() {
                continue;
            }
        }
        if let Some(v) = &me.validator {
            if !v.matches(msg) {
                continue;
            }
        }
        let Ok(target) = q_target.get(edge) else {
            continue;
        };

        let source_state = q_source.get(edge).map(|s| s.0).unwrap_or(state);

        writer.write(TransitionMessage {
            machine: machine_entity,
            source: source_state,
            target: target.0,
            edge: Some(edge),
        });
        matched_writer.write(Matched {
            message: msg.clone(),
            machine: machine_entity,
            source: source_state,
            target: target.0,
            edge,
        });
        return true;
    }
    false
}

/// Find the parallel region root for a state. If the state is under a
/// parallel parent (has children, no InitialState), returns the immediate
/// child of that parallel parent that contains `state`.
fn find_parallel_region_root(
    state: Entity,
    q_substate_of: &Query<&SubstateOf>,
    q_initial: &Query<&InitialState>,
    q_children: &Query<&Substates>,
) -> Entity {
    let mut previous = state;
    for ancestor in q_substate_of.iter_ancestors(state) {
        let has_children = q_children
            .get(ancestor)
            .ok()
            .map(|c| c.into_iter().next().is_some())
            .unwrap_or(false);
        let is_parallel = !q_initial.contains(ancestor) && has_children;
        if is_parallel {
            return previous;
        }
        previous = ancestor;
    }
    state
}
