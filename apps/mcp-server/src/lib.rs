//! Library interface for `mcp-server`.
//!
//! Exposes [`run_with_store`] so that `apps/desktop-ui` can embed the UI
//! inspection MCP server in-process without spawning a child process.

pub mod handlers;
pub mod server;
mod ui_server;

pub use ui_server::run_with_store;

use std::sync::Arc;
use debug_session_view::DebugSessionView;
use runtime_core::backend::DebugBackend;
use std::path::PathBuf;

/// Run the debug MCP server in-process with a shared [`DebugSessionView`].
pub async fn run_with_view(
    backend: Arc<dyn DebugBackend>,
    executable: PathBuf,
    args: Vec<String>,
    view: DebugSessionView,
) -> anyhow::Result<()> {
    server::run(backend, executable, args, "stdio", 0, Some(view)).await
}
