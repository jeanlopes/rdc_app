//! Library interface for `mcp-server`.
//!
//! Exposes [`run_with_store`] so that `apps/desktop-ui` can embed the UI
//! inspection MCP server in-process without spawning a child process.

mod handlers;
mod ui_server;

pub use ui_server::run_with_store;
