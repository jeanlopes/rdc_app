use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

use runtime_core::backend::DebugBackend;

mod server;
mod handlers;

#[derive(clap::ValueEnum, Clone, Default, Debug)]
enum BackendChoice {
    #[default]
    LldbNative,
    WinDebugBridge,
}

#[derive(Parser)]
#[command(name = "mcp-server", about = "RDC MCP server — AI gateway to the LLDB debugger")]
struct Cli {
    /// Path to the executable to debug
    #[arg(long)]
    executable: PathBuf,

    /// Arguments to pass to the executable
    #[arg(long, num_args = 0.., value_delimiter = ' ')]
    args: Vec<String>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Transport (stdio | http)
    #[arg(long, default_value = "stdio")]
    transport: String,

    /// HTTP port (only used with --transport http)
    #[arg(long, default_value = "3000")]
    port: u16,

    /// Debug backend to use
    #[arg(long, default_value = "lldb-native")]
    backend: BackendChoice,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(&cli.log_level)),
        )
        .init();

    info!(
        executable = %cli.executable.display(),
        transport = %cli.transport,
        "RDC mcp-server starting"
    );

    let backend: Arc<dyn DebugBackend> = match cli.backend {
        BackendChoice::LldbNative => {
            info!("using lldb-native backend");
            Arc::new(lldb_native::LldbNativeHandle::spawn()
                .map_err(|e| anyhow::anyhow!("Failed to initialise lldb-native: {}", e))?)
        }
        BackendChoice::WinDebugBridge => {
            info!("using win-debug-bridge backend");
            Arc::new(win_debug_bridge::thread::WindowsDebugHandle::spawn()
                .map_err(|e| anyhow::anyhow!("Failed to initialise win-debug-bridge: {}", e))?)
        }
    };

    info!("debug backend ready");

    server::run(backend, cli.executable, cli.args, &cli.transport, cli.port, None).await
}
