//! Snapshot diffing: detect added, removed, and changed widgets between frames.

use serde::{Deserialize, Serialize};
use crate::stable_id::StableWidgetId;
use crate::widget_node::SerializableRect;
use crate::snapshot::UiSnapshot;

/// Result of comparing two consecutive [`UiSnapshot`]s.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SnapshotDiff {
    /// Frame index of the older snapshot.
    pub frame_from: u64,
    /// Frame index of the newer snapshot.
    pub frame_to: u64,
    /// Widgets present in `to` but not in `from`.
    pub added: Vec<StableWidgetId>,
    /// Widgets present in `from` but not in `to`.
    pub removed: Vec<StableWidgetId>,
    /// Widgets present in both frames whose state changed.
    pub changed: Vec<(StableWidgetId, WidgetStateDelta)>,
}

/// State changes for a widget that existed in both frames.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetStateDelta {
    /// New `hovered` value if it changed, otherwise `None`.
    pub hovered_changed: Option<bool>,
    /// New `clicked` value if it changed, otherwise `None`.
    pub clicked_changed: Option<bool>,
    /// New `focused` value if it changed, otherwise `None`.
    pub focused_changed: Option<bool>,
    /// New rect if position or size changed, otherwise `None`.
    pub rect_changed: Option<SerializableRect>,
    /// New `clipped` value if it changed, otherwise `None`.
    pub clipped_changed: Option<bool>,
    /// New label if it changed, otherwise `None`.
    pub label_changed: Option<Option<String>>,
    /// True when the widget moved within its parent (list reorder).
    pub position_moved: bool,
}

impl WidgetStateDelta {
    fn is_empty(&self) -> bool {
        self.hovered_changed.is_none()
            && self.clicked_changed.is_none()
            && self.focused_changed.is_none()
            && self.rect_changed.is_none()
            && self.clipped_changed.is_none()
            && self.label_changed.is_none()
            && !self.position_moved
    }
}

impl UiSnapshot {
    /// Compute the diff between two snapshots.
    ///
    /// Widgets are compared by [`StableWidgetId`]. A widget that moved position
    /// in a dynamic list appears in `changed` with `position_moved: true`,
    /// NOT in `added`/`removed`.
    pub fn diff(from: &UiSnapshot, to: &UiSnapshot) -> SnapshotDiff {
        use std::collections::HashMap;

        let from_map: HashMap<StableWidgetId, _> =
            from.widgets.iter().map(|w| (w.id, w)).collect();
        let to_map: HashMap<StableWidgetId, _> =
            to.widgets.iter().map(|w| (w.id, w)).collect();

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();

        for id in to_map.keys() {
            if !from_map.contains_key(id) {
                added.push(*id);
            }
        }
        for (id, from_widget) in &from_map {
            if let Some(to_widget) = to_map.get(id) {
                let delta = WidgetStateDelta {
                    hovered_changed: (from_widget.hovered != to_widget.hovered)
                        .then_some(to_widget.hovered),
                    clicked_changed: (from_widget.clicked != to_widget.clicked)
                        .then_some(to_widget.clicked),
                    focused_changed: (from_widget.focused != to_widget.focused)
                        .then_some(to_widget.focused),
                    rect_changed: (from_widget.rect != to_widget.rect)
                        .then_some(to_widget.rect),
                    clipped_changed: (from_widget.clipped != to_widget.clipped)
                        .then_some(to_widget.clipped),
                    label_changed: (from_widget.label != to_widget.label)
                        .then_some(to_widget.label.clone()),
                    // position_moved: parent is same but ordinal within parent changed
                    position_moved: from_widget.parent == to_widget.parent
                        && position_changed(from_widget, to_widget, from, to),
                };
                if !delta.is_empty() {
                    changed.push((*id, delta));
                }
            } else {
                removed.push(*id);
            }
        }

        SnapshotDiff {
            frame_from: from.frame_index,
            frame_to: to.frame_index,
            added,
            removed,
            changed,
        }
    }
}

