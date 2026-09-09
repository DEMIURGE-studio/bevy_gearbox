use std::marker::PhantomData;

use bevy::platform::collections::HashSet;
use bevy::prelude::*;

use crate::components::*;
use crate::helpers::{depth_rank, region_root};
use crate::resolve::{CandidateGroups, PendingCount, TransitionMessage};

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
///
/// A validator sees only the message. For conditions that need world access
/// (the machine's components, resources), put a marker component on the edge
/// and veto it from a system in
/// [`BlockerPhase`](crate::GearboxPhase::BlockerPhase); a vetoed candidate
/// falls through to the next edge in [`Transitions`] order.
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
/// 2. The validator (if set) accepts the message
/// 3. No blocker system vetoes it, and no better-ranked candidate (a deeper
///    state's edge, or an earlier edge on the same state) survives
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

/// Written by [`message_edge_listener`] for every edge that matches a message
/// of type `M` and is proposed as a [`TransitionMessage`] candidate. Carries
/// the original message along with the transition context.
///
/// Read in [`SideEffectPhase`](crate::GearboxPhase::SideEffectPhase) systems.
/// Check [`BlockedEdges`](crate::resolve::BlockedEdges): a `Matched` whose
/// edge is blocked was vetoed or lost to a better-ranked candidate and its
/// transition will not be applied.
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

/// System that reads incoming messages of type `M` and proposes every matching
/// edge on the active configuration as a [`TransitionMessage`] candidate, plus
/// a [`Matched<M>`] for each.
///
/// Selection follows statechart rules. For each parallel region, edges are
/// gathered from the active leaf up through its ancestors to the region root;
/// all of them share one candidate group, ranked deeper-state-first and then
/// by [`Transitions`] order, so after blockers run
/// [`select_transitions`](crate::resolve::select_transitions) keeps the first
/// surviving candidate per region. If no region has a matching edge, the
/// states above the regions (up to the machine root) are tried as one group.
///
/// A delayed `MessageEdge` (one carrying [`Delay`]) reached before any
/// undelayed candidate starts its [`EdgeTimer`] and consumes the message for
/// that region; the transition itself is proposed when the timer elapses.
///
/// Runs inside [`GearboxSchedule`](crate::GearboxSchedule) in
/// [`GearboxPhase::EdgeDetectPhase`](crate::GearboxPhase::EdgeDetectPhase) so
/// it participates in the per-frame resolution loop: a message written the
/// same frame a machine is spawned is seen after the machine's initial state
/// has been activated.
pub fn message_edge_listener<M: GearboxMessage>(
    mut reader: MessageReader<M>,
    mut writer: MessageWriter<TransitionMessage>,
    mut matched_writer: MessageWriter<Matched<M>>,
    mut pending: ResMut<PendingCount>,
    mut groups: ResMut<CandidateGroups>,
    mut commands: Commands,
    q_machine: Query<&StateMachine>,
    q_transitions: Query<&Transitions>,
    q_edge: Query<&MessageEdge<M>>,
    q_target: Query<&Target>,
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

        // One candidate group per parallel region, gathered from the active
        // leaf up to the region root.
        let mut regions: HashSet<Entity> = HashSet::new();
        let mut any_region_matched = false;
        for &leaf in &machine.active_leaves {
            let region = region_root(leaf, machine_entity, &q_substate_of, &q_initial, &q_children);
            if !regions.insert(region) {
                continue;
            }
            let group = groups.next();
            let mut proposed = false;
            let mut current = Some(leaf);
            while let Some(state) = current {
                if !machine.active.contains(&state) {
                    break;
                }
                let deferred = propose_at_state(
                    state, machine_entity, group, &msg, &mut proposed,
                    &q_transitions, &q_edge, &q_target, &q_source, &q_substate_of, &q_delay, &q_timer,
                    &mut writer, &mut matched_writer, &mut pending.0, &mut commands,
                );
                if deferred || state == region {
                    break;
                }
                current = q_substate_of.get(state).ok().map(|rel| rel.0);
            }
            any_region_matched |= proposed;
        }

        // Nothing inside any region matched: try the states above the regions,
        // up to and including the machine root, as a single group.
        if !any_region_matched {
            let group = groups.next();
            let mut proposed = false;
            let mut visited: HashSet<Entity> = HashSet::new();
            'regions: for &region in &regions {
                let mut current = if region == machine_entity {
                    Some(machine_entity)
                } else {
                    q_substate_of.get(region).ok().map(|rel| rel.0)
                };
                while let Some(state) = current {
                    if !visited.insert(state) {
                        break;
                    }
                    let deferred = propose_at_state(
                        state, machine_entity, group, &msg, &mut proposed,
                        &q_transitions, &q_edge, &q_target, &q_source, &q_substate_of, &q_delay, &q_timer,
                        &mut writer, &mut matched_writer, &mut pending.0, &mut commands,
                    );
                    if deferred {
                        break 'regions;
                    }
                    if state == machine_entity {
                        break;
                    }
                    current = q_substate_of.get(state).ok().map(|rel| rel.0);
                }
            }
        }
    }
}

