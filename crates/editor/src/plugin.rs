use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use std::collections::HashMap;

use crate::editor::actions::{
    close_doc_and_unsubscribe, on_connect_requested, on_disconnect_requested, on_open_requested,
    on_reconnect_requested, on_refresh_index_requested,
};
use crate::editor::adapter::project_graph_into_doc;
use crate::editor::docs::Docs;
use crate::editor::model::store::EditorStore;
use crate::editor::model::types::ConnectionState as EditorConnectionState;
use crate::editor::model::types::IndexItem;
use crate::editor::workspace::Workspace;
use crate::model::{ComponentEntry, StateMachineGraph};
use crate::persistence::{apply_sidecar_to_doc, load_sidecar, parse_sidecar_text};
use crate::types::EntityId;
use bevy_gearbox_protocol::client::{
    ClientCommand, ClientMessage, ClientPlugin, NetCommand, NetMessage,
};
use bevy_gearbox_protocol::components as c;
use serde_json::Map as JsonMap;
use serde_json::Value as JsonValue;

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ClientPlugin)
            .insert_resource(UiState {
                url_edit: String::new(),
                connecting: false,
                error: None,
                machines: vec![],
                graphs: HashMap::new(),
                sidecar_texts: HashMap::new(),

                pending_machine_events: HashMap::new(),
                last_transition_seq: HashMap::new(),
            })
            .init_resource::<Workspace>()
            .init_resource::<Docs>()
            .insert_resource(EditorStore::default())
            .add_systems(Startup, setup_camera)
            .add_systems(Update, (poll_network, sync_snapshots_to_workspace))
            // Register editor observers (events are triggered via commands.trigger(...))
            .add_observer(on_connect_requested)
            .add_observer(on_disconnect_requested)
            .add_observer(on_reconnect_requested)
            .add_observer(on_refresh_index_requested)
            .add_observer(on_open_requested)
            .add_observer(crate::editor::actions::on_close_requested)
            .add_observer(crate::editor::actions::on_unsubscribe_requested)
            .add_observer(crate::editor::actions::on_save_as_requested)
            .add_observer(crate::editor::actions::on_save_substates_requested)
            .add_observer(on_set_edge_delay_requested)
            .add_observer(on_clear_edge_delay_requested)
            .add_observer(on_set_edge_kind_requested)
            .add_observer(on_spawn_state_machine)
            .add_observer(on_spawn_substate);

        use bevy_egui::EguiPrimaryContextPass;
        app.add_systems(EguiPrimaryContextPass, ui_system);
    }
}

