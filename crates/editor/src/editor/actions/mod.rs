use bevy::prelude::*;
use crate::types::EntityId;
use super::session::store::EditorStore;
use super::session::types::{ConnectionState, IndexFilter};
use bevy_gearbox_protocol::client::{ClientCommand, NetCommand};
use crate::editor::workspace::Workspace;
use crate::editor::open_docs::Docs;

#[derive(Debug, Clone)]
pub struct EndpointConfig { pub endpoint: String }

pub fn connect(store: &mut EditorStore, endpoint: EndpointConfig) {
    store.connection = ConnectionState::Connecting;
    store.last_endpoint = Some(endpoint.endpoint.clone());
}

pub fn disconnect(store: &mut EditorStore) {
    store.connection = ConnectionState::Disconnected;
    store.clear_session();
}

pub fn reconnect(store: &mut EditorStore) {
    // Drop the session, then reconnect to the last endpoint if there is one.
    let endpoint = store.last_endpoint.clone();
    store.clear_session();
    store.connection = ConnectionState::Disconnected;
    if endpoint.is_some() {
        store.connection = ConnectionState::Connecting;
        store.session_id = store.session_id.wrapping_add(1);
    }
}

pub fn refresh_index(_store: &mut EditorStore, _filter: IndexFilter) {
    // The index is filled asynchronously from discovery events.
}

// Request events, handled by observers in `plugin.rs`.
/// Connect to the given protocol endpoint.
#[derive(Debug, Clone, Event)]
pub struct ConnectRequested { pub endpoint: String }

/// Drop the connection and clear the session.
#[derive(Debug, Clone, Event)]
pub struct DisconnectRequested;

/// Reconnect to the last endpoint.
#[derive(Debug, Clone, Event)]
pub struct ReconnectRequested;

/// Re-request the list of machines, filtered by `query`.
#[derive(Debug, Clone, Event)]
pub struct RefreshIndexRequested { pub query: String }

/// Open a machine on the board and subscribe to its watch stream.
#[derive(Debug, Clone, Event)]
pub struct OpenRequested { pub entity: EntityId }

// Observers: mutate store
pub fn on_connect_requested(evt: On<ConnectRequested>, mut store: ResMut<EditorStore>, mut proto_cmd: MessageWriter<ClientCommand>, _proto_net: MessageWriter<NetCommand>) {
    // Optimistically bump session and set last endpoint
    connect(&mut store, EndpointConfig { endpoint: evt.endpoint.clone() });
    // Route URL and connection intent to the protocol client
    proto_cmd.write(ClientCommand::SetUrl { url: evt.endpoint.clone() });
    // Kick off initial refresh only; discovery will start after a successful refresh
    proto_cmd.write(ClientCommand::RefreshMachines);
}

pub fn on_disconnect_requested(_evt: On<DisconnectRequested>, mut store: ResMut<EditorStore>, mut proto_net: MessageWriter<NetCommand>) {
    // Stop discovery stream when disconnecting
    proto_net.write(NetCommand::StopDiscovery);
    disconnect(&mut store);
}

pub fn on_reconnect_requested(_evt: On<ReconnectRequested>, mut store: ResMut<EditorStore>, mut proto_cmd: MessageWriter<ClientCommand>, mut proto_net: MessageWriter<NetCommand>) {
    reconnect(&mut store);
    if let Some(ep) = store.last_endpoint.clone() {
        proto_cmd.write(ClientCommand::SetUrl { url: ep });
    }
    proto_net.write(NetCommand::StartDiscovery);
    proto_cmd.write(ClientCommand::RefreshMachines);
}

pub fn on_refresh_index_requested(evt: On<RefreshIndexRequested>, mut store: ResMut<EditorStore>, mut proto_cmd: MessageWriter<ClientCommand>) {
    refresh_index(&mut store, IndexFilter { query: evt.query.clone() });
    proto_cmd.write(ClientCommand::RefreshMachines);
}

