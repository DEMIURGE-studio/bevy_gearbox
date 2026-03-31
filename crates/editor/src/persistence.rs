use bevy::prelude::*;
use bevy_egui::egui;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ======================
// Editor sidecar schema
// ======================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarViewport { pub pan: (f32, f32), pub zoom: f32 }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeLayout { pub pos: (f32, f32), pub collapsed: bool }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EdgeLayout { pub pill_center: (f32, f32) }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Sidecar {
    pub schema_version: u32,
    pub scene_basename: Option<String>,
    pub scene_hash: Option<String>,
    pub graph_fingerprint: Option<String>,
    pub viewport: Option<SidecarViewport>,
    pub nodes: std::collections::HashMap<String, NodeLayout>,
    pub edges: std::collections::HashMap<String, EdgeLayout>,
}

impl Sidecar {
    pub fn new() -> Self {
        Self { schema_version: 1, scene_basename: None, scene_hash: None, graph_fingerprint: None, viewport: None, nodes: Default::default(), edges: Default::default() }
    }
}

pub fn load_sidecar(path: impl AsRef<std::path::Path>) -> std::io::Result<Sidecar> {
    let text = std::fs::read_to_string(path)?;
    let sidecar: Sidecar = ron::de::from_str(&text).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("ron: {e}")))?;
    Ok(sidecar)
}

pub fn parse_sidecar_text(text: &str) -> std::io::Result<Sidecar> {
    let sidecar: Sidecar = ron::de::from_str(text).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("ron: {e}")))?;
    Ok(sidecar)
}

// ======================
// Key + fingerprint API
// ======================

use crate::editor::view_model::GraphDoc;
use crate::model::StateMachineGraph;
use crate::types::EntityId;

fn get_node_name(graph: &StateMachineGraph, id: &EntityId) -> String { graph.get_display_name(id) }

fn build_parent_path(graph: &StateMachineGraph, id: &EntityId) -> String {
    // Build path from root down to the provided parent id, inclusive.
    let mut parts: Vec<String> = Vec::new();
    let mut cur = Some(*id);
    while let Some(cid) = cur { parts.push(get_node_name(graph, &cid)); cur = graph.get_parent(&cid); }
    parts.reverse();
    parts.join("/")
}

pub fn node_key(graph: &StateMachineGraph, id: &EntityId) -> String {
    // Simplified variant without explicit state variant string for now
    let parent = graph.get_parent(id);
    let parent_path = match parent { Some(pid) => build_parent_path(graph, &pid), None => String::new() };
    let name = get_node_name(graph, id);
    format!("{}|{}", parent_path, name)
}

// Legacy key (pre-fix): exclude the immediate parent from parent_path.
fn legacy_build_parent_path(graph: &StateMachineGraph, id: &EntityId) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = Some(*id);
    while let Some(cid) = cur { let p = graph.get_parent(&cid); if p.is_some() { cur = p; parts.push(get_node_name(graph, &cid)); continue; } else { parts.push(get_node_name(graph, &cid)); break; } }
    if parts.len() >= 1 { let _ = parts.remove(0); }
    parts.reverse();
    parts.join("/")
}

fn legacy_node_key(graph: &StateMachineGraph, id: &EntityId) -> String {
    let parent = graph.get_parent(id);
    let parent_path = match parent { Some(pid) => legacy_build_parent_path(graph, &pid), None => String::new() };
    let name = get_node_name(graph, id);
    format!("{}|{}", parent_path, name)
}

pub fn edge_key(graph: &StateMachineGraph, eid: &EntityId) -> String {
    let e = match graph.edges.get(eid) { Some(e) => e, None => return format!("{:?}", eid) };
    let src = node_key(graph, &e.source);
    let dst = node_key(graph, &e.target);
    let label = e.display_label.clone().unwrap_or_else(|| "Edge".to_string());
    format!("{} -> {}|{}", src, dst, label)
}

