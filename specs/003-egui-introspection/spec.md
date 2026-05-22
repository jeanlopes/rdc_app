# Feature Specification: egui UI Introspection Layer

**Feature Branch**: `003-egui-introspection`

**Created**: 2026-05-22

**Status**: Draft

**Input**: Phase 3 — transform egui into a semantically observable interface by extracting internal runtime state (no computer vision).

---

## Context

This feature creates `egui-introspection`, a Rust crate that instruments an egui application to produce a **semantic UI tree** — a structured, queryable snapshot of every widget currently rendered: its position, label, interaction state, and parent-child relationships.

The primary consumer is an AI agent that must understand, locate, and reason about the user interface without access to source code or visual screenshots. The developer enables introspection by integrating the crate; no changes to individual widget calls are required.

---

## Clarifications

### Session 2026-05-22

- Q: Should Phase 3 include the MCP tool surface, or only the capture crate? → A: Phase 3 includes both the capture crate and MCP tools (Option B). Acceptance criteria are only testable end-to-end with the MCP surface present.
- Q: Should WidgetId be stable across dynamic list reorders, or accept egui's native hash-based churn? → A: Require stable semantic identity (Option B) — the introspection layer assigns persistent handles so the AI can track a specific widget across frames regardless of list reordering.
- Q: Are `LayoutPass` and `PaintCmd` mandatory deliverables or stretch goals in Phase 3? → A: Both mandatory (Option C) — full `UiSnapshot` as originally specified, including `layout_passes: Vec<LayoutPass>` and `paint_commands: Vec<PaintCmd>`.
- Q: Should `WidgetNode` expose the widget's kind/type? → A: Yes, add `widget_kind: WidgetKind` enum (Option B) — covers `Button`, `TextEdit`, `Label`, `Panel`, `ScrollArea`, `Checkbox`, `Slider`, `Window`, `Other`.
- Q: How does the MCP server access the snapshot — in-process, IPC, or file? → A: In-process (Option B) — the egui application embeds the MCP server; the MCP tool handler reads the latest snapshot directly from memory with no IPC overhead.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Widget Location by AI Agent (Priority: P1)

An AI agent needs to find a specific widget in the running UI — for example, a button labeled "Sort" or a text input labeled "Search". It queries the latest snapshot and receives the widget's identity, position, and current state.

**Why this priority**: All higher-level AI operations (clicking, inspecting, navigating) depend on the ability to locate widgets first. Without this, every other scenario is blocked.

**Independent Test**: Integrate `egui-introspection` into `debug-target-example`'s egui window. After one rendered frame, call `snapshot.find_by_label("Sort")` and assert the returned `WidgetNode` has a non-zero rect and correct interaction flags.

**Acceptance Scenarios**:

1. **Given** a running egui application with a visible button labeled "Confirm", **When** the AI requests the latest snapshot, **Then** the snapshot contains a `WidgetNode` with `label == "Confirm"` and a non-empty bounding rect.
2. **Given** a widget that is currently hovered by the mouse, **When** the AI reads its `WidgetNode`, **Then** `hovered == true`.
3. **Given** a widget outside the visible viewport (scrolled out of view), **When** the snapshot is queried, **Then** the widget is present but its rect is flagged as clipped.
4. **Given** multiple widgets with identical labels, **When** queried by label, **Then** all matching nodes are returned with unique `WidgetId`s.

---

### User Story 2 — Layout Problem Detection (Priority: P2)

An AI agent inspects a panel that a user reports as "broken." It reads the snapshot to determine whether widgets are clipped, overflowing their containers, or have zero-size rects — conditions invisible to a human screenshot.

**Why this priority**: Layout bugs are a core use case for AI-assisted UI debugging. Detecting clipping programmatically eliminates the need for visual analysis.

**Independent Test**: Render a panel whose child exceeds the parent's height. Query `snapshot.clipped_widgets()` and assert it returns the overflowing child node.

**Acceptance Scenarios**:

1. **Given** a widget whose rect extends beyond its parent container's rect, **When** the AI calls `snapshot.clipped_widgets()`, **Then** the widget appears in the returned list with a `clipped: true` flag.
2. **Given** all widgets fitting within their containers, **When** the AI calls `snapshot.clipped_widgets()`, **Then** an empty list is returned.
3. **Given** a widget with a zero-size rect (collapsed panel), **When** the AI inspects the snapshot, **Then** `rect.is_empty() == true` is exposed in the node.

---

### User Story 3 — Focus and Interaction State Inspection (Priority: P3)

An AI agent determines which widget holds keyboard focus and which was most recently clicked. This lets it understand the user's active context without reading mouse/keyboard events directly.

