//! The gearbox editor: a standalone Bevy + egui app that connects to a running
//! game over `bevy_gearbox_protocol` and shows its state machines on a canvas.
//!
//! Run the game with `ServerPlugin` (the examples do), then:
//!
//! ```sh
//! cargo run -p bevy_gearbox_editor
//! ```
//!
//! Connect to the default endpoint (`127.0.0.1:15703`), open a machine from the
//! explorer, and watch transitions light up. Right-click nodes to rename,
//! re-parent, set the initial state, or add transitions; drag to lay out; use
//! "Save As" to write the chart as a `.scn.ron` scene with a `.sm.ron` layout
//! sidecar next to it. Layout is done on demand with a Sugiyama pass
//! (`editor::auto_layout`).
//!
//! Data flow: protocol JSON -> `model::StateMachineGraph` -> `editor::adapter`
//! projects it into a `ViewScene` -> `editor::view` draws it and returns
//! `DocEvents` -> `editor::shell` turns those into protocol events.
use bevy::prelude::*;
use bevy_egui::EguiPlugin;

pub mod editor;
pub mod model;
pub mod persistence;
mod plugin;
pub mod types;

use plugin::EditorPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(EguiPlugin::default())
        .add_plugins(EditorPlugin)
        .run();
}
