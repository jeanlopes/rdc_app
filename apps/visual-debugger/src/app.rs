//! VisualDebuggerApp — eframe::App implementation.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use debug_session_view::{DebugSessionState, DebugSessionView, ToolbarAction};
use rfd::FileDialog;
use egui::Id;
use tokio::runtime::Runtime;
use tracing::instrument;
use lldb_bridge::LldbDebugHandle;
use protocol::tools::execution::{ExecutionEvent, ExecutionEventKind};
use runtime_core::breakpoint::BreakpointKind;
use runtime_core::session::{DebugTarget, SessionState};
use serde_json::Value as JsonValue;

use crate::address_bar::{AddressBar, AddressBarState};
use crate::file_tree::FileTree;
use crate::source_view::SourceView;
use crate::toolbar::{ChordState, Toolbar};

/// Main application state.
pub struct VisualDebuggerApp {
    pub view: DebugSessionView,
    pub debug_handle: Arc<std::sync::Mutex<Option<Arc<LldbDebugHandle>>>>,
    pub source_view: SourceView,
    pub file_tree: FileTree,
    pub address_bar: AddressBarState,
    pub chord_state: ChordState,
    pub toolbar: Toolbar,
    pub error_banner: Option<String>,
    pub runtime: Arc<Runtime>,
}

impl VisualDebuggerApp {
    pub fn new(
        view: DebugSessionView,
        debug_handle: Arc<std::sync::Mutex<Option<Arc<LldbDebugHandle>>>>,
        runtime: Arc<Runtime>,
    ) -> Self {
        Self {
            view,
            debug_handle,
            source_view: SourceView::default(),
            file_tree: FileTree::default(),
            address_bar: AddressBarState::default(),
            chord_state: ChordState::default(),
            toolbar: Toolbar::new(),
            error_banner: None,
            runtime,
        }
    }

