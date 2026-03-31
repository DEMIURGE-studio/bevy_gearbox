use bevy::prelude::*;
use crate::types::EntityId;
use super::types::{ConnectionState, StateMachineIndex};

#[derive(Debug, Default, Resource)]
pub struct EditorStore {
    pub connection: ConnectionState,
    /// Last endpoint used for connection (if any)
    pub last_endpoint: Option<String>,
    /// Monotonically increasing session identifier; increment on each successful connect/reconnect
    pub session_id: u64,
    pub index: StateMachineIndex,
    /// Currently active/open document shown in the canvas
    pub active_doc: Option<EntityId>,
}

impl EditorStore {
    pub fn clear_session(&mut self) {
        self.index = StateMachineIndex::default();
        self.active_doc = None;
    }
}


