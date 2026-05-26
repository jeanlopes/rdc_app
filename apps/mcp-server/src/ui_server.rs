//! In-process MCP server for UI inspection tools.
//!
//! Spawned by `apps/desktop-ui` as a background Tokio task.  Reads JSON-RPC
//! requests from stdin and dispatches them to the six UI inspection handlers
//! in [`crate::handlers::ui_inspection`].

use anyhow::Result;
use egui_introspection::IntrospectionStore;
use tracing::info;

/// Run the UI inspection MCP server with a shared [`IntrospectionStore`].
///
/// Blocks the calling async context reading newline-delimited JSON-RPC 2.0
/// messages from stdin and writing responses to stdout.  Intended to be
/// called from a dedicated background thread via
/// `tokio::runtime::Runtime::block_on`.
pub async fn run_with_store(store: IntrospectionStore) -> Result<()> {
    info!("UI inspection MCP server ready (stdio transport)");
    dispatch_loop(store).await
}

async fn dispatch_loop(store: IntrospectionStore) -> Result<()> {
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
                    "error": { "code": -32700, "message": format!("parse error: {e}") }
                });
                writeln!(stdout.lock(), "{}", response)?;
                stdout.lock().flush()?;
                continue;
            }
        };

        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request["method"].as_str().unwrap_or("").to_string();
        let params = request.get("params").cloned().unwrap_or(Value::Null);

        let result = dispatch_ui(&store, &method, &params);
        let response = json!({ "jsonrpc": "2.0", "id": id, "result": result });
        writeln!(stdout.lock(), "{}", response)?;
        stdout.lock().flush()?;
    }

    Ok(())
}

fn dispatch_ui(
    store: &IntrospectionStore,
    method: &str,
    params: &serde_json::Value,
) -> serde_json::Value {
    use crate::handlers::ui_inspection;
    use serde_json::json;

    match method {
        "ui_snapshot" => ui_inspection::ui_snapshot(store),
        "ui_find_widget" => {
            let label = params.get("label").and_then(|v| v.as_str()).unwrap_or("");
            let kind = params.get("kind").and_then(|v| v.as_str());
            ui_inspection::ui_find_widget(store, label, kind)
        }
        "ui_widget_info" => {
            let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
            ui_inspection::ui_widget_info(store, id)
        }
        "ui_children" => {
            let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
            ui_inspection::ui_children(store, id)
        }
        "ui_clipped_widgets" => ui_inspection::ui_clipped_widgets(store),
        "ui_snapshot_diff" => ui_inspection::ui_snapshot_diff(store),
        unknown => json!({ "error": "unknown_method", "method": unknown }),
    }
}
