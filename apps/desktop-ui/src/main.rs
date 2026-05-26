mod introspection_bridge;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // Create the shared store before either the egui app or the MCP server starts.
    let store = egui_introspection::IntrospectionStore::new();

    // Spawn the in-process MCP UI server on a background thread with its own
    // single-threaded Tokio runtime.  It reads JSON-RPC from stdin and
    // dispatches to the six UI inspection tools.
    let store_for_mcp = store.clone();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
            .block_on(mcp_server::run_with_store(store_for_mcp))
            .unwrap_or_else(|e| tracing::error!("MCP UI server exited: {e}"));
    });

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "RDC Desktop UI",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(introspection_bridge::RdcApp::new_with_store(store, cc)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))
}
