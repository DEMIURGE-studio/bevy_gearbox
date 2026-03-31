use super::view_model::{GraphDoc, StateKind};
use super::layout::{NodeLayout, LayoutConfig};
use super::context_menu::{build_context_menu, MenuItemKind, MenuSelection};
use crate::editor::workspace::{ RenameInline, EdgeBuildState, EdgeMenuState};
use crate::types::EntityId;
use bevy_egui::egui;

#[derive(Debug, Default, Clone)]
pub struct DocEvents {
    pub claim_drag: bool,
    pub drag_stopped: bool,
    pub context_menu_selection: Option<MenuSelection>,
    pub edge_build_set: Option<crate::editor::workspace::EdgeBuildState>,
    pub edge_build_clear: bool,
    pub edge_menu_open: Option<crate::editor::workspace::EdgeMenuState>,
    pub edge_menu_close: bool,
    pub pending_edge_create: Option<crate::editor::workspace::PendingEdgeCreate>,
    pub preview_edge_remove: Option<crate::editor::workspace::PreviewEdge>,
    pub rename_start: Option<RenameInline>,
    pub rename_edit: Option<RenameInline>,
    pub rename_commit: Option<RenameInline>,
    pub rename_cancel: Option<(EntityId, EntityId)>,
    pub set_edge_delay: Option<(EntityId, EntityId, f32)>,
    pub clear_edge_delay: Option<(EntityId, EntityId)>,
    // Inline delay editing lifecycle
    pub delay_start: Option<crate::editor::workspace::DelayInline>,
    pub delay_edit: Option<crate::editor::workspace::DelayInline>,
    pub delay_commit: Option<crate::editor::workspace::DelayInline>,
    pub delay_cancel: Option<(EntityId, EntityId)>,
    pub set_edge_kind: Option<(EntityId, EntityId, bool)>,
}

#[derive(Debug, Default, Clone)]
pub struct ViewBoardCtx {
    pub edge_build: Option<EdgeBuildState>,
    pub edge_menu: Option<EdgeMenuState>,
    pub available_event_edges: Vec<String>,
    pub preview_edges: Vec<crate::editor::workspace::PreviewEdge>,
    pub rename_inline: Option<RenameInline>,
    pub delay_inline: Option<crate::editor::workspace::DelayInline>,
    pub board_drag_owner: Option<EntityId>,
}

/// Minimal read-only view with pan/zoom and basic nodes/edges rendering.
pub fn draw_doc(
    ui: &mut egui::Ui,
    doc: &mut GraphDoc,
    selection: &mut Option<EntityId>,
    doc_id: EntityId,
    ctx: &ViewBoardCtx,
) -> DocEvents {
    let desired = ui.available_size_before_wrap();
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click_and_drag());
    draw_doc_on_board(ui, rect, &response, doc, selection, doc_id, ctx, true)
}

