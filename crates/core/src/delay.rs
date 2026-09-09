use bevy::prelude::*;

use crate::components::*;
use crate::helpers::depth_rank;
use crate::resolve::{CandidateGroups, PendingCount, TransitionMessage};

/// Delay edges whose timers elapsed this frame, grouped by source state and
/// waiting to be proposed as transition candidates by
/// [`propose_delayed_transitions`].
#[derive(Resource, Default)]
pub struct ElapsedDelays(Vec<ElapsedGroup>);

struct ElapsedGroup {
    machine: Entity,
    source: Entity,
    /// `(edge, index in the source's Transitions)`, in Transitions order.
    edges: Vec<(Entity, u32)>,
}

/// Start timers for AlwaysEdge+Delay edges when their source state is entered.
/// Runs in [`GearboxPhase::EntryPhase`](crate::GearboxPhase::EntryPhase).
pub(crate) fn start_delay_timers(
    q_newly_active: Query<Entity, Added<Active>>,
    q_transitions: Query<&Transitions>,
    q_always: Query<(), With<AlwaysEdge>>,
    q_delay: Query<&Delay>,
    mut commands: Commands,
) {
    for state in &q_newly_active {
        let Ok(transitions) = q_transitions.get(state) else {
            continue;
        };
        for &edge in transitions {
            if q_always.get(edge).is_ok() {
                if let Ok(delay) = q_delay.get(edge) {
                    commands
                        .entity(edge)
                        .insert(EdgeTimer(Timer::new(delay.duration, TimerMode::Once)));
                }
            }
        }
    }
}

/// Cancel timers for edges whose source state was exited.
/// Runs in [`GearboxPhase::ExitPhase`](crate::GearboxPhase::ExitPhase).
pub(crate) fn cancel_delay_timers(
    mut removed: RemovedComponents<Active>,
    q_transitions: Query<&Transitions>,
    q_delay: Query<(), With<Delay>>,
    mut commands: Commands,
) {
    for state in removed.read() {
        let Ok(transitions) = q_transitions.get(state) else {
            continue;
        };
        for &edge in transitions {
            if q_delay.get(edge).is_ok() {
                commands.entity(edge).try_remove::<EdgeTimer>();
            }
        }
    }
}

/// Tick every active delay timer and record the edges that just finished in
/// [`ElapsedDelays`]. Works for both `AlwaysEdge + Delay` (timer started on
/// state entry) and `MessageEdge + Delay` (timer started when the message first
/// matched).
///
/// Runs in the outer schedule before the per-frame
/// [`GearboxSchedule`](crate::GearboxSchedule) loop, so elapsed delays are
/// proposed in the first iteration and any cascade they trigger resolves in
/// the same frame.
pub(crate) fn tick_delay_timers(
    time: Res<Time>,
    q_transitions: Query<(Entity, &Transitions)>,
    mut q_timer: Query<&mut EdgeTimer>,
    q_delay: Query<(), With<Delay>>,
    q_substate_of: Query<&SubstateOf>,
    q_machine: Query<&StateMachine>,
    mut elapsed: ResMut<ElapsedDelays>,
) {
    for (source, transitions) in &q_transitions {
        let root = q_substate_of.root_ancestor(source);
        let Ok(machine) = q_machine.get(root) else {
            continue;
        };
        if !machine.is_active(&source) {
            continue;
        }
        let mut fired: Vec<(Entity, u32)> = Vec::new();
        for (index, &edge) in transitions.into_iter().enumerate() {
            if !q_delay.contains(edge) {
                continue;
            }
            let Ok(mut timer) = q_timer.get_mut(edge) else {
                continue;
            };
            timer.0.tick(time.delta());
            if timer.0.just_finished() {
                fired.push((edge, index as u32));
            }
        }
        if !fired.is_empty() {
            elapsed.0.push(ElapsedGroup {
                machine: root,
                source,
                edges: fired,
            });
        }
    }
}

/// Propose the edges recorded in [`ElapsedDelays`] as transition candidates.
/// Edges of one source that elapsed in the same frame share a group, ranked
/// by [`Transitions`] order, so guarded delayed edges fall through to the next
/// one (the XState `after: { ms: [ .. ] }` list).
///
/// Runs in [`GearboxPhase::EdgeDetectPhase`](crate::GearboxPhase::EdgeDetectPhase).
pub(crate) fn propose_delayed_transitions(
    mut elapsed: ResMut<ElapsedDelays>,
    mut groups: ResMut<CandidateGroups>,
    mut writer: MessageWriter<TransitionMessage>,
    mut pending: ResMut<PendingCount>,
    q_target: Query<&Target>,
    q_substate_of: Query<&SubstateOf>,
) {
    for fired in elapsed.0.drain(..) {
        let group = groups.next();
        let depth = depth_rank(fired.source, &q_substate_of);
        for (edge, index) in fired.edges {
            let Ok(target) = q_target.get(edge) else {
                continue;
            };
            writer.write(TransitionMessage {
                machine: fired.machine,
                source: fired.source,
                target: target.0,
                edge: Some(edge),
                blocked: false,
                group,
                rank: (depth, index),
            });
            pending.0 += 1;
        }
    }
}
