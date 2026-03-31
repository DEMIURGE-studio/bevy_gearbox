#![cfg(feature = "client")]
use bevy::prelude::*;
use serde_json::{json, Map, Value};
use std::sync::Arc;

use crate::methods::*;
use crate::components as wire;

#[derive(Resource, Clone)]
pub struct ClientConfig { pub url: String }

#[derive(Debug)]
pub enum Error {
    Http(reqwest::Error),
    Json(serde_json::Error),
    Rpc { code: i64, message: String, data: Option<Value> },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Http(e) => write!(f, "http: {}", e),
            Error::Json(e) => write!(f, "json: {}", e),
            Error::Rpc { code, message, .. } => write!(f, "rpc {}: {}", code, message),
        }
    }
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error { fn from(e: reqwest::Error) -> Self { Error::Http(e) } }
impl From<serde_json::Error> for Error { fn from(e: serde_json::Error) -> Self { Error::Json(e) } }

#[derive(Resource, Clone)]
pub struct Client {
    pub base_url: String,
    http: reqwest::Client,
}

impl Client {
    pub fn new(base_url: String) -> Self {
        Self { base_url, http: reqwest::Client::new() }
    }

    pub async fn registry_schema(&self) -> Result<Value, Error> {
        let v = self.jsonrpc_call(crate::methods::REGISTRY_SCHEMA, None).await?;
        Ok(v.get("result").cloned().unwrap_or(v))
    }

    async fn jsonrpc_call(&self, method: &str, params: Option<Value>) -> Result<Value, Error> {
        let id = 1u64;
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let resp = self.http.post(&self.base_url).json(&body).send().await?;
        let status = resp.status();
        let v: Value = resp.json().await?;
        if let Some(err) = v.get("error") {
            let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(-32000);
            let message = err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error").to_string();
            let data = err.get("data").cloned();
            return Err(Error::Rpc { code, message, data });
        }
        if !status.is_success() {
            // best-effort message
            return Err(Error::Rpc { code: status.as_u16() as i64, message: "http status".to_string(), data: Some(v) });
        }
        Ok(v)
    }

    // ===============
    // Thin world.* helpers (direct wrappers)
    // ===============

    pub async fn get_components(
        &self,
        entity: u64,
        components: Option<&[&str]>,
    ) -> Result<Map<String, Value>, Error> {
        let params = match components {
            Some(list) => json!({"entity": entity, "components": list}),
            None => json!({"entity": entity}),
        };
        let v = self.jsonrpc_call(WORLD_GET_COMPONENTS, Some(params)).await?;
        // Accept common BRP result shapes: {result:{components:{...}}} or {result:{...}} or direct object
        if let Some(obj) = v.get("result").and_then(|r| r.get("components")).and_then(|c| c.as_object()) {
            return Ok(obj.clone());
        }
        if let Some(obj) = v.get("result").and_then(|r| r.as_object()) {
            return Ok(obj.clone());
        }
        if let Some(obj) = v.as_object() {
            return Ok(obj.clone());
        }
        Err(Error::Rpc { code: -32603, message: "unexpected response for world.get_components".to_string(), data: Some(v) })
    }

    pub async fn insert_components(
        &self,
        entity: u64,
        components: Map<String, Value>,
    ) -> Result<(), Error> {
        let params = json!({"entity": entity, "components": components});
        let _ = self.jsonrpc_call(WORLD_INSERT_COMPONENTS, Some(params)).await?;
        Ok(())
    }

    pub async fn remove_components(
        &self,
        entity: u64,
        keys: &[&str],
    ) -> Result<(), Error> {
        let params = json!({"entity": entity, "components": keys});
        let _ = self.jsonrpc_call(WORLD_REMOVE_COMPONENTS, Some(params)).await?;
        Ok(())
    }

	pub async fn spawn(&self, components: Map<String, Value>) -> Result<u64, Error> {
		let params = json!({"components": components});
		let v = self.jsonrpc_call(WORLD_SPAWN, Some(params)).await?;
		if let Some(id) = v.get("result").and_then(|r| r.get("entity")).and_then(|n| n.as_u64()) { return Ok(id); }
		if let Some(id) = v.get("result").and_then(|n| n.as_u64()) { return Ok(id); }
		if let Some(id) = v.get("entity").and_then(|n| n.as_u64()) { return Ok(id); }
		if let Some(id) = v.as_u64() { return Ok(id); }
		Err(Error::Rpc { code: -32603, message: "unexpected response for world.spawn".to_string(), data: Some(v) })
	}

	pub async fn despawn(&self, entity: u64) -> Result<(), Error> {
		let params = json!({"entity": entity});
		let _ = self.jsonrpc_call(WORLD_DESPAWN, Some(params)).await?;
		Ok(())
	}

	pub async fn reset_region(&self, root: u64) -> Result<(), Error> {
		let params = json!({"root": root});
		let _ = self.jsonrpc_call(EDITOR_RESET_REGION, Some(params)).await?;
		Ok(())
	}

	// ===============
	// Query and file helpers used by editor flows
	// ===============

