use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use crate::error::DebuggerError;
use crate::process::ProcessHandle;

/// Unique identifier for a debug session.
pub type SessionId = Uuid;

/// Description of the binary and launch parameters for a debug session.
///
/// # Example
/// ```
/// use runtime_core::session::DebugTarget;
/// use std::collections::HashMap;
/// let target = DebugTarget {
///     executable: "/usr/bin/my_app".into(),
///     args: vec!["--flag".to_string()],
///     env: HashMap::new(),
///     working_dir: None,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugTarget {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub working_dir: Option<PathBuf>,
}

/// Reason why the target process paused.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PauseReason {
    Breakpoint(u32),
    UserRequest,
    Step,
    Panic,
    Signal(String),
    Exception(String),
}

/// Lifecycle state of a debug session.
/// Use [`DebugSession::transition`] to move between states.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionState {
    Idle,
    Launching,
    Running,
    Paused(PauseReason),
    Stepping,
    Terminated(i32),
    Error(String),
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Launching => write!(f, "Launching"),
            Self::Running => write!(f, "Running"),
            Self::Paused(_) => write!(f, "Paused"),
            Self::Stepping => write!(f, "Stepping"),
            Self::Terminated(code) => write!(f, "Terminated({})", code),
            Self::Error(msg) => write!(f, "Error({})", msg),
        }
    }
}

/// Top-level container for a single debugging engagement.
///
/// # Example
/// ```
/// use runtime_core::session::{DebugSession, DebugTarget};
/// use std::collections::HashMap;
/// let target = DebugTarget {
///     executable: "/usr/bin/my_app".into(),
///     args: vec![],
///     env: HashMap::new(),
///     working_dir: None,
/// };
/// let session = DebugSession::new(target);
/// assert!(matches!(session.state, runtime_core::session::SessionState::Idle));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugSession {
    pub id: SessionId,
    pub target: DebugTarget,
    pub state: SessionState,
    pub created_at: u64,
    pub process: Option<ProcessHandle>,
}

impl DebugSession {
    pub fn new(target: DebugTarget) -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self {
            id: Uuid::new_v4(),
            target,
            state: SessionState::Idle,
            created_at,
            process: None,
        }
    }

    /// Transition to a new state. Returns Err if the transition is not permitted.
    pub fn transition(&mut self, new_state: SessionState) -> Result<(), DebuggerError> {
        let allowed = match (&self.state, &new_state) {
            (SessionState::Idle, SessionState::Launching) => true,
            (SessionState::Launching, SessionState::Running) => true,
            (SessionState::Launching, SessionState::Error(_)) => true,
            (SessionState::Running, SessionState::Paused(_)) => true,
            (SessionState::Running, SessionState::Terminated(_)) => true,
            (SessionState::Running, SessionState::Error(_)) => true,
            (SessionState::Paused(_), SessionState::Running) => true,
            (SessionState::Paused(_), SessionState::Stepping) => true,
            (SessionState::Paused(_), SessionState::Terminated(_)) => true,
            (SessionState::Paused(_), SessionState::Error(_)) => true,
            (SessionState::Stepping, SessionState::Paused(_)) => true,
            (SessionState::Stepping, SessionState::Error(_)) => true,
            _ => false,
        };
        if allowed {
            tracing::debug!(from = %self.state, to = %new_state, "session state transition");
            self.state = new_state;
            Ok(())
        } else {
            Err(DebuggerError::InvalidState {
                current: self.state.to_string(),
                required: "valid transition target",
            })
        }
    }
}

#[cfg(test)]
mod tests {}
