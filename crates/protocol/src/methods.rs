//! JSON-RPC method names. Every method the server registers and the client
//! calls is named here; neither side uses a string literal.
//!
//! Names ending in `+watch` are Bevy Remote Protocol watch methods: the server
//! keeps the HTTP response open and streams `data: {..}` lines.

// Bevy Remote Protocol built-ins.
pub const WORLD_GET_COMPONENTS: &str = "world.get_components";
pub const WORLD_GET_COMPONENTS_WATCH: &str = "world.get_components+watch";
pub const WORLD_INSERT_COMPONENTS: &str = "world.insert_components";
pub const WORLD_REMOVE_COMPONENTS: &str = "world.remove_components";
pub const WORLD_SPAWN: &str = "world.spawn_entity";
pub const WORLD_DESPAWN: &str = "world.despawn_entity";
pub const WORLD_QUERY: &str = "world.query";
pub const REGISTRY_SCHEMA: &str = "registry.schema";

// Compatibility.
pub const PROTOCOL_VERSION: &str = "protocol.version";

// Streams.
pub const EDITOR_DISCOVERY_WATCH: &str = "editor.discovery+watch";
pub const EDITOR_MACHINE_WATCH: &str = "editor.machine+watch";
pub const EDITOR_CONTROL_WATCH: &str = "editor.control+watch";
pub const EDITOR_MACHINE_SUBSCRIBE: &str = "editor.machine_subscribe";
pub const EDITOR_MACHINE_UNSUBSCRIBE: &str = "editor.machine_unsubscribe";

// Files: scenes and layout sidecars under the game's `assets/`.
pub const EDITOR_SAVE_AS: &str = "editor.save_as";
pub const EDITOR_SAVE_SUBSTATES: &str = "editor.save_substates";
pub const EDITOR_SAVE_GRAPH: &str = "editor.save_graph";
pub const EDITOR_SAVE_SIDECAR: &str = "editor.save_sidecar";
pub const EDITOR_LOAD_SIDECAR: &str = "editor.load_sidecar";
pub const EDITOR_FIND_SIDECAR_BY_FINGERPRINT: &str = "editor.find_sidecar_by_fingerprint";
pub const EDITOR_SET_STATE_MACHINE_ID: &str = "editor.set_state_machine_id";
pub const EDITOR_SIDECAR_FOR_MACHINE: &str = "editor.sidecar_for_machine";

// Graph snapshot and chart edits.
pub const EDITOR_MACHINE_GRAPH: &str = "editor.machine_graph";
pub const EDITOR_SPAWN_SUBSTATE: &str = "editor.spawn_substate";
pub const EDITOR_DELETE_SUBTREE: &str = "editor.delete_subtree";
pub const EDITOR_RESET_REGION: &str = "editor.reset_region";
pub const EDITOR_CREATE_TRANSITION: &str = "editor.create_transition";
pub const EDITOR_MAKE_LEAF: &str = "editor.make_leaf";
pub const EDITOR_MAKE_PARENT: &str = "editor.make_parent";
pub const EDITOR_MAKE_PARALLEL: &str = "editor.make_parallel";
pub const EDITOR_SET_INITIAL_STATE: &str = "editor.set_initial_state";

// Game -> editor control bus.
pub const EDITOR_OPEN_ON_CLIENT: &str = "editor.open_on_client";
pub const EDITOR_OPEN_IF_RELATED: &str = "editor.open_if_related";