fn legacy_edge_key(graph: &StateMachineGraph, eid: &EntityId) -> String {
    let e = match graph.edges.get(eid) { Some(e) => e, None => return format!("{:?}", eid) };
    let src = legacy_node_key(graph, &e.source);
    let dst = legacy_node_key(graph, &e.target);
    let label = e.display_label.clone().unwrap_or_else(|| "Edge".to_string());
    format!("{} -> {}|{}", src, dst, label)
}

pub fn compute_graph_fingerprint(graph: &StateMachineGraph) -> String {
    let mut node_keys: Vec<String> = graph.nodes.keys().map(|id| node_key(graph, id)).collect();
    node_keys.sort();
    let mut edge_keys: Vec<String> = graph.edges.keys().map(|id| edge_key(graph, id)).collect();
    edge_keys.sort();
    let canonical = format!("nodes:\n{}\nedges:\n{}", node_keys.join("\n"), edge_keys.join("\n"));
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let bytes = hasher.finalize();
    format!("sha256:{}", hex::encode(bytes))
}

// ===============
// Apply/Extract
// ===============

/// Extract a sidecar limited to the subtree rooted at `root`.
pub fn extract_sidecar_for_subtree(doc: &GraphDoc, root: &EntityId) -> Sidecar {
    let mut sc = Sidecar::new();
    if let Some(graph) = &doc.graph {
        // Compute a local origin for subtree serialization: the subtree root's current world min
        let base_min = doc.scene.node_rects.get(root).map(|r| r.min).unwrap_or(egui::pos2(0.0, 0.0));
        // Collect subtree nodes (DFS) and internal edges
        use std::collections::{HashSet, VecDeque};
        let mut nodes_set: HashSet<EntityId> = HashSet::new();
        let mut q: VecDeque<EntityId> = VecDeque::new();
        q.push_back(*root);
        while let Some(cur) = q.pop_front() { if !nodes_set.insert(cur) { continue; } for child in graph.get_children(&cur).into_iter() { q.push_back(child); } }
        // Build a temporary subgraph for fingerprinting
        let mut sub = crate::model::StateMachineGraph::new(crate::model::StateNode::new(*root));
        // Insert nodes
        for id in nodes_set.iter() {
            if let Some(n) = graph.nodes.get(id) { sub.nodes.insert(*id, n.clone()); }
        }
        // Insert edges where both endpoints are within the subtree
        for (eid, e) in graph.edges.iter() {
            if nodes_set.contains(&e.source) && nodes_set.contains(&e.target) {
                sub.edges.insert(*eid, e.clone());
            }
        }
        sc.graph_fingerprint = Some(compute_graph_fingerprint(&sub));

        // Capture views only for nodes/edges in the subtree
        for (id, sv) in doc.scene.states.iter() {
            if !nodes_set.contains(id) { continue; }
            let key = node_key(&sub, id);
            sc.nodes.insert(key, NodeLayout { pos: (sv.rect.min.x - base_min.x, sv.rect.min.y - base_min.y), collapsed: false });
        }
        for (eid, _ev) in doc.scene.edges.iter() {
            if !sub.edges.contains_key(eid) { continue; }
            let key = edge_key(&sub, eid);
            let center = doc.scene.node_rects.get(eid).map(|r| r.center()).unwrap_or(egui::pos2(0.0, 0.0));
            sc.edges.insert(key, EdgeLayout { pill_center: (center.x - base_min.x, center.y - base_min.y) });
        }
    }
    sc.viewport = Some(SidecarViewport { pan: (doc.transform.pan.x, doc.transform.pan.y), zoom: doc.transform.zoom });
    sc
}

