use serde::{Deserialize, Serialize};
use runtime_core::process::SourceLocation;

#[derive(Debug, Deserialize)]
pub struct StepInput {
    pub thread_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionEventKind {
    BreakpointHit,
    StepComplete,
    Paused,
    Terminated { exit_code: i32 },
    PanicDetected { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionEvent {
    pub kind: ExecutionEventKind,
    pub thread_id: u64,
    pub location: Option<SourceLocation>,
}
