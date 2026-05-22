---
description: "Task list for 003-egui-introspection — egui UI Introspection Layer"
---

# Tasks: egui UI Introspection Layer

**Input**: Design documents from `specs/003-egui-introspection/`

**Prerequisites**: plan.md ✅, spec.md ✅, data-model.md ✅, research.md ✅, contracts/ ✅

**Tests**: REQUIRED — plan.md § Phase 2 Test Coverage defines explicit test names per story.

**Organization**: Tasks grouped by user story. Each phase delivers an independently testable increment.

## Format: `[ID] [P?] [Story?] Description — file path`

- **[P]**: Parallelizable (different files, no inter-task dependency)
- **[Story]**: US1–US5 per spec.md

## Path conventions

```
crates/egui-introspection/src/   ← new crate
apps/mcp-server/src/tools/       ← new tool module
apps/desktop-ui/src/             ← integration bridge (assumed to exist)
```

---

## Phase 1: Crate Setup

**Purpose**: Create the `egui-introspection` crate skeleton, register it in the workspace, and create all empty module files so the workspace compiles immediately.

- [X] T001 Create `crates/egui-introspection/Cargo.toml` with `[features] introspection = []`, `egui`, `serde`, `serde_json`, `tokio`, `tracing` dependencies
- [X] T002 Add `egui-introspection = { path = "crates/egui-introspection" }` to root `Cargo.toml` workspace members
- [X] T003 [P] Create `crates/egui-introspection/src/lib.rs` with `#![warn(missing_docs)]` and module declarations (`pub mod context`, `pub mod ui`, `pub mod snapshot`, etc.)
- [X] T004 [P] Create empty module stubs: `crates/egui-introspection/src/widget_node.rs`, `stable_id.rs`, `context.rs`, `ui.rs`, `snapshot.rs`, `diff.rs`, `layout.rs`, `paint.rs`
- [X] T005 [P] Create `apps/mcp-server/src/tools/ui_inspection.rs` with empty tool handler stubs for all 6 tools
- [X] T006 [P] Create `apps/desktop-ui/src/introspection_bridge.rs` with an empty `IntrospectionBridge` struct
- [X] T007 Run `cargo check --workspace` — must compile with zero errors before proceeding

**Checkpoint**: `cargo check --workspace` green with all new empty stubs.

---

## Phase 2: Foundational Types

**Purpose**: Implement the core data types (`StableWidgetId`, `WidgetNode`, `UiSnapshot`, etc.) and the `StableIdRegistry`. These are shared across all user stories.

**⚠️ CRITICAL**: No user story implementation can start until T008–T016 are complete.

- [X] T008 Implement `StableWidgetId(u64)` newtype and `WidgetKind` enum (`Button | TextEdit | Label | Panel | ScrollArea | Checkbox | Slider | Window | ComboBox | RadioButton | Other(String)`) — `crates/egui-introspection/src/widget_node.rs`
- [X] T009 Implement `StableIdRegistry` with `assign()` and `lookup()` methods (hash of `(WidgetKind, label, parent_id, ordinal)` → stable u64) — `crates/egui-introspection/src/stable_id.rs`
- [X] T010 Write `stable_id_same_widget_same_id` — same semantic tuple on two frames → same `StableWidgetId` — `crates/egui-introspection/src/stable_id.rs`
- [X] T011 [P] Write `stable_id_reorder_survives` — widget at ordinal 0 moved to ordinal 2 → same ID — `crates/egui-introspection/src/stable_id.rs`
- [X] T012 [P] Write `stable_id_duplicate_label_different_ordinal` — two `Button("Delete")` in same parent → two different IDs — `crates/egui-introspection/src/stable_id.rs`
- [X] T013 [P] Write `stable_id_zero_never_assigned` — `StableWidgetId(0)` is never returned by `assign()` — `crates/egui-introspection/src/stable_id.rs`
- [X] T014 [P] Implement `WidgetNode` struct (all fields per data-model.md) and `Rect`/`Color32` re-exports — `crates/egui-introspection/src/widget_node.rs`
- [X] T015 [P] Implement `InputState`, `LayoutPass` structs with `serde` derives — `crates/egui-introspection/src/layout.rs`
- [X] T016 [P] Implement `PaintCmd` and `PaintPrimitive` enum with `serde` derives — `crates/egui-introspection/src/paint.rs`
- [X] T017 [P] Implement `SnapshotDiff` and `WidgetStateDelta` structs with `serde` derives — `crates/egui-introspection/src/diff.rs`
- [X] T018 Implement `UiSnapshot` struct (all fields) + `IntrospectionStore` (`Arc<tokio::sync::RwLock<Option<UiSnapshot>>>` + `watch::Sender<u64>`) — `crates/egui-introspection/src/snapshot.rs`
- [X] T019 Run `cargo test -p egui-introspection stable_id` — all 4 `stable_id_*` tests pass

