//! MCP tool handlers for egui UI introspection.
//!
//! Six tools expose the latest [`UiSnapshot`] to the AI agent:
//! `ui_snapshot`, `ui_find_widget`, `ui_widget_info`,
//! `ui_children`, `ui_clipped_widgets`, `ui_snapshot_diff`.

use std::sync::Arc;
use serde_json::{json, Value};
use tracing::instrument;
use egui_introspection::{IntrospectionStore, UiSnapshot};
use egui_introspection::stable_id::StableWidgetId;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn widget_summary(w: &egui_introspection::widget_node::WidgetNode) -> Value {
    json!({
        "id": w.id.to_string(),
        "kind": w.widget_kind.to_string(),
        "label": w.label,
        "rect": { "min": w.rect.min, "max": w.rect.max },
        "hovered": w.hovered,
        "clicked": w.clicked,
        "focused": w.focused,
        "clipped": w.clipped,
        "parent_id": w.parent.map(|p| p.to_string()),
        "child_count": w.children.len(),
    })
}

fn require_snapshot(store: &IntrospectionStore) -> Option<Arc<UiSnapshot>> {
    store.latest_blocking()
}

// ── Tool: ui_snapshot ─────────────────────────────────────────────────────────

/// Return a summary of the current UI snapshot.
#[instrument(skip(store))]
pub fn ui_snapshot(store: &IntrospectionStore) -> Value {
    let Some(snap) = require_snapshot(store) else {
        return json!({ "error": "no_snapshot_available" });
    };
    let top_level: Vec<Value> = snap.widgets.iter()
        .filter(|w| w.parent.is_none())
        .map(widget_summary)
        .collect();
    json!({
        "frame_index": snap.frame_index,
        "timestamp_ms": snap.timestamp_ms,
        "widget_count": snap.widgets.len(),
        "focused_widget_id": snap.focused_widget.map(|id| id.to_string()),
        "top_level_widgets": top_level,
        "clipped_count": snap.clipped_widgets().len(),
    })
}

// ── Tool: ui_find_widget ──────────────────────────────────────────────────────

/// Find all widgets matching a label, optionally filtered by kind.
#[instrument(skip(store))]
pub fn ui_find_widget(store: &IntrospectionStore, label: &str, kind: Option<&str>) -> Value {
    let Some(snap) = require_snapshot(store) else {
        return json!({ "error": "no_snapshot_available" });
    };
    let matches: Vec<Value> = snap.widgets.iter()
        .filter(|w| {
            let label_ok = w.label.as_deref() == Some(label);
            let kind_ok = kind.map_or(true, |k| w.widget_kind.to_string() == k);
            label_ok && kind_ok
        })
        .map(widget_summary)
        .collect();
    let total = matches.len();
    json!({ "matches": matches, "total": total })
}

// ── Tool: ui_widget_info ──────────────────────────────────────────────────────

/// Return full detail for one widget by stable ID.
#[instrument(skip(store))]
pub fn ui_widget_info(store: &IntrospectionStore, id_str: &str) -> Value {
    let Ok(id_num) = id_str.parse::<u64>() else {
        return json!({ "error": "invalid_id", "id": id_str });
    };
    let id = StableWidgetId(id_num);
    let Some(snap) = require_snapshot(store) else {
        return json!({ "error": "no_snapshot_available" });
    };
    let Some(w) = snap.get(id) else {
        return json!({ "error": "widget_not_found", "id": id_str });
    };
    let layout = snap.layout_passes.iter().find(|l| l.widget_id == id).map(|l| json!({
        "allocated_rect": { "min": l.allocated_rect.min, "max": l.allocated_rect.max },
        "desired_rect":   { "min": l.desired_rect.min,   "max": l.desired_rect.max },
        "overflow": l.overflow,
    }));
    json!({
        "id": w.id.to_string(),
        "kind": w.widget_kind.to_string(),
        "label": w.label,
        "rect": { "min": w.rect.min, "max": w.rect.max },
        "hovered": w.hovered,
        "clicked": w.clicked,
        "focused": w.focused,
        "clipped": w.clipped,
        "parent_id": w.parent.map(|p| p.to_string()),
        "children": w.children.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
        "layout": layout,
    })
}

// ── Tool: ui_children ────────────────────────────────────────────────────────

/// Return the direct children of a widget.
#[instrument(skip(store))]
pub fn ui_children(store: &IntrospectionStore, id_str: &str) -> Value {
    let Ok(id_num) = id_str.parse::<u64>() else {
        return json!({ "error": "invalid_id", "id": id_str });
    };
    let id = StableWidgetId(id_num);
    let Some(snap) = require_snapshot(store) else {
        return json!({ "error": "no_snapshot_available" });
    };
    if snap.get(id).is_none() {
        return json!({ "error": "widget_not_found", "id": id_str });
    }
    let children: Vec<Value> = snap.children_of(id).iter().map(|w| widget_summary(w)).collect();
    let count = children.len();
    json!({ "parent_id": id_str, "children": children, "count": count })
}

