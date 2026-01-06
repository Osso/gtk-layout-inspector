//! Layout debugging tools for GTK4 applications.
//!
//! This crate provides tools to inspect the widget tree and layout bounds
//! of a GTK4 UI, producing a text representation that can be analyzed
//! by external tools (like Claude Code).
//!
//! # Usage
//!
//! ```rust,ignore
//! use gtk_layout_inspector::{dump_widget_tree, LayoutDump};
//!
//! // Get the layout dump from your window
//! let dump = dump_widget_tree(&window);
//! println!("{}", dump);
//!
//! // Or write to file
//! std::fs::write("layout.txt", dump.to_string())?;
//! ```
//!
//! # Server Feature
//!
//! Enable the `server` feature for IPC-based runtime inspection:
//!
//! ```rust,ignore
//! use gtk_layout_inspector::server::{self, Command};
//!
//! // In your app startup:
//! let (cmd_rx, _guard) = server::init();
//!
//! // Poll for commands in your main loop:
//! glib::timeout_add_local(Duration::from_millis(50), move || {
//!     while let Ok(cmd) = cmd_rx.try_recv() {
//!         match cmd {
//!             Command::Dump { respond } => {
//!                 let dump = dump_widget_tree(&window);
//!                 let _ = respond.send(dump.to_string());
//!             }
//!             // ... handle other commands
//!         }
//!     }
//!     ControlFlow::Continue
//! });
//! ```

mod output;
mod traverse;

#[cfg(feature = "server")]
pub mod server;

pub use output::{LayoutDump, LayoutEntry, WidgetInfo};
pub use traverse::{dump_widget_tree, find_button_by_label, find_entry_by_placeholder};
