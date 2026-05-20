use lldb_bridge::thread::LLDBHandle;
use protocol::tools::inspection::{
    EvalInput, EvalOutput, ReadLocalsInput, ReadStackInput,
    StackOutput, ThreadListOutput, VariableListOutput,
};
use runtime_core::error::DebuggerError;
use tracing::instrument;

#[instrument(skip(handle, input))]
pub async fn handle_read_locals(
    handle: &LLDBHandle,
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
    handle: &LLDBHandle,
    input: ReadStackInput,
) -> Result<StackOutput, DebuggerError> {
    let frames = handle.read_stack(input.thread_id, input.max_frames).await?;
    let thread_id = input.thread_id.unwrap_or(0);
    Ok(StackOutput { thread_id, frames })
}

#[instrument(skip(handle, input), fields(expr = %input.expression))]
pub async fn handle_evaluate_expression(
    handle: &LLDBHandle,
    input: EvalInput,
) -> Result<EvalOutput, DebuggerError> {
    let result = handle
        .evaluate_expression(input.expression, input.thread_id, input.frame_index)
        .await?;
    Ok(EvalOutput { result })
}

#[instrument(skip(handle))]
pub async fn handle_list_threads(handle: &LLDBHandle) -> Result<ThreadListOutput, DebuggerError> {
    let threads = handle.list_threads().await?;
    Ok(ThreadListOutput {
        threads,
        selected_thread: 0,
    })
}
