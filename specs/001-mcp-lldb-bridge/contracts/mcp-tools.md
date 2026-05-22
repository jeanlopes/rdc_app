# MCP Tool Contracts: Phase 1 — LLDB Bridge

**Feature**: 001-mcp-lldb-bridge
**Date**: 2026-05-20

All tools are exposed by `apps/mcp-server` via the MCP protocol (JSON-RPC 2.0).
Input/output types are defined in `crates/protocol`.
All tools return `Result<T, DebuggerError>`; on error the MCP server maps to a JSON-RPC error
response with the appropriate code.

---

## Session Management

### `launch_process`

Start a debug session by launching a target executable under LLDB supervision.

**Input**:
```json
{
  "executable": "/path/to/binary",
  "args": ["--flag", "value"],
  "env": { "RUST_LOG": "debug" },
  "working_dir": "/optional/cwd"
}
```

**Output**:
```json
{
  "session_id": "uuid-v4",
  "pid": 12345,
  "state": "Running"
}
```

**Errors**: `LLDBError`, `InvalidState` (session already running)

---

### `get_session_state`

Return the current session state and process info.

**Input**: `{}`

**Output**:
```json
{
  "session_id": "uuid-v4",
  "state": "Paused",
  "pause_reason": { "type": "Breakpoint", "id": 1 },
  "pid": 12345,
  "selected_thread": 1
}
```

---

## Breakpoint Management

### `set_breakpoint`

Set a breakpoint at a source line (most common) or function name.

**Input**:
```json
{
  "kind": "source_line",
  "file": "src/layout.rs",
  "line": 412,
  "condition": "remaining_width < 0"
}
```

Alternative kind:
```json
{
  "kind": "function_name",
  "name": "layout::measure"
}
```

**Output**:
```json
{
  "id": 1,
  "resolved": true,
  "locations": [
    {
      "address": "0x100001234",
      "file": "src/layout.rs",
      "line": 412
    }
  ]
}
```

**Errors**: `LLDBError`, `InvalidState` (no active session)

---

### `remove_breakpoint`

Remove a breakpoint by ID.

**Input**:
```json
{ "id": 1 }
```

**Output**: `{}`

**Errors**: `BreakpointNotFound`

---

### `list_breakpoints`

List all active breakpoints.

**Input**: `{}`

**Output**:
```json
{
  "breakpoints": [
    {
      "id": 1,
      "kind": "source_line",
      "file": "src/layout.rs",
      "line": 412,
      "condition": "remaining_width < 0",
      "hit_count": 3,
      "enabled": true
    }
  ]
}
```

---

## Execution Control

### `continue_execution`

Resume process execution from a paused state.

**Input**: `{}`

**Output** (returned when execution next pauses):
```json
{
  "event": "BreakpointHit",
  "thread_id": 1,
  "stop_reason": { "type": "Breakpoint", "id": 1 },
  "location": { "file": "src/layout.rs", "line": 412, "column": 5 }
}
```

**Errors**: `InvalidState` (not in `Paused` state)

---

### `pause_execution`

Interrupt a running process.

**Input**: `{}`

**Output**:
```json
{
  "event": "Paused",
  "thread_id": 1,
  "stop_reason": { "type": "UserRequest" },
  "location": { "file": "src/main.rs", "line": 88 }
}
```

**Errors**: `InvalidState` (not in `Running` state)

---

### `step_over`

Execute one source line without descending into function calls.

**Input**:
```json
{ "thread_id": 1 }
```
`thread_id` defaults to the selected thread if omitted.

**Output**:
```json
{
  "event": "StepComplete",
  "thread_id": 1,
  "stop_reason": { "type": "Step" },
  "location": { "file": "src/layout.rs", "line": 413 }
}
```

**Errors**: `InvalidState` (not in `Paused` state)

---

### `step_into`

Execute one source line, descending into function calls.

**Input**: Same as `step_over`

