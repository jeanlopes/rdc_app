use serde::{Deserialize, Serialize};
use crate::process::SourceLocation;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
