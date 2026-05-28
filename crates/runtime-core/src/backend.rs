use async_trait::async_trait;

use crate::{
    breakpoint::{Breakpoint, BreakpointId, BreakpointKind},
    error::DebuggerError,
    event::ExecutionEvent,
    process::{StackFrame, ThreadId, ThreadInfo},
    session::{DebugTarget, SessionState},
    variable::{EvalResult, Variable},
};

/// Backend-agnostic async contract for a debug session.
///
/// `apps/mcp-server` uses `Arc<dyn DebugBackend>` and selects the
/// implementation at startup via `--backend lldb-native | win-debug-bridge`.
///
/// # Example
/// ```no_run
/// # use runtime_core::backend::DebugBackend;
/// # use runtime_core::session::{DebugTarget, SessionState};
/// # use std::sync::Arc;
/// # async fn example(backend: Arc<dyn DebugBackend>) {
/// let target = DebugTarget {
///     executable: "my_app.exe".into(),
///     args: vec![],
///     env: Default::default(),
///     working_dir: None,
/// };
/// let (pid, state) = backend.launch_process(target).await.unwrap();
/// # }
/// ```
#[async_trait]
pub trait DebugBackend: Send + Sync {
    // ── Session ──────────────────────────────────────────────────────────────

    /// Launch a binary as a new debugged process.
    /// Returns `(pid, initial_state)`. The process is stopped at the entry point.
    async fn launch_process(
        &self,
        target: DebugTarget,
    ) -> Result<(u32, SessionState), DebuggerError>;

    /// Attach the debugger to an already-running process by PID.
    /// Returns `(pid, initial_state)`.
    async fn attach_to_pid(
        &self,
        pid: u64,
    ) -> Result<(u64, SessionState), DebuggerError>;

    /// Return the current session / process state.
    async fn get_state(&self) -> Result<SessionState, DebuggerError>;

    // ── Breakpoints ──────────────────────────────────────────────────────────

    /// Set a breakpoint. Returns the created `Breakpoint` with its assigned ID.
    async fn set_breakpoint(
        &self,
        kind: BreakpointKind,
        condition: Option<String>,
    ) -> Result<Breakpoint, DebuggerError>;

    /// Remove a breakpoint by ID.
    async fn remove_breakpoint(&self, id: BreakpointId) -> Result<(), DebuggerError>;

    /// Return all active breakpoints.
    async fn list_breakpoints(&self) -> Result<Vec<Breakpoint>, DebuggerError>;

    // ── Execution control ────────────────────────────────────────────────────

    /// Resume execution. Blocks until the next stop event.
    async fn continue_execution(&self) -> Result<ExecutionEvent, DebuggerError>;

    /// Interrupt a running process and pause it.
    async fn pause_execution(&self) -> Result<ExecutionEvent, DebuggerError>;

    /// Step over the current source line. Blocks until the process stops again.
    async fn step_over(
        &self,
        thread_id: Option<ThreadId>,
    ) -> Result<ExecutionEvent, DebuggerError>;

    /// Step into the current source line. Blocks until the process stops again.
    async fn step_into(
        &self,
        thread_id: Option<ThreadId>,
    ) -> Result<ExecutionEvent, DebuggerError>;

    /// Step out of the current function. Blocks until the process stops again.
    async fn step_out(
        &self,
        thread_id: Option<ThreadId>,
    ) -> Result<ExecutionEvent, DebuggerError>;

    // ── Inspection ───────────────────────────────────────────────────────────

    /// Return in-scope local variables for the given frame.
    async fn read_locals(
        &self,
        thread_id: Option<ThreadId>,
        frame_index: u32,
        probe_context: Option<String>,
        max_depth: u32,
    ) -> Result<Vec<Variable>, DebuggerError>;

    /// Return the call stack for the given thread.
    async fn read_stack(
        &self,
        thread_id: Option<ThreadId>,
        max_frames: u32,
    ) -> Result<Vec<StackFrame>, DebuggerError>;

    /// Evaluate an expression in the context of the given frame.
    async fn evaluate_expression(
        &self,
        expression: String,
        thread_id: Option<ThreadId>,
        frame_index: u32,
    ) -> Result<EvalResult, DebuggerError>;

    /// List all threads in the current process.
    async fn list_threads(&self) -> Result<Vec<ThreadInfo>, DebuggerError>;
}
