// T010, T011, T012 — stub
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type ThreadId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessHandle {
    pub pid: u32,
    pub selected_thread: ThreadId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    pub file: PathBuf,
    pub line: u32,
    pub column: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackFrame {
    pub index: u32,
    pub function_name: Option<String>,
    pub module: Option<String>,
    pub source_location: Option<SourceLocation>,
    pub is_inlined: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ThreadState {
    Running,
    Stopped,
    Suspended,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StopReason {
    Breakpoint { id: u32, location: u32 },
    Step,
    Signal { name: String, number: u32 },
    Exception { description: String },
    PlanComplete,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadInfo {
    pub id: ThreadId,
    pub name: Option<String>,
    pub state: ThreadState,
    pub stop_reason: Option<StopReason>,
    pub frame_count: usize,
}
