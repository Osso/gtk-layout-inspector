//! IPC debug server for runtime UI automation.
//!
//! Provides a Unix socket server that accepts commands from external tools
//! (like Claude Code) to inspect and interact with GTK4 applications.
//!
//! # Quick Start
//!
//! ```ignore
//! use gtk_layout_inspector::server::{self, Command, ScreenshotData};
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
//!             Command::Screenshot { respond } => {
//!                 // Use capture_with_retry for reliability (3 attempts, 50ms delay)
//!                 let result = ScreenshotData::capture_with_retry(&window, 3, 50);
//!                 let _ = respond.send(result);
//!             }
//!             // ... handle other commands
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
    /// Take a screenshot (returns base64-encoded JPEG).
    Screenshot,
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
    /// Screenshot as base64-encoded JPEG.
    Screenshot(String),
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
    /// Take screenshot - call respond with RGBA pixel data (width, height, pixels).
    Screenshot {
        respond: oneshot::Sender<Result<ScreenshotData, String>>,
    },
}

/// Raw screenshot data from the application.
/// The application is responsible for capturing the screenshot and providing RGBA pixel data.
#[derive(Debug)]
pub struct ScreenshotData {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // RGBA format
}

impl ScreenshotData {
    /// Capture a screenshot of a GTK widget.
    ///
    /// Uses `gtk::WidgetPaintable` to render the widget to a texture,
    /// then extracts the pixel data.
    ///
    /// # Example
    ///
    /// ```ignore
    /// Command::Screenshot { respond } => {
    ///     // Use capture_with_retry for reliability (3 attempts, 50ms delay)
    ///     let result = ScreenshotData::capture_with_retry(&window, 3, 50);
    ///     let _ = respond.send(result);
    /// }
    /// ```
    pub fn capture(widget: &impl gtk4::prelude::IsA<gtk4::Widget>) -> Result<Self, String> {
        use gtk4::gdk;
        use gtk4::prelude::*;

        // Check if widget is ready for rendering
        if !widget.is_realized() {
            return Err("Widget is not realized".to_string());
        }
        if !widget.is_mapped() {
            return Err("Widget is not mapped (not visible)".to_string());
        }

        // Get the widget's native (window/surface)
        let native = widget.native().ok_or("Widget has no native surface")?;

        // Get the widget's allocated size
        let width = widget.allocated_width();
        let height = widget.allocated_height();

        if width <= 0 || height <= 0 {
            return Err("Widget has no allocated size".to_string());
        }

        // Create a paintable from the widget
        let paintable = gtk4::WidgetPaintable::new(Some(widget.as_ref()));

        // Get an immutable snapshot of the current state - this ensures we have
        // a stable image even if the widget is animating or updating
        let current_image = paintable.current_image();

        // Create a snapshot and render the paintable
        let snapshot = gtk4::Snapshot::new();
        current_image.snapshot(
            snapshot.upcast_ref::<gdk::Snapshot>(),
            width as f64,
            height as f64,
        );

        // Get the render node - can be None if widget has no visible content
        let node = snapshot.to_node().ok_or_else(|| {
            "Failed to create render node: widget has no visible content to render".to_string()
        })?;

        // Get the renderer from the native
        let renderer = native.renderer().ok_or("Native has no renderer")?;

        // Render to a texture
        let texture = renderer.render_texture(&node, None);

        // Get texture dimensions
        let tex_width = texture.width() as u32;
        let tex_height = texture.height() as u32;

        // Save to a temporary PNG file and read back
        let tmp_path = std::env::temp_dir().join(format!("gtk-screenshot-{}.png", std::process::id()));
        texture
            .save_to_png(&tmp_path)
            .map_err(|e| format!("Failed to save PNG: {}", e))?;

        // Use the image crate to decode PNG and get RGBA pixels
        let img = image::open(&tmp_path)
            .map_err(|e| format!("Failed to decode PNG: {}", e))?;

        // Clean up temp file
        let _ = std::fs::remove_file(&tmp_path);

        let rgba = img.to_rgba8();
        let pixels = rgba.into_raw();

        Ok(ScreenshotData {
            width: tex_width,
            height: tex_height,
            pixels,
        })
    }

