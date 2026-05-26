# Tasks: Visual Debugger UI

**Input**: Design documents from `specs/004-visual-debugger-ui/`

**Prerequisites**: plan.md ✅ | spec.md ✅ | research.md ✅ | data-model.md ✅ | contracts/ ✅

**Tests**: Included per plan.md Phase 2 test coverage section.

**Organization**: Grouped by user story to enable independent implementation and testing.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1–US4)

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Create workspace skeleton for new crate and new app binary.

- [X] T001 Add `crates/debug-session-view` and `apps/visual-debugger` to workspace `members` in `Cargo.toml`
- [X] T002 Add `walkdir = "2"` and `debug-session-view = { path = "crates/debug-session-view" }` to `[workspace.dependencies]` in `Cargo.toml`
- [X] T003 [P] Create `crates/debug-session-view/Cargo.toml` (lib crate, deps: `tokio` sync, `serde`, `tracing`, `thiserror`)
- [X] T004 [P] Create `crates/debug-session-view/src/lib.rs` skeleton with `mod action; mod state; mod view;` and empty re-exports
- [X] T005 [P] Create `apps/visual-debugger/Cargo.toml` (bin crate, deps: `egui`, `eframe`, `debug-session-view`, `win-debug-bridge`, `runtime-core`, `walkdir`, `tokio`, `tracing`, `tracing-subscriber`, `anyhow`)
- [X] T006 [P] Create `apps/visual-debugger/src/main.rs` minimal skeleton (empty `fn main()`)
- [X] T007 Verify `cargo build --workspace` compiles clean after skeleton creation

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: All shared types and the `DebugSessionView` bus that every user story depends on.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [X] T008 [P] Implement `ToolbarAction` enum (12 variants + `all()` iterator + `Display` + `FromStr`) in `crates/debug-session-view/src/action.rs`
- [X] T009 [P] Implement `DebugSessionState` enum (`Idle | Running | Paused | Terminated`) in `crates/debug-session-view/src/state.rs`
- [X] T010 Implement `BreakpointEntry { file: PathBuf, line: u32, resolved: bool }` in `crates/debug-session-view/src/state.rs`
- [X] T011 Implement `ThreadInfo { thread_id: u64, name: Option<String>, is_active: bool }` in `crates/debug-session-view/src/state.rs`
- [X] T012 Implement `DebugUIState` struct with all fields per data-model, plus `add_breakpoint()`, `remove_breakpoint()`, `press_action()`, `is_pressed()`, `set_error()`, `clear_error()` methods in `crates/debug-session-view/src/state.rs`
- [X] T013 Implement `DebugSessionView` (`Arc<tokio::sync::RwLock<DebugUIState>>` + `watch::Sender<u64>`) with `new()`, `publish()`, `latest_blocking()`, `latest()`, `subscribe()`, `clone()` in `crates/debug-session-view/src/view.rs`
- [X] T014 Wire all public re-exports in `crates/debug-session-view/src/lib.rs`; add `#![warn(missing_docs)]` and top-level doc comment with usage example
- [X] T015 [P] Unit tests `all_toolbar_actions_display` and `toolbar_action_roundtrip` in `crates/debug-session-view/src/action.rs`
- [X] T016 [P] Unit tests `debug_ui_state_new_is_idle`, `breakpoint_add_and_remove`, `recent_actions_press_stored`, `error_banner_set_and_clear` in `crates/debug-session-view/src/state.rs`
- [X] T017 [P] Unit tests `view_publish_notifies_watch`, `view_read_after_publish`, `view_clone_shares_arc` in `crates/debug-session-view/src/view.rs`
- [X] T018 Verify `cargo test -p debug-session-view` — all 9 tests pass

**Checkpoint**: `crates/debug-session-view` is complete. All user story implementation can now begin.

---

## Phase 3: User Story 1 — Manual Debug Session (Priority: P1) 🎯 MVP

**Goal**: A developer can launch the app, view a source file with syntax highlighting, set breakpoints by clicking the gutter, and step through code using the toolbar or keyboard shortcuts. Every button shows a 200ms pressed animation.

**Independent Test**: Launch `apps/visual-debugger --executable <binary>`. Open `src/main.rs`. Click gutter line 10 → red dot appears. Press F10 → Step Over button depresses for 200ms, yellow highlight advances one line.

### Syntax Tokenizer