    /// Process toolbar actions and send debug commands.
    #[instrument(skip(self, ctx))]
    fn process_actions(&mut self, ctx: &egui::Context, actions: Vec<ToolbarAction>) {
        tracing::info!(count = actions.len(), "process_actions called");
        for action in actions {
            tracing::info!(?action, "processing action");
            // Record press in shared state so UI animates
            self.runtime.block_on(async {
                self.view.publish_action(action).await;
            });
            ctx.request_repaint_after(Duration::from_millis(200));

            // Helper to log into the terminal buffer
            let log = |msg: String| {
                tracing::info!(%msg, "ui-log");
                let view = self.view.clone();
                self.runtime.spawn(async move {
                    let mut state = view.state.write().await;
                    state.log_terminal(msg);
                    let _ = view.notifier.send(*view.notifier.borrow() + 1);
                });
            };

            let has_handle = self.debug_handle.lock().unwrap().is_some();
            tracing::info!(?action, has_handle, "action dispatch");

            if let Some(ref handle) = *self.debug_handle.lock().unwrap() {
                let handle = Arc::clone(handle);
                let view = self.view.clone();
                let action = action;
                log(format!("[action] {:?}", action));
                self.runtime.spawn(async move {
                    let result = match action {
                        ToolbarAction::Continue => Some(handle.continue_execution().await),
                        ToolbarAction::StepOver => Some(handle.step_over(None).await),
                        ToolbarAction::StepInto => Some(handle.step_into(None).await),
                        ToolbarAction::StepOut => Some(handle.step_out(None).await),
                        ToolbarAction::BreakAll => Some(handle.pause_execution().await),
                        ToolbarAction::StopDebugging => {
                            // TODO: terminate session
                            None
                        }
                        ToolbarAction::Restart => {
                            // TODO: restart session
                            None
                        }
                        _ => None,
                    };
                    match result {
                        Some(Ok(event)) => {
                            let mut state = view.state.write().await;
                            apply_execution_event(&mut state, &event);
                            state.log_terminal(format!("[event] {:?}", event));
                            let _ = view.notifier.send(*view.notifier.borrow() + 1);
                        }
                        Some(Err(e)) => {
                            let mut state = view.state.write().await;
                            state.set_error(format!("{:?}: {:?}", action, e));
                            state.log_terminal(format!("[error] {:?}: {:?}", action, e));
                            let _ = view.notifier.send(*view.notifier.borrow() + 1);
                        }
                        None => {}
                    }
                });
            } else if action == ToolbarAction::Continue {
                // No handle yet — auto-build from the active .rs file
                let active_file = self.view.latest_blocking().active_file;
                if let Some(ref rs_file) = active_file {
                    if let Some(cargo_toml) = find_cargo_toml(rs_file) {
                        log(format!(
                            "[start] Rust project detected: {}",
                            cargo_toml.display()
                        ));
                        let view = self.view.clone();
                        let debug_handle = Arc::clone(&self.debug_handle);
                        let rs_file = rs_file.clone();
                        self.runtime.spawn(async move {
                            tracing::info!(cargo_toml = %cargo_toml.display(), rs_file = %rs_file.display(), "AUTO_BUILD task started");
                            match resolve_rust_binary(&cargo_toml, &rs_file).await {
                                Some((bin_name, exe_path)) => {
                                    tracing::info!(%bin_name, exe_path = %exe_path.display(), "resolved binary from cargo metadata");
                                    {
                                        let mut state = view.state.write().await;
                                        state.log_terminal(format!(
                                            "[start] Building binary '{}'...",
                                            bin_name
                                        ));
                                        let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                    }

                                    tracing::info!("running cargo build...");
                                    let output = tokio::process::Command::new("cargo")
                                        .arg("build")
                                        .arg("--bin")
                                        .arg(&bin_name)
                                        .current_dir(cargo_toml.parent().unwrap())
                                        .output()
                                        .await;
                                    tracing::info!(?output, "cargo build finished");

                                    match output {
                                        Ok(out) if out.status.success() => {
                                            {
                                                let mut state = view.state.write().await;
                                                state.log_terminal(
                                                    "[start] Build succeeded".to_string(),
                                                );
                                                let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                            }

                                            if !exe_path.exists() {
                                                let mut state = view.state.write().await;
                                                state.log_terminal(format!(
                                                    "[error] Expected exe not found: {}",
                                                    exe_path.display()
                                                ));
                                                let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                                return;
                                            }

                                            match LldbDebugHandle::spawn() {
                                                Ok(handle) => {
                                                    let handle = Arc::new(handle);
                                                    *debug_handle.lock().unwrap() =
                                                        Some(Arc::clone(&handle));
                                                    let target = DebugTarget {
                                                        executable: exe_path.clone(),
                                                        args: vec![],
                                                        env: Default::default(),
                                                        working_dir: None,
                                                    };
                                                    match handle.launch_process(target).await {
                                                        Ok((pid, _session_state)) => {
                                                            {
                                                                let mut state = view.state.write().await;
                                                                state.log_terminal(format!(
                                                                    "[start] Launched PID {} — PAUSED at entry point",
                                                                    pid
                                                                ));
                                                                let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                                            }

                                                            // Sync all existing UI breakpoints before continuing
                                                            let ui_bps = {
                                                                let state = view.state.read().await;
                                                                state.breakpoints.clone()
                                                            };
                                                            tracing::info!(count = ui_bps.len(), "syncing existing UI breakpoints");
                                                            for bp in ui_bps {
                                                                let kind = BreakpointKind::SourceLine {
                                                                    file: bp.file.clone(),
                                                                    line: bp.line,
                                                                };
                                                                match handle.set_breakpoint(kind, None).await {
                                                                    Ok(bk) => {
                                                                        let mut guard = view.state.write().await;
                                                                        if let Some(entry) = guard.breakpoints.iter_mut().find(|b| b.file == bp.file && b.line == bp.line) {
                                                                            entry.resolved = true;
                                                                            entry.backend_id = Some(bk.id);
                                                                        }
                                                                        guard.log_terminal(format!(
                                                                            "[bp] Auto-set {}:{} @ 0x{:x} (id={})",
                                                                            bp.file.display(),
                                                                            bp.line,
                                                                            bk.locations.first().map(|l| l.address).unwrap_or(0),
                                                                            bk.id
                                                                        ));
                                                                        let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                                                    }
                                                                    Err(e) => {
                                                                        let mut guard = view.state.write().await;
                                                                        guard.log_terminal(format!(
                                                                            "[bp] Failed to auto-set {}:{} — {:?}",
                                                                            bp.file.display(),
                                                                            bp.line,
                                                                            e
                                                                        ));
                                                                        let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                                                    }
                                                                }
                                                            }

                                                            // Now continue so the process runs until it hits a breakpoint
                                                            tracing::info!("auto-continuing after setting breakpoints");
                                                            match handle.continue_execution().await {
                                                                Ok(event) => {
                                                                    let mut state = view.state.write().await;
                                                                    apply_execution_event(&mut state, &event);
                                                                    state.log_terminal(format!("[event] {:?}", event));
                                                                    let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                                                }
                                                                Err(e) => {
                                                                    let mut state = view.state.write().await;
                                                                    state.set_error(format!("Continue failed: {:?}", e));
                                                                    state.log_terminal(format!("[error] Continue failed: {:?}", e));
                                                                    let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                                                }
                                                            }
                                                        }
                                                        Err(e) => {
                                                            let mut state = view.state.write().await;
                                                            state.set_error(format!(
                                                                "Failed to launch: {:?}",
                                                                e
                                                            ));
                                                            state.log_terminal(format!(
                                                                "[error] Failed to launch: {:?}",
                                                                e
                                                            ));
                                                            let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    let mut state = view.state.write().await;
                                                    state.log_terminal(format!(
                                                        "[error] Failed to spawn debug handle: {:?}",
                                                        e
                                                    ));
                                                    let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                                }
                                            }
                                        }
                                        Ok(out) => {
                                            let stderr = String::from_utf8_lossy(&out.stderr);
                                            let mut state = view.state.write().await;
                                            state.log_terminal(format!(
                                                "[error] cargo build failed:\n{}",
                                                stderr
                                            ));
                                            let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                        }
                                        Err(e) => {
                                            let mut state = view.state.write().await;
                                            state.log_terminal(format!(
                                                "[error] Failed to run cargo build: {}",
                                                e
                                            ));
                                            let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                        }
                                    }
                                }
                                None => {
                                    let mut state = view.state.write().await;
                                    state.log_terminal(
                                        "[error] Could not determine binary name from Cargo.toml"
                                            .to_string(),
                                    );
                                    let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                }
                            }
                        });
                    } else {
                        log("[error] No Cargo.toml found for the selected .rs file".to_string());
                    }
                } else {
                    log("[error] Select a .rs file before starting".to_string());
                }
            } else {
                // No debug binary attached — show friendly message
                log("[error] No debug binary attached. Use --executable or select a binary.".to_string());
                self.runtime.block_on(async {
                    let mut state = self.view.state.write().await;
                    state.set_error("No debug binary attached. Use --executable or select a binary.");
                });
            }
        }
    }
}

/// Walk up the directory tree from `start` looking for Cargo.toml.
fn find_cargo_toml(start: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut dir = start.parent()?;
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => return None,
        }
    }
}

