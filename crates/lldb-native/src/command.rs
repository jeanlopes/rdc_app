use tokio::sync::oneshot;
use runtime_core::{
    error::DebuggerError,
    session::{DebugTarget, SessionState},
    breakpoint::{Breakpoint, BreakpointId, BreakpointKind},
    process::{StackFrame, ThreadId, ThreadInfo},
    variable::{EvalResult, Variable},
    event::{ExecutionEvent},
};

pub(crate) enum LldbCommand {
    LaunchProcess {
        target: DebugTarget,
        reply: oneshot::Sender<Result<(u32, SessionState), DebuggerError>>,
    },
    AttachToPid {
        pid: u64,
        reply: oneshot::Sender<Result<(u64, SessionState), DebuggerError>>,
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
    Continue {
        reply: oneshot::Sender<Result<ExecutionEvent, DebuggerError>>,
    },
    Pause {
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
    EvaluateExpr {
        expression: String,
        thread_id: Option<ThreadId>,
        frame_index: u32,
        reply: oneshot::Sender<Result<EvalResult, DebuggerError>>,
    },
    ListThreads {
        reply: oneshot::Sender<Result<Vec<ThreadInfo>, DebuggerError>>,
    },
}
