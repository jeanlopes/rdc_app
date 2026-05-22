//! [`IntrospectableUi`] — wraps `egui::Ui` and records every widget response.

use std::sync::{Arc, Mutex};
use crate::layout::LayoutPass;
use crate::stable_id::{StableIdRegistry, StableWidgetId};
use crate::widget_node::{SerializableRect, WidgetKind, WidgetNode};

/// A record of one widget captured during a frame.
#[derive(Debug)]
pub struct CapturedWidget {
    /// The widget node.
    pub node: WidgetNode,
    /// The layout pass for this widget.
    pub layout: LayoutPass,
}

/// Shared per-frame collector. Filled by `IntrospectableUi`, consumed by `IntrospectionContext`.
#[derive(Debug, Default)]
pub struct FrameCollector {
    /// Captured widgets for the current frame.
    pub widgets: Vec<CapturedWidget>,
}

impl FrameCollector {
    /// Record a widget response.
    pub fn record(
        &mut self,
        id: StableWidgetId,
        egui_id: egui::Id,
        kind: WidgetKind,
        label: Option<String>,
        response: &egui::Response,
        parent: Option<StableWidgetId>,
        clip_rect: egui::Rect,
        focused: bool,
    ) {
        let rect: SerializableRect = response.rect.into();
        let clipped = {
            let clip: SerializableRect = clip_rect.into();
            !clip.contains_rect(&rect)
        };
        let node = WidgetNode {
            id,
            egui_id: egui_id.value(),
            widget_kind: kind,
            label,
            rect,
            hovered: response.hovered(),
            clicked: response.clicked(),
            focused,
            clipped,
            children: vec![],
            parent,
        };
        let layout = LayoutPass::new(id, response.rect, clip_rect);
        self.widgets.push(CapturedWidget { node, layout });
    }
}

/// Wraps `egui::Ui` to intercept widget calls and build a semantic widget tree.
///
/// Use [`IntrospectableUi::new`] to wrap the root `Ui` provided by `eframe`,
/// then call widget methods on `IntrospectableUi` as you would on `egui::Ui`.
pub struct IntrospectableUi<'a> {
    ui: &'a mut egui::Ui,
    collector: Arc<Mutex<FrameCollector>>,
    id_registry: Arc<Mutex<StableIdRegistry>>,
    parent_id: Option<StableWidgetId>,
}

impl<'a> IntrospectableUi<'a> {
    /// Wrap a root `egui::Ui`.
    pub fn new(
        ui: &'a mut egui::Ui,
        collector: Arc<Mutex<FrameCollector>>,
        id_registry: Arc<Mutex<StableIdRegistry>>,
    ) -> Self {
        Self { ui, collector, id_registry, parent_id: None }
    }

    fn with_parent(
        ui: &'a mut egui::Ui,
        collector: Arc<Mutex<FrameCollector>>,
        id_registry: Arc<Mutex<StableIdRegistry>>,
        parent_id: Option<StableWidgetId>,
    ) -> Self {
        Self { ui, collector, id_registry, parent_id }
    }

    fn assign_id(&self, kind: WidgetKind, label: &str) -> StableWidgetId {
        let kind_tag = kind_to_tag(&kind);
        self.id_registry
            .lock()
            .unwrap()
            .assign(kind_tag, label, self.parent_id.unwrap_or_default())
    }

    fn record(
        &self,
        id: StableWidgetId,
        egui_id: egui::Id,
        kind: WidgetKind,
        label: Option<String>,
        response: &egui::Response,
    ) {
        let clip_rect = self.ui.clip_rect();
        let focused = self.ui.ctx().memory(|m| m.focused() == Some(egui_id));
        self.collector.lock().unwrap().record(
            id, egui_id, kind, label, response,
            self.parent_id, clip_rect, focused,
        );
    }

    // ── Widget wrappers ───────────────────────────────────────────────────────

    /// A clickable button. Returns the egui [`egui::Response`].
    pub fn button(&mut self, text: impl Into<egui::WidgetText>) -> egui::Response {
        let text = text.into();
        let label_str = text.text().to_string();
        let id = self.assign_id(WidgetKind::Button, &label_str);
        let response = self.ui.button(text);
        self.record(id, response.id, WidgetKind::Button, Some(label_str), &response);
        response
    }

    /// A non-interactive label.
    pub fn label(&mut self, text: impl Into<egui::WidgetText>) -> egui::Response {
        let text = text.into();
        let label_str = text.text().to_string();
        let id = self.assign_id(WidgetKind::Label, &label_str);
        let response = self.ui.label(text);
        self.record(id, response.id, WidgetKind::Label, Some(label_str), &response);
        response
    }

    /// A single-line text editor.
    pub fn text_edit_singleline(&mut self, text: &mut String) -> egui::Response {
        let id = self.assign_id(WidgetKind::TextEdit, "");
        let response = self.ui.text_edit_singleline(text);
        self.record(id, response.id, WidgetKind::TextEdit, None, &response);
        response
    }

    /// A boolean checkbox.
    pub fn checkbox(&mut self, checked: &mut bool, text: impl Into<egui::WidgetText>) -> egui::Response {
        let text = text.into();
        let label_str = text.text().to_string();
        let id = self.assign_id(WidgetKind::Checkbox, &label_str);
        let response = self.ui.checkbox(checked, text);
        self.record(id, response.id, WidgetKind::Checkbox, Some(label_str), &response);
        response
    }

    /// A horizontal layout scope. Children are also captured.
    pub fn horizontal<R>(&mut self, add_contents: impl FnOnce(&mut IntrospectableUi<'_>) -> R) -> R {
        let collector = Arc::clone(&self.collector);
        let id_registry = Arc::clone(&self.id_registry);
        let parent_id = self.parent_id;
        self.ui.horizontal(|inner_ui| {
            let mut iui = IntrospectableUi::with_parent(inner_ui, collector, id_registry, parent_id);
            add_contents(&mut iui)
        }).inner
    }

    /// A vertical layout scope. Children are also captured.
    pub fn vertical<R>(&mut self, add_contents: impl FnOnce(&mut IntrospectableUi<'_>) -> R) -> R {
        let collector = Arc::clone(&self.collector);
        let id_registry = Arc::clone(&self.id_registry);
        let parent_id = self.parent_id;
        self.ui.vertical(|inner_ui| {
            let mut iui = IntrospectableUi::with_parent(inner_ui, collector, id_registry, parent_id);
            add_contents(&mut iui)
        }).inner
    }

    /// Access the underlying `egui::Ui` directly for unsupported operations.
    pub fn raw(&mut self) -> &mut egui::Ui {
        self.ui
    }
}

fn kind_to_tag(kind: &WidgetKind) -> u8 {
    match kind {
        WidgetKind::Button => 0,
        WidgetKind::TextEdit => 1,
        WidgetKind::Label => 2,
        WidgetKind::Panel => 3,
        WidgetKind::ScrollArea => 4,
        WidgetKind::Checkbox => 5,
        WidgetKind::Slider => 6,
        WidgetKind::Window => 7,
        WidgetKind::ComboBox => 8,
        WidgetKind::RadioButton => 9,
        WidgetKind::Other(_) => 255,
    }
}
