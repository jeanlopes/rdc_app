# MCP UI Inspection Tools Contract

**Crate**: `apps/mcp-server` — `ui_inspection` module
**Protocol**: MCP (Model Context Protocol) — JSON-RPC over stdio
**Date**: 2026-05-22

All tools operate on the **latest completed frame's** `UiSnapshot`. Requests that arrive between frames receive the last committed snapshot — never a partial frame.

---

## Tool: `ui_snapshot`

Returns a summary of the current UI snapshot — widget count, focused widget, and top-level widget list.

### Input
```json
{}
```

### Output
```json
{
  "frame_index": 142,
  "timestamp_ms": 5820,
  "widget_count": 47,
  "focused_widget_id": "9182736450",
  "top_level_widgets": [
    {
      "id": "1",
      "kind": "Window",
      "label": "Debug Panel",
      "rect": { "min": [0, 0], "max": [400, 600] },
      "child_count": 12
    }
  ],
  "clipped_count": 2
}
```

### Error cases
- `{ "error": "no_snapshot_available" }` — no frame has completed yet.

---

## Tool: `ui_find_widget`

Finds all widgets matching a label string, optionally filtered by widget kind.

### Input
```json
{
  "label": "Sort",
  "kind": "Button"   // optional; omit to search all kinds
}
```

### Output
```json
{
  "matches": [
    {
      "id": "4738291056",
      "kind": "Button",
      "label": "Sort",
      "rect": { "min": [120, 340], "max": [200, 365] },
      "hovered": false,
      "clicked": false,
      "focused": false,
      "clipped": false,
      "parent_id": "8827361092"
    }
  ],
  "total": 1
}
```

### Notes
- Returns empty `matches: []` when nothing matches (not an error).
- Label matching is case-sensitive and exact.
- `id` is a string representation of `StableWidgetId` (u64 as decimal string).

---

## Tool: `ui_widget_info`

Returns full detail for one widget by stable ID.

### Input
```json
{
  "id": "4738291056"
}
```

### Output
```json
{
  "id": "4738291056",
  "kind": "Button",
  "label": "Sort",
  "rect": { "min": [120, 340], "max": [200, 365] },
  "hovered": false,
  "clicked": false,
  "focused": false,
  "clipped": false,
  "parent_id": "8827361092",
  "children": [],
  "layout": {
    "allocated_rect": { "min": [120, 340], "max": [200, 365] },
    "desired_rect":   { "min": [120, 340], "max": [200, 365] },
    "overflow": false
  }
}
```

### Error cases
- `{ "error": "widget_not_found", "id": "4738291056" }` — ID not in current snapshot.

---

## Tool: `ui_children`

Returns the direct children of a widget (one level only).

### Input
```json
{
  "id": "8827361092"
}
```

### Output
```json
{
  "parent_id": "8827361092",
  "children": [
    {
      "id": "4738291056",
      "kind": "Button",
      "label": "Sort",
      "rect": { "min": [120, 340], "max": [200, 365] },
      "clipped": false
    },
    {
      "id": "5512389001",
      "kind": "Label",
      "label": "Status: idle",
      "rect": { "min": [120, 370], "max": [280, 390] },
      "clipped": false
    }
  ],
  "count": 2
}
```

### Error cases
- `{ "error": "widget_not_found", "id": "..." }` — parent ID not in current snapshot.

---

## Tool: `ui_clipped_widgets`

Returns all widgets whose rendered rect is constrained by a parent clip rect (overflow / scroll-clipped).

### Input
```json
{}
```

### Output
```json
{
  "clipped_widgets": [
    {
      "id": "3392817465",
      "kind": "Label",
      "label": "This text overflows the panel",
      "rect": { "min": [0, 580], "max": [390, 600] },
      "desired_rect": { "min": [0, 580], "max": [390, 640] },
      "parent_id": "1192837465"
    }
  ],
  "total": 1
}
```

---

## Tool: `ui_snapshot_diff`

Diffs the current snapshot against the previous frame's snapshot. Returns what changed.

### Input
```json
{}
```

### Output
```json
{
  "frame_from": 141,
  "frame_to": 142,
  "added": [
    { "id": "9920011234", "kind": "Window", "label": "Error Dialog" }
  ],
  "removed": [],
  "changed": [
    {
      "id": "4738291056",
      "hovered_changed": true,
      "clicked_changed": null,
      "focused_changed": null,
      "rect_changed": null,
      "position_moved": false
    }
  ]
}
```

### Error cases
- `{ "error": "no_previous_frame" }` — only one frame has been captured so far (frame 0).

---

## Serialization Rules

All coordinates are `[x, y]` pixel arrays measured from the top-left corner of the window. `Rect` is `{ "min": [x, y], "max": [x, y] }`. `StableWidgetId` is serialized as a decimal string (not integer) to avoid JSON 64-bit integer precision loss. `Color32` is serialized as `"#RRGGBBAA"` hex string.

## Versioning

These tools are versioned as part of `crates/protocol`. Breaking changes require a MAJOR semver bump per Constitution Principle VII.
