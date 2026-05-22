# Implementation Plan: egui UI Introspection Layer

**Branch**: `003-egui-introspection` | **Date**: 2026-05-22 | **Spec**: specs/003-egui-introspection/spec.md

> **Note**: Implementation is the next step after this plan. All design decisions are documented here and in `research.md`. No production code changes are made by this plan document.

## Summary

Create `crates/egui-introspection` — a Rust crate that instruments an egui application to produce a **semantic UI tree** (`UiSnapshot`) after every rendered frame. The snapshot exposes every widget's identity, kind, label, position, interaction state, and parent-child relationships. A stable semantic ID system survives dynamic list reorders. Six MCP tools in `apps/mcp-server` expose snapshot queries to the AI agent in-process. The egui render loop and MCP server share state via `Arc<tokio::sync::RwLock<UiSnapshot>>` with a `watch` channel for frame-completion notification.

## Technical Context

**Language/Version**: Rust stable (MSRV 1.75, declared in workspace `Cargo.toml`)

**Primary Dependencies**:
- `egui` — version already in workspace (UI framework being instrumented)
- `tokio` — async runtime for MCP server (already in workspace)
- `rmcp` — MCP protocol SDK (already in workspace, used by `apps/mcp-server`)
- `serde` + `serde_json` — snapshot serialization for MCP responses
- `tracing` — structured logging per Constitution VI

**Storage**: N/A — snapshots live in `Arc<tokio::sync::RwLock<Option<UiSnapshot>>>` in process memory; only the current and previous frame are retained.

**Testing**: `cargo test -p egui-introspection` for unit tests; `cargo test --workspace` for integration. Tests use synthetic `egui::Context` without a real window (egui supports headless rendering).

**Target Platform**: Windows 10/11 x86-64 (no Linux/macOS per Constitution)

**Project Type**: Library crate (`crates/egui-introspection`) + MCP tool module extension (`apps/mcp-server`)

**Performance Goals**:
- Snapshot capture: < 2ms overhead per frame (SC-002)
- MCP tool query: < 10ms from request to response (SC-001)
- Widget lookup by label: O(n) scan, < 10ms for trees up to 1000 widgets

**Constraints**:
- Zero-cost when disabled: `#[cfg(feature = "introspection")]` compile-time opt-in; no overhead when feature is off
- No egui fork: all instrumentation via the public `Ui` / `Context` API
- No IPC: MCP server runs in the same process as the egui application
- `unsafe` allowed only for the three-condition proof (Constitution VI); none expected in this crate

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Runtime Intelligence | ✅ PASS | Semantic widget tree gives AI ground truth about UI state; no vision or inference required |
| II. Crate-First Modularity | ✅ PASS | New standalone crate `crates/egui-introspection`; single responsibility (capture + query API). Already listed in constitution workspace layout. |
| III. MCP as AI-Debugger Contract | ✅ PASS | All AI queries go through 6 MCP tools in `apps/mcp-server`. `egui-introspection` crate does NOT expose direct methods to the AI — only to the MCP handler. |
| IV. Deterministic Replay | ⚠️ DEFERRED | Phase 3 does not include trace storage. `UiSnapshot` is designed to be serializable for future trace integration. Not a blocker. |
| V. Autonomous Agent Discipline | ⚠️ DEFERRED | Not applicable — this is infrastructure, not agent logic. |
| VI. Rust Safety First | ✅ PASS | Pure Rust + egui public API. No `unsafe` expected. Zero external runtime deps. |
| VII. Open Platform Foundation | ✅ PASS | All `pub` items in `crates/egui-introspection` will carry `///` doc comments and examples. |

**Gate result**: PASS

## Project Structure

### Documentation (this feature)

```text
specs/003-egui-introspection/
├── plan.md              ← this file
├── research.md          ← Phase 0 decisions (6 technical decisions)
├── data-model.md        ← Phase 1 entities and relationships
├── contracts/
│   └── mcp-ui-tools.md  ← 6 MCP tool schemas
└── tasks.md             ← Phase 2 output (/speckit-tasks command)
```

### Source Code

```text
crates/egui-introspection/
├── Cargo.toml
└── src/
    ├── lib.rs             # Re-exports; crate root with pub API + doc examples
    ├── context.rs         # IntrospectionContext — wraps per-frame capture lifecycle
    ├── ui.rs              # IntrospectableUi — wraps egui::Ui; intercepts widget calls
    ├── snapshot.rs        # UiSnapshot, IntrospectionStore
    ├── widget_node.rs     # WidgetNode, WidgetKind, StableWidgetId
    ├── stable_id.rs       # StableIdRegistry — semantic ID assignment
    ├── diff.rs            # SnapshotDiff, WidgetStateDelta; UiSnapshot::diff()
    ├── layout.rs          # LayoutPass — pre-clip/post-clip rect capture
    └── paint.rs           # PaintCmd, PaintPrimitive — from FullOutput.shapes

apps/mcp-server/src/tools/
└── ui_inspection.rs       # 6 MCP tool handlers (ui_snapshot, ui_find_widget, etc.)

apps/desktop-ui/src/
└── introspection_bridge.rs # IntrospectionStore init + egui frame-end hook
```

**Structure Decision**: Single new crate + extension to existing `apps/mcp-server`. No new apps binary required. `apps/desktop-ui` gains a thin integration module that wires `IntrospectableUi` into its existing egui `eframe` / `App` implementation.

## Phase 0: Research (Complete)

See `research.md` for all 6 decisions. Summary:

