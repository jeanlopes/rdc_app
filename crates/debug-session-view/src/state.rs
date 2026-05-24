//! Shared UI state for the debug session.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use crate::action::ToolbarAction;

/// Current high-level state of the debug session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DebugSessionState {
    /// No binary loaded.
    #[default]
    Idle,
    /// Target process executing.
    Running,
    /// Hit breakpoint or step completed.
    Paused,
    /// Process exited.
    Terminated,
}

/// A source location with resolve status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakpointEntry {
    /// Absolute path.
    pub file: PathBuf,
    /// 1-based line number.
    pub line: u32,
    /// True if the debugger confirmed a valid code address.
    pub resolved: bool,
}

/// One thread in the running process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadInfo {
    /// OS thread id.
    pub thread_id: u64,
    /// Optional name.
    pub name: Option<String>,
    /// True for the thread owning the current frame.
    pub is_active: bool,
}

/// The shared in-process state between the debug event producers and the egui render loop.
#[derive(Debug, Clone)]
pub struct DebugUIState {
    /// File currently shown in source viewer.
    pub active_file: Option<PathBuf>,
    /// 1-based line number of execution cursor.
    pub active_line: Option<u32>,
    /// All known breakpoints across files.
    pub breakpoints: Vec<BreakpointEntry>,
    /// Current session state.
    pub session_state: DebugSessionState,
    /// Last press timestamp per action.
    pub recent_actions: HashMap<ToolbarAction, Instant>,
    /// Threads for "Show Threads in Source".
    pub thread_list: Vec<ThreadInfo>,
    /// Toggle state for thread overlay.
    pub show_threads: bool,
    /// Transient error message (clears on next action).
    pub error_banner: Option<String>,
}

impl Default for DebugUIState {
    fn default() -> Self {
        Self {
            active_file: None,
            active_line: None,
            breakpoints: Vec::new(),
            session_state: DebugSessionState::Idle,
            recent_actions: HashMap::new(),
            thread_list: Vec::new(),
            show_threads: false,
            error_banner: None,
        }
    }
}

impl DebugUIState {
    /// Add a breakpoint if it does not already exist.
    #[tracing::instrument(skip(self))]
    pub fn add_breakpoint(&mut self, entry: BreakpointEntry) {
        if !self.breakpoints.iter().any(|b| b.file == entry.file && b.line == entry.line) {
            self.breakpoints.push(entry);
        }
    }

    /// Remove a breakpoint at the given file and line.
    #[tracing::instrument(skip(self))]
    pub fn remove_breakpoint(&mut self, file: &PathBuf, line: u32) {
        self.breakpoints.retain(|b| !(b.file == *file && b.line == line));
    }

    /// Record that an action was just pressed.
    #[tracing::instrument(skip(self))]
    pub fn press_action(&mut self, action: ToolbarAction) {
        self.recent_actions.insert(action, Instant::now());
        self.error_banner = None;
    }

    /// Check whether the given action is currently in its pressed state.
    #[tracing::instrument(skip(self))]
    pub fn is_pressed(&self, action: ToolbarAction) -> bool {
        self.recent_actions
            .get(&action)
            .map(|t| t.elapsed().as_millis() < 200)
            .unwrap_or(false)
    }

    /// Set the transient error banner.
    #[tracing::instrument(skip(self, message))]
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.error_banner = Some(message.into());
    }

    /// Clear the transient error banner.
    #[tracing::instrument(skip(self))]
    pub fn clear_error(&mut self) {
        self.error_banner = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_ui_state_new_is_idle() {
        let state = DebugUIState::default();
        assert_eq!(state.session_state, DebugSessionState::Idle);
        assert!(state.active_file.is_none());
        assert!(state.active_line.is_none());
    }

    #[test]
    fn breakpoint_add_and_remove() {
        let mut state = DebugUIState::default();
        let entry = BreakpointEntry {
            file: PathBuf::from("src/main.rs"),
            line: 10,
            resolved: false,
        };
        state.add_breakpoint(entry.clone());
        assert_eq!(state.breakpoints.len(), 1);

        state.remove_breakpoint(&PathBuf::from("src/main.rs"), 10);
        assert!(state.breakpoints.is_empty());
    }

    #[test]
    fn recent_actions_press_stored() {
        let mut state = DebugUIState::default();
        state.press_action(ToolbarAction::Continue);
        assert!(state.recent_actions.contains_key(&ToolbarAction::Continue));
        let elapsed = state.recent_actions[&ToolbarAction::Continue].elapsed();
        assert!(elapsed.as_secs() < 1);
    }

    #[test]
    fn error_banner_set_and_clear() {
        let mut state = DebugUIState::default();
        state.set_error("oops");
        assert_eq!(state.error_banner, Some("oops".to_string()));
        state.clear_error();
        assert!(state.error_banner.is_none());
    }
}