**Checkpoint**: `cargo test -p egui-introspection` compiles and `stable_id_*` tests pass.

---

## Phase 3: User Story 1 — Widget Location by AI Agent (Priority: P1) 🎯 MVP

**Goal**: AI can locate any visible labeled widget via the `ui_find_widget` MCP tool. End-to-end: egui frame → `IntrospectableUi` → `UiSnapshot` → MCP response.

**Independent Test**: After wrapping `egui::Ui` with `IntrospectableUi` for one headless frame that renders a `Button("Sort")`, calling `ui_find_widget("Sort")` returns a node with correct `rect` and `hovered` flag. Run with `cargo test -p egui-introspection find` and `cargo test -p mcp-server ui_find`.

### Tests for US1

- [X] T020 [P] [US1] Write `snapshot_widget_index_consistent` — after a synthetic frame, `widget_index.len() == widgets.len()` — `crates/egui-introspection/src/snapshot.rs`
- [X] T021 [P] [US1] Write `find_by_label_returns_all_matches` — two `Button("Delete")` in same frame → `find_by_label("Delete")` returns 2 nodes — `crates/egui-introspection/src/snapshot.rs`
- [X] T022 [P] [US1] Write `tool_ui_find_widget_returns_match` — synthetic `IntrospectionStore` with one `Button("Sort")` → MCP tool returns `total: 1` — `apps/mcp-server/src/tools/ui_inspection.rs`
- [X] T023 [P] [US1] Write `tool_ui_find_widget_no_match_empty` — query for unknown label → `matches: []`, no error — `apps/mcp-server/src/tools/ui_inspection.rs`

### Implementation for US1

- [X] T024 [US1] Implement `IntrospectableUi` wrapper struct that forwards all `egui::Ui` methods and collects each widget's `Response` into a per-frame registry — `crates/egui-introspection/src/ui.rs`
- [X] T025 [US1] Implement `IntrospectionContext` with `begin_frame()` and `end_frame(egui::FullOutput)` that builds a `UiSnapshot` from collected responses — `crates/egui-introspection/src/context.rs`
- [X] T026 [US1] Implement `UiSnapshot::find_by_label()` returning `Vec<&WidgetNode>` — `crates/egui-introspection/src/snapshot.rs`
- [X] T027 [US1] Implement `IntrospectionStore::publish()` (called by egui main loop after `end_frame`) and `IntrospectionStore::latest()` (called by MCP handlers) — `crates/egui-introspection/src/snapshot.rs`
- [X] T028 [US1] Implement `tool_ui_snapshot` and `tool_ui_find_widget` MCP handlers reading from `IntrospectionStore` — `apps/mcp-server/src/tools/ui_inspection.rs`
- [X] T029 [US1] Register `ui_snapshot` and `ui_find_widget` tools in `apps/mcp-server` main tool registry — `apps/mcp-server/src/main.rs`
- [X] T030 [US1] Integrate `IntrospectableUi` and `IntrospectionStore` into `apps/desktop-ui` frame loop — `apps/desktop-ui/src/introspection_bridge.rs`
- [X] T031 [US1] Run `cargo test -p egui-introspection` and `cargo test -p mcp-server ui_find` — all US1 tests pass

**Checkpoint**: `tool_ui_find_widget("Sort")` returns the correct `WidgetNode` in a headless test. End-to-end US1 validated.

---

## Phase 4: User Story 2 — Layout Problem Detection (Priority: P2)

**Goal**: AI can detect clipped and overflowing widgets via `ui_clipped_widgets`. `LayoutPass` and `PaintCmd` are captured each frame.