| Unknown | Resolution |
|---------|------------|
| Widget enumeration without call-site changes | `IntrospectableUi` wrapper collects `Response` objects |
| Stable ID across dynamic list reorders | Semantic hash: (kind, label, parent_id, ordinal) |
| PaintCmd capture | `FullOutput.shapes` post-`end_frame()` |
| LayoutPass (pre-clip rect) | `response.rect` vs `ui.clip_rect()` comparison |
| Async/sync bridge | Tokio on background thread + `Arc<tokio::sync::RwLock>` + `watch` channel |
| MCP tool surface | 6 tools in `apps/mcp-server`; see `contracts/mcp-ui-tools.md` |

## Phase 1: Design (Complete)

Artifacts generated:
- `data-model.md` — 10 types: `UiSnapshot`, `WidgetNode`, `StableWidgetId`, `WidgetKind`, `SnapshotDiff`, `WidgetStateDelta`, `LayoutPass`, `PaintCmd`, `InputState`, `IntrospectionStore`
- `contracts/mcp-ui-tools.md` — 6 MCP tools with full input/output schemas

## Phase 2: Test Coverage

### Strategy

Tests are split into two tiers following the same pattern as `002-pdb-stepping-panic`:

- **Unit tests** — `#[test]` in each source file, no real egui window. Use `egui::Context` in headless mode (egui supports `Context::run()` without a display).
- **Integration tests** — `#[test] #[ignore]` requiring `apps/desktop-ui` running; used to verify in-process MCP snapshot retrieval.

### crates/egui-introspection — unit test coverage

#### `stable_id.rs`
| Test | Assertion |
|------|-----------|
| `stable_id_same_widget_same_id` | Same (kind, label, parent, ordinal) on two consecutive frames → same `StableWidgetId` |
| `stable_id_reorder_survives` | Widget moved from ordinal 0 to ordinal 2 → same ID as before |
| `stable_id_duplicate_label_different_ordinal` | Two `Button("Delete")` in same parent → different IDs via ordinal tiebreaker |
| `stable_id_zero_never_assigned` | No assigned ID equals `StableWidgetId(0)` |

#### `diff.rs`
| Test | Assertion |
|------|-----------|
| `diff_identical_frames_empty` | Two identical snapshots → `added`, `removed`, `changed` all empty |
| `diff_added_widget` | Widget in `b` not in `a` → appears in `added` |
| `diff_removed_widget` | Widget in `a` not in `b` → appears in `removed` |
| `diff_hovered_state_changed` | `hovered` flips true→false → appears in `changed` with `hovered_changed: Some(false)` |
| `diff_reorder_not_add_remove` | Widget changes ordinal position → in `changed` with `position_moved: true`; NOT in `added` or `removed` |

#### `layout.rs`
| Test | Assertion |
|------|-----------|
| `overflow_detected_when_desired_exceeds_clip` | `desired_rect` taller than `clip_rect` → `overflow: true` |
| `no_overflow_when_fits` | `desired_rect` fully inside `clip_rect` → `overflow: false` |

#### `snapshot.rs`
| Test | Assertion |
|------|-----------|
| `snapshot_widget_index_consistent` | `widget_index.len() == widgets.len()` after synthetic frame |
| `find_by_label_returns_all_matches` | Two buttons with same label → both returned |
| `focused_widget_at_most_one` | Synthetic snapshot with two `focused: true` nodes → error / invariant violation |
| `clipped_widgets_list` | Three widgets, one clipped → `clipped_widgets()` returns exactly one |

#### `diff.rs` — hierarchy
| Test | Assertion |
|------|-----------|
| `children_of_returns_direct_children_only` | Panel with nested scroll + button → `children_of(panel)` returns scroll; button not included |
| `parent_of_root_returns_none` | Root window → `parent_of(root)` returns `None` |

### apps/mcp-server — ui_inspection tools

| Test | Assertion |
|------|-----------|
| `tool_ui_find_widget_returns_match` | Synthetic store with a `Button("Sort")` → `ui_find_widget("Sort")` returns 1 match |
| `tool_ui_find_widget_no_match_empty` | Query for unknown label → `matches: []`, no error |
| `tool_ui_widget_info_not_found` | Unknown ID → `{ "error": "widget_not_found" }` |
| `tool_ui_clipped_widgets_empty_when_none` | No clipped widgets → `total: 0` |
| `tool_ui_snapshot_diff_no_previous` | First frame → `{ "error": "no_previous_frame" }` |

## Acceptance Criteria Mapping

| Spec Criterion | Test Coverage |
|----------------|---------------|
| AI locates visible labeled widget (SC-001, US1) | `tool_ui_find_widget_returns_match` + latency benchmark |
| Clipping detection (US2, SC-003) | `overflow_detected_*` + `tool_ui_clipped_widgets_*` |
| Focus state (US3, SC-004) | `tool_ui_find_widget` with `focused: true` |
| Hierarchy navigation (US4) | `children_of_*` + `parent_of_*` |
| Snapshot diff (US5, SC-006) | All `diff_*` tests |
| MCP tools accessible (FR-011, FR-012) | All `tool_*` tests |
| Stable ID across reorders | `stable_id_reorder_survives` |

## Assumptions

- `egui` version in `Cargo.toml` is ≥ 0.23 (provides `FullOutput.shapes` and `Context::memory()` API).
- `apps/desktop-ui` already exists and uses `eframe`; the integration module adds < 20 lines to its `App::update()` method.
- The `IntrospectableUi` wrapper is constructed once per `App::update()` call, wrapping the root `egui::Ui` provided by `eframe`.
- The Tokio runtime for the MCP server is already running (from Phase 1, `apps/mcp-server`); Phase 3 adds the `IntrospectionStore` as a new dependency injected at startup.