- [X] T019 [P] [US1] Implement `TokenClass` enum (`Keyword | StringLiteral | CharLiteral | LineComment | BlockComment | TypeIdent | Other`) in `apps/visual-debugger/src/syntax.rs`
- [X] T020 [P] [US1] Implement `tokenize_line(src: &str) -> Vec<(TokenClass, &str)>` single-pass scanner with 52-keyword table in `apps/visual-debugger/src/syntax.rs`
- [X] T021 [P] [US1] Unit tests `tokenize_keyword`, `tokenize_string_literal`, `tokenize_line_comment`, `tokenize_block_comment`, `tokenize_type_ident`, `tokenize_empty_line` in `apps/visual-debugger/src/syntax.rs`
- [X] T022 [P] [US1] Unit test `tokenize_10k_lines_no_panic` (generate 10k Rust lines, assert no panic, elapsed < 100ms) in `apps/visual-debugger/src/syntax.rs`

### Source Viewer

- [X] T023 [US1] Implement `SourceLine { number, tokens, is_active, has_breakpoint, breakpoint_resolved }` and `SourceView::build_lines()` from file content + `DebugUIState` in `apps/visual-debugger/src/source_view.rs`
- [X] T024 [US1] Implement `SourceView::visible_range(scroll_offset, panel_height, row_height) -> Range<u32>` and `needs_scroll(active_line, range) -> bool` in `apps/visual-debugger/src/source_view.rs`
- [X] T025 [P] [US1] Unit tests `active_line_in_visible_range`, `active_line_outside_needs_scroll`, `breakpoint_flag_on_source_line` in `apps/visual-debugger/src/source_view.rs`
- [X] T026 [US1] Implement virtual-scroll egui widget: `ScrollArea` with fixed-height rows, invisible `allocate_space` for off-screen lines, painter calls for visible rows in `apps/visual-debugger/src/source_view.rs`
- [X] T027 [US1] Implement yellow active-line highlight (background color fill behind line tokens) and line number column in `apps/visual-debugger/src/source_view.rs`
- [X] T028 [US1] Implement gutter click handler: toggle `BreakpointEntry` on clicked line; render red filled circle for resolved dots, dimmed circle for unresolved, in `apps/visual-debugger/src/source_view.rs`
- [X] T029 [US1] Implement `SourceView::scroll_to_active()` — compute scroll offset to bring active line into viewport center, store in `ScrollArea` id state in `apps/visual-debugger/src/source_view.rs`

### Toolbar

- [X] T030 [US1] Implement `ToolbarButton` array (11 entries) with label, icon glyph (Unicode or embedded), shortcut string, and `KeyCombo` binding in `apps/visual-debugger/src/toolbar.rs`
- [X] T031 [US1] Implement `KeyCombo` enum (`Single` and `Chord` variants) and `ChordState` tracker for two-key sequences (Ctrl+R, F11 etc.) in `apps/visual-debugger/src/toolbar.rs`
- [X] T032 [US1] Implement `Toolbar::check_keyboard(ctx, state) -> Vec<ToolbarAction>` per-frame shortcut scanner in `apps/visual-debugger/src/toolbar.rs`
- [X] T033 [US1] Implement `Toolbar::render(ui, state) -> Vec<ToolbarAction>` — horizontal button strip with hover tooltips showing label + shortcut, pressed visual override when `state.is_pressed(action)` in `apps/visual-debugger/src/toolbar.rs`
- [X] T034 [P] [US1] Unit tests `animation_active_within_200ms`, `animation_cleared_after_200ms`, `all_shortcuts_unique` in `apps/visual-debugger/src/toolbar.rs`

### Address Bar (basic display for US1)

- [X] T035 [P] [US1] Implement `AddressBarState` struct and `AddressBar::render_display(ui, path)` (read-only path label) in `apps/visual-debugger/src/address_bar.rs`
- [X] T036 [P] [US1] Unit test `address_bar_shows_active_file` in `apps/visual-debugger/src/address_bar.rs`

### App Wiring (US1)

- [X] T037 [US1] Implement `VisualDebuggerApp` struct (fields: `DebugSessionView`, `WindowsDebugHandle`, `SourceView`, `AddressBarState`, `ChordState`) and `eframe::App::update()` skeleton in `apps/visual-debugger/src/app.rs`
- [X] T038 [US1] Wire toolbar actions → `win-debug-bridge` commands (step_over, step_into, step_out, continue, stop, restart) in `apps/visual-debugger/src/app.rs`
- [X] T039 [US1] Wire debug bridge frame-change callback → `DebugSessionView::publish()` (updates `active_file`, `active_line`, `session_state`) in `apps/visual-debugger/src/app.rs`
- [X] T040 [US1] Wire `DebugSessionView` read → `SourceView::build_lines()` each frame; call `scroll_to_active()` when active line changes in `apps/visual-debugger/src/app.rs`
- [X] T041 [US1] Implement `main.rs` full entry point: parse `--executable` CLI arg, create `WindowsDebugHandle`, create `DebugSessionView`, launch eframe with `VisualDebuggerApp` in `apps/visual-debugger/src/main.rs`
- [X] T042 [US1] Verify `cargo test -p visual-debugger` — all US1 unit tests pass