**Independent Test**: Render a panel whose child overflows its height. `snapshot.clipped_widgets()` returns the overflowing child. Run with `cargo test -p egui-introspection overflow`.

### Tests for US2

- [X] T032 [P] [US2] Write `overflow_detected_when_desired_exceeds_clip` — `desired_rect` taller than `clip_rect` → `LayoutPass.overflow == true` — `crates/egui-introspection/src/layout.rs`
- [X] T033 [P] [US2] Write `no_overflow_when_fits` — `desired_rect` fully inside `clip_rect` → `overflow: false` — `crates/egui-introspection/src/layout.rs`
- [X] T034 [P] [US2] Write `tool_ui_clipped_widgets_empty_when_none` — no clipped widgets in store → `total: 0` — `apps/mcp-server/src/tools/ui_inspection.rs`

### Implementation for US2

- [X] T035 [US2] Extend `IntrospectableUi::collect_response()` to capture `LayoutPass` per widget: compare `response.rect` vs `ui.clip_rect()` and compute `desired_rect` from `ui.max_rect()` — `crates/egui-introspection/src/ui.rs`
- [X] T036 [US2] Implement `PaintCmd` capture in `IntrospectionContext::end_frame()` — convert `FullOutput.shapes` `Vec<ClippedShape>` → `Vec<PaintCmd>` — `crates/egui-introspection/src/context.rs`
- [X] T037 [US2] Implement `UiSnapshot::clipped_widgets()` returning `Vec<&WidgetNode>` where `clipped == true` — `crates/egui-introspection/src/snapshot.rs`
- [X] T038 [US2] Implement `tool_ui_clipped_widgets` MCP handler — `apps/mcp-server/src/tools/ui_inspection.rs`
- [X] T039 [US2] Register `ui_clipped_widgets` tool in `apps/mcp-server` main tool registry — `apps/mcp-server/src/main.rs`
- [X] T040 [US2] Run `cargo test -p egui-introspection overflow` — all US2 tests pass

**Checkpoint**: `clipped_widgets()` correctly returns overflowing widgets in headless test.

---

## Phase 5: User Story 3 — Focus and Interaction State (Priority: P3)

**Goal**: AI can query which widget holds keyboard focus and whether a widget was clicked this frame via `ui_widget_info`.

**Independent Test**: Programmatically set egui focus to a `TextEdit`, capture snapshot, assert `snapshot.focused_widget()` returns the `TextEdit`'s `StableWidgetId`. Run with `cargo test -p egui-introspection focus`.

### Tests for US3

- [X] T041 [P] [US3] Write `focused_widget_at_most_one` — synthetic snapshot with exactly one `focused: true` node → `focused_widget()` returns `Some(that_id)` — `crates/egui-introspection/src/snapshot.rs`
- [X] T042 [P] [US3] Write `focused_widget_none_when_no_focus` — no focused node → `focused_widget()` returns `None` — `crates/egui-introspection/src/snapshot.rs`
- [X] T043 [P] [US3] Write `tool_ui_widget_info_not_found` — unknown ID → `{ "error": "widget_not_found" }` — `apps/mcp-server/src/tools/ui_inspection.rs`

### Implementation for US3

- [X] T044 [US3] Extend `IntrospectionContext::end_frame()` to query `egui::Context::memory()` for the focused widget id and populate `UiSnapshot.focused_widget` — `crates/egui-introspection/src/context.rs`
- [X] T045 [US3] Implement `UiSnapshot::focused_widget()` accessor returning `Option<&WidgetNode>` — `crates/egui-introspection/src/snapshot.rs`
- [X] T046 [US3] Implement `tool_ui_widget_info` MCP handler (returns full `WidgetNode` detail including `layout` section) — `apps/mcp-server/src/tools/ui_inspection.rs`
- [X] T047 [US3] Register `ui_widget_info` tool in `apps/mcp-server` main tool registry — `apps/mcp-server/src/main.rs`
- [X] T048 [US3] Run `cargo test -p egui-introspection focus` — all US3 tests pass

**Checkpoint**: `focused_widget()` returns the correct widget after a programmatic focus change.

---