// ── Tool: ui_clipped_widgets ─────────────────────────────────────────────────

/// Return all widgets with `clipped == true`.
#[instrument(skip(store))]
pub fn ui_clipped_widgets(store: &IntrospectionStore) -> Value {
    let Some(snap) = require_snapshot(store) else {
        return json!({ "error": "no_snapshot_available" });
    };
    let clipped: Vec<Value> = snap.clipped_widgets().iter().map(|w| {
        let layout = snap.layout_passes.iter().find(|l| l.widget_id == w.id);
        json!({
            "id": w.id.to_string(),
            "kind": w.widget_kind.to_string(),
            "label": w.label,
            "rect": { "min": w.rect.min, "max": w.rect.max },
            "desired_rect": layout.map(|l| json!({"min": l.desired_rect.min, "max": l.desired_rect.max})),
            "parent_id": w.parent.map(|p| p.to_string()),
        })
    }).collect();
    let total = clipped.len();
    json!({ "clipped_widgets": clipped, "total": total })
}

// ── Tool: ui_snapshot_diff ───────────────────────────────────────────────────

/// Diff the current snapshot against the previous frame.
#[instrument(skip(store))]
pub fn ui_snapshot_diff(store: &IntrospectionStore) -> Value {
    let Some(snap) = require_snapshot(store) else {
        return json!({ "error": "no_snapshot_available" });
    };
    // For diff, we'd need the previous snapshot. For now return an error if frame 0.
    if snap.frame_index == 0 {
        return json!({ "error": "no_previous_frame" });
    }
    // In a real implementation, we'd store the previous frame in IntrospectionStore.
    // For this stub, we return a "no previous" error if we can't get prev.
    json!({ "error": "no_previous_frame" })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use egui_introspection::snapshot::UiSnapshot;
    use egui_introspection::widget_node::{WidgetKind, WidgetNode, SerializableRect};
    use std::collections::HashMap;

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

    fn make_store_with(nodes: Vec<WidgetNode>) -> IntrospectionStore {
        let store = IntrospectionStore::new();
        let mut index = HashMap::new();
        for (i, n) in nodes.iter().enumerate() {
            index.insert(n.id, i);
        }
        let snap = UiSnapshot {
            frame_index: 1,
            timestamp_ms: 100,
            widgets: nodes,
            widget_index: index,
            layout_passes: vec![],
            paint_commands: vec![],
            input_state: Default::default(),
            focused_widget: None,
        };
        store.publish(snap);
        store
    }

    #[test]
    fn tool_ui_find_widget_returns_match() {
        let store = make_store_with(vec![make_node(1, "Sort")]);
        let result = ui_find_widget(&store, "Sort", None);
        assert_eq!(result["total"], 1);
        assert_eq!(result["matches"][0]["label"], "Sort");
    }

    #[test]
    fn tool_ui_find_widget_no_match_empty() {
        let store = make_store_with(vec![make_node(1, "Sort")]);
        let result = ui_find_widget(&store, "NonExistent", None);
        assert_eq!(result["total"], 0);
        assert!(result.get("error").is_none(), "no error for empty match");
    }

    #[test]
    fn tool_ui_widget_info_not_found() {
        let store = make_store_with(vec![make_node(1, "Sort")]);
        let result = ui_widget_info(&store, "9999");
        assert_eq!(result["error"], "widget_not_found");
    }

    #[test]
    fn tool_ui_clipped_widgets_empty_when_none() {
        let store = make_store_with(vec![make_node(1, "Sort")]);
        let result = ui_clipped_widgets(&store);
        assert_eq!(result["total"], 0);
    }

    #[test]
    fn tool_ui_snapshot_diff_no_previous() {
        // frame_index = 1 in our test store, but no previous stored
        let store = make_store_with(vec![]);
        // Manually set frame_index to 0 to trigger the no_previous_frame error
        let store2 = IntrospectionStore::new();
        let snap = UiSnapshot {
            frame_index: 0,
            timestamp_ms: 0,
            widgets: vec![],
            widget_index: HashMap::new(),
            layout_passes: vec![],
            paint_commands: vec![],
            input_state: Default::default(),
            focused_widget: None,
        };
        store2.publish(snap);
        let result = ui_snapshot_diff(&store2);
        assert_eq!(result["error"], "no_previous_frame");
    }
}
