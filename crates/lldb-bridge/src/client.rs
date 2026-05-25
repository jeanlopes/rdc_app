use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::json;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, warn};

use runtime_core::breakpoint::BreakpointKind;
use runtime_core::error::DebuggerError;
use runtime_core::process::{SourceLocation, StackFrame, ThreadInfo};
use runtime_core::session::{DebugTarget, PauseReason, SessionState};
use runtime_core::variable::{EvalResult, Variable};

use protocol::tools::execution::{ExecutionEvent, ExecutionEventKind};

use crate::transport::DapTransport;

/// High-level DAP client wrapping codelldb.
pub struct DapClient {
    transport: DapTransport,
    event_rx: Mutex<mpsc::Receiver<serde_json::Value>>,
    next_bp_id: AtomicU32,
    /// Maps our internal breakpoint id -> DAP breakpoint id
    bp_id_map: Mutex<HashMap<u32, i64>>,
    /// Reverse map: DAP id -> internal id
    bp_rev_map: Mutex<HashMap<i64, u32>>,
    stop_thread_id: Mutex<Option<i64>>,
}

impl DapClient {
    pub fn new(transport: DapTransport, event_rx: mpsc::Receiver<serde_json::Value>) -> Self {
        Self {
            transport,
            event_rx: Mutex::new(event_rx),
            next_bp_id: AtomicU32::new(1),
            bp_id_map: Mutex::new(HashMap::new()),
            bp_rev_map: Mutex::new(HashMap::new()),
            stop_thread_id: Mutex::new(None),
        }
    }

    /// Perform initialize handshake.
    ///
    /// CodeLLDB sends the `initialized` event *after* `launch`, not immediately
    /// after the `initialize` response. We therefore only wait for the response
    /// here and let the event be consumed by the normal event loop.
    pub async fn initialize(&self) -> Result<(), DebuggerError> {
        info!("DAP handshake: initialize");
        let resp = self
            .transport
            .request(
                "initialize",
                Some(json!({
                    "clientID": "rdc-visual-debugger",
                    "clientName": "RDC Visual Debugger",
                    "adapterID": "lldb",
                    "linesStartAt1": true,
                    "columnsStartAt1": true,
                    "supportsRunInTerminalRequest": false,
                })),
            )
            .await
            .map_err(|e| DebuggerError::DebuggerError(format!("DAP initialize failed: {}", e)))?;
        info!("DAP initialize response received: {:?}", resp);
        Ok(())
    }

    pub async fn launch(&self, target: DebugTarget) -> Result<(), DebuggerError> {
        info!(executable = %target.executable.display(), "DAP launch");
        let exe = target.executable.to_string_lossy().to_string();
        let cwd = target
            .working_dir
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());

        let mut args = json!({
            "name": "debug",
            "type": "lldb",
            "request": "launch",
            "program": exe,
            "args": target.args,
            "stopOnEntry": true,
        });
        if let Some(c) = cwd {
            args["cwd"] = c.into();
        }

