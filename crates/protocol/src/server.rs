#![cfg(feature = "server")]
use bevy::prelude::*;

use bevy::remote::{BrpError, BrpResult, RemoteMethodSystemId, RemoteMethods, error_codes};
use serde::Deserialize;
use serde_json::Value;
use std::net::SocketAddr;

use bevy_gearbox as gearbox;
use crate::methods::{PROTOCOL_VERSION, EDITOR_MACHINE_GRAPH};
use crate::methods::EDITOR_RESET_REGION;
use crate::methods::EDITOR_CREATE_TRANSITION;
use std::collections::{HashMap, VecDeque};

#[derive(Default)]
pub struct ServerPlugin {
    pub headers: Vec<(String, String)>,
    pub bind_address: Option<SocketAddr>,
}

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        // Install Bevy Remote HTTP server
        let mut http = {
            let addr = self.bind_address.unwrap_or_else(|| "127.0.0.1:15703".parse().expect("bind addr"));
            bevy::remote::http::RemoteHttpPlugin::default()
                .with_address(addr.ip())
                .with_port(addr.port())
        };
        if !self.headers.is_empty() {
            let mut headers = bevy::remote::http::Headers::new();
            for (k, v) in &self.headers { headers = headers.insert(k.clone(), v.clone()); }
            http = http.with_headers(headers);
        }
        app.add_plugins(bevy::remote::RemotePlugin::default());
        app.add_plugins(http);

        // Trackers and observers for +watch ring buffers
        app.init_resource::<MachineTrackers>()
            .add_observer(on_transition_edge)
            .add_observer(on_state_entered)
            .add_observer(on_state_exited)
            ;

        // Register RPCs (+watch and convenience endpoints). Start minimal; extend as needed.
        register_editor_subscription_rpcs(app);
        register_editor_watch_rpcs(app);
        register_editor_file_rpcs(app);
        register_editor_convenience_rpcs(app);
        register_editor_transition_rpcs(app);
        register_editor_machine_graph_rpc(app);
        register_editor_node_rpcs(app);
        register_editor_control_rpcs(app);
        register_version_rpc(app);
        // Observers for control bus triggers (usable from game code via events)
        app.add_observer(on_open_on_client_event)
            .add_observer(on_open_if_related_event);
    }
}

// =========================
// File RPCs (ported real behavior)
// =========================
#[derive(Deserialize)]
struct SaveGraphParams { entity: Entity, path: String }

fn serialize_scene(world: &World, scene: &bevy::scene::DynamicScene) -> Result<String, String> {
    let reg = world.resource::<AppTypeRegistry>();
    let reg = reg.read();
    scene.serialize(&reg).map_err(|e| format!("serialize scene: {e}"))
}

fn atomic_write(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::fs;
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.flush()?;
    }
    #[cfg(target_os = "windows")]
    {
        fs::rename(&tmp, path)?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::fs::remove_file(path);
        std::fs::rename(&tmp, path)
    }
}

fn collect_state_machine_entities(world: &mut World, root: Entity) -> Vec<Entity> {
    use std::collections::HashSet;
    // Visit all descendants via RelationshipTarget helpers and include transition edge entities
    let mut out: Vec<Entity> = Vec::new();
    if !world.entities().contains(root) { return out; }
    let mut seen: HashSet<Entity> = HashSet::new();
    let mut q_children_state = world.query::<&gearbox::Substates>();
    let q_children = q_children_state.query(world);
    // Always include the root
    seen.insert(root);
    out.push(root);
    // Descendants
    for e in q_children.iter_descendants(root) {
        if seen.insert(e) { out.push(e); }
    }
    // Include transition edges referenced by any visited node
    let snapshot = out.clone();
    for e in snapshot.into_iter() {
        if let Some(transitions) = world.get::<gearbox::transitions::Transitions>(e) {
            for &edge in transitions.into_iter() {
                if world.entities().contains(edge) && seen.insert(edge) { out.push(edge); }
            }
        }
    }
    out
}

fn build_scene_from_root(world: &mut World, root: Entity) -> bevy::scene::DynamicScene {
    use bevy::scene::DynamicSceneBuilder;
    let entities = collect_state_machine_entities(world, root);
    let builder = DynamicSceneBuilder::from_world(world)
        .allow_all()
        .extract_entities(entities.into_iter())
        .build();
    builder
}

// Remove ephemeral data from components inside a DynamicScene prior to serialization
fn scrub_state_machine_ephemeral(scene: &mut bevy::scene::DynamicScene) {
    for ent in scene.entities.iter_mut() {
        for comp in ent.components.iter_mut() {
            if comp.represents::<gearbox::StateMachine>() {
                if let Some(refl) = comp.try_as_reflect_mut() {
                    if let Some(sm) = refl.downcast_mut::<gearbox::StateMachine>() {
                        sm.active.clear();
                        sm.active_leaves.clear();
                    }
                }
            }
        }
    }
}

// =========================
// Save As / Save Substates
// =========================

#[derive(Deserialize)]
struct SaveAsParams { entity: Entity, path: String }

fn ensure_absolute_assets_path(path: String) -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(path);
    if !p.is_absolute() { p = std::path::PathBuf::from("assets").join(p); }
    if p.extension().and_then(|s| s.to_str()) != Some("ron") { p.set_extension("scn.ron"); }
    p
}

