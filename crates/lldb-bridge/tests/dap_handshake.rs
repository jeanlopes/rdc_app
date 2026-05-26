//! Integration test: verify DAP handshake with codelldb works.

use std::path::PathBuf;

fn find_codelldb() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default());
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest_dir);

    let tools = workspace_root.join("tools/codelldb/extension/adapter/codelldb.exe");
    if tools.exists() {
        return tools;
    }
    let direct = workspace_root.join("codelldb/extension/adapter/codelldb.exe");
    if direct.exists() {
        return direct;
    }
    panic!("codelldb not found. Please install CodeLLDB extension or extract standalone adapter.");
}

#[tokio::test]
async fn test_dap_initialize_handshake() {
    let codelldb_path = find_codelldb();
    println!("Using codelldb: {}", codelldb_path.display());

    let (transport, event_rx) = lldb_bridge::DapTransport::spawn(codelldb_path)
        .expect("failed to spawn DapTransport");

    let client = lldb_bridge::DapClient::new(transport, event_rx);

    println!("Sending initialize request...");
    client.initialize().await.expect("initialize handshake failed");
    println!("Initialize handshake succeeded!");
}
