//! Toolbar with 11 debugger buttons, keyboard shortcuts, and 200ms press animation.

use std::time::Instant;

use debug_session_view::{DebugUIState, ToolbarAction};
use egui::{Key, Modifiers};

/// A keyboard shortcut, supporting single-key and two-key chord sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// A keyboard shortcut, supporting single-key and two-key chord sequences.
pub enum KeyCombo {
    /// Single key with optional modifiers.
    Single { modifiers: Modifiers, key: Key },
    /// Two-key chord sequence.
    Chord {
        first: (Modifiers, Key),
        second: (Modifiers, Key),
    },
}

/// Tracks the first key of a chord across frames.
#[derive(Debug, Default, Clone, Copy)]
/// Tracks the first key of a chord across frames.
pub struct ChordState {
    pub first_modifiers: Modifiers,
    pub first_key: Option<Key>,
    pub pressed_at: Option<Instant>,
}

impl ChordState {
    /// Clear stale chord state (> 500ms).
    pub fn clear_stale(&mut self) {
        if let Some(t) = self.pressed_at {
            if t.elapsed().as_millis() > 500 {
                *self = Self::default();
            }
        }
    }

    /// Record the first key of a potential chord.
    pub fn record_first(&mut self, modifiers: Modifiers, key: Key) {
        self.first_modifiers = modifiers;
        self.first_key = Some(key);
        self.pressed_at = Some(Instant::now());
    }

    /// Consume the chord state and return the recorded key.
    pub fn take(&mut self) -> Option<(Modifiers, Key)> {
        let result = self.first_key.map(|k| (self.first_modifiers, k));
        *self = Self::default();
        result
    }
}

/// Render metadata for one toolbar button.
#[derive(Debug, Clone)]
/// Render metadata for one toolbar button.
pub struct ToolbarButton {
    pub action: ToolbarAction,
    pub label: &'static str,
    pub icon: &'static str,
    pub shortcut: &'static str,
    pub shortcut_key: KeyCombo,
}

impl ToolbarButton {
    /// All toolbar buttons.
    pub fn all() -> Vec<ToolbarButton> {
        vec![
            ToolbarButton {
                action: ToolbarAction::Continue,
                label: "Continue",
                icon: "▶",
                shortcut: "F5",
                shortcut_key: KeyCombo::Single {
                    modifiers: Modifiers::NONE,
                    key: Key::F5,
                },
            },
            ToolbarButton {
                action: ToolbarAction::BreakAll,
                label: "Break All",
                icon: "⏸",
                shortcut: "Ctrl+Alt+Break",
                shortcut_key: KeyCombo::Single {
                    modifiers: Modifiers::CTRL | Modifiers::ALT,
                    key: Key::Escape, // placeholder for Break key
                },
            },
            ToolbarButton {
                action: ToolbarAction::StopDebugging,
                label: "Stop",
                icon: "⏹",
                shortcut: "Shift+F5",
                shortcut_key: KeyCombo::Single {
                    modifiers: Modifiers::SHIFT,
                    key: Key::F5,
                },
            },
            ToolbarButton {
                action: ToolbarAction::Restart,
                label: "Restart",
                icon: "🔄",
                shortcut: "Ctrl+Shift+F5",
                shortcut_key: KeyCombo::Single {
                    modifiers: Modifiers::CTRL | Modifiers::SHIFT,
                    key: Key::F5,
                },
            },
            ToolbarButton {
                action: ToolbarAction::StepBackInto,
                label: "Step Back Into",
                icon: "⏮",
                shortcut: "Ctrl+R, F11",
                shortcut_key: KeyCombo::Chord {
                    first: (Modifiers::CTRL, Key::R),
                    second: (Modifiers::NONE, Key::F11),
                },
            },
            ToolbarButton {
                action: ToolbarAction::StepBackOver,
                label: "Step Back Over",
                icon: "⏭",
                shortcut: "Ctrl+R, F10",
                shortcut_key: KeyCombo::Chord {
                    first: (Modifiers::CTRL, Key::R),
                    second: (Modifiers::NONE, Key::F10),
                },
            },
            ToolbarButton {
                action: ToolbarAction::StepBackOut,
                label: "Step Back Out",
                icon: "⏏",
                shortcut: "Ctrl+R, Shift+F11",
                shortcut_key: KeyCombo::Chord {
                    first: (Modifiers::CTRL, Key::R),
                    second: (Modifiers::SHIFT, Key::F11),
                },
            },
            ToolbarButton {
                action: ToolbarAction::ShowNextStatement,
                label: "Show Next",
                icon: "🔍",
                shortcut: "Alt+Num *",
                shortcut_key: KeyCombo::Single {
                    modifiers: Modifiers::ALT,
                    key: Key::Num8, // placeholder for Num *
                },
            },
            ToolbarButton {
                action: ToolbarAction::StepInto,
                label: "Step Into",
                icon: "↓",
                shortcut: "F11",
                shortcut_key: KeyCombo::Single {
                    modifiers: Modifiers::NONE,
                    key: Key::F11,
                },
            },
            ToolbarButton {
                action: ToolbarAction::StepOver,
                label: "Step Over",
                icon: "↷",
                shortcut: "F10",
                shortcut_key: KeyCombo::Single {
                    modifiers: Modifiers::NONE,
                    key: Key::F10,
                },
            },
            ToolbarButton {
                action: ToolbarAction::StepOut,
                label: "Step Out",
                icon: "↑",
                shortcut: "Shift+F11",
                shortcut_key: KeyCombo::Single {
                    modifiers: Modifiers::SHIFT,
                    key: Key::F11,
                },
            },
            ToolbarButton {
                action: ToolbarAction::ShowThreadsInSource,
                label: "Threads",
                icon: "🧵",
                shortcut: "",
                shortcut_key: KeyCombo::Single {
                    modifiers: Modifiers::NONE,
                    key: Key::Num0,
                },
            },
        ]
    }
}

