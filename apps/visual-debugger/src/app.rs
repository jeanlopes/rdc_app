//! VisualDebuggerApp — eframe::App implementation.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use debug_session_view::{DebugSessionView, ToolbarAction};
use egui::Id;
use tokio::runtime::Runtime;
use tracing::instrument;
use win_debug_bridge::thread::WindowsDebugHandle;

use crate::address_bar::{AddressBar, AddressBarState};
use crate::file_tree::FileTree;
use crate::source_view::SourceView;
use crate::toolbar::{ChordState, Toolbar};

/// Main application state.
pub struct VisualDebuggerApp {
    pub view: DebugSessionView,
    pub debug_handle: Option<Arc<WindowsDebugHandle>>,
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
        debug_handle: Option<Arc<WindowsDebugHandle>>,
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
        for action in actions {
            // Record press in shared state so UI animates
            self.runtime.block_on(async {
                self.view.publish_action(action).await;
            });
            ctx.request_repaint_after(Duration::from_millis(200));

            if let Some(ref handle) = self.debug_handle {
                let handle = Arc::clone(handle);
                let view = self.view.clone();
                let action = action;
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
                    if let Some(Err(e)) = result {
                        let mut state = view.state.write().await;
                        state.set_error(format!("{:?}: {:?}", action, e));
                    }
                });
            } else {
                // No debug binary attached — show friendly message
                self.runtime.block_on(async {
                    let mut state = self.view.state.write().await;
                    state.set_error("No debug binary attached. Use --executable or select a binary.");
                });
            }
        }
    }
}

impl eframe::App for VisualDebuggerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Read latest state from shared bus
        let state = self.view.latest_blocking();
        self.error_banner = state.error_banner.clone();

        // Update source view when file changes
        self.source_view.build_lines(&state);
        if state.active_line.is_some() {
            // Scroll to active line on frame change
            self.source_view.scroll_to_active(600.0, 20.0);
        }

        // Update address bar when file changes
        if let Some(ref path) = state.active_file {
            self.address_bar.set_path(path);
        }

        // Initialise file tree root on first frame
        if self.file_tree.root.lock().unwrap().is_empty() {
            let root_dir = state.active_file.as_ref()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            self.file_tree.scan_directory(root_dir);
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            if self.debug_handle.is_none() {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::YELLOW, "⚠ No debug binary attached");
                    if ui.button("Attach binary…").clicked() {
                        // Open a file dialog via address bar edit mode
                        self.address_bar.editing = true;
                        self.address_bar.edit_text.clear();
                    }
                });
            }
            let keyboard_actions = self.toolbar.check_keyboard(ctx, &mut self.chord_state);
            let click_actions = self.toolbar.render(ui, &state);
            let mut all_actions = keyboard_actions;
            all_actions.extend(click_actions);
            self.process_actions(ctx, all_actions);
        });

        egui::SidePanel::left("file_tree").show(ctx, |ui| {
            if let Some(selected) = self.file_tree.render(ui, &state.active_file) {
                let mut new_state = state.clone();
                new_state.active_file = Some(selected);
                self.source_view.build_lines(&new_state);
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

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(ref err) = self.error_banner {
                ui.colored_label(egui::Color32::RED, err);
            }
            let view_id = Id::new("source_view");
            let mut state_mut = state.clone();
            self.source_view.render(ui, &mut state_mut, view_id);
            // Write back any breakpoint changes made during render
            if state_mut.breakpoints != state.breakpoints {
                let view = self.view.clone();
                self.runtime.spawn(async move {
                    let mut guard = view.state.write().await;
                    guard.breakpoints = state_mut.breakpoints;
                    let _ = view.notifier.send(*view.notifier.borrow() + 1);
                });
            }
        });
    }
}
