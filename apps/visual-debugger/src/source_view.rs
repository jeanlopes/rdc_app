//! Virtual-scroll source viewer with gutter, breakpoints, and active-line highlight.

use std::ops::Range;
use std::path::PathBuf;

use debug_session_view::{BreakpointEntry, DebugUIState};

use crate::syntax::{tokenize_line, TokenClass};

/// A single rendered line in the source viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
/// A single rendered line in the source viewer.
pub struct SourceLine {
    /// 1-based line number.
    pub number: u32,
    /// Syntax-highlighted spans.
    pub tokens: Vec<(TokenClass, String)>,
    /// True if this is the current execution line.
    pub is_active: bool,
    /// True if a BreakpointEntry exists at this line.
    pub has_breakpoint: bool,
    /// False → dimmed dot in gutter.
    pub breakpoint_resolved: bool,
}

/// Manages source file content and rendering state.
#[derive(Debug, Default)]
/// Manages source file content and rendering state.
pub struct SourceView {
    /// Currently loaded file path.
    pub file_path: Option<PathBuf>,
    /// Raw text lines.
    pub lines: Vec<String>,
    /// Lines enriched with breakpoint / active state.
    pub built_lines: Vec<SourceLine>,
    /// Current scroll offset in pixels.
    pub scroll_offset: f32,
    /// Active execution line (1-based).
    pub active_line: Option<u32>,
}

impl SourceView {
    /// Load file content and build source lines from state.
    pub fn build_lines(&mut self, state: &DebugUIState) {
        if state.active_file != self.file_path {
            self.file_path = state.active_file.clone();
            self.lines = if let Some(ref path) = self.file_path {
                std::fs::read_to_string(path)
                    .unwrap_or_default()
                    .lines()
                    .map(String::from)
                    .collect()
            } else {
                Vec::new()
            };
        }

        let active = state.active_line;
        self.active_line = active;

        self.built_lines = self
            .lines
            .iter()
            .enumerate()
            .map(|(i, text)| {
                let number = (i + 1) as u32;
                let is_active = active == Some(number);
                let bp = state
                    .breakpoints
                    .iter()
                    .find(|b| Some(&b.file) == self.file_path.as_ref() && b.line == number);
                let has_breakpoint = bp.is_some();
                let breakpoint_resolved = bp.map(|b| b.resolved).unwrap_or(false);

                let tokens = tokenize_line(text)
                    .into_iter()
                    .map(|(class, s)| (class, s.to_string()))
                    .collect();

                SourceLine {
                    number,
                    tokens,
                    is_active,
                    has_breakpoint,
                    breakpoint_resolved,
                }
            })
            .collect();
    }

    /// Returns the visible line range given scroll offset and panel height.
    pub fn visible_range(&self, scroll_offset: f32, panel_height: f32, row_height: f32) -> Range<u32> {
        let start = ((scroll_offset / row_height).floor() as u32).saturating_sub(1);
        let count = ((panel_height / row_height).ceil() as u32) + 2;
        let end = (start + count).min(self.built_lines.len() as u32);
        start..end
    }

    /// Check whether the active line is inside the visible range.
    pub fn needs_scroll(&self, active_line: u32, range: &Range<u32>) -> bool {
        active_line < range.start || active_line >= range.end
    }

    /// Compute scroll offset to bring active line into viewport center.
    pub fn scroll_to_active(&mut self, panel_height: f32, row_height: f32) {
        if let Some(active) = self.active_line {
            let target = (active.saturating_sub(1)) as f32 * row_height;
            let center_offset = target - panel_height / 2.0;
            self.scroll_offset = center_offset.max(0.0);
        }
    }