## Phase 6: User Story 4 — Hierarchical Navigation (Priority: P4)

**Goal**: AI can traverse the widget tree parent-to-child and child-to-parent via `ui_children` MCP tool.

**Independent Test**: Render a `Panel > ScrollArea > Button` hierarchy. `snapshot.children_of(panel_id)` returns one entry (the scroll area), not the button. `parent_of(scroll_id)` returns the panel. Run with `cargo test -p egui-introspection hierarchy`.

### Tests for US4

- [X] T049 [P] [US4] Write `children_of_returns_direct_children_only` — panel containing scrollarea containing button → `children_of(panel)` returns `[scroll_id]`, not `button_id` — `crates/egui-introspection/src/snapshot.rs`
- [X] T050 [P] [US4] Write `parent_of_root_returns_none` — root window → `parent_of(root_id)` returns `None` — `crates/egui-introspection/src/snapshot.rs`

### Implementation for US4

- [X] T051 [US4] Extend `IntrospectableUi` to track the current parent `StableWidgetId` as widgets are nested (push/pop via `ui.push_id()`/scoped layout calls) — `crates/egui-introspection/src/ui.rs`
- [X] T052 [US4] Implement `UiSnapshot::children_of()` and `parent_of()` using `widget_index` + `WidgetNode.children`/`.parent` — `crates/egui-introspection/src/snapshot.rs`
- [X] T053 [US4] Implement `tool_ui_children` MCP handler — `apps/mcp-server/src/tools/ui_inspection.rs`
- [X] T054 [US4] Register `ui_children` tool in `apps/mcp-server` main tool registry — `apps/mcp-server/src/main.rs`
- [X] T055 [US4] Run `cargo test -p egui-introspection hierarchy` — all US4 tests pass

**Checkpoint**: `children_of` and `parent_of` correctly navigate a 3-level hierarchy in headless test.

---

## Phase 7: User Story 5 — Snapshot Diffing (Priority: P5)

**Goal**: AI can detect which widgets changed between consecutive frames (added, removed, state-changed) via `ui_snapshot_diff`.

**Independent Test**: Capture two consecutive snapshots; the second has a new `Window("Error")`. `UiSnapshot::diff(a, b).added` contains the error window's widgets. Run with `cargo test -p egui-introspection diff`.

### Tests for US5

- [X] T056 [P] [US5] Write `diff_identical_frames_empty` — diff two identical snapshots → `added`, `removed`, `changed` all empty — `crates/egui-introspection/src/diff.rs`
- [X] T057 [P] [US5] Write `diff_added_widget` — snapshot B has a widget not in A → appears in `diff.added` — `crates/egui-introspection/src/diff.rs`
- [X] T058 [P] [US5] Write `diff_removed_widget` — snapshot A has a widget not in B → appears in `diff.removed` — `crates/egui-introspection/src/diff.rs`
- [X] T059 [P] [US5] Write `diff_hovered_state_changed` — widget `hovered` flips true→false → in `diff.changed` with `hovered_changed: Some(false)` — `crates/egui-introspection/src/diff.rs`
- [X] T060 [P] [US5] Write `diff_reorder_not_add_remove` — widget changes ordinal position → in `changed` with `position_moved: true`, NOT in `added` or `removed` — `crates/egui-introspection/src/diff.rs`
- [X] T061 [P] [US5] Write `tool_ui_snapshot_diff_no_previous` — first frame (no `previous`) → `{ "error": "no_previous_frame" }` — `apps/mcp-server/src/tools/ui_inspection.rs`

### Implementation for US5

- [X] T062 [US5] Implement `UiSnapshot::diff(a: &UiSnapshot, b: &UiSnapshot) -> SnapshotDiff` — compares by `StableWidgetId`; uses reorder detection for `position_moved` — `crates/egui-introspection/src/diff.rs`
- [X] T063 [US5] Extend `IntrospectionStore` to retain `previous: Option<Arc<UiSnapshot>>` alongside `current` — `crates/egui-introspection/src/snapshot.rs`
- [X] T064 [US5] Implement `tool_ui_snapshot_diff` MCP handler — calls `UiSnapshot::diff(prev, curr)` — `apps/mcp-server/src/tools/ui_inspection.rs`
- [X] T065 [US5] Register `ui_snapshot_diff` tool in `apps/mcp-server` main tool registry — `apps/mcp-server/src/main.rs`
- [X] T066 [US5] Run `cargo test -p egui-introspection diff` — all 5 `diff_*` tests pass