**Output**: Same shape as `step_over`

---

### `step_out`

Execute until the current function returns, stopping at the call site.

**Input**: `{ "thread_id": 1 }`

**Output**: Same shape as `step_over`, `location` is the call site.

---

## Inspection

### `read_locals`

Read local variables in a stack frame, with optional semantic probe context.

**Input**:
```json
{
  "thread_id": 1,
  "frame_index": 0,
  "probe_context": "measure_layout",
  "max_depth": 4
}
```
- `frame_index`: defaults to 0 (innermost frame)
- `probe_context`: if set, qualifies variable names as `{context}.{name}` and filters to
  variables matching the probe list registered via `register_probe` (or all locals if no probe
  registered for this context)
- `max_depth`: max recursion depth for struct expansion, defaults to 4

**Output**:
```json
{
  "frame": { "index": 0, "function": "layout::measure", "file": "src/layout.rs", "line": 412 },
  "variables": [
    {
      "name": "current_x",
      "qualified_name": "measure_layout.current_x",
      "type": "i32",
      "value": 88,
      "semantic_context": "measure_layout"
    },
    {
      "name": "remaining_width",
      "qualified_name": "measure_layout.remaining_width",
      "type": "i32",
      "value": -12,
      "semantic_context": "measure_layout"
    },
    {
      "name": "overflow",
      "qualified_name": "measure_layout.overflow",
      "type": "bool",
      "value": true,
      "semantic_context": "measure_layout"
    }
  ]
}
```

**Errors**: `InvalidState`, `ThreadNotFound`

---

### `read_stack`

Read the full call stack for a thread.

**Input**:
```json
{
  "thread_id": 1,
  "max_frames": 32
}
```

**Output**:
```json
{
  "thread_id": 1,
  "frames": [
    {
      "index": 0,
      "function": "layout::measure",
      "module": "rdc_app",
      "file": "src/layout.rs",
      "line": 412,
      "column": 5,
      "is_inlined": false
    },
    {
      "index": 1,
      "function": "layout::layout_pass",
      "module": "rdc_app",
      "file": "src/layout.rs",
      "line": 380
    }
  ]
}
```

**Errors**: `InvalidState`, `ThreadNotFound`

---

### `evaluate_expression`

Evaluate an LLDB expression in the context of a frame.

**Input**:
```json
{
  "expression": "remaining_width + current_x",
  "thread_id": 1,
  "frame_index": 0
}
```

**Output**:
```json
{
  "expression": "remaining_width + current_x",
  "value": 76,
  "type": "i32",
  "error": null
}
```

On error:
```json
{
  "expression": "undefined_var",
  "value": null,
  "type": null,
  "error": "use of undeclared identifier 'undefined_var'"
}
```

**Errors**: `InvalidState`, `EvalError`

---

### `list_threads`

List all threads in the current process.

**Input**: `{}`

**Output**:
```json
{
  "threads": [
    {
      "id": 1,
      "name": "main",
      "state": "Stopped",
      "stop_reason": { "type": "Breakpoint", "id": 1 },
      "frame_count": 12
    },
    {
      "id": 2,
      "name": "tokio-runtime-worker",
      "state": "Running",
      "stop_reason": null,
      "frame_count": 0
    }
  ],
  "selected_thread": 1
}
```

**Errors**: `InvalidState`

---

## Error Response Format

All errors are returned as MCP error responses:

```json
{
  "error": {
    "code": -32000,
    "message": "InvalidState: expected Paused, current state is Running",
    "data": {
      "error_type": "InvalidState",
      "current_state": "Running",
      "required_state": "Paused"
    }
  }
}
```

Error codes:
| Code | Type |
|------|------|
| -32000 | `DebuggerError` (generic) |
| -32001 | `InvalidState` |
| -32002 | `LLDBError` |
| -32003 | `BreakpointNotFound` |
| -32004 | `ThreadNotFound` |
| -32005 | `EvalError` |