/// Propose every edge on `state` whose `MessageEdge<M>` accepts `msg`.
///
/// Returns `true` if a delayed edge consumed the message (its timer was
/// started, or is already running) before any undelayed candidate was
/// proposed in this group; the caller stops walking the chain. Delayed edges
/// encountered after an undelayed candidate are skipped.
#[allow(clippy::too_many_arguments)]
fn propose_at_state<M: GearboxMessage>(
    state: Entity,
    machine: Entity,
    group: u64,
    msg: &M,
    proposed: &mut bool,
    q_transitions: &Query<&Transitions>,
    q_edge: &Query<&MessageEdge<M>>,
    q_target: &Query<&Target>,
    q_source: &Query<&Source>,
    q_substate_of: &Query<&SubstateOf>,
    q_delay: &Query<&Delay>,
    q_timer: &Query<(), With<EdgeTimer>>,
    writer: &mut MessageWriter<TransitionMessage>,
    matched_writer: &mut MessageWriter<Matched<M>>,
    pending: &mut usize,
    commands: &mut Commands,
) -> bool {
    let Ok(transitions) = q_transitions.get(state) else {
        return false;
    };
    let depth = depth_rank(state, q_substate_of);

    for (index, &edge) in transitions.into_iter().enumerate() {
        let Ok(me) = q_edge.get(edge) else {
            continue;
        };
        if let Some(v) = &me.validator {
            if !v.matches(msg) {
                continue;
            }
        }

        if let Ok(delay) = q_delay.get(edge) {
            if *proposed {
                continue;
            }
            if !q_timer.contains(edge) {
                commands
                    .entity(edge)
                    .insert(EdgeTimer(Timer::new(delay.duration, TimerMode::Once)));
            }
            *proposed = true;
            return true;
        }

        let Ok(target) = q_target.get(edge) else {
            continue;
        };
        let source = q_source.get(edge).map(|s| s.0).unwrap_or(state);

        writer.write(TransitionMessage {
            machine,
            source,
            target: target.0,
            edge: Some(edge),
            blocked: false,
            group,
            rank: (depth, index as u32),
        });
        matched_writer.write(Matched {
            message: msg.clone(),
            machine,
            source,
            target: target.0,
            edge,
        });
        *pending += 1;
        *proposed = true;
    }
    false
}

// ---------------------------------------------------------------------------
// Done message — emitted when a TerminalState is entered
// ---------------------------------------------------------------------------

/// Emitted when a [`TerminalState`] is entered.
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
/// Runs in [`GearboxPhase::EdgeDetectPhase`](crate::GearboxPhase::EdgeDetectPhase),
/// ahead of the edge listeners, so the parent's `MessageEdge<Done>` is
/// considered in the same iteration.
pub fn emit_terminal_done(
    q_new: Query<(Entity, &SubstateOf), (Added<Active>, With<TerminalState>)>,
    mut writer: MessageWriter<Done>,
) {
    for (_entity, parent) in &q_new {
        writer.write(Done::new(parent.0));
    }
}