**Checkpoint**: All 5 diff tests pass. `ui_snapshot_diff` MCP tool returns correct add/remove/change sets.

---

## Phase 8: Polish & Cross-Cutting

**Purpose**: Documentation, tracing, edge-case coverage, final validation.

- [X] T067 [P] Add `///` doc comments + usage example to all `pub` items in `crates/egui-introspection/src/lib.rs` (per Constitution VII)
- [X] T068 [P] Add `///` doc comments to all `pub` items in `snapshot.rs`, `widget_node.rs`, `context.rs`, `ui.rs`
- [X] T069 [P] Add `tracing::debug!` / `tracing::instrument` to `IntrospectionContext::end_frame()`, `IntrospectionStore::publish()`, and all 6 MCP tool handlers (Constitution VI observability gate)
- [X] T070 [P] Write edge-case test `deep_nesting_no_stack_overflow` — 25-level widget hierarchy → no panic during snapshot build — `crates/egui-introspection/src/snapshot.rs`
- [X] T071 [P] Write edge-case test `zero_size_widget_present_in_tree` — zero-size spacer → in snapshot with `rect.is_empty() == true` — `crates/egui-introspection/src/snapshot.rs`
- [X] T072 [P] Write edge-case test `snapshot_during_render_returns_last_complete` — `IntrospectionStore::latest()` never returns a partial frame — `crates/egui-introspection/src/snapshot.rs`
- [X] T073 Run `cargo test --workspace` — zero failures, all new tests pass
- [X] T074 Run `cargo clippy --workspace -- -D warnings` — zero lint warnings

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No deps — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 stubs compiling
- **US1–US5 (Phases 3–7)**: Depend on Phase 2 (foundational types); can proceed sequentially in priority order
- **Polish (Phase 8)**: Depends on all story phases complete

### User Story Dependencies

- **US1 (P1)**: Requires Phase 2 → MVP deliverable; independent of US2–US5
- **US2 (P2)**: Requires Phase 2 + US1 (`IntrospectableUi` must exist to extend); independent of US3–US5
- **US3 (P3)**: Requires Phase 2 + US1 (`IntrospectionContext` must exist); independent of US2, US4–US5
- **US4 (P4)**: Requires Phase 2 + US1 (parent tracking extends `IntrospectableUi`)
- **US5 (P5)**: Requires Phase 2 + US1 (snapshot infrastructure must exist); independent of US2–US4

### Parallel Opportunities

- All Phase 1 `[P]` tasks: T003–T006 in parallel
- All Phase 2 `[P]` tasks: T010–T013, T014–T017 in parallel after T008–T009
- Within each story phase: all `[P]`-marked test tasks can be written in parallel
- Stories US2, US3, US4, US5 can proceed in parallel after US1 `IntrospectableUi` exists (T024 complete)

---

## Implementation Strategy

### MVP (US1 only — phases 1–3)

1. Phase 1: Create crate skeleton (T001–T007)
2. Phase 2: Implement all foundational types (T008–T019)
3. Phase 3: Implement US1 — widget location via MCP (T020–T031)
4. **STOP**: Validate `ui_find_widget("Sort")` returns correct node in headless test
5. Demo: AI can locate any labeled widget

### Incremental Delivery

- US1 → widget location (MVP)
- US2 → layout/clipping detection
- US3 → focus/interaction state
- US4 → hierarchical navigation
- US5 → frame diffing
- Polish → observability, docs, edge cases

---

## Notes

- All unit tests use `egui::Context` in headless mode (no display required) — `cargo test` runs everywhere
- `[P]` = tasks writing to different files; safe to execute concurrently
- Each story phase should compile and test independently: `cargo test -p egui-introspection <filter>` per phase
- `StableIdRegistry` is the most novel component — T009–T013 must pass before any user story work
- MCP tools in `apps/mcp-server` follow the same registration pattern as existing debugger tools in `apps/mcp-server/src/main.rs`