**Checkpoint**: Manual debugging is fully functional. Human can drive a debug session end-to-end.

---

## Phase 4: User Story 2 — AI Agent Observation (Priority: P2)

**Goal**: Every AI agent debug action (step, breakpoint, file switch) is reflected live in the UI — button pressed animation, gutter dot change, yellow highlight moves — indistinguishable from a human action.

**Independent Test**: Inject `ToolbarAction::StepOver` into `DebugSessionView` from a test thread. Assert `app.is_pressed(StepOver) == true` within one frame. Inject `set_breakpoint(src/main.rs, 42)`. Assert red dot appears in gutter at line 42.

- [X] T043 [US2] Add `debug-session-view` dependency to `apps/mcp-server/Cargo.toml`
- [X] T044 [US2] Add `DebugSessionView` parameter to `mcp-server`'s `run_with_store`-equivalent entry point; thread it through to handler dispatch in `apps/mcp-server/src/server.rs`
- [X] T045 [P] [US2] Add `DebugSessionView::publish_action(ToolbarAction)` helper method that writes `recent_actions` timestamp and sends watch notification in `crates/debug-session-view/src/view.rs`
- [X] T046 [P] [US2] Write `ToolbarAction::StepOver/StepInto/StepOut/Continue` into `DebugSessionView` after each corresponding MCP execution handler returns in `apps/mcp-server/src/handlers/execution.rs`
- [X] T047 [P] [US2] Write `ToolbarAction` and breakpoint change into `DebugSessionView` after each MCP breakpoint handler in `apps/mcp-server/src/handlers/breakpoints.rs`
- [X] T048 [US2] Spawn in-process MCP server Tokio thread from `apps/visual-debugger/src/main.rs` (same pattern as `apps/desktop-ui`), passing shared `DebugSessionView` and `WindowsDebugHandle`
- [X] T049 [US2] Verify `cargo test -p mcp-server` passes with no regressions after `DebugSessionView` injection
- [X] T050 [US2] Verify `cargo test --workspace` clean

**Checkpoint**: AI-driven actions animate the same buttons and gutter dots as human actions.

---

## Phase 5: User Story 3 — File Tree Navigation (Priority: P3)

**Goal**: A developer browses the project directory in the left panel, expands/collapses directories, clicks a file to open it in the source viewer, independent of the debug frame.

**Independent Test**: Launch app. Expand `src/` in file tree. Click `lib.rs`. Source viewer loads file with line numbers and no yellow highlight. When debug frame changes to `main.rs`, file tree highlights `main.rs` entry.

- [X] T051 [P] [US3] Implement `FileTreeNode { path, name, kind, children }` and `FileTreeKind` enum in `apps/visual-debugger/src/file_tree.rs`
- [X] T052 [US3] Implement background directory scanner: `std::thread::spawn` with `walkdir::WalkDir`, populates `Mutex<Vec<FileTreeNode>>` for root children in `apps/visual-debugger/src/file_tree.rs`
- [X] T053 [US3] Implement `FileTree::render(ui, active_file) -> Option<PathBuf>` using `egui::CollapsingHeader` for directories, `ui.selectable_label` for files; highlight active file in `apps/visual-debugger/src/file_tree.rs`
- [X] T054 [US3] Implement lazy expand: on first `CollapsingHeader` open, trigger background scan of that directory's children in `apps/visual-debugger/src/file_tree.rs`
- [X] T055 [US3] Wire file tree selection → load file into `SourceView` in `apps/visual-debugger/src/app.rs`
- [X] T056 [US3] Wire `DebugSessionView.active_file` change → scroll file tree to highlight newly active file in `apps/visual-debugger/src/app.rs`

**Checkpoint**: File tree navigation works independently of debug state. Active file tracks the debug frame.

---

## Phase 6: User Story 4 — Address Bar Full Navigation (Priority: P4)

**Goal**: The address bar shows the current file path and accepts typed/pasted paths for direct navigation, with inline error feedback for invalid paths.

**Independent Test**: Type a valid absolute path in the address bar, press Enter. Source viewer loads the file. Type a non-existent path, press Enter. Inline error appears in the bar; current file remains visible.