fn find_unreflected_components(world: &World, entities: &[Entity]) -> Vec<String> {
    let regs = world.resource::<AppTypeRegistry>().read();
    let mut out: Vec<String> = Vec::new();
    for &e in entities.iter() {
        if !world.entities().contains(e) { continue; }
        let original = world.entity(e);
        for component_id in original.archetype().iter_components() {
            if let Some(info) = world.components().get_info(component_id) {
                if let Some(type_id) = info.type_id() {
                    let has_refl = regs.get(type_id).and_then(|r| r.data::<bevy::ecs::reflect::ReflectComponent>()).is_some();
                    if !has_refl { out.push(info.name().to_string()); }
                }
            }
        }
    }
    out.sort(); out.dedup();
    out
}

fn save_as_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: SaveAsParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    if !world.entities().contains(p.entity) {
        return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None });
    }
    // Preflight: ensure all components in subtree are reflect-serializable
    let set = collect_state_machine_entities(world, p.entity);
    let bad = find_unreflected_components(world, &set);
    if !bad.is_empty() {
        return Err(BrpError { code: error_codes::INVALID_PARAMS, message: format!("unserializable components present: {}", bad.join(", ")), data: None });
    }
    let mut scene = build_scene_from_root(world, p.entity);
    // Clear ephemeral StateMachine fields prior to serialization
    scrub_state_machine_ephemeral(&mut scene);
    // Remove SubstateOf on the selected root so it becomes a top-level root when loading
    if let Some(ent) = scene.entities.iter_mut().find(|e| e.entity == p.entity) {
        ent.components.retain(|c| !c.represents::<gearbox::SubstateOf>());
    }
    let ron = serialize_scene(world, &scene).map_err(|msg| BrpError { code: error_codes::INTERNAL_ERROR, message: msg, data: None })?;
    let path = ensure_absolute_assets_path(p.path);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| BrpError { code: error_codes::INTERNAL_ERROR, message: format!("mkdirs: {e}"), data: None })?; }
    atomic_write(&path, &ron).map_err(|e| BrpError { code: error_codes::INTERNAL_ERROR, message: format!("write: {e}"), data: None })?;
    Ok(serde_json::json!({"ok": true, "path": path.to_string_lossy().to_string()}))
}

#[derive(Deserialize)]
struct SaveSubstatesParams { entity: Entity }

fn iter_descendants_with_id(world: &mut World, root: Entity) -> Vec<(Entity, String)> {
    let mut out: Vec<(Entity, String)> = Vec::new();
    if !world.entities().contains(root) { return out; }
    let mut q_children_state = world.query::<&gearbox::Substates>();
    let q_children = q_children_state.query(world);
    for cur in q_children.iter_descendants(root) {
        if let Some(id) = world.get::<gearbox::StateMachineId>(cur) {
            if !id.0.is_empty() { out.push((cur, id.0.clone())); }
        }
    }
    out
}

fn save_substates_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: SaveSubstatesParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    if !world.entities().contains(p.entity) {
        return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None });
    }
    let targets = iter_descendants_with_id(world, p.entity);
    let mut results: Vec<Value> = Vec::new();
    for (ent, id) in targets.into_iter() {
        let path = ensure_absolute_assets_path(format!("{}.scn.ron", id));
        let set = collect_state_machine_entities(world, ent);
        let bad = find_unreflected_components(world, &set);
        if !bad.is_empty() {
            results.push(serde_json::json!({"entity": entity_to_bits(ent), "id": id, "ok": false, "error": format!("unserializable components: {}", bad.join(", ")) }));
            continue;
        }
        let mut scene = build_scene_from_root(world, ent);
        // Clear ephemeral StateMachine fields prior to serialization
        scrub_state_machine_ephemeral(&mut scene);
        // Remove SubstateOf on the saved sub-machine root
        if let Some(e) = scene.entities.iter_mut().find(|e| e.entity == ent) { e.components.retain(|c| !c.represents::<gearbox::SubstateOf>()); }
        match serialize_scene(world, &scene) {
            Ok(ron) => {
                if let Some(parent) = path.parent() { if let Err(e) = std::fs::create_dir_all(parent) { results.push(serde_json::json!({"entity": entity_to_bits(ent), "id": id, "ok": false, "error": format!("mkdirs: {}", e)})); continue; } }
                match atomic_write(&path, &ron) {
                    Ok(_) => { results.push(serde_json::json!({"entity": entity_to_bits(ent), "id": id, "ok": true, "path": path.to_string_lossy().to_string()})); },
                    Err(e) => { results.push(serde_json::json!({"entity": entity_to_bits(ent), "id": id, "ok": false, "error": format!("write: {}", e)})); },
                }
            }
            Err(msg) => { results.push(serde_json::json!({"entity": entity_to_bits(ent), "id": id, "ok": false, "error": msg })); }
        }
    }
    Ok(serde_json::json!({"results": results}))
}

fn save_graph_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: SaveGraphParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    if !world.entities().contains(p.entity) {
        return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None });
    }
    // Ensure .scn.ron extension and assets path defaulting
    let mut path = std::path::PathBuf::from(p.path);
    if !path.is_absolute() { path = std::path::PathBuf::from("assets").join(path); }
    if path.extension().and_then(|s| s.to_str()) != Some("ron") { path.set_extension("scn.ron"); }
    let mut scene = build_scene_from_root(world, p.entity);
    // Clear ephemeral StateMachine fields prior to serialization
    scrub_state_machine_ephemeral(&mut scene);
    let ron = serialize_scene(world, &scene).map_err(|msg| BrpError { code: error_codes::INTERNAL_ERROR, message: msg, data: None })?;
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| BrpError { code: error_codes::INTERNAL_ERROR, message: format!("mkdirs: {e}"), data: None })?; }
    atomic_write(&path, &ron).map_err(|e| BrpError { code: error_codes::INTERNAL_ERROR, message: format!("write: {e}"), data: None })?;
    Ok(serde_json::json!({"ok": true}))
}

