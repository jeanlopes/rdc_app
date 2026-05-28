use std::sync::Arc;

use runtime_core::backend::DebugBackend;
use runtime_core::error::DebuggerError;
use runtime_core::session::DebugTarget;
use protocol::tools::session::{LaunchInput, LaunchOutput, SessionStateOutput};
use tracing::{error, info, instrument};
use tokio::sync::Mutex;

use debug_session_view::DebugSessionView;

/// Shared state accessible from all handlers.
pub struct SessionContext {
    pub backend: Arc<dyn DebugBackend>,
    pub session_id: Arc<Mutex<Option<String>>>,
    pub pid: Arc<Mutex<Option<u32>>>,
    pub view: Option<DebugSessionView>,
}

impl SessionContext {
    pub fn new(backend: Arc<dyn DebugBackend>, view: Option<DebugSessionView>) -> Self {
        Self {
            backend,
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

    let (pid, state) = ctx.backend.launch_process(target).await
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
    let state = ctx.backend.get_state().await?;
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
