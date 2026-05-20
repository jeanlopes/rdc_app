# Feature Specification: Phase 1 — MCP + LLDB Bridge

**Feature Branch**: `001-mcp-lldb-bridge`

**Created**: 2026-05-20

**Status**: Draft

**Input**: Phase 1 of the RDC AI-native Runtime Intelligence Platform roadmap

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Process Launch & Session Lifecycle (Priority: P1)

An AI agent targets a compiled Rust binary, opens a debug session via MCP, and confirms the
process is running under LLDB supervision. The agent can also query current session state at
any time and terminate the session cleanly.

**Why this priority**: Nothing else is possible without a running debug session. This is the
absolute foundation.

**Independent Test**: Run `mcp-server --executable ./target/debug/hello_world`, call
`launch_process`, verify response contains `state: Running` and a valid PID. Then call
`get_session_state` and verify it reflects the running state.

**Acceptance Scenarios**:

1. **Given** an executable binary at a known path, **When** `launch_process` is called with
   that path, **Then** the process starts under LLDB and the response contains `state: Running`
   and a non-zero `pid`.
2. **Given** a running session, **When** `get_session_state` is called, **Then** the response
   reflects the current state (`Running`, `Paused`, etc.) and selected thread.
3. **Given** a running session, **When** the target process exits naturally, **Then** the
   session transitions to `Terminated` with the correct exit code.
4. **Given** an invalid executable path, **When** `launch_process` is called, **Then** an
   `LLDBError` is returned and no session is created.

---

### User Story 2 — Breakpoint Management & Execution Control (Priority: P2)

An AI agent sets a breakpoint at a known source line, resumes execution, observes the
breakpoint being hit, and navigates through code using step commands.

**Why this priority**: Breakpoints + execution control are the core debugging workflow.
Required before any meaningful state inspection.

**Independent Test**: Set a breakpoint at a known line in a test binary, call
`continue_execution`, verify the response event is `BreakpointHit` at the expected location.
Then call `step_over` and verify the line advances by 1.

**Acceptance Scenarios**:

1. **Given** a running session, **When** `set_breakpoint` is called with a valid source file
   and line, **Then** the response contains `resolved: true` and a `BreakpointId`.
2. **Given** a set breakpoint, **When** `continue_execution` is called, **Then** execution
   stops at the breakpoint and returns a `BreakpointHit` event with the correct location.
3. **Given** a paused session, **When** `step_over` is called, **Then** execution advances
   exactly one source line and returns `StepComplete` with the new location.
4. **Given** a paused session, **When** `step_into` is called on a line with a function call,
   **Then** execution enters the function and stops at its first line.
5. **Given** a paused session inside a function, **When** `step_out` is called, **Then**
   execution returns to the call site.
6. **Given** a running session, **When** `pause_execution` is called, **Then** the process
   pauses and returns a `Paused` event with `UserRequest` stop reason.
7. **Given** a set breakpoint, **When** `remove_breakpoint` is called with its ID, **Then**
   the breakpoint is deleted and subsequent execution no longer stops there.

---

### User Story 3 — Runtime State Inspection (Priority: P3)

An AI agent, stopped at a breakpoint, inspects local variables, the call stack, evaluates
expressions, and lists all threads. Variables captured via semantic probe context carry
qualified names for AI-interpretable meaning.

**Why this priority**: Inspection is the payload — without it the agent can navigate but not
understand. Depends on breakpoint + execution control (US2) being functional.

**Independent Test**: At a known breakpoint where local variable `remaining_width` is known
to be negative, call `read_locals` with `probe_context: "measure_layout"`. Verify the
response contains `measure_layout.remaining_width` with a negative value.

**Acceptance Scenarios**:

1. **Given** a paused session, **When** `read_locals` is called, **Then** all local variables
   in the current frame are returned with correct types and values.
2. **Given** a paused session, **When** `read_locals` is called with `probe_context:
   "measure_layout"`, **Then** variable names are qualified as `measure_layout.{name}`.
3. **Given** a paused session, **When** `read_stack` is called, **Then** a list of stack
   frames is returned, each with function name and source location.
4. **Given** a paused session, **When** `evaluate_expression` is called with a valid Rust
   expression, **Then** the computed result is returned with its type.
5. **Given** a paused session with a struct variable, **When** `read_locals` is called with
   `max_depth: 3`, **Then** nested struct fields are expanded up to 3 levels deep.
