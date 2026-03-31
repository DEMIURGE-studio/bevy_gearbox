use bevy::prelude::*;
use bevy_egui::egui;
use crate::model::StateMachineGraph;
use crate::types::EntityId;
use bevy_gearbox_protocol::components as c;
use super::view_model::{GraphDoc, StateKind, StateView, EdgeView, LayoutTree, ViewScene};

/// Merge a fresh snapshot into an existing GraphDoc, preserving layout where possible
pub fn project_graph_into_doc(doc: &mut GraphDoc, snapshot: StateMachineGraph) {
    // Preserve previous scene for rect carry-over
    let prev_scene = std::mem::take(&mut doc.scene);

    // Authoritative classification by components:
    // Edge if it has Target; otherwise treat as State.
    let mut edge_ids_by_target: std::collections::HashSet<EntityId> = std::collections::HashSet::new();
    for (eid, _edge) in snapshot.edges.iter() {
        if snapshot.entity_data.get(eid).map(|b| b.contains(c::TARGET)).unwrap_or(false) { edge_ids_by_target.insert(*eid); }
    }
    for (nid, _node) in snapshot.nodes.iter() {
        if snapshot.entity_data.get(nid).map(|b| b.contains(c::TARGET)).unwrap_or(false) { edge_ids_by_target.insert(*nid); }
    }

    // Build state views, preserving rects
    let mut states: std::collections::HashMap<EntityId, StateView> = std::collections::HashMap::new();
    let default_state_size = egui::vec2(140.0, 60.0);
    for (id, _node) in snapshot.nodes.iter() {
        // Skip any entity classified as an edge by Target component
        if edge_ids_by_target.contains(id) { continue; }
        let mut rect = prev_scene.node_rects.get(id).copied()
            .unwrap_or_else(|| egui::Rect::from_min_size(egui::pos2(0.0, 0.0), default_state_size));
        let label = snapshot.get_display_name(id);
        let is_container = !snapshot.get_children(id).is_empty();
        let has_initial = snapshot.has_component(id, c::INITIAL_STATE);
        let kind = if !is_container { StateKind::Leaf } else if !has_initial { StateKind::Parallel } else { StateKind::Sequence };
        // If previously a container and now leaf, shrink to default at same min
        if let Some(prev) = prev_scene.states.get(id) {
            if !matches!(prev.kind, StateKind::Leaf) && matches!(kind, StateKind::Leaf) {
                rect = egui::Rect::from_min_size(rect.min, default_state_size);
            }
        }
        states.insert(*id, StateView { id: *id, rect, label, kind });
    }

    // Initial placement for unseen nodes
    apply_initial_layout_for_unseen_nodes(&snapshot, &mut states);

    // Build edge views
    let mut edges: std::collections::HashMap<EntityId, EdgeView> = std::collections::HashMap::new();
    let default_pill_half = egui::vec2(40.0, 12.0);
    for (eid, edge) in snapshot.edges.iter() {
        // Only include entities classified as edges by Target
        if !edge_ids_by_target.contains(eid) { continue; }
        let center = {
            let s = states.get(&edge.source).map(|n| n.rect.center()).unwrap_or(egui::pos2(0.0, 0.0));
            let t = states.get(&edge.target).map(|n| n.rect.center()).unwrap_or(egui::pos2(0.0, 0.0));
            egui::pos2((s.x + t.x) * 0.5, (s.y + t.y) * 0.5)
        };
        let rect = prev_scene.node_rects.get(eid).copied()
            .unwrap_or_else(|| egui::Rect::from_center_size(center, default_pill_half * 2.0));
        let mut label = edge.display_label.clone().unwrap_or_else(|| snapshot.component_bag(eid).map(crate::model::choose_edge_label_bag).unwrap_or_else(|| "Edge".to_string()));
        if label == "Edge" { label = format!("{}", eid); }
        let pill_parent = compute_pill_parent_for_edge(&snapshot, edge.source, edge.target);
        edges.insert(*eid, EdgeView { id: *eid, source: edge.source, target: edge.target, label, rect, pill_parent });
    }

    // Deterministic orders
    let (node_order, edge_order) = compute_draw_orders(&snapshot);

    // Build layout tree (parent_of, children_of, containers)
    let mut parent_of: std::collections::HashMap<EntityId, Option<EntityId>> = std::collections::HashMap::new();
    let mut children_of: std::collections::HashMap<EntityId, Vec<EntityId>> = std::collections::HashMap::new();
    let mut containers: std::collections::HashSet<EntityId> = std::collections::HashSet::new();
    for (id, _node) in snapshot.nodes.iter() {
        parent_of.insert(*id, snapshot.get_parent(id));
        if !snapshot.get_children(id).is_empty() { containers.insert(*id); }
    }
    for (eid, ev) in edges.iter() { parent_of.insert(*eid, ev.pill_parent); }

    // Initialize children lists for all potential parents
    for id in states.keys() { children_of.entry(*id).or_default(); }
    // Pills first per parent in deterministic order
    for eid in edge_order.iter() {
        if let Some(Some(pid)) = parent_of.get(eid) { children_of.entry(*pid).or_default().push(*eid); }
    }
    // Then graph children
    for (pid, _node) in snapshot.nodes.iter() {
        let list = children_of.entry(*pid).or_default();
        for cid in snapshot.get_children(pid).into_iter() { list.push(cid); }
    }

    // Unified rects
    let mut node_rects: std::collections::HashMap<EntityId, egui::Rect> = std::collections::HashMap::new();
    for (id, sv) in states.iter() { node_rects.insert(*id, sv.rect); }
    for (id, ev) in edges.iter() { node_rects.insert(*id, ev.rect); }

    // Unified draw order: parents, edges, leaves
    let mut draw_order: Vec<EntityId> = Vec::new();
    for nid in node_order.iter() { if containers.contains(nid) { draw_order.push(*nid); } }
    for eid in edge_order.iter() { draw_order.push(*eid); }
    for nid in node_order.iter() { if !containers.contains(nid) { draw_order.push(*nid); } }

    // Initial child mapping (for indicators)
    let mut initial_substate_of: std::collections::HashMap<EntityId, EntityId> = std::collections::HashMap::new();
    let mut is_initial_child: std::collections::HashSet<EntityId> = std::collections::HashSet::new();
    for (id, _node) in snapshot.nodes.iter() {
        if snapshot.has_component(id, c::INITIAL_STATE) {
            // best-effort: use the first valid child that exists in graph
            for child in snapshot.get_children(id).into_iter() {
                initial_substate_of.insert(*id, child);
                is_initial_child.insert(child);
                break;
            }
        }
    }

    doc.graph = Some(snapshot);
    doc.scene = ViewScene { states, edges, node_rects, tree: LayoutTree { parent_of, children_of, containers }, draw_order };
    doc.initial_substate_of = initial_substate_of;
    doc.is_initial_child = is_initial_child;
}

