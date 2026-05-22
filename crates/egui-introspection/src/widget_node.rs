//! Core widget data types: [`WidgetNode`], [`WidgetKind`], [`StableWidgetId`], [`InputState`].

use serde::{Deserialize, Serialize};

pub use crate::stable_id::StableWidgetId;

/// The kind of an egui widget. Used by the AI to determine what operations are valid.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WidgetKind {
    /// A clickable button.
    Button,
    /// A single-line or multi-line text editor.
    TextEdit,
    /// A non-interactive text label.
    Label,
    /// A container panel (Frame, CentralPanel, SidePanel, TopBottomPanel).
    Panel,
    /// A scrollable area.
    ScrollArea,
    /// A boolean checkbox.
    Checkbox,
    /// A numeric slider.
    Slider,
    /// A floating window.
    Window,
    /// A drop-down selector.
    ComboBox,
    /// A radio button in a group.
    RadioButton,
    /// An unknown or custom widget.
    Other(String),
}

impl std::fmt::Display for WidgetKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Button => write!(f, "Button"),
            Self::TextEdit => write!(f, "TextEdit"),
            Self::Label => write!(f, "Label"),
            Self::Panel => write!(f, "Panel"),
            Self::ScrollArea => write!(f, "ScrollArea"),
            Self::Checkbox => write!(f, "Checkbox"),
            Self::Slider => write!(f, "Slider"),
            Self::Window => write!(f, "Window"),
            Self::ComboBox => write!(f, "ComboBox"),
            Self::RadioButton => write!(f, "RadioButton"),
            Self::Other(s) => write!(f, "Other({s})"),
        }
    }
}

/// A single rendered UI element captured in a [`crate::UiSnapshot`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetNode {
    /// Stable semantic ID that persists across dynamic list reorders.
    pub id: StableWidgetId,
    /// egui's native hash-based ID (for internal cross-referencing only).
    pub egui_id: u64,
    /// The kind of widget.
    pub widget_kind: WidgetKind,
    /// Visible text label, if any.
    pub label: Option<String>,
    /// Allocated (post-clip) bounding box in screen pixels.
    pub rect: SerializableRect,
    /// Mouse is over this widget this frame.
    pub hovered: bool,
    /// Primary mouse button was released over this widget this frame.
    pub clicked: bool,
    /// This widget holds keyboard focus.
    pub focused: bool,
    /// This widget's rect is constrained by a parent clip rect.
    pub clipped: bool,
    /// Ordered list of direct child widget IDs.
    pub children: Vec<StableWidgetId>,
    /// Parent widget ID, or `None` for root-level widgets.
    pub parent: Option<StableWidgetId>,
}

/// A serializable 2D rectangle (mirrors `egui::Rect`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct SerializableRect {
    /// Top-left corner [x, y].
    pub min: [f32; 2],
    /// Bottom-right corner [x, y].
    pub max: [f32; 2],
}

impl SerializableRect {
    /// True if the rectangle has zero area.
    pub fn is_empty(&self) -> bool {
        self.max[0] <= self.min[0] || self.max[1] <= self.min[1]
    }

    /// True if `other` is fully contained within this rect.
    pub fn contains_rect(&self, other: &SerializableRect) -> bool {
        self.min[0] <= other.min[0]
            && self.min[1] <= other.min[1]
            && self.max[0] >= other.max[0]
            && self.max[1] >= other.max[1]
    }
}

impl From<egui::Rect> for SerializableRect {
    fn from(r: egui::Rect) -> Self {
        Self {
            min: [r.min.x, r.min.y],
            max: [r.max.x, r.max.y],
        }
    }
}

impl From<SerializableRect> for egui::Rect {
    fn from(r: SerializableRect) -> Self {
        egui::Rect::from_min_max(
            egui::pos2(r.min[0], r.min[1]),
            egui::pos2(r.max[0], r.max[1]),
        )
    }
}

/// Mouse and keyboard state at the time of a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InputState {
    /// Mouse cursor position, or `None` if off-window.
    pub mouse_pos: Option<[f32; 2]>,
    /// Primary (left) mouse button held.
    pub mouse_down: bool,
    /// Secondary (right) mouse button held.
    pub mouse_secondary: bool,
    /// Alt modifier held.
    pub alt: bool,
    /// Ctrl modifier held.
    pub ctrl: bool,
    /// Shift modifier held.
    pub shift: bool,
    /// Scroll wheel delta [dx, dy] this frame.
    pub scroll_delta: [f32; 2],
}

impl From<&egui::InputState> for InputState {
    fn from(s: &egui::InputState) -> Self {
        Self {
            mouse_pos: s.pointer.hover_pos().map(|p| [p.x, p.y]),
            mouse_down: s.pointer.primary_down(),
            mouse_secondary: s.pointer.secondary_down(),
            alt: s.modifiers.alt,
            ctrl: s.modifiers.ctrl,
            shift: s.modifiers.shift,
            scroll_delta: [s.raw_scroll_delta.x, s.raw_scroll_delta.y],
        }
    }
}
