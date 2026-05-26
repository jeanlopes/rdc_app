# Implementation Plan: Visual Debugger UI

**Branch**: `005-visual-debugger-ui` | **Date**: 2026-05-24 | **Spec**: specs/004-visual-debugger-ui/spec.md

> **Note**: Implementation is the next step after this plan. All design decisions are documented in `research.md`. No production code is written by this plan.

## Summary

Create `apps/visual-debugger` — an egui desktop application that renders a source file with line numbers, syntax colouring, breakpoint gutter dots, and a yellow active-line highlight. An 11-button toolbar drives the debug session via the existing `win-debug-bridge`. All button actions animate with a 200ms pressed effect whether triggered by a human or the AI agent. A new `crates/debug-session-view` crate provides the shared in-process state bus (same `Arc<RwLock> + watch` pattern as `crates/egui-introspection`). The file tree panel and address bar round out the navigation surface.

---

## Technical Context

**Language/Version**: Rust stable (MSRV 1.75, declared in workspace `Cargo.toml`)

**Primary Dependencies**:
- `egui` 0.29 + `eframe` 0.29 — already in workspace
- `win-debug-bridge` — existing debug bridge crate (shared `WindowsDebugHandle`)
- `runtime-core` — session state machine already defined
- `walkdir` — directory traversal for file tree (new workspace dep)
- `tokio` (sync features) — `Arc<RwLock>` + `watch` channel for `DebugSessionView`
- `tracing` — per Constitution VI

**Storage**: N/A — no persistence; `DebugUIState` lives in process memory only.

**Testing**: `cargo test -p debug-session-view` for crate unit tests; `cargo test -p visual-debugger` for rendering smoke tests (headless egui `Context`).

**Target Platform**: Windows 10/11 x86-64.

**Project Type**: Library crate (`crates/debug-session-view`) + desktop binary (`apps/visual-debugger`).

**Performance Goals**:
- Render loop: 60 fps with a 10,000-line source file open (virtual scroll).
- Active-line scroll: < 100ms from frame-change event to line visible in viewport (SC-003).
- File tree expand: < 100ms for any directory node (background scan, SC-004).
- Button animation: appears within one frame (< 16ms) of activation (SC-002).

**Constraints**:
- Zero additional C/FFI dependencies beyond what `win-debug-bridge` already uses.
- `unsafe` only in `win-debug-bridge` (already scoped); none expected in new code.
- Binary size budget: +5 MB over baseline (no `syntect`).

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Runtime Intelligence | ✅ PASS | UI surfaces live execution state (active line, breakpoints, thread list) directly from the debug bridge; not a passive screenshot or log viewer. |
| II. Crate-First Modularity | ✅ PASS | All shared capability (DebugUIState, DebugSessionView, ToolbarAction) lives in `crates/debug-session-view`. Rendering logic stays in `apps/visual-debugger`. No capability siloed in the app binary. |
| III. MCP as AI-Debugger Contract | ✅ PASS | The AI agent issues debug commands through MCP tools in `apps/mcp-server`. The UI receives the *reflected* effect via `DebugSessionView` writes from the MCP handler — the UI never receives AI commands directly. |
| IV. Deterministic Replay | ⚠️ DEFERRED | Phase 4 does not add trace recording. `DebugUIState` is designed to be serializable for future replay integration. Not a blocker. |
| V. Autonomous Agent Discipline | ⚠️ N/A | This is UI infrastructure, not agent logic. |
| VI. Rust Safety First | ✅ PASS | No new `unsafe`. Hand-rolled tokenizer is pure safe Rust. `walkdir` is pure Rust. `egui`/`eframe` already in workspace. |
| VII. Open Platform Foundation | ✅ PASS | All `pub` items in `crates/debug-session-view` will carry `///` doc comments. |

**Gate result**: PASS

---

## Project Structure

### Documentation (this feature)