/// Horizontal toolbar widget.
#[derive(Debug, Default)]
/// Horizontal toolbar widget.
pub struct Toolbar {
    pub buttons: Vec<ToolbarButton>,
}

impl Toolbar {
    pub fn new() -> Self {
        Self {
            buttons: ToolbarButton::all(),
        }
    }

    /// Scan keyboard input each frame and return triggered actions.
    pub fn check_keyboard(
        &self,
        ctx: &egui::Context,
        chord_state: &mut ChordState,
    ) -> Vec<ToolbarAction> {
        let mut actions = Vec::new();
        chord_state.clear_stale();

        ctx.input(|i| {
            for ev in &i.events {
                if let egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    ..
                } = ev
                {
                    // Check chords first
                    if let Some((first_mod, first_key)) = chord_state.take() {
                        for btn in &self.buttons {
                            if let KeyCombo::Chord { second, .. } = btn.shortcut_key {
                                if first_mod == first_key_expected_modifiers(btn.shortcut_key)
                                    && first_key == key_expected_first_key(btn.shortcut_key)
                                    && *modifiers == second.0
                                    && *key == second.1
                                {
                                    actions.push(btn.action);
                                }
                            }
                        }
                        continue;
                    }

                    // Check single keys and chord starters
                    for btn in &self.buttons {
                        match btn.shortcut_key {
                            KeyCombo::Single { modifiers: expected_mod, key: expected_key } => {
                                if *modifiers == expected_mod && *key == expected_key {
                                    actions.push(btn.action);
                                }
                            }
                            KeyCombo::Chord { first, .. } => {
                                if *modifiers == first.0 && *key == first.1 {
                                    chord_state.record_first(*modifiers, *key);
                                }
                            }
                        }
                    }
                }
            }
        });

        actions
    }

    /// Render the toolbar and return clicked actions.
    pub fn render(&self, ui: &mut egui::Ui, state: &DebugUIState) -> Vec<ToolbarAction> {
        let mut actions = Vec::new();
        ui.horizontal(|ui| {
            for btn in &self.buttons {
                let pressed = state.is_pressed(btn.action);
                let btn_text = format!("{} {}", btn.icon, btn.label);
                let mut visuals = ui.visuals().widgets.inactive.clone();
                if pressed {
                    visuals = ui.visuals().widgets.active.clone();
                }

                let response = ui
                    .add(
                        egui::Button::new(egui::RichText::new(&btn_text).small())
                            .fill(visuals.bg_fill)
                            .stroke(visuals.bg_stroke),
                    )
                    .on_hover_text(format!("{} ({})", btn.label, btn.shortcut));

                if response.clicked() {
                    actions.push(btn.action);
                }
            }
        });
        actions
    }
}

fn first_key_expected_modifiers(combo: KeyCombo) -> Modifiers {
    match combo {
        KeyCombo::Chord { first, .. } => first.0,
        _ => Modifiers::NONE,
    }
}

fn key_expected_first_key(combo: KeyCombo) -> Key {
    match combo {
        KeyCombo::Chord { first, .. } => first.1,
        _ => Key::Escape,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use debug_session_view::DebugUIState;
    use std::collections::HashMap;

    #[test]
    fn animation_active_within_200ms() {
        let mut state = DebugUIState::default();
        state.press_action(ToolbarAction::Continue);
        assert!(state.is_pressed(ToolbarAction::Continue));
    }

    #[test]
    fn animation_cleared_after_200ms() {
        let mut state = DebugUIState::default();
        state.press_action(ToolbarAction::Continue);
        // We can't sleep 250ms in a fast test — instead verify the logic threshold
        // by checking that the stored instant is older than 0ms but the fn checks < 200ms.
        // For a robust test we would need to mock Instant, which is beyond this scope.
        // Instead we assert the internal entry exists and rely on the `is_pressed` threshold.
        std::thread::sleep(std::time::Duration::from_millis(250));
        assert!(!state.is_pressed(ToolbarAction::Continue));
    }

    #[test]
    fn all_shortcuts_unique() {
        let buttons = ToolbarButton::all();
        let mut seen = HashMap::new();
        for btn in buttons.iter() {
            if !matches!(
                btn.shortcut_key,
                KeyCombo::Single {
                    modifiers: Modifiers::NONE,
                    key: Key::Num0,
                    ..
                }
            ) {
                assert!(
                    seen.insert(btn.shortcut_key, btn.action).is_none(),
                    "duplicate shortcut: {:?}",
                    btn.shortcut_key
                );
            }
        }
    }
}