/// Draw a document onto a provided board rect, sharing input `response` with other docs.
/// If `draw_background` is true, paints the board background; otherwise skips it.
pub fn draw_doc_on_board(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    doc: &mut GraphDoc,
    selection: &mut Option<EntityId>,
    doc_id: EntityId,
    ctx: &ViewBoardCtx,
    draw_background: bool,
) -> DocEvents {
    let painter = ui.painter_at(rect);
    if draw_background {
        painter.rect_filled(rect, 0.0, egui::Color32::from_gray(20));
    }

    // Tick highlight animations and request repaint while animating
    let animating = doc.tick_highlights(0.92);
    if animating { ui.ctx().request_repaint(); }

    // Scene provides stable ordering; no per-frame edge ordering needed

    // Construct NodeLayout from the prebuilt scene
    let mut layout = NodeLayout::new(
        doc.scene.node_rects.clone(),
        doc.scene.tree.parent_of.clone(),
        doc.scene.tree.children_of.clone(),
        doc.scene.tree.containers.clone(),
        doc.graph.as_ref().map(|g| g.root),
    );
    let cfg = LayoutConfig::default();
    // Pre-draw layout updates
    layout.clamp_children_left_top(&cfg);
    layout.fit_parents_to_children(&cfg, None);
    // Sync rects back to scene
    for (id, rect) in layout.node_rects.iter() { doc.set_rect(id, *rect); }

    // Selection-aware draw order from layout: when selecting an edge, bias to its parent (or source)
    let effective_selected = doc.dragging.or(*selection);
    let selected_for_bias = effective_selected.and_then(|sel| {
        if let Some(ev) = doc.scene.edges.get(&sel) {
            if let Some(pid) = doc.scene.tree.parent_of.get(&sel).and_then(|p| *p) { Some(pid) } else { Some(ev.source) }
        } else { Some(sel) }
    });
    let base_order = layout.compute_draw_order(selected_for_bias).to_vec();

    // Overlay edges: selected state's incoming ∪ outgoing, or the selected edge itself
    let mut overlay_edges: std::collections::HashSet<EntityId> = std::collections::HashSet::new();
    if let Some(sel) = effective_selected {
        if doc.scene.edges.contains_key(&sel) { overlay_edges.insert(sel); }
        else if let Some(graph) = &doc.graph {
                    if let Some(out_ids) = graph.adjacency_out.get(&sel) { for e in out_ids { overlay_edges.insert(*e); } }
                    if let Some(in_ids) = graph.adjacency_in.get(&sel) { for e in in_ids { overlay_edges.insert(*e); } }
            overlay_edges.retain(|eid| doc.scene.edges.contains_key(eid));
        }
    }

    // Draw each edge only once: remove overlay edges from base order, then append them on top
    let mut order: Vec<EntityId> = base_order
        .into_iter()
        .filter(|id| !overlay_edges.contains(id))
        .collect();
    if !overlay_edges.is_empty() {
        for eid in doc.scene.draw_order.iter() { if overlay_edges.contains(eid) { order.push(*eid); } }
    }
    doc.scene.draw_order = order.clone();

    // Events accumulator for this doc frame
    let mut _events = DocEvents::default();

    // Interactions: node/pill dragging vs background pan; hit test in front-to-back order
    let pointer_pos = response.ctx.input(|i| i.pointer.hover_pos());
    let mut hovered_entity: Option<EntityId> = None;
    if let Some(pos) = pointer_pos {
        for eid in order.iter().rev() {
            if doc.scene.edges.contains_key(eid) {
                if let Some(edge) = doc.scene.edges.get(eid) {
                        let zoom = doc.transform.zoom;
                    let label_dyn = doc.graph.as_ref().map(|g| g.get_label_for(eid)).unwrap_or(edge.label.clone());
                    let text_size_s = doc.cached_label_size_screen(&label_dyn, zoom, &painter);
                        let pill_pad_x = 10.0 * zoom;
                        let pill_pad_y = 6.0 * zoom;
                        let pill_size_s = egui::vec2(text_size_s.x + 2.0 * pill_pad_x, text_size_s.y + 2.0 * pill_pad_y);
                    let center_w = doc.scene.node_rects.get(eid).map(|r| r.center()).unwrap_or(egui::pos2(0.0, 0.0));
                        let pill_center_s = doc.transform.to_screen(center_w);
                        let pill_rect_s = egui::Rect::from_center_size(pill_center_s, pill_size_s);
                        if pill_rect_s.contains(pos) { hovered_entity = Some(*eid); break; }
                    }
            } else {
                        if let Some(rect) = layout.interactive_rect_screen(eid, &cfg, &doc.transform) {
                            if rect.contains(pos) { hovered_entity = Some(*eid); break; }
                }
            }
        }
    }

    // On drag start: capture draggable if hovering; also select it. Else pan background
    if response.drag_started() && response.ctx.input(|i| i.pointer.primary_down()) {
        // Only allow this document to start a drag if no other doc owns the drag, or this doc already owns it
        let allowed = match ctx.board_drag_owner {
            Some(owner) => owner == doc_id,
            None => true,
        };
        if allowed {
            if let Some(ent) = hovered_entity {
                if let Some(cursor) = response.ctx.input(|i| i.pointer.hover_pos()) {
                let pointer_world = doc.transform.to_world(cursor);
                // Compute rect for entity (node rect or pill rect in world space)
                let rect_w = doc.scene.node_rects.get(&ent).copied().unwrap_or(egui::Rect::from_min_max(pointer_world, pointer_world));
                let anchor = egui::vec2(pointer_world.x - rect_w.min.x, pointer_world.y - rect_w.min.y);
                    doc.dragging = Some(ent);
                    doc.drag_anchor_world = Some(anchor);
                *selection = Some(ent);
                // Claim board-wide drag ownership for this document (entity drag)
                _events.claim_drag = true;
                }
            }
        }
    }

    // Click to select; clicking empty background clears selection
    if response.clicked() {
        *selection = hovered_entity;
    }

    // Right-click context menu trigger: only create an interact response for the hovered entity
    // so topmost widget wins for hit-testing, but keep menu open independently of hover.
    let mut context_menu_selection: Option<MenuSelection> = None;
    if doc.graph.is_some() && ctx.edge_build.is_none() {
        for eid in order.iter() {
            // Compute interactive rect in screen space: full rect for nodes, pill rect for edges
            let rect_screen = if let Some(sv) = doc.scene.states.get(eid) {
                egui::Rect::from_min_max(doc.transform.to_screen(sv.rect.min), doc.transform.to_screen(sv.rect.max))
            } else if let Some(ev) = doc.scene.edges.get(eid) {
                    let zoom = doc.transform.zoom;
                let label_dyn = doc.graph.as_ref().map(|g| g.get_label_for(eid)).unwrap_or(ev.label.clone());
                let text_size_s = doc.cached_label_size_screen(&label_dyn, zoom, &ui.painter());
                    let pill_pad_x = 10.0 * zoom;
                    let pill_pad_y = 6.0 * zoom;
                    let pill_size_s = egui::vec2(text_size_s.x + 2.0 * pill_pad_x, text_size_s.y + 2.0 * pill_pad_y);
                let center_w = doc.scene.node_rects.get(eid).map(|r| r.center()).unwrap_or(ev.rect.center());
                    let pill_center_s = doc.transform.to_screen(center_w);
                    egui::Rect::from_center_size(pill_center_s, pill_size_s)
            } else { continue };
            // Build a stable, collision-free id per doc, per entity, and per kind (node vs edge)
            let kind_tag: &str = if doc.scene.edges.contains_key(eid) { "edge" } else { "node" };
            let id = egui::Id::new(("node_ctx", doc_id, kind_tag)).with(*eid);
            let resp = ui.interact(rect_screen, id, egui::Sense::click());
            if resp.clicked() { *selection = Some(*eid); }
            resp.context_menu(|menu_ui| {
                *selection = Some(*eid);
                menu_ui.set_min_width(160.0);
                // If we're in edge-kind selection for this node, override with edge-kind menu
                if let Some(edge_menu) = ctx.edge_menu.clone() {
                    if edge_menu.doc == doc_id && edge_menu.target == *eid {
                        if menu_ui.button("Always").clicked() {
                            _events.pending_edge_create = Some(crate::editor::workspace::PendingEdgeCreate { doc: doc_id, source: edge_menu.source, target: edge_menu.target, kind: "Always".to_string() });
                            _events.preview_edge_remove = Some(crate::editor::workspace::PreviewEdge { doc: doc_id, source: edge_menu.source, target: edge_menu.target });
                            _events.edge_menu_close = true;
                            _events.edge_build_clear = true;
                            menu_ui.close();
                        }
                        for label in ctx.available_event_edges.clone().into_iter() {
                            if menu_ui.button(&label).clicked() {
                                _events.pending_edge_create = Some(crate::editor::workspace::PendingEdgeCreate { doc: doc_id, source: edge_menu.source, target: edge_menu.target, kind: label.clone() });
                                _events.preview_edge_remove = Some(crate::editor::workspace::PreviewEdge { doc: doc_id, source: edge_menu.source, target: edge_menu.target });
                                _events.edge_menu_close = true;
                                _events.edge_build_clear = true;
                                menu_ui.close();
                            }
                        }
                        menu_ui.separator();
                        if menu_ui.button("Cancel").clicked() {
                            _events.edge_menu_close = true;
                            _events.edge_build_clear = true;
                            menu_ui.close();
                        }
                        return;
                    }
                }
                if doc.scene.states.contains_key(eid) {
                        if let Some(graph) = &doc.graph {
                            let items = build_context_menu(graph, *eid);
                            if items.is_empty() {
                                menu_ui.close();
                                return;
                            }
                            for item in items.into_iter() {
                                if menu_ui.button(item.label).clicked() {
                                    let sel = match item.kind {
                                        MenuItemKind::MakeLeaf => MenuSelection::MakeLeaf { target: *eid },
                                        MenuItemKind::MakeParent => MenuSelection::MakeParent { target: *eid },
                                        MenuItemKind::MakeParallel => MenuSelection::MakeParallel { target: *eid },
                                        MenuItemKind::Save => MenuSelection::SaveStateMachine { target: *eid },
                                        MenuItemKind::SaveSubstates => MenuSelection::SaveSubstates { target: *eid },
                                        MenuItemKind::Rename => MenuSelection::RenameEntity { target: *eid },
                                        MenuItemKind::Delete => MenuSelection::DeleteEntity { target: *eid },
                                        MenuItemKind::MakeInitial { parent } => MenuSelection::MakeInitial { parent, new_initial: *eid },
                                        MenuItemKind::AddChild => MenuSelection::AddChildStateMachine { target: *eid },
                                    };
                                    context_menu_selection = Some(sel);
                                    menu_ui.close();
                                }
                            }
                        } else {
                            menu_ui.close();
                        }
                    } else {
                        // Edge context menu
                        // EdgeKind toggle
                        let mut is_internal_now = false;
                        if let Some(graph) = &doc.graph {
                            if let Some(bag) = graph.component_bag(eid) {
                                if let Some(entry) = bag.get(bevy_gearbox_protocol::components::EDGE_KIND) {
                                    is_internal_now = edge_kind_is_internal(&entry.value_json);
                                }
                            }
                        }
                        if is_internal_now {
                            if menu_ui.button("Mark External").clicked() {
                                _events.set_edge_kind = Some((doc_id, *eid, false));
                                menu_ui.close();
                            }
                        } else {
                            if menu_ui.button("Mark Internal").clicked() {
                                _events.set_edge_kind = Some((doc_id, *eid, true));
                                menu_ui.close();
                            }
                        }
                        menu_ui.separator();
                        // Edit delay inline
                        if menu_ui.button("Edit Delay…").clicked() {
                            // Seed buffer from current delay if present
                            let mut seed = String::new();
                            if let Some(graph) = &doc.graph {
                                if let Some(bag) = graph.component_bag(eid) {
                                    if let Some(entry) = bag.get(bevy_gearbox_protocol::components::DELAY) {
                                        if let Some(secs) = extract_delay_secs(&entry.value_json) { seed = format!("{}", secs); }
                                    }
                                }
                            }
                            _events.delay_start = Some(crate::editor::workspace::DelayInline { doc: doc_id, target: *eid, text: seed });
                            menu_ui.close();
                        }
                        if menu_ui.button("Clear Delay").clicked() {
                            _events.clear_edge_delay = Some((doc_id, *eid));
                            menu_ui.close();
                        }
                        menu_ui.separator();
                        if menu_ui.button("Rename").clicked() {
                            context_menu_selection = Some(MenuSelection::RenameEntity { target: *eid });
                            menu_ui.close();
                        }
                        if menu_ui.button("Delete").clicked() {
                            context_menu_selection = Some(MenuSelection::DeleteEntity { target: *eid });
                            menu_ui.close();
                    }
                }
            });
        }
        // Persistent menu rendering handled by egui::Context; nothing to draw here.
    }

    // During drag: move draggable in world coords, with clamping to parent content via NodeLayout
    // Drag delta consumed by shell for board-level background pan
    if response.dragged() {
        // Only process movement for the owning document if there is an owner
        if let Some(owner) = ctx.board_drag_owner {
            if owner == doc_id {
                if let (Some(ent), Some(anchor)) = (doc.dragging, doc.drag_anchor_world) {
                    if let Some(cursor) = response.ctx.input(|i| i.pointer.hover_pos()) {
                        let pointer_world = doc.transform.to_world(cursor);
                        let desired_min = egui::pos2(pointer_world.x - anchor.x, pointer_world.y - anchor.y);
                        if !doc.scene.edges.contains_key(&ent) {
                                let _ = layout.move_node_clamped_and_propagate(ent, desired_min, &cfg);
                                // Sync rects back to scene
                                for (id, rect) in layout.node_rects.iter() { doc.set_rect(id, *rect); }
                        } else {
                                // Compute pill size in world from cached label size, set desired rect, then clamp via layout
                                let label = doc.graph.as_ref().map(|g| g.get_label_for(&ent)).or_else(|| doc.scene.edges.get(&ent).map(|v| v.label.clone())).unwrap_or_default();
                                let zoom = doc.transform.zoom;
                                let size_s = doc.cached_label_size_screen(&label, zoom, &painter);
                                let pad_s = egui::vec2(10.0 * zoom, 6.0 * zoom);
                                let size_w = egui::vec2((size_s.x + 2.0 * pad_s.x) / zoom, (size_s.y + 2.0 * pad_s.y) / zoom);
                                let rect = egui::Rect::from_min_size(desired_min, size_w);
                                layout.node_rects.insert(ent, rect);
                                layout.clamp_children_left_top(&cfg);
                                // Sync rects back to scene
                                for (id, rect) in layout.node_rects.iter() { doc.set_rect(id, *rect); }
                        }
                    }
                } else {
                    // Board-level background drag pan handled in shell/layout.rs
                }
            }
        } else if let (Some(ent), Some(anchor)) = (doc.dragging, doc.drag_anchor_world) {
            // No owner yet; allow initial movement for the doc that captured this frame
            if let Some(cursor) = response.ctx.input(|i| i.pointer.hover_pos()) {
                let pointer_world = doc.transform.to_world(cursor);
                let desired_min = egui::pos2(pointer_world.x - anchor.x, pointer_world.y - anchor.y);
                if !doc.scene.edges.contains_key(&ent) {
                        let _ = layout.move_node_clamped_and_propagate(ent, desired_min, &cfg);
                        // Sync rects back to scene
                        for (id, rect) in layout.node_rects.iter() { doc.set_rect(id, *rect); }
                } else {
                        // Compute pill size in world from cached label size, set desired rect, then clamp via layout
                        let label = doc.graph.as_ref().map(|g| g.get_label_for(&ent)).or_else(|| doc.scene.edges.get(&ent).map(|v| v.label.clone())).unwrap_or_default();
                        let zoom = doc.transform.zoom;
                        let size_s = doc.cached_label_size_screen(&label, zoom, &painter);
                        let pad_s = egui::vec2(10.0 * zoom, 6.0 * zoom);
                        let size_w = egui::vec2((size_s.x + 2.0 * pad_s.x) / zoom, (size_s.y + 2.0 * pad_s.y) / zoom);
                        let rect = egui::Rect::from_min_size(desired_min, size_w);
                        layout.node_rects.insert(ent, rect);
                        layout.clamp_children_left_top(&cfg);
                        // Sync rects back to scene
                        for (id, rect) in layout.node_rects.iter() { doc.set_rect(id, *rect); }
                }
            }
        } else {
            // Board-level background drag pan handled in shell/layout.rs
        }
    }

    // Canvas autopan while dragging is handled in shell/layout.rs

    if response.drag_stopped() { doc.dragging = None; doc.drag_anchor_world = None; _events.drag_stopped = true; }

    // Draw graph if any
    if doc.graph.is_none() { _events.context_menu_selection = context_menu_selection; return _events; }

    // Layout handled via NodeLayout above; legacy pre-draw sizing/clamp pass removed.

    // Single-pass layered draw using computed order
    let zoom = doc.transform.zoom;
    let font_px = (14.0 * zoom).clamp(6.0, 64.0);
    let font_id = egui::FontId::proportional(font_px);
    let pad = 8.0 * zoom;
    // header height is derived from layout.header_rect; keep constant here only for sizing heuristics elsewhere if needed

    // Helpers for edge geometry
    let rect_from_inside_toward = |rect: egui::Rect, toward: egui::Pos2| -> egui::Pos2 {
        let c = rect.center();
        let d = toward - c;
        let hx = rect.width() * 0.5_f32;
        let hy = rect.height() * 0.5_f32;
        let sx = if d.x.abs() > 0.0001 { hx / d.x.abs() } else { f32::INFINITY };
        let sy = if d.y.abs() > 0.0001 { hy / d.y.abs() } else { f32::INFINITY };
        let s = sx.min(sy);
        c + d * s
    };
    let rect_from_outside_toward_center = |rect: egui::Rect, from: egui::Pos2| -> egui::Pos2 {
        rect_from_inside_toward(rect, from)
    };

    // Helper to draw a dashed rounded-rectangle border in screen space
    let draw_dashed_rounded_rect = |rect: egui::Rect, radius: f32, color: egui::Color32, thickness: f32, dash: f32, gap: f32| {
        let draw_segmented = |a: egui::Pos2, b: egui::Pos2| {
            let total_len = (b - a).length();
            if total_len <= 0.0 { return; }
            let dir = (b - a) / total_len;
            let mut t = 0.0;
            while t < total_len {
                let seg_len = dash.min(total_len - t);
                let start = a + dir * t;
                let end = a + dir * (t + seg_len);
                painter.line_segment([start, end], egui::Stroke { width: thickness, color });
                t += dash + gap;
            }
        };
        let draw_dashed_arc = |center: egui::Pos2, r: f32, a0: f32, a1: f32| {
            if r <= 0.0 { return; }
            let arc_len = r * (a1 - a0).abs();
            if arc_len <= 0.0 { return; }
            let dir_sign = if a1 >= a0 { 1.0 } else { -1.0 };
            let mut s = 0.0;
            while s < arc_len {
                let seg_len = dash.min(arc_len - s);
                let a_start = a0 + dir_sign * (s / r);
                let a_end = a0 + dir_sign * ((s + seg_len) / r);
                let p0 = egui::pos2(center.x + r * a_start.cos(), center.y + r * a_start.sin());
                let p1 = egui::pos2(center.x + r * a_end.cos(), center.y + r * a_end.sin());
                painter.line_segment([p0, p1], egui::Stroke { width: thickness, color });
                s += dash + gap;
            }
        };

        let x0 = rect.min.x;
        let y0 = rect.min.y;
        let x1 = rect.max.x;
        let y1 = rect.max.y;
        let r = radius.clamp(0.0, ((x1 - x0).abs().min((y1 - y0).abs())) * 0.5);

        if r <= 0.0 {
            // Fallback: square rectangle
            draw_segmented(egui::pos2(x0, y0), egui::pos2(x1, y0));
            draw_segmented(egui::pos2(x1, y0), egui::pos2(x1, y1));
            draw_segmented(egui::pos2(x1, y1), egui::pos2(x0, y1));
            draw_segmented(egui::pos2(x0, y1), egui::pos2(x0, y0));
            return;
        }

        // Straight segments (shortened by radius on both ends)
        draw_segmented(egui::pos2(x0 + r, y0), egui::pos2(x1 - r, y0)); // top
        draw_segmented(egui::pos2(x1, y0 + r), egui::pos2(x1, y1 - r)); // right
        draw_segmented(egui::pos2(x1 - r, y1), egui::pos2(x0 + r, y1)); // bottom
        draw_segmented(egui::pos2(x0, y1 - r), egui::pos2(x0, y0 + r)); // left

        // Corner arcs (screen space). Angles assume +x right, +y down.
        let pi = std::f32::consts::PI;
        // Top-left: from pi to 1.5*pi
        draw_dashed_arc(egui::pos2(x0 + r, y0 + r), r, pi, 1.5 * pi);
        // Top-right: from 1.5*pi to 2*pi
        draw_dashed_arc(egui::pos2(x1 - r, y0 + r), r, 1.5 * pi, 2.0 * pi);
        // Bottom-right: from 0 to 0.5*pi
        draw_dashed_arc(egui::pos2(x1 - r, y1 - r), r, 0.0, 0.5 * pi);
        // Bottom-left: from 0.5*pi to pi
        draw_dashed_arc(egui::pos2(x0 + r, y1 - r), r, 0.5 * pi, pi);
    };

    // Helper to draw "initial" indicator: small solid circle outside top-left with a curved arrow to the node's left edge
    let draw_initial_indicator = |rect_screen: egui::Rect| {
        // Solid circle, half previous size
        let r = 4.0 * zoom;
        let x_offset = 16.0 * zoom;
        let y_offset = 4.0 * zoom;
        let start = egui::pos2(rect_screen.min.x - x_offset, rect_screen.min.y - y_offset);

        // Terminate on the left edge of the node, slightly below the top-left corner
        let end = egui::pos2(rect_screen.min.x, rect_screen.min.y + 8.0 * zoom);

        // Cubic Bézier controls to start downward then turn right
        let k = 14.0 * zoom;
        let c1 = egui::pos2(start.x, start.y + k);         // vertical tangent at start
        let c2 = egui::pos2(end.x - k, end.y);             // horizontal tangent at end (pointing right)

        // Draw solid circle
        painter.circle_filled(start, r, egui::Color32::WHITE);

        // Sample and draw the cubic Bézier
        let segments = 20;
        let mut prev = start;
        for i in 1..=segments {
            let t = (i as f32) / (segments as f32);
            let omt = 1.0 - t;
            // Cubic Bézier interpolation
            let x = omt * omt * omt * start.x
                + 3.0 * omt * omt * t * c1.x
                + 3.0 * omt * t * t * c2.x
                + t * t * t * end.x;
            let y = omt * omt * omt * start.y
                + 3.0 * omt * omt * t * c1.y
                + 3.0 * omt * t * t * c2.y
                + t * t * t * end.y;
            let p = egui::pos2(x, y);
            painter.line_segment([prev, p], egui::Stroke::new(2.0, egui::Color32::WHITE));
            prev = p;
        }

        // Arrowhead pointing along the end tangent (cubic derivative at t=1 is proportional to end - c2)
        let end_tangent = (end - c2).normalized();
        let arrow_len = 10.0 * zoom;
        let arrow_w = 8.0 * zoom;
        let tip = end;
        let base = tip - end_tangent * arrow_len;
        let perp = egui::pos2(-end_tangent.y, end_tangent.x);
        let left = base + perp.to_vec2() * (arrow_w * 0.5);
        let right = base - perp.to_vec2() * (arrow_w * 0.5);
        painter.add(egui::Shape::convex_polygon(
            vec![tip, left, right],
            egui::Color32::WHITE,
            egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
        ));
    };

    // Helper: draw a translucent selection halo ring around a rect (screen space)
    let draw_selection_halo = |rect_screen: egui::Rect, rounding: egui::CornerRadius| {
        let halo_w = (0.75 + 0.5 * zoom.sqrt()).clamp(0.75, 2.0);
        let halo_rect = rect_screen.expand(4.0);
        painter.rect(
            halo_rect,
            rounding,
            egui::Color32::TRANSPARENT,
            egui::Stroke::new(halo_w, egui::Color32::from_rgba_premultiplied(120, 180, 255, 32)),
            egui::StrokeKind::Outside,
        );
    };

    // Helper: see free function `is_direct_substate_of_parallel`

    for id in order.iter() {
        if let Some(sv) = doc.scene.states.get(id) {
            // Debug: print classification and name when drawing as a state
            let is_container = !matches!(sv.kind, StateKind::Leaf);
            if is_container {
                let rect_world = sv.rect;
                let min = doc.transform.to_screen(rect_world.min);
                let max = doc.transform.to_screen(rect_world.max);
                let rect_screen = egui::Rect::from_min_max(min, max);
                let rounding = egui::CornerRadius::same(6);
                // Fill (container body stays gray; header changes color)
                let base_fill = egui::Color32::from_rgb(30, 30, 35);
                let base_yellow = egui::Color32::from_rgb(230, 200, 40);
                let bright_yellow = egui::Color32::from_rgb(255, 240, 0);
                let lerp_color = |a: egui::Color32, b: egui::Color32, t: f32| -> egui::Color32 {
                    let cl = |x: f32| -> u8 { x.clamp(0.0, 255.0) as u8 };
                    let ta = t.clamp(0.0, 1.0);
                    let inv = 1.0 - ta;
                    let r = a.r() as f32 * inv + b.r() as f32 * ta;
                    let g = a.g() as f32 * inv + b.g() as f32 * ta;
                    let bch = a.b() as f32 * inv + b.b() as f32 * ta;
                    egui::Color32::from_rgb(cl(r), cl(g), cl(bch))
                };
                painter.rect_filled(rect_screen, rounding, base_fill);
                let header_rect_world = layout.header_rect(id, &cfg).unwrap_or(rect_world);
                let header_rect = egui::Rect::from_min_max(doc.transform.to_screen(header_rect_world.min), doc.transform.to_screen(header_rect_world.max));
                let is_active = doc.graph.as_ref().map(|g| g.is_active(id)).unwrap_or(false);
                let flash_t = doc.node_flash.get(id).copied().unwrap_or(0.0);
                let fade_t = doc.node_fade.get(id).copied().unwrap_or(0.0);

                let base_header_color = egui::Color32::from_rgb(38, 38, 46);
                let mut header_color = base_header_color;

                if is_active {
                    header_color = base_yellow;
                } else if fade_t > 0.0 {
                    header_color = lerp_color(base_yellow, base_header_color, 1.0 - fade_t);
                }

                if flash_t > 0.0 {
                    header_color = lerp_color(header_color, bright_yellow, flash_t);
                }

                painter.rect_filled(header_rect, egui::CornerRadius::same(6), header_color);
                painter.hline(header_rect.x_range(), header_rect.max.y, egui::Stroke::new(1.0, egui::Color32::from_gray(90)));
                // Text color: black when header is yellow, else white; lerp on fade
                let mut text_col = egui::Color32::WHITE;
                if is_active {
                    text_col = egui::Color32::BLACK;
                } else if fade_t > 0.0 {
                    // approximate: crossfade black->white opposite to header fade
                    let alpha = 1.0 - fade_t;
                    text_col = lerp_color(egui::Color32::BLACK, egui::Color32::WHITE, alpha);
                }
                // Inline rename for container nodes
                let edit_rect = egui::Rect::from_min_max(
                    egui::pos2(header_rect.min.x + pad, header_rect.center().y - 10.0 * zoom),
                    egui::pos2(header_rect.max.x - pad, header_rect.center().y + 10.0 * zoom),
                );
                draw_label_or_inline_editor(
                    ui,
                    ctx,
                    doc_id,
                    id,
                    edit_rect,
                    &painter,
                    egui::pos2(header_rect.min.x + pad, header_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &doc.graph.as_ref().map(|g| g.get_label_for(id)).unwrap_or_else(|| sv.label.clone()),
                    &font_id,
                    text_col,
                    &mut _events,
                );
                // Selection halo (drawn before border so border stays crisp)
                let is_selected = selection.as_ref().map(|s| *s == *id).unwrap_or(false);
                if is_selected { draw_selection_halo(rect_screen, egui::CornerRadius::same(8)); }
                // Arrow-handle to start edge building when selected and not already building
                if is_selected && ctx.edge_build.is_none() {
                    let handle_r = 6.0 * zoom;
                    let margin = 4.0 * zoom;
                    let handle_center = egui::pos2(
                        rect_screen.max.x - (handle_r + margin),
                        rect_screen.min.y + (handle_r + margin),
                    );
                    let handle_rect = egui::Rect::from_center_size(handle_center, egui::vec2(handle_r * 2.0, handle_r * 2.0));
                    let hid = egui::Id::new(("edge_handle", doc_id, "node")).with(*id);
                    let hresp = ui.interact(handle_rect, hid, egui::Sense::click());
                    // Blue circle
                    painter.circle_filled(handle_center, handle_r, egui::Color32::from_rgb(110, 190, 255));
                    // White plus
                    let plus_len = handle_r * 1.0;
                    let half = plus_len * 0.5;
                    let stroke = egui::Stroke::new(1.5 * zoom.max(1.0), egui::Color32::WHITE);
                    painter.line_segment([egui::pos2(handle_center.x - half, handle_center.y), egui::pos2(handle_center.x + half, handle_center.y)], stroke);
                    painter.line_segment([egui::pos2(handle_center.x, handle_center.y - half), egui::pos2(handle_center.x, handle_center.y + half)], stroke);
                    if hresp.clicked() {
                        _events.edge_build_set = Some(EdgeBuildState { doc: doc_id, source: *id, just_started: true });
                    }
                }
                // Border: dashed if direct child of a Parallel (draw after header so it stays visible)
                let is_direct_substate_of_parallel = is_direct_substate_of_parallel(doc, id);
                if is_direct_substate_of_parallel {
                    let dash = 6.0;
                    let gap = 4.0;
                    draw_dashed_rounded_rect(rect_screen, 6.0, egui::Color32::from_gray(160), 1.0, dash, gap);
                } else {
                    painter.rect(
                        rect_screen,
                        rounding,
                        egui::Color32::TRANSPARENT,
                        egui::Stroke::new(1.0, egui::Color32::from_gray(160)),
                        egui::StrokeKind::Outside,
                    );
                }
                // Initial indicator for nodes that are the parent's initial child
                if doc.is_initial_child.contains(id) {
                    draw_initial_indicator(rect_screen);
                }
            } else {
                // Leaf state rendering (see container branch for shared helpers)
                let rect_world = sv.rect;
                let min = doc.transform.to_screen(rect_world.min);
                let max = doc.transform.to_screen(rect_world.max);
                let rect_screen = egui::Rect::from_min_max(min, max);
                let rounding = egui::CornerRadius::same(6);
                // Fill (leaf body changes fully; header rule doesn't apply here)
                let base_fill = egui::Color32::from_rgb(30, 30, 35);
                let base_yellow = egui::Color32::from_rgb(230, 200, 40);
                let bright_yellow = egui::Color32::from_rgb(255, 240, 0);
                let lerp_color = |a: egui::Color32, b: egui::Color32, t: f32| -> egui::Color32 {
                    let cl = |x: f32| -> u8 { x.clamp(0.0, 255.0) as u8 };
                    let ta = t.clamp(0.0, 1.0);
                    let inv = 1.0 - ta;
                    let r = a.r() as f32 * inv + b.r() as f32 * ta;
                    let g = a.g() as f32 * inv + b.g() as f32 * ta;
                    let bch = a.b() as f32 * inv + b.b() as f32 * ta;
                    egui::Color32::from_rgb(cl(r), cl(g), cl(bch))
                };
                let is_active = doc.graph.as_ref().map(|g| g.is_active(id)).unwrap_or(false);
                let flash_t = doc.node_flash.get(id).copied().unwrap_or(0.0);
                let fade_t = doc.node_fade.get(id).copied().unwrap_or(0.0);

                let mut fill_color = base_fill;

                if is_active {
                    fill_color = base_yellow;
                } else if fade_t > 0.0 {
                    fill_color = lerp_color(base_yellow, base_fill, 1.0 - fade_t);
                }

                if flash_t > 0.0 {
                    fill_color = lerp_color(fill_color, bright_yellow, flash_t);
                }

                painter.rect_filled(rect_screen, rounding, fill_color);
                // Selection halo (drawn before border so border stays crisp)
                let is_selected = selection.as_ref().map(|s| *s == *id).unwrap_or(false);
                if is_selected { draw_selection_halo(rect_screen, egui::CornerRadius::same(8)); }
                // Arrow-handle to start edge building when selected and not already building
                if is_selected && ctx.edge_build.is_none() {
                    let handle_r = 6.0 * zoom;
                    let margin = 4.0 * zoom;
                    let handle_center = egui::pos2(
                        rect_screen.max.x - (handle_r + margin),
                        rect_screen.min.y + (handle_r + margin),
                    );
                    let hresp = ui.interact(
                        egui::Rect::from_center_size(handle_center, egui::vec2(handle_r * 2.0, handle_r * 2.0)),
                        egui::Id::new(("edge_handle", doc_id, *id)),
                        egui::Sense::click(),
                    );
                    // Blue circle
                    painter.circle_filled(handle_center, handle_r, egui::Color32::from_rgb(110, 190, 255));
                    // White plus
                    let plus_len = handle_r * 1.0;
                    let half = plus_len * 0.5;
                    let stroke = egui::Stroke::new(1.5 * zoom.max(1.0), egui::Color32::WHITE);
                    painter.line_segment([egui::pos2(handle_center.x - half, handle_center.y), egui::pos2(handle_center.x + half, handle_center.y)], stroke);
                    painter.line_segment([egui::pos2(handle_center.x, handle_center.y - half), egui::pos2(handle_center.x, handle_center.y + half)], stroke);
                    if hresp.clicked() {
                        _events.edge_build_set = Some(EdgeBuildState { doc: doc_id, source: *id, just_started: true });
                    }
                }
                // Border: dashed if direct child of a Parallel
                let is_direct_substate_of_parallel = is_direct_substate_of_parallel(doc, id);
                if is_direct_substate_of_parallel {
                    let dash = 6.0;
                    let gap = 4.0;
                    draw_dashed_rounded_rect(rect_screen, 6.0, egui::Color32::from_gray(160), 1.0, dash, gap);
                } else {
            painter.rect(
                rect_screen,
                rounding,
                        egui::Color32::TRANSPARENT,
                egui::Stroke::new(1.0, egui::Color32::from_gray(160)),
                egui::StrokeKind::Outside,
            );
                }
                // Text color: black when yellow-ish, else white; also lerp on fade
                let mut text_col = egui::Color32::WHITE;
                if is_active { text_col = egui::Color32::BLACK; }
                else if fade_t > 0.0 {
                    let alpha = 1.0 - fade_t;
                    text_col = lerp_color(egui::Color32::BLACK, egui::Color32::WHITE, alpha);
                }
                // Inline rename for leaf nodes
                let label_top = rect_screen.center_top() + egui::vec2(0.0, 12.0 * zoom);
                let edit_rect = egui::Rect::from_min_max(
                    egui::pos2(rect_screen.min.x + 8.0 * zoom, label_top.y - 2.0 * zoom),
                    egui::pos2(rect_screen.max.x - 8.0 * zoom, label_top.y + 18.0 * zoom),
                );
                draw_label_or_inline_editor(
                    ui,
                    ctx,
                    doc_id,
                    id,
                    edit_rect,
                    &painter,
                    label_top,
                    egui::Align2::CENTER_TOP,
                    &doc.graph.as_ref().map(|g| g.get_label_for(id)).unwrap_or_else(|| sv.label.clone()),
                    &font_id,
                    text_col,
                    &mut _events,
                );
                // Initial indicator for nodes that are the parent's initial child
                if doc.is_initial_child.contains(id) {
                    draw_initial_indicator(rect_screen);
                }
            }
        } else if let Some(ev) = doc.scene.edges.get(id) {
            // Edge rendering
            let source = ev.source;
            let target = ev.target;
            let Some(src_view) = doc.scene.states.get(&source) else { continue };
            let Some(dst_view) = doc.scene.states.get(&target) else { continue };
                let src_rect_w = src_view.rect;
                let dst_rect_w = dst_view.rect;

                let src_rect_s = egui::Rect::from_min_max(doc.transform.to_screen(src_rect_w.min), doc.transform.to_screen(src_rect_w.max));
                let dst_rect_s = egui::Rect::from_min_max(doc.transform.to_screen(dst_rect_w.min), doc.transform.to_screen(dst_rect_w.max));

            let pill_center_s = doc.transform.to_screen(doc.scene.node_rects.get(id).map(|r| r.center()).unwrap_or(ev.rect.center()));
            let base_label = doc.graph.as_ref().map(|g| g.get_label_for(id)).unwrap_or(ev.label.clone());
            // Optional second line for Delay
            let mut delay_line: Option<String> = None;
            if let Some(graph) = &doc.graph {
                if let Some(bag) = graph.component_bag(id) {
                    if let Some(entry) = bag.get(bevy_gearbox_protocol::components::DELAY) {
                        delay_line = format_delay_tag(&entry.value_json);
                    }
                }
            }
            let name_size_s = doc.cached_label_size_screen(&base_label, zoom, &painter);
            let delay_size_s = delay_line.as_ref().map(|s| doc.cached_label_size_screen(s, zoom, &painter)).unwrap_or(egui::vec2(0.0, 0.0));
            let line_gap = 2.0 * zoom;
            let total_text_h = if delay_line.is_some() { name_size_s.y + line_gap + delay_size_s.y } else { name_size_s.y };
            let text_w = name_size_s.x.max(delay_size_s.x);
            let pill_pad_x = 10.0 * zoom;
            let pill_pad_y = 6.0 * zoom;
            let pill_size_s = egui::vec2(text_w + 2.0 * pill_pad_x, total_text_h + 2.0 * pill_pad_y);
            let pill_rect_s = egui::Rect::from_center_size(pill_center_s, pill_size_s);

                // Selection halo around pill (drawn before pill border)
                let is_selected = selection.as_ref().map(|s| *s == *id).unwrap_or(false);
                if is_selected {
                    let rounding = egui::CornerRadius::same((pill_size_s.y * 0.5).round() as u8);
                    draw_selection_halo(pill_rect_s, rounding);
                }

                let a_start = rect_from_inside_toward(src_rect_s, pill_center_s);
                let a_end = rect_from_outside_toward_center(pill_rect_s, a_start);
                // If source is an ancestor of target, draw an outward-and-back curve to create a loop illusion
                let mut is_ancestor_edge = false;
                {
                    let mut cur = Some(target);
                    while let Some(cid) = cur {
                        if cid == source { is_ancestor_edge = true; break; }
                    cur = doc.scene.tree.parent_of.get(&cid).and_then(|p| *p);
                    }
                }
                // Edge highlight color (bright yellow -> base gray)
                let bright_yellow = egui::Color32::from_rgb(255, 240, 0);
                let base_gray_line = egui::Color32::from_gray(120);
                let base_gray_edge = egui::Color32::from_gray(160);
                let lerp_color = |a: egui::Color32, b: egui::Color32, t: f32| -> egui::Color32 {
                    let cl = |x: f32| -> u8 { x.clamp(0.0, 255.0) as u8 };
                    let ta = t.clamp(0.0, 1.0);
                    let inv = 1.0 - ta;
                    let r = a.r() as f32 * inv + b.r() as f32 * ta;
                    let g = a.g() as f32 * inv + b.g() as f32 * ta;
                    let bch = a.b() as f32 * inv + b.b() as f32 * ta;
                    egui::Color32::from_rgb(cl(r), cl(g), cl(bch))
                };
                let t_edge = doc.edge_flash.get(id).copied().unwrap_or(0.0);
                let alpha = 1.0 - t_edge;
                let edge_line_col = lerp_color(bright_yellow, base_gray_line, alpha);
                let edge_col = lerp_color(bright_yellow, base_gray_edge, alpha);
                // Edge pill body/text: flash bright yellow then lerp back to base fill & text to white
                let base_fill = egui::Color32::from_rgb(30, 30, 35);
                let pill_fill_col = lerp_color(bright_yellow, base_fill, alpha);
                let pill_text_col = lerp_color(egui::Color32::BLACK, egui::Color32::WHITE, alpha);

                // Determine if EdgeKind is Internal for dashed rendering
                let mut is_internal = false;
                if let Some(graph) = &doc.graph {
                    if let Some(bag) = graph.component_bag(id) {
                        if let Some(entry) = bag.get(bevy_gearbox_protocol::components::EDGE_KIND) {
                            is_internal = edge_kind_is_internal(&entry.value_json);
                        }
                    }
                }

                // Helper: dashed straight line
                let draw_dashed_line = |a: egui::Pos2, b: egui::Pos2, color: egui::Color32| {
                    let total = (b - a).length();
                    if total <= 0.0 { return; }
                    let dir = (b - a) / total;
                    let dash = 6.0 * zoom;
                    let gap = 4.0 * zoom;
                    let stroke_w = 2.0;
                    let mut t = 0.0f32;
                    while t < total {
                        let seg = dash.min(total - t);
                        let p0 = a + dir * t;
                        let p1 = a + dir * (t + seg);
                        painter.line_segment([p0, p1], egui::Stroke::new(stroke_w, color));
                        t += dash + gap;
                    }
                };

                if is_ancestor_edge {
                    // Determine outward normal based on which side a_start lies on
                    let eps = 0.5;
                    let normal = if (a_start.x - src_rect_s.min.x).abs() <= eps { egui::vec2(-1.0, 0.0) }
                    else if (a_start.x - src_rect_s.max.x).abs() <= eps { egui::vec2(1.0, 0.0) }
                    else if (a_start.y - src_rect_s.min.y).abs() <= eps { egui::vec2(0.0, -1.0) }
                    else if (a_start.y - src_rect_s.max.y).abs() <= eps { egui::vec2(0.0, 1.0) }
                    else {
                        // Fallback: pick axis-aligned direction away from rect center
                        let d = (a_start - src_rect_s.center()).normalized();
                        if d.x.abs() >= d.y.abs() { egui::vec2(d.x.signum(), 0.0) } else { egui::vec2(0.0, d.y.signum()) }
                    };
                    let loop_out = 48.0 * zoom; // loop distance in screen space
                    let p_out = a_start + normal * loop_out;
                    // Sample a quadratic bezier (a_start -> p_out -> a_end)
                    let segments = 20;
                    let mut prev = a_start;
                    for i in 1..=segments {
                        let t = (i as f32) / (segments as f32);
                        let omt = 1.0 - t;
                        let x = omt * omt * a_start.x + 2.0 * omt * t * p_out.x + t * t * a_end.x;
                        let y = omt * omt * a_start.y + 2.0 * omt * t * p_out.y + t * t * a_end.y;
                        let p = egui::pos2(x, y);
                        if is_internal {
                            // draw every other segment to emulate dashed curve
                            if i % 2 == 1 { painter.line_segment([prev, p], egui::Stroke::new(2.0, edge_line_col)); }
                        } else {
                            painter.line_segment([prev, p], egui::Stroke::new(2.0, edge_line_col));
                        }
                        prev = p;
                    }
                } else {
                if is_internal { draw_dashed_line(a_start, a_end, edge_line_col); } else { painter.line_segment([a_start, a_end], egui::Stroke::new(2.0, edge_line_col)); }
                }

                let b_end = rect_from_inside_toward(dst_rect_s, pill_center_s);
                let b_start = rect_from_outside_toward_center(pill_rect_s, b_end);
                if is_internal { draw_dashed_line(b_start, b_end, edge_line_col); } else { painter.line_segment([b_start, b_end], egui::Stroke::new(2.0, edge_line_col)); }

                let dir = (b_end - b_start).normalized();
                let arrow_len = 10.0 * zoom;
                let arrow_w = 8.0 * zoom;
                let tip = b_end;
                let base = tip - dir * arrow_len;
                let perp = egui::pos2(-dir.y, dir.x);
                let left = base + perp.to_vec2() * (arrow_w * 0.5);
                let right = base - perp.to_vec2() * (arrow_w * 0.5);
                painter.add(egui::Shape::convex_polygon(
                    vec![tip, left, right],
                    edge_col,
                egui::Stroke::new(0.0, egui::Color32::from_rgba_premultiplied(0, 0, 0, 0)),
                ));

                let rounding = egui::CornerRadius::same((pill_size_s.y * 0.5).round() as u8);
                painter.rect(
                    pill_rect_s,
                    rounding,
                    pill_fill_col,
                    egui::Stroke::new(1.0, edge_col),
                    egui::StrokeKind::Outside,
                );
                // Inline rename for edge pills (first line: Name)
                let edit_inset = egui::vec2(6.0 * zoom, 4.0 * zoom);
                let edit_rect_full = pill_rect_s.shrink2(edit_inset);
                let name_top = edit_rect_full.center().y - total_text_h * 0.5;
                let name_center = egui::pos2(edit_rect_full.center().x, name_top);
                // Allocate a rect just tall enough for the first line for text edit hitbox
                let name_edit_rect = egui::Rect::from_min_size(
                    egui::pos2(edit_rect_full.min.x, name_top),
                    egui::vec2(edit_rect_full.width(), name_size_s.y),
                );
                draw_label_or_inline_editor(
                    ui,
                    ctx,
                    doc_id,
                    id,
                    name_edit_rect,
                    &painter,
                    name_center,
                    egui::Align2::CENTER_TOP,
                    &base_label,
                    &font_id,
                    pill_text_col,
                    &mut _events,
                );
                // Second line: Delay text or inline editor
                let delay_top = name_top + name_size_s.y + line_gap;
                let delay_center = egui::pos2(edit_rect_full.center().x, delay_top);
                let delay_edit_rect = egui::Rect::from_min_size(
                    egui::pos2(edit_rect_full.min.x, delay_top),
                    egui::vec2(edit_rect_full.width(), delay_size_s.y.max(name_size_s.y)),
                );
                // If this edge is in delay-inline edit mode, draw input; else draw read-only text when present
                let in_delay_edit = ctx.delay_inline.as_ref().map(|d| d.doc == doc_id && d.target == *id).unwrap_or(false);
                if in_delay_edit {
                    draw_delay_inline_editor(
                        ui,
                        ctx,
                        doc_id,
                        id,
                        delay_edit_rect,
                        &painter,
                        delay_center,
                        &font_id,
                        pill_text_col,
                        &mut _events,
                    );
                } else if let Some(ref dl) = delay_line {
                    painter.text(delay_center, egui::Align2::CENTER_TOP, dl, font_id.clone(), pill_text_col);
                }
        }
    }

    // (old extra sizing pass removed; handled above)
    // (old extra sizing pass removed; handled above)
    // Edge-build interaction and preview rendering
    let mut _edge_cancel = false;
    let mut _open_edge_menu: Option<EdgeMenuState> = None;
    let mut _stop_dashed_build = false;
    if let Some(build) = ctx.edge_build.clone() {
        if build.doc == doc_id {
            // Determine preview end point: hovered node center (if valid) or cursor
            let cursor_opt = response.ctx.input(|i| i.pointer.hover_pos());
            let mut snap_to_target: Option<EntityId> = None;
            // Draw a dotted preview from source to cursor while building
            if let (Some(cursor), Some(src_view)) = (cursor_opt, doc.scene.states.get(&build.source)) {
                let src_rect_s = egui::Rect::from_min_max(doc.transform.to_screen(src_view.rect.min), doc.transform.to_screen(src_view.rect.max));
                let start = rect_from_inside_toward(src_rect_s, cursor);
                let end = cursor;
                let color = egui::Color32::from_rgb(160, 220, 255);
                let stroke_w = 2.0;
                // simple dashed line
                let draw_dashed_line = |a: egui::Pos2, b: egui::Pos2, dash: f32, gap: f32| {
                    let total = (b - a).length();
                    if total <= 0.0 { return; }
                    let dir = (b - a) / total;
                    let mut t = 0.0f32;
                    while t < total {
                        let seg = dash.min(total - t);
                        let p0 = a + dir * t;
                        let p1 = a + dir * (t + seg);
                        painter.line_segment([p0, p1], egui::Stroke::new(stroke_w, color));
                        t += dash + gap;
                    }
                };
                draw_dashed_line(start, end, 6.0, 4.0);
                // Arrow head at cursor
                let dir = (end - start).normalized();
                let arrow_len = 10.0 * doc.transform.zoom;
                let arrow_w = 8.0 * doc.transform.zoom;
                let tip = end;
                let base = tip - dir * arrow_len;
                let perp = egui::pos2(-dir.y, dir.x);
                let left = base + perp.to_vec2() * (arrow_w * 0.5);
                let right = base - perp.to_vec2() * (arrow_w * 0.5);
                painter.add(egui::Shape::convex_polygon(
                    vec![tip, left, right],
                    color,
                    egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
                ));
            }
            if let Some(cursor) = cursor_opt {
                for eid in order.iter().rev() {
                    if let Some(sv) = doc.scene.states.get(eid) {
                        let rect = egui::Rect::from_min_max(doc.transform.to_screen(sv.rect.min), doc.transform.to_screen(sv.rect.max));
                        if rect.contains(cursor) { snap_to_target = Some(*eid); break; }
                    }
                }
            }
            // Cancel build on escape
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) { _stop_dashed_build = true; }
            // Only confirm target selection on explicit click over a node
            let pressed = ui.input(|i| i.pointer.primary_pressed());
            if pressed {
                if let (Some(cursor), Some(target)) = (cursor_opt, snap_to_target) {
                    _open_edge_menu = Some(EdgeMenuState { doc: doc_id, source: build.source, target, pos: cursor, just_opened: true, filter: String::new() });
                    _stop_dashed_build = true;
                }
            }
        }
    }
    if _edge_cancel { _events.edge_build_clear = true; _events.edge_menu_close = true; }
    if let Some(m) = _open_edge_menu.take() { _events.edge_menu_open = Some(m); }
    if _stop_dashed_build { _events.edge_build_clear = true; }
    // When an edge-target is chosen but menu not necessarily open, draw solid preview until selection
    if let Some(menu) = ctx.edge_menu.clone() {
        if menu.doc == doc_id {
            if let (Some(src_view), Some(dst_view)) = (doc.scene.states.get(&menu.source), doc.scene.states.get(&menu.target)) {
                // draw solid preview omitted for brevity
            }
        }
    }
    // Edge kind menu popup
    if let Some(menu) = ctx.edge_menu.clone() {
        if menu.doc == doc_id {
            let w = 200.0;
            let mut filter_buf = menu.filter.clone();
            let popup = egui::Area::new(egui::Id::new(("edge_menu", doc_id)))
                .fixed_pos(menu.pos)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |menu_ui| {
                        menu_ui.set_min_width(w);
                        if menu_ui.add_sized(egui::vec2(w, 24.0), egui::Button::new("Always")).clicked() {
                            _events.pending_edge_create = Some(crate::editor::workspace::PendingEdgeCreate { doc: doc_id, source: menu.source, target: menu.target, kind: "Always".to_string() });
                            _events.preview_edge_remove = Some(crate::editor::workspace::PreviewEdge { doc: doc_id, source: menu.source, target: menu.target });
                            _events.edge_menu_close = true;
                            return;
                        }
                        menu_ui.separator();
                        egui::containers::ScrollArea::vertical().max_height(220.0).show(menu_ui, |menu_ui| {
                            let mut items: Vec<String> = ctx.available_event_edges.clone();
                            if !filter_buf.trim().is_empty() {
                                let q = filter_buf.to_lowercase();
                                items.retain(|label| label.to_lowercase().contains(&q));
                            }
                            for label in items.into_iter() {
                                if menu_ui.add_sized(egui::vec2(w, 24.0), egui::Button::new(&label)).clicked() {
                                    _events.pending_edge_create = Some(crate::editor::workspace::PendingEdgeCreate { doc: doc_id, source: menu.source, target: menu.target, kind: label.clone() });
                                    _events.preview_edge_remove = Some(crate::editor::workspace::PreviewEdge { doc: doc_id, source: menu.source, target: menu.target });
                                    _events.edge_menu_close = true;
                                    return;
                                }
                            }
                        });
                        menu_ui.separator();
                        if menu_ui.add_sized(egui::vec2(w, 24.0), egui::Button::new("Cancel")).clicked() {
                            _events.edge_menu_close = true;
                            return;
                        }
                    });
                });
            // Persist search filter via events is a future step; skip persistence here for purity
            // Close the popup on outside click, with one-frame suppression right after opening
            if ui.input(|i| i.pointer.any_pressed()) {
                if menu.just_opened {
                    // one-frame suppression handled implicitly; no mutation to ctx
                } else {
                    let pos_opt = ui.ctx().input(|i| i.pointer.hover_pos());
                    let inside = pos_opt.map(|p| popup.response.rect.contains(p)).unwrap_or(false);
                    if !inside { _events.edge_menu_close = true; }
                }
            }
        }
    }

    // Draw persisted preview edges (solid line with arrowhead)
    if doc.graph.is_some() {
        for pe in ctx.preview_edges.iter().filter(|pe| pe.doc == doc_id) {
            let Some(src_view) = doc.scene.states.get(&pe.source) else { continue };
            let Some(dst_view) = doc.scene.states.get(&pe.target) else { continue };
            let src_rect_s = egui::Rect::from_min_max(doc.transform.to_screen(src_view.rect.min), doc.transform.to_screen(src_view.rect.max));
            let dst_rect_s = egui::Rect::from_min_max(doc.transform.to_screen(dst_view.rect.min), doc.transform.to_screen(dst_view.rect.max));
            let start = rect_from_inside_toward(src_rect_s, dst_rect_s.center());
            let end = rect_from_inside_toward(dst_rect_s, src_rect_s.center());
            let color = egui::Color32::from_rgb(160, 220, 255);
            ui.painter().line_segment([start, end], egui::Stroke::new(2.0, color));
            let dir = (end - start).normalized();
            let arrow_len = 10.0 * zoom;
            let arrow_w = 8.0 * zoom;
            let tip = end;
            let base = tip - dir * arrow_len;
            let perp = egui::pos2(-dir.y, dir.x);
            let left = base + perp.to_vec2() * (arrow_w * 0.5);
            let right = base - perp.to_vec2() * (arrow_w * 0.5);
            ui.painter().add(egui::Shape::convex_polygon(
                vec![tip, left, right],
                color,
                egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
            ));
        }
    }

    // Board-level zoom handled in shell; skip here to avoid duplicating per-doc.

    _events.context_menu_selection = context_menu_selection;
    _events
}

