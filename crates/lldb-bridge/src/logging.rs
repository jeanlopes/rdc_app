//! Synchronous DAP traffic logger.
//!
//! Writes every DAP message sent and received to a file so we can debug
//! protocol issues without relying on tracing (which may drop messages if
//! tasks panic).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

/// Direction of the logged message.
#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Sent,
    Received,
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Direction::Sent => write!(f, "-->"),
            Direction::Received => write!(f, "<--"),
        }
    }
}

pub struct TrafficLogger {
    file: Mutex<File>,
}

impl TrafficLogger {
    /// Open (or create) the log file at the given path.
    pub fn new(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .write(true)
            .open(path)?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }

    /// Log a single DAP message.
    pub fn log(&self, dir: Direction, msg: &serde_json::Value) {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        let line = format!("[{:.3}] {} {}\n", ts, dir, msg);
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(line.as_bytes());
            let _ = f.flush();
        }
    }
}

/// Global logger instance used by the transport layer.
///
/// The log file is written to `target/dap-traffic.log` relative to the
/// workspace root (detected via `CARGO_MANIFEST_DIR` of the lldb-bridge crate).
pub fn global_logger() -> Option<&'static TrafficLogger> {
    use std::sync::OnceLock;
    static LOGGER: OnceLock<TrafficLogger> = OnceLock::new();
    Some(LOGGER.get_or_init(|| {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default());
        let workspace_root = manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or(manifest_dir);
        let path = workspace_root.join("target/dap-traffic.log");
        TrafficLogger::new(&path).unwrap_or_else(|e| {
            eprintln!("[lldb-bridge] WARNING: failed to create DAP traffic log at {}: {}", path.display(), e);
            // Fallback to a temp file so the logger still works
            let tmp = std::env::temp_dir().join("lldb-bridge-dap-traffic.log");
            TrafficLogger::new(&tmp).expect("failed to create fallback DAP traffic log")
        })
    }))
}
