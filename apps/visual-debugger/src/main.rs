//! Visual Debugger — egui desktop application for manual debugging and AI observation.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use debug_session_view::DebugSessionView;
use tracing::info;

mod app;
mod address_bar;
mod file_tree;
mod session_config;
mod source_view;
mod syntax;
mod toolbar;

use app::VisualDebuggerApp;

/// Visual Debugger UI
#[derive(Parser, Debug)]
#[command(name = "visual-debugger")]
struct Args {
    /// Path to the executable to debug (optional).
    #[arg(long)]
    executable: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    // Show our logs at INFO; silence noisy GPU/render crates.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(
                "info,wgpu_core=off,wgpu_hal=off,wgpu=off,naga=off,eframe=off,egui=off",
            )
        });
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let args = Args::parse();
    let executable = args.executable;

    if let Some(ref exec) = executable {
        info!("Starting visual-debugger with executable: {:?}", exec);
    } else {
        info!("Starting visual-debugger — no executable; use file tree + Start to debug");
    }

    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime"),
    );

    let view = DebugSessionView::new();

    if let Some(ref exec) = executable {
        if !exec.exists() {
            let msg = format!("Binary not found: {:?}", exec);
            runtime.block_on(async {
                let mut state = view.state.write().await;
                state.set_error(&msg);
                state.session_state = debug_session_view::DebugSessionState::Idle;
            });
        }
    }

    let debug_handle: Arc<std::sync::Mutex<Option<Arc<dyn runtime_core::backend::DebugBackend>>>> =
        Arc::new(std::sync::Mutex::new(
            if executable.is_some() {
                lldb_native::LldbNativeHandle::spawn()
                    .ok()
                    .map(|h| -> Arc<dyn runtime_core::backend::DebugBackend> { Arc::new(h) })
            } else {
                None
            }
        ));

    {
        let guard = debug_handle.lock().unwrap();
        if let Some(ref handle) = *guard {
            if let Some(ref exec) = executable {
                let handle = Arc::clone(handle);
                let exec = exec.clone();
                runtime.spawn(async move {
                    let target = runtime_core::session::DebugTarget {
                        executable: exec,
                        args: vec![],
                        env: Default::default(),
                        working_dir: None,
                    };
                    if let Err(e) = handle.launch_process(target).await {
                        tracing::error!("Failed to launch process: {:?}", e);
                    }
                });
            }
        }
    }

    // Spawn in-process MCP debug server sharing the same DebugSessionView
    {
        let guard = debug_handle.lock().unwrap();
        if let Some(ref handle) = *guard {
            if let Some(ref exec) = executable {
                let mcp_handle = Arc::clone(handle);
                let mcp_view = view.clone();
                let mcp_executable = exec.clone();
                std::thread::spawn(move || {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("mcp tokio runtime")
                        .block_on(mcp_server::run_with_view(
                            mcp_handle,
                            mcp_executable,
                            vec![],
                            mcp_view,
                        ))
                        .unwrap_or_else(|e| tracing::error!("MCP server exited: {e}"));
                });
            }
        }
    }

    let native_options = eframe::NativeOptions::default();
    let app_view = view.clone();
    let app_runtime = Arc::clone(&runtime);
    eframe::run_native(
        "RDC Visual Debugger",
        native_options,
        Box::new(move |cc| {
            let mut fonts = egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(VisualDebuggerApp::new(
                app_view,
                debug_handle,
                app_runtime,
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))
}
