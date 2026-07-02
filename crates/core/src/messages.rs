use std::marker::PhantomData;

use bevy::platform::collections::HashSet;
use bevy::prelude::*;

use crate::components::*;
use crate::resolve::{PendingCount, TransitionMessage};

/// Trait implemented by user message types that can trigger state machine
/// transitions.
///
/// Derive it with `#[derive(GearboxMessage)]`, marking the addressed entity
/// with `#[gearbox(target)]`:
///
/// ```rust,ignore
/// #[derive(Message, Clone, Reflect, GearboxMessage)]
/// struct Attack {
///     #[gearbox(target)]
///     machine: Entity,
///     damage: f32,
/// }
/// ```
///
/// The derive fills in `target()` (from the marked field) and defaults
/// `Validator` to [`AcceptAll`]; override it with `#[gearbox(validator = ..)]`.
pub trait GearboxMessage: Message + Clone + Send + Sync + bevy::reflect::TypePath + 'static {
    /// Per-edge validator type. Use [`AcceptAll`] if every edge of this
    /// message type should match unconditionally.
    type Validator: MessageValidator<Self> + Default + Clone + Send + Sync;

    /// Which entity this message is addressed to. Can be a state machine root
    /// or any substate - the message listener walks `SubstateOf` to find the
    /// root machine automatically.
    fn target(&self) -> Entity;
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
/// 2. The transition is not blocked by a blocker system
/// 3. The validator (if set) accepts the message
#[derive(Component, Reflect)]
#[reflect(Component, where M: bevy::reflect::TypePath)]
pub struct MessageEdge<M: GearboxMessage> {
    #[reflect(ignore)]
    _marker: PhantomData<M>,
    /// Optional per-edge validator. When `None`, all messages of type `M` are accepted.
    #[reflect(ignore)]
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

impl<M: GearboxMessage> Clone for MessageEdge<M> {
    fn clone(&self) -> Self {
        Self {
            _marker: PhantomData,
            validator: self.validator.clone(),
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
// Matched<M>
// ---------------------------------------------------------------------------

/// Written by [`message_edge_listener`] when a message of type `M` successfully
/// matches an edge and produces a [`TransitionMessage`]. Carries the original
/// message along with the transition context.
///
/// Read in [`SideEffectPhase`](crate::GearboxPhase::SideEffectPhase) systems.
/// Check [`BlockedEdges`](crate::resolve::BlockedEdges) to skip blocked transitions.
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

// ---------------------------------------------------------------------------
// message_edge_listener
// ---------------------------------------------------------------------------

/// System that reads incoming messages of type `M`, finds matching edges on
/// active states (leaf-first, walk ancestors), and writes [`TransitionMessage`]s
/// and [`Matched<M>`] messages.
///
/// Runs inside [`GearboxSchedule`](crate::GearboxSchedule) in
/// [`GearboxPhase::EdgeDetectPhase`](crate::GearboxPhase::EdgeDetectPhase) so
/// it participates in the per-frame resolution loop. This matters when a
/// user message is written the same frame a [`StateMachine`](crate::StateMachine)
/// is spawned: the first loop iteration activates the initial state and the
/// next iteration's listener pass sees populated `active_leaves` and fires
/// the transition, so the whole cascade resolves before the frame ends.
pub fn message_edge_listener<M: GearboxMessage>(
    mut reader: MessageReader<M>,
    mut writer: MessageWriter<TransitionMessage>,
    mut matched_writer: MessageWriter<Matched<M>>,
    mut pending: ResMut<PendingCount>,
    mut commands: Commands,
    q_machine: Query<&StateMachine>,
    q_transitions: Query<&Transitions>,
    q_edge: Query<&MessageEdge<M>>,
    q_target: Query<&Target>,
    q_branch: Query<&BranchTransition>,
    q_source: Query<&Source>,
    q_substate_of: Query<&SubstateOf>,
    q_initial: Query<&InitialState>,
    q_children: Query<&Substates>,
    q_delay: Query<&Delay>,
    q_timer: Query<(), With<EdgeTimer>>,
) {
    let msgs: Vec<_> = reader.read().cloned().collect();
    for msg in msgs {
        let machine_entity = q_substate_of.root_ancestor(msg.target());
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
                &q_branch,
                &q_source,
                &q_substate_of,
                machine,
                &mut visited,
                &mut writer,
                &mut matched_writer,
                &mut pending.0,
                &mut commands,
                &q_delay,
                &q_timer,
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
                &q_branch,
                &q_source,
                &mut writer,
                &mut matched_writer,
                &mut pending.0,
                &mut commands,
                &q_delay,
                &q_timer,
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
    q_branch: &Query<&BranchTransition>,
    q_source: &Query<&Source>,
    q_substate_of: &Query<&SubstateOf>,
    machine: &StateMachine,
    visited: &mut HashSet<Entity>,
    writer: &mut MessageWriter<TransitionMessage>,
    matched_writer: &mut MessageWriter<Matched<M>>,
    pending: &mut usize,
    commands: &mut Commands,
    q_delay: &Query<&Delay>,
    q_timer: &Query<(), With<EdgeTimer>>,
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
            q_branch,
            q_source,
            writer,
            matched_writer,
            pending,
            commands,
            q_delay,
            q_timer,
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
    q_branch: &Query<&BranchTransition>,
    q_source: &Query<&Source>,
    writer: &mut MessageWriter<TransitionMessage>,
    matched_writer: &mut MessageWriter<Matched<M>>,
    pending: &mut usize,
    commands: &mut Commands,
    q_delay: &Query<&Delay>,
    q_timer: &Query<(), With<EdgeTimer>>,
) -> bool {
    use crate::resolve::resolve_edge_target;

    let Ok(transitions) = q_transitions.get(state) else {
        return false;
    };
    for &edge in transitions {
        let Ok(me) = q_edge.get(edge) else {
            continue;
        };
        if let Some(v) = &me.validator {
            if !v.matches(msg) {
                continue;
            }
        }

        // Delayed message edge: the message starts a timer. The
        // transition fires when the timer expires (via tick_delay_timers).
        if let Ok(delay) = q_delay.get(edge) {
            if q_timer.get(edge).is_err() {
                // No timer yet — start one. The message is consumed
                // (returns true) but the transition is deferred.
                commands.entity(edge).insert(
                    EdgeTimer(Timer::new(delay.duration, TimerMode::Once)),
                );
            }
            // Timer already running — message is consumed but ignored.
            return true;
        }

        let Some(target) = resolve_edge_target(edge, q_branch, q_target) else {
            continue;
        };

        let source_state = q_source.get(edge).map(|s| s.0).unwrap_or(state);

        writer.write(TransitionMessage {
            machine: machine_entity,
            source: source_state,
            target,
            edge: Some(edge),
            blocked: false,
        });
        matched_writer.write(Matched {
            message: msg.clone(),
            machine: machine_entity,
            source: source_state,
            target,
            edge,
        });
        *pending += 1;
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Done message — emitted when a TerminalState is entered
// ---------------------------------------------------------------------------

/// Emitted when a [`TerminalState`](crate::TerminalState) is entered.
/// Targets the parent state so `MessageEdge<Done>` on the parent can fire.
#[derive(Message, Clone, Debug, Reflect)]
pub struct Done {
    entity: Entity,
}

impl Done {
    pub fn new(parent: Entity) -> Self {
        Self { entity: parent }
    }
}

impl GearboxMessage for Done {
    type Validator = AcceptAll;
    fn target(&self) -> Entity {
        self.entity
    }
}

/// System that emits [`Done`] messages when a [`TerminalState`] gains [`Active`].
/// Runs in [`GearboxPhase::EntryPhase`].
pub fn emit_terminal_done(
    q_new: Query<(Entity, &SubstateOf), (Added<Active>, With<TerminalState>)>,
    mut writer: MessageWriter<Done>,
) {
    for (_entity, parent) in &q_new {
        writer.write(Done::new(parent.0));
    }
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