- [X] T057 [US4] Extend `AddressBar` to support editable mode: focus click → text input replaces label display in `apps/visual-debugger/src/address_bar.rs`
- [X] T058 [US4] Implement Enter key handler: validate path exists → load file in source viewer; else set `AddressBarState.error` in `apps/visual-debugger/src/address_bar.rs`
- [X] T059 [US4] Render inline error text below address bar when `AddressBarState.error` is `Some` in `apps/visual-debugger/src/address_bar.rs`
- [X] T060 [P] [US4] Unit tests `address_bar_invalid_path_sets_error`, `address_bar_valid_path_clears_error` in `apps/visual-debugger/src/address_bar.rs`
- [X] T061 [US4] Wire address bar path submission → `SourceView` file load in `apps/visual-debugger/src/app.rs`

**Checkpoint**: All four user stories are independently functional and testable.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Edge case handling, observability, and final validation.

- [X] T062 [P] Add `tracing::instrument` to `DebugSessionView::publish()` and state-mutating methods in `crates/debug-session-view/src/view.rs`
- [X] T063 Call `ctx.request_repaint_after(Duration::from_millis(200))` whenever a `ToolbarAction` is pressed, ensuring animation clears without user input in `apps/visual-debugger/src/app.rs`
- [X] T064 Edge case: active line outside viewport — verify `SourceView::scroll_to_active()` fires on every frame-change event in `apps/visual-debugger/src/source_view.rs`
- [X] T065 Edge case: source file > 10,000 lines — manual smoke test virtual scroll; assert frame time stays < 16ms in `apps/visual-debugger/src/source_view.rs`
- [X] T066 Edge case: binary not found at launch — show `error_banner` and `session_state = Idle` instead of panic in `apps/visual-debugger/src/main.rs`
- [X] T067 Edge case: step-back commands return "unsupported" from bridge — set `error_banner` with friendly message; do not crash in `apps/visual-debugger/src/app.rs`
- [X] T068 Edge case: multiple rapid AI actions (< 50ms apart) — verify each button animates without skipping (each has its own `Instant` entry, not overwritten) in `apps/visual-debugger/src/toolbar.rs`
- [X] T069 [P] Add `///` doc comments to all `pub` items in `crates/debug-session-view/src/lib.rs`, `state.rs`, `action.rs`, `view.rs`
- [X] T070 `cargo test --workspace` — all tests pass, 0 ignored failures, no new warnings

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 completion — BLOCKS all user stories
- **US1 (Phase 3)**: Depends on Phase 2 — the MVP; no dependency on US2/US3/US4
- **US2 (Phase 4)**: Depends on Phase 3 (needs the running app and MCP wiring baseline)
- **US3 (Phase 5)**: Depends on Phase 3 (needs `SourceView` load API)
- **US4 (Phase 6)**: Depends on Phase 3 (needs `SourceView` and `AddressBarState` baseline)
- **Polish (Phase 7)**: Depends on all desired user stories complete

### Within Each Phase

- `[P]` tasks within a phase touch different files — run in parallel
- Non-`[P]` tasks within a phase must be sequential (depend on prior output)
- Tests always written before implementation where both appear in same sub-group

### Parallel Opportunities

```bash
# Phase 2 — all parallel after T009-T012 sequential chain:
T015 (action tests) || T016 (state tests) || T017 (view tests)

# Phase 3 — syntax tokenizer and toolbar have no shared files:
T019-T022 (syntax.rs) || T030-T034 (toolbar.rs) || T035-T036 (address_bar.rs)

# Phase 3 — source_view internal:
T023 → T024 → T025 (sequential, share file)
T026 → T027 → T028 → T029 (sequential, share file)
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (T001–T007)
2. Complete Phase 2: Foundational (T008–T018) — CRITICAL BLOCKER
3. Complete Phase 3: User Story 1 (T019–T042)
4. **STOP and VALIDATE**: Launch app, step through a binary manually
5. Demo if ready

### Incremental Delivery

1. Phase 1 + 2 → Foundation ready
2. + Phase 3 → Manual debug session works → **MVP**
3. + Phase 4 → AI agent actions animate live
4. + Phase 5 → File tree navigation
5. + Phase 6 → Address bar navigation
6. + Phase 7 → Polish

---

## Notes

- `[P]` tasks touch different files and have no inter-task state dependency
- Each user story phase is independently completable and testable
- `cargo test -p debug-session-view` after T018; `cargo test -p visual-debugger` after T042; `cargo test --workspace` after T070
- Commit after each phase checkpoint
- Total tasks: **70** across 7 phases
