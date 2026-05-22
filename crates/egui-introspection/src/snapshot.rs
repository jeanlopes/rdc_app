//! [`UiSnapshot`] — point-in-time capture of the entire egui widget tree.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, watch};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::layout::LayoutPass;
use crate::paint::PaintCmd;
use crate::stable_id::StableWidgetId;
use crate::widget_node::{InputState, WidgetNode};

/// Point-in-time capture of the entire UI widget tree for one rendered frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiSnapshot {
    /// Monotonically increasing frame counter.
    pub frame_index: u64,
    /// Milliseconds since session start.
    pub timestamp_ms: u64,
    /// All widget nodes in depth-first render order.
    pub widgets: Vec<WidgetNode>,
    /// O(1) lookup: `StableWidgetId` → index into `widgets`.
    pub widget_index: HashMap<StableWidgetId, usize>,
    /// One layout entry per widget.
    pub layout_passes: Vec<LayoutPass>,
    /// Ordered render command stream.
    pub paint_commands: Vec<PaintCmd>,
    /// Mouse + keyboard state this frame.
    pub input_state: InputState,
    /// The single focused widget, or `None`.
    pub focused_widget: Option<StableWidgetId>,
}

impl UiSnapshot {
    /// Find all widget nodes whose label matches `label` exactly (case-sensitive).
    pub fn find_by_label(&self, label: &str) -> Vec<&WidgetNode> {
        self.widgets
            .iter()
            .filter(|w| w.label.as_deref() == Some(label))
            .collect()
    }

    /// Return the widget holding keyboard focus, or `None`.
    pub fn focused_widget(&self) -> Option<&WidgetNode> {
        self.focused_widget.and_then(|id| self.get(id))
    }

    /// Return all widgets whose `clipped` flag is set.
    pub fn clipped_widgets(&self) -> Vec<&WidgetNode> {
        self.widgets.iter().filter(|w| w.clipped).collect()
    }

    /// Look up a widget by its stable ID.
    pub fn get(&self, id: StableWidgetId) -> Option<&WidgetNode> {
        self.widget_index.get(&id).and_then(|&i| self.widgets.get(i))
    }

    /// Return the direct children of `parent_id`.
    pub fn children_of(&self, parent_id: StableWidgetId) -> Vec<&WidgetNode> {
        self.get(parent_id)
            .map(|p| p.children.iter().filter_map(|&id| self.get(id)).collect())
            .unwrap_or_default()
    }

    /// Return the parent of `widget_id`, or `None` for root-level widgets.
    pub fn parent_of(&self, widget_id: StableWidgetId) -> Option<&WidgetNode> {
        self.get(widget_id)?.parent.and_then(|pid| self.get(pid))
    }
}

/// Shared in-process state between the egui render loop and MCP handlers.
///
/// The egui main loop calls [`IntrospectionStore::publish`] after each
/// `end_frame()`. MCP handlers call [`IntrospectionStore::latest`].
#[derive(Clone)]
pub struct IntrospectionStore {
    current: Arc<RwLock<Option<Arc<UiSnapshot>>>>,
    notifier: watch::Sender<u64>,
}

impl IntrospectionStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        let (tx, _) = watch::channel(0u64);
        Self {
            current: Arc::new(RwLock::new(None)),
            notifier: tx,
        }
    }

    /// Subscribe to frame-completion notifications.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.notifier.subscribe()
    }

    /// Publish a completed frame snapshot (called from the egui render thread).
    pub fn publish(&self, snapshot: UiSnapshot) {
        debug!(frame = snapshot.frame_index, widgets = snapshot.widgets.len(), "snapshot published");
        let frame = snapshot.frame_index;
        let arc = Arc::new(snapshot);
        // blocking_write: safe from sync context (egui main loop)
        *self.current.blocking_write() = Some(arc);
        let _ = self.notifier.send(frame);
    }

    /// Get the latest snapshot, or `None` if no frame has completed yet.
    ///
    /// Async-safe: can be called from Tokio task context.
    pub async fn latest(&self) -> Option<Arc<UiSnapshot>> {
        self.current.read().await.clone()
    }

    /// Get the latest snapshot from a synchronous context (e.g. tests).
    pub fn latest_blocking(&self) -> Option<Arc<UiSnapshot>> {
        self.current.blocking_read().clone()
    }
}