    /// Capture a screenshot with automatic retry on transient failures.
    ///
    /// Retries up to `max_attempts` times with a small delay between attempts.
    /// This handles cases where the widget temporarily has no visible content
    /// (e.g., during animations or window transitions).
    ///
    /// # Arguments
    /// * `widget` - The widget to capture
    /// * `max_attempts` - Maximum number of capture attempts (default: 3)
    /// * `delay_ms` - Delay between attempts in milliseconds (default: 50)
    pub fn capture_with_retry(
        widget: &impl gtk4::prelude::IsA<gtk4::Widget>,
        max_attempts: u32,
        delay_ms: u64,
    ) -> Result<Self, String> {
        let mut last_error = String::new();

        for attempt in 1..=max_attempts {
            match Self::capture(widget) {
                Ok(data) => return Ok(data),
                Err(e) => {
                    last_error = e;
                    // Only retry on "no visible content" errors, not on fundamental issues
                    if !last_error.contains("no visible content")
                        && !last_error.contains("not realized")
                        && !last_error.contains("not mapped")
                    {
                        return Err(last_error);
                    }
                    if attempt < max_attempts {
                        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                    }
                }
            }
        }

        Err(format!(
            "{} (after {} attempts)",
            last_error, max_attempts
        ))
    }
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

/// Clean up stale sockets from dead processes.
fn cleanup_stale_sockets() {
    let pattern = "/tmp/gtk-debug-*.sock";
    if let Ok(entries) = glob::glob(pattern) {
        for entry in entries.flatten() {
            // Extract PID from filename: /tmp/gtk-debug-{pid}.sock
            if let Some(filename) = entry.file_name().and_then(|f| f.to_str()) {
                if let Some(pid_str) = filename
                    .strip_prefix("gtk-debug-")
                    .and_then(|s| s.strip_suffix(".sock"))
                {
                    if let Ok(pid) = pid_str.parse::<i32>() {
                        // Check if process is still alive using kill(pid, 0)
                        // Returns 0 if process exists, -1 with ESRCH if not
                        let exists = unsafe { libc::kill(pid, 0) } == 0;
                        if !exists {
                            if std::fs::remove_file(&entry).is_ok() {
                                eprintln!(
                                    "[gtk-debug] Cleaned up stale socket: {}",
                                    entry.display()
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Encode screenshot data as JPEG and return base64 string.
fn encode_screenshot_jpeg(data: &ScreenshotData, quality: u8) -> Result<String, String> {
    use base64::Engine;
    use image::{ImageBuffer, Rgba};
    use std::io::Cursor;

    // Create image from RGBA pixels
    let img: ImageBuffer<Rgba<u8>, _> =
        ImageBuffer::from_raw(data.width, data.height, data.pixels.clone())
            .ok_or("Invalid image dimensions")?;

    // Convert to RGB (JPEG doesn't support alpha)
    let rgb_img = image::DynamicImage::ImageRgba8(img).to_rgb8();

    // Encode as JPEG with specified quality (1-100)
    let mut jpeg_data = Cursor::new(Vec::new());
    rgb_img
        .write_to(&mut jpeg_data, image::ImageFormat::Jpeg)
        .map_err(|e| format!("Failed to encode JPEG: {}", e))?;

    // Note: image crate uses default quality ~75. For quality control, use JpegEncoder directly
    let mut output = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, quality);
    encoder
        .encode(&rgb_img, rgb_img.width(), rgb_img.height(), image::ExtendedColorType::Rgb8)
        .map_err(|e| format!("Failed to encode JPEG: {}", e))?;

    // Base64 encode
    Ok(base64::engine::general_purpose::STANDARD.encode(&output))
}

/// Initialize the debug server.
///
/// Returns a tuple of (receiver, guard). Keep the guard alive for the socket to persist.
/// The socket is automatically removed when the guard is dropped.
pub fn init() -> (mpsc::Receiver<Command>, SocketGuard) {
    // Clean up any stale sockets from crashed processes
    cleanup_stale_sockets();

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
            Ok(Request::Screenshot) => {
                let (tx, rx) = oneshot::channel();
                if cmd_tx
                    .send(Command::Screenshot { respond: tx })
                    .await
                    .is_err()
                {
                    let _ = conn.write(&Response::Error("App closed".into())).await;
                    continue;
                }
                match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
                    Ok(Ok(Ok(data))) => {
                        // Encode as JPEG with quality 15 (matches iced-layout-inspector)
                        match encode_screenshot_jpeg(&data, 15) {
                            Ok(base64) => {
                                let _ = conn.write(&Response::Screenshot(base64)).await;
                            }
                            Err(e) => {
                                let _ = conn.write(&Response::Error(e)).await;
                            }
                        }
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

    /// Take a screenshot and return base64-encoded JPEG.
    pub fn screenshot<P: AsRef<Path>>(socket: P) -> Result<String, IpcError> {
        let resp: Response = Client::call(socket, &Request::Screenshot)?;
        match resp {
            Response::Screenshot(data) => Ok(data),
            Response::Error(e) => Err(IpcError::Io(std::io::Error::other(e))),
            _ => Err(IpcError::Io(std::io::Error::other("Unexpected response"))),
        }
    }

    /// Take a screenshot and save to a file.
    pub fn screenshot_to_file<P: AsRef<Path>, Q: AsRef<Path>>(
        socket: P,
        path: Q,
    ) -> Result<(), IpcError> {
        use base64::Engine;

        let base64_data = screenshot(socket)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&base64_data)
            .map_err(|e| IpcError::Io(std::io::Error::other(e.to_string())))?;
        std::fs::write(path, bytes).map_err(IpcError::Io)?;
        Ok(())
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
