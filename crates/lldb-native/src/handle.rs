use std::sync::mpsc;

use async_trait::async_trait;
use tokio::sync::oneshot;

use runtime_core::{
    backend::DebugBackend,
    breakpoint::{Breakpoint, BreakpointId, BreakpointKind},
    error::DebuggerError,
    event::ExecutionEvent,
    process::{StackFrame, ThreadId, ThreadInfo},
    session::{DebugTarget, SessionState},
    variable::{EvalResult, Variable},
};

use crate::command::LldbCommand;
use crate::thread::LldbDebugThread;

/// Async handle to the in-process LLDB debug backend.
///
/// Implements [`DebugBackend`] — construct via [`LldbNativeHandle::spawn`] and
/// share across tasks with `Arc<dyn DebugBackend>`.
///
/// # Example
/// ```no_run
/// use lldb_native::LldbNativeHandle;
/// use runtime_core::backend::DebugBackend;
/// use runtime_core::session::DebugTarget;
/// use runtime_core::breakpoint::BreakpointKind;
/// use std::sync::Arc;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let handle: Arc<dyn DebugBackend> = Arc::new(LldbNativeHandle::spawn()?);
/// handle.set_breakpoint(BreakpointKind::SourceLine {
///     file: "main.rs".into(), line: 10,
/// }, None).await?;
/// let target = DebugTarget {
///     executable: "my_app.exe".into(),
///     args: vec![],
///     env: Default::default(),
///     working_dir: None,
/// };
/// let (pid, _state) = handle.launch_process(target).await?;
/// let event = handle.continue_execution().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct LldbNativeHandle {
    tx: mpsc::SyncSender<LldbCommand>,
}

impl LldbNativeHandle {
    /// Spawn the dedicated LLDB OS thread and return a handle.
    pub fn spawn() -> Result<Self, DebuggerError> {
        // Bounded channel with capacity 32 to apply light back-pressure.
        let (tx, rx) = mpsc::sync_channel::<LldbCommand>(32);
        std::thread::Builder::new()
            .name("lldb-native-worker".to_string())
            .spawn(move || LldbDebugThread::run(rx))
            .map_err(|e| DebuggerError::DebuggerError(e.to_string()))?;
        Ok(Self { tx })
    }

    /// Send a command to the LLDB thread and await the reply.
    async fn send<T: Send + 'static>(
        &self,
        make_cmd: impl FnOnce(oneshot::Sender<T>) -> LldbCommand,
    ) -> Result<T, DebuggerError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let cmd = make_cmd(reply_tx);
        self.tx
            .send(cmd)
            .map_err(|_| DebuggerError::DebuggerError("lldb-native thread disconnected".into()))?;
        reply_rx
            .await
            .map_err(|_| DebuggerError::DebuggerError("lldb-native reply channel dropped".into()))
    }
}

#[async_trait]
impl DebugBackend for LldbNativeHandle {
    async fn launch_process(&self, target: DebugTarget) -> Result<(u32, SessionState), DebuggerError> {
        self.send(|r| LldbCommand::LaunchProcess { target, reply: r }).await?
    }

    async fn attach_to_pid(&self, pid: u64) -> Result<(u64, SessionState), DebuggerError> {
        self.send(|r| LldbCommand::AttachToPid { pid, reply: r }).await?
    }

    async fn get_state(&self) -> Result<SessionState, DebuggerError> {
        self.send(|r| LldbCommand::GetState { reply: r }).await?
    }

    async fn set_breakpoint(&self, kind: BreakpointKind, condition: Option<String>) -> Result<Breakpoint, DebuggerError> {
        self.send(|r| LldbCommand::SetBreakpoint { kind, condition, reply: r }).await?
    }

    async fn remove_breakpoint(&self, id: BreakpointId) -> Result<(), DebuggerError> {
        self.send(|r| LldbCommand::RemoveBreakpoint { id, reply: r }).await?
    }

    async fn list_breakpoints(&self) -> Result<Vec<Breakpoint>, DebuggerError> {
        self.send(|r| LldbCommand::ListBreakpoints { reply: r }).await?
    }

    async fn continue_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| LldbCommand::Continue { reply: r }).await?
    }

    async fn pause_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| LldbCommand::Pause { reply: r }).await?
    }

    async fn step_over(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| LldbCommand::StepOver { thread_id, reply: r }).await?
    }

    async fn step_into(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| LldbCommand::StepInto { thread_id, reply: r }).await?
    }

    async fn step_out(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| LldbCommand::StepOut { thread_id, reply: r }).await?
    }

    async fn read_locals(
        &self,
        thread_id: Option<ThreadId>,
        frame_index: u32,
        probe_context: Option<String>,
        max_depth: u32,
    ) -> Result<Vec<Variable>, DebuggerError> {
        self.send(|r| LldbCommand::ReadLocals { thread_id, frame_index, probe_context, max_depth, reply: r }).await?
    }

    async fn read_stack(&self, thread_id: Option<ThreadId>, max_frames: u32) -> Result<Vec<StackFrame>, DebuggerError> {
        self.send(|r| LldbCommand::ReadStack { thread_id, max_frames, reply: r }).await?
    }

    async fn evaluate_expression(
        &self,
        expression: String,
        thread_id: Option<ThreadId>,
        frame_index: u32,
    ) -> Result<EvalResult, DebuggerError> {
        self.send(|r| LldbCommand::EvaluateExpr { expression, thread_id, frame_index, reply: r }).await?
    }

    async fn list_threads(&self) -> Result<Vec<ThreadInfo>, DebuggerError> {
        self.send(|r| LldbCommand::ListThreads { reply: r }).await?
    }
}
