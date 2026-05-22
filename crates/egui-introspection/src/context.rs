//! [`IntrospectionContext`] — per-frame capture lifecycle.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashMap;
use tracing::instrument;

use crate::paint::from_clipped_shapes;
use crate::snapshot::{IntrospectionStore, UiSnapshot};
use crate::stable_id::{StableIdRegistry, StableWidgetId};
use crate::ui::{FrameCollector, IntrospectableUi};
use crate::widget_node::{InputState, WidgetNode};

/// Manages the per-frame widget capture lifecycle.
///
/// Create one `IntrospectionContext` per session, then call:
/// 1. [`begin_frame`](Self::begin_frame) before egui renders
/// 2. [`wrap`](Self::wrap) to get an [`IntrospectableUi`] for the root `Ui`
/// 3. [`end_frame`](Self::end_frame) after egui's `Context::run()` returns
pub struct IntrospectionContext {
    store: IntrospectionStore,
    id_registry: Arc<Mutex<StableIdRegistry>>,
    collector: Arc<Mutex<FrameCollector>>,
    frame_index: u64,
    session_start_ms: u64,
}

impl IntrospectionContext {
    /// Create a new context tied to `store`.
    pub fn new(store: IntrospectionStore) -> Self {
        let start = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self {
            store,
            id_registry: Arc::new(Mutex::new(StableIdRegistry::new())),
            collector: Arc::new(Mutex::new(FrameCollector::default())),
            frame_index: 0,
            session_start_ms: start,
        }
    }

    /// Reset per-frame state. Call once before the egui frame begins.
    pub fn begin_frame(&mut self) {
        self.id_registry.lock().unwrap().begin_frame();
        *self.collector.lock().unwrap() = FrameCollector::default();
    }

    /// Wrap a root `egui::Ui` to capture all widget responses.
    pub fn wrap<'a>(&self, ui: &'a mut egui::Ui) -> IntrospectableUi<'a> {
        IntrospectableUi::new(ui, Arc::clone(&self.collector), Arc::clone(&self.id_registry))
    }

    /// Finalise the frame: build a [`UiSnapshot`] and publish it to the store.
    ///
    /// `full_output` is the value returned by `egui::Context::end_frame()` (or by
    /// `egui::Context::run()`).  It is used to capture paint commands and input state.
    #[instrument(skip_all, fields(frame = self.frame_index))]
    pub fn end_frame(&mut self, ctx: &egui::Context, shapes: Vec<egui::epaint::ClippedShape>) {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
            .saturating_sub(self.session_start_ms);

        let paint_commands = from_clipped_shapes(&shapes);

        let input_state: InputState = ctx.input(|i| InputState::from(i));

        // Focused egui id
        let focused_egui_id: Option<u64> = ctx.memory(|m| m.focused().map(|id| id.value()));

        let captured = {
            let mut col = self.collector.lock().unwrap();
            std::mem::take(&mut col.widgets)
        };

        // Build widget nodes with parent-child relationships.
        // For now parent tracking is via `CapturedWidget.node.parent` set during capture.
        // Wire up children lists.
        let mut widgets: Vec<WidgetNode> = captured.iter().map(|c| c.node.clone()).collect();
        let layout_passes = captured.iter().map(|c| c.layout.clone()).collect();

        // Build children lists from parent references
        let mut parent_to_children: HashMap<StableWidgetId, Vec<StableWidgetId>> = HashMap::new();
        for w in &widgets {
            if let Some(pid) = w.parent {
                parent_to_children.entry(pid).or_default().push(w.id);
            }
        }
        for w in &mut widgets {
            if let Some(children) = parent_to_children.get(&w.id) {
                w.children = children.clone();
            }
        }

        // Resolve focused widget
        let focused_widget: Option<StableWidgetId> = focused_egui_id.and_then(|fid| {
            widgets.iter().find(|w| w.egui_id == fid).map(|w| w.id)
        });

        // Update focused flag
        if let Some(fid) = focused_widget {
            for w in &mut widgets {
                w.focused = w.id == fid;
            }
        }

        let mut widget_index = HashMap::new();
        for (i, w) in widgets.iter().enumerate() {
            widget_index.insert(w.id, i);
        }

        let snapshot = UiSnapshot {
            frame_index: self.frame_index,
            timestamp_ms,
            widgets,
            widget_index,
            layout_passes,
            paint_commands,
            input_state,
            focused_widget,
        };

        self.frame_index += 1;
        self.store.publish(snapshot);
    }
}
