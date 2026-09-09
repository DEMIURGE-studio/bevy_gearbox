//! Wire protocol between a running game and the gearbox editor.
//!
//! The game side (`server` feature) is [`server::ServerPlugin`]: it installs
//! the Bevy Remote Protocol HTTP server and registers `editor.*` JSON-RPC
//! methods on top of the built-in `world.*` ones. `editor.machine_graph`
//! returns a snapshot of one machine (states, edges, and a component bag per
//! entity keyed by reflected type path, see [`components`]); `+watch` methods
//! stream discovery, per-machine transition events, and control commands;
//! node and edge RPCs mutate the chart; file RPCs save a subtree as a
//! `.scn.ron` scene and manage the editor's `.sm.ron` layout sidecars.
//!
//! The editor side (`client` feature, default) is [`client::ClientPlugin`]: a
//! Tokio-backed [`client::Client`] with one async method per RPC, watch tasks
//! that forward stream events into Bevy as [`client::NetMessage`]s, and
//! observers that turn the [`events`] a UI triggers into RPC calls.
//!
//! [`methods`] lists the method names; [`components`] the type-path keys used
//! in component bags.
/// Address the editor server binds and the editor client connects to by default.
pub const DEFAULT_ADDRESS: &str = "127.0.0.1:15703";
/// [`DEFAULT_ADDRESS`] as an HTTP URL.
pub const DEFAULT_URL: &str = "http://127.0.0.1:15703";

pub mod components;
pub mod events;
pub mod methods;

#[cfg(feature = "client")] pub mod client;
#[cfg(feature = "server")] pub mod server;