/// Helper: draw a label normally, or an inline text editor if this entity is being renamed.
/// Commits on Enter (records pending_rename_commit) and cancels on click outside.
fn draw_label_or_inline_editor(
    ui: &mut egui::Ui,
    ctx: &ViewBoardCtx,
    doc_id: EntityId,
    target_id: &EntityId,
    edit_rect: egui::Rect,
    painter: &egui::Painter,
    text_pos: egui::Pos2,
    align: egui::Align2,
    label: &str,
    font_id: &egui::FontId,
    color: egui::Color32,
    events: &mut DocEvents,
) {
    let is_renaming = ctx.rename_inline.as_ref().map(|r| r.doc == doc_id && r.target == *target_id).unwrap_or(false);
    if is_renaming {
        // Work on a local buffer, then write back depending on commit/cancel
        let mut buf = ctx.rename_inline.as_ref().map(|r| r.text.clone()).unwrap_or_else(|| label.to_string());
        let mut commit = false;
        let mut cancelled = false;
        let id = egui::Id::new(("inline_rename", doc_id, target_id.0));
        let resp = ui.interact(edit_rect, id, egui::Sense::click());
        let response = ui.put(edit_rect, egui::TextEdit::singleline(&mut buf));
        // Commit only on Enter
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) { commit = true; }
        // click outside to cancel
        let clicked_outside = ui.input(|i| i.pointer.any_pressed()) && !resp.clicked() && !response.hovered();
        if clicked_outside { cancelled = true; }
        if commit {
            events.rename_commit = Some(RenameInline { doc: doc_id, target: *target_id, text: buf.clone() });
        } else if cancelled {
            events.rename_cancel = Some((doc_id, *target_id));
        } else {
            // Persist ongoing edit while typing
            if response.changed() {
                events.rename_edit = Some(RenameInline { doc: doc_id, target: *target_id, text: buf });
            }
        }
    } else {
        painter.text(text_pos, align, label, font_id.clone(), color);
    }
}