```text
specs/004-visual-debugger-ui/
├── plan.md              ← this file
├── research.md          ← Phase 0 decisions (7 technical decisions)
├── data-model.md        ← Phase 1 entities
├── contracts/
│   └── toolbar-actions.md  ← action contract: keyboard map + AI event format
└── tasks.md             ← Phase 2 output (/speckit-tasks)
```

### Source Code

```text
crates/debug-session-view/
├── Cargo.toml
└── src/
    ├── lib.rs            # re-exports; crate root with pub API + doc examples
    ├── state.rs          # DebugUIState, DebugSessionState, BreakpointEntry, ThreadInfo
    ├── action.rs         # ToolbarAction enum + impl Display
    └── view.rs           # DebugSessionView — Arc<RwLock> + watch channel

apps/visual-debugger/
├── Cargo.toml
└── src/
    ├── main.rs           # eframe entry point; creates DebugSessionView + WindowsDebugHandle
    ├── app.rs            # VisualDebuggerApp: eframe::App impl, frame dispatch
    ├── toolbar.rs        # 11 buttons: render, keyboard shortcuts, animation logic
    ├── source_view.rs    # Virtual-scroll source viewer, gutter, highlight
    ├── syntax.rs         # Hand-rolled Rust tokenizer → Vec<(TokenClass, &str)>
    ├── file_tree.rs      # CollapsingHeader tree, background walkdir scan
    └── address_bar.rs    # Path display + editable navigation field
```

**Structure Decision**: Single new crate + single new app binary. `crates/debug-session-view` carries all shared state (satisfies Principle II). `apps/visual-debugger` is purely the rendering and interaction layer. No changes to `apps/mcp-server` are required for this phase; MCP ↔ UI wiring via `DebugSessionView` is wired at `main.rs` startup.

---

## Phase 0: Research (Complete)

See `research.md` for all 7 decisions. Summary:

| Unknown | Resolution |
|---------|------------|
| Syntax highlighting | Hand-rolled Rust tokenizer, zero new deps |
| Large file performance | Fixed-row virtual list inside `ScrollArea` |
| AI event bus | `crates/debug-session-view` (Arc<RwLock> + watch) |
| File tree | `walkdir` + `CollapsingHeader` + background thread |
| Button animation | `Instant` timestamp map + `request_repaint_after` |
| Keyboard shortcuts | Per-frame `ctx.input()` + chord state machine |
| Debug bridge | Shared `WindowsDebugHandle` (same as `mcp-server`) |

---

## Phase 1: Design (Complete)

Artifacts generated:
- `data-model.md` — 12 types: `DebugUIState`, `DebugSessionState`, `BreakpointEntry`, `ToolbarAction`, `ToolbarButton`, `KeyCombo`, `SourceLine`, `TokenClass`, `ThreadInfo`, `DebugSessionView`, `FileTreeNode`, `AddressBarState`
- `contracts/toolbar-actions.md` — action-to-shortcut mapping, AI event format, breakpoint lifecycle, error states

---

## Phase 2: Test Coverage

### Strategy

Same two-tier approach as features 002 and 003:

- **Unit tests** — `#[test]` in each source file, no window, no debug bridge.
- **Integration tests** — `#[test] #[ignore]` requiring a running debug session.

### `crates/debug-session-view`

#### `state.rs`
| Test | Assertion |
|------|-----------|
| `debug_ui_state_new_is_idle` | Fresh `DebugUIState::default()` → `session_state == Idle`, no active line |
| `breakpoint_add_and_remove` | Add entry for `(file, 10)`, assert present; remove it, assert absent |
| `recent_actions_press_stored` | Insert `Continue` action, assert `recent_actions[Continue].elapsed() < 1ms` |
| `error_banner_set_and_clear` | Set `error_banner`, assert `Some`; call `clear_error()`, assert `None` |

#### `view.rs`
| Test | Assertion |
|------|-----------|
| `view_publish_notifies_watch` | `publish()` → watch receiver sees new value within 1ms |
| `view_read_after_publish` | `read_blocking()` returns the last published state |
| `view_clone_shares_arc` | Two clones of `DebugSessionView` see the same published state |

