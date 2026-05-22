# Data Model: egui UI Introspection Layer

**Feature**: `003-egui-introspection` | **Date**: 2026-05-22

---

## Core Types

### `UiSnapshot`

Point-in-time capture of the entire UI widget tree for one rendered frame.

```
UiSnapshot {
    frame_index:      u64                           // monotonically increasing frame counter
    timestamp_ms:     u64                           // wall-clock ms since session start
    widgets:          Vec<WidgetNode>               // all nodes, in depth-first render order
    widget_index:     HashMap<StableWidgetId, usize> // O(1) lookup by stable ID → vec index
    layout_passes:    Vec<LayoutPass>               // one entry per widget
    paint_commands:   Vec<PaintCmd>                 // ordered render stream
    input_state:      InputState                    // mouse + keyboard state this frame
    focused_widget:   Option<StableWidgetId>        // single focused widget, or None
    previous:         Option<Arc<UiSnapshot>>       // previous frame (for diff; None at frame 0)
}
```

**Invariants**:
- `widgets.len() == layout_passes.len()` — every rendered widget has exactly one layout entry.
- `widget_index.len() == widgets.len()` — index contains every widget exactly once.
- `frame_index` strictly increases; no frame is published twice.

---

### `WidgetNode`

A single rendered UI element.

```
WidgetNode {
    id:            StableWidgetId          // stable semantic ID (survives list reorders)
    egui_native_id: egui::Id              // egui's native hash-based ID (internal use only)
    widget_kind:   WidgetKind             // enum of known egui widget types
    label:         Option<String>         // visible text, if any
    rect:          Rect                   // allocated (post-clip) bounding box, in screen coords
    hovered:       bool                   // mouse is over this widget this frame
    clicked:       bool                   // primary mouse button released over widget this frame
    focused:       bool                   // widget holds keyboard focus this frame
    clipped:       bool                   // rect is constrained by parent clip rect
    children:      Vec<StableWidgetId>    // direct children, in render order
    parent:        Option<StableWidgetId> // None for root-level widgets
}
```

**Invariants**:
- If `clipped == true`, the widget's `rect` is the intersection of its desired rect and the parent's clip rect.
- `children` is empty for leaf widgets (`Button`, `Label`, `TextEdit`, `Checkbox`, `Slider`).
- `focused == true` for at most one widget per snapshot.

---

### `StableWidgetId`

A persistent semantic identifier assigned by the introspection layer.

```
StableWidgetId(u64)
```

Derived from: `hash(widget_kind, label_text_or_empty, parent_stable_id, ordinal_within_parent)`

- Survives list reordering: a button that moves from position 2 to position 5 retains the same `StableWidgetId`.
- For widgets with identical `(kind, label)` within the same parent, `ordinal_within_parent` is the tiebreaker.
- `0` is reserved as the null/invalid sentinel; valid IDs start at `1`.

---

### `WidgetKind`

Enum of known egui widget types. Used by the AI to determine what operations are valid.

```
WidgetKind {
    Button,
    TextEdit,
    Label,
    Panel,         // Frame, CentralPanel, SidePanel, TopBottomPanel
    ScrollArea,
    Checkbox,
    Slider,
    Window,
    ComboBox,
    RadioButton,
    Other(String)  // unknown/custom widget type name
}
```

---

### `SnapshotDiff`

Result of comparing two consecutive `UiSnapshot`s.

```
SnapshotDiff {
    frame_from:   u64                            // older frame_index
    frame_to:     u64                            // newer frame_index
    added:        Vec<StableWidgetId>            // widgets in `to` not in `from`
    removed:      Vec<StableWidgetId>            // widgets in `from` not in `to`
    changed:      Vec<(StableWidgetId, WidgetStateDelta)>  // widgets present in both, state changed
}
```

**Note**: A widget that moved position in a dynamic list appears in `changed` with `position_moved: true`, NOT in `added`/`removed` — this is the key benefit of stable semantic IDs.

---

### `WidgetStateDelta`

State changes for a widget that existed in both frames.

```
WidgetStateDelta {
    hovered_changed:  Option<bool>    // Some(new_value) if hovered changed
    clicked_changed:  Option<bool>    // Some(new_value) if clicked changed
    focused_changed:  Option<bool>    // Some(new_value) if focused changed
    rect_changed:     Option<Rect>    // Some(new_rect) if position/size changed
    clipped_changed:  Option<bool>    // Some(new_value) if clipping status changed
    label_changed:    Option<String>  // Some(new_label) if label text changed
    position_moved:   bool            // widget moved within its parent (list reorder)
}
```

