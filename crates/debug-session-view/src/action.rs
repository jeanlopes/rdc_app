//! Toolbar actions that can be triggered by the user or the AI agent.

use std::fmt;
use std::str::FromStr;

/// One of the 11 debugger commands plus the thread toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ToolbarAction {
    /// Resume execution.
    Continue,
    /// Pause all threads.
    BreakAll,
    /// Terminate the session.
    StopDebugging,
    /// Stop and re-launch.
    Restart,
    /// Steps back into last call.
    StepBackInto,
    /// Steps back over last statement.
    StepBackOver,
    /// Steps back out of current frame.
    StepBackOut,
    /// Scrolls viewer to active line.
    ShowNextStatement,
    /// Steps into next call.
    StepInto,
    /// Steps over next statement.
    StepOver,
    /// Steps out of current frame.
    StepOut,
    /// Toggles thread overlay.
    ShowThreadsInSource,
}

impl ToolbarAction {
    /// Returns an iterator over all toolbar actions.
    pub fn all() -> impl Iterator<Item = ToolbarAction> {
        use ToolbarAction::*;
        [
            Continue,
            BreakAll,
            StopDebugging,
            Restart,
            StepBackInto,
            StepBackOver,
            StepBackOut,
            ShowNextStatement,
            StepInto,
            StepOver,
            StepOut,
            ShowThreadsInSource,
        ]
        .into_iter()
    }
}

impl fmt::Display for ToolbarAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ToolbarAction::Continue => "Continue",
            ToolbarAction::BreakAll => "BreakAll",
            ToolbarAction::StopDebugging => "StopDebugging",
            ToolbarAction::Restart => "Restart",
            ToolbarAction::StepBackInto => "StepBackInto",
            ToolbarAction::StepBackOver => "StepBackOver",
            ToolbarAction::StepBackOut => "StepBackOut",
            ToolbarAction::ShowNextStatement => "ShowNextStatement",
            ToolbarAction::StepInto => "StepInto",
            ToolbarAction::StepOver => "StepOver",
            ToolbarAction::StepOut => "StepOut",
            ToolbarAction::ShowThreadsInSource => "ShowThreadsInSource",
        };
        write!(f, "{}", s)
    }
}

/// Error when parsing an unknown toolbar action.
#[derive(Debug, Clone, thiserror::Error)]
#[error("unknown toolbar action: {0}")]
pub struct ParseToolbarActionError(String);

impl FromStr for ToolbarAction {
    type Err = ParseToolbarActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Continue" => Ok(ToolbarAction::Continue),
            "BreakAll" => Ok(ToolbarAction::BreakAll),
            "StopDebugging" => Ok(ToolbarAction::StopDebugging),
            "Restart" => Ok(ToolbarAction::Restart),
            "StepBackInto" => Ok(ToolbarAction::StepBackInto),
            "StepBackOver" => Ok(ToolbarAction::StepBackOver),
            "StepBackOut" => Ok(ToolbarAction::StepBackOut),
            "ShowNextStatement" => Ok(ToolbarAction::ShowNextStatement),
            "StepInto" => Ok(ToolbarAction::StepInto),
            "StepOver" => Ok(ToolbarAction::StepOver),
            "StepOut" => Ok(ToolbarAction::StepOut),
            "ShowThreadsInSource" => Ok(ToolbarAction::ShowThreadsInSource),
            other => Err(ParseToolbarActionError(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_toolbar_actions_display() {
        for action in ToolbarAction::all() {
            let s = action.to_string();
            assert!(!s.is_empty(), "display for {:?} must be non-empty", action);
        }
    }

    #[test]
    fn toolbar_action_roundtrip() {
        for action in ToolbarAction::all() {
            let s = action.to_string();
            let parsed: ToolbarAction = s.parse().unwrap();
            assert_eq!(parsed, action);
        }
    }
}