#[derive(Deserialize)]
struct SaveSidecarParams { path: String, contents: String }

fn save_sidecar_handler(In(params): In<Option<Value>>, _world: &mut World) -> BrpResult {
    let p: SaveSidecarParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    let mut path = std::path::PathBuf::from(p.path);
    if !path.is_absolute() { path = std::path::PathBuf::from("assets").join(path); }
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| BrpError { code: error_codes::INTERNAL_ERROR, message: format!("mkdirs: {e}"), data: None })?; }
    atomic_write(&path, &p.contents).map_err(|e| BrpError { code: error_codes::INTERNAL_ERROR, message: format!("write: {e}"), data: None })?;
    Ok(serde_json::json!({"ok": true}))
}

#[derive(Deserialize)]
struct LoadSidecarParams { path: String }

fn load_sidecar_handler(In(params): In<Option<Value>>, _world: &mut World) -> BrpResult {
    let p: LoadSidecarParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    let mut path = std::path::PathBuf::from(p.path);
    if !path.is_absolute() { path = std::path::PathBuf::from("assets").join(path); }
    let txt = std::fs::read_to_string(&path).map_err(|e| BrpError { code: error_codes::INTERNAL_ERROR, message: format!("read: {e}"), data: None })?;
    Ok(serde_json::json!({"text": txt}))
}

#[derive(Deserialize)]
struct FindByFingerprintParams { fp: String }

fn find_sidecar_by_fingerprint_handler(In(params): In<Option<Value>>, _world: &mut World) -> BrpResult {
    let p: FindByFingerprintParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    // Simple scan: current dir and ./assets for *.sm.ron containing the fingerprint
    let mut roots: Vec<std::path::PathBuf> = vec![std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))];
    let assets = std::path::PathBuf::from("assets");
    if assets.exists() { roots.push(assets); }
    for root in roots.into_iter() {
        let walker = walkdir::WalkDir::new(&root).max_depth(6);
        for entry in walker.into_iter().filter_map(|e| e.ok()) {
            let path = entry.path().to_path_buf();
            if !path.is_file() { continue; }
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) { if ext != "ron" { continue; } }
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) { if !name.ends_with(".sm.ron") { continue; } }
            if let Ok(txt) = std::fs::read_to_string(&path) {
                if txt.contains(&p.fp) { return Ok(serde_json::json!({"text": txt})); }
            }
        }
    }
    Ok(serde_json::json!({"text": null}))
}

#[derive(Deserialize)]
struct SetStateMachineId { entity: Entity, path: String }

fn set_state_machine_id_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: SetStateMachineId = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    if !world.entities().contains(p.entity) {
        return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None });
    }
    let mut e = world.entity_mut(p.entity);
    e.insert(gearbox::StateMachineId(p.path));
    Ok(serde_json::json!({"ok": true}))
}

#[derive(Deserialize)]
struct SidecarForMachineParams { entity: Entity }

fn sidecar_for_machine_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: SidecarForMachineParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    if !world.entities().contains(p.entity) {
        return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None });
    }
    // Resolve by StateMachineId("id") -> assets/<id>.sm.ron
    if let Some(id) = world.get::<gearbox::StateMachineId>(p.entity) {
        let fname = format!("{}.sm.ron", id.0);
        let mut path = std::path::PathBuf::from(&fname);
        if !path.is_absolute() { path = std::path::PathBuf::from("assets").join(path); }
        match std::fs::read_to_string(&path) {
            Ok(txt) => return Ok(serde_json::json!({"text": txt})),
            Err(_) => { /* fall through to None */ }
        }
    }
    Ok(serde_json::json!({"text": null}))
}

fn register_editor_file_rpcs(app: &mut App) {
    if !app.world().contains_resource::<RemoteMethods>() { return; }
    let world = app.main_mut().world_mut();
    let save_id = world.register_system(save_graph_handler);
    let save_as_id = world.register_system(save_as_handler);
    let save_subs_id = world.register_system(save_substates_handler);
    let save_sc_id = world.register_system(save_sidecar_handler);
    let load_sc_id = world.register_system(load_sidecar_handler);
    let find_sc_id = world.register_system(find_sidecar_by_fingerprint_handler);
    let set_state_machine_id = world.register_system(set_state_machine_id_handler);
    let sidecar_for_machine_id = world.register_system(sidecar_for_machine_handler);
    let mut methods = world.resource_mut::<RemoteMethods>();
    methods.insert("editor.save_graph", RemoteMethodSystemId::Instant(save_id));
    methods.insert("editor.save_as", RemoteMethodSystemId::Instant(save_as_id));
    methods.insert("editor.save_substates", RemoteMethodSystemId::Instant(save_subs_id));
    methods.insert("editor.save_sidecar", RemoteMethodSystemId::Instant(save_sc_id));
    methods.insert("editor.load_sidecar", RemoteMethodSystemId::Instant(load_sc_id));
    methods.insert("editor.find_sidecar_by_fingerprint", RemoteMethodSystemId::Instant(find_sc_id));
    methods.insert("editor.set_state_machine_id", RemoteMethodSystemId::Instant(set_state_machine_id));
    methods.insert(crate::methods::EDITOR_SIDECAR_FOR_MACHINE, RemoteMethodSystemId::Instant(sidecar_for_machine_id));
}

// =========================
// +watch RPCs (skeletons)
// =========================
#[derive(Deserialize)]
struct MachineWatchParams {
    entity: Entity,
    #[serde(default)]
    last_transition_seq: u64,
}

