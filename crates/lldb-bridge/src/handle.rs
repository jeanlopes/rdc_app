use std::path::PathBuf;
use std::sync::Arc;

use runtime_core::breakpoint::{Breakpoint, BreakpointId, BreakpointKind, BreakpointLocation};
use runtime_core::error::DebuggerError;
use runtime_core::process::{SourceLocation, StackFrame, ThreadId, ThreadInfo};
use runtime_core::session::{DebugTarget, PauseReason, SessionState};
use runtime_core::variable::{EvalResult, Variable};

use protocol::tools::execution::ExecutionEvent;

use crate::client::DapClient;
use crate::transport::DapTransport;

/// Async handle to the LLDB debug adapter (codelldb).
#[derive(Clone)]
pub struct LldbDebugHandle {
    client: Arc<DapClient>,
}

impl LldbDebugHandle {
    /// Spawn codelldb, perform DAP handshake, and return a handle.
    #[deprecated(since = "0.1.0", note = "use lldb-native::LldbNativeHandle instead")]
    pub fn spawn() -> Result<Self, DebuggerError> {
        let codelldb_path = find_codelldb().map_err(|e| DebuggerError::DebuggerError(e.to_string()))?;
        let (transport, event_rx) =
            DapTransport::spawn(codelldb_path).map_err(|e| DebuggerError::DebuggerError(e.to_string()))?;
        let client = Arc::new(DapClient::new(transport, event_rx));

        // We need to run initialize synchronously before returning.
        // Since spawn() is sync, we'll use a temporary runtime or return a future.
        // For compatibility with WindowsDebugHandle::spawn() which is sync,
        // we defer initialize to the first async call.
        Ok(Self { client })
    }

    /// Internal: ensure DAP handshake is done.
    async fn ensure_init(&self) -> Result<(), DebuggerError> {
        // TODO: use a once-cell or atomic flag to avoid re-initializing
        self.client.initialize().await
    }

    pub async fn launch_process(&self, target: DebugTarget) -> Result<(u32, SessionState), DebuggerError> {
        self.ensure_init().await?;
        self.client.launch(target).await?;
        // codelldb defers the launch response until configurationDone is sent.
        self.client.configuration_done().await?;

        // Wait for the first stopped event (entry point because stopOnEntry:true).
        let event = self.client.wait_for_stop().await?;
        let state = match &event.kind {
            protocol::tools::execution::ExecutionEventKind::Paused => {
                // Entry stop — continue so it runs until the first breakpoint.
                let cont_event = self.client.continue_execution().await?;
                match cont_event.kind {
                    protocol::tools::execution::ExecutionEventKind::BreakpointHit
                    | protocol::tools::execution::ExecutionEventKind::StepComplete
                    | protocol::tools::execution::ExecutionEventKind::Paused => {
                        SessionState::Paused(PauseReason::Breakpoint(0))
                    }
                    protocol::tools::execution::ExecutionEventKind::Terminated { .. } => {
                        SessionState::Terminated(0)
                    }
                    _ => SessionState::Running,
                }
            }
            protocol::tools::execution::ExecutionEventKind::BreakpointHit => {
                SessionState::Paused(PauseReason::Breakpoint(0))
            }
            protocol::tools::execution::ExecutionEventKind::Terminated { .. } => {
                SessionState::Terminated(0)
            }
            _ => SessionState::Running,
        };
        Ok((0, state))
    }

    pub async fn get_state(&self) -> Result<SessionState, DebuggerError> {
        // DAP does not have a direct get-state; we track it ourselves.
        Ok(SessionState::Running)
    }

    pub async fn set_breakpoint(
        &self,
        kind: BreakpointKind,
        _condition: Option<String>,
    ) -> Result<Breakpoint, DebuggerError> {
        self.ensure_init().await?;
        let (id, dap_id, loc) = self.client.set_breakpoint(&kind).await?;
        let address = dap_id as u64; // placeholder
        Ok(Breakpoint {
            id,
            kind,
            condition: None,
            hit_count: 0,
            enabled: true,
            locations: vec![BreakpointLocation {
                address,
                source_location: loc,
                resolved: dap_id >= 0,
            }],
        })
    }

    pub async fn remove_breakpoint(&self, id: BreakpointId) -> Result<(), DebuggerError> {
        self.client.remove_breakpoint(id).await
    }

    pub async fn continue_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        self.client.continue_execution().await
    }

    pub async fn pause_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        self.client.pause_execution().await
    }

    pub async fn step_over(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.client.step_over(thread_id).await
    }

    pub async fn step_into(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.client.step_into(thread_id).await
    }

    pub async fn step_out(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.client.step_out(thread_id).await
    }

    pub async fn read_locals(
        &self,
        _thread_id: Option<ThreadId>,
        _frame_index: u32,
        _probe_context: Option<String>,
        _max_depth: u32,
    ) -> Result<Vec<Variable>, DebuggerError> {
        // TODO: implement via DAP scopes + variables
        Ok(vec![])
    }

    pub async fn read_stack(
        &self,
        _thread_id: Option<ThreadId>,
        _max_frames: u32,
    ) -> Result<Vec<StackFrame>, DebuggerError> {
        // TODO: implement via DAP stackTrace
        Ok(vec![])
    }

    pub async fn evaluate_expression(
        &self,
        _expression: String,
        _thread_id: Option<ThreadId>,
        _frame_index: u32,
    ) -> Result<EvalResult, DebuggerError> {
        // TODO: implement via DAP evaluate
        Ok(EvalResult {
            expression: "".into(),
            value: runtime_core::variable::VariableValue::Opaque { summary: "not implemented".into() },
            type_name: "".into(),
            error: Some("evaluate not implemented".into()),
        })
    }

    pub async fn list_threads(&self) -> Result<Vec<ThreadInfo>, DebuggerError> {
        // TODO: implement via DAP threads
        Ok(vec![])
    }

    pub async fn list_breakpoints(&self) -> Result<Vec<Breakpoint>, DebuggerError> {
        // TODO: implement via DAP breakpoints
        Ok(vec![])
    }
}

fn find_codelldb() -> anyhow::Result<PathBuf> {
    // 1. Check PATH
    if let Ok(path) = which::which("codelldb") {
        return Ok(path);
    }

    // 2. Check bundled location relative to project
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default());
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest_dir);

    // 2a. Check tools/codelldb/ (where the user extracted the standalone adapter)
    let tools_bundled = workspace_root.join("tools/codelldb/extension/adapter/codelldb.exe");
    if tools_bundled.exists() {
        return Ok(tools_bundled);
    }

    // 2b. Check codelldb/ directly at workspace root (fallback)
    let bundled = workspace_root.join("codelldb/extension/adapter/codelldb.exe");
    if bundled.exists() {
        return Ok(bundled);
    }

    // 3. Check VS Code extensions
    let home = dirs::home_dir().unwrap_or_default();
    for ext_dir in [
        home.join(".vscode/extensions"),
        home.join(".vscode-insiders/extensions"),
    ] {
        if let Ok(entries) = std::fs::read_dir(&ext_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("vadimcn.vscode-lldb") {
                    let candidate = entry.path().join("adapter/codelldb.exe");
                    if candidate.exists() {
                        return Ok(candidate);
                    }
                }
            }
        }
    }

    Err(anyhow::anyhow!(
        "codelldb not found. Please install CodeLLDB extension in VS Code, \
         or download the standalone adapter from https://github.com/vadimcn/codelldb/releases \
         and extract it to ./tools/codelldb/ (so that ./tools/codelldb/extension/adapter/codelldb.exe exists)"
    ))
}
