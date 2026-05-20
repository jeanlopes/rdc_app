use tokio::sync::{mpsc, oneshot};
use tracing::instrument;
use runtime_core::{
    error::DebuggerError,
    session::{DebugTarget, SessionState},
    process::{ThreadId, ThreadInfo, StackFrame},
    breakpoint::{Breakpoint, BreakpointId, BreakpointKind},
    variable::{Variable, EvalResult},
};

// Re-exported so callers in lldb-bridge::lib and mcp-server can use it.
pub use protocol::tools::execution::ExecutionEvent;

/// One variant per debugger operation. Each carries its input payload and a
/// oneshot channel for the reply.
pub enum LLDBCommand {
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

/// Async handle to the LLDB OS thread. Clone-able; wraps the mpsc sender.
#[derive(Clone)]
pub struct LLDBHandle {
    tx: mpsc::Sender<LLDBCommand>,
}

impl LLDBHandle {
    /// Spawn the LLDB OS thread and return a handle to communicate with it.
    pub fn spawn() -> Result<Self, DebuggerError> {
        let (tx, rx) = mpsc::channel::<LLDBCommand>(64);
        let backend = crate::python_backend::PythonBackend::new()?;
        std::thread::Builder::new()
            .name("lldb-worker".to_string())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("lldb thread tokio runtime");
                rt.block_on(run_loop(backend, rx));
            })
            .map_err(|e| DebuggerError::LLDBError(e.to_string()))?;
        Ok(Self { tx })
    }

    /// Send a command and await the reply (bridges async caller → sync LLDB thread).
    async fn send<T>(
        &self,
        make_cmd: impl FnOnce(oneshot::Sender<T>) -> LLDBCommand,
    ) -> Result<T, DebuggerError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(make_cmd(reply_tx))
            .await
            .map_err(|_| DebuggerError::LLDBError("lldb thread disconnected".into()))?;
        reply_rx
            .await
            .map_err(|_| DebuggerError::LLDBError("lldb reply channel dropped".into()))
    }

    #[instrument(skip(self))]
    pub async fn launch_process(&self, target: DebugTarget) -> Result<(u32, SessionState), DebuggerError> {
        self.send(|r| LLDBCommand::LaunchProcess { target, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn get_state(&self) -> Result<SessionState, DebuggerError> {
        self.send(|r| LLDBCommand::GetState { reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn set_breakpoint(&self, kind: BreakpointKind, condition: Option<String>) -> Result<Breakpoint, DebuggerError> {
        self.send(|r| LLDBCommand::SetBreakpoint { kind, condition, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn remove_breakpoint(&self, id: BreakpointId) -> Result<(), DebuggerError> {
        self.send(|r| LLDBCommand::RemoveBreakpoint { id, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn list_breakpoints(&self) -> Result<Vec<Breakpoint>, DebuggerError> {
        self.send(|r| LLDBCommand::ListBreakpoints { reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn continue_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| LLDBCommand::ContinueExecution { reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn pause_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| LLDBCommand::PauseExecution { reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn step_over(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| LLDBCommand::StepOver { thread_id, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn step_into(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| LLDBCommand::StepInto { thread_id, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn step_out(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.send(|r| LLDBCommand::StepOut { thread_id, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn read_locals(&self, thread_id: Option<ThreadId>, frame_index: u32, probe_context: Option<String>, max_depth: u32) -> Result<Vec<Variable>, DebuggerError> {
        self.send(|r| LLDBCommand::ReadLocals { thread_id, frame_index, probe_context, max_depth, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn read_stack(&self, thread_id: Option<ThreadId>, max_frames: u32) -> Result<Vec<StackFrame>, DebuggerError> {
        self.send(|r| LLDBCommand::ReadStack { thread_id, max_frames, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn evaluate_expression(&self, expression: String, thread_id: Option<ThreadId>, frame_index: u32) -> Result<EvalResult, DebuggerError> {
        self.send(|r| LLDBCommand::EvaluateExpression { expression, thread_id, frame_index, reply: r }).await?
    }

    #[instrument(skip(self))]
    pub async fn list_threads(&self) -> Result<Vec<ThreadInfo>, DebuggerError> {
        self.send(|r| LLDBCommand::ListThreads { reply: r }).await?
    }
}

/// Event loop running on the LLDB OS thread — dispatches commands to PythonBackend.
async fn run_loop(backend: crate::python_backend::PythonBackend, mut rx: mpsc::Receiver<LLDBCommand>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            LLDBCommand::LaunchProcess { target, reply } => {
                let _ = reply.send(backend.launch_process(target));
            }
            LLDBCommand::GetState { reply } => {
                let _ = reply.send(backend.get_state());
            }
            LLDBCommand::SetBreakpoint { kind, condition, reply } => {
                let _ = reply.send(backend.set_breakpoint(kind, condition));
            }
            LLDBCommand::RemoveBreakpoint { id, reply } => {
                let _ = reply.send(backend.remove_breakpoint(id));
            }
            LLDBCommand::ListBreakpoints { reply } => {
                let _ = reply.send(backend.list_breakpoints());
            }
            LLDBCommand::ContinueExecution { reply } => {
                let _ = reply.send(backend.continue_execution());
            }
            LLDBCommand::PauseExecution { reply } => {
                let _ = reply.send(backend.pause_execution());
            }
            LLDBCommand::StepOver { thread_id, reply } => {
                let _ = reply.send(backend.step_over(thread_id));
            }
            LLDBCommand::StepInto { thread_id, reply } => {
                let _ = reply.send(backend.step_into(thread_id));
            }
            LLDBCommand::StepOut { thread_id, reply } => {
                let _ = reply.send(backend.step_out(thread_id));
            }
            LLDBCommand::ReadLocals { thread_id, frame_index, probe_context, max_depth, reply } => {
                let _ = reply.send(backend.read_locals(thread_id, frame_index, probe_context, max_depth));
            }
            LLDBCommand::ReadStack { thread_id, max_frames, reply } => {
                let _ = reply.send(backend.read_stack(thread_id, max_frames));
            }
            LLDBCommand::EvaluateExpression { expression, thread_id, frame_index, reply } => {
                let _ = reply.send(backend.evaluate_expression(expression, thread_id, frame_index));
            }
            LLDBCommand::ListThreads { reply } => {
                let _ = reply.send(backend.list_threads());
            }
        }
    }
    tracing::info!("lldb-worker loop exited — channel closed");
}