fn entity_to_bits(e: Entity) -> u64 { e.to_bits() }

fn discovery_watch_handler(_in: In<Option<Value>>, world: &mut World) -> BrpResult<Option<Value>> {
    // Minimal snapshot: list current machines with optional names
    let mut events: Vec<Value> = Vec::new();
    let mut q = world.query::<(Entity, &gearbox::StateMachine, Option<&Name>)>();
    for (e, _sm, name) in q.iter(world) {
        let mut ev = serde_json::json!({
            "kind": "machine_created",
            "machine": entity_to_bits(e),
        });
        if let Some(n) = name {
            let s: &str = n.as_str();
            if !s.is_empty() {
                if let Some(obj) = ev.as_object_mut() {
                    obj.insert("name".to_string(), serde_json::Value::String(s.to_string()));
                }
            }
        }
        events.push(ev);
    }
    Ok(Some(serde_json::json!({"events": events})))
}

fn machine_watch_handler(In(_params): In<Option<Value>>, _world: &World) -> BrpResult<Option<Value>> {
    let params: Option<Value> = _params;
    let p: MachineWatchParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError {
        code: error_codes::INVALID_PARAMS,
        message: format!("invalid params: {e}"),
        data: None,
    })?;

    // Gate by subscription counts if present
    if let Some(subs) = _world.get_resource::<Subscriptions>() {
        let count = subs.counts.get(&p.entity).copied().unwrap_or(0);
        if count == 0 {
            return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "not subscribed".to_string(), data: None });
        }
    }

    // If we have trackers, flush events newer than cursors (transition only)
    if let Some(trackers) = _world.get_resource::<MachineTrackers>() {
        if let Some(tracker) = trackers.trackers.get(&p.entity) {
            let mut out: Vec<Value> = Vec::new();
            for ev in tracker.events.iter() {
                let kind = ev.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                let seq = ev.get("seq").and_then(|v| v.as_u64()).unwrap_or(0);
                match kind {
                    "transition_edge" | "state_entered" | "state_exited" if seq > p.last_transition_seq => out.push(ev.clone()),
                    _ => {}
                }
            }
            if out.is_empty() { return Ok(None); }
            return Ok(Some(serde_json::json!({"events": out})));
        }
    }

    Ok(None)
}

fn register_editor_watch_rpcs(app: &mut App) {
    if !app.world().contains_resource::<RemoteMethods>() { return; }
    let world = app.main_mut().world_mut();
    let discovery_watch = world.register_system(discovery_watch_handler);
    let machine_watch = world.register_system(machine_watch_handler);
    let mut methods = world.resource_mut::<RemoteMethods>();
    methods.insert("editor.discovery+watch", RemoteMethodSystemId::Watching(discovery_watch));
    methods.insert("editor.machine+watch", RemoteMethodSystemId::Watching(machine_watch));
}

// =========================
// Control RPCs and +watch
// =========================

#[derive(Deserialize)]
struct OpenOnClientParams { entity: Entity }

#[derive(Deserialize)]
struct OpenIfRelatedParams { target: Entity, related: Entity }

fn control_watch_handler(_in: In<Option<Value>>, world: &mut World) -> BrpResult<Option<Value>> {
    // Drain any queued control events as a batch
    if !world.contains_resource::<ControlBus>() { world.insert_resource(ControlBus::default()); }
    let mut bus = world.resource_mut::<ControlBus>();
    if bus.queue.is_empty() { return Ok(None); }
    let mut events: Vec<Value> = Vec::new();
    while let Some(ev) = bus.queue.pop_front() { events.push(ev); }
    Ok(Some(serde_json::json!({"events": events})))
}

fn open_on_client_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: OpenOnClientParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    if !world.entities().contains(p.entity) { return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None }); }
    if !world.contains_resource::<ControlBus>() { world.insert_resource(ControlBus::default()); }
    let mut bus = world.resource_mut::<ControlBus>();
    bus.queue.push_back(serde_json::json!({"kind":"open","entity": entity_to_bits(p.entity)}));
    Ok(serde_json::json!({"ok": true}))
}

fn open_if_related_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: OpenIfRelatedParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    if !world.entities().contains(p.target) || !world.entities().contains(p.related) { return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None }); }
    // Consider 'related is open' as 'machine watch is active' using Subscriptions counts
    if let Some(subs) = world.get_resource::<Subscriptions>() {
        if subs.counts.get(&p.related).copied().unwrap_or(0) > 0 {
            if !world.contains_resource::<ControlBus>() { world.insert_resource(ControlBus::default()); }
            let mut bus = world.resource_mut::<ControlBus>();
            bus.queue.push_back(serde_json::json!({"kind":"open","entity": entity_to_bits(p.target)}));
        }
    }
    Ok(serde_json::json!({"ok": true}))
}

fn register_editor_control_rpcs(app: &mut App) {
    if !app.world().contains_resource::<RemoteMethods>() { return; }
    if !app.world().contains_resource::<ControlBus>() { app.insert_resource(ControlBus::default()); }
    let world = app.main_mut().world_mut();
    let control_watch_id = world.register_system(control_watch_handler);
    let open_id = world.register_system(open_on_client_handler);
    let open_if_id = world.register_system(open_if_related_handler);
    let mut methods = world.resource_mut::<RemoteMethods>();
    methods.insert("editor.control+watch", RemoteMethodSystemId::Watching(control_watch_id));
    methods.insert("editor.open_on_client", RemoteMethodSystemId::Instant(open_id));
    methods.insert("editor.open_if_related", RemoteMethodSystemId::Instant(open_if_id));
}