---

### `LayoutPass`

Layout information for one widget in one frame.

```
LayoutPass {
    widget_id:      StableWidgetId  // matches WidgetNode.id
    allocated_rect: Rect            // post-clip rect (same as WidgetNode.rect)
    desired_rect:   Rect            // pre-clip rect (approximated from max_rect + allocation)
    clip_rect:      Rect            // parent's clip rect at time of layout
    overflow:       bool            // desired_rect extends beyond clip_rect
}
```

**Overflow detection**: `overflow = !clip_rect.contains_rect(desired_rect)`

---

### `PaintCmd`

A single render instruction from egui's paint pass.

```
PaintCmd {
    clip_rect:  Rect
    primitive:  PaintPrimitive
}

PaintPrimitive {
    Rect   { rect: Rect, fill: Color32, stroke: Stroke }
    Text   { pos: Pos2, text: String, color: Color32 }
    Circle { center: Pos2, radius: f32, fill: Color32, stroke: Stroke }
    Line   { points: Vec<Pos2>, stroke: Stroke }
    Mesh   { vertex_count: u32, index_count: u32 }
    Other
}
```

---

### `InputState`

Mouse and keyboard state at the time of the snapshot.

```
InputState {
    mouse_pos:       Option<Pos2>   // None if cursor is off-window
    mouse_down:      bool           // primary (left) mouse button held
    mouse_secondary: bool           // secondary (right) mouse button held
    modifiers:       Modifiers      // { alt, ctrl, shift, mac_cmd, command }
    scroll_delta:    Vec2           // scroll wheel delta this frame
}
```

---

### `IntrospectionStore`

Shared in-process state between the egui render loop and the MCP server.

```
IntrospectionStore {
    current:  Arc<tokio::sync::RwLock<Option<UiSnapshot>>>  // latest completed frame
    notifier: tokio::sync::watch::Sender<u64>               // sends frame_index on each new frame
}
```

**Thread model**:
- egui main loop: calls `store.current.blocking_write()` after each `end_frame()`.
- MCP handler threads: call `store.current.read().await` to get the latest snapshot.
- MCP handlers subscribe to `notifier` to be woken on new frame availability.

---

## Relationships

```
UiSnapshot ──1:N──► WidgetNode       (all widgets in the frame)
UiSnapshot ──1:N──► LayoutPass       (one per widget)
UiSnapshot ──1:N──► PaintCmd         (render stream)
UiSnapshot ──1:1──► InputState       (input snapshot)
UiSnapshot ──0:1──► UiSnapshot       (previous frame, for diff)
WidgetNode ──1:1──► StableWidgetId   (identity)
WidgetNode ──0:1──► WidgetNode       (parent)
WidgetNode ──1:N──► WidgetNode       (children)
LayoutPass ──1:1──► WidgetNode       (via StableWidgetId)
SnapshotDiff ──2──► UiSnapshot       (from, to)
IntrospectionStore ──1:1──► UiSnapshot (latest frame)
```

---

## StableIdRegistry (internal, not exposed via MCP)

Maintains the mapping from semantic tuple to stable u64. Lives inside `IntrospectionContext`.

```
StableIdRegistry {
    next_id:  u64                                              // starts at 1
    map:      HashMap<(WidgetKind, String, StableWidgetId, usize), StableWidgetId>
                                //  kind   label  parent    ordinal → stable id
}
```

On each frame:
1. Clear the per-frame `ordinal_counters` map.
2. For each collected `Response`, look up `(kind, label, parent_id, ordinal)` in `map`.
3. If found → reuse the existing `StableWidgetId`.
4. If not found → assign `next_id`, increment, insert into `map`.

---

## Crate Boundary

`crates/egui-introspection` owns and exports:
- All types above except `StableIdRegistry` (internal).
- `IntrospectableUi` (wraps `egui::Ui`; developer-facing).
- `IntrospectionContext` (manages per-frame capture; developer-facing).

`apps/mcp-server` consumes:
- `IntrospectionStore` (shared reference).
- `UiSnapshot`, `WidgetNode`, `SnapshotDiff`, `WidgetKind`, `StableWidgetId` (for MCP responses).
