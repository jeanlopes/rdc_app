# Research: Visual Debugger UI

**Feature**: `004-visual-debugger-ui` | **Date**: 2026-05-24

---

## Decision 1: Syntax Highlighting Strategy

**Decision**: Hand-rolled Rust tokenizer using a single-pass scanner. Recognises seven token classes: `Keyword`, `StringLiteral`, `CharLiteral`, `LineComment`, `BlockComment`, `TypeIdent` (CamelCase identifiers), and `Other`. Produces a `Vec<(TokenClass, &str)>` per line for coloring in the egui painter.

**Rationale**: `syntect` would require bundling the entire Oniguruma-based regex engine plus a syntax pack (≥ 3 MB added to binary). The spec requires only "basic syntax colouring (keywords, strings, comments)" — not full semantic highlighting. A 200-line tokenizer in `crates/debug-session-view/src/syntax.rs` satisfies all acceptance scenarios, ships zero extra dependencies, and compiles in milliseconds. Keyword list covers the 52 Rust reserved words.

**Alternatives considered**:
- `syntect` with embedded Rust syntax pack — rejected: heavyweight, slow cold compile, adds `onig` or `fancy-regex` C dependency (violates Principle VI zero-runtime policy if not purely Rust).
- `tree-sitter` with Rust grammar — rejected: even heavier, requires C bindings, overkill for display-only coloring.

---

## Decision 2: Virtual Scroll for Large Source Files

**Decision**: Implement a fixed-height-row virtual list inside egui's `ScrollArea`. Record the per-character line height from egui's font metrics once per frame. Derive the visible row range as `scroll_offset / row_height .. (scroll_offset + panel_height) / row_height`. Render only those rows using `ui.allocate_space()` for invisible rows (to preserve correct scroll extent) and `ui.label()`/painter calls for visible rows.

**Rationale**: egui's `ScrollArea` renders all child widgets by default — for a 10,000-line file this causes ~200ms layout per frame, violating SC-003 (100ms active-line scroll) and the general 60 fps target. A virtual list is standard for this class of UI; the approach is well-documented in the egui GitHub discussions. Fixed row height is a safe assumption for monospaced source code display.

**Alternatives considered**:
- Render all lines unconditionally — rejected: unacceptable performance on large files.
- Split file into pages — rejected: breaks natural scrolling behaviour expected by users.

---

## Decision 3: AI-to-UI Event Bus

**Decision**: Create `crates/debug-session-view` providing `DebugSessionView` — a shared in-process state store using `Arc<tokio::sync::RwLock<DebugUIState>>` + `tokio::sync::watch::Sender<u64>` for change notification. Identical pattern to `crates/egui-introspection`'s `IntrospectionStore`. The `visual-debugger` egui loop reads the state each frame via `blocking_read()`; the MCP server or debug bridge writes to it after each operation via `blocking_write()`.

**Rationale**: Constitution Principle II forbids siloing capabilities inside `apps/`; the shared state must live in a crate. The `IntrospectionStore` pattern is already proven in this codebase (feature 003). Reusing the same pattern keeps architectural consistency. A `watch` channel avoids polling overhead while remaining compatible with egui's synchronous render loop. No new async patterns need to be introduced.

**Alternatives considered**:
- Extend `runtime-core` with UI fields — rejected: `runtime-core` is already scope-complete as the session state machine; mixing UI state (button press timestamps, scroll positions) into it violates single-responsibility.
- `std::sync::mpsc` channel — rejected: queue-based channels require draining; the "latest value" semantics of `watch` are more appropriate for frame-driven UI rendering.
- Shared memory / named pipe — rejected: spec explicitly requires in-process communication.

---

## Decision 4: File Tree Rendering

**Decision**: Use `walkdir` crate to enumerate directories on-demand (lazy, expand on click). Store expanded/collapsed state per directory path in a `HashSet<PathBuf>`. Render each level with egui's `CollapsingHeader` which provides built-in expand/collapse semantics. Scan is triggered in a background thread to avoid blocking the render loop; results are published to a `Vec<DirEntry>` behind a `Mutex`.

**Rationale**: egui doesn't ship a tree view component; `CollapsingHeader` is the closest primitive and natively handles the clicked-to-expand interaction. `walkdir` is a lightweight pure-Rust crate (already a transitive dep via several workspace crates). Background scanning prevents frame drops when the user expands a directory with many children.