/// Choose the pill parent for an edge. For edges between a parent and its child (in either
/// direction), the parent is always the pill parent. Otherwise, fallback to target sibling rule.
fn compute_pill_parent_for_edge(graph: &StateMachineGraph, source: EntityId, target: EntityId) -> Option<EntityId> {
    let src_parent = graph.get_parent(&source);
    let dst_parent = graph.get_parent(&target);

    // source is parent of target
    if dst_parent == Some(source) { return Some(source); }
    // target is parent of source
    if src_parent == Some(target) { return Some(target); }

    // Fallback: sibling of target → target's parent if present, else target itself
    dst_parent.or(Some(target))
}

fn apply_initial_layout_for_unseen_nodes(graph: &StateMachineGraph, node_views: &mut std::collections::HashMap<EntityId, StateView>) {
    let default_size = egui::vec2(140.0, 60.0);
    let v_spacing = 100.0;
    let content_padding = egui::vec2(24.0, 24.0);
    let header_h_world = 24.0;
    let origin = egui::pos2(100.0, 100.0);

    // DFS traversal to assign positions; maintain next row per parent
    let mut stack: Vec<EntityId> = Vec::new();
    if graph.nodes.contains_key(&graph.root) { stack.push(graph.root); }
    let mut next_row_per_parent: std::collections::HashMap<EntityId, usize> = std::collections::HashMap::new();

    while let Some(id) = stack.pop() {
        // Ensure parent processed before children
        let mut kids = graph.get_children(&id);
        kids.reverse();
        for child in kids.into_iter() { stack.push(child); }

        // Assign root if unseen
        if id == graph.root {
            if let Some(root_view) = node_views.get_mut(&id) {
                if root_view.rect.min == egui::pos2(0.0, 0.0) {
                    root_view.rect = egui::Rect::from_min_size(origin, default_size);
                }
            }
            continue;
        }

        // For other nodes, if unseen, place inside parent's content area
        let parent_id = match graph.get_parent(&id) { Some(p) => p, None => continue };
        let parent_min = node_views.get(&parent_id).map(|p| p.rect.min).unwrap_or(origin);
        let parent_has_header = !graph.get_children(&parent_id).is_empty();
        let row = *next_row_per_parent.entry(parent_id).or_insert(0);
        if let Some(view) = node_views.get_mut(&id) {
            if view.rect.min == egui::pos2(0.0, 0.0) {
                let x = parent_min.x + content_padding.x;
                let y = parent_min.y + content_padding.y + (if parent_has_header { header_h_world } else { 0.0 }) + row as f32 * v_spacing;
                view.rect = egui::Rect::from_min_size(egui::pos2(x, y), default_size);
                if let Some(r) = next_row_per_parent.get_mut(&parent_id) { *r += 1; }
            }
        }
    }
}

fn compute_draw_orders(graph: &StateMachineGraph) -> (Vec<EntityId>, Vec<EntityId>) {
    // Node order: DFS from root following children order
    let mut node_order: Vec<EntityId> = Vec::new();
    let mut stack: Vec<EntityId> = Vec::new();
    let mut seen: std::collections::HashSet<EntityId> = std::collections::HashSet::new();
    if graph.nodes.contains_key(&graph.root) { stack.push(graph.root); }
    while let Some(id) = stack.pop() {
        if !seen.insert(id) { continue; }
        node_order.push(id);
        let mut kids = graph.get_children(&id);
        kids.reverse();
        for child in kids.into_iter() { stack.push(child); }
    }

    // Build a ranking for nodes to order edges by source appearance
    let mut rank: std::collections::HashMap<EntityId, usize> = std::collections::HashMap::new();
    for (i, id) in node_order.iter().enumerate() { rank.insert(*id, i); }

    let mut edge_order: Vec<EntityId> = Vec::new();
    // Prefer adjacency_out per node order for determinism
    for node_id in node_order.iter() {
        if let Some(out) = graph.adjacency_out.get(node_id) {
            for eid in out { edge_order.push(*eid); }
        } else {
            // fallback: scan edges with this source, sorted by target rank then by Debug of id
            let mut edges: Vec<&crate::model::Edge> = graph.edges.values().filter(|e| &e.source == node_id).collect();
            edges.sort_by_key(|e| (*rank.get(&e.target).unwrap_or(&usize::MAX), format!("{:?}", e.id)));
            for e in edges { edge_order.push(e.id); }
        }
    }

    (node_order, edge_order)
}