	pub async fn world_query(&self, data: Value, filter: Value, strict: bool) -> Result<Vec<Value>, Error> {
		let params = json!({"data": data, "filter": filter, "strict": strict});
		let v = self.jsonrpc_call(WORLD_QUERY, Some(params)).await?;
		if let Some(arr) = v.get("result").and_then(|r| r.as_array()) { return Ok(arr.clone()); }
		if let Some(arr) = v.as_array() { return Ok(arr.clone()); }
		Err(Error::Rpc { code: -32603, message: "unexpected response for world.query".to_string(), data: Some(v) })
	}

	pub async fn save_graph(&self, entity: u64, path: &str) -> Result<(), Error> {
		let params = json!({"entity": entity, "path": path});
		let _ = self.jsonrpc_call(EDITOR_SAVE_GRAPH, Some(params)).await?;
		Ok(())
	}

    pub async fn save_as(&self, entity: u64, path: &str) -> Result<String, Error> {
        let params = json!({"entity": entity, "path": path});
        let v = self.jsonrpc_call("editor.save_as", Some(params)).await?;
        let p = v.get("result").and_then(|r| r.get("path")).and_then(|s| s.as_str()).unwrap_or("").to_string();
        Ok(p)
    }

    pub async fn save_substates(&self, entity: u64) -> Result<serde_json::Value, Error> {
        let params = json!({"entity": entity});
        let v = self.jsonrpc_call("editor.save_substates", Some(params)).await?;
        Ok(v.get("result").cloned().unwrap_or(v))
    }

	pub async fn save_sidecar(&self, path: &str, contents: &str) -> Result<(), Error> {
		let params = json!({"path": path, "contents": contents});
		let _ = self.jsonrpc_call(EDITOR_SAVE_SIDECAR, Some(params)).await?;
		Ok(())
	}

	pub async fn load_sidecar(&self, path: &str) -> Result<Option<String>, Error> {
		let params = json!({"path": path});
		let v = self.jsonrpc_call(EDITOR_LOAD_SIDECAR, Some(params)).await?;
		Ok(v.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()))
	}

	pub async fn find_sidecar_by_fingerprint(&self, fp: &str) -> Result<Option<String>, Error> {
		let params = json!({"fp": fp});
		let v = self.jsonrpc_call(EDITOR_FIND_SIDECAR_BY_FINGERPRINT, Some(params)).await?;
		Ok(v.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()))
	}

	pub async fn sidecar_for_machine(&self, entity: u64) -> Result<Option<String>, Error> {
		let params = json!({"entity": entity});
		let v = self.jsonrpc_call(EDITOR_SIDECAR_FOR_MACHINE, Some(params)).await?;
		// Accept {result:{text}} or top-level {text}
		if let Some(s) = v.get("result").and_then(|r| r.get("text")).and_then(|t| t.as_str()) { return Ok(Some(s.to_string())); }
		Ok(v.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()))
	}

	pub async fn set_state_machine_id(&self, entity: u64, path: &str) -> Result<(), Error> {
		let params = json!({"entity": entity, "path": path});
		let _ = self.jsonrpc_call(EDITOR_SET_STATE_MACHINE_ID, Some(params)).await?;
		Ok(())
	}

    pub async fn spawn_substate(&self, parent: u64, name: Option<&str>) -> Result<u64, Error> {
        let params = match name {
            Some(n) => json!({"parent": parent, "name": n}),
            None => json!({"parent": parent}),
        };
        let v = self.jsonrpc_call(crate::methods::EDITOR_SPAWN_SUBSTATE, Some(params)).await?;
        if let Some(id) = v.get("result").and_then(|r| r.get("entity")).and_then(|n| n.as_u64()) { return Ok(id); }
        if let Some(id) = v.get("entity").and_then(|n| n.as_u64()) { return Ok(id); }
        Err(Error::Rpc { code: -32603, message: "unexpected response for editor.spawn_substate".to_string(), data: Some(v) })
    }

    pub async fn delete_subtree(&self, root: u64) -> Result<(), Error> {
        let params = json!({"root": root});
        let _ = self.jsonrpc_call(crate::methods::EDITOR_DELETE_SUBTREE, Some(params)).await?;
        Ok(())
    }

    pub async fn make_leaf(&self, target: u64) -> Result<(), Error> {
        let params = json!({"target": target});
        let _ = self.jsonrpc_call(crate::methods::EDITOR_MAKE_LEAF, Some(params)).await?;
        Ok(())
    }

    pub async fn make_parent(&self, target: u64) -> Result<(), Error> {
        let params = json!({"target": target});
        let _ = self.jsonrpc_call(crate::methods::EDITOR_MAKE_PARENT, Some(params)).await?;
        Ok(())
    }

    pub async fn make_parallel(&self, target: u64) -> Result<(), Error> {
        let params = json!({"target": target});
        let _ = self.jsonrpc_call(crate::methods::EDITOR_MAKE_PARALLEL, Some(params)).await?;
        Ok(())
    }

    pub async fn create_transition(&self, source: u64, target: u64, kind: &str) -> Result<u64, Error> {
        let params = json!({"source": source, "target": target, "kind": kind});
        let v = self.jsonrpc_call(crate::methods::EDITOR_CREATE_TRANSITION, Some(params)).await?;
        if let Some(id) = v.get("result").and_then(|r| r.get("entity")).and_then(|n| n.as_u64()) { return Ok(id); }
        if let Some(id) = v.get("entity").and_then(|n| n.as_u64()) { return Ok(id); }
        Err(Error::Rpc { code: -32603, message: "unexpected response for editor.create_transition".to_string(), data: Some(v) })
    }

    pub async fn rename(&self, entity: u64, name: &str) -> Result<(), Error> {
        let params = json!({ "entity": entity, "components": { wire::NAME: name } });
        let _ = self.jsonrpc_call(WORLD_INSERT_COMPONENTS, Some(params)).await?;
        Ok(())
    }

    pub async fn version(&self) -> Result<u32, Error> {
        let v = self.jsonrpc_call(PROTOCOL_VERSION, None).await?;
        Ok(v.get("result").and_then(|r| r.get("version")).and_then(|n| n.as_u64()).unwrap_or(0) as u32)
    }

    pub async fn open_on_client(&self, entity: u64) -> Result<(), Error> {
        let params = json!({"entity": entity});
        let _ = self.jsonrpc_call("editor.open_on_client", Some(params)).await?;
        Ok(())
    }

    pub async fn open_if_related(&self, target: u64, related: u64) -> Result<(), Error> {
        let params = json!({"target": target, "related": related});
        let _ = self.jsonrpc_call("editor.open_if_related", Some(params)).await?;
        Ok(())
    }
}

