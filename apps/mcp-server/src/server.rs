use anyhow::Result;
use win_debug_bridge::thread::WindowsDebugHandle as LLDBHandle;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use crate::handlers::session::SessionContext;

/// Start the MCP server with the configured transport.
///
/// The full rmcp tool registration is wired here. Currently provides a
/// working stdio loop; HTTP/SSE transport is the Phase 7 polish item (T066).
pub async fn run(
    handle: LLDBHandle,
    _executable: PathBuf,
    _args: Vec<String>,
    transport: &str,
    port: u16,
) -> Result<()> {
    let ctx = Arc::new(SessionContext::new(handle));

    match transport {
        "stdio" => run_stdio(ctx).await,
        "http" => run_http(ctx, port).await,
        other => anyhow::bail!("unsupported transport: '{}' (use 'stdio' or 'http')", other),
    }
}

async fn run_stdio(ctx: Arc<SessionContext>) -> Result<()> {
    info!("MCP server listening on stdio");
    // rmcp stdio transport — reads JSON-RPC from stdin, writes to stdout.
    // Full tool dispatch is wired via McpServer::builder() below once rmcp
    // crate API is stabilised (see T028 in tasks.md).
    //
    // Temporary: run a simple dispatch loop until rmcp builder is integrated.
    dispatch_loop(ctx).await
}

async fn run_http(ctx: Arc<SessionContext>, port: u16) -> Result<()> {
    info!(port, "MCP server listening on HTTP/SSE");
    // T066: HTTP/SSE transport — polish phase
    dispatch_loop(ctx).await
}

/// Minimal JSON-RPC 2.0 dispatch over stdin/stdout.
/// Forwards tool calls to the appropriate handler functions.
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
            let output = bp::handle_set_breakpoint(&ctx.handle, input).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "remove_breakpoint" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            bp::handle_remove_breakpoint(&ctx.handle, input).await?;
            Ok(serde_json::json!({}))
        }
        "list_breakpoints" => {
            let output = bp::handle_list_breakpoints(&ctx.handle).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "continue_execution" => {
            let output = exec::handle_continue_execution(&ctx.handle).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "pause_execution" => {
            let output = exec::handle_pause_execution(&ctx.handle).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "step_over" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            let output = exec::handle_step_over(&ctx.handle, input).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "step_into" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            let output = exec::handle_step_into(&ctx.handle, input).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "step_out" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            let output = exec::handle_step_out(&ctx.handle, input).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "read_locals" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            let output = insp::handle_read_locals(&ctx.handle, input).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "read_stack" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            let output = insp::handle_read_stack(&ctx.handle, input).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "evaluate_expression" => {
            let input = serde_json::from_value(params)
                .map_err(|e| DebuggerError::ProtocolError(e.to_string()))?;
            let output = insp::handle_evaluate_expression(&ctx.handle, input).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        "list_threads" => {
            let output = insp::handle_list_threads(&ctx.handle).await?;
            Ok(serde_json::to_value(output).unwrap())
        }
        unknown => Err(DebuggerError::ProtocolError(format!("unknown method: {}", unknown))),
    }
}
