//! Canonical JSON-RPC method names used by the protocol.

pub const WORLD_GET_COMPONENTS: &str = "world.get_components";
pub const WORLD_INSERT_COMPONENTS: &str = "world.insert_components";
pub const WORLD_REMOVE_COMPONENTS: &str = "world.remove_components";
pub const WORLD_SPAWN: &str = "world.spawn_entity";
pub const WORLD_DESPAWN: &str = "world.despawn_entity";
pub const WORLD_QUERY: &str = "world.query";

pub const PROTOCOL_VERSION: &str = "protocol.version";
pub const REGISTRY_SCHEMA: &str = "registry.schema";

pub const EDITOR_MACHINE_SUBSCRIBE: &str = "editor.machine_subscribe";
pub const EDITOR_MACHINE_UNSUBSCRIBE: &str = "editor.machine_unsubscribe";

pub const EDITOR_SAVE_GRAPH: &str = "editor.save_graph";
pub const EDITOR_SAVE_SIDECAR: &str = "editor.save_sidecar";
pub const EDITOR_LOAD_SIDECAR: &str = "editor.load_sidecar";
pub const EDITOR_FIND_SIDECAR_BY_FINGERPRINT: &str = "editor.find_sidecar_by_fingerprint";
pub const EDITOR_SET_STATE_MACHINE_ID: &str = "editor.set_state_machine_id";
pub const EDITOR_SIDECAR_FOR_MACHINE: &str = "editor.sidecar_for_machine";

// Graph snapshot
pub const EDITOR_MACHINE_GRAPH: &str = "editor.machine_graph";
pub const EDITOR_SPAWN_STATE_MACHINE: &str = "editor.spawn_state_machine";
pub const EDITOR_SPAWN_SUBSTATE: &str = "editor.spawn_substate";
pub const EDITOR_DELETE_SUBTREE: &str = "editor.delete_subtree";
pub const EDITOR_RESET_REGION: &str = "editor.reset_region";
pub const EDITOR_CREATE_TRANSITION: &str = "editor.create_transition";

// Node transformations
pub const EDITOR_MAKE_LEAF: &str = "editor.make_leaf";
pub const EDITOR_MAKE_PARENT: &str = "editor.make_parent";
pub const EDITOR_MAKE_PARALLEL: &str = "editor.make_parallel";