**Why this priority**: Focus and click state are required for the AI to reason about form flow, modal dialogs, and step-by-step interactions. Depends on P1 (widget tree) being available.

**Independent Test**: Programmatically trigger a click on a text input, then query `snapshot.focused_widget()` and assert it matches the input's `WidgetId`.

**Acceptance Scenarios**:

1. **Given** the user has clicked a text input, **When** the AI reads the snapshot, **Then** `snapshot.focused_widget()` returns the `WidgetId` of that input and `focused == true` on its `WidgetNode`.
2. **Given** no widget is focused (e.g., click on empty canvas), **When** `snapshot.focused_widget()` is called, **Then** `None` is returned.
3. **Given** a button that was clicked in the current frame, **When** the snapshot is queried, **Then** the button's `WidgetNode` has `clicked == true`.

---

### User Story 4 — Hierarchical Navigation (Priority: P4)

An AI agent traverses the widget tree to understand containment relationships — for example, determining that a label belongs to a specific panel, or that a button is inside a scroll area.

**Why this priority**: Contextual understanding of widget placement is required for accurate semantic descriptions and for scoped queries ("find all buttons inside the settings panel").

**Independent Test**: Render a `Panel > ScrollArea > Button` hierarchy. Walk `snapshot.children_of(panel_id)` and assert the scroll area is found; recurse to assert the button is found within.

**Acceptance Scenarios**:

1. **Given** a `Panel` containing a `Button`, **When** the AI calls `snapshot.children_of(panel_id)`, **Then** the returned list includes the button's `WidgetId`.
2. **Given** a widget id, **When** `snapshot.parent_of(widget_id)` is called, **Then** the correct parent `WidgetId` is returned, or `None` for root-level widgets.
3. **Given** a subtree query for a panel, **When** all descendants are collected, **Then** the results match the widgets actually rendered inside that panel.

---

### User Story 5 — Snapshot Diffing Between Frames (Priority: P5)

An AI agent detects which widgets changed between two consecutive frames — new widgets appearing, widgets disappearing, or state changes (e.g., a button going from enabled to disabled).

**Why this priority**: Diffing enables the AI to react to UI transitions without re-analyzing the entire tree, and is the foundation for detecting state-driven UI bugs.

**Independent Test**: Capture snapshot at frame N; trigger a dialog to open; capture snapshot at frame N+1. Assert `diff.added` contains the dialog's widgets.

**Acceptance Scenarios**:

1. **Given** two consecutive snapshots where a modal dialog opened, **When** `UiSnapshot::diff(a, b)` is called, **Then** `diff.added` contains all widgets introduced by the dialog.
2. **Given** two identical consecutive snapshots, **When** diffed, **Then** `diff.added` and `diff.removed` are both empty.
3. **Given** a widget whose `hovered` state changed between frames, **When** diffed, **Then** `diff.changed` contains that widget's id with the updated state.

---

### Edge Cases

- Widget with no text label — identified by `WidgetId` only; `label` is `None`.
- Deeply nested hierarchies (> 20 levels) — must not cause stack overflow during traversal.
- Dynamic widgets that appear and disappear within the same frame — captured in the snapshot for that frame only.
- Zero-size widgets (invisible spacers, collapsed panels) — present in tree with `rect.is_empty() == true`.
- Snapshot requested during frame rendering — returns the last completed frame's snapshot; never a partial frame.
- Widgets in a dynamically reordered list — stable semantic ID tracks the widget to its new position; diff reports a position change rather than remove + add.
- Two widgets with identical label and type in the same parent — distinguished by their sequential position index; ambiguity is flagged in the snapshot.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The introspection layer MUST capture a complete `UiSnapshot` after every rendered frame without requiring changes to individual widget call sites.
- **FR-002**: Each `WidgetNode` MUST expose: a **stable semantic `WidgetId`**, **`widget_kind`** (one of: `Button`, `TextEdit`, `Label`, `Panel`, `ScrollArea`, `Checkbox`, `Slider`, `Window`, `Other`), bounding rect, label text (if any), `hovered`, `clicked`, `focused` flags, and an ordered list of child `WidgetId`s.
- **FR-003**: The system MUST detect widgets whose bounding rect extends beyond their parent container's rect and mark them with `clipped: true`.
- **FR-004**: The snapshot MUST expose which single widget currently holds keyboard focus via `focused_widget() -> Option<WidgetId>`.
- **FR-005**: The snapshot MUST support label-based search returning all matching `WidgetNode`s.
- **FR-006**: The snapshot MUST support hierarchy traversal: given a `WidgetId`, return its children and its parent.
- **FR-007**: The system MUST provide a `diff` operation between two `UiSnapshot` instances returning added, removed, and changed widget sets.
- **FR-008**: Snapshot capture overhead MUST be imperceptible to the end user (no visible frame drops).
- **FR-009**: The introspection layer MUST be opt-in and zero-cost when disabled at compile time.
- **FR-010**: The system MUST NOT use computer vision, screenshot analysis, or pixel inspection — all state is derived from the egui runtime directly.
- **FR-011**: The system MUST expose MCP tools that allow an AI agent to query the latest `UiSnapshot` — including widget lookup by label, hierarchy traversal, clipping detection, and focus state — without writing Rust code.
- **FR-012**: MCP tool responses MUST serialize `WidgetNode` data (including `widget_kind`, `label`, `rect`, and interaction flags) in a format the AI agent can parse without additional transformation.
- **FR-015**: The egui application MUST host the MCP server in-process. The introspection layer MUST publish each completed frame's snapshot to a shared in-process store readable by MCP tool handlers without IPC or file I/O.
- **FR-013**: The system MUST capture `LayoutPass` data per frame, exposing each widget's pre-clip desired rect alongside its final allocated rect, to enable overflow and clipping analysis.
- **FR-014**: The system MUST capture `PaintCmd` data per frame, recording the ordered sequence of render instructions, to enable detection of render-layout mismatches and invisible-but-present widgets.

