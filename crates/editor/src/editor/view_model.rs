use std::collections::HashMap;
use bevy::prelude::*;
use bevy_egui::egui;
use crate::model::StateMachineGraph;
use super::canvas::CanvasTransform;
use crate::types::EntityId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKind { Leaf, Sequence, Parallel }

#[derive(Debug, Clone)]
pub struct StateView {
    pub id: EntityId,
    pub rect: egui::Rect,
    pub label: String,
    pub kind: StateKind,
}

#[derive(Debug, Clone)]
pub struct EdgeView {
    pub id: EntityId,
    pub source: EntityId,
    pub target: EntityId,
    pub label: String,
    /// Pill rect in world space
    pub rect: egui::Rect,
    /// Parent container that governs pill transform/clamping (sibling of target)
    pub pill_parent: Option<EntityId>,
}

#[derive(Debug, Default, Clone)]
pub struct LayoutTree {
    pub parent_of: HashMap<EntityId, Option<EntityId>>,
    pub children_of: HashMap<EntityId, Vec<EntityId>>,
    pub containers: std::collections::HashSet<EntityId>,
}

#[derive(Debug, Default, Clone)]
pub struct ViewScene {
    pub states: HashMap<EntityId, StateView>,
    pub edges: HashMap<EntityId, EdgeView>,
    pub node_rects: HashMap<EntityId, egui::Rect>,
    pub tree: LayoutTree,
    pub draw_order: Vec<EntityId>,
}

#[derive(Debug, Default)]
pub struct GraphDoc {
    pub graph: Option<StateMachineGraph>,
    /// World↔screen mapping for this document
    pub transform: CanvasTransform,
    /// Unified drag state for either a node or a pill (by EntityId)
    pub dragging: Option<EntityId>,
    pub drag_anchor_world: Option<egui::Vec2>,
    /// Prebuilt scene and layout
    pub scene: ViewScene,
    /// Mapping of parent -> initial child state (if any)
    pub initial_substate_of: HashMap<EntityId, EntityId>,
    /// Set of nodes that are the initial child of their parent
    pub is_initial_child: std::collections::HashSet<EntityId>,
    /// Cache of text label sizes in screen pixels keyed by (label, font_px_rounded)
    pub label_px_cache: std::sync::RwLock<HashMap<(String, u32), egui::Vec2>>,
    /// Flash intensities for nodes (newly active): 0..1
    pub node_flash: std::collections::HashMap<EntityId, f32>,
    /// Flash intensities for edges that just fired: 0..1
    pub edge_flash: std::collections::HashMap<EntityId, f32>,
    /// Fade for nodes that were deactivated: 0..1 (yellow -> base)
    pub node_fade: std::collections::HashMap<EntityId, f32>,
}


impl GraphDoc {
    /// Returns the cached screen-pixel size for a label at the current zoom, measuring if needed.
    pub fn cached_label_size_screen(&self, label: &str, zoom: f32, painter: &egui::Painter) -> egui::Vec2 {
        let font_px = (14.0 * zoom).clamp(6.0, 64.0);
        let key = (label.to_string(), font_px.round() as u32);
        if let Some(sz) = self.label_px_cache.read().ok().and_then(|m| m.get(&key).cloned()) { return sz; }
        let font_id = egui::FontId::proportional(font_px);
        let galley = painter.layout_no_wrap(label.to_string(), font_id, egui::Color32::WHITE);
        let size = galley.size();
        if let Ok(mut map) = self.label_px_cache.write() { map.insert(key, size); }
        size
    }

    /// Returns the cached world-space size for a label at the current zoom.
    pub fn cached_label_size_world(&self, label: &str, zoom: f32, painter: &egui::Painter) -> egui::Vec2 {
        let size_s = self.cached_label_size_screen(label, zoom, painter);
        egui::vec2(size_s.x / zoom, size_s.y / zoom)
    }
    /// Returns the rect for an entity (node or edge pill) if present.
    pub fn get_rect(&self, id: &EntityId) -> Option<egui::Rect> {
        self.scene.node_rects.get(id).copied()
    }

    /// Sets the rect for an entity (node or edge pill) if present.
    pub fn set_rect(&mut self, id: &EntityId, rect: egui::Rect) {
        if let Some(r) = self.scene.node_rects.get_mut(id) { *r = rect; }
        if let Some(sv) = self.scene.states.get_mut(id) { sv.rect = rect; }
        if let Some(ev) = self.scene.edges.get_mut(id) { ev.rect = rect; }
    }

    /// Returns the transform parent for an entity (node parent or pill parent).
    pub fn parent_of(&self, id: &EntityId) -> Option<EntityId> {
        self.scene.tree.parent_of.get(id).and_then(|p| *p)
    }

    /// Mark an edge as having just fired
    pub fn flash_edge(&mut self, id: EntityId) {
        self.edge_flash.insert(id, 1.0);
    }

    /// Decay all flash intensities; return true if any are still animating
    pub fn tick_highlights(&mut self, decay: f32) -> bool {
        let eps = 0.01;
        let mut any = false;
        self.node_flash.retain(|_, v| {
            *v *= decay;
            if *v > eps { any = true; true } else { false }
        });
        self.edge_flash.retain(|_, v| {
            *v *= decay;
            if *v > eps { any = true; true } else { false }
        });
        self.node_fade.retain(|_, v| {
            *v *= decay;
            if *v > eps { any = true; true } else { false }
        });
        any
    }
}


