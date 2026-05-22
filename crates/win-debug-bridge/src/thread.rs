use tokio::sync::{mpsc, oneshot};
use tracing::instrument;
use runtime_core::{
    error::DebuggerError,
    session::{DebugTarget, SessionState},
    process::{ThreadId, ThreadInfo, StackFrame},
    breakpoint::{Breakpoint, BreakpointId, BreakpointKind},
    variable::{Variable, EvalResult},
};
pub use protocol::tools::execution::ExecutionEvent;

/// One variant per debugger operation.
/// Each carries its input payload and a oneshot reply channel.
pub enum DebugCommand {
    LaunchProcess {
        target: DebugTarget,
        reply: oneshot::Sender<Result<(u32, SessionState), DebuggerError>>,
    },
    GetState {
        reply: oneshot::Sender<Result<SessionState, DebuggerError>>,
    },
    SetBreakpoint {
        kind: BreakpointKind,
        condition: Option<String>,
        reply: oneshot::Sender<Result<Breakpoint, DebuggerError>>,
    },
    RemoveBreakpoint {
        id: BreakpointId,
        reply: oneshot::Sender<Result<(), DebuggerError>>,
    },
    ListBreakpoints {
        reply: oneshot::Sender<Result<Vec<Breakpoint>, DebuggerError>>,
    },
    ContinueExecution {
        reply: oneshot::Sender<Result<ExecutionEvent, DebuggerError>>,
    },
    PauseExecution {
        reply: oneshot::Sender<Result<ExecutionEvent, DebuggerError>>,
    },
    StepOver {
        thread_id: Option<ThreadId>,
        reply: oneshot::Sender<Result<ExecutionEvent, DebuggerError>>,
    },
    StepInto {
        thread_id: Option<ThreadId>,
        reply: oneshot::Sender<Result<ExecutionEvent, DebuggerError>>,
    },
    StepOut {
        thread_id: Option<ThreadId>,
        reply: oneshot::Sender<Result<ExecutionEvent, DebuggerError>>,
    },
    ReadLocals {
        thread_id: Option<ThreadId>,
        frame_index: u32,
        probe_context: Option<String>,
        max_depth: u32,
        reply: oneshot::Sender<Result<Vec<Variable>, DebuggerError>>,
    },
    ReadStack {
        thread_id: Option<ThreadId>,
        max_frames: u32,
        reply: oneshot::Sender<Result<Vec<StackFrame>, DebuggerError>>,
    },
    EvaluateExpression {
        expression: String,
        thread_id: Option<ThreadId>,
        frame_index: u32,
        reply: oneshot::Sender<Result<EvalResult, DebuggerError>>,
    },
    ListThreads {
        reply: oneshot::Sender<Result<Vec<ThreadInfo>, DebuggerError>>,
    },
}

/// Async handle to the Windows debug OS thread.
#[derive(Clone)]
pub struct WindowsDebugHandle {
    tx: mpsc::Sender<DebugCommand>,
}

