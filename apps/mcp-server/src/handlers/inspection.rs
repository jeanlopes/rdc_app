use runtime_core::backend::DebugBackend;
use runtime_core::error::DebuggerError;
use protocol::tools::inspection::{
    EvalInput, EvalOutput, ReadLocalsInput, ReadStackInput,
    StackOutput, ThreadListOutput, VariableListOutput,
};
use tracing::instrument;

#[instrument(skip(handle, input))]
pub async fn handle_read_locals(
    handle: &dyn DebugBackend,
    input: ReadLocalsInput,
) -> Result<VariableListOutput, DebuggerError> {
    let variables = handle
        .read_locals(input.thread_id, input.frame_index, input.probe_context.clone(), input.max_depth)
        .await?;
    Ok(VariableListOutput {
        probe_context: input.probe_context,
        variables,
    })
}

#[instrument(skip(handle, input))]
pub async fn handle_read_stack(
    handle: &dyn DebugBackend,
    input: ReadStackInput,
) -> Result<StackOutput, DebuggerError> {
    let frames = handle.read_stack(input.thread_id, input.max_frames).await?;
    let thread_id = input.thread_id.unwrap_or(0);
    Ok(StackOutput { thread_id, frames })
}

#[instrument(skip(handle, input), fields(expr = %input.expression))]
pub async fn handle_evaluate_expression(
    handle: &dyn DebugBackend,
    input: EvalInput,
) -> Result<EvalOutput, DebuggerError> {
    let result = handle
        .evaluate_expression(input.expression, input.thread_id, input.frame_index)
        .await?;
    Ok(EvalOutput { result })
}

#[instrument(skip(handle))]
pub async fn handle_list_threads(handle: &dyn DebugBackend) -> Result<ThreadListOutput, DebuggerError> {
    let threads = handle.list_threads().await?;
    Ok(ThreadListOutput {
        threads,
        selected_thread: 0,
    })
}