impl Default for IntrospectionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget_node::{WidgetKind, SerializableRect};

    fn make_rect() -> SerializableRect {
        SerializableRect { min: [0.0, 0.0], max: [100.0, 30.0] }
    }

    fn make_node(id: u64, label: &str) -> WidgetNode {
        WidgetNode {
            id: StableWidgetId(id),
            egui_id: 0,
            widget_kind: WidgetKind::Button,
            label: Some(label.to_string()),
            rect: make_rect(),
            hovered: false,
            clicked: false,
            focused: false,
            clipped: false,
            children: vec![],
            parent: None,
        }
    }

    fn make_snapshot(nodes: Vec<WidgetNode>) -> UiSnapshot {
        let mut index = HashMap::new();
        for (i, n) in nodes.iter().enumerate() {
            index.insert(n.id, i);
        }
        UiSnapshot {
            frame_index: 0,
            timestamp_ms: 0,
            widgets: nodes,
            widget_index: index,
            layout_passes: vec![],
            paint_commands: vec![],
            input_state: Default::default(),
            focused_widget: None,
        }
    }

    #[test]
    fn snapshot_widget_index_consistent() {
        let nodes = vec![make_node(1, "A"), make_node(2, "B"), make_node(3, "C")];
        let snap = make_snapshot(nodes);
        assert_eq!(snap.widget_index.len(), snap.widgets.len());
    }

    #[test]
    fn find_by_label_returns_all_matches() {
        let mut n1 = make_node(1, "Delete");
        let mut n2 = make_node(2, "Delete");
        n2.widget_kind = WidgetKind::Label;
        let snap = make_snapshot(vec![n1, n2]);
        let matches = snap.find_by_label("Delete");
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn focused_widget_at_most_one() {
        let mut node = make_node(1, "Input");
        node.focused = true;
        let mut snap = make_snapshot(vec![node]);
        snap.focused_widget = Some(StableWidgetId(1));
        let result = snap.focused_widget();
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, StableWidgetId(1));
    }

    #[test]
    fn focused_widget_none_when_no_focus() {
        let snap = make_snapshot(vec![make_node(1, "X")]);
        assert!(snap.focused_widget().is_none());
    }

    #[test]
    fn clipped_widgets_list() {
        let mut n1 = make_node(1, "A");
        n1.clipped = true;
        let n2 = make_node(2, "B");
        let n3 = make_node(3, "C");
        let snap = make_snapshot(vec![n1, n2, n3]);
        assert_eq!(snap.clipped_widgets().len(), 1);
    }

    #[test]
    fn children_of_returns_direct_children_only() {
        // Panel → ScrollArea → Button
        let button_id = StableWidgetId(3);
        let scroll_id = StableWidgetId(2);
        let panel_id = StableWidgetId(1);

        let mut button = make_node(3, "Click");
        button.parent = Some(scroll_id);

        let mut scroll = make_node(2, "");
        scroll.widget_kind = WidgetKind::ScrollArea;
        scroll.children = vec![button_id];
        scroll.parent = Some(panel_id);

        let mut panel = make_node(1, "Panel");
        panel.widget_kind = WidgetKind::Panel;
        panel.children = vec![scroll_id];

        let snap = make_snapshot(vec![panel, scroll, button]);
        let children = snap.children_of(panel_id);
        assert_eq!(children.len(), 1, "only direct child (scroll) should be returned");
        assert_eq!(children[0].id, scroll_id);
        // button should not appear as direct child of panel
        assert!(!children.iter().any(|n| n.id == button_id));
    }

    #[test]
    fn parent_of_root_returns_none() {
        let node = make_node(1, "Root");
        let snap = make_snapshot(vec![node]);
        assert!(snap.parent_of(StableWidgetId(1)).is_none());
    }

    #[test]
    fn deep_nesting_no_stack_overflow() {
        // Build a 25-level deep chain
        let mut nodes = Vec::new();
        let mut parent: Option<StableWidgetId> = None;
        for i in 1u64..=25 {
            let mut node = make_node(i, &format!("level_{i}"));
            node.parent = parent;
            if let Some(pid) = parent {
                if let Some(last) = nodes.last_mut() {
                    let p: &mut WidgetNode = last;
                    p.children.push(StableWidgetId(i));
                }
            }
            parent = Some(StableWidgetId(i));
            nodes.push(node);
        }
        let snap = make_snapshot(nodes);
        // Traverse all 25 levels
        let mut current = snap.get(StableWidgetId(1));
        let mut depth = 0;
        while let Some(node) = current {
            depth += 1;
            current = node.children.first().and_then(|&c| snap.get(c));
        }
        assert_eq!(depth, 25);
    }

    #[test]
    fn zero_size_widget_present_in_tree() {
        let mut node = make_node(1, "Spacer");
        node.rect = SerializableRect { min: [10.0, 10.0], max: [10.0, 10.0] }; // zero size
        let snap = make_snapshot(vec![node]);
        let w = snap.get(StableWidgetId(1)).unwrap();
        assert!(w.rect.is_empty(), "zero-size rect should report is_empty() == true");
    }

    #[test]
    fn snapshot_during_render_returns_last_complete() {
        let store = IntrospectionStore::new();
        // Before any publish, latest_blocking returns None
        assert!(store.latest_blocking().is_none());
        // After publish, returns the snapshot
        let snap = make_snapshot(vec![make_node(1, "X")]);
        store.publish(snap);
        assert!(store.latest_blocking().is_some());
    }
}