// =========================
// Protocol version RPC
// =========================
fn version_handler(_in: In<Option<Value>>, _world: &World) -> BrpResult {
    // Single u32 version for now; expand to { min, max } if needed
    Ok(serde_json::json!({"version": 1u32}))
}

fn register_version_rpc(app: &mut App) {
    if !app.world().contains_resource::<RemoteMethods>() { return; }
    let world = app.main_mut().world_mut();
    let id = world.register_system(version_handler);
    let mut methods = world.resource_mut::<RemoteMethods>();
    methods.insert(PROTOCOL_VERSION, RemoteMethodSystemId::Instant(id));
}

// =========================
// Subscriptions (skeleton)
// =========================
#[derive(Resource, Default)]
struct Subscriptions { counts: std::collections::HashMap<Entity, u32> }

#[derive(Deserialize)]
struct SubscribeParams { entity: Entity }

fn parse_params<T: for<'de> Deserialize<'de>>(params: Option<Value>) -> Result<T, BrpError> {
    serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError {
        code: error_codes::INVALID_PARAMS,
        message: format!("invalid params: {e}"),
        data: None,
    })
}

fn subscribe_machine_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: SubscribeParams = parse_params(params)?;
    if !world.entities().contains(p.entity) {
        return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None });
    }
    let mut counts = world.resource_mut::<Subscriptions>();
    let c = counts.counts.entry(p.entity).or_insert(0);
    *c = c.saturating_add(1);
    
    // Trigger MachineSubscribed event
    world.commands().trigger(crate::events::MachineSubscribed { target: p.entity });
    
    Ok(serde_json::json!({"ok": true}))
}

fn unsubscribe_machine_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: SubscribeParams = parse_params(params)?;
    if !world.entities().contains(p.entity) {
        return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None });
    }
    let mut counts = world.resource_mut::<Subscriptions>();
    if let Some(c) = counts.counts.get_mut(&p.entity) {
        *c = c.saturating_sub(1);
        if *c == 0 { counts.counts.remove(&p.entity); }
    }
    Ok(serde_json::json!({"ok": true}))
}

fn register_editor_subscription_rpcs(app: &mut App) {
    if !app.world().contains_resource::<RemoteMethods>() { return; }
    if !app.world().contains_resource::<Subscriptions>() { app.insert_resource(Subscriptions::default()); }
    let world = app.main_mut().world_mut();
    let sub_id = world.register_system(subscribe_machine_handler);
    let unsub_id = world.register_system(unsubscribe_machine_handler);
    let mut methods = world.resource_mut::<RemoteMethods>();
    methods.insert("editor.machine_subscribe", RemoteMethodSystemId::Instant(sub_id));
    methods.insert("editor.machine_unsubscribe", RemoteMethodSystemId::Instant(unsub_id));
}

// =========================
// Trackers and observers
// =========================
const RING_CAPACITY: usize = 4096;

#[derive(Default, Resource)]
struct MachineTrackers {
    trackers: HashMap<Entity, MachineTracker>,
}

// =========================
// Control bus (+watch) to send one-shot editor control messages to clients
// =========================

#[derive(Default, Resource)]
struct ControlBus {
    // Simple FIFO of control events; broadcast to any client listening
    queue: VecDeque<serde_json::Value>,
}

fn on_open_on_client_event(evt: On<crate::events::OpenOnClient>, mut bus: ResMut<ControlBus>) {
    bus.queue.push_back(serde_json::json!({"kind":"open","entity": entity_to_bits(evt.target)}));
}

fn on_open_if_related_event(evt: On<crate::events::OpenIfRelated>, subs: Res<Subscriptions>, mut bus: ResMut<ControlBus>) {
    if subs.counts.get(&evt.related).copied().unwrap_or(0) > 0 {
        bus.queue.push_back(serde_json::json!({"kind":"open","entity": entity_to_bits(evt.target)}));
    }
}

// =========================
// Convenience editor RPCs (minimal)
// =========================
#[derive(serde::Deserialize)]
struct ResetRegionParams { root: Entity }

fn reset_region_handler(In(params): In<Option<Value>>, _world: &mut World) -> BrpResult {
    let p: ResetRegionParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError {
        code: error_codes::INVALID_PARAMS,
        message: format!("invalid params: {e}"),
        data: None,
    })?;
    if !_world.entities().contains(p.root) {
        return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None });
    }
    _world.commands().trigger(crate::events::ResetRegion { target: p.root });
    Ok(serde_json::json!({"ok": true}))
}

fn register_editor_convenience_rpcs(app: &mut App) {
    if !app.world().contains_resource::<RemoteMethods>() { return; }
    let world = app.main_mut().world_mut();
    let reset_id = world.register_system(reset_region_handler);
    let mut methods = world.resource_mut::<RemoteMethods>();
    methods.insert(EDITOR_RESET_REGION, RemoteMethodSystemId::Instant(reset_id));
}

// =========================
// Node mutation RPCs (spawn substate, delete subtree of states)
// =========================

#[derive(Deserialize)]
struct SpawnSubstateParams { parent: Entity, #[serde(default)] name: Option<String> }

fn spawn_substate_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: SpawnSubstateParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    if !world.entities().contains(p.parent) {
        return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None });
    }
    let entity = {
        let mut e = world.spawn_empty();
        if let Some(name) = p.name.as_ref() { e.insert(Name::new(name.clone())); }
        e.insert(gearbox::SubstateOf(p.parent));
        e.id()
    };
    Ok(serde_json::json!({"entity": entity.to_bits()}))
}

