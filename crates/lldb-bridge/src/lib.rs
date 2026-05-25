//! LLDB bridge via Debug Adapter Protocol (DAP).
//!
//! Replaces win-debug-bridge by communicating with codelldb (or lldb-dap)
//! over stdin/stdout using the DAP JSON-RPC protocol.

#![warn(missing_docs)]

mod client;
mod handle;
mod logging;
mod transport;

pub use client::DapClient;
pub use handle::LldbDebugHandle;
pub use transport::DapTransport;
