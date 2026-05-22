use thiserror::Error;

/// All errors that can originate from the debugger backend or session layer.
///
/// # Example
/// ```
/// use runtime_core::error::DebuggerError;
/// let e = DebuggerError::ProcessNotFound;
/// assert!(e.to_string().contains("process not found"));
/// ```
#[derive(Debug, Error)]
pub enum DebuggerError {
    #[error("invalid state: current={current}, required={required}")]
    InvalidState { current: String, required: &'static str },
    #[error("debugger error: {0}")]
    DebuggerError(String),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_process_not_found_display() {
        assert!(DebuggerError::ProcessNotFound.to_string().contains("process not found"));
    }

    #[test]
    fn error_breakpoint_not_found_display() {
        assert!(DebuggerError::BreakpointNotFound(5).to_string().contains("5"));
    }

    #[test]
    fn error_invalid_state_display() {
        let e = DebuggerError::InvalidState {
            current: "Running".to_string(),
            required: "Paused",
        };
        let s = e.to_string();
        assert!(s.contains("Running"));
        assert!(s.contains("Paused"));
    }
}