pub fn on_rename(rename: On<crate::events::Rename>, client: Res<Client>, rt: Res<TokioRuntime>) {
    // fire-and-forget via Tokio runtime; watches ensure convergence
    let name = rename.name.clone();
    let id = rename.target.to_bits();
    let client_cloned = client.clone();
    let rt = rt.0.clone();
    rt.spawn(async move {
        let _ = client_cloned.rename(id, &name).await;
    });
}

pub fn on_despawn(despawn: On<crate::events::Despawn>, client: Res<Client>, rt: Res<TokioRuntime>) {
    let id = despawn.target.to_bits();
    let client_cloned = client.clone();
    let rt = rt.0.clone();
    rt.spawn(async move {
        let _ =client_cloned.despawn(id).await;
    });
}

pub fn on_reset_region(reset_region: On<crate::events::ResetRegion>, client: Res<Client>, rt: Res<TokioRuntime>) {
    let root = reset_region.target.to_bits();
    let client_cloned = client.clone();
    let rt = rt.0.clone();
    rt.spawn(async move {
        let _ = client_cloned.reset_region(root).await;
    });
}

pub fn on_create_transition(
    ev: On<crate::events::CreateTransition>,
    client: Res<Client>,
    rt: Res<TokioRuntime>,
    mut writer: MessageWriter<ClientCommand>,
) {
    let machine = ev.machine.to_bits();
    let source = ev.source.to_bits();
    let target = ev.target.to_bits();
    let kind = ev.kind.clone();
    let client_cloned = client.clone();
    let rt = rt.0.clone();
    // Fire RPC then request a graph refresh for the machine root
    rt.spawn(async move {
        let _ = client_cloned.create_transition(source, target, &kind).await;
    });
    writer.write(ClientCommand::FetchGraph { id: machine });
}

pub fn on_change_node_type(
    ev: On<crate::events::ChangeNodeType>,
    client: Res<Client>,
    rt: Res<TokioRuntime>,
) {
    let target = ev.target.to_bits();
    let to = ev.to;
    let client_cloned = client.clone();
    let rt = rt.0.clone();
    rt.spawn(async move {
        match to {
            crate::events::NodeType::Leaf => { let _ = client_cloned.make_leaf(target).await; }
            crate::events::NodeType::Parent => { let _ = client_cloned.make_parent(target).await; }
            crate::events::NodeType::Parallel => { let _ = client_cloned.make_parallel(target).await; }
        }
    });
}

// =========================
// High-level client commands/messages (event-based API)
// =========================

#[derive(Message)]
pub enum ClientCommand {
    RefreshMachines,
    SetUrl { url: String },
    FetchGraph { id: u64 },
    LoadSidecarByPath { id: u64, path: String },
    SidecarForMachine { id: u64 },
}

#[derive(Message, Clone)]
pub enum ClientMessage {
    RefreshResult(Result<Vec<MachineSummary>, String>),
    GraphResult { id: u64, graph: serde_json::Value },
    SidecarFound { id: u64, text: String },
    SidecarMissing { id: u64 },
    EventEdgeVariants { variants: Vec<String> },
}

async fn list_state_machines(client: &Client) -> Result<Vec<MachineSummary>, String> {
    let rows = client
        .world_query(serde_json::json!({}), serde_json::json!({"with":[crate::components::STATE_MACHINE]}), false)
        .await
        .map_err(|e| e.to_string())?;
    let mut out: Vec<MachineSummary> = Vec::new();
    for row in rows.into_iter() {
        if let Some(id) = row.get("entity").and_then(|e| e.as_u64()) {
            let comps = client
                .get_components(id, Some(&[crate::components::NAME]))
                .await
                .unwrap_or_default();
            let name = comps.get(crate::components::NAME).and_then(|v| v.as_str()).map(|s| s.to_string());
            out.push(MachineSummary { id, name });
        }
    }
    Ok(out)
}