### Key Entities

- **UiSnapshot**: Point-in-time capture of the entire UI widget tree for one rendered frame. Contains all `WidgetNode`s, current `InputState`, and frame metadata.
- **WidgetNode**: A single renderable element. Attributes: `id`, `widget_kind` (enum: `Button | TextEdit | Label | Panel | ScrollArea | Checkbox | Slider | Window | Other`), `label` (optional), `rect`, `hovered`, `clicked`, `focused`, `clipped`, `children: Vec<WidgetId>`.
- **SnapshotDiff**: Result of comparing two `UiSnapshot`s. Contains `added: Vec<WidgetId>`, `removed: Vec<WidgetId>`, `changed: Vec<(WidgetId, WidgetStateDelta)>`.
- **InputState**: Mouse position, pressed buttons, active keyboard modifiers at the time of the snapshot.
- **LayoutPass**: Record of layout computation for a frame. Contains pre-clip bounding boxes for each widget, enabling detection of overflow (widget desired size vs. allocated size) and clipping regions.
- **PaintCmd**: A single render instruction captured from the egui paint pass. Used to verify what was actually drawn versus what the layout intended — enables detection of invisible-but-present widgets and render-layer mismatches.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An AI agent can locate any visible, labeled widget in under 10ms after a frame completes.
- **SC-002**: Snapshot capture adds less than 2ms overhead per frame on a standard development machine (measured as frame time delta with and without introspection enabled).
- **SC-003**: `snapshot.clipped_widgets()` correctly identifies all clipped widgets in 100% of a defined test suite of layout scenarios.
- **SC-004**: `snapshot.focused_widget()` returns the correct result in 100% of tested focus-change scenarios (click, tab, programmatic focus).
- **SC-005**: An AI agent can construct a complete parent-to-leaf path for any widget in the tree using only `parent_of` / `children_of` in ≤ N calls where N equals the tree depth.
- **SC-006**: `UiSnapshot::diff` correctly categorizes all added, removed, and state-changed widgets across a test suite of 20 representative frame transitions.

---

## Assumptions

- egui version compatibility: targets the egui version already in use by the project (integration via existing `Cargo.toml` dependency).
- The application runs on Windows x86-64; multi-platform support is out of scope for Phase 3.
- The egui application embeds the MCP server in the same process. The introspection layer publishes each completed frame's snapshot to a shared in-process store (e.g., an `Arc<RwLock<UiSnapshot>>`). MCP tool handlers read from this store on demand. No IPC, shared memory, or file I/O is required for snapshot access.
- Widget identity: the introspection layer maintains a stable semantic `WidgetId` derived from widget label, type, and parent chain. When a dynamic list reorders, the same logical widget retains the same `WidgetId` across frames. Deliberate ID conflicts introduced by the host application are out of scope.
- Phase 3 delivers both the in-process `egui-introspection` capture crate **and** the MCP tool surface that exposes snapshot queries to the AI agent. This is required for end-to-end acceptance criteria to be testable.
- `LayoutPass` and `PaintCmd` capture are **mandatory** Phase 3 deliverables. The complete `UiSnapshot` structure — `widgets`, `layout_passes`, `paint_commands`, and `input_state` — must be captured each frame.