**Alternatives considered**:
- Recursive `fs::read_dir` — rejected: not lazy, blocks render thread on large trees.
- `egui_extras` `TableBody` — rejected: tabular, not hierarchical; does not model parent-child directory nesting.

---

## Decision 5: Toolbar Button Press Animation

**Decision**: Store a `HashMap<ToolbarAction, std::time::Instant>` in `DebugUIState`. When a toolbar action fires (human keypress, mouse click, or AI event), insert `Instant::now()` for that action. Each egui frame, check `pressed_at.elapsed() < Duration::from_millis(200)`; if true, render the button with a `Visuals::pressed` style override (darker background, inset shadow). Request a repaint via `ctx.request_repaint_after(Duration::from_millis(200))` to ensure the pressed state clears even without user input.

**Rationale**: egui is immediate-mode — it has no built-in timed animation system. Storing a timestamp and comparing against a wall-clock threshold is the standard egui pattern for timed visual effects (documented in the egui book). `request_repaint_after` guarantees the frame that clears the animation fires within 200ms without busy-polling.

**Alternatives considered**:
- Persistent `pressed: bool` field reset on next interaction — rejected: doesn't meet the 200ms spec requirement; would only clear on the next user event.
- A dedicated animation ticker in a background thread — rejected: adds complexity with no benefit; `request_repaint_after` is sufficient.

---

## Decision 6: Keyboard Shortcut Handling

**Decision**: Evaluate all keyboard shortcuts in a single `ctx.input(|i| {...})` call at the top of `App::update()`, before rendering. Map each recognised key combination to the corresponding `ToolbarAction` and enqueue it in a `Vec<ToolbarAction>` that is processed during the same frame. Multi-key sequences (Ctrl+R, F11) are handled by tracking whether the previous input event in the same frame included `Ctrl+R`, using a `DoubleShortcutState` field in the app.

**Rationale**: egui's input system gives a `RawInput::events: Vec<Event>` per frame. For standard single-modifier shortcuts (F5, Shift+F5, Ctrl+Shift+F5) this is straightforward. The Step-Back shortcuts use a two-key chord (Ctrl+R then F11/F10/Shift+F11); this requires a small state machine that records whether `Ctrl+R` was seen in the previous frame, then checks for the second key. This is the only chord sequence and does not justify a general keybinding framework.

**Alternatives considered**:
- Global OS-level hotkey hooks (winapi `RegisterHotKey`) — rejected: intercepts keys even when the app is backgrounded, unexpected side-effects.
- A third-party keybinding crate — rejected: overkill for 11 fixed shortcuts; a 30-line match statement suffices.

---

## Decision 7: Debug Bridge Integration

**Decision**: The `visual-debugger` app depends directly on `win-debug-bridge` (same as `mcp-server`). Toolbar actions that execute real debug operations send commands through a `WindowsDebugHandle` channel (existing thread-based bridge). The UI does not block on the response; it fires and returns. The debug handle publishes result events back to `DebugSessionView` asynchronously (the bridge thread writes `active_file`, `active_line`, `breakpoints` into the shared state).

**Rationale**: `mcp-server` already demonstrates this pattern — a UI/protocol layer depends on `win-debug-bridge` and communicates via its thread channel. Reusing the same integration point avoids a parallel debug bridge. The visual debugger and the MCP server can share a single `WindowsDebugHandle` instance (passed via `Arc`).

**Alternatives considered**:
- Route all debug commands through the MCP server's JSON-RPC loop — rejected: adds a JSON serialisation round-trip for every F10/F11 keypress; too slow for interactive debugging.
- Spawn a new debug bridge per app — rejected: two bridges to the same process would conflict at the Windows Debug API level.

---

## Summary Table

| Decision | Chosen Approach | Key Constraint Met |
|----------|-----------------|--------------------|
| Syntax highlighting | Hand-rolled Rust tokenizer | Zero new deps, Principle VI |
| Large file scroll | Fixed-height virtual list | SC-003 (< 100ms scroll) |
| AI event bus | `crates/debug-session-view` (same as IntrospectionStore) | Principle II (no app silo) |
| File tree | `walkdir` + `CollapsingHeader` + background scan | SC-004 (< 100ms expand) |
| Button animation | `Instant` timestamp + `request_repaint_after` | SC-002 (200ms ± 10ms) |
| Keyboard shortcuts | Per-frame `ctx.input()` + chord state machine | FR-002 |
| Debug bridge | Shared `WindowsDebugHandle` (same as mcp-server) | FR-011 |
