use win_debug_bridge::thread::WindowsDebugHandle as LLDBHandle;
use protocol::tools::breakpoints::{
    BreakpointOutput, ListBreakpointsOutput, RemoveBreakpointInput, SetBreakpointInput,
};
use runtime_core::error::DebuggerError;
use tracing::{info, instrument};

#[instrument(skip(handle, input))]
pub async fn handle_set_breakpoint(
    handle: &LLDBHandle,
    input: SetBreakpointInput,
) -> Result<BreakpointOutput, DebuggerError> {
    let (kind, condition) = input.into_kind_and_condition();
    let bp = handle.set_breakpoint(kind, condition).await?;
    info!(breakpoint_id = bp.id, resolved = !bp.locations.is_empty(), "breakpoint set");
    Ok(BreakpointOutput { breakpoint: bp })
}

#[instrument(skip(handle, input))]
pub async fn handle_remove_breakpoint(
    handle: &LLDBHandle,
    input: RemoveBreakpointInput,
) -> Result<(), DebuggerError> {
    handle.remove_breakpoint(input.id).await?;
    info!(breakpoint_id = input.id, "breakpoint removed");
    Ok(())
}

#[instrument(skip(handle))]
pub async fn handle_list_breakpoints(
    handle: &LLDBHandle,
) -> Result<ListBreakpointsOutput, DebuggerError> {
    let breakpoints = handle.list_breakpoints().await?;
    Ok(ListBreakpointsOutput { breakpoints })
}
