//! IPC debug server for runtime UI automation.
//!
//! Provides a Unix socket server that accepts commands from external tools
//! (like Claude Code) to inspect and interact with GTK4 applications.
//!
//! # Quick Start
//!
//! ```ignore
//! use gtk_layout_inspector::server::{self, Command};
//! use gtk_layout_inspector::dump_widget_tree;
//! use std::time::Duration;
//! use glib::ControlFlow;
//!
//! // In your app startup, after window is created:
//! let (mut cmd_rx, _guard) = server::init();
//!
//! // Store window reference for the closure
//! let window_weak = window.downgrade();
//!
//! // Poll for commands in GTK main loop:
//! glib::timeout_add_local(Duration::from_millis(50), move || {
//!     while let Ok(cmd) = cmd_rx.try_recv() {
//!         let Some(window) = window_weak.upgrade() else {
//!             continue;
//!         };
//!         match cmd {
//!             Command::Dump { respond } => {
//!                 let dump = dump_widget_tree(&window);
//!                 let _ = respond.send(dump.to_string());
//!             }
//!             Command::Click { label, respond } => {
//!                 // Find and click button
//!             }
//!             Command::Input { field, value, respond } => {
//!                 // Find entry and set text
//!             }
//!             Command::Submit { respond } => {
//!                 // Activate focused widget
//!             }
//!         }
//!     }
//!     ControlFlow::Continue
//! });
//! ```

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// Request types sent over IPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Dump the current layout tree.
    Dump,
    /// Set text in an entry field (identified by placeholder).
    Input { field: String, value: String },
    /// Click a button (identified by label text).
    Click { label: String },
    /// Activate the currently focused widget (like pressing Enter).
    Submit,
    /// Ping to check if server is alive.
    Ping,
    /// Get layout as JSON instead of text.
    DumpJson,
    /// Send a key press event.
    KeyPress { key: String },
}

/// Response types sent over IPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    /// Layout dump as text.
    Layout(String),
    /// Operation succeeded.
    Ok,
    /// Pong response.
    Pong,
    /// Error message.
    Error(String),
}

/// Commands sent to the app from the debug server.
#[derive(Debug)]
pub enum Command {
    /// Dump layout - call respond with the layout string when ready.
    Dump { respond: oneshot::Sender<String> },
    /// Dump layout as JSON.
    DumpJson { respond: oneshot::Sender<String> },
    /// Set text input value.
    Input {
        field: String,
        value: String,
        respond: oneshot::Sender<Result<(), String>>,
    },
    /// Click a button.
    Click {
        label: String,
        respond: oneshot::Sender<Result<(), String>>,
    },
    /// Submit/activate (press Enter).
    Submit {
        respond: oneshot::Sender<Result<(), String>>,
    },
    /// Send a key press.
    KeyPress {
        key: String,
        respond: oneshot::Sender<Result<(), String>>,
    },
}

/// Get the socket path for the current process.
pub fn socket_path() -> PathBuf {
    PathBuf::from(format!("/tmp/gtk-debug-{}.sock", std::process::id()))
}

/// Guard that removes the socket file when dropped.
pub struct SocketGuard {
    path: PathBuf,
    shutdown: Arc<AtomicBool>,
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = std::fs::remove_file(&self.path);
        eprintln!("[gtk-debug] Cleaned up {}", self.path.display());
    }
}

/// Initialize the debug server.
///
/// Returns a tuple of (receiver, guard). Keep the guard alive for the socket to persist.
/// The socket is automatically removed when the guard is dropped.
pub fn init() -> (mpsc::Receiver<Command>, SocketGuard) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(16);
    let path = socket_path();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(run_server(cmd_tx, shutdown_clone));
    });

    let guard = SocketGuard { path, shutdown };
    (cmd_rx, guard)
}

/// Re-export the receiver type for convenience.
pub type CommandReceiver = mpsc::Receiver<Command>;

