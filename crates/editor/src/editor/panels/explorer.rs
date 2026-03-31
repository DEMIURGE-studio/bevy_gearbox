use bevy::prelude::*;
use bevy_egui::egui;
use crate::editor::model::store::EditorStore;
use crate::editor::actions::{OpenRequested, CloseRequested};
use crate::editor::workspace::Workspace;
use crate::editor::docs::Docs;

pub fn draw(ui: &mut egui::Ui, store: &mut EditorStore, commands: &mut Commands, docs: &Docs) {
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut store.index.filter.query);
    });
    ui.separator();
    if store.index.is_loading { ui.label("Loading..."); return; }
    if let Some(err) = &store.index.error { ui.colored_label(egui::Color32::RED, err); }
    let mut to_open: Option<crate::types::EntityId> = None;
    let mut to_close: Option<crate::types::EntityId> = None;

    // Single unified list: open items (yellow, close on click) first, then closed items (open on click)
    let open_ids: std::collections::HashSet<crate::types::EntityId> = docs.map.keys().copied().collect();

    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
        let query = store.index.filter.query.trim().to_lowercase();
        let mut filtered: Vec<crate::editor::model::types::IndexItem> = store.index.items.iter().cloned().filter(|it| {
            if query.is_empty() { return true; }
            let name = it.name.as_deref().unwrap_or("");
            let id_text = it.entity.to_string();
            name.to_lowercase().contains(&query) || id_text.to_lowercase().contains(&query)
        }).collect();

        // Partition into open first, then closed
        let mut open_items: Vec<crate::editor::model::types::IndexItem> = Vec::new();
        let mut closed_items: Vec<crate::editor::model::types::IndexItem> = Vec::new();
        for it in filtered.drain(..) {
            if open_ids.contains(&it.entity) { open_items.push(it); } else { closed_items.push(it); }
        }

        // Render open items (yellow; click to close)
        for item in open_items.into_iter() {
            let title = match &item.name {
                Some(name) => format!("{}  ({})", name, item.entity),
                None => format!("{}", item.entity),
            };
            let button = egui::Button::new(egui::RichText::new(title).color(egui::Color32::BLACK))
                .fill(egui::Color32::from_rgb(230, 200, 40));
            if ui.add_sized([ui.available_width(), 24.0], button).clicked() { to_close = Some(item.entity); }
        }

        // Render closed items (default; click to open)
        for item in closed_items.into_iter() {
            let title = match &item.name {
                Some(name) => format!("{}  ({})", name, item.entity),
                None => format!("{}", item.entity),
            };
            if ui.add_sized([ui.available_width(), 24.0], egui::Button::new(title)).clicked() { to_open = Some(item.entity); }
        }
    });
    if let Some(entity) = to_open { commands.trigger(OpenRequested { entity }); }
    if let Some(entity) = to_close { commands.trigger(CloseRequested { entity }); }
}


