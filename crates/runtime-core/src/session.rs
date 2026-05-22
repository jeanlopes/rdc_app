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
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::error::DebuggerError;

    fn make_target() -> DebugTarget {
        DebugTarget {
            executable: "/bin/test".into(),
            args: vec![],
            env: HashMap::new(),
            working_dir: None,
        }
    }

    #[test]
    fn session_new_starts_idle() {
        let session = DebugSession::new(make_target());
        assert_eq!(session.state, SessionState::Idle);
    }

    #[test]
    fn transition_idle_to_launching() {
        let mut session = DebugSession::new(make_target());
        assert!(session.transition(SessionState::Launching).is_ok());
    }

    #[test]
    fn transition_idle_to_running_fails() {
        let mut session = DebugSession::new(make_target());
        assert!(matches!(session.transition(SessionState::Running), Err(DebuggerError::InvalidState { .. })));
    }

    #[test]
    fn transition_running_to_paused() {
        let mut session = DebugSession::new(make_target());
        session.transition(SessionState::Launching).unwrap();
        session.transition(SessionState::Running).unwrap();
        assert!(session.transition(SessionState::Paused(PauseReason::UserRequest)).is_ok());
    }

    #[test]
    fn transition_paused_to_stepping() {
        let mut session = DebugSession::new(make_target());
        session.transition(SessionState::Launching).unwrap();
        session.transition(SessionState::Running).unwrap();
        session.transition(SessionState::Paused(PauseReason::UserRequest)).unwrap();
        assert!(session.transition(SessionState::Stepping).is_ok());
    }

    #[test]
    fn transition_stepping_to_paused() {
        let mut session = DebugSession::new(make_target());
        session.transition(SessionState::Launching).unwrap();
        session.transition(SessionState::Running).unwrap();
        session.transition(SessionState::Paused(PauseReason::UserRequest)).unwrap();
        session.transition(SessionState::Stepping).unwrap();
        assert!(session.transition(SessionState::Paused(PauseReason::Step)).is_ok());
    }

    #[test]
    fn transition_paused_to_running() {
        let mut session = DebugSession::new(make_target());
        session.transition(SessionState::Launching).unwrap();
        session.transition(SessionState::Running).unwrap();
        session.transition(SessionState::Paused(PauseReason::UserRequest)).unwrap();
        assert!(session.transition(SessionState::Running).is_ok());
    }

    #[test]
    fn transition_terminated_is_terminal() {
        let mut session = DebugSession::new(make_target());
        session.transition(SessionState::Launching).unwrap();
        session.transition(SessionState::Running).unwrap();
        session.transition(SessionState::Terminated(0)).unwrap();
        assert!(matches!(session.transition(SessionState::Running), Err(DebuggerError::InvalidState { .. })));
    }

    #[test]
    fn full_session_lifecycle() {
        let mut session = DebugSession::new(make_target());
        session.transition(SessionState::Launching).unwrap();
        session.transition(SessionState::Running).unwrap();
        session.transition(SessionState::Paused(PauseReason::Breakpoint(1))).unwrap();
        session.transition(SessionState::Stepping).unwrap();
        session.transition(SessionState::Paused(PauseReason::Step)).unwrap();
        session.transition(SessionState::Running).unwrap();
        session.transition(SessionState::Terminated(0)).unwrap();
    }
}
