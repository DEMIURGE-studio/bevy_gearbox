use bevy::prelude::*;
use super::types::{ConnectionState, StateMachineIndex};

#[derive(Debug, Default, Resource)]
pub struct EditorStore {
    pub connection: ConnectionState,
    /// Last endpoint used for connection (if any)
    pub last_endpoint: Option<String>,
    /// Monotonically increasing session identifier; increment on each successful connect/reconnect
    pub session_id: u64,
    pub index: StateMachineIndex,
}

impl EditorStore {
    pub fn clear_session(&mut self) {
        self.index = StateMachineIndex::default();
    }
}