#[derive(Resource, Clone)]
pub(crate) struct UiState {
    url_edit: String,
    connecting: bool,
    error: Option<String>,
    machines: Vec<(EntityId, Option<String>)>,
    graphs: HashMap<EntityId, StateMachineGraph>,
    /// Latest sidecar text fetched over RPC per machine (if any)
    sidecar_texts: HashMap<EntityId, String>,
    /// Accumulated machine +watch events awaiting application to docs
    pending_machine_events: HashMap<EntityId, Vec<JsonValue>>,
    /// Per-machine cursors for stateless +watch
    last_transition_seq: HashMap<EntityId, u64>,
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// Start watches for a machine root: machine events plus StateMachine and Name components.
fn setup_watch_root(net_cmd: &mut MessageWriter<NetCommand>, id: u64) {
    net_cmd.write(NetCommand::StartMachine { id });
    net_cmd.write(NetCommand::StopComponents { id });
    net_cmd.write(NetCommand::StartComponents {
        id,
        components: vec![
            bevy_gearbox_protocol::components::NAME.to_string(),
            bevy_gearbox_protocol::components::STATE_MACHINE.to_string(),
        ],
    });
}

/// Start minimal watch for a state node (currently Name only).
fn setup_watch_state(net_cmd: &mut MessageWriter<NetCommand>, id: u64) {
    net_cmd.write(NetCommand::StartComponents {
        id,
        components: vec![bevy_gearbox_protocol::components::NAME.to_string()],
    });
}

/// Start minimal watch for an edge (currently Name only).
fn setup_watch_edge(net_cmd: &mut MessageWriter<NetCommand>, id: u64) {
    net_cmd.write(NetCommand::StartComponents {
        id,
        components: vec![
            bevy_gearbox_protocol::components::NAME.to_string(),
            bevy_gearbox_protocol::components::DELAY.to_string(),
            bevy_gearbox_protocol::components::EDGE_KIND.to_string(),
        ],
    });
}

fn on_spawn_state_machine(
    evt: On<bevy_gearbox_protocol::events::SpawnStateMachine>,
    client: Res<bevy_gearbox_protocol::client::Client>,
    rt: Res<bevy_gearbox_protocol::client::TokioRuntime>,
    mut client_cmd: MessageWriter<bevy_gearbox_protocol::client::ClientCommand>,
) {
    let name = evt
        .name
        .clone()
        .unwrap_or_else(|| "New State Machine".to_string());
    let client_cloned = client.clone();
    let mut comps: JsonMap<String, JsonValue> = JsonMap::new();
    // Insert StateMachine marker with default value and a Name
    comps.insert(
        bevy_gearbox_protocol::components::STATE_MACHINE.to_string(),
        JsonValue::Object(JsonMap::new()),
    );
    comps.insert(
        bevy_gearbox_protocol::components::NAME.to_string(),
        JsonValue::String(name),
    );
    rt.0.spawn(async move {
        if let Ok(id) = client_cloned.spawn(comps).await {
            // Ask server to instruct the client to open this machine via control channel
            let _ = client_cloned.open_on_client(id).await;
        }
    });
    // Prompt a quick index refresh; discovery will also pick it up shortly
    client_cmd.write(bevy_gearbox_protocol::client::ClientCommand::RefreshMachines);
}

fn on_spawn_substate(
    evt: On<bevy_gearbox_protocol::events::SpawnSubstate>,
    client: Res<bevy_gearbox_protocol::client::Client>,
    rt: Res<bevy_gearbox_protocol::client::TokioRuntime>,
) {
    let parent = evt.parent.to_bits();
    let name = evt.name.clone().unwrap_or_else(|| "New State".to_string());
    let client_cloned = client.clone();
    rt.0.spawn(async move {
        let _ = client_cloned.spawn_substate(parent, Some(&name)).await;
    });
}

fn on_set_edge_delay_requested(
    req: On<crate::editor::actions::SetEdgeDelayRequested>,
    client: Res<bevy_gearbox_protocol::client::Client>,
    rt: Res<bevy_gearbox_protocol::client::TokioRuntime>,
) {
    let id = req.target.0;
    let secs_f32 = req.seconds.max(0.0);
    let secs_u64 = secs_f32.floor() as u64;
    let nanos_u32 = ((secs_f32 - secs_u64 as f32) * 1_000_000_000.0)
        .round()
        .clamp(0.0, 999_999_999.0) as u32;
    let mut duration = JsonMap::new();
    duration.insert(
        "secs".to_string(),
        JsonValue::Number(serde_json::Number::from(secs_u64)),
    );
    duration.insert(
        "nanos".to_string(),
        JsonValue::Number(serde_json::Number::from(nanos_u32)),
    );
    let mut delay_obj = JsonMap::new();
    delay_obj.insert("duration".to_string(), JsonValue::Object(duration));
    let mut comps = JsonMap::new();
    comps.insert(
        bevy_gearbox_protocol::components::DELAY.to_string(),
        JsonValue::Object(delay_obj),
    );
    let client_cloned = client.clone();
    rt.0.spawn(async move {
        let _ = client_cloned.insert_components(id as u64, comps).await;
    });
}

fn on_clear_edge_delay_requested(
    req: On<crate::editor::actions::ClearEdgeDelayRequested>,
    client: Res<bevy_gearbox_protocol::client::Client>,
    rt: Res<bevy_gearbox_protocol::client::TokioRuntime>,
) {
    let id = req.target.0;
    let client_cloned = client.clone();
    rt.0.spawn(async move {
        let _ = client_cloned
            .remove_components(id as u64, &[bevy_gearbox_protocol::components::DELAY])
            .await;
    });
}

fn on_set_edge_kind_requested(
    req: On<crate::editor::actions::SetEdgeKindRequested>,
    client: Res<bevy_gearbox_protocol::client::Client>,
    rt: Res<bevy_gearbox_protocol::client::TokioRuntime>,
) {
    let id = req.target.0;
    let mut comps = JsonMap::new();
    // Use the same shape the server sends back over watch (plain string variant)
    let variant = if req.internal { "Internal" } else { "External" };
    comps.insert(
        bevy_gearbox_protocol::components::EDGE_KIND.to_string(),
        JsonValue::String(variant.to_string()),
    );
    let client_cloned = client.clone();
    rt.0.spawn(async move {
        let _ = client_cloned.insert_components(id as u64, comps).await;
    });
}

fn poll_network(
    mut ui: ResMut<UiState>,
    mut client_msgs: MessageReader<ClientMessage>,
    mut net_msgs: MessageReader<NetMessage>,
    mut net_cmd: MessageWriter<NetCommand>,
    mut client_cmd: MessageWriter<ClientCommand>,
    mut store: ResMut<EditorStore>,
    mut workspace: ResMut<Workspace>,
    mut docs: ResMut<Docs>,
) {
    let mut processed = 0usize;
    const MAX_PER_FRAME: usize = 64;

    // Helper to ensure we are marked as connected if we're receiving traffic
    let mut ensure_connected = |store: &mut EditorStore| {
        if !matches!(store.connection, EditorConnectionState::Connected { .. }) {
            let ep = store
                .last_endpoint
                .clone()
                .unwrap_or_else(|| "http://127.0.0.1:15703".to_string());
            store.connection = EditorConnectionState::Connected {
                session_id: store.session_id,
                endpoint: ep,
            };
        }
    };

    // Handle client responses (e.g., RefreshMachines)
    for msg in client_msgs.read() {
        if processed >= MAX_PER_FRAME {
            break;
        }
        ensure_connected(&mut store);
        match msg {
            ClientMessage::RefreshResult(Ok(list)) => {
                // Update UI cache and editor index
                ui.machines = list
                    .iter()
                    .map(|m| (EntityId(m.id), m.name.clone()))
                    .collect();
                ui.connecting = false;
                ui.error = None;
                store.index.is_loading = false;
                store.index.error = None;
                store.index.items = list
                    .iter()
                    .map(|m| IndexItem {
                        name: m.name.clone(),
                        entity: EntityId(m.id),
                    })
                    .collect();
                // Auto-close any open docs whose entities are no longer in the index
                {
                    let valid: std::collections::HashSet<EntityId> =
                        store.index.items.iter().map(|it| it.entity).collect();
                    let to_close: Vec<EntityId> = docs
                        .map
                        .keys()
                        .copied()
                        .filter(|id| !valid.contains(id))
                        .collect();
                    for id in to_close.into_iter() {
                        close_doc_and_unsubscribe(id, &mut workspace, &mut docs, &mut net_cmd);
                    }
                }
                // Mark connected for UI button logic
                let ep = store
                    .last_endpoint
                    .clone()
                    .unwrap_or_else(|| "http://127.0.0.1:15703".to_string());
                store.connection = EditorConnectionState::Connected {
                    session_id: store.session_id,
                    endpoint: ep,
                };
                // Now that the refresh succeeded, start discovery watch
                net_cmd.write(NetCommand::StartDiscovery);
                // And start control watch for server-initiated editor commands
                net_cmd.write(NetCommand::StartControl);
                processed += 1;
            }
            ClientMessage::RefreshResult(Err(e)) => {
                ui.connecting = false;
                ui.error = Some(e.clone());
                store.index.is_loading = false;
                store.index.error = Some(e.clone());
                store.connection = EditorConnectionState::Disconnected;
                processed += 1;
            }
            ClientMessage::GraphResult { id, graph } => {
                if let Some(sm_graph) = convert_wire_graph_to_state_machine_graph(graph.clone()) {
                    let doc_id = EntityId(*id);
                    // Stash graph snapshot for projection
                    ui.graphs.insert(doc_id, sm_graph.clone());
                    // Begin watches for this machine: transitions/name via machine watch; active via StateMachine component
                    setup_watch_root(&mut net_cmd, *id);
                    // Active visualization will be driven by StateMachine component snapshots
                    // Request sidecar for the machine root
                    client_cmd.write(ClientCommand::SidecarForMachine { id: *id });
                    // Also request sidecars for any substate nodes that declare a StateMachineId
                    let mut requested = 0usize;
                    for (nid, _node) in sm_graph.nodes.iter() {
                        if sm_graph
                            .entity_data
                            .get(nid)
                            .map(|bag| bag.contains(c::STATE_MACHINE_ID))
                            .unwrap_or(false)
                        {
                            client_cmd.write(ClientCommand::SidecarForMachine { id: nid.0 });
                            requested += 1;
                        }
                    }
                }
            }
            ClientMessage::SidecarFound { id, text } => {
                let doc_id = EntityId(*id);
                ui.sidecar_texts.insert(doc_id, text.clone());
                processed += 1;
            }
            ClientMessage::SidecarMissing { .. } => {
                processed += 1;
            }
            ClientMessage::EventEdgeVariants { variants } => {
                workspace.available_event_edges = variants.clone();
                processed += 1;
            }
        }
    }
    // Handle net watch messages (discovery, machine deltas)
    for msg in net_msgs.read() {
        if processed >= MAX_PER_FRAME {
            break;
        }
        ensure_connected(&mut store);
        match msg.clone() {
            NetMessage::Discovery(batch) => {
                for m in batch.into_iter() {
                    if let Some(name) = m.name.clone() {
                        if let Some(ix) = ui.machines.iter_mut().position(|(id, _)| id.0 == m.id) {
                            ui.machines[ix] = (EntityId(m.id), Some(name));
                        } else {
                            ui.machines.push((EntityId(m.id), Some(name)));
                        }
                    } else {
                        ui.machines.retain(|(id, _)| id.0 != m.id);
                    }
                }
                ui.machines.sort_by_key(|(id, _)| id.0);
                store.index.items = ui
                    .machines
                    .iter()
                    .map(|(id, name)| IndexItem {
                        name: name.clone(),
                        entity: *id,
                    })
                    .collect();
                // Auto-close any open docs whose entities are no longer in the index
                {
                    let valid: std::collections::HashSet<EntityId> =
                        store.index.items.iter().map(|it| it.entity).collect();
                    let to_close: Vec<EntityId> = docs
                        .map
                        .keys()
                        .copied()
                        .filter(|id| !valid.contains(id))
                        .collect();
                    for id in to_close.into_iter() {
                        close_doc_and_unsubscribe(id, &mut workspace, &mut docs, &mut net_cmd);
                    }
                }
                processed += 1;
            }
            NetMessage::ControlOpen { id } => {
                let entity = EntityId(id);
                // Ensure a doc entry exists and seed its transform
                let doc = docs.map.entry(entity).or_default();
                doc.transform.pan = workspace.board_transform.pan;
                doc.transform.zoom = workspace.board_transform.zoom;
                // Begin watches and request graph snapshot
                setup_watch_root(&mut net_cmd, id);
                client_cmd.write(ClientCommand::FetchGraph { id });
                processed += 1;
            }
            NetMessage::Machine { id, events } => {
                let doc_id = EntityId(id);
                // Update last seqs and stash events
                let mut max_t = ui.last_transition_seq.get(&doc_id).copied().unwrap_or(0);
                for ev in events.iter() {
                    let seq = ev.get("seq").and_then(|v| v.as_u64()).unwrap_or(0);
                    let kind = ev.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                    if kind == "transition_edge"
                        || kind == "state_entered"
                        || kind == "state_exited"
                    {
                        if seq > max_t {
                            max_t = seq;
                        }
                    }
                }
                ui.last_transition_seq.insert(doc_id, max_t);
                ui.pending_machine_events
                    .entry(doc_id)
                    .or_default()
                    .extend(events.into_iter());
                processed += 1;
            }
            NetMessage::Components {
                id,
                components,
                removed,
            } => {
                // Apply Name changes to any open doc containing this entity
                let target = EntityId(id);
                let name_key = bevy_gearbox_protocol::components::NAME;
                let name_opt = components
                    .get(name_key)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                // Debug: log entity name and full packet contents from watch
                {
                    let entity_label = name_opt.clone().unwrap_or_else(|| id.to_string());
                    let packet = serde_json::json!({
                        "id": id,
                        "components": components.clone(),
                        "removed": removed.clone(),
                    });
                }
                // Drive active visualization from StateMachine component snapshots on the machine root
                let sm_key = bevy_gearbox_protocol::components::STATE_MACHINE;
                let mut pending_sm_active: Option<(Vec<EntityId>, Vec<EntityId>)> = None;
                if let Some(sm_val) = components.get(sm_key) {
                    if let Some(obj) = sm_val.as_object() {
                        let parse_set = |k: &str| -> Vec<EntityId> {
                            obj.get(k)
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|x| {
                                            x.as_u64().or_else(|| {
                                                x.as_str().and_then(|s| s.parse::<u64>().ok())
                                            })
                                        })
                                        .map(EntityId)
                                        .collect::<Vec<EntityId>>()
                                })
                                .unwrap_or_default()
                        };
                        let act = parse_set("active");
                        let leaves = parse_set("active_leaves");
                        pending_sm_active = Some((act, leaves));
                    }
                    // Ensure a doc exists for this machine root; create minimal graph if missing
                    let root_id = EntityId(id);
                    let entry = docs.map.entry(root_id).or_default();
                    if entry.graph.is_none() {
                        let node = crate::model::StateNode::new(root_id);
                        entry.graph = Some(crate::model::StateMachineGraph::new(node));
                    }
                } else if removed.iter().any(|k| k == sm_key) {
                    // If StateMachine was removed, treat as empty set (unlikely for a machine root)
                    pending_sm_active = Some((Vec::new(), Vec::new()));
                }
                for (_doc_id, doc) in docs.map.iter_mut() {
                    // Update central per-entity component store on the graph if present
                    if let Some(g) = doc.graph.as_mut() {
                        let bag = g.entity_data.entry(target).or_default();
                        for (k, v) in components.iter() {
                            bag.insert(ComponentEntry::new(k.clone(), v.clone()));
                        }
                        for key in removed.iter() {
                            let _ = bag.remove(key);
                        }
                    }
                    if let Some(v) = doc.scene.states.get_mut(&target) {
                        if let Some(ref name) = name_opt {
                            v.label = name.clone();
                        }
                    }
                    if let Some(v) = doc.scene.edges.get_mut(&target) {
                        if let Some(ref name) = name_opt {
                            v.label = name.clone();
                        }
                    }
                    if let Some(g) = doc.graph.as_mut() {
                        if let Some(e) = g.edges.get_mut(&target) {
                            if let Some(ref name) = name_opt {
                                e.display_label = Some(name.clone());
                            }
                        }
                    }
                }
                // Apply StateMachine.active snapshot immediately to the doc's graph with flash/fade
                if let Some((act, _leaves)) = pending_sm_active {
                    let new: std::collections::HashSet<EntityId> = act.into_iter().collect();
                    let doc_id = EntityId(id);
                    if let Some(doc) = docs.map.get_mut(&doc_id) {
                        let prev: std::collections::HashSet<EntityId> = doc
                            .graph
                            .as_ref()
                            .map(|g| g.active_nodes.clone())
                            .unwrap_or_default();
                        let mut added: Vec<EntityId> = Vec::new();
                        let mut removed: Vec<EntityId> = Vec::new();
                        for a in new.iter() {
                            if !prev.contains(a) {
                                added.push(*a);
                            }
                        }
                        for p in prev.iter() {
                            if !new.contains(p) {
                                removed.push(*p);
                            }
                        }
                        for u in added.into_iter() {
                            doc.node_flash.insert(u, 1.0);
                        }
                        for u in removed.into_iter() {
                            doc.node_fade.insert(u, 1.0);
                        }
                        if let Some(g) = doc.graph.as_mut() {
                            g.set_active(new.clone().into_iter());
                        }
                    }
                }
                // Late-bound StateMachineId: if an ID arrives for an entity present in any open doc, fetch its sidecar
                {
                    let smid_key = bevy_gearbox_protocol::components::STATE_MACHINE_ID;
                    if components.contains_key(smid_key) {
                        let present_in_any_doc = docs.map.values().any(|doc| {
                            if let Some(g) = doc.graph.as_ref() {
                                g.nodes.contains_key(&target)
                            } else {
                                false
                            }
                        });
                        if present_in_any_doc {
                            client_cmd.write(ClientCommand::SidecarForMachine { id });
                        }
                    }
                }
                processed += 1;
            }
        }
    }
    // Drain any pending explicit fetch requests enqueued by UI actions
    if !workspace.pending_fetch_docs.is_empty() {
        let docs: Vec<EntityId> = std::mem::take(&mut workspace.pending_fetch_docs);
        for d in docs.into_iter() {
            client_cmd.write(ClientCommand::FetchGraph { id: d.0 });
        }
    }
}