pub fn apply_sidecar_to_doc(doc: &mut GraphDoc, sidecar: &Sidecar) {
    if let Some(graph) = &doc.graph {
        for (id, sv) in doc.scene.states.iter_mut() {
            let key = node_key(graph, id);
            let mut found = sidecar.nodes.get(&key);
            if found.is_none() {
                let legacy = legacy_node_key(graph, id);
                found = sidecar.nodes.get(&legacy);
            }
            if let Some(n) = found {
                let size = sv.rect.size();
                sv.rect = egui::Rect::from_min_size(egui::pos2(n.pos.0, n.pos.1), size);
                if let Some(r) = doc.scene.node_rects.get_mut(id) { *r = sv.rect; }
            }
        }
        for (eid, ev) in doc.scene.edges.iter_mut() {
            let key = edge_key(graph, eid);
            let mut found = sidecar.edges.get(&key);
            if found.is_none() {
                let legacy = legacy_edge_key(graph, eid);
                found = sidecar.edges.get(&legacy);
            }
            if let Some(e) = found {
                let size = ev.rect.size();
                let min = egui::pos2(e.pill_center.0 - size.x * 0.5, e.pill_center.1 - size.y * 0.5);
                ev.rect = egui::Rect::from_min_size(min, size);
                if let Some(r) = doc.scene.node_rects.get_mut(eid) { *r = ev.rect; }
            }
        }
    }
}

/// Apply a sidecar whose keys were generated relative to a subtree rooted at `root`.
/// This matches keys using a temporary subgraph that contains only the subtree and
/// overlays positions for nodes/edges within that subtree in the full document.
pub fn apply_sidecar_to_subtree(doc: &mut GraphDoc, sidecar: &Sidecar, root: &EntityId) {
    use std::collections::{HashSet, VecDeque};
    if doc.graph.is_none() { return; }
    let graph = doc.graph.clone().unwrap();
    // World-space offset to place subtree-local positions: current subtree root min
    let base_min = doc.scene.node_rects.get(root).map(|r| r.min).unwrap_or(egui::pos2(0.0, 0.0));
    // 1) Collect subtree node ids
    let mut nodes_set: HashSet<EntityId> = HashSet::new();
    let mut q: VecDeque<EntityId> = VecDeque::new();
    q.push_back(*root);
    while let Some(cur) = q.pop_front() { if !nodes_set.insert(cur) { continue; } for child in graph.get_children(&cur).into_iter() { q.push_back(child); } }
    // 2) Build a temporary subgraph for key computation
    let mut sub = crate::model::StateMachineGraph::new(crate::model::StateNode::new(*root));
    for id in nodes_set.iter() {
        if let Some(n) = graph.nodes.get(id) { sub.nodes.insert(*id, n.clone()); }
    }
    for (eid, e) in graph.edges.iter() {
        if nodes_set.contains(&e.source) && nodes_set.contains(&e.target) { sub.edges.insert(*eid, e.clone()); }
    }
    // 3) Apply node/edge layouts using subtree-relative keys
    for (id, sv) in doc.scene.states.iter_mut() {
        if !nodes_set.contains(id) { continue; }
        let key = node_key(&sub, id);
        let mut found = sidecar.nodes.get(&key);
        if found.is_none() {
            let legacy = legacy_node_key(&sub, id);
            found = sidecar.nodes.get(&legacy);
        }
        if let Some(n) = found {
            let size = sv.rect.size();
            sv.rect = egui::Rect::from_min_size(egui::pos2(n.pos.0 + base_min.x, n.pos.1 + base_min.y), size);
            if let Some(r) = doc.scene.node_rects.get_mut(id) { *r = sv.rect; }
        }
    }
    for (eid, ev) in doc.scene.edges.iter_mut() {
        if !sub.edges.contains_key(eid) { continue; }
        let key = edge_key(&sub, eid);
        let mut found = sidecar.edges.get(&key);
        if found.is_none() {
            let legacy = legacy_edge_key(&sub, eid);
            found = sidecar.edges.get(&legacy);
        }
        if let Some(e) = found {
            let size = ev.rect.size();
            let center = egui::pos2(e.pill_center.0 + base_min.x, e.pill_center.1 + base_min.y);
            let min = egui::pos2(center.x - size.x * 0.5, center.y - size.y * 0.5);
            ev.rect = egui::Rect::from_min_size(min, size);
            if let Some(r) = doc.scene.node_rects.get_mut(eid) { *r = ev.rect; }
        }
    }
    // Do not apply viewport for subtree overlays; keep parent's viewport unchanged
}


