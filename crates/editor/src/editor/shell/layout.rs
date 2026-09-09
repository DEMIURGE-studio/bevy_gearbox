use bevy::prelude::Commands;
use bevy_egui::egui;
use crate::editor::panels;
use crate::editor::session::store::EditorStore;
use crate::editor::workspace::Workspace;
use crate::editor::open_docs::Docs;

pub fn draw(ui: &mut egui::Ui, store: &mut EditorStore, commands: &mut Commands, workspace: &mut Workspace, docs: &mut Docs) {
    draw_save_as_prompt(ui.ctx(), commands, workspace);
    ui.vertical(|ui| {
        // Top bar: connection controls across entire app
        ui.horizontal(|ui| {
            panels::connection::draw(ui, store, commands);
        });
        ui.add_space(4.0);

        ui.separator();

        // Below: left panel + canvas, sized to fill remaining height
        let available_size = egui::vec2(ui.available_width(), ui.available_height());
        let _resp = ui.allocate_ui_with_layout(available_size, egui::Layout::left_to_right(egui::Align::Min), |ui| {
            // Left panel: fixed width; search + remote state machines
            let left_width = 260.0;
            let desired_left = egui::vec2(left_width, ui.available_height());
            let _resp = ui.allocate_ui_with_layout(desired_left, egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(left_width);
                ui.heading("Explorer");
                panels::explorer::draw(ui, store, commands, docs);
            });

            ui.separator();

            // Canvas: single shared board where all open documents are drawn together
            let desired_canvas = ui.available_size_before_wrap();
            let _resp = ui.allocate_ui_with_layout(desired_canvas, egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_min_size(desired_canvas);
                let board_size = ui.available_size_before_wrap();
                let (board_rect, board_resp) = ui.allocate_exact_size(board_size, egui::Sense::click_and_drag());
                let board_painter = ui.painter_at(board_rect);
                board_painter.rect_filled(board_rect, 0.0, egui::Color32::from_gray(20));

                // Background context menu (workspace)
                board_resp.context_menu(|menu_ui| {
                    menu_ui.set_min_width(180.0);
                    if menu_ui.button("New State Machine").clicked() {
                        // Spawn a new state machine with an initial Name; server/watch will update the index
                        commands.trigger(bevy_gearbox_protocol::events::SpawnStateMachine { name: Some("New State Machine".to_string()) });
                        menu_ui.close();
                    }
                });

                // Board-level zoom: apply wheel zoom to all documents once per frame and persist to workspace.board_transform
                let scroll_y = ui.ctx().input(|i| i.smooth_scroll_delta.y);
                if scroll_y != 0.0 && !ui.ctx().egui_wants_pointer_input() {
                    let scroll: f32 = scroll_y;
                    if scroll.abs() > 0.0 {
                        let factor = 1.0 + (-scroll * 0.001);
                        let cursor = ui.ctx().input(|i| i.pointer.hover_pos()).unwrap_or(board_rect.center());
                        let cursor = cursor.clamp(board_rect.min, board_rect.max);
                        for (_id, doc) in docs.map.iter_mut() {
                            doc.transform.zoom_around_screen_point(factor, cursor);
                        }
                        workspace.board_transform.zoom_around_screen_point(factor, cursor);
                    }
                }

                // Board-level background drag pan: apply once per frame to all documents
                let delta_screen = board_resp.drag_delta();
                if board_resp.dragged() && workspace.board_drag_doc.is_none() {
                    if delta_screen.length_sq() > 0.0 && ui.ctx().input(|i| i.pointer.primary_down()) {
                        for (_id, doc) in docs.map.iter_mut() {
                            doc.transform.pan_screen_delta(delta_screen);
                        }
                        workspace.board_transform.pan_screen_delta(delta_screen);
                    }
                }

                // Board-level autopan while dragging near viewport edges
                if let Some(owner_doc) = workspace.board_drag_doc {
                    if let Some(cursor) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                        if let Some(d) = docs.map.get(&owner_doc) {
                            if d.dragging.is_some() {
                                let pan = crate::editor::layout::NodeLayout::autopan_suggestion(board_rect, cursor, 24.0, 10.0);
                                if pan != egui::Vec2::ZERO {
                                    for (_id, doc) in docs.map.iter_mut() { doc.transform.pan_screen_delta(pan); }
                                    workspace.board_transform.pan_screen_delta(pan);
                                }
                            }
                        }
                    }
                }

                if !docs.map.is_empty() {
                    let mut ids: Vec<crate::types::EntityId> = docs.map.keys().copied().collect();
                    ids.sort_by_key(|e| e.0);
                    for doc_id in ids {
                        // Local view of global selection for this doc
                        let prev_global = workspace.global_selection;
                        let mut sel_local = prev_global.and_then(|(d, t)| if d == doc_id { Some(t) } else { None });
                        // Build read-only view context for this doc
                        let ctx = crate::editor::view::ViewBoardCtx {
                            edge_build: workspace.edge_build.clone(),
                            edge_menu: workspace.edge_menu.clone(),
                            available_message_edges: workspace.available_message_edges.clone(),
                            preview_edges: workspace.preview_edges.clone(),
                            rename_inline: workspace.rename_inline.clone(),
                            delay_inline: workspace.delay_inline.clone(),
                            board_drag_owner: workspace.board_drag_doc,
                        };
                        // Draw using a scoped mutable borrow to this document
                        let ev = {
                            let entry = docs.map.get_mut(&doc_id).expect("document must exist");
                            crate::editor::view::draw_doc_on_board(ui, board_rect, &board_resp, entry, &mut sel_local, doc_id, &ctx, false)
                        };
                        if let Some(selection) = ev.context_menu_selection {
                            match selection {
                                crate::editor::context_menu::MenuSelection::SaveStateMachine { target } => {
                                    let existing = docs.map.get(&doc_id).and_then(|d| d.graph.as_ref()).and_then(|g| g.machine_id(&target));
                                    match existing {
                                        Some(id) => commands.trigger(crate::editor::actions::SaveRequested { doc: doc_id, target, id }),
                                        None => workspace.save_as_prompt = Some(crate::editor::workspace::SaveAsPrompt { doc: doc_id, target, id: "statemachine".to_string(), just_opened: true }),
                                    }
                                }
                                crate::editor::context_menu::MenuSelection::SaveStateMachineAs { target } => {
                                    let existing = docs.map.get(&doc_id).and_then(|d| d.graph.as_ref()).and_then(|g| g.machine_id(&target));
                                    workspace.save_as_prompt = Some(crate::editor::workspace::SaveAsPrompt {
                                        doc: doc_id,
                                        target,
                                        id: existing.unwrap_or_else(|| "statemachine".to_string()),
                                        just_opened: true,
                                    });
                                }
                                crate::editor::context_menu::MenuSelection::SaveSubstates { target } => {
                                    commands.trigger(crate::editor::actions::SaveSubstatesRequested { target });
                                }
                                crate::editor::context_menu::MenuSelection::RenameEntity { target } => {
                                    let mut default_text = String::new();
                                    if let Some(doc_ref) = docs.map.get(&doc_id) {
                                        if let Some(g) = &doc_ref.graph { default_text = g.get_display_name(&target); }
                                        if default_text.is_empty() {
                                            if let Some(v) = doc_ref.scene.states.get(&target) { default_text = v.label.clone(); }
                                            else if let Some(v) = doc_ref.scene.edges.get(&target) { default_text = v.label.clone(); }
                                        }
                                    }
                                    workspace.rename_inline = Some(crate::editor::workspace::RenameInline { doc: doc_id, target, text: default_text });
                                }
                                // No direct menu selection for delay (handled in view context menu)
                                crate::editor::context_menu::MenuSelection::MakeLeaf { target } => {
                                    let e = bevy::prelude::Entity::from_bits(target.0);
                                    commands.trigger(bevy_gearbox_protocol::events::ChangeNodeType { target: e, to: bevy_gearbox_protocol::events::NodeType::Leaf });
                                    workspace.pending_fetch_docs.push(doc_id);
                                }
                                crate::editor::context_menu::MenuSelection::MakeParent { target } => {
                                    let e = bevy::prelude::Entity::from_bits(target.0);
                                    commands.trigger(bevy_gearbox_protocol::events::ChangeNodeType { target: e, to: bevy_gearbox_protocol::events::NodeType::Parent });
                                    workspace.pending_fetch_docs.push(doc_id);
                                }
                                crate::editor::context_menu::MenuSelection::MakeParallel { target } => {
                                    let e = bevy::prelude::Entity::from_bits(target.0);
                                    commands.trigger(bevy_gearbox_protocol::events::ChangeNodeType { target: e, to: bevy_gearbox_protocol::events::NodeType::Parallel });
                                    workspace.pending_fetch_docs.push(doc_id);
                                }
                                crate::editor::context_menu::MenuSelection::MakeInitial { parent, new_initial } => {
                                    commands.trigger(bevy_gearbox_protocol::events::SetInitialState {
                                        parent: bevy::prelude::Entity::from_bits(parent.0),
                                        child: bevy::prelude::Entity::from_bits(new_initial.0),
                                    });
                                    workspace.pending_fetch_docs.push(doc_id);
                                }
                                crate::editor::context_menu::MenuSelection::AddChildStateMachine { target } => {
                                    let e = bevy::prelude::Entity::from_bits(target.0);
                                    commands.trigger(bevy_gearbox_protocol::events::SpawnSubstate { parent: e, name: Some("New State".to_string()) });
                                    workspace.pending_fetch_docs.push(doc_id);
                                }
                                crate::editor::context_menu::MenuSelection::DeleteEntity { target } => {
                                    let e = bevy::prelude::Entity::from_bits(target.0);
                                    commands.trigger(bevy_gearbox_protocol::events::Despawn { target: e });
                                    workspace.pending_fetch_docs.push(doc_id);
                                }
                                crate::editor::context_menu::MenuSelection::AutoLayoutSubtree { target } => {
                                    if let Some(entry) = docs.map.get_mut(&doc_id) {
                                        if let Some(graph) = entry.graph.clone() {
                                            let cfg = crate::editor::auto_layout::AutoLayoutConfig::default();
                                            let layout_cfg = crate::editor::layout::LayoutConfig::default();
                                            let _report = crate::editor::auto_layout::auto_layout_subtree(
                                                &mut entry.scene,
                                                &graph,
                                                target,
                                                &cfg,
                                                &layout_cfg,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        // Apply drag ownership changes from events
                        if ev.claim_drag { workspace.board_drag_doc = Some(doc_id); }
                        if ev.drag_stopped && workspace.board_drag_doc == Some(doc_id) { workspace.board_drag_doc = None; }
                        // Apply edge build/menu/create events
                        if let Some(s) = ev.edge_build_set { workspace.edge_build = Some(s); }
                        if ev.edge_build_clear { workspace.edge_build = None; }
                        if let Some(m) = ev.edge_menu_open { workspace.edge_menu = Some(m); }
                        if ev.edge_menu_close { workspace.edge_menu = None; }
                        if let Some(req) = ev.pending_edge_create { workspace.pending_edge_create = Some(req); }
                        if let Some((doc, edge)) = ev.clear_edge_delay { commands.trigger(crate::editor::actions::ClearEdgeDelayRequested { target: edge }); workspace.pending_fetch_docs.push(doc); }
                        if let Some((doc, edge, internal)) = ev.set_edge_kind { commands.trigger(crate::editor::actions::SetEdgeKindRequested { target: edge, internal }); workspace.pending_fetch_docs.push(doc); }
                        if let Some(pe) = ev.preview_edge_remove { workspace.preview_edges.retain(|x| !(x.doc == pe.doc && x.source == pe.source && x.target == pe.target)); }
                        // Apply rename inline events
                        if let Some(edit) = ev.rename_edit { workspace.rename_inline = Some(edit); }
                        if let Some(commit) = ev.rename_commit { workspace.pending_rename_commit = Some(commit); workspace.rename_inline = None; }
                        if let Some((d, t)) = ev.rename_cancel { if workspace.rename_inline.as_ref().map(|r| r.doc == d && r.target == t).unwrap_or(false) { workspace.rename_inline = None; } }
                        // Apply delay inline events
                        if let Some(start) = ev.delay_start { workspace.delay_inline = Some(start); }
                        if let Some(edit) = ev.delay_edit { workspace.delay_inline = Some(edit); }
                        if let Some(commit) = ev.delay_commit { workspace.pending_delay_commit = Some(commit); }
                        if let Some((d, t)) = ev.delay_cancel { if workspace.delay_inline.as_ref().map(|r| r.doc == d && r.target == t).unwrap_or(false) { workspace.delay_inline = None; } }
                        // Reconcile global selection based on this doc's local selection delta
                        match (prev_global, sel_local) {
                            (_, Some(t)) => workspace.global_selection = Some((doc_id, t)),
                            (Some((d, _)), None) if d == doc_id => workspace.global_selection = None,
                            _ => {}
                        }
                    }

                    // Apply global commits after per-doc draws
                    if let Some(commit) = workspace.pending_rename_commit.take() {
                        let e = bevy::prelude::Entity::from_bits(commit.target.0);
                        commands.trigger(bevy_gearbox_protocol::events::Rename { target: e, name: commit.text.clone() });
                    }
                    if let Some(commit) = workspace.pending_delay_commit.take() {
                        if let Ok(secs) = commit.text.trim().parse::<f32>() {
                            commands.trigger(crate::editor::actions::SetEdgeDelayRequested { target: commit.target, seconds: secs });
                            workspace.delay_inline = None;
                        } else {
                            // Invalid input: do nothing, keep inline open
                            workspace.delay_inline = Some(commit);
                        }
                    }
                    if let Some(req) = workspace.pending_edge_create.take() {
                        let m = bevy::prelude::Entity::from_bits(req.doc.0);
                        let s = bevy::prelude::Entity::from_bits(req.source.0);
                        let t = bevy::prelude::Entity::from_bits(req.target.0);
                        commands.trigger(bevy_gearbox_protocol::events::CreateTransition { machine: m, source: s, target: t, kind: req.kind.clone() });
                        workspace.pending_fetch_docs.push(req.doc);
                    }
                }
            });
        });
    });
}

/// The "Save As" prompt: one text field for the `StateMachineId`, showing the
/// files the game will write. Enter or "Save" commits; Escape or "Cancel" closes.
fn draw_save_as_prompt(ctx: &egui::Context, commands: &mut Commands, workspace: &mut Workspace) {
    let Some(mut prompt) = workspace.save_as_prompt.clone() else { return };
    let mut commit = false;
    let mut cancel = false;
    egui::Window::new("Save state machine")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label("Machine id. Use / for a folder under the game's assets/, e.g. enemies/goblin:");
            let resp = ui.add(egui::TextEdit::singleline(&mut prompt.id).desired_width(280.0));
            if prompt.just_opened {
                resp.request_focus();
                prompt.just_opened = false;
            }
            let id = crate::editor::actions::normalize_machine_id(&prompt.id);
            ui.monospace(format!("assets/{id}.scn.ron"));
            ui.monospace(format!("assets/{id}.sm.ron"));
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() { commit = true; }
                if ui.button("Cancel").clicked() { cancel = true; }
            });
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { commit = true; }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) { cancel = true; }
        });
    if commit {
        commands.trigger(crate::editor::actions::SaveRequested { doc: prompt.doc, target: prompt.target, id: prompt.id.clone() });
        workspace.pending_fetch_docs.push(prompt.doc);
        workspace.save_as_prompt = None;
    } else if cancel {
        workspace.save_as_prompt = None;
    } else {
        workspace.save_as_prompt = Some(prompt);
    }
}
