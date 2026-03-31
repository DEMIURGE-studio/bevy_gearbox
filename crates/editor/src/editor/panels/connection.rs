use bevy::prelude::*;
use bevy_egui::egui;
use crate::editor::model::types::ConnectionState;
use crate::editor::actions::{ConnectRequested, DisconnectRequested, ReconnectRequested};
use crate::editor::model::store::EditorStore;

pub fn draw(ui: &mut egui::Ui, store: &mut EditorStore, commands: &mut Commands) {
    ui.horizontal(|ui| {
        let mut endpoint = match &store.connection {
            ConnectionState::Connected { endpoint, .. } => endpoint.clone(),
            _ => String::from("http://127.0.0.1:15703"),
        };
        ui.label("Endpoint");
        ui.text_edit_singleline(&mut endpoint);

        let button_text = match store.connection {
            ConnectionState::Connected { .. } => "Disconnect",
            ConnectionState::Connecting => "Connecting...",
            ConnectionState::Disconnected => "Connect",
        };

        if ui.button(button_text).clicked() {
            match store.connection {
                ConnectionState::Connected { .. } | ConnectionState::Connecting => {
                    commands.trigger(DisconnectRequested);
                }
                ConnectionState::Disconnected => {
                    commands.trigger(ConnectRequested { endpoint });
                }
            }
        }
        if matches!(store.connection, ConnectionState::Connected { .. }) {
            if ui.button("Reconnect").clicked() {
                commands.trigger(ReconnectRequested);
            }
        }
    });
}