async fn run_server(cmd_tx: mpsc::Sender<Command>, shutdown: Arc<AtomicBool>) {
    use peercred_ipc::Server;

    let path = socket_path();

    let server = match Server::bind(&path) {
        Ok(s) => {
            eprintln!("[gtk-debug] Listening on {}", path.display());
            s
        }
        Err(e) => {
            eprintln!("[gtk-debug] Failed to bind: {}", e);
            return;
        }
    };

    loop {
        if shutdown.load(Ordering::SeqCst) {
            eprintln!("[gtk-debug] Server shutting down");
            break;
        }

        // Use timeout to periodically check shutdown flag
        let accept_result =
            tokio::time::timeout(std::time::Duration::from_millis(100), server.accept()).await;

        let (mut conn, _caller) = match accept_result {
            Ok(Ok((conn, caller))) => (conn, caller),
            Ok(Err(e)) => {
                eprintln!("[gtk-debug] Accept error: {}", e);
                continue;
            }
            Err(_) => continue, // Timeout, check shutdown flag
        };

        let request: Result<Request, _> = conn.read().await;
        match request {
            Ok(Request::Dump) => {
                let (tx, rx) = oneshot::channel();
                if cmd_tx.send(Command::Dump { respond: tx }).await.is_err() {
                    let _ = conn.write(&Response::Error("App closed".into())).await;
                    continue;
                }
                match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
                    Ok(Ok(layout)) => {
                        let _ = conn.write(&Response::Layout(layout)).await;
                    }
                    _ => {
                        let _ = conn.write(&Response::Error("Timeout".into())).await;
                    }
                }
            }
            Ok(Request::DumpJson) => {
                let (tx, rx) = oneshot::channel();
                if cmd_tx.send(Command::DumpJson { respond: tx }).await.is_err() {
                    let _ = conn.write(&Response::Error("App closed".into())).await;
                    continue;
                }
                match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
                    Ok(Ok(layout)) => {
                        let _ = conn.write(&Response::Layout(layout)).await;
                    }
                    _ => {
                        let _ = conn.write(&Response::Error("Timeout".into())).await;
                    }
                }
            }
            Ok(Request::Input { field, value }) => {
                let (tx, rx) = oneshot::channel();
                if cmd_tx
                    .send(Command::Input {
                        field,
                        value,
                        respond: tx,
                    })
                    .await
                    .is_err()
                {
                    let _ = conn.write(&Response::Error("App closed".into())).await;
                    continue;
                }
                match tokio::time::timeout(std::time::Duration::from_secs(2), rx).await {
                    Ok(Ok(Ok(()))) => {
                        let _ = conn.write(&Response::Ok).await;
                    }
                    Ok(Ok(Err(e))) => {
                        let _ = conn.write(&Response::Error(e)).await;
                    }
                    _ => {
                        let _ = conn.write(&Response::Error("Timeout".into())).await;
                    }
                }
            }
            Ok(Request::Click { label }) => {
                let (tx, rx) = oneshot::channel();
                if cmd_tx
                    .send(Command::Click { label, respond: tx })
                    .await
                    .is_err()
                {
                    let _ = conn.write(&Response::Error("App closed".into())).await;
                    continue;
                }
                match tokio::time::timeout(std::time::Duration::from_secs(2), rx).await {
                    Ok(Ok(Ok(()))) => {
                        let _ = conn.write(&Response::Ok).await;
                    }
                    Ok(Ok(Err(e))) => {
                        let _ = conn.write(&Response::Error(e)).await;
                    }
                    _ => {
                        let _ = conn.write(&Response::Error("Timeout".into())).await;
                    }
                }
            }
            Ok(Request::Submit) => {
                let (tx, rx) = oneshot::channel();
                if cmd_tx.send(Command::Submit { respond: tx }).await.is_err() {
                    let _ = conn.write(&Response::Error("App closed".into())).await;
                    continue;
                }
                match tokio::time::timeout(std::time::Duration::from_secs(2), rx).await {
                    Ok(Ok(Ok(()))) => {
                        let _ = conn.write(&Response::Ok).await;
                    }
                    Ok(Ok(Err(e))) => {
                        let _ = conn.write(&Response::Error(e)).await;
                    }
                    _ => {
                        let _ = conn.write(&Response::Error("Timeout".into())).await;
                    }
                }
            }
            Ok(Request::Ping) => {
                let _ = conn.write(&Response::Pong).await;
            }
            Ok(Request::KeyPress { key }) => {
                let (tx, rx) = oneshot::channel();
                if cmd_tx
                    .send(Command::KeyPress { key, respond: tx })
                    .await
                    .is_err()
                {
                    let _ = conn.write(&Response::Error("App closed".into())).await;
                    continue;
                }
                match tokio::time::timeout(std::time::Duration::from_secs(2), rx).await {
                    Ok(Ok(Ok(()))) => {
                        let _ = conn.write(&Response::Ok).await;
                    }
                    Ok(Ok(Err(e))) => {
                        let _ = conn.write(&Response::Error(e)).await;
                    }
                    _ => {
                        let _ = conn.write(&Response::Error("Timeout".into())).await;
                    }
                }
            }
            Err(e) => {
                eprintln!("[gtk-debug] Read error: {}", e);
            }
        }
    }
}