fn client_commands(
    mut reader: MessageReader<ClientCommand>,
    rt: Res<TokioRuntime>,
    mut client: ResMut<Client>,
    mut writer: MessageWriter<ClientMessage>,
    mut conn: ResMut<ConnectionState>,
) {
    for cmd in reader.read() {
        match *cmd {
            ClientCommand::RefreshMachines => {
                let client_cloned = client.clone();
                let r = rt.0.block_on(async move { list_state_machines(&client_cloned).await });
                writer.write(ClientMessage::RefreshResult(r));
            }
            ClientCommand::SetUrl { ref url } => {
                client.base_url = url.clone();
                conn.state = ConnectionPhase::Connecting;
                conn.endpoint = Some(url.clone());
                // Probe connectivity via protocol.version
                let client_cloned = client.clone();
                let ver = rt.0.block_on(async move { client_cloned.version().await });
                match ver {
                    Ok(_) => {
                        conn.state = ConnectionPhase::Connected;
                        // Fetch registry schema and publish discovered EventEdge<T> variants
                        let client_cloned2 = client.clone();
                        if let Ok(schema) = rt.0.block_on(async move { client_cloned2.registry_schema().await }) {
                            let variants = extract_event_edge_variants(&schema);
                            if !variants.is_empty() { writer.write(ClientMessage::EventEdgeVariants { variants }); }
                        }
                    }
                    Err(_) => { conn.state = ConnectionPhase::Disconnected; }
                }
            }
            ClientCommand::FetchGraph { id } => {
                let client_cloned = client.clone();
                let v = rt.0.block_on(async move {
                    client_cloned.jsonrpc_call(EDITOR_MACHINE_GRAPH, Some(serde_json::json!({"entity": id}))).await
                });
                if let Ok(v) = v {
                    let graph = v.get("result").cloned().unwrap_or(v);
                    writer.write(ClientMessage::GraphResult { id, graph });
                } else if let Err(e) = v {
                    writer.write(ClientMessage::GraphResult { id, graph: serde_json::json!({"error": e.to_string()}) });
                }
            }
            ClientCommand::LoadSidecarByPath { id, ref path } => {
                let client_cloned = client.clone();
                let path_for_call = path.clone();
                let r = rt.0.block_on(async move { client_cloned.load_sidecar(&path_for_call).await });
                match r {
                    Ok(Some(text)) => { let _ = writer.write(ClientMessage::SidecarFound { id, text }); }
                    Ok(None) => { let _ = writer.write(ClientMessage::SidecarMissing { id }); }
                    Err(_e) => { let _ = writer.write(ClientMessage::SidecarMissing { id }); }
                }
            }
            ClientCommand::SidecarForMachine { id } => {
                let client_cloned = client.clone();
                let r = rt.0.block_on(async move { client_cloned.sidecar_for_machine(id).await });
                match r {
                    Ok(Some(text)) => { let _ = writer.write(ClientMessage::SidecarFound { id, text }); }
                    Ok(None) => { let _ = writer.write(ClientMessage::SidecarMissing { id }); }
                    Err(_e) => { let _ = writer.write(ClientMessage::SidecarMissing { id }); }
                }
            }
        }
    }
}

#[derive(Default)]
pub struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        if app.world().get_resource::<ClientConfig>().is_none() {
            let url = std::env::var("GEARBOX_PROTOCOL_URL").or_else(|_| std::env::var("BRP_URL")).unwrap_or_else(|_| "http://127.0.0.1:15703".to_string());
            app.insert_resource(ClientConfig { url: url.clone() });
            app.insert_resource(Client::new(url));
        } else if app.world().get_resource::<Client>().is_none() {
            let url = app.world().get_resource::<ClientConfig>().unwrap().url.clone();
            app.insert_resource(Client::new(url));
        }
        if app.world().get_resource::<TokioRuntime>().is_none() {
            let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
            app.insert_resource(TokioRuntime(Arc::new(rt)));
        }
        app.init_resource::<WatchManager>();
        app.init_resource::<Compatibility>();
        app.init_resource::<ConnectionState>();
        app.add_message::<NetMessage>();
        app.add_message::<NetCommand>();
        app.add_message::<ClientCommand>();
        app.add_message::<ClientMessage>();
        app.add_observer(on_rename);
        app.add_observer(on_create_transition);
		app.add_observer(on_despawn);
		app.add_observer(on_reset_region);
        app.add_observer(on_change_node_type);
        app.add_systems(Startup, version_check_startup);
        app.add_systems(Update, (connection_guard, net_commands, watch_events, client_commands));
    }
}

// =========================
// Watch Manager (+re-arming)
// =========================