#### `action.rs`
| Test | Assertion |
|------|-----------|
| `all_toolbar_actions_display` | `ToolbarAction::all()` iterator — each variant `.to_string()` is non-empty |
| `toolbar_action_roundtrip` | `ToolbarAction::from_str(action.to_string())` round-trips all 12 variants |

### `apps/visual-debugger`

#### `syntax.rs`
| Test | Assertion |
|------|-----------|
| `tokenize_keyword` | `"fn foo"` → first token is `(Keyword, "fn")` |
| `tokenize_string_literal` | `r#""hello""#` → contains `(StringLiteral, …)` span |
| `tokenize_line_comment` | `"// comment"` → entire line is `(LineComment, …)` |
| `tokenize_block_comment` | `"/* a */ x"` → `BlockComment` followed by `Other` |
| `tokenize_type_ident` | `"MyStruct"` → `(TypeIdent, "MyStruct")` |
| `tokenize_empty_line` | `""` → empty token list, no panic |
| `tokenize_10k_lines_no_panic` | Generate 10,000 Rust lines; tokenize all; assert no panic and < 100ms |

#### `source_view.rs`
| Test | Assertion |
|------|-----------|
| `active_line_in_visible_range` | `SourceView::visible_range(active=500, height=600, row_h=20)` → range contains 500 |
| `active_line_outside_needs_scroll` | Line 1 active, scroll at line 200 → `needs_scroll() == true` |
| `breakpoint_flag_on_source_line` | `SourceLine` built with matching `BreakpointEntry` → `has_breakpoint == true` |

#### `toolbar.rs`
| Test | Assertion |
|------|-----------|
| `animation_active_within_200ms` | Press `Continue` at t=0; check at t=100ms → `is_pressed(Continue) == true` |
| `animation_cleared_after_200ms` | Press `Continue` at t=0; check at t=250ms → `is_pressed(Continue) == false` |
| `all_shortcuts_unique` | `ToolbarButton::all()` → no two entries share the same `KeyCombo` |

#### `address_bar.rs`
| Test | Assertion |
|------|-----------|
| `address_bar_shows_active_file` | Set `active_file = Some("src/main.rs")` → `display_path == "src/main.rs"` |
| `address_bar_invalid_path_sets_error` | Submit non-existent path → `error == Some(…)` |
| `address_bar_valid_path_clears_error` | Prior error; submit valid path → `error == None` |

---

## Acceptance Criteria Mapping

| Spec Criterion | Test Coverage |
|----------------|---------------|
| 11 toolbar buttons functional (SC-001, FR-001) | `all_shortcuts_unique` + `all_toolbar_actions_display` |
| 200ms press animation (SC-002, FR-003) | `animation_active_within_200ms` + `animation_cleared_after_200ms` |
| < 100ms scroll to active line (SC-003, FR-012) | `active_line_outside_needs_scroll` + `active_line_in_visible_range` |
| Breakpoint gutter dots (FR-006) | `breakpoint_add_and_remove` + `breakpoint_flag_on_source_line` |
| AI event mirrors human animation (SC-005, FR-010) | `view_publish_notifies_watch` + `animation_active_within_200ms` |
| File switch on frame change (SC-006, FR-007) | `view_read_after_publish` + `active_line_in_visible_range` |
| Address bar path display (FR-008) | `address_bar_shows_active_file` + `address_bar_invalid_path_sets_error` |

---

## Assumptions

- `egui` 0.29 and `eframe` 0.29 are already in workspace deps; no version changes needed.
- `walkdir` will be added as a new workspace dependency (pure Rust, no C).
- `WindowsDebugHandle` from `win-debug-bridge` is safely shareable as `Arc<WindowsDebugHandle>` across threads (already done in `mcp-server`).
- The binary path to debug is provided as a CLI argument (`--executable`), matching `mcp-server`'s convention.
- `apps/visual-debugger` and `apps/mcp-server` may run as separate processes; the initial implementation supports the separate-process model.
- Syntax colouring is display-only; it does not affect line numbers, breakpoint positions, or any debug logic.