fn ui_system(
    mut egui_ctx: EguiContexts,
    mut store: ResMut<EditorStore>,
    mut commands: Commands,
    mut workspace: ResMut<Workspace>,
    mut docs: ResMut<Docs>,
) {
    if let Ok(ctx) = egui_ctx.ctx_mut() {
        egui::CentralPanel::default().show(ctx, |ui_egui| {
            crate::editor::shell::layout::draw(
                ui_egui,
                &mut store,
                &mut commands,
                &mut workspace,
                &mut docs,
            );
        });
    }
}

fn sync_snapshots_to_workspace(
    mut docs: ResMut<Docs>,
    mut ui: ResMut<UiState>,
    mut net_cmd: MessageWriter<NetCommand>,
) {
    let mut consume_sidecar_for: Vec<EntityId> = Vec::new();
    // Apply machine event batches (canonicalize ids before applying)
    // Flash edges in any open doc that contains the edge (root or substates)
    let pending_events = std::mem::take(&mut ui.pending_machine_events);
    let mut edges_to_flash: Vec<EntityId> = Vec::new();
    let mut states_to_flash: Vec<EntityId> = Vec::new();
    let mut states_to_fade: Vec<EntityId> = Vec::new();

    for (_id, events) in pending_events.into_iter() {
        for ev in events.into_iter() {
            let kind = ev.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            match kind {
                "transition_edge" => {
                    let edge_raw = ev.get("edge").and_then(|v| {
                        v.as_u64()
                            .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
                    });
                    if let Some(edge) = edge_raw {
                        let edge = crate::util::canonicalize_entity_u64(edge);
                        edges_to_flash.push(EntityId(edge));
                    }
                }
                "state_entered" => {
                    let entity_raw = ev.get("entity").and_then(|v| {
                        v.as_u64()
                            .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
                    });
                    if let Some(entity) = entity_raw {
                        let entity = crate::util::canonicalize_entity_u64(entity);
                        states_to_flash.push(EntityId(entity));
                    }
                }
                "state_exited" => {
                    let entity_raw = ev.get("entity").and_then(|v| {
                        v.as_u64()
                            .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
                    });
                    if let Some(entity) = entity_raw {
                        let entity = crate::util::canonicalize_entity_u64(entity);
                        states_to_fade.push(EntityId(entity));
                    }
                }
                _ => {}
            }
        }
    }

    for (_doc_id, doc) in docs.map.iter_mut() {
        for eid in edges_to_flash.iter().copied() {
            if doc.scene.edges.contains_key(&eid) {
                doc.flash_edge(eid);
            }
        }
        for sid in states_to_flash.iter().copied() {
            if doc.scene.states.contains_key(&sid) {
                doc.node_flash.insert(sid, 1.0);
            }
        }
        for sid in states_to_fade.iter().copied() {
            if doc.scene.states.contains_key(&sid) {
                doc.node_fade.insert(sid, 1.0);
            }
        }
    }
    // Drain snapshot inbox: apply once, then clear from UiState
    let mut to_remove: Vec<EntityId> = Vec::new();
    for (id, graph) in ui.graphs.iter() {
        // Capture metrics before taking a mutable borrow of docs entry
        let was_empty = docs.map.get(id).and_then(|d| d.graph.as_ref()).is_none();
        let entry = docs.map.entry(*id).or_default();
        let prev_active = entry.graph.as_ref().map(|g| g.active_nodes.clone());
        project_graph_into_doc(entry, graph.clone());
        // After projecting the graph, start a Name component watch on all nodes and edges
        if let Some(g) = entry.graph.as_ref() {
            for nid in g.nodes.keys() {
                setup_watch_state(&mut net_cmd, nid.0);
            }
            for eid in g.edges.keys() {
                // Re-arm edge component watch with the extended set (Name, Delay, EdgeKind)
                net_cmd.write(NetCommand::StopComponents { id: eid.0 });
                setup_watch_edge(&mut net_cmd, eid.0);
            }
        }
        // Reapply previous active set from old graph so colors persist
        if let (Some(prev), Some(g)) = (prev_active, entry.graph.as_mut()) {
            g.set_active(prev.into_iter());
        }
        // After mutation, avoid borrowing workspace again; use the entry we have
        to_remove.push(*id);
        // Try applying sidecar when: (a) first load, or (b) new sidecar text arrived
        let mut applied = false;
        if let Some(text) = ui.sidecar_texts.get(id) {
            match parse_sidecar_text(text) {
                Ok(sc) => {
                    apply_sidecar_to_doc(entry, &sc);
                    applied = true;
                }
                Err(e) => (),
            }
            // mark for single-consume once attempted (avoid re-applying every frame)
            consume_sidecar_for.push(*id);
        }
        if !applied && was_empty {
            // Fallbacks for local disk resolution for convenience when app and editor share filesystem
            if let Some(id_text) = graph
                .entity_data
                .get(&graph.root)
                .and_then(|bag| bag.get(c::STATE_MACHINE_ID))
                .and_then(|e| e.value_json.as_str())
            {
                // Derive file name from id
                let ptr_str = format!("{}.sm.ron", id_text);
                let mut tried: Vec<std::path::PathBuf> = Vec::new();
                let candidate_direct = std::path::PathBuf::from(&ptr_str);
                tried.push(candidate_direct.clone());
                let candidate_assets = std::path::PathBuf::from("assets").join(&ptr_str);
                tried.push(candidate_assets.clone());
                for p in tried {
                    if p.exists() {
                        if let Ok(sc) = load_sidecar(&p) {
                            apply_sidecar_to_doc(entry, &sc);
                            applied = true;
                            break;
                        }
                    }
                }
            }
            if !applied {
                // As a final fallback when no sidecar is found anywhere, ensure a derived default layout
                // is applied so the editor shows states/edges at reasonable default positions.
                if entry.graph.is_some() && entry.scene.node_rects.is_empty() {
                    project_graph_into_doc(entry, graph.clone());
                }
            }
        }
    }
    // Remove consumed snapshots
    for id in to_remove {
        ui.graphs.remove(&id);
    }
    // Apply any sidecars that arrived independently of new snapshots (decoupled from inbox)
    // Only apply if the doc already has a graph to target
    let extra_sidecars: Vec<(EntityId, String)> = ui
        .sidecar_texts
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    for (target_entity, text) in extra_sidecars.iter() {
        for (doc_id, doc) in docs.map.iter_mut() {
            if doc.graph.is_none() {
                continue;
            }
            let graph = doc.graph.as_ref().unwrap();
            // If this sidecar targets the doc root, apply whole-doc overlay; otherwise, apply to subtree if present
            if graph.nodes.get(&graph.root).is_some() {
                let is_doc_root = graph.root.0 == target_entity.0;
                let mut applied_here = false;
                if is_doc_root {
                    if let Ok(sc) = parse_sidecar_text(text) {
                        apply_sidecar_to_doc(doc, &sc);
                        applied_here = true;
                    }
                } else {
                    // Check if the target entity exists as a node in this doc
                    let target_id = *target_entity;
                    if graph.nodes.contains_key(&target_id) {
                        if let Ok(sc) = parse_sidecar_text(text) {
                            crate::persistence::apply_sidecar_to_subtree(doc, &sc, &target_id);
                            applied_here = true;
                        }
                    }
                }
                if applied_here {
                    consume_sidecar_for.push(*target_entity);
                }
            }
        }
    }
    // Now consume fetched sidecar texts
    for id in consume_sidecar_for {
        ui.sidecar_texts.remove(&id);
    }
}