#[derive(Deserialize)]
struct DeleteSubtreeParams { root: Entity }

fn delete_subtree_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: DeleteSubtreeParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    if !world.entities().contains(p.root) {
        return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None });
    }
    // Collect descendant states
    let mut q_children_state = world.query::<&gearbox::Substates>();
    let q_children = q_children_state.query(world);
    let mut to_delete_states: Vec<Entity> = Vec::new();
    for cur in q_children.iter_descendants(p.root) { to_delete_states.push(cur); }
    if to_delete_states.is_empty() { return Ok(serde_json::json!({"ok": true, "deleted": 0u32})); }
    // Collect related transition edges for those states
    use std::collections::HashSet;
    let mut edges: HashSet<Entity> = HashSet::new();
    for s in to_delete_states.iter().copied() {
        if let Some(ts) = world.get::<gearbox::transitions::Transitions>(s) { for &e in ts.into_iter() { edges.insert(e); } }
        if let Some(incoming) = world.get::<gearbox::transitions::TargetedBy>(s) { for &e in incoming.into_iter() { edges.insert(e); } }
    }
    // Despawn edges first, then states
    for e in edges.into_iter() { let _ = world.despawn(e); }
    for s in to_delete_states.into_iter() { let _ = world.despawn(s); }
    Ok(serde_json::json!({"ok": true}))
}

fn register_editor_node_rpcs(app: &mut App) {
    if !app.world().contains_resource::<RemoteMethods>() { return; }
    let world = app.main_mut().world_mut();
    let spawn_id = world.register_system(spawn_substate_handler);
    let del_id = world.register_system(delete_subtree_handler);
    let make_leaf_id = world.register_system(make_leaf_handler);
    let make_parent_id = world.register_system(make_parent_handler);
    let make_parallel_id = world.register_system(make_parallel_handler);
    let mut methods = world.resource_mut::<RemoteMethods>();
    methods.insert(crate::methods::EDITOR_SPAWN_SUBSTATE, RemoteMethodSystemId::Instant(spawn_id));
    methods.insert(crate::methods::EDITOR_DELETE_SUBTREE, RemoteMethodSystemId::Instant(del_id));
    methods.insert(crate::methods::EDITOR_MAKE_LEAF, RemoteMethodSystemId::Instant(make_leaf_id));
    methods.insert(crate::methods::EDITOR_MAKE_PARENT, RemoteMethodSystemId::Instant(make_parent_id));
    methods.insert(crate::methods::EDITOR_MAKE_PARALLEL, RemoteMethodSystemId::Instant(make_parallel_id));
}

#[derive(Deserialize)]
struct MakeLeafParams { target: Entity }

fn make_leaf_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: MakeLeafParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    if !world.entities().contains(p.target) { return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None }); }
    // Despawn all descendants
    let _ = delete_subtree_handler(In(Some(serde_json::json!({"root": p.target}))), world);
    // Remove components on the target
    let mut e = world.entity_mut(p.target);
    if e.get::<gearbox::InitialState>().is_some() { e.remove::<gearbox::InitialState>(); }
    if e.get::<gearbox::Substates>().is_some() { e.remove::<gearbox::Substates>(); }
    Ok(serde_json::json!({"ok": true}))
}

#[derive(Deserialize)]
struct MakeParentParams { target: Entity, #[serde(default)] name: Option<String> }

fn make_parent_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: MakeParentParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    if !world.entities().contains(p.target) { return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None }); }
    // Ensure a child exists
    let mut has_child = false;
    if let Some(children) = world.get::<gearbox::Substates>(p.target) { for _ in children.into_iter() { has_child = true; break; } }
    let child = if has_child {
        // Pick first child as initial
        world.get::<gearbox::Substates>(p.target).and_then(|c| c.into_iter().next().copied())
    } else {
        // Spawn a child
        let mut e = world.spawn_empty();
        if let Some(name) = p.name.as_ref() { e.insert(Name::new(name.clone())); }
        e.insert((
            Name::new("New State"),
            gearbox::SubstateOf(p.target)
        ));
        Some(e.id())
    };
    if let Some(cid) = child {
        world.entity_mut(p.target).insert(gearbox::InitialState(cid));
    }
    Ok(serde_json::json!({"ok": true}))
}

#[derive(Deserialize)]
struct MakeParallelParams { target: Entity, #[serde(default)] name: Option<String> }

fn make_parallel_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: MakeParallelParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError { code: error_codes::INVALID_PARAMS, message: format!("invalid params: {e}"), data: None })?;
    if !world.entities().contains(p.target) { return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None }); }
    // Ensure a child exists to keep it a parent node
    let mut has_child = false;
    if let Some(children) = world.get::<gearbox::Substates>(p.target) { for _ in children.into_iter() { has_child = true; break; } }
    if !has_child {
        let mut e = world.spawn_empty();
        if let Some(name) = p.name.as_ref() { e.insert(Name::new(name.clone())); }
        e.insert((
            Name::new("New State"),
            gearbox::SubstateOf(p.target)
        ));
    }
    // Remove InitialState to make parallel
    let mut parent = world.entity_mut(p.target);
    if parent.get::<gearbox::InitialState>().is_some() { parent.remove::<gearbox::InitialState>(); }
    Ok(serde_json::json!({"ok": true}))
}

// =========================
// Transition creation RPCs
// =========================

#[derive(Deserialize)]
struct CreateTransitionParams { source: Entity, target: Entity, kind: String }

