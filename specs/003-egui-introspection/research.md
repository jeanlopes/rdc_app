# Research: egui UI Introspection Layer

**Feature**: `003-egui-introspection` | **Date**: 2026-05-22

---

## Decision 1: Widget Enumeration Strategy

**Decision**: Wrap `egui::Ui` with an `IntrospectableUi` proxy that intercepts every widget call and records the returned `Response` (rect, id, hovered, clicked). Supplement with `Context::memory()` post-frame for persistent state (focus, drag). The developer replaces only the root `Ui` construction — all child widget calls are unchanged.

**Rationale**: egui is a pure immediate-mode framework with no persistent widget registry. Widgets are re-created every frame; the only record of a widget's existence is its `Response`. Every `ui.button()`, `ui.text_edit_singleline()`, etc. returns a `Response`. By wrapping `Ui` and collecting responses, we capture the complete widget tree without touching individual call sites. `Context::memory()` provides the persistent interaction state (focus, scroll positions) that `Response` does not carry between frames.

**Alternatives considered**:
- Post-process `FullOutput.shapes` only — rejects because `ClippedShape` data lacks labels, IDs, and interaction flags; reconstructing semantics from primitives is brittle.
- Fork egui to add a native widget registry — rejects because it violates the zero-external-runtime-dependency principle and creates an unmanageable maintenance burden.

---

## Decision 2: Stable Widget Identity

**Decision**: Assign stable semantic `WidgetId`s derived from `(widget_kind, label_text, parent_stable_id, ordinal_within_parent)`. A per-session `StableIdRegistry` maps this 4-tuple to a persistent `u64`. On each frame, walk the collected responses and match by content, not by position. Widgets that reorder retain the same semantic ID; the diff records a position change rather than remove+add.

**Rationale**: egui's native `Id` is a hash of widget type and source-code location. For dynamic lists, list-item position changes cause ID churn — identical semantic widgets get different native IDs each frame. Semantic ID derivation from (kind, label, parent, ordinal) is position-independent for ordering changes while remaining deterministic. Ordinal-within-parent handles the ambiguous case of two `Delete` buttons in the same container.

**Alternatives considered**:
- Use egui's native `Id` directly — rejects because list reordering causes spurious remove+add diffs, undermining the AI's ability to track widgets across frames.
- Require developer annotation of stable IDs — rejects because the spec requires zero changes to widget call sites; fully automatic semantic identity is the goal.

---

## Decision 3: PaintCmd Capture

**Decision**: Capture `FullOutput.shapes: Vec<ClippedShape>` returned by `Context::end_frame()`. Convert each `epaint::Shape` variant to a simplified `PaintCmd` enum: `Rect`, `Text`, `Circle`, `Path`, `Mesh`, `Other`. Store the `clip_rect` from each `ClippedShape`. This capture happens after the egui frame closes, with no rendering interference.

**Rationale**: `FullOutput.shapes` is the authoritative, complete render stream in egui 0.23+. All visible content originates here before tessellation. The `Shape` enum has 11 variants; we abstract to 6 for AI-readable output. `Text` shapes carry the text string, enabling cross-reference with `WidgetNode.label`. Capturing post-`end_frame()` means no impact on the rendering hot path.

**Alternatives considered**:
- Inspect tessellated triangles from `Context::tessellate()` — rejects because tessellation destroys shape semantics; distinguishing a button's rectangle from a background rectangle requires heuristics.

---

## Decision 4: LayoutPass Capture

**Decision**: Record each widget's `Response.rect` (the allocated, potentially clipped rect) alongside the parent `Ui::clip_rect()` at the time of the call. Clipping is detected by comparing `response.rect` against `ui.clip_rect()`: if `response.rect` extends outside the clip rect, mark `clipped: true` and store both the desired rect (approximated as `response.rect` before intersection) and the clipped rect. Pre-clip desired rect is approximated by tracking child allocations in `Ui::max_rect()`.

