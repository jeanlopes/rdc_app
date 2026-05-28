use serde::Deserialize;

// Re-export from runtime-core — ExecutionEvent is authoritative there.
pub use runtime_core::event::{ExecutionEvent, ExecutionEventKind};

#[derive(Debug, Deserialize)]
pub struct StepInput {
    pub thread_id: Option<u64>,
}
