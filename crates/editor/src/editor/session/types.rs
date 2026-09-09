use crate::types::EntityId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected { session_id: u64, endpoint: String },
}

impl Default for ConnectionState {
    fn default() -> Self { ConnectionState::Disconnected }
}

#[derive(Debug, Clone, Default)]
pub struct IndexFilter {
    pub query: String,
}

#[derive(Debug, Clone)]
pub struct IndexItem {
    pub name: Option<String>,
    pub entity: EntityId,
}

#[derive(Debug, Default, Clone)]
pub struct StateMachineIndex {
    pub items: Vec<IndexItem>,
    pub filter: IndexFilter,
    pub is_loading: bool,
    pub error: Option<String>,
}