impl WindowsDebugHandle {
    /// Spawn the Windows debug OS thread and return a handle.
    ///
    /// # Example
    /// ```no_run
    /// use win_debug_bridge::thread::WindowsDebugHandle;
    /// let handle = WindowsDebugHandle::spawn().unwrap();
    /// ```
    pub fn spawn() -> Result<Self, DebuggerError> {
        let (tx, rx) = mpsc::channel::<DebugCommand>(64);
        let backend = crate::windows_backend::WindowsDebugBackend::new()?;
        std::thread::Builder::new()
            .name("win-debug-worker".to_string())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("win-debug thread runtime");
                rt.block_on(run_loop(backend, rx));
            })
            .map_err(|e| DebuggerError::DebuggerError(e.to_string()))?;
        Ok(Self { tx })
    }

    async fn send<T>(
        &self,
        make_cmd: impl FnOnce(oneshot::Sender<T>) -> DebugCommand,
    ) -> Result<T, DebuggerError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(make_cmd(reply_tx))
            .await
            .map_err(|_| DebuggerError::DebuggerError("debug thread disconnected".into()))?;
        reply_rx
            .await
            .map_err(|_| DebuggerError::DebuggerError("reply channel dropped".into()))
    }

    #[instrument(skip(self))]
    pub async fn launch_process(&self, target: DebugTarget) -> Result<(u32, SessionState), DebuggerError> {
        self.send(|r| DebugCommand::LaunchProcess { target, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn get_state(&self) -> Result<SessionState, DebuggerError> {
        self.send(|r| DebugCommand::GetState { reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn set_breakpoint(&self, kind: BreakpointKind, condition: Option<String>) -> Result<Breakpoint, DebuggerError> {
        self.send(|r| DebugCommand::SetBreakpoint { kind, condition, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn remove_breakpoint(&self, id: BreakpointId) -> Result<(), DebuggerError> {
        self.send(|r| DebugCommand::RemoveBreakpoint { id, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn list_breakpoints(&self) -> Result<Vec<Breakpoint>, DebuggerError> {
        self.send(|r| DebugCommand::ListBreakpoints { reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn continue_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| DebugCommand::ContinueExecution { reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn pause_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| DebugCommand::PauseExecution { reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn step_over(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| DebugCommand::StepOver { thread_id, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn step_into(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| DebugCommand::StepInto { thread_id, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn step_out(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| DebugCommand::StepOut { thread_id, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn read_locals(&self, thread_id: Option<ThreadId>, frame_index: u32, probe_context: Option<String>, max_depth: u32) -> Result<Vec<Variable>, DebuggerError> {
        self.send(|r| DebugCommand::ReadLocals { thread_id, frame_index, probe_context, max_depth, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn read_stack(&self, thread_id: Option<ThreadId>, max_frames: u32) -> Result<Vec<StackFrame>, DebuggerError> {
        self.send(|r| DebugCommand::ReadStack { thread_id, max_frames, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn evaluate_expression(&self, expression: String, thread_id: Option<ThreadId>, frame_index: u32) -> Result<EvalResult, DebuggerError> {
        self.send(|r| DebugCommand::EvaluateExpression { expression, thread_id, frame_index, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn list_threads(&self) -> Result<Vec<ThreadInfo>, DebuggerError> {
        self.send(|r| DebugCommand::ListThreads { reply: r }).await?
    }
}

async fn run_loop(backend: crate::windows_backend::WindowsDebugBackend, mut rx: mpsc::Receiver<DebugCommand>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            DebugCommand::LaunchProcess { target, reply } => { let _ = reply.send(backend.launch_process(target)); }
            DebugCommand::GetState { reply } => { let _ = reply.send(backend.get_state()); }
            DebugCommand::SetBreakpoint { kind, condition, reply } => { let _ = reply.send(backend.set_breakpoint(kind, condition)); }
            DebugCommand::RemoveBreakpoint { id, reply } => { let _ = reply.send(backend.remove_breakpoint(id)); }
            DebugCommand::ListBreakpoints { reply } => { let _ = reply.send(backend.list_breakpoints()); }
            DebugCommand::ContinueExecution { reply } => { let _ = reply.send(backend.continue_execution()); }
            DebugCommand::PauseExecution { reply } => { let _ = reply.send(backend.pause_execution()); }
            DebugCommand::StepOver { thread_id, reply } => { let _ = reply.send(backend.step_over(thread_id)); }
            DebugCommand::StepInto { thread_id, reply } => { let _ = reply.send(backend.step_into(thread_id)); }
            DebugCommand::StepOut { thread_id, reply } => { let _ = reply.send(backend.step_out(thread_id)); }
            DebugCommand::ReadLocals { thread_id, frame_index, probe_context, max_depth, reply } => {
                let _ = reply.send(backend.read_locals(thread_id, frame_index, probe_context, max_depth));
            }
            DebugCommand::ReadStack { thread_id, max_frames, reply } => { let _ = reply.send(backend.read_stack(thread_id, max_frames)); }
            DebugCommand::EvaluateExpression { expression, thread_id, frame_index, reply } => {
                let _ = reply.send(backend.evaluate_expression(expression, thread_id, frame_index));
            }
            DebugCommand::ListThreads { reply } => { let _ = reply.send(backend.list_threads()); }
        }
    }
    tracing::info!("win-debug-worker loop exited");
}