pub fn on_open_requested(
    evt: On<OpenRequested>,
    _store: ResMut<EditorStore>,
    workspace: ResMut<Workspace>,
    mut docs: ResMut<Docs>,
    _commands: Commands,
    mut proto_net: MessageWriter<NetCommand>,
    mut proto_cmd: MessageWriter<ClientCommand>,
) {
    // Ensure a doc entry exists immediately for drawing feedback; keep previously open docs (multi-open)
    let doc = docs.map.entry(evt.entity).or_default();
    // Seed the new document's transform from the global board transform so zoom/pan are consistent
    // This is a shallow copy of the current board transform; subsequent board ops will update all docs
    // via the shell/view board handlers.
    doc.transform.pan = workspace.board_transform.pan;
    doc.transform.zoom = workspace.board_transform.zoom;
    // Start per-machine watch stream (do not stop others)
    proto_net.write(NetCommand::StartMachine { id: evt.entity.0 });
    // Request a fresh graph snapshot via protocol (handled asynchronously)
    proto_cmd.write(ClientCommand::FetchGraph { id: evt.entity.0 });
}

/// Stop watching a machine without closing its document.
#[derive(Debug, Clone, Event)]
pub struct UnsubscribeRequested { pub entity: EntityId }

pub fn on_unsubscribe_requested(evt: On<UnsubscribeRequested>, mut proto_net: MessageWriter<NetCommand>) {
    // Decoupled unsubscribe: stop server-side feeds for this machine. Do not couple to new selection.
    // Also stop the root StateMachine component watch so re-opening forces an initial snapshot.
    proto_net.write(NetCommand::StopComponents { id: evt.entity.0 });
    proto_net.write(NetCommand::StopMachine { id: evt.entity.0 });
}

/// Close a machine's document and stop watching it.
#[derive(Debug, Clone, Event)]
pub struct CloseRequested { pub entity: EntityId }

/// Close an open document and unsubscribe from all related server-side feeds.
pub fn close_doc_and_unsubscribe(
    entity: EntityId,
    workspace: &mut Workspace,
    docs: &mut Docs,
    proto_net: &mut MessageWriter<NetCommand>,
)
{
    // Stop server-side feeds for this machine and any node/edge component watches if known
    if let Some(doc) = docs.map.get(&entity) {
        if let Some(g) = &doc.graph {
            for nid in g.nodes.keys() { proto_net.write(NetCommand::StopComponents { id: nid.0 }); }
            for eid in g.edges.keys() { proto_net.write(NetCommand::StopComponents { id: eid.0 }); }
        }
    }
    proto_net.write(NetCommand::StopComponents { id: entity.0 });
    proto_net.write(NetCommand::StopMachine { id: entity.0 });
    // Drop editor-side state after stopping watches
    let _ = docs.map.remove(&entity);
    if let Some((d, _)) = workspace.global_selection {
        if d == entity { workspace.global_selection = None; }
    }
}

pub fn on_close_requested(
    evt: On<CloseRequested>,
    mut workspace: ResMut<Workspace>,
    mut docs: ResMut<Docs>,
    mut proto_net: MessageWriter<NetCommand>,
) {
    close_doc_and_unsubscribe(evt.entity, &mut workspace, &mut docs, &mut proto_net);
}

/// Save the subtree under `target` as `assets/<id>.scn.ron` plus the layout
/// sidecar `assets/<id>.sm.ron`, on the game side, and set `StateMachineId(id)`
/// on the target so later saves and sidecar lookups find it.
#[derive(Debug, Clone, Event)]
pub struct SaveRequested { pub doc: EntityId, pub target: EntityId, pub id: String }

pub fn on_save_requested(
    req: On<SaveRequested>,
    docs: Res<Docs>,
    client: Res<bevy_gearbox_protocol::client::Client>,
    rt: Res<bevy_gearbox_protocol::client::TokioRuntime>,
) {
    let id_text = normalize_machine_id(&req.id);
    let scn_path = format!("{id_text}.scn.ron");
    let sm_path = format!("{id_text}.sm.ron");

    // Serialize the layout now, from the doc as drawn; upload it only if the scene saved.
    let sidecar_text: Option<String> = docs.map.get(&req.doc).and_then(|doc| {
        let sc = crate::persistence::extract_sidecar_for_subtree(doc, &req.target);
        ron::ser::to_string_pretty(&sc, ron::ser::PrettyConfig::new()).ok()
    });

    let entity_bits = req.target.0;
    let client_cloned = client.clone();
    rt.0.spawn(async move {
        if let Err(e) = client_cloned.set_state_machine_id(entity_bits, &id_text).await {
            error!("Save: could not set StateMachineId({id_text:?}): {e}");
            return;
        }
        let outcome = match client_cloned.save_as(entity_bits, &scn_path).await {
            Ok(o) => o,
            Err(e) => {
                error!("Save: the game failed to save {scn_path}: {e}");
                return;
            }
        };
        info!("Save: wrote {}", outcome.path);
        if !outcome.skipped.is_empty() {
            warn!(
                "Save: left out of the scene (no Reflect registration): {}",
                outcome.skipped.join(", ")
            );
        }
        if let Some(txt) = sidecar_text {
            if let Err(e) = client_cloned.save_sidecar(&sm_path, &txt).await {
                error!("Save: the game failed to save the layout sidecar {sm_path}: {e}");
            }
        }
    });
}

