use runtime_core::backend::DebugBackend;
use runtime_core::error::DebuggerError;
use protocol::tools::breakpoints::{
    BreakpointOutput, ListBreakpointsOutput, RemoveBreakpointInput, SetBreakpointInput,
};
use tracing::{info, instrument};
use debug_session_view::ToolbarAction;

#[instrument(skip(handle, input))]
pub async fn handle_set_breakpoint(
    handle: &dyn DebugBackend,
    input: SetBreakpointInput,
    view: &Option<debug_session_view::DebugSessionView>,
) -> Result<BreakpointOutput, DebuggerError> {
    let (kind, condition) = input.into_kind_and_condition();
    let bp = handle.set_breakpoint(kind, condition).await?;
    if let Some(v) = view {
        v.publish_action(ToolbarAction::ShowNextStatement).await;
    }
    info!(breakpoint_id = bp.id, resolved = !bp.locations.is_empty(), "breakpoint set");
    Ok(BreakpointOutput { breakpoint: bp })
}

#[instrument(skip(handle, input))]
pub async fn handle_remove_breakpoint(
    handle: &dyn DebugBackend,
    input: RemoveBreakpointInput,
    view: &Option<debug_session_view::DebugSessionView>,
) -> Result<(), DebuggerError> {
    handle.remove_breakpoint(input.id).await?;
    if let Some(v) = view {
        v.publish_action(ToolbarAction::ShowNextStatement).await;
    }
    info!(breakpoint_id = input.id, "breakpoint removed");
    Ok(())
}

#[instrument(skip(handle))]
pub async fn handle_list_breakpoints(
    handle: &dyn DebugBackend,
) -> Result<ListBreakpointsOutput, DebuggerError> {
    let breakpoints = handle.list_breakpoints().await?;
    Ok(ListBreakpointsOutput { breakpoints })
}