fn simple_type_name(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

fn inner_generic(name: &str) -> Option<&str> {
    let lb = name.find('<')?;
    let rb = name.rfind('>')?;
    if rb > lb + 1 { Some(&name[lb + 1..rb]) } else { None }
}

fn create_transition_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: CreateTransitionParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError {
        code: error_codes::INVALID_PARAMS,
        message: format!("invalid params: {e}"),
        data: None,
    })?;

    if !world.entities().contains(p.source) || !world.entities().contains(p.target) {
        return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None });
    }

    // Spawn the edge entity and attach Source/Target and edge kind marker
    let entity = {
        let mut e = world.spawn_empty();
        let entity = e.id();
        e.insert(gearbox::transitions::Source(p.source))
            .insert(gearbox::transitions::Target(p.target))
            .insert(gearbox::transitions::EdgeKind::External);
        entity
    };

    // Determine marker and display name
    let edge_label: String;
    if p.kind == "Always" {
        // Insert AlwaysEdge outside the spawn scope
        world.entity_mut(entity).insert(gearbox::transitions::AlwaysEdge);
        edge_label = "Always".to_string();
    } else {
        // Find a reflected component registration for EventEdge<T> whose inner T simple name matches p.kind
        use bevy::reflect::TypeRegistration;
        let reg_arc = world.resource::<AppTypeRegistry>().0.clone();
        let reg_read = reg_arc.read();
        let mut found: Option<&TypeRegistration> = None;
        for registration in reg_read.iter() {
            let ty_path = registration.type_info().type_path();
            if !ty_path.contains(crate::components::EVENT_EDGE_SUBSTR) { continue; }
            if let Some(inner) = inner_generic(ty_path) {
                if simple_type_name(inner) == p.kind {
                    found = Some(registration);
                    break;
                }
            }
        }
        let Some(registration) = found else {
            // Clean up the spawned placeholder to avoid leaks
            let _ = world.despawn(entity);
            return Err(BrpError { code: error_codes::INVALID_PARAMS, message: format!("unknown event edge kind: {}", p.kind), data: None });
        };

        // Insert the reflected EventEdge<T> component via ReflectComponent.
        // Use an empty DynamicStruct so from_reflect_with_fallback uses the reflected Default.
        if let Some(refl_comp) = registration.data::<bevy::ecs::reflect::ReflectComponent>() {
            let refl_comp_cloned = refl_comp.clone();
            drop(reg_read);
            let mut ew = world.entity_mut(entity);
            let empty = bevy::reflect::DynamicStruct::default();
            let reg_read_again = reg_arc.read();
            refl_comp_cloned.insert(&mut ew, &empty, &*reg_read_again);
        } else {
            let _ = world.despawn(entity);
            return Err(BrpError { code: error_codes::INTERNAL_ERROR, message: "not a ReflectComponent".to_string(), data: None });
        }

        edge_label = p.kind.clone();
    }

    // Auto-name edge for editor labeling
    if !edge_label.is_empty() {
        world.entity_mut(entity).insert(Name::new(edge_label));
    }

    Ok(serde_json::json!({"entity": entity.to_bits()}))
}

fn register_editor_transition_rpcs(app: &mut App) {
    if !app.world().contains_resource::<RemoteMethods>() { return; }
    let world = app.main_mut().world_mut();
    let create_id = world.register_system(create_transition_handler);
    let mut methods = world.resource_mut::<RemoteMethods>();
    methods.insert(EDITOR_CREATE_TRANSITION, RemoteMethodSystemId::Instant(create_id));
}

// =========================
// Machine graph RPC (string-centric wire format)
// =========================
#[derive(serde::Deserialize)]
struct MachineGraphParams { entity: Entity }