fn position_changed(
    from_w: &crate::widget_node::WidgetNode,
    to_w: &crate::widget_node::WidgetNode,
    from_snap: &UiSnapshot,
    to_snap: &UiSnapshot,
) -> bool {
    // Check if the widget's position in its parent's children list changed
    let ordinal_in = |snap: &UiSnapshot, parent_id: Option<StableWidgetId>, target_id: StableWidgetId| {
        parent_id
            .and_then(|pid| snap.widget_index.get(&pid))
            .and_then(|&idx| snap.widgets.get(idx))
            .map(|parent| parent.children.iter().position(|&c| c == target_id))
            .flatten()
    };
    let from_ord = ordinal_in(from_snap, from_w.parent, from_w.id);
    let to_ord = ordinal_in(to_snap, to_w.parent, to_w.id);
    from_ord != to_ord
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget_node::{SerializableRect, WidgetKind};
    use crate::snapshot::UiSnapshot;
    use crate::widget_node::WidgetNode;
    use std::collections::HashMap;

    fn make_rect(x: f32, y: f32) -> SerializableRect {
        SerializableRect { min: [x, y], max: [x + 100.0, y + 30.0] }
    }

    fn make_node(id: u64, label: &str, hovered: bool) -> WidgetNode {
        WidgetNode {
            id: StableWidgetId(id),
            egui_id: 0,
            widget_kind: WidgetKind::Button,
            label: Some(label.to_string()),
            rect: make_rect(0.0, 0.0),
            hovered,
            clicked: false,
            focused: false,
            clipped: false,
            children: vec![],
            parent: None,
        }
    }

    fn make_snapshot(frame: u64, nodes: Vec<WidgetNode>) -> UiSnapshot {
        let mut index = HashMap::new();
        for (i, n) in nodes.iter().enumerate() {
            index.insert(n.id, i);
        }
        UiSnapshot {
            frame_index: frame,
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
    fn diff_identical_frames_empty() {
        let a = make_snapshot(0, vec![make_node(1, "Sort", false)]);
        let b = make_snapshot(1, vec![make_node(1, "Sort", false)]);
        let diff = UiSnapshot::diff(&a, &b);
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert!(diff.changed.is_empty());
    }

    #[test]
    fn diff_added_widget() {
        let a = make_snapshot(0, vec![]);
        let b = make_snapshot(1, vec![make_node(1, "New", false)]);
        let diff = UiSnapshot::diff(&a, &b);
        assert_eq!(diff.added, vec![StableWidgetId(1)]);
        assert!(diff.removed.is_empty());
    }

    #[test]
    fn diff_removed_widget() {
        let a = make_snapshot(0, vec![make_node(1, "Old", false)]);
        let b = make_snapshot(1, vec![]);
        let diff = UiSnapshot::diff(&a, &b);
        assert!(diff.added.is_empty());
        assert_eq!(diff.removed, vec![StableWidgetId(1)]);
    }

    #[test]
    fn diff_hovered_state_changed() {
        let a = make_snapshot(0, vec![make_node(1, "Sort", true)]);
        let b = make_snapshot(1, vec![make_node(1, "Sort", false)]);
        let diff = UiSnapshot::diff(&a, &b);
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].1.hovered_changed, Some(false));
    }

    #[test]
    fn diff_reorder_not_add_remove() {
        // Widget 1 moves from ordinal 0 to ordinal 1 (after widget 2)
        let mut root_a = make_node(99, "root", false);
        root_a.widget_kind = WidgetKind::Panel;
        root_a.children = vec![StableWidgetId(1), StableWidgetId(2)];

        let mut w1 = make_node(1, "Item", false);
        w1.parent = Some(StableWidgetId(99));
        let mut w2 = make_node(2, "Other", false);
        w2.parent = Some(StableWidgetId(99));

        let mut root_b = root_a.clone();
        root_b.children = vec![StableWidgetId(2), StableWidgetId(1)]; // reordered

        let a = make_snapshot(0, vec![root_a, w1.clone(), w2.clone()]);
        let b = make_snapshot(1, vec![root_b, w2, w1]);

        let diff = UiSnapshot::diff(&a, &b);
        // widget 1 should be in changed (position moved), not added/removed
        assert!(!diff.added.contains(&StableWidgetId(1)), "reordered widget should not be in added");
        assert!(!diff.removed.contains(&StableWidgetId(1)), "reordered widget should not be in removed");
    }
}