    /// Render the source view in egui.
    pub fn render(&mut self, ui: &mut egui::Ui, state: &mut DebugUIState, view_id: egui::Id) {
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
        let available = ui.available_size();
        let _panel_height = available.y;

        egui::ScrollArea::vertical()
            .id_salt(view_id)
            .show_rows(ui, row_height, self.built_lines.len(), |ui, row_range| {
                for row in row_range {
                    if let Some(line) = self.built_lines.get(row) {
                        let rect = ui.available_rect_before_wrap();
                        let line_rect = egui::Rect::from_min_size(
                            rect.min,
                            egui::vec2(rect.width(), row_height),
                        );

                        // Active line background
                        if line.is_active {
                            ui.painter().rect_filled(line_rect, 0.0, egui::Color32::from_rgb(255, 255, 0));
                        }

                        ui.horizontal(|ui| {
                            // Gutter with line number
                            let gutter_width = 48.0;
                            let gutter_rect = egui::Rect::from_min_size(
                                ui.cursor().min,
                                egui::vec2(gutter_width, row_height),
                            );
                            ui.allocate_rect(gutter_rect, egui::Sense::click());

                            // Line number
                            ui.add_sized(
                                egui::vec2(gutter_width - 16.0, row_height),
                                egui::Label::new(
                                    egui::RichText::new(format!("{}", line.number))
                                        .monospace()
                                        .small(),
                                ),
                            );

                            // Breakpoint dot
                            if line.has_breakpoint {
                                let dot_color = if line.breakpoint_resolved {
                                    egui::Color32::from_rgb(200, 0, 0)
                                } else {
                                    egui::Color32::from_rgb(100, 0, 0)
                                };
                                let dot_rect = egui::Rect::from_min_size(
                                    gutter_rect.min + egui::vec2(gutter_width - 14.0, row_height / 2.0 - 4.0),
                                    egui::vec2(8.0, 8.0),
                                );
                                ui.painter().circle_filled(dot_rect.center(), 4.0, dot_color);
                            }

                            // Gutter click handler
                            if ui.interact(gutter_rect, ui.id().with(("gutter", row)), egui::Sense::click()).clicked() {
                                if line.has_breakpoint {
                                    state.remove_breakpoint(&self.file_path.clone().unwrap_or_default(), line.number);
                                } else {
                                    state.add_breakpoint(BreakpointEntry {
                                        file: self.file_path.clone().unwrap_or_default(),
                                        line: line.number,
                                        resolved: false,
                                    });
                                }
                            }

                            // Tokens
                            for (class, text) in &line.tokens {
                                let color = match class {
                                    TokenClass::Keyword => egui::Color32::from_rgb(200, 100, 200),
                                    TokenClass::StringLiteral => egui::Color32::from_rgb(100, 200, 100),
                                    TokenClass::CharLiteral => egui::Color32::from_rgb(100, 200, 100),
                                    TokenClass::LineComment => egui::Color32::from_rgb(128, 128, 128),
                                    TokenClass::BlockComment => egui::Color32::from_rgb(128, 128, 128),
                                    TokenClass::TypeIdent => egui::Color32::from_rgb(100, 200, 255),
                                    TokenClass::Other => ui.visuals().text_color(),
                                };
                                ui.add(egui::Label::new(
                                    egui::RichText::new(text).monospace().small().color(color),
                                ).selectable(false));
                            }
                        });
                    }
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn active_line_in_visible_range() {
        let mut view = SourceView::default();
        view.built_lines = (1..=1000u32).map(|n| SourceLine {
            number: n,
            tokens: vec![],
            is_active: false,
            has_breakpoint: false,
            breakpoint_resolved: false,
        }).collect();
        // Scroll so that line 500 is in the viewport
        let scroll_offset = (500u32.saturating_sub(1)) as f32 * 20.0;
        let range = view.visible_range(scroll_offset, 600.0, 20.0);
        assert!(range.contains(&500));
    }

    #[test]
    fn active_line_outside_needs_scroll() {
        let mut view = SourceView::default();
        view.built_lines = (1..=1000u32).map(|n| SourceLine {
            number: n,
            tokens: vec![],
            is_active: false,
            has_breakpoint: false,
            breakpoint_resolved: false,
        }).collect();
        let range = view.visible_range(4000.0, 600.0, 20.0); // scrolled to ~line 200
        assert!(view.needs_scroll(1, &range));
    }

    #[test]
    fn breakpoint_flag_on_source_line() {
        let mut state = DebugUIState::default();
        state.active_file = Some(PathBuf::from("src/main.rs"));
        state.add_breakpoint(BreakpointEntry {
            file: PathBuf::from("src/main.rs"),
            line: 5,
            resolved: true,
        });

        let mut view = SourceView::default();
        view.lines = vec![
            "line1".into(),
            "line2".into(),
            "line3".into(),
            "line4".into(),
            "line5".into(),
        ];
        view.build_lines(&state);

        assert!(view.built_lines[4].has_breakpoint);
        assert!(view.built_lines[4].breakpoint_resolved);
    }
}
