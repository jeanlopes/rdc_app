# Data Model: Visual Debugger UI

**Feature**: `004-visual-debugger-ui` | **Date**: 2026-05-24

---

## Core Types

### `DebugUIState`

The shared in-process state between the debug event producers (debug bridge, MCP server, AI agent) and the egui render loop. Lives inside `DebugSessionView`.

```
DebugUIState {
    active_file:       Option<PathBuf>                  // file currently shown in source viewer
    active_line:       Option<u32>                      // 1-based line number of execution cursor
    breakpoints:       Vec<BreakpointEntry>             // all known breakpoints across files
    session_state:     DebugSessionState                // Idle | Running | Paused | Terminated
    recent_actions:    HashMap<ToolbarAction, Instant>  // last press timestamp per action
    thread_list:       Vec<ThreadInfo>                  // threads for "Show Threads in Source"
    show_threads:      bool                             // toggle state for thread overlay
    error_banner:      Option<String>                   // transient error message (clears on next action)
}
```

**Invariants**:
- `active_line` is `Some` only when `session_state == Paused`.
- `recent_actions` entries older than 500ms may be pruned each frame to avoid unbounded growth.
- `error_banner` is `None` unless the last operation resulted in a non-fatal error.

---

### `DebugSessionState`

Mirrors the state machine from `runtime-core::DebugSession` but adds the `Idle` variant for the pre-launch state.

```
DebugSessionState {
    Idle,        // no binary loaded
    Running,     // target process executing
    Paused,      // hit breakpoint or Step completed
    Terminated,  // process exited
}
```

---

### `BreakpointEntry`

A source location with resolve status.

```
BreakpointEntry {
    file:      PathBuf      // absolute path
    line:      u32          // 1-based line number
    resolved:  bool         // true if the debugger confirmed a valid code address
}
```

---

### `ToolbarAction`

One of the 11 debugger commands plus the thread toggle.

```
ToolbarAction {
    Continue,
    BreakAll,
    StopDebugging,
    Restart,
    StepBackInto,
    StepBackOver,
    StepBackOut,
    ShowNextStatement,
    StepInto,
    StepOver,
    StepOut,
    ShowThreadsInSource,   // toggles DebugUIState.show_threads
}
```

---

### `ToolbarButton`

Render metadata for one toolbar button. Static, constructed once at startup.

```
ToolbarButton {
    action:       ToolbarAction
    label:        &'static str        // e.g. "Continue"
    icon:         Icon                // egui icon glyph or embedded image
    shortcut:     &'static str        // e.g. "F5", "Shift+F5"
    shortcut_key: KeyCombo            // machine-readable key binding
}
```

---

### `KeyCombo`

A keyboard shortcut, supporting single-key and two-key chord sequences.

```
KeyCombo {
    Single { modifiers: Modifiers, key: Key }
    Chord  { first: (Modifiers, Key), second: (Modifiers, Key) }
}
```

---

### `SourceLine`

A single rendered line in the source viewer.

```
SourceLine {
    number:   u32                          // 1-based
    tokens:   Vec<(TokenClass, String)>    // syntax-highlighted spans
    is_active: bool                        // true if this is the current execution line
    has_breakpoint: bool                   // true if a BreakpointEntry exists at this line
    breakpoint_resolved: bool              // false → dimmed dot in gutter
}
```

---

### `TokenClass`

Syntax highlighting categories for the hand-rolled Rust tokenizer.

```
TokenClass {
    Keyword,        // fn, let, pub, impl, struct, enum, match, …
    StringLiteral,  // "…"
    CharLiteral,    // '…'
    LineComment,    // // …
    BlockComment,   // /* … */
    TypeIdent,      // CamelCase identifiers
    Other,          // everything else
}
```

---

### `ThreadInfo`

One thread in the running process.

```
ThreadInfo {
    thread_id:  u64
    name:       Option<String>
    is_active:  bool           // true for the thread owning the current frame
}
```

---

### `DebugSessionView`

Shared in-process bus between the egui render loop and the debug/AI layer. Same pattern as `IntrospectionStore` from `crates/egui-introspection`.

```
DebugSessionView {
    state:     Arc<tokio::sync::RwLock<DebugUIState>>
    notifier:  tokio::sync::watch::Sender<u64>  // frame counter increments on each write
}
```

**Thread model**:
- Debug bridge thread (sync): calls `view.state.blocking_write()` after each debug event.
- AI MCP handler threads (async): call `view.state.write().await`.
- egui render loop (sync): calls `view.state.blocking_read()` each frame.

---

### `FileTreeNode`

One node in the file tree.

```
FileTreeNode {
    path:     PathBuf
    name:     String         // display name (last path component)
    kind:     FileTreeKind
    children: Vec<FileTreeNode>   // empty until expanded; populated on first expand
}

FileTreeKind { Directory, File }
```

---

### `AddressBarState`

Tracks the address bar's edit state independently of the source viewer.

```
AddressBarState {
    display_path:  String     // shown when not editing
    edit_text:     String     // buffer while user types
    editing:       bool       // true when bar has focus
    error:         Option<String>   // set on invalid/missing path; cleared on next keystroke
}
```

---

## Relationships

```
DebugSessionView ──1:1──► DebugUIState          (shared live state)
DebugUIState     ──0:N──► BreakpointEntry        (all breakpoints)
DebugUIState     ──1:N──► ToolbarAction (recent) (animation timestamps)
DebugUIState     ──0:N──► ThreadInfo             (thread list)
SourceLine       ──1:N──► (TokenClass, String)   (syntax spans)
FileTreeNode     ──0:N──► FileTreeNode            (children, lazy-loaded)
ToolbarButton    ──1:1──► ToolbarAction           (identity)
ToolbarButton    ──1:1──► KeyCombo                (shortcut binding)
```

---

## Crate Boundary

`crates/debug-session-view` owns and exports:
- `DebugUIState`, `DebugSessionState`, `BreakpointEntry`, `ToolbarAction`, `ThreadInfo`
- `DebugSessionView` (shared bus)

`apps/visual-debugger` owns (rendering layer, not exported):
- `ToolbarButton`, `KeyCombo`, `SourceLine`, `TokenClass`
- `FileTreeNode`, `AddressBarState`
- All egui rendering logic