/// A `StateMachineId` from what the user typed. The id is a path relative to
/// the game's `assets/` folder, so `enemies/goblin` saves to
/// `assets/enemies/goblin.scn.ron`. Normalisation: either slash works, empty
/// and `.` segments are dropped, `..` is dropped (no escaping `assets/`), a
/// leading `assets/` is stripped, and any trailing `.scn.ron` / `.sm.ron` /
/// `.ron` / `.scn` / `.sm` is removed from the file name (people type file
/// names). Never empty.
pub fn normalize_machine_id(input: &str) -> String {
    let mut segments: Vec<&str> = input
        .trim()
        .split(['/', '\\'])
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .collect();
    if segments.first() == Some(&"assets") {
        segments.remove(0);
    }
    let Some(last) = segments.pop() else {
        return "statemachine".to_string();
    };
    let mut name = last;
    loop {
        let before = name;
        for suffix in [".scn.ron", ".sm.ron", ".ron", ".scn", ".sm"] {
            if let Some(stripped) = name.strip_suffix(suffix) {
                name = stripped;
                break;
            }
        }
        if name == before || name.is_empty() {
            break;
        }
    }
    if name.is_empty() {
        name = "statemachine";
    }
    segments.push(name);
    segments.join("/")
}

#[cfg(test)]
mod tests {
    use super::normalize_machine_id;

    #[test]
    fn machine_id_strips_stacked_suffixes() {
        assert_eq!(normalize_machine_id("statemachine"), "statemachine");
        assert_eq!(normalize_machine_id("  character  "), "character");
        assert_eq!(normalize_machine_id("character.scn.ron"), "character");
        assert_eq!(normalize_machine_id("character.sm.ron"), "character");
        assert_eq!(normalize_machine_id("statemachine.sm.ron.sm.ron"), "statemachine");
        assert_eq!(normalize_machine_id("ability.ron"), "ability");
        assert_eq!(normalize_machine_id(""), "statemachine");
        assert_eq!(normalize_machine_id(".sm.ron"), "statemachine");
    }

    #[test]
    fn machine_id_keeps_folders_under_assets() {
        assert_eq!(normalize_machine_id("enemies/goblin"), "enemies/goblin");
        assert_eq!(normalize_machine_id("enemies\\goblin.scn.ron"), "enemies/goblin");
        assert_eq!(normalize_machine_id("assets/enemies/goblin"), "enemies/goblin");
        assert_eq!(normalize_machine_id("/enemies//goblin/"), "enemies/goblin");
        assert_eq!(normalize_machine_id("../../etc/passwd"), "etc/passwd");
        assert_eq!(normalize_machine_id("enemies/"), "enemies");
    }
}

/// Save every substate that carries a `StateMachineId` to its own scene.
#[derive(Debug, Clone, Event)]
pub struct SaveSubstatesRequested { pub target: EntityId }

pub fn on_save_substates_requested(
    req: On<SaveSubstatesRequested>,
    client: Res<bevy_gearbox_protocol::client::Client>,
    rt: Res<bevy_gearbox_protocol::client::TokioRuntime>,
) {
    let id = req.target.0;
    let client_cloned = client.clone();
    rt.0.spawn(async move {
        let _ = client_cloned.save_substates(id).await;
    });
}

/// Insert a `Delay` of `seconds` on an edge.
#[derive(Debug, Clone, Event)]
pub struct SetEdgeDelayRequested { pub target: EntityId, pub seconds: f32 }

/// Remove the `Delay` from an edge.
#[derive(Debug, Clone, Event)]
pub struct ClearEdgeDelayRequested { pub target: EntityId }

/// Set an edge's `EdgeKind` to internal or external.
#[derive(Debug, Clone, Event)]
pub struct SetEdgeKindRequested { pub target: EntityId, pub internal: bool }