fn convert_wire_graph_to_state_machine_graph(
    graph: serde_json::Value,
) -> Option<StateMachineGraph> {
    use crate::model as m;
    use crate::types::EntityId;
    let root_s = graph.get("root").and_then(|v| v.as_str())?;
    let root_u = root_s.parse::<u64>().ok()?;
    let mut out = m::StateMachineGraph::new(m::StateNode::new(EntityId(root_u)));
    // Nodes
    if let Some(nodes) = graph.get("nodes").and_then(|v| v.as_array()) {
        for n in nodes.iter() {
            let id_u = n
                .get("id")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())?;
            let id = EntityId(id_u);
            let node = out
                .nodes
                .remove(&id)
                .unwrap_or_else(|| m::StateNode::new(id));
            if let Some(comps) = n.get("components").and_then(|v| v.as_object()) {
                // Build adjacency from relationships if provided
                if let Some(out_v) = comps
                    .get(bevy_gearbox_protocol::components::TRANSITIONS)
                    .and_then(|v| v.as_array())
                {
                    let mut outs: Vec<EntityId> = Vec::new();
                    for s in out_v.iter().filter_map(|vv| vv.as_str()) {
                        if let Ok(u) = s.parse::<u64>() {
                            outs.push(EntityId(u));
                        }
                    }
                    if !outs.is_empty() {
                        out.adjacency_out.insert(id, outs);
                    }
                }
                if let Some(in_v) = comps
                    .get(bevy_gearbox_protocol::components::TARGETED_BY)
                    .and_then(|v| v.as_array())
                {
                    let mut ins: Vec<EntityId> = Vec::new();
                    for s in in_v.iter().filter_map(|vv| vv.as_str()) {
                        if let Ok(u) = s.parse::<u64>() {
                            ins.push(EntityId(u));
                        }
                    }
                    if !ins.is_empty() {
                        out.adjacency_in.insert(id, ins);
                    }
                }
            }
            out.nodes.insert(id, node);
            // Seed central component store for this node
            if let Some(comps) = n.get("components").and_then(|v| v.as_object()) {
                let mut bag = m::ComponentBag::default();
                for (k, v) in comps.iter() {
                    bag.insert(m::ComponentEntry::new(k.clone(), v.clone()));
                }
                out.entity_data.insert(id, bag);
            }
        }
    }
    // Edges
    if let Some(edges) = graph.get("edges").and_then(|v| v.as_array()) {
        for e in edges.iter() {
            let id_u = e
                .get("id")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())?;
            let src_u = e
                .get("source")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())?;
            let tgt_u = e
                .get("target")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            if tgt_u == 0 {
                continue;
            }
            let id = EntityId(id_u);
            let src = EntityId(src_u);
            let tgt = EntityId(tgt_u);
            let mut edge = m::Edge::new(id, src, tgt);
            if let Some(comps) = e.get("components").and_then(|v| v.as_object()) {
                for (k, v) in comps.iter() {
                    edge.components
                        .insert(m::ComponentEntry::new(k.clone(), v.clone()));
                }
                // Derive and store a stable display label so sidecar edge keys can match
                edge.display_label = Some(crate::model::choose_edge_label_bag(&edge.components));
            }
            out.edges.insert(id, edge);
            // Seed central component store for this edge
            if let Some(comps) = e.get("components").and_then(|v| v.as_object()) {
                let mut bag = m::ComponentBag::default();
                for (k, v) in comps.iter() {
                    bag.insert(m::ComponentEntry::new(k.clone(), v.clone()));
                }
                out.entity_data.insert(id, bag);
            }
        }
    }
    Some(out)
}
