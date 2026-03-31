use std::collections::{HashMap, HashSet};

use bevy_egui::egui;

use crate::types::EntityId as NodeId;

use super::canvas::CanvasTransform;

/// Configuration for node-only layout and interaction.
#[derive(Debug, Clone)]
pub struct LayoutConfig {
    /// Padding from a parent node's outer rect to its inner content area (world units).
    pub content_padding: egui::Vec2,
    /// Header height for container nodes (world units). Applied when a node has children.
    pub header_height_world: f32,
    /// Minimal node size (world units).
    pub min_node_size: egui::Vec2,
    /// Whether to clamp child nodes to the left/top of the parent's content area.
    /// Right/bottom are not clamped; parents will expand to include children.
    pub clamp_left_top: bool,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            content_padding: egui::vec2(24.0, 24.0),
            header_height_world: 24.0,
            min_node_size: egui::vec2(140.0, 60.0),
            clamp_left_top: true,
        }
    }
}

/// Edge-agnostic node layout state.
/// Holds node rects and hierarchy relationships; performs ordering and layout updates.
#[derive(Debug, Default, Clone)]
pub struct NodeLayout {
    /// World-space rectangles for nodes.
    pub node_rects: HashMap<NodeId, egui::Rect>,
    /// Parent for each node (None for root).
    pub parent_of: HashMap<NodeId, Option<NodeId>>,
    /// Children in display order for each node.
    pub children_of: HashMap<NodeId, Vec<NodeId>>,
    /// Nodes that should be treated as containers (i.e., have headers/content areas).
    /// This is intentionally independent from `children_of` so that leaves that own
    /// attachment children (like pills) are not considered containers.
    pub container_nodes: HashSet<NodeId>,
    /// Optional known root; if absent, a root is inferred as the node with no parent.
    pub root: Option<NodeId>,
    /// Deterministic node-only draw order (parents first, then descendants).
    pub draw_order_nodes: Vec<NodeId>,
}

impl NodeLayout {
    pub fn new(
        node_rects: HashMap<NodeId, egui::Rect>,
        parent_of: HashMap<NodeId, Option<NodeId>>,
        children_of: HashMap<NodeId, Vec<NodeId>>,
        container_nodes: HashSet<NodeId>,
        root: Option<NodeId>,
    ) -> Self {
        Self { node_rects, parent_of, children_of, container_nodes, root, draw_order_nodes: Vec::new() }
    }

    /// Returns true if the node is a container (explicitly listed in `container_nodes`).
    pub fn is_container(&self, id: &NodeId) -> bool {
        self.container_nodes.contains(id)
    }

    /// Returns the root node, preferring the configured root, else inferring from `parent_of`.
    pub fn root(&self) -> Option<NodeId> {
        if let Some(r) = self.root { return Some(r); }
        for (id, p) in &self.parent_of {
            if p.is_none() { return Some(*id); }
        }
        None
    }

    /// Returns the world-space header rect for a node. For leaves, returns the full rect.
    pub fn header_rect(&self, id: &NodeId, cfg: &LayoutConfig) -> Option<egui::Rect> {
        let rect = *self.node_rects.get(id)?;
        if self.is_container(id) {
            let max = egui::pos2(rect.max.x, rect.min.y + cfg.header_height_world);
            Some(egui::Rect::from_min_max(rect.min, max))
        } else {
            Some(rect)
        }
    }