        self.transport
            .request("launch", Some(args))
            .await
            .map_err(|e| DebuggerError::DebuggerError(format!("DAP launch failed: {}", e)))?;
        Ok(())
    }

    pub async fn set_breakpoint(
        &self,
        kind: &BreakpointKind,
    ) -> Result<(u32, i64, Option<SourceLocation>), DebuggerError> {
        let (file, line) = match kind {
            BreakpointKind::SourceLine { file, line } => (file.clone(), *line),
            _ => {
                return Err(DebuggerError::DebuggerError(
                    "only SourceLine breakpoints supported via DAP".into(),
                ))
            }
        };

        let source = json!({
            "name": file.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
            "path": file.to_string_lossy().to_string(),
        });
        let breakpoints = json!([{ "line": line }]);

        let resp = self
            .transport
            .request(
                "setBreakpoints",
                Some(json!({
                    "source": source,
                    "breakpoints": breakpoints,
                })),
            )
            .await
            .map_err(|e| DebuggerError::DebuggerError(format!("setBreakpoints failed: {}", e)))?;

        let dap_id = resp
            .get("body")
            .and_then(|b| b.get("breakpoints"))
            .and_then(|b| b.as_array())
            .and_then(|arr| arr.first())
            .and_then(|bp| bp.get("id"))
            .and_then(|id| id.as_i64())
            .unwrap_or(-1);

        let verified = resp
            .get("body")
            .and_then(|b| b.get("breakpoints"))
            .and_then(|b| b.as_array())
            .and_then(|arr| arr.first())
            .and_then(|bp| bp.get("verified"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let loc = if verified {
            resp.get("body")
                .and_then(|b| b.get("breakpoints"))
                .and_then(|b| b.as_array())
                .and_then(|arr| arr.first())
                .and_then(|bp| {
                    let path = bp.get("source").and_then(|s| s.get("path")).and_then(|p| p.as_str())?;
                    let ln = bp.get("line").and_then(|l| l.as_u64())? as u32;
                    Some(SourceLocation {
                        file: PathBuf::from(path),
                        line: ln,
                        column: None,
                    })
                })
        } else {
            None
        };

        let id = self.next_bp_id.fetch_add(1, Ordering::SeqCst);
        {
            let mut map = self.bp_id_map.lock().await;
            map.insert(id, dap_id);
        }
        {
            let mut rev = self.bp_rev_map.lock().await;
            rev.insert(dap_id, id);
        }

        info!(id, dap_id, "breakpoint set");
        Ok((id, dap_id, loc))
    }

    pub async fn remove_breakpoint(&self, id: u32) -> Result<(), DebuggerError> {
        let dap_id = {
            let map = self.bp_id_map.lock().await;
            *map.get(&id).unwrap_or(&-1)
        };
        if dap_id < 0 {
            return Err(DebuggerError::BreakpointNotFound(id));
        }
        // DAP does not have a direct removeBreakpoint command.
        // We use setBreakpoints with an empty list for the source file.
        // To do this properly we'd need to track file per breakpoint.
        // For MVP: just clear all breakpoints for that file or ignore.
        // A proper implementation tracks source per breakpoint.
        info!(id, dap_id, "breakpoint removal requested (not fully implemented in MVP)");
        Ok(())
    }

    pub async fn continue_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        info!("DAP continue");
        self.transport
            .request("continue", Some(json!({})))
            .await
            .map_err(|e| DebuggerError::DebuggerError(format!("continue failed: {}", e)))?;
        self.wait_for_stop().await
    }

    pub async fn pause_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        info!("DAP pause");
        self.transport
            .request("pause", Some(json!({})))
            .await
            .map_err(|e| DebuggerError::DebuggerError(format!("pause failed: {}", e)))?;
        self.wait_for_stop().await
    }

    pub async fn step_over(&self, thread_id: Option<u64>) -> Result<ExecutionEvent, DebuggerError> {
        info!("DAP next");
        let tid = thread_id.unwrap_or(0);
        self.transport
            .request("next", Some(json!({ "threadId": tid })))
            .await
            .map_err(|e| DebuggerError::DebuggerError(format!("next failed: {}", e)))?;
        self.wait_for_stop().await
    }

    pub async fn step_into(&self, thread_id: Option<u64>) -> Result<ExecutionEvent, DebuggerError> {
        info!("DAP stepIn");
        let tid = thread_id.unwrap_or(0);
        self.transport
            .request("stepIn", Some(json!({ "threadId": tid })))
            .await
            .map_err(|e| DebuggerError::DebuggerError(format!("stepIn failed: {}", e)))?;
        self.wait_for_stop().await
    }

    pub async fn step_out(&self, thread_id: Option<u64>) -> Result<ExecutionEvent, DebuggerError> {
        info!("DAP stepOut");
        let tid = thread_id.unwrap_or(0);
        self.transport
            .request("stepOut", Some(json!({ "threadId": tid })))
            .await
            .map_err(|e| DebuggerError::DebuggerError(format!("stepOut failed: {}", e)))?;
        self.wait_for_stop().await
    }

    pub async fn disconnect(&self) -> Result<(), DebuggerError> {
        info!("DAP disconnect");
        let _ = self.transport.request("disconnect", Some(json!({}))).await;
        Ok(())
    }

    /// Wait until a 'stopped' or 'terminated'/'exited' event arrives.
    async fn wait_for_stop(&self) -> Result<ExecutionEvent, DebuggerError> {
        let mut rx = self.event_rx.lock().await;
        loop {
            let event = rx
                .recv()
                .await
                .ok_or_else(|| DebuggerError::DebuggerError("event channel closed".into()))?;
            let evt_type = event.get("event").and_then(|v| v.as_str()).unwrap_or("");
            info!(event = evt_type, "DAP event received");

            match evt_type {
                "stopped" => {
                    let body = event.get("body").cloned().unwrap_or(json!({}));
                    let reason = body.get("reason").and_then(|r| r.as_str()).unwrap_or("unknown");
                    let thread_id = body.get("threadId").and_then(|t| t.as_i64()).unwrap_or(0);
                    {
                        let mut g = self.stop_thread_id.lock().await;
                        *g = Some(thread_id);
                    }

                    let kind = match reason {
                        "breakpoint" | "data breakpoint" => ExecutionEventKind::BreakpointHit,
                        "step" => ExecutionEventKind::StepComplete,
                        "exception" => ExecutionEventKind::PanicDetected {
                            message: "exception".into(),
                        },
                        _ => ExecutionEventKind::Paused,
                    };

                    // Get stack trace to find current source location
                    let location = self.fetch_top_frame_location(thread_id as u64).await.ok();

                    return Ok(ExecutionEvent {
                        kind,
                        thread_id: thread_id as u64,
                        location,
                    });
                }
                "terminated" => {
                    return Ok(ExecutionEvent {
                        kind: ExecutionEventKind::Terminated { exit_code: 0 },
                        thread_id: 0,
                        location: None,
                    });
                }
                "exited" => {
                    let code = event
                        .get("body")
                        .and_then(|b| b.get("exitCode"))
                        .and_then(|c| c.as_i64())
                        .unwrap_or(0) as i32;
                    return Ok(ExecutionEvent {
                        kind: ExecutionEventKind::Terminated { exit_code: code },
                        thread_id: 0,
                        location: None,
                    });
                }
                "output" => {
                    // Log output from the debuggee
                    let body = event.get("body").cloned().unwrap_or(json!({}));
                    if let Some(output) = body.get("output").and_then(|o| o.as_str()) {
                        info!("debuggee output: {}", output.trim());
                    }
                    continue;
                }
                _ => {
                    // Ignore other events and keep waiting
                    continue;
                }
            }
        }
    }

    async fn fetch_top_frame_location(&self, thread_id: u64) -> Result<SourceLocation, DebuggerError> {
        let resp = self
            .transport
            .request("stackTrace", Some(json!({ "threadId": thread_id, "startFrame": 0, "levels": 1 })))
            .await
            .map_err(|e| DebuggerError::DebuggerError(format!("stackTrace failed: {}", e)))?;

        let frame = resp
            .get("body")
            .and_then(|b| b.get("stackFrames"))
            .and_then(|f| f.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| DebuggerError::DebuggerError("no stack frames".into()))?;

        let path = frame
            .get("source")
            .and_then(|s| s.get("path"))
            .and_then(|p| p.as_str())
            .map(PathBuf::from)
            .ok_or_else(|| DebuggerError::DebuggerError("no source path in frame".into()))?;
        let line = frame
            .get("line")
            .and_then(|l| l.as_u64())
            .unwrap_or(1) as u32;

        Ok(SourceLocation {
            file: path,
            line,
            column: None,
        })
    }

    // TODO: read_locals, read_stack, evaluate_expression, list_threads
}
