//! Protocol crate — MCP tool input/output types and error code mapping.
//!
//! All types in [`tools`] are serialized to/from JSON-RPC 2.0 messages by the
//! `mcp-server` binary. The [`error`] module maps [`runtime_core::error::DebuggerError`]
//! variants to MCP error codes defined in `contracts/mcp-tools.md`.

pub mod error;
pub mod tools;