fn is_direct_substate_of_parallel(doc: &GraphDoc, child_id: &EntityId) -> bool {
    let parent_opt = doc.scene.tree.parent_of.get(child_id).and_then(|p| *p);
    let by_view = parent_opt
        .and_then(|pid| doc.scene.states.get(&pid))
        .map(|pv| matches!(pv.kind, StateKind::Parallel))
        .unwrap_or(false);
    if by_view { return true; }
    if let (Some(graph), Some(pid)) = (&doc.graph, parent_opt) {
        let has_initial = graph.has_component(&pid, bevy_gearbox_protocol::components::INITIAL_STATE);
        let has_children = !graph.get_children(&pid).is_empty();
        if has_children && !has_initial { return true; }
    }
    false
}

fn format_delay_tag(v: &serde_json::Value) -> Option<String> {
    // Expect object { duration: { secs, nanos } } or { duration: { secs_f64 } } or direct seconds number
    if let Some(obj) = v.as_object() {
        if let Some(dur) = obj.get("duration") {
            if let Some(d) = dur.as_object() {
                let secs = d.get("secs").and_then(|x| x.as_u64()).unwrap_or(0) as f64;
                let nanos = d.get("nanos").and_then(|x| x.as_u64()).unwrap_or(0) as f64;
                if secs > 0.0 || nanos > 0.0 {
                    let total = secs + nanos / 1_000_000_000.0;
                    return Some(format!("Delay: {:.2}s", total));
                }
                if let Some(sf) = d.get("secs_f64").and_then(|x| x.as_f64()) {
                    return Some(format!("Delay: {:.2}s", sf));
                }
                if let Some(sf) = d.get("secs_f32").and_then(|x| x.as_f64()) {
                    return Some(format!("Delay: {:.2}s", sf));
                }
            }
        }
    }
    if let Some(n) = v.as_f64() { return Some(format!("Delay: {:.2}s", n)); }
    None
}