/// Query `cargo metadata` and return the binary name + expected exe path.
async fn resolve_rust_binary(cargo_toml: &std::path::Path, _rs_file: &std::path::Path) -> Option<(String, std::path::PathBuf)> {
    let output = tokio::process::Command::new("cargo")
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--manifest-path")
        .arg(cargo_toml)
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let meta: JsonValue = serde_json::from_slice(&output.stdout).ok()?;
    let packages = meta.get("packages")?.as_array()?;
    for pkg in packages {
        let manifest = pkg.get("manifest_path")?.as_str()?;
        if std::path::Path::new(manifest) != cargo_toml {
            continue;
        }
        let targets = pkg.get("targets")?.as_array()?;
        // Collect all bin targets
        let mut bin_targets = Vec::new();
        for t in targets.iter() {
            if let Some(kind_arr) = t.get("kind").and_then(|k| k.as_array()) {
                if kind_arr.iter().any(|k| k.as_str() == Some("bin")) {
                    bin_targets.push(t);
                }
            }
        }
        let chosen = if bin_targets.len() == 1 {
            bin_targets.into_iter().next()?
        } else {
            bin_targets
                .iter()
                .find(|t| {
                    t.get("src_path")
                        .and_then(|s| s.as_str())
                        .map(|s| s.ends_with("main.rs"))
                        .unwrap_or(false)
                })
                .copied()
                .or_else(|| {
                    targets.iter().find(|t| {
                        t.get("kind")
                            .and_then(|k| k.as_array())
                            .map(|arr| arr.iter().any(|k| k.as_str() == Some("bin")))
                            .unwrap_or(false)
                    })
                })?
        };
        let name = chosen.get("name")?.as_str()?.to_string();
        let target_dir = meta.get("target_directory")?.as_str()?;
        let exe = std::path::PathBuf::from(target_dir)
            .join("debug")
            .join(format!("{}.exe", name));
        return Some((name, exe));
    }
    None
}

/// Map a runtime session state to the UI-facing state.
fn map_session_state(state: &SessionState) -> DebugSessionState {
    match state {
        SessionState::Idle | SessionState::Launching => DebugSessionState::Idle,
        SessionState::Running | SessionState::Stepping => DebugSessionState::Running,
        SessionState::Paused(_) => DebugSessionState::Paused,
        SessionState::Terminated(_) => DebugSessionState::Terminated,
        SessionState::Error(_) => DebugSessionState::Idle,
    }
}

