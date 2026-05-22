use win_debug_bridge::thread::{ExecutionEvent, WindowsDebugHandle as LLDBHandle};
use protocol::tools::execution::{ExecutionEventKind, StepInput};
use runtime_core::error::DebuggerError;
use tracing::{info, instrument, warn};

#[instrument(skip(handle))]
pub async fn handle_continue_execution(handle: &LLDBHandle) -> Result<ExecutionEvent, DebuggerError> {
    let event = handle.continue_execution().await?;
    match &event.kind {
        ExecutionEventKind::BreakpointHit => info!(thread_id = event.thread_id, "breakpoint hit"),
        ExecutionEventKind::PanicDetected { message } => {
            warn!(thread_id = event.thread_id, panic = %message, "panic detected");
        }
        ExecutionEventKind::Terminated { exit_code } => {
            info!(exit_code, "process terminated");
        }
        _ => {}
    }
    Ok(event)
}

#[instrument(skip(handle))]
pub async fn handle_pause_execution(handle: &LLDBHandle) -> Result<ExecutionEvent, DebuggerError> {
    handle.pause_execution().await
}

#[instrument(skip(handle))]
pub async fn handle_step_over(handle: &LLDBHandle, input: StepInput) -> Result<ExecutionEvent, DebuggerError> {
    let event = handle.step_over(input.thread_id).await?;
    info!(thread_id = event.thread_id, "step_over complete");
    Ok(event)
}

#[instrument(skip(handle))]
pub async fn handle_step_into(handle: &LLDBHandle, input: StepInput) -> Result<ExecutionEvent, DebuggerError> {
    handle.step_into(input.thread_id).await
}

#[instrument(skip(handle))]
pub async fn handle_step_out(handle: &LLDBHandle, input: StepInput) -> Result<ExecutionEvent, DebuggerError> {
    handle.step_out(input.thread_id).await
}