fn extract_delay_secs(v: &serde_json::Value) -> Option<f64> {
    if let Some(obj) = v.as_object() {
        if let Some(dur) = obj.get("duration") {
            if let Some(d) = dur.as_object() {
                let secs = d.get("secs").and_then(|x| x.as_u64()).unwrap_or(0) as f64;
                let nanos = d.get("nanos").and_then(|x| x.as_u64()).unwrap_or(0) as f64;
                if secs > 0.0 || nanos > 0.0 { return Some(secs + nanos / 1_000_000_000.0); }
                if let Some(sf) = d.get("secs_f64").and_then(|x| x.as_f64()) { return Some(sf); }
                if let Some(sf) = d.get("secs_f32").and_then(|x| x.as_f64()) { return Some(sf); }
            }
        }
    }
    v.as_f64()
}

fn draw_delay_inline_editor(
    ui: &mut egui::Ui,
    ctx: &ViewBoardCtx,
    doc_id: EntityId,
    target_id: &EntityId,
    edit_rect: egui::Rect,
    painter: &egui::Painter,
    text_pos: egui::Pos2,
    font_id: &egui::FontId,
    color: egui::Color32,
    events: &mut DocEvents,
) {
    let is_editing = ctx.delay_inline.as_ref().map(|r| r.doc == doc_id && r.target == *target_id).unwrap_or(false);
    let current = ctx.delay_inline.as_ref().map(|r| r.text.clone()).unwrap_or_default();
    if is_editing {
        let mut buf = current;
        let mut commit = false;
        let mut cancelled = false;
        let id = egui::Id::new(("inline_delay", doc_id, target_id.0));
        let resp = ui.interact(edit_rect, id, egui::Sense::click());
        let response = ui.put(edit_rect, egui::TextEdit::singleline(&mut buf).hint_text("seconds"));
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) { commit = true; }
        let clicked_outside = ui.input(|i| i.pointer.any_pressed()) && !resp.clicked() && !response.hovered();
        if clicked_outside { cancelled = true; }
        if commit {
            events.delay_commit = Some(crate::editor::workspace::DelayInline { doc: doc_id, target: *target_id, text: buf.clone() });
        } else if cancelled {
            events.delay_cancel = Some((doc_id, *target_id));
        } else if response.changed() {
            events.delay_edit = Some(crate::editor::workspace::DelayInline { doc: doc_id, target: *target_id, text: buf });
        }
    } else {
        // Fallback draw (should not be called when not editing, caller draws read-only text)
        painter.text(text_pos, egui::Align2::CENTER_TOP, "", font_id.clone(), color);
    }
}

fn edge_kind_is_internal(v: &serde_json::Value) -> bool {
    // Accept common shapes: "Internal", {"Internal": {...}}, {"variant":"Internal"}
    if let Some(s) = v.as_str() { return s.eq_ignore_ascii_case("Internal"); }
    if let Some(obj) = v.as_object() {
        if obj.contains_key("Internal") { return true; }
        if let Some(variant) = obj.get("variant").and_then(|x| x.as_str()) { return variant.eq_ignore_ascii_case("Internal"); }
    }
    false
}