/// Client functions for sending commands to a GTK app.
pub mod client {
    use super::*;
    use peercred_ipc::{Client, IpcError};
    use std::path::Path;

    /// Dump the current layout as text.
    pub fn dump<P: AsRef<Path>>(socket: P) -> Result<String, IpcError> {
        let resp: Response = Client::call(socket, &Request::Dump)?;
        match resp {
            Response::Layout(s) => Ok(s),
            Response::Error(e) => Err(IpcError::Io(std::io::Error::other(e))),
            _ => Err(IpcError::Io(std::io::Error::other("Unexpected response"))),
        }
    }

    /// Dump the current layout as JSON.
    pub fn dump_json<P: AsRef<Path>>(socket: P) -> Result<String, IpcError> {
        let resp: Response = Client::call(socket, &Request::DumpJson)?;
        match resp {
            Response::Layout(s) => Ok(s),
            Response::Error(e) => Err(IpcError::Io(std::io::Error::other(e))),
            _ => Err(IpcError::Io(std::io::Error::other("Unexpected response"))),
        }
    }

    /// Type text into a field identified by placeholder.
    pub fn input<P: AsRef<Path>>(socket: P, field: &str, value: &str) -> Result<(), IpcError> {
        let resp: Response = Client::call(
            socket,
            &Request::Input {
                field: field.to_string(),
                value: value.to_string(),
            },
        )?;
        match resp {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(IpcError::Io(std::io::Error::other(e))),
            _ => Err(IpcError::Io(std::io::Error::other("Unexpected response"))),
        }
    }

    /// Click a button by label.
    pub fn click<P: AsRef<Path>>(socket: P, label: &str) -> Result<(), IpcError> {
        let resp: Response = Client::call(
            socket,
            &Request::Click {
                label: label.to_string(),
            },
        )?;
        match resp {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(IpcError::Io(std::io::Error::other(e))),
            _ => Err(IpcError::Io(std::io::Error::other("Unexpected response"))),
        }
    }

    /// Submit the current form (press Enter).
    pub fn submit<P: AsRef<Path>>(socket: P) -> Result<(), IpcError> {
        let resp: Response = Client::call(socket, &Request::Submit)?;
        match resp {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(IpcError::Io(std::io::Error::other(e))),
            _ => Err(IpcError::Io(std::io::Error::other("Unexpected response"))),
        }
    }

    /// Ping the app to check if it's running.
    pub fn ping<P: AsRef<Path>>(socket: P) -> Result<(), IpcError> {
        let resp: Response = Client::call(socket, &Request::Ping)?;
        match resp {
            Response::Pong => Ok(()),
            _ => Err(IpcError::Io(std::io::Error::other("Unexpected response"))),
        }
    }

    /// Send a key press event.
    pub fn key_press<P: AsRef<Path>>(socket: P, key: &str) -> Result<(), IpcError> {
        let resp: Response = Client::call(
            socket,
            &Request::KeyPress {
                key: key.to_string(),
            },
        )?;
        match resp {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(IpcError::Io(std::io::Error::other(e))),
            _ => Err(IpcError::Io(std::io::Error::other("Unexpected response"))),
        }
    }

    /// Find running GTK debug servers, cleaning up stale sockets.
    pub fn find_servers() -> Vec<PathBuf> {
        glob::glob("/tmp/gtk-debug-*.sock")
            .map(|paths| {
                paths
                    .filter_map(Result::ok)
                    .filter(|path| {
                        // Extract PID and check if process is still running
                        if let Some(pid) = extract_pid(path) {
                            if is_process_running(pid) {
                                return true;
                            }
                            // Process is dead, remove stale socket
                            let _ = std::fs::remove_file(path);
                        }
                        false
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn extract_pid(path: &Path) -> Option<u32> {
        path.file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix("gtk-debug-"))
            .and_then(|s| s.strip_suffix(".sock"))
            .and_then(|s| s.parse().ok())
    }

    fn is_process_running(pid: u32) -> bool {
        Path::new(&format!("/proc/{}", pid)).exists()
    }
}