#[derive(Resource, Clone)]
pub struct TokioRuntime(pub Arc<tokio::runtime::Runtime>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionPhase { Disconnected, Connecting, Connected }

impl Default for ConnectionPhase { fn default() -> Self { ConnectionPhase::Disconnected } }

#[derive(Resource, Clone, Default)]
pub struct ConnectionState {
    pub state: ConnectionPhase,
    pub endpoint: Option<String>,
}

#[derive(Resource, Default)]
pub struct WatchManager {
    ctl_tx: Option<tokio::sync::mpsc::UnboundedSender<WatchCtl>>,
    evt_rx: Option<tokio::sync::mpsc::UnboundedReceiver<WatchEvt>>,
    cursors: std::collections::HashMap<u64, u64>,
    // When true, we've already flushed/closed watchers for a disconnected state
    disconnected_flushed: bool,
}

#[derive(Debug)]
enum WatchCtl {
    StartDiscovery { url: String },
    StopDiscovery,
    StartMachine { url: String, id: u64, last_transition_seq: u64 },
    StopMachine { url: String, id: u64 },
    StartComponents { url: String, id: u64, components: Vec<String> },
    StopComponents { url: String, id: u64 },
    StartControl { url: String },
}

#[derive(Debug, Clone)]
pub struct MachineSummary { pub id: u64, pub name: Option<String> }

#[derive(Debug)]
enum WatchEvt {
    Discovery(Vec<MachineSummary>),
    Machine { id: u64, events: Vec<Value> },
    Error(String),
    Components { id: u64, components: serde_json::Map<String, Value>, removed: Vec<String> },
    ControlOpen(u64),
}

fn ensure_watch_manager(rt: &tokio::runtime::Runtime, mgr: &mut WatchManager) {
    if mgr.ctl_tx.is_some() { return; }
    let (ctl_tx, mut ctl_rx) = tokio::sync::mpsc::unbounded_channel::<WatchCtl>();
    let (evt_tx, evt_rx) = tokio::sync::mpsc::unbounded_channel::<WatchEvt>();
    mgr.ctl_tx = Some(ctl_tx);
    mgr.evt_rx = Some(evt_rx);

    rt.spawn(async move {
        use futures_util::StreamExt as _;
        use std::collections::HashMap;
        let client = reqwest::Client::new();
        let mut discovery_handle: Option<tokio::task::JoinHandle<()>> = None;
        let mut machine_handles: HashMap<u64, tokio::task::JoinHandle<()>> = HashMap::new();
        let mut component_handles: HashMap<u64, tokio::task::JoinHandle<()>> = HashMap::new();
        let mut control_handle: Option<tokio::task::JoinHandle<()>> = None;
        while let Some(cmd) = ctl_rx.recv().await {
            match cmd {
                WatchCtl::StartDiscovery { url } => {
                    // Idempotent: if discovery is already running, do nothing
                    if discovery_handle.is_none() {
                        let tx = evt_tx.clone();
                        let client_clone = client.clone();
                        discovery_handle = Some(tokio::spawn(async move {
                            // Track last known snapshot of machines to suppress duplicates
                            let mut known: std::collections::BTreeMap<u64, Option<String>> = std::collections::BTreeMap::new();
                            loop {
                                let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"editor.discovery+watch","params":null});
                                let resp = match client_clone.post(&url).json(&req).send().await { Ok(r) => r, Err(e) => { let _ = tx.send(WatchEvt::Error(format!("watch discovery http: {}", e))); tokio::time::sleep(std::time::Duration::from_millis(300)).await; continue; } };
                                let mut stream = resp.bytes_stream();
                                while let Some(chunk) = stream.next().await {
                                    match chunk {
                                        Ok(bytes) => {
                                            if let Ok(text) = std::str::from_utf8(&bytes) {
                                                // Build a current snapshot map from this chunk's events
                                                let mut current: std::collections::BTreeMap<u64, Option<String>> = std::collections::BTreeMap::new();
                                                for line in text.split('\n') {
                                                    let line = line.trim_start();
                                                    if !line.starts_with("data: ") { continue; }
                                                    let json_str = &line[6..];
                                                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                                                        if let Some(events) = v.get("result").and_then(|r| r.get("events")).and_then(|e| e.as_array()) {
                                                            for ev in events {
                                                                let kind = ev.get("kind").and_then(|s| s.as_str()).unwrap_or("");
                                                                match kind {
                                                                    "machine_created" | "machine_renamed" | "machine_id_set" => {
                                                                        if let Some(raw) = ev.get("machine").and_then(|v| v.as_u64()) {
                                                                            let name = ev.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
                                                                            current.insert(raw, name);
                                                                        }
                                                                    }
                                                                    "machine_removed" => {
                                                                        if let Some(raw) = ev.get("machine").and_then(|v| v.as_u64()) { let _ = current.remove(&raw); }
                                                                    }
                                                                    _ => {}
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                // If server sends a full snapshot each tick, compute a diff vs known
                                                if !current.is_empty() {
                                                    // Compute diffs
                                                    let mut diff: Vec<MachineSummary> = Vec::new();
                                                    // Added or changed
                                                    for (id, name) in current.iter() {
                                                        if known.get(id) != Some(name) {
                                                            diff.push(MachineSummary { id: *id, name: name.clone() });
                                                        }
                                                    }
                                                    // Removed
                                                    for id in known.keys() {
                                                        if !current.contains_key(id) {
                                                            diff.push(MachineSummary { id: *id, name: None });
                                                        }
                                                    }
                                                    if !diff.is_empty() {
                                                        let _ = tx.send(WatchEvt::Discovery(diff));
                                                    }
                                                    known = current;
                                                }
                                            }
                                        }
                                        Err(e) => { let _ = tx.send(WatchEvt::Error(format!("watch discovery stream: {}", e))); break; }
                                    }
                                }
                                // reconnect loop
                            }
                        }));
                    }
                }
                WatchCtl::StopDiscovery => { if let Some(h) = discovery_handle.take() { h.abort(); } }
                WatchCtl::StartMachine { url, id, mut last_transition_seq } => {
                    // Idempotent: if this machine watch is already running, do nothing
                    if machine_handles.contains_key(&id) { continue; }
                    let tx = evt_tx.clone();
                    let client_clone = client.clone();
                    let handle = tokio::spawn(async move {
                        // Subscribe once before starting stream
                        let _ = client_clone.post(&url).json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":EDITOR_MACHINE_SUBSCRIBE,"params":{"entity":id}})).send().await;
                        loop {
                            let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"editor.machine+watch","params":{"entity":id,"last_transition_seq":last_transition_seq}});
                            let resp = match client_clone.post(&url).json(&req).send().await { Ok(r) => r, Err(e) => { let _ = tx.send(WatchEvt::Error(format!("watch http: {}", e))); tokio::time::sleep(std::time::Duration::from_millis(300)).await; continue; } };
                            let mut stream = resp.bytes_stream();
                            while let Some(chunk) = stream.next().await {
                                match chunk {
                                    Ok(bytes) => {
                                        if let Ok(text) = std::str::from_utf8(&bytes) {
                                            let mut out: Vec<Value> = Vec::new();
                                            for line in text.split('\n') {
                                                let line = line.trim_start();
                                                if !line.starts_with("data: ") { continue; }
                                                let json_str = &line[6..];
                                                if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                                                    if let Some(events) = v.get("result").and_then(|r| r.get("events")).and_then(|e| e.as_array()) {
                                                        if !events.is_empty() {
                                                            for ev in events.iter() {
                                                                if let Some(seq) = ev.get("seq").and_then(|v| v.as_u64()) {
                                                                    let kind = ev.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                                                                    if kind == "transition_edge" || kind == "state_entered" || kind == "state_exited" {
                                                                        if seq > last_transition_seq { last_transition_seq = seq; }
                                                                    }
                                                                }
                                                            }
                                                            out.extend(events.iter().cloned());
                                                        }
                                                    }
                                                }
                                            }
                                            if !out.is_empty() {
                                                let _ = tx.send(WatchEvt::Machine { id, events: out });
                                                // Long-poll: after delivering a non-empty batch, reconnect with updated cursors
                                                break;
                                            }
                                        }
                                    }
                                    Err(e) => { let _ = tx.send(WatchEvt::Error(format!("watch stream: {}", e))); break; }
                                }
                            }
                        }
                    });
                    machine_handles.insert(id, handle);
                }
                WatchCtl::StopMachine { url, id } => {
                    // Best-effort unsubscribe
                    let _ = client.post(&url).json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":EDITOR_MACHINE_UNSUBSCRIBE,"params":{"entity":id}})).send().await;
                    if let Some(h) = machine_handles.remove(&id) { h.abort(); }
                }
                WatchCtl::StartComponents { url, id, components } => {
                    if component_handles.contains_key(&id) { continue; }
                    let tx = evt_tx.clone();
                    let client_clone = client.clone();
                    let comps = components.clone();
                    let handle = tokio::spawn(async move {
                        // One-shot initial snapshot to seed the cache
                        let initial_req = serde_json::json!({
                            "jsonrpc":"2.0","id":1,
                            "method": crate::methods::WORLD_GET_COMPONENTS,
                            "params": {"entity": id, "components": comps}
                        });
                        if let Ok(resp) = client_clone.post(&url).json(&initial_req).send().await {
                            if let Ok(v) = resp.json::<serde_json::Value>().await {
                                let res = v.get("result").cloned().unwrap_or(v.clone());
                                let comps_obj = res.get("components").and_then(|c| c.as_object()).cloned().unwrap_or_else(|| res.as_object().cloned().unwrap_or_default());
                                if !comps_obj.is_empty() {
                                    let _ = tx.send(WatchEvt::Components { id, components: comps_obj, removed: Vec::new() });
                                }
                            }
                        }
                        loop {
                            let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"world.get_components+watch","params":{"entity":id,"components": comps}});
                            let resp = match client_clone.post(&url).json(&req).send().await { Ok(r) => r, Err(e) => { let _ = tx.send(WatchEvt::Error(format!("components watch http: {}", e))); tokio::time::sleep(std::time::Duration::from_millis(300)).await; continue; } };
                            let mut stream = resp.bytes_stream();
                            while let Some(chunk) = stream.next().await {
                                match chunk {
                                    Ok(bytes) => {
                                        if let Ok(text) = std::str::from_utf8(&bytes) {
                                            for line in text.split('\n') {
                                                let line = line.trim_start();
                                                if !line.starts_with("data: ") { continue; }
                                                let json_str = &line[6..];
                                                if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                                                    let res = v.get("result").cloned().unwrap_or(v.clone());
                                                    let comps_obj = res.get("components").and_then(|c| c.as_object()).cloned().unwrap_or_default();
                                                    let removed_arr: Vec<String> = res.get("removed").and_then(|a| a.as_array()).map(|arr| arr.iter().filter_map(|s| s.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
                                                    if !comps_obj.is_empty() || !removed_arr.is_empty() {
                                                        let _ = tx.send(WatchEvt::Components { id, components: comps_obj, removed: removed_arr });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => { let _ = tx.send(WatchEvt::Error(format!("components watch stream: {}", e))); break; }
                                }
                            }
                            // reconnect
                        }
                    });
                    component_handles.insert(id, handle);
                }
                WatchCtl::StopComponents { url: _url, id } => {
                    if let Some(h) = component_handles.remove(&id) { h.abort(); }
                }
                WatchCtl::StartControl { url } => {
                    if control_handle.is_none() {
                        let tx = evt_tx.clone();
                        let client_clone = client.clone();
                        control_handle = Some(tokio::spawn(async move {
                            loop {
                                let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"editor.control+watch","params":null});
                                let resp = match client_clone.post(&url).json(&req).send().await { Ok(r) => r, Err(e) => { let _ = tx.send(WatchEvt::Error(format!("watch control http: {}", e))); tokio::time::sleep(std::time::Duration::from_millis(300)).await; continue; } };
                                let mut stream = resp.bytes_stream();
                                while let Some(chunk) = stream.next().await {
                                    match chunk {
                                        Ok(bytes) => {
                                            if let Ok(text) = std::str::from_utf8(&bytes) {
                                                for line in text.split('\n') {
                                                    let line = line.trim_start();
                                                    if !line.starts_with("data: ") { continue; }
                                                    let json_str = &line[6..];
                                                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                                                        if let Some(events) = v.get("result").and_then(|r| r.get("events")).and_then(|e| e.as_array()) {
                                                            for ev in events.iter() {
                                                                let kind = ev.get("kind").and_then(|s| s.as_str()).unwrap_or("");
                                                                if kind == "open" {
                                                                    if let Some(id) = ev.get("entity").and_then(|v| v.as_u64()) { let _ = tx.send(WatchEvt::ControlOpen(id)); }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => { let _ = tx.send(WatchEvt::Error(format!("watch control stream: {}", e))); break; }
                                    }
                                }
                            }
                        }));
                    }
                }
            }
        }
    });
}

#[derive(Message, Clone)]
pub enum NetMessage {
    Discovery(Vec<MachineSummary>),
    Machine { id: u64, events: Vec<Value> },
    Components { id: u64, components: serde_json::Map<String, Value>, removed: Vec<String> },
    ControlOpen { id: u64 },
}

#[derive(Message)]
pub enum NetCommand {
    StartDiscovery,
    StopDiscovery,
    StartMachine { id: u64 },
    StopMachine { id: u64 },
    StartComponents { id: u64, components: Vec<String> },
    StopComponents { id: u64 },
    StartControl,
}

fn net_commands(
    mut reader: MessageReader<NetCommand>,
    rt: Res<TokioRuntime>,
    client: Res<Client>,
    mut mgr: ResMut<WatchManager>,
) {
    ensure_watch_manager(&rt.0, &mut *mgr);
    for cmd in reader.read() {
        match *cmd {
            NetCommand::StartDiscovery => {
                if let Some(tx) = &mgr.ctl_tx { let _ = tx.send(WatchCtl::StartDiscovery { url: client.base_url.clone() }); }
            }
            NetCommand::StopDiscovery => {
                if let Some(tx) = &mgr.ctl_tx { let _ = tx.send(WatchCtl::StopDiscovery); }
            }
            NetCommand::StartControl => {
                if let Some(tx) = &mgr.ctl_tx { let _ = tx.send(WatchCtl::StartControl { url: client.base_url.clone() }); }
            }
            NetCommand::StartMachine { id } => {
                let lt = mgr.cursors.get(&id).copied().unwrap_or(0);
                if let Some(tx) = &mgr.ctl_tx { let _ = tx.send(WatchCtl::StartMachine { url: client.base_url.clone(), id, last_transition_seq: lt }); }
            }
            NetCommand::StopMachine { id } => {
                if let Some(tx) = &mgr.ctl_tx { let _ = tx.send(WatchCtl::StopMachine { url: client.base_url.clone(), id }); }
            }
            NetCommand::StartComponents { id, ref components } => {
                if let Some(tx) = &mgr.ctl_tx { let _ = tx.send(WatchCtl::StartComponents { url: client.base_url.clone(), id, components: components.clone() }); }
            }
            NetCommand::StopComponents { id } => {
                if let Some(tx) = &mgr.ctl_tx { let _ = tx.send(WatchCtl::StopComponents { url: client.base_url.clone(), id }); }
            }
        }
    }
}

fn watch_events(
    mut mgr: ResMut<WatchManager>,
    mut writer: MessageWriter<NetMessage>,
) {
    if mgr.evt_rx.is_some() {
        let mut rx = mgr.evt_rx.take().unwrap();
        let mut drained = 0usize;
        while let Ok(evt) = rx.try_recv() {
            drained += 1;
            match evt {
                WatchEvt::Discovery(batch) => {
                    writer.write(NetMessage::Discovery(batch));
                }
                WatchEvt::Machine { id, events } => {
                    // Filter duplicates using stored cursors
                    let prev_t = mgr.cursors.get(&id).copied().unwrap_or(0);
                    let mut max_t = prev_t;
                    let mut filtered: Vec<serde_json::Value> = Vec::with_capacity(events.len());
                    for ev in events.into_iter() {
                        let kind = ev.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                        let seq = ev.get("seq").and_then(|v| v.as_u64()).unwrap_or(0);
                        match kind {
                            "transition_edge" => {
                                if seq > prev_t { filtered.push(ev.clone()); if seq > max_t { max_t = seq; } }
                            }
                            _ => {
                                // pass through unknown kinds
                                filtered.push(ev.clone());
                            }
                        }
                    }
                    // Update cursors from filtered
                    mgr.cursors.insert(id, max_t);
                    if !filtered.is_empty() {
                        let mut _tc = 0usize; let _total = filtered.len();
                        for ev in filtered.iter() {
                            if ev.get("kind").and_then(|v| v.as_str()) == Some("transition_edge") { _tc += 1; }
                        }
                        writer.write(NetMessage::Machine { id, events: filtered });
                    }
                }
                WatchEvt::Error(_e) => (),
                WatchEvt::Components { id, components, removed } => {
                    writer.write(NetMessage::Components { id, components, removed });
                }
                WatchEvt::ControlOpen(id) => { writer.write(NetMessage::ControlOpen { id }); }
            }
        }
        if drained > 0 { /* optional log */ }
        mgr.evt_rx = Some(rx);
    }
}

// Close all active watches when disconnected; no-op while connected
fn connection_guard(
    conn: Res<ConnectionState>,
    mut mgr: ResMut<WatchManager>,
    mut writer: MessageWriter<NetCommand>,
){
    match conn.state {
        ConnectionPhase::Connected => {
            // Reset guard so future disconnects flush again
            if mgr.disconnected_flushed { mgr.disconnected_flushed = false; }
        }
        _ => {
            if !mgr.disconnected_flushed {
                // Stop discovery
                writer.write(NetCommand::StopDiscovery);
                // Stop all known machine/component watches (safe even if not running)
                let ids: Vec<u64> = mgr.cursors.keys().copied().collect();
                for id in ids.into_iter() {
                    writer.write(NetCommand::StopMachine { id });
                    writer.write(NetCommand::StopComponents { id });
                }
                mgr.disconnected_flushed = true;
            }
        }
    }
}

// =========================
// Protocol version check
// =========================

const SUPPORTED_VERSION_MIN: u32 = 1;
const SUPPORTED_VERSION_MAX: u32 = 1;

#[derive(Resource, Clone, Default)]
pub struct Compatibility {
    pub server_version: Option<u32>,
    pub compatible: bool,
    pub message: Option<String>,
}

fn version_check_startup(
    client: Res<Client>,
    rt: Res<TokioRuntime>,
    mut compat: ResMut<Compatibility>,
) {
    match rt.0.block_on(client.version()) {
        Ok(ver) => {
            let ok = ver >= SUPPORTED_VERSION_MIN && ver <= SUPPORTED_VERSION_MAX;
            compat.server_version = Some(ver);
            compat.compatible = ok;
            if ok {
                compat.message = Some(format!("Protocol version OK: {} (supported {}..={})", ver, SUPPORTED_VERSION_MIN, SUPPORTED_VERSION_MAX));
                bevy::log::info!("[protocol] version {} compatible", ver);
            } else {
                compat.message = Some(format!("Protocol version {} unsupported (supported {}..={})", ver, SUPPORTED_VERSION_MIN, SUPPORTED_VERSION_MAX));
                bevy::log::error!("[protocol] version {} unsupported (supported {}..={})", ver, SUPPORTED_VERSION_MIN, SUPPORTED_VERSION_MAX);
            }
        }
        Err(e) => {
            compat.server_version = None;
            compat.compatible = false;
            compat.message = Some(format!("Failed to get protocol version: {}", e));
            bevy::log::error!("[protocol] failed to get version: {}", e);
        }
    }
}


// =========================
// Helper: extract EventEdge<T> variants from registry.schema
// =========================
fn extract_event_edge_variants(schema: &serde_json::Value) -> Vec<String> {
    fn collect_strings(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::String(s) => out.push(s.clone()),
            serde_json::Value::Array(arr) => { for x in arr { collect_strings(x, out); } }
            serde_json::Value::Object(map) => { for (_k, vv) in map { collect_strings(vv, out); } }
            _ => {}
        }
    }
    fn inner_generic(name: &str) -> Option<String> {
        let lb = name.find('<')?;
        let rb = name.rfind('>')?;
        if rb > lb + 1 {
            return Some(name[lb + 1..rb].to_string());
        }
        None
    }
    fn simple_type(name: &str) -> String {
        name.rsplit("::").next().unwrap_or(name).to_string()
    }
    let mut strings: Vec<String> = Vec::new();
    collect_strings(schema, &mut strings);
    let mut out: Vec<String> = Vec::new();
    for s in strings.into_iter() {
        if s.contains(crate::components::EVENT_EDGE_SUBSTR) {
            if let Some(inner) = inner_generic(&s) { out.push(simple_type(&inner)); }
        }
    }
    out.sort();
    out.dedup();
    out
}