    /// Compute node-only draw order: containers first, then descendants in DFS.
    /// If `selected` is provided, the selected branch is scheduled last among siblings along its ancestor chain.
    pub fn compute_draw_order(&mut self, selected: Option<NodeId>) -> &[NodeId] {
        let mut order: Vec<NodeId> = Vec::new();

        // Build selection bias map: parent -> child to place last
        let mut selected_by_parent: HashMap<NodeId, NodeId> = HashMap::new();
        if let Some(sel) = selected {
            // Only bias if the selected is a known node
            if self.node_rects.contains_key(&sel) {
                let mut cur = Some(sel);
                while let Some(cid) = cur {
                    if let Some(pid) = self.parent_of.get(&cid).and_then(|p| *p) {
                        selected_by_parent.insert(pid, cid);
                        cur = Some(pid);
                    } else {
                        break;
                    }
                }
            }
        }

        let Some(root) = self.root() else { self.draw_order_nodes = order; return &self.draw_order_nodes; };

        // Non-recursive DFS to build order deterministically
        let mut stack: Vec<NodeId> = Vec::new();
        stack.push(root);

        while let Some(id) = stack.pop() {
            // Enter: draw containers first (backgrounds), leaves also emit themselves now
            order.push(id);

            // Determine child order
            let mut children: Vec<NodeId> = self.children_of.get(&id).cloned().unwrap_or_default();
            if let Some(sel_child) = selected_by_parent.get(&id) {
                if let Some(pos) = children.iter().position(|c| c == sel_child) {
                    let v = children.remove(pos);
                    children.push(v);
                }
            }

            // Schedule children in reverse (stack is LIFO → original order preserved)
            for child in children.into_iter().rev() {
                stack.push(child);
            }
        }

        self.draw_order_nodes = order;
        &self.draw_order_nodes
    }

    /// Clamp each child to the left/top content bounds of its parent.
    pub fn clamp_children_left_top(&mut self, cfg: &LayoutConfig) {
        if !cfg.clamp_left_top { return; }
        let mut updates: Vec<(NodeId, egui::Rect)> = Vec::new();
        for (id, parent_opt) in self.parent_of.clone().into_iter() {
            let Some(parent) = parent_opt else { continue };
            let Some(parent_rect) = self.node_rects.get(&parent).cloned() else { continue };
            let Some(child_rect) = self.node_rects.get(&id).cloned() else { continue };

            let header = if self.is_container(&parent) { cfg.header_height_world } else { 0.0 };
            let content_min = egui::pos2(
                parent_rect.min.x + cfg.content_padding.x,
                parent_rect.min.y + cfg.content_padding.y + header,
            );

            let mut rect = child_rect;
            if rect.min.x < content_min.x {
                let dx = content_min.x - rect.min.x;
                rect = rect.translate(egui::vec2(dx, 0.0));
            }
            if rect.min.y < content_min.y {
                let dy = content_min.y - rect.min.y;
                rect = rect.translate(egui::vec2(0.0, dy));
            }
            if rect != child_rect { updates.push((id, rect)); }
        }
        for (id, rect) in updates { self.node_rects.insert(id, rect); }
    }

    /// Expand or shrink each parent to include its children and optional attachments.
    /// Attachments are arbitrary world-space rects grouped by parent (e.g., edge pills).
    pub fn fit_parents_to_children(
        &mut self,
        cfg: &LayoutConfig,
        attachments_by_parent: Option<&HashMap<NodeId, Vec<egui::Rect>>>,
    ) {
        let mut updates: Vec<(NodeId, egui::Rect)> = Vec::new();
        for (id, view_rect) in self.node_rects.clone().into_iter() {
            // Only consider nodes that have children recorded
            let Some(children) = self.children_of.get(&id) else { continue };
            if children.is_empty() { continue; }

            let base_min = view_rect.min;
            let header = if self.is_container(&id) { cfg.header_height_world } else { 0.0 };

            let mut req_max = egui::pos2(
                base_min.x + cfg.content_padding.x,
                base_min.y + cfg.content_padding.y + header,
            );

            for child_id in children.iter() {
                if let Some(child_rect) = self.node_rects.get(child_id) {
                    req_max.x = req_max.x.max(child_rect.max.x + cfg.content_padding.x);
                    req_max.y = req_max.y.max(child_rect.max.y + cfg.content_padding.y);
                }
            }

            if let Some(attachments) = attachments_by_parent.and_then(|m| m.get(&id)) {
                for r in attachments.iter() {
                    req_max.x = req_max.x.max(r.max.x + cfg.content_padding.x);
                    req_max.y = req_max.y.max(r.max.y + cfg.content_padding.y);
                }
            }

            // Enforce minimal size
            req_max.x = req_max.x.max(base_min.x + cfg.min_node_size.x);
            req_max.y = req_max.y.max(base_min.y + cfg.min_node_size.y);

            let new_rect = egui::Rect::from_min_max(base_min, req_max);
            if new_rect != view_rect { updates.push((id, new_rect)); }
        }
        for (id, rect) in updates { self.node_rects.insert(id, rect); }
    }