**Rationale**: egui's layout is single-pass immediate mode — there is no explicit "desired vs. allocated" separation exposed in the public API. `Response.rect` is always the post-clip allocated rect. However, `Ui::max_rect()` gives the theoretical layout space, and `Ui::clip_rect()` gives the clipping boundary. Comparing `response.rect` to the parent clip rect is sufficient to detect overflow for the spec's layout-analysis requirements. This is non-invasive and captures necessary data from within the `IntrospectableUi` wrapper.

**Alternatives considered**:
- Instrument `egui::Ui::allocate_rect()` directly — rejects because it requires forking egui; the `Response`-based approach is non-invasive.

---

## Decision 5: Async/Sync Bridge (egui main thread + Tokio MCP server)

**Decision**: Run the Tokio runtime on a dedicated background thread via `std::thread::spawn(|| Runtime::new().unwrap().block_on(...))`. Share the latest snapshot via `Arc<tokio::sync::RwLock<Option<UiSnapshot>>>`. The egui main loop calls `store.blocking_write()` after each `end_frame()` to publish the completed snapshot. Tokio async MCP handlers call `store.read().await` on demand. A `tokio::sync::watch` channel signals new-frame availability to avoid MCP handlers busy-polling.

**Rationale**: The egui event loop blocks the main OS thread; running Tokio on the main thread would deadlock. `tokio::sync::RwLock` is async-aware: `blocking_write()` is safe to call from synchronous code without deadlocking the Tokio executor. `Arc` provides shared ownership across threads. The `watch` channel pattern (producer writes, consumers subscribe) is idiomatic for "latest value with change notification" — exactly the per-frame snapshot use case. This pattern is documented in egui GitHub discussions (#521) as the standard Tokio/egui in-process approach.

**Alternatives considered**:
- `std::sync::RwLock` — rejects because mixing with `tokio::sync` methods across thread boundaries requires careful lock ordering to avoid deadlocks; `tokio::sync::RwLock` is the safe choice when the consumer is async.
- Separate processes with IPC — rejects because the clarification explicitly chose in-process; IPC adds latency and complexity incompatible with the < 10ms query target.

---

## Decision 6: MCP Tool Surface Design

**Decision**: Add a `ui_inspection` module to `apps/mcp-server` exposing six MCP tools: `ui_snapshot` (full tree), `ui_find_widget` (by label/kind), `ui_widget_info` (single node by ID), `ui_children` (children of a node), `ui_clipped_widgets` (all clipped nodes), and `ui_snapshot_diff` (diff between last two snapshots). The `IntrospectionStore` held by `crates/egui-introspection` is passed to the MCP server as a shared dependency.

**Rationale**: Six tools maps 1:1 to the acceptance criteria in the spec (locate, inspect, hierarchy, clipping, diff). Keeping the tool count small reduces AI token overhead per interaction. `ui_find_widget` returns a list to handle duplicate labels. `ui_snapshot_diff` requires storing the previous snapshot in the `IntrospectionStore` alongside the current one — a minor extension to the data model.

**Alternatives considered**:
- Single `ui_snapshot` tool returning the full tree every call — rejects because large widget trees would exceed MCP message size limits and waste AI context on irrelevant widgets.
- One tool per user story — rejects because fine-grained tools (15+) increase AI tool selection complexity; six focused tools cover all use cases.

---

## Summary Table

| Decision | Chosen Approach | Key Constraint Met |
|----------|-----------------|--------------------|
| Widget enumeration | `IntrospectableUi` wrapper | No widget call-site changes |
| Stable ID | Semantic (kind + label + parent + ordinal) | Survives list reorders |
| PaintCmd | `FullOutput.shapes` post-`end_frame()` | Zero render overhead |
| LayoutPass | `response.rect` vs `ui.clip_rect()` | Non-invasive, no egui fork |
| Async bridge | Tokio on background thread + `Arc<tokio::sync::RwLock>` + `watch` channel | <10ms MCP query latency |
| MCP tools | 6 focused tools in `apps/mcp-server` | Constitution III (MCP contract) |