fn machine_graph_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let p: MachineGraphParams = serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|e| BrpError {
        code: error_codes::INVALID_PARAMS,
        message: format!("invalid params: {e}"),
        data: None,
    })?;

    if !world.entities().contains(p.entity) {
        return Err(BrpError { code: error_codes::INVALID_PARAMS, message: "invalid entity".to_string(), data: None });
    }

    // Traverse states (Substates hierarchy) and collect nodes
    use std::collections::{HashSet, VecDeque, BTreeMap};
    let mut visited: HashSet<Entity> = HashSet::new();
    let mut q_children = world.query::<&gearbox::Substates>();
    let mut q_name = world.query::<&Name>();
    let mut q_initial = world.query::<Option<&gearbox::InitialState>>();
    let mut q_transitions = world.query::<Option<&gearbox::transitions::Transitions>>();
    let mut q_targeted_by = world.query::<Option<&gearbox::transitions::TargetedBy>>();

    let mut nodes: Vec<Value> = Vec::new();
    let mut edges: Vec<Value> = Vec::new();

    let mut queue: VecDeque<(Option<Entity>, Entity)> = VecDeque::new();
    queue.push_back((None, p.entity));
    while let Some((parent, cur)) = queue.pop_front() {
        if !visited.insert(cur) { continue; }
        // Collect node fields (string-centric)
        let mut components: BTreeMap<String, Value> = BTreeMap::new();
        if let Some(name) = q_name.get(world, cur).ok() { components.insert(crate::components::NAME.to_string(), Value::String(name.as_str().to_string())); }
        if let Some(init) = q_initial.get(world, cur).ok().flatten() {
            // InitialState points to a child; serialize as string of entity bits for simplicity
            components.insert(crate::components::INITIAL_STATE.to_string(), Value::String(init.0.to_bits().to_string()));
        }
        // Include StateMachineId if present to allow clients to resolve sidecar path
        if let Some(id) = world.get::<gearbox::StateMachineId>(cur) {
            components.insert(crate::components::STATE_MACHINE_ID.to_string(), Value::String(id.0.clone()));
        }
        // Children
        let mut children_ids: Vec<String> = Vec::new();
        if let Some(children) = q_children.get(world, cur).ok() {
            for c in children.into_iter().copied() { children_ids.push(c.to_bits().to_string()); queue.push_back((Some(cur), c)); }
        }
        if !children_ids.is_empty() {
            components.insert(crate::components::STATE_CHILDREN.to_string(), Value::Array(children_ids.into_iter().map(|s| Value::String(s)).collect()));
        }

        // Relationships for edges (provide adjacency directly on nodes)
        if let Some(transitions) = q_transitions.get(world, cur).ok().flatten() {
            let ids: Vec<Value> = transitions
                .into_iter()
                .copied()
                .map(|e| Value::String(e.to_bits().to_string()))
                .collect();
            if !ids.is_empty() {
                components.insert(crate::components::TRANSITIONS.to_string(), Value::Array(ids));
            }
        }
        if let Some(incoming) = q_targeted_by.get(world, cur).ok().flatten() {
            let ids: Vec<Value> = incoming
                .into_iter()
                .copied()
                .map(|e| Value::String(e.to_bits().to_string()))
                .collect();
            if !ids.is_empty() {
                components.insert(crate::components::TARGETED_BY.to_string(), Value::Array(ids));
            }
        }

        nodes.push(serde_json::json!({
            "id": cur.to_bits().to_string(),
            "parent": parent.map(|p| p.to_bits().to_string()),
            "components": components,
        }));

        // Edges from this node
        if let Some(transitions) = q_transitions.get(world, cur).ok().flatten() {
            for edge in transitions.into_iter().copied() {
                // Minimal edge fields: id/source/target and a few components as strings
                let mut ecomps: BTreeMap<String, Value> = BTreeMap::new();
                if let Some(t) = world.get::<gearbox::transitions::Target>(edge) {
                    ecomps.insert(crate::components::TARGET.to_string(), Value::String(t.0.to_bits().to_string()));
                }
                if world.get::<gearbox::transitions::AlwaysEdge>(edge).is_some() {
                    ecomps.insert(crate::components::ALWAYS_EDGE.to_string(), Value::String("true".to_string()));
                }
                if let Some(name) = world.get::<Name>(edge) {
                    ecomps.insert(crate::components::NAME.to_string(), Value::String(name.as_str().to_string()));
                }
                edges.push(serde_json::json!({
                    "id": edge.to_bits().to_string(),
                    "source": cur.to_bits().to_string(),
                    "target": ecomps.get(crate::components::TARGET).cloned().unwrap_or(Value::Null),
                    "components": ecomps,
                }));
            }
        }
    }

    Ok(serde_json::json!({
        "root": p.entity.to_bits().to_string(),
        "nodes": nodes,
        "edges": edges,
        "version": 1u32,
    }))
}

fn register_editor_machine_graph_rpc(app: &mut App) {
    if !app.world().contains_resource::<RemoteMethods>() { return; }
    let world = app.main_mut().world_mut();
    let id = world.register_system(machine_graph_handler);
    let mut methods = world.resource_mut::<RemoteMethods>();
    methods.insert(EDITOR_MACHINE_GRAPH, RemoteMethodSystemId::Instant(id));
}

#[derive(Default)]
struct MachineTracker {
    transition_seq: u64,
    events: VecDeque<Value>,
}

fn push_event(tracker: &mut MachineTracker, ev: Value) {
    if tracker.events.len() >= RING_CAPACITY { tracker.events.pop_front(); }
    tracker.events.push_back(ev);
}


fn on_transition_edge(
    transition: On<gearbox::EdgeTraversed>,
    q_source: Query<&gearbox::transitions::Source>,
    q_substate_of: Query<&gearbox::SubstateOf>,
    mut trackers: ResMut<MachineTrackers>,
) {
    let edge = transition.target;
    let Ok(source) = q_source.get(edge) else { return; };
    let root = q_substate_of.root_ancestor(source.0);
    let tr = trackers.trackers.entry(root).or_default();
    tr.transition_seq = tr.transition_seq.saturating_add(1);
    let ev = serde_json::json!({
        "kind": "transition_edge",
        "seq": tr.transition_seq,
        "edge": entity_to_bits(edge).to_string(),
    });
    push_event(tr, ev);
}

fn on_state_entered(
    enter_state: On<gearbox::EnterState>,
    q_substate_of: Query<&gearbox::SubstateOf>,
    mut trackers: ResMut<MachineTrackers>,
) {
    let target = enter_state.target;
    let root = q_substate_of.root_ancestor(target);
    let tr = trackers.trackers.entry(root).or_default();
    tr.transition_seq = tr.transition_seq.saturating_add(1);
    let ev = serde_json::json!({
        "kind": "state_entered",
        "seq": tr.transition_seq,
        "entity": entity_to_bits(target).to_string(),
    });
    push_event(tr, ev);
}

fn on_state_exited(
    exit_state: On<gearbox::ExitState>,
    q_substate_of: Query<&gearbox::SubstateOf>,
    mut trackers: ResMut<MachineTrackers>,
) {
    let target = exit_state.target;
    let root = q_substate_of.root_ancestor(target);
    let tr = trackers.trackers.entry(root).or_default();
    tr.transition_seq = tr.transition_seq.saturating_add(1);
    let ev = serde_json::json!({
        "kind": "state_exited",
        "seq": tr.transition_seq,
        "entity": entity_to_bits(target).to_string(),
    });
    push_event(tr, ev);
}