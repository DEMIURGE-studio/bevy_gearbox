use bevy::prelude::*;

#[derive(EntityEvent, Debug, Clone)]
pub struct Rename {
    #[event_target]
    pub target: Entity,
    pub name: String,
}

#[derive(EntityEvent, Debug, Clone)]
pub struct Despawn { #[event_target] pub target: Entity }

#[derive(Event, Debug, Clone)]
pub struct SpawnStateMachine { pub name: Option<String> }

#[derive(Event, Debug, Clone)]
pub struct SpawnSubstate { pub parent: Entity, pub name: Option<String> }

#[derive(Event, Debug, Clone)]
pub struct SetInitialState { pub parent: Entity, pub child: Entity }

#[derive(Debug, Clone, Copy)]
pub enum NodeType { Leaf, Parent, Parallel }

#[derive(EntityEvent, Debug, Clone)]
pub struct ChangeNodeType { #[event_target] pub target: Entity, pub to: NodeType }

#[derive(EntityEvent, Debug, Clone)]
pub struct ResetRegion { #[event_target] pub target: Entity }

#[derive(EntityEvent, Debug, Clone)]
pub struct MachineSubscribed { #[event_target] pub target: Entity }

#[derive(Event, Debug, Clone)]
pub struct CreateTransition { pub machine: Entity, pub source: Entity, pub target: Entity, pub kind: String }

#[derive(EntityEvent, Debug, Clone)]
pub struct OpenOnClient { #[event_target] pub target: Entity }

#[derive(EntityEvent, Debug, Clone)]
pub struct OpenIfRelated { #[event_target] pub target: Entity, pub related: Entity }

