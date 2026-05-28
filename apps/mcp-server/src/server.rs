use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use runtime_core::backend::DebugBackend;
use crate::handlers::session::SessionContext;
use debug_session_view::DebugSessionView;

/// Start the MCP server with the configured transport.
pub async fn run(
    backend: Arc<dyn DebugBackend>,
    _executable: PathBuf,
    _args: Vec<String>,
    transport: &str,
    port: u16,
    view: Option<DebugSessionView>,
) -> Result<()> {
    let ctx = Arc::new(SessionContext::new(backend, view));

    match transport {
        "stdio" => run_stdio(ctx).await,
        "http" => run_http(ctx, port).await,
        other => anyhow::bail!("unsupported transport: '{}' (use 'stdio' or 'http')", other),
    }
}

async fn run_stdio(ctx: Arc<SessionContext>) -> Result<()> {
    info!("MCP server listening on stdio");
    dispatch_loop(ctx).await
}

async fn run_http(ctx: Arc<SessionContext>, port: u16) -> Result<()> {
    info!(port, "MCP server listening on HTTP/SSE");
    dispatch_loop(ctx).await
}

/// Minimal JSON-RPC 2.0 dispatch over stdin/stdout.
async fn dispatch_loop(ctx: Arc<SessionContext>) -> Result<()> {
    use std::io::{BufRead, Write};
    use serde_json::{json, Value};

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("parse error: {}", e) }
                });
                writeln!(stdout.lock(), "{}", response)?;
                continue;
            }
        };

        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request["method"].as_str().unwrap_or("").to_string();
        let params = request.get("params").cloned().unwrap_or(Value::Null);

        let response = dispatch(&ctx, &method, params).await
            .map(|result| json!({ "jsonrpc": "2.0", "id": id, "result": result }))
            .unwrap_or_else(|e| {
                let code = protocol::error::to_mcp_error_code(&e);
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": code, "message": e.to_string() }
                })
            });

        writeln!(stdout.lock(), "{}", response)?;
        stdout.lock().flush()?;
    }

    Ok(())
}

async fn dispatch(
    ctx: &SessionContext,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, runtime_core::error::DebuggerError> {
    use crate::handlers::{breakpoints as bp, execution as exec, inspection as insp, session};
    use runtime_core::error::DebuggerError;

    let backend = ctx.backend.as_ref();

    match method {
        "launch_process" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            let output = session::handle_launch_process(ctx, input).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "get_session_state" => {
            let output = session::handle_get_session_state(ctx).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "set_breakpoint" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            let output = bp::handle_set_breakpoint(backend, input, &ctx.view).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "remove_breakpoint" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            bp::handle_remove_breakpoint(backend, input, &ctx.view).await?;
            Ok(serde_json::json!({}))
        }
        "list_breakpoints" => {
            let output = bp::handle_list_breakpoints(backend).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "continue_execution" => {
            let output = exec::handle_continue_execution(backend, &ctx.view).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "pause_execution" => {
            let output = exec::handle_pause_execution(backend, &ctx.view).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "step_over" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            let output = exec::handle_step_over(backend, input, &ctx.view).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "step_into" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            let output = exec::handle_step_into(backend, input, &ctx.view).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "step_out" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            let output = exec::handle_step_out(backend, input, &ctx.view).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "read_locals" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            let output = insp::handle_read_locals(backend, input).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "read_stack" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            let output = insp::handle_read_stack(backend, input).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "evaluate_expression" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            let output = insp::handle_evaluate_expression(backend, input).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "list_threads" => {
            let output = insp::handle_list_threads(backend).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        unknown => Err(DebuggerError::ProtocolError(format!("unknown method: {}", unknown))),
    }
}
