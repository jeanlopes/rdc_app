use runtime_core::error::DebuggerError;

/// MCP JSON-RPC error codes per contracts/mcp-tools.md
pub const ERR_DEBUGGER: i32 = -32000;
pub const ERR_INVALID_STATE: i32 = -32001;
pub const ERR_LLDB: i32 = -32002;
pub const ERR_BREAKPOINT_NOT_FOUND: i32 = -32003;
pub const ERR_THREAD_NOT_FOUND: i32 = -32004;
pub const ERR_EVAL: i32 = -32005;

pub fn to_mcp_error_code(err: &DebuggerError) -> i32 {
    match err {
        DebuggerError::InvalidState { .. } => ERR_INVALID_STATE,
        DebuggerError::DebuggerError(_) => ERR_LLDB,
        DebuggerError::BreakpointNotFound(_) => ERR_BREAKPOINT_NOT_FOUND,
        DebuggerError::ThreadNotFound(_) => ERR_THREAD_NOT_FOUND,
        DebuggerError::EvalError { .. } => ERR_EVAL,
        _ => ERR_DEBUGGER,
    }
}

#[cfg(test)]
mod tests {}
