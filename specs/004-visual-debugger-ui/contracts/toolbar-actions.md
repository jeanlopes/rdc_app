# Contract: Toolbar Actions

**Feature**: `004-visual-debugger-ui` | **Date**: 2026-05-24

This document defines the contract between:
- **Producers**: human user (keyboard/mouse), AI agent (MCP tool calls)
- **Consumer**: `DebugSessionView` shared state + debug bridge

---

## Action Definitions

Each toolbar action has a canonical identity (`ToolbarAction` variant), one or more keyboard
activations, and an expected effect on the debug session.

| Action | Keyboard Shortcut | Session Precondition | Effect |
|--------|-------------------|----------------------|--------|
| Continue | F5 | Paused | Resumes execution |
| BreakAll | Ctrl+Alt+Break | Running | Pauses all threads |
| StopDebugging | Shift+F5 | Running \| Paused | Terminates the session |
| Restart | Ctrl+Shift+F5 | Any | Stops and re-launches |
| StepBackInto | Ctrl+R, F11 | Paused | Steps back into last call |
| StepBackOver | Ctrl+R, F10 | Paused | Steps back over last statement |
| StepBackOut | Ctrl+R, Shift+F11 | Paused | Steps back out of current frame |
| ShowNextStatement | Alt+Num * | Paused | Scrolls viewer to active line |
| StepInto | F11 | Paused | Steps into next call |
| StepOver | F10 | Paused | Steps over next statement |
| StepOut | Shift+F11 | Paused | Steps out of current frame |
| ShowThreadsInSource | (button only) | Any | Toggles thread overlay |

---

## Activation Contract

When a toolbar action is activated (by any source):

1. `DebugUIState.recent_actions[action] = Instant::now()` — triggers 200ms visual press effect.
2. The corresponding debug bridge command is sent (for step/continue/stop actions).
3. `error_banner` is cleared.
4. The debug bridge responds asynchronously: on completion, it writes updated `active_file`,
   `active_line`, `breakpoints`, and `session_state` into `DebugUIState`.

---

## AI Agent Action Event

When the AI agent (via MCP) executes a debug operation, it MUST also publish the corresponding
`ToolbarAction` to `DebugSessionView` so the UI animates the button. Format:

```json
{
  "action": "StepOver",
  "source": "ai",
  "timestamp_ms": 1716550123456
}
```

The `source` field is informational only; the UI renders the same animation regardless of source.

---

## Breakpoint Contract

| Operation | Trigger | Effect on DebugUIState |
|-----------|---------|------------------------|
| Add breakpoint | User clicks gutter \| AI event | Append `BreakpointEntry{file, line, resolved: false}` |
| Resolve breakpoint | Debug bridge confirms address | Set `resolved: true` for matching entry |
| Remove breakpoint | User clicks existing dot \| AI event | Remove matching `BreakpointEntry` |
| Hit breakpoint | Bridge fires hit event | `session_state = Paused`, `active_file`, `active_line` updated |

---

## Error States

| Error | Representation |
|-------|---------------|
| Invalid path in address bar | `AddressBarState.error = Some("File not found: …")` |
| Binary not found at launch | `DebugUIState.error_banner = Some("Binary not found: …")` |
| Debug bridge lost connection | `session_state = Terminated`, `error_banner` set |
| Unresolvable breakpoint | `BreakpointEntry.resolved = false` (dimmed dot, no error banner) |
| Step-back unsupported | `error_banner = Some("Step back not supported by this debug target")` |
