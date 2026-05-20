// T007 — implemented below
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DebuggerError {
    #[error("invalid state: current={current}, required={required}")]
    InvalidState { current: String, required: &'static str },
    #[error("lldb error: {0}")]
    LLDBError(String),
    #[error("process not found")]
    ProcessNotFound,
    #[error("breakpoint not found: {0}")]
    BreakpointNotFound(u32),
    #[error("thread not found: {0}")]
    ThreadNotFound(u64),
    #[error("eval error in `{expression}`: {message}")]
    EvalError { expression: String, message: String },
    #[error("serialization error: {0}")]
    SerializationError(String),
    #[error("protocol error: {0}")]
    ProtocolError(String),
}
