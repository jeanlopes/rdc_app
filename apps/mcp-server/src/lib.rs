//! Library interface for `mcp-server`.
//!
//! Exposes [`run_with_store`] so that `apps/desktop-ui` can embed the UI
//! inspection MCP server in-process without spawning a child process.

pub mod handlers;
pub mod server;
mod ui_server;

pub use ui_server::run_with_store;

use debug_session_view::DebugSessionView;
use lldb_bridge::LldbDebugHandle;
use std::path::PathBuf;

/// Run the debug MCP server in-process with a shared [`DebugSessionView`].
///
/// Spawns the same JSON-RPC dispatch loop as the standalone `mcp-server` binary,
/// but shares UI state so that AI-driven actions animate the visual debugger.
pub async fn run_with_view(
    handle: LldbDebugHandle,
    executable: PathBuf,
    args: Vec<String>,
    view: DebugSessionView,
) -> anyhow::Result<()> {
    server::run(handle, executable, args, "stdio", 0, Some(view)).await
}
