use win_debug_bridge::thread::WindowsDebugHandle as LLDBHandle;
use protocol::tools::session::{LaunchInput, LaunchOutput, SessionStateOutput};
use runtime_core::error::DebuggerError;
use runtime_core::session::DebugTarget;
use tracing::{error, info, instrument};
use std::sync::Arc;
use tokio::sync::Mutex;

use debug_session_view::DebugSessionView;

/// Shared state accessible from all handlers.
pub struct SessionContext {
    pub handle: LLDBHandle,
    pub session_id: Arc<Mutex<Option<String>>>,
    pub pid: Arc<Mutex<Option<u32>>>,
    pub view: Option<DebugSessionView>,
}

impl SessionContext {
    pub fn new(handle: LLDBHandle, view: Option<DebugSessionView>) -> Self {
        Self {
            handle,
            session_id: Arc::new(Mutex::new(None)),
            pid: Arc::new(Mutex::new(None)),
            view,
        }
    }
}

#[instrument(skip(ctx, input))]
pub async fn handle_launch_process(
    ctx: &SessionContext,
    input: LaunchInput,
) -> Result<LaunchOutput, DebuggerError> {
    info!(
        executable = %input.executable.display(),
        args = ?input.args,
        "launching process"
    );

    let target = DebugTarget {
        executable: input.executable,
        args: input.args,
        env: input.env,
        working_dir: input.working_dir,
    };

    let (pid, state) = ctx.handle.launch_process(target).await
        .map_err(|e| {
            error!(error = %e, "launch_process failed");
            e
        })?;

    let session_id = uuid::Uuid::new_v4().to_string();
    *ctx.session_id.lock().await = Some(session_id.clone());
    *ctx.pid.lock().await = Some(pid);

    info!(pid, session_id = %session_id, "process launched successfully");

    Ok(LaunchOutput { session_id, pid, state })
}

#[instrument(skip(ctx))]
pub async fn handle_get_session_state(
    ctx: &SessionContext,
) -> Result<SessionStateOutput, DebuggerError> {
    let state = ctx.handle.get_state().await?;
    let session_id = ctx.session_id.lock().await
        .clone()
        .unwrap_or_else(|| "none".to_string());
    let pid = *ctx.pid.lock().await;

    Ok(SessionStateOutput {
        session_id,
        state,
        pid,
        selected_thread: None,
    })
}