/// Update UI state from an execution event returned by the debug thread.
fn apply_execution_event(state: &mut debug_session_view::DebugUIState, event: &ExecutionEvent) {
    match event.kind {
        ExecutionEventKind::BreakpointHit | ExecutionEventKind::StepComplete | ExecutionEventKind::Paused => {
            state.session_state = DebugSessionState::Paused;
            if let Some(ref loc) = event.location {
                state.active_file = Some(loc.file.clone());
                state.active_line = Some(loc.line);
            }
        }
        ExecutionEventKind::Terminated { exit_code } => {
            state.session_state = DebugSessionState::Terminated;
            state.active_line = None;
            state.log_terminal(format!("[event] Process terminated with exit code {}", exit_code));
        }
        ExecutionEventKind::PanicDetected { ref message } => {
            state.session_state = DebugSessionState::Paused;
            state.log_terminal(format!("[event] Panic detected: {}", message));
        }
    }
}

impl eframe::App for VisualDebuggerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Read latest state from shared bus
        let state = self.view.latest_blocking();
        tracing::info!(active_file = ?state.active_file, active_line = ?state.active_line, session_state = ?state.session_state, "UPDATE frame");
        self.error_banner = state.error_banner.clone();

        // Update source view when file changes
        self.source_view.build_lines(&state);
        if let Some(line) = state.active_line {
            tracing::info!(line, "scrolling to active line");
            self.source_view.scroll_to_active(600.0, 20.0);
        }

        // Update address bar when file changes
        if let Some(ref path) = state.active_file {
            tracing::info!(path = %path.display(), "active file changed");
            self.address_bar.set_path(path);
        }

        // Initialise file tree root on first frame
        if self.file_tree.root_path.is_none() {
            let root_dir = std::env::current_dir().ok()
                .unwrap_or_else(|| PathBuf::from("."));
            self.file_tree.change_root(root_dir);
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            let has_handle = self.debug_handle.lock().unwrap().is_some();
            tracing::info!(has_handle, "toolbar render");
            if !has_handle {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::YELLOW, "⚠ No debug binary attached");
                    if ui.button("Attach binary…").clicked() {
                        self.address_bar.editing = true;
                        self.address_bar.edit_text.clear();
                    }
                });
            }
            let keyboard_actions = self.toolbar.check_keyboard(ctx, &mut self.chord_state);
            let click_actions = self.toolbar.render(ui, &state);
            let mut all_actions = keyboard_actions;
            all_actions.extend(click_actions);
            tracing::info!(actions = ?all_actions, "dispatching actions");
            self.process_actions(ctx, all_actions);
        });

        egui::SidePanel::left("file_tree").show(ctx, |ui| {
            if let Some(selected) = self.file_tree.render(ui, &state.active_file) {
                // Build source immediately for the current frame
                let mut frame_state = state.clone();
                frame_state.active_file = Some(selected.clone());
                self.source_view.build_lines(&frame_state);
                // Update address bar immediately
                self.address_bar.set_path(&selected);
                // Persist selection so subsequent frames don't revert
                let view = self.view.clone();
                self.runtime.block_on(async {
                    let mut guard = view.state.write().await;
                    guard.active_file = Some(selected);
                    let _ = view.notifier.send(*view.notifier.borrow() + 1);
                });
            }
        });

        egui::TopBottomPanel::top("address_bar").show(ctx, |ui| {
            if self.address_bar.editing {
                if let Some(path) = AddressBar::render_edit(ui, &mut self.address_bar) {
                    // Load file in source viewer
                    let mut new_state = self.view.latest_blocking();
                    new_state.active_file = Some(path);
                    self.source_view.build_lines(&new_state);
                }
            } else {
                AddressBar::render_display(ui, &mut self.address_bar);
            }
        });

        egui::TopBottomPanel::bottom("terminal")
            .min_height(120.0)
            .max_height(250.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Terminal / Logs");
                    if ui.button("Clear").clicked() {
                        let view = self.view.clone();
                        self.runtime.spawn(async move {
                            let mut state = view.state.write().await;
                            state.terminal_logs.clear();
                            let _ = view.notifier.send(*view.notifier.borrow() + 1);
                        });
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &state.terminal_logs {
                            ui.monospace(line);
                        }
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(ref err) = self.error_banner {
                ui.colored_label(egui::Color32::RED, err);
            }
            let view_id = Id::new("source_view");
            let mut state_mut = state.clone();
            self.source_view.render(ui, &mut state_mut, view_id);
            // Write back any breakpoint changes made during render
            if state_mut.breakpoints != state.breakpoints {
                tracing::info!(old_count = state.breakpoints.len(), new_count = state_mut.breakpoints.len(), "BREAKPOINT CHANGE detected");
                // Determine added / removed by (file, line) key
                let old_set: std::collections::HashSet<_> =
                    state.breakpoints.iter().map(|b| (b.file.clone(), b.line)).collect();
                let new_set: std::collections::HashSet<_> =
                    state_mut.breakpoints.iter().map(|b| (b.file.clone(), b.line)).collect();
                let added: Vec<_> = state_mut
                    .breakpoints
                    .iter()
                    .filter(|b| !old_set.contains(&(b.file.clone(), b.line)))
                    .cloned()
                    .collect();
                let removed: Vec<_> = state
                    .breakpoints
                    .iter()
                    .filter(|b| !new_set.contains(&(b.file.clone(), b.line)))
                    .cloned()
                    .collect();
                tracing::info!(added = added.len(), removed = removed.len(), "breakpoint delta");
                for bp in &added {
                    tracing::info!(file = %bp.file.display(), line = bp.line, "breakpoint ADDED");
                }
                for bp in &removed {
                    tracing::info!(file = %bp.file.display(), line = bp.line, backend_id = ?bp.backend_id, "breakpoint REMOVED");
                }

                // Persist UI state immediately so the red dot appears/disappears
                let view = self.view.clone();
                let state_for_write = state_mut.clone();
                self.runtime.spawn(async move {
                    let mut guard = view.state.write().await;
                    guard.breakpoints = state_for_write.breakpoints;
                    let _ = view.notifier.send(*view.notifier.borrow() + 1);
                });

                // Sync with backend if we have a debug handle
                let has_handle = self.debug_handle.lock().unwrap().is_some();
                tracing::info!(has_handle, "about to sync breakpoints with backend");
                if let Some(ref handle) = *self.debug_handle.lock().unwrap() {
                    let handle = Arc::clone(handle);
                    let view = self.view.clone();
                    self.runtime.spawn(async move {
                        for bp in added {
                            let kind = BreakpointKind::SourceLine {
                                file: bp.file.clone(),
                                line: bp.line,
                            };
                            match handle.set_breakpoint(kind, None).await {
                                Ok(bk) => {
                                    let mut guard = view.state.write().await;
                                    if let Some(entry) = guard
                                        .breakpoints
                                        .iter_mut()
                                        .find(|b| b.file == bp.file && b.line == bp.line)
                                    {
                                        entry.resolved = true;
                                        entry.backend_id = Some(bk.id);
                                    }
                                    guard.log_terminal(format!(
                                        "[bp] Resolved {}:{} @ 0x{:x} (id={})",
                                        bp.file.display(),
                                        bp.line,
                                        bk.locations.first().map(|l| l.address).unwrap_or(0),
                                        bk.id
                                    ));
                                    let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                }
                                Err(e) => {
                                    let mut guard = view.state.write().await;
                                    guard.log_terminal(format!(
                                        "[bp] Failed to resolve {}:{} — {:?}",
                                        bp.file.display(),
                                        bp.line,
                                        e
                                    ));
                                    let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                }
                            }
                        }
                        for bp in removed {
                            if let Some(id) = bp.backend_id {
                                if let Err(e) = handle.remove_breakpoint(id).await {
                                    let mut guard = view.state.write().await;
                                    guard.log_terminal(format!(
                                        "[bp] Failed to remove breakpoint id={} — {:?}",
                                        id, e
                                    ));
                                    let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                } else {
                                    let mut guard = view.state.write().await;
                                    guard.log_terminal(format!(
                                        "[bp] Removed breakpoint id={} ({}:{})",
                                        id,
                                        bp.file.display(),
                                        bp.line
                                    ));
                                    let _ = view.notifier.send(*view.notifier.borrow() + 1);
                                }
                            }
                        }
                    });
                }
            }
        });

        // Open native folder picker when requested — blocks until dialog is dismissed
        if self.file_tree.open_folder_requested {
            self.file_tree.open_folder_requested = false;
            if let Some(path) = FileDialog::new()
                .set_title("Selecionar pasta")
                .pick_folder()
            {
                self.file_tree.change_root(path);
                ctx.request_repaint();
            }
        }
    }
}