    /// Move a node and all its transform-descendants by `delta`.
    pub fn propagate_move_from(&mut self, moved: NodeId, delta: egui::Vec2) {
        if delta == egui::Vec2::ZERO { return; }
        // Apply to moved node
        if let Some(r) = self.node_rects.get_mut(&moved) { *r = r.translate(delta); }
        // Depth-first over children
        let mut stack: Vec<NodeId> = self.children_of.get(&moved).cloned().unwrap_or_default();
        while let Some(id) = stack.pop() {
            if let Some(r) = self.node_rects.get_mut(&id) { *r = r.translate(delta); }
            if let Some(more) = self.children_of.get(&id) { for &c in more { stack.push(c); } }
        }
    }

    /// Move a node to a desired world-space min position, clamped against its parent's content area.
    /// Returns the applied delta that was propagated to descendants.
    pub fn move_node_clamped_and_propagate(
        &mut self,
        id: NodeId,
        desired_min: egui::Pos2,
        cfg: &LayoutConfig,
    ) -> egui::Vec2 {
        let Some(old_rect) = self.node_rects.get(&id).copied() else { return egui::Vec2::ZERO };
        let size = old_rect.size();
        let mut new_rect = egui::Rect::from_min_size(desired_min, size);

        if let Some(parent) = self.parent_of.get(&id).and_then(|p| *p) {
            if let Some(parent_rect) = self.node_rects.get(&parent).copied() {
                let header = if self.is_container(&parent) { cfg.header_height_world } else { 0.0 };
                let content_min = egui::pos2(
                    parent_rect.min.x + cfg.content_padding.x,
                    parent_rect.min.y + cfg.content_padding.y + header,
                );
                if cfg.clamp_left_top {
                    if new_rect.min.x < content_min.x {
                        let dx = content_min.x - new_rect.min.x;
                        new_rect = new_rect.translate(egui::vec2(dx, 0.0));
                    }
                    if new_rect.min.y < content_min.y {
                        let dy = content_min.y - new_rect.min.y;
                        new_rect = new_rect.translate(egui::vec2(0.0, dy));
                    }
                }
            }
        }

        let delta = new_rect.min - old_rect.min;
        if delta == egui::Vec2::ZERO { return egui::Vec2::ZERO; }
        // Update moved node and descendants
        self.propagate_move_from(id, delta);
        delta
    }

    /// Compute the interactive screen-space rect for a node (header for containers; full rect for leaves).
    pub fn interactive_rect_screen(&self, id: &NodeId, cfg: &LayoutConfig, xf: &CanvasTransform) -> Option<egui::Rect> {
        let rect_w = *self.node_rects.get(id)?;
        if self.is_container(id) {
            let header_w = egui::Rect::from_min_max(rect_w.min, egui::pos2(rect_w.max.x, rect_w.min.y + cfg.header_height_world));
            Some(egui::Rect::from_min_max(xf.to_screen(header_w.min), xf.to_screen(header_w.max)))
        } else {
            Some(egui::Rect::from_min_max(xf.to_screen(rect_w.min), xf.to_screen(rect_w.max)))
        }
    }

    /// Returns a suggested pan delta (in screen space) when dragging near viewport edges.
    pub fn autopan_suggestion(viewport_screen: egui::Rect, cursor_screen: egui::Pos2, margin: f32, step: f32) -> egui::Vec2 {
        let mut pan = egui::Vec2::ZERO;
        if cursor_screen.x < viewport_screen.min.x + margin { pan.x += step; }
        if cursor_screen.x > viewport_screen.max.x - margin { pan.x -= step; }
        if cursor_screen.y < viewport_screen.min.y + margin { pan.y += step; }
        if cursor_screen.y > viewport_screen.max.y - margin { pan.y -= step; }
        pan
    }
}