6. **Given** a multi-threaded target paused by a breakpoint, **When** `list_threads` is
   called, **Then** all threads are returned with their names, states, and stop reasons.

---

### User Story 4 — Panic Detection (Priority: P4)

An AI agent running a target that panics receives a structured `PanicDetected` event with
the panic message, enabling the agent to identify and locate the cause without manual
intervention.

**Why this priority**: Panic detection is the primary use-case for autonomous debugging.
Lower priority than basic inspection because it requires US1-3 to be functional.

**Independent Test**: Run a binary known to panic with `index out of bounds`, call
`continue_execution`, verify the response event is `PanicDetected` with a `message`
containing "index out of bounds" and a valid source location.

**Acceptance Scenarios**:

1. **Given** a running session where the target panics, **When** `continue_execution` is
   outstanding, **Then** it returns a `PanicDetected` event with the panic message and the
   source location of the `panic!` macro invocation.
2. **Given** a `PanicDetected` event, **When** `read_stack` is called, **Then** the full
   call stack leading to the panic is available.
3. **Given** a `PanicDetected` event, **When** `read_locals` is called at the panic frame,
   **Then** the variables in scope at the panic site are returned.

---

### Edge Cases

- What happens when `set_breakpoint` targets a line with no executable code (e.g., a comment
  or blank line)? → LLDB should resolve to the nearest executable line; response includes the
  actual resolved location.
- What happens when `read_locals` is called in a frame with no debug info (optimized code)?
  → Return an empty list with a `no_debug_info: true` flag in the response.
- What happens when a struct variable has a cyclic reference? → Serialization stops at the
  cycle and inserts `{ "$ref": "<address>" }` per the variable-serialization contract.
- What happens when `evaluate_expression` is called with an expression that side-effects the
  target? → LLDB evaluates it; the result is returned. Side effects are the caller's
  responsibility.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST expose an MCP server accepting tool calls over stdio transport.
- **FR-002**: System MUST support `launch_process` to start a debug session under LLDB.
- **FR-003**: System MUST implement `set_breakpoint`, `remove_breakpoint`, and
  `list_breakpoints` with source-line and function-name kinds.
- **FR-004**: System MUST implement `continue_execution`, `pause_execution`, `step_over`,
  `step_into`, and `step_out` execution control tools.
- **FR-005**: System MUST implement `read_locals`, `read_stack`, `evaluate_expression`, and
  `list_threads` inspection tools.
- **FR-006**: `read_locals` MUST support `probe_context` parameter for semantic variable
  annotation (qualified names).
- **FR-007**: System MUST detect Rust panics and return a `PanicDetected` execution event
  with the panic message and source location.
- **FR-008**: Variable serialization MUST handle cyclic references, depth limits, and large
  collections without hanging or producing unbounded output.
- **FR-009**: LLDB operations MUST NOT block the Tokio async executor thread.
- **FR-010**: All `pub` crate APIs MUST be documented with `///` comments.

### Key Entities

- **DebugSession**: Lifecycle container, session ID, state machine (see data-model.md)
- **Breakpoint**: Source-line or function-name, condition, hit count, lifecycle
- **Variable**: Typed value with optional semantic annotation
- **StackFrame**: Source location, function name, frame index
- **ThreadInfo**: Thread ID, name, state, stop reason
- **ExecutionEvent**: Event kind, location, stop reason (returned by execution tools)

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All 11 MCP tools respond within 100ms p95 on a local LLDB session.
- **SC-002**: `launch_process` succeeds for any valid Rust binary compiled with debug symbols.
- **SC-003**: AI agent can navigate from process launch to reading a local variable at a
  breakpoint in ≤ 5 MCP tool calls.
- **SC-004**: `read_locals` with `probe_context` returns qualified variable names matching the
  format `{context}.{variable}` for all variables in scope.
- **SC-005**: Panic detection fires for any Rust `panic!`, `unwrap()` on `None`, and
  index-out-of-bounds conditions.
- **SC-006**: Variable serialization completes in < 50ms for frames with ≤ 50 locals at
  depth 4, regardless of struct complexity.

## Assumptions

- Target binaries are compiled with Rust debug symbols (`cargo build` default, not `--release`).
- LLDB 14+ with Python bindings is installed on the host running `mcp-server`.
- Each `mcp-server` instance manages exactly one debug session (multi-session is Phase 2+).
- The AI client speaks MCP protocol version 2024-11-05 or later.
- Windows support is best-effort; Linux and macOS are the primary targets for Phase 1.
