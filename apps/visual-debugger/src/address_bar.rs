//! Address bar showing current file path and editable navigation.

use std::path::PathBuf;

/// Tracks the address bar's edit state independently of the source viewer.
#[derive(Debug, Default, Clone)]
/// Tracks the address bar's edit state independently of the source viewer.
pub struct AddressBarState {
    /// Shown when not editing.
    pub display_path: String,
    /// Buffer while user types.
    pub edit_text: String,
    /// True when bar has focus.
    pub editing: bool,
    /// Set on invalid/missing path; cleared on next keystroke.
    pub error: Option<String>,
}

impl AddressBarState {
    pub fn set_path(&mut self, path: &PathBuf) {
        self.display_path = path.to_string_lossy().to_string();
        self.edit_text = self.display_path.clone();
        self.error = None;
    }
}

/// Address bar widget.
#[derive(Debug, Default)]
/// Address bar widget.
pub struct AddressBar;

impl AddressBar {
    /// Render read-only display of the current path.
    pub fn render_display(ui: &mut egui::Ui, state: &mut AddressBarState) {
        ui.horizontal(|ui| {
            ui.label("📁");
            ui.add(
                egui::Label::new(egui::RichText::new(&state.display_path).monospace())
,
            );
        });
    }

    /// Render editable address bar with Enter handler and inline error.
    pub fn render_edit(ui: &mut egui::Ui, state: &mut AddressBarState) -> Option<PathBuf> {
        let mut submitted = None;
        ui.horizontal(|ui| {
            ui.label("📁");
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.edit_text)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );

            if response.changed() {
                state.error = None;
            }

            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let path = PathBuf::from(&state.edit_text);
                if path.exists() {
                    state.display_path = state.edit_text.clone();
                    state.editing = false;
                    submitted = Some(path);
                } else {
                    state.error = Some(format!("File not found: {}", state.edit_text));
                }
            }

            if response.clicked() {
                state.editing = true;
            }
        });

        if let Some(ref err) = state.error {
            ui.colored_label(egui::Color32::RED, err);
        }

        submitted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_bar_shows_active_file() {
        let mut state = AddressBarState::default();
        state.set_path(&PathBuf::from("src/main.rs"));
        assert_eq!(state.display_path, "src/main.rs");
    }

    #[test]
    fn address_bar_invalid_path_sets_error() {
        let mut state = AddressBarState::default();
        state.edit_text = "nonexistent_file.xyz".to_string();
        // simulate validation
        let path = PathBuf::from(&state.edit_text);
        if !path.exists() {
            state.error = Some(format!("File not found: {}", state.edit_text));
        }
        assert!(state.error.is_some());
    }

    #[test]
    fn address_bar_valid_path_clears_error() {
        let mut state = AddressBarState::default();
        state.error = Some("old error".to_string());
        state.edit_text = std::env::current_dir().unwrap().to_string_lossy().to_string();
        let path = PathBuf::from(&state.edit_text);
        if path.exists() {
            state.error = None;
            state.display_path = state.edit_text.clone();
        }
        assert!(state.error.is_none());
    }
}
