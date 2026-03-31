use bevy::prelude::*;
use bevy_egui::EguiPlugin;

pub mod editor;
pub mod model;
pub mod persistence;
mod plugin;
pub mod types;
pub mod util;

use plugin::EditorPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(EguiPlugin::default())
        .add_plugins(EditorPlugin)
        .run();
}
