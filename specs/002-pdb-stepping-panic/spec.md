# Feature Specification: Debug Engine Completion — Source Symbols, Stepping & Panic Messages

**Feature Branch**: `002-pdb-stepping-panic`

**Created**: 2026-05-20

**Status**: Draft

**Input**: Complete the unimplemented acceptance criteria from `specs/001-mcp-lldb-bridge/plan.md`
that were stubbed during the win-debug-bridge refactoring.

## Context

The `win-debug-bridge` crate can launch a process under Windows debug supervision, detect
breakpoints at raw memory addresses, enumerate threads, and detect unhandled exceptions.
The following capabilities are not yet functional:

- Source-level breakpoints (file + line number)
- Reading local variable values
- Semantic variable context (`probe_context`)
- Execution stepping (step over, step into, step out)
- Call stack inspection with function names and source locations
- Expression evaluation against current process state
- Extracting the Rust panic message text when a panic is detected

This feature spec covers all of the above — the three natural work pieces that complete
the `001-mcp-lldb-bridge` acceptance criteria.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Source-Line Breakpoints (Priority: P1)

An AI agent sets a breakpoint at a named source file and line number. When execution reaches
that line, the agent receives a `BreakpointHit` event with the exact source location confirmed.

**Why this priority**: All other inspection capabilities (read_locals, read_stack) are only
meaningful after the process is paused at a known location. This is the prerequisite for
everything else.

**Independent Test**: Set a breakpoint at the `// BP:` comment line inside `bubble_sort` in
`debug-target-example`, launch the process, call `continue_execution`, verify the response
is `BreakpointHit` at the expected file and line number.

**Acceptance Scenarios**:

1. **Given** a running process, **When** `set_breakpoint` is called with a valid source file
   path and line number, **Then** the response contains `resolved: true` and the confirmed
   source location.
2. **Given** a breakpoint set at a valid line, **When** `continue_execution` is called,
   **Then** the process pauses and returns `BreakpointHit` with `location.file` and
   `location.line` matching the requested line.
3. **Given** a breakpoint set at a line with no executable code (e.g., a comment),
   **When** `set_breakpoint` is called, **Then** the response resolves to the nearest
   executable line and reports the actual resolved line in the output.
4. **Given** `set_breakpoint` called with a function name instead of a line,
   **When** the function is entered, **Then** `BreakpointHit` fires at the function entry.

---

### User Story 2 — Local Variable Inspection with Semantic Context (Priority: P2)

An AI agent paused at a breakpoint reads the local variables in scope — with their names, types,
and current values. When a `probe_context` is supplied, variable names are qualified as
`{context}.{variable}`, giving the AI semantically meaningful labels.

**Why this priority**: Raw execution control without variable visibility is not useful for
debugging. This is the core inspection capability.

**Independent Test**: Paused inside `bubble_sort` at the inner-loop breakpoint, call
`read_locals`. Verify the response contains `pass`, `i`, `swapped`, and `arr` with values
that match the expected state on the first iteration (e.g., `pass=0`, `i=0`, `swapped=false`).
Then call `read_locals` with `probe_context: "bubble_sort"` and verify names appear as
`bubble_sort.pass`, `bubble_sort.swapped`.

**Acceptance Scenarios**:

1. **Given** a process paused inside `bubble_sort`, **When** `read_locals` is called with no
   `probe_context`, **Then** the response contains variables `pass`, `i`, `swapped`, and `arr`
   with correct types and values matching the first iteration.
2. **Given** the same pause state, **When** `read_locals` is called with
   `probe_context: "bubble_sort"`, **Then** each variable name is qualified as
   `bubble_sort.{name}` (e.g., `bubble_sort.pass`, `bubble_sort.swapped`).
3. **Given** `arr` is a `Vec<i32>` with 7 elements, **When** `read_locals` is called,
   **Then** the `arr` variable value contains the array elements in their current order.
4. **Given** a nested struct variable, **When** `read_locals` is called with `max_depth: 3`,
   **Then** fields up to 3 levels deep are expanded.

---

### User Story 3 — Execution Stepping (Priority: P3)

An AI agent steps through source lines one at a time, descends into function calls, and
returns from functions. After each step, the agent receives the new source location.

**Why this priority**: Stepping enables the AI to observe how state changes across consecutive
lines, which is the core of hypothesis testing.

**Independent Test**: Paused at `bubble_sort(&mut arr)` in `main`, call `step_into`. Verify
the response is `StepComplete` with `location.function` = `bubble_sort` and the line number
pointing to the first executable line inside `bubble_sort`. Then call `step_over` and verify
the line number advances by 1. Then call `step_out` and verify the response is `StepComplete`
with `location.function` = `main`.

**Acceptance Scenarios**:

1. **Given** a process paused at a line, **When** `step_over` is called, **Then** the response
   is `StepComplete` with `location.line` = previous line + 1 (or the next executable line).
2. **Given** a process paused at a function-call line, **When** `step_into` is called,
   **Then** the response is `StepComplete` with `location.function` matching the called
   function name.
3. **Given** a process paused inside a function, **When** `step_out` is called, **Then** the
   response is `StepComplete` with `location` pointing to the line immediately after the
   call site in the caller.
4. **Given** `step_over` called on a function-call line, **Then** the function executes
   completely and `StepComplete` lands on the line after the call — it does NOT descend into
   the function.

---

### User Story 4 — Call Stack Inspection (Priority: P4)

An AI agent reads the full call stack at a pause point, with each frame showing the function
name and source location. This enables the AI to understand execution context and navigate
to frames of interest.

**Why this priority**: Stack inspection is required for the AI to understand how the program
reached its current state — essential for root-cause analysis.

**Independent Test**: Paused inside `bubble_sort`, call `read_stack`. Verify the response
contains at least two frames: frame 0 = `bubble_sort` at the expected line, frame 1 = `main`
at the `bubble_sort(&mut arr)` call site.

**Acceptance Scenarios**:

1. **Given** a process paused inside `bubble_sort`, **When** `read_stack` is called, **Then**
   the response contains frame 0 with `function_name: "bubble_sort"` and a valid `source_location`.
2. **Given** the same pause, **Then** frame 1 contains `function_name` referencing the caller
   (`main` or the wrapper function) with its source location.
3. **Given** `read_stack` called with `max_frames: 1`, **Then** exactly one frame is returned.
4. **Given** a deeply nested call, **When** `read_stack` is called, **Then** frames are ordered
   innermost-first (frame 0 = currently executing function).

---

### User Story 5 — Expression Evaluation (Priority: P5)

An AI agent evaluates simple Rust expressions in the context of the current stack frame and
receives the computed result with its type. This enables dynamic inspection of derived values
not directly accessible as named variables.

**Why this priority**: Allows the AI to test hypotheses without needing a variable to already
exist — e.g., computing `remaining_width < 0` or reading `arr[0]` after sorting.

**Independent Test**: Paused inside `bubble_sort`, call `evaluate_expression` with `arr[0]`.
Verify the response contains the integer value of the first element in the current array state.

**Acceptance Scenarios**:

1. **Given** `arr = [64, 34, 25, 12, 22, 11, 90]` before any swaps, **When**
   `evaluate_expression("arr[0]")` is called, **Then** the response value is `64`.
2. **Given** a variable `pass: usize = 2`, **When** `evaluate_expression("pass + 1")` is
   called, **Then** the response value is `3`.
3. **Given** an invalid expression (e.g., undefined variable), **When**
   `evaluate_expression("undefined_var")` is called, **Then** the response `error` field
   contains a non-empty message and `value` is null.

---

### User Story 6 — Panic Message Extraction (Priority: P6)

When a Rust binary panics, the AI agent receives the complete panic message text — not just
the fact that an exception occurred. This allows the AI to understand the cause of the panic
without needing to inspect memory manually.

**Why this priority**: Knowing "index out of bounds: the len is 3 but the index is 99" is far
more actionable for autonomous debugging than knowing "an unhandled exception occurred."

**Independent Test**: Launch `debug-target-example.exe panic`, call `continue_execution`.
Verify the response event is `PanicDetected` and `message` contains
`"index out of bounds: the len is 3 but the index is 99"`.

**Acceptance Scenarios**:

1. **Given** a process running `debug-target-example panic`, **When** `continue_execution`
   is called, **Then** the response `kind` is `PanicDetected` and `message` contains the
   exact Rust panic message including the cause and values.
2. **Given** a panic caused by `unwrap()` on `None`, **When** `continue_execution` returns
   `PanicDetected`, **Then** `message` contains "called `Option::unwrap()` on a `None` value".
3. **Given** `PanicDetected` returned, **When** `read_stack` is called, **Then** the stack
   is still accessible and shows the frames leading to the panic.
4. **Given** `PanicDetected` returned, **When** `read_locals` is called at the panic frame,
   **Then** the variables in scope at the panic site are returned.

---

### Edge Cases

- What happens when `set_breakpoint` targets a line in an inlined function? → Resolves to
  the nearest non-inlined executable line; response documents the actual resolved location.
- What happens when `read_locals` is called in a frame with no debug info (e.g., a stdlib
  frame)? → Returns an empty list with a `no_debug_info: true` flag in the response.
- What happens when `step_over` is called while the process is in `Running` state? → Returns
  `DebuggerError::InvalidState`.
- What happens when `evaluate_expression` contains a side-effecting expression (e.g.,
  incrementing a variable)? → The expression is evaluated and the side effect occurs in the
  target; this is expected behavior and the caller's responsibility.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: `set_breakpoint` MUST accept source file path + line number and resolve it to
  an executable address using the binary's debug symbols.
- **FR-002**: `set_breakpoint` MUST return `resolved: true` and the confirmed source location
  when the symbol lookup succeeds.
- **FR-003**: `read_locals` MUST return all in-scope local variables with their names, types,
  and current values when called from a paused state with debug symbols available.
- **FR-004**: `read_locals` with `probe_context` MUST qualify each variable name as
  `{context}.{name}` in the `qualified_name` field of the response.
- **FR-005**: `step_over` MUST advance execution by exactly one source line without
  descending into function calls, then return `StepComplete` with the new source location.
- **FR-006**: `step_into` MUST descend into the next function call on the current line,
  then return `StepComplete` with the new source location inside the called function.
- **FR-007**: `step_out` MUST execute the current function to its return, then return
  `StepComplete` with the source location at the call site in the caller.
- **FR-008**: `read_stack` MUST return ordered stack frames (innermost first), each with
  `function_name` and `source_location` populated when debug symbols are available.
- **FR-009**: `evaluate_expression` MUST evaluate the given expression in the context of
  the current frame and return the result with its type name.
- **FR-010**: `continue_execution` MUST return `PanicDetected` with the full Rust panic
  message string when the target binary panics (not just an exception code).

### Key Entities

- **SourceLocation**: File path + line number + optional column; returned by stepping,
  breakpoint resolution, and stack frames.
- **Variable**: Name, qualified name, type, value; returned by `read_locals`. Value is
  typed (scalar, array, struct) per the existing variable serialization contract.
- **StackFrame**: Index, function name, source location, inlined flag; returned by `read_stack`.
- **PanicMessage**: The complete Rust panic message string included in `PanicDetected` events.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An AI agent can navigate from `launch_process` to reading local variables at
  a source-line breakpoint in ≤ 5 MCP tool calls.
- **SC-002**: `read_locals` returns correct variable values for all scalar and array variables
  in `bubble_sort` on the first paused iteration with zero manual memory address input from
  the AI.
- **SC-003**: All 12 acceptance criteria from `specs/001-mcp-lldb-bridge/plan.md` pass against
  `debug-target-example.exe` after this feature is implemented.
- **SC-004**: `PanicDetected` message matches the exact string printed by Rust's panic handler
  (verifiable by comparing with `RUST_BACKTRACE=0` stderr output from the same binary).
- **SC-005**: `step_over` and `step_into` complete within 200ms p95 on a local Windows 11
  machine running `debug-target-example.exe`.

## Assumptions

- `debug-target-example.exe` is compiled with `cargo build` (debug symbols present in `.pdb`
  file next to the binary). Release builds without PDB files are out of scope.
- The `.pdb` file is located in the same directory as the `.exe` (standard Cargo output layout).
- Variable values are read at the moment of the pause — no live watch / continuous monitoring.
- Only x86-64 (64-bit) Windows binaries are in scope; 32-bit binaries are out of scope.
- Expression evaluation is limited to simple field access and array indexing for Phase 1;
  arbitrary Rust expression compilation is out of scope.
- Panic detection applies to Rust `panic!`, `unwrap()`, `expect()`, and index-out-of-bounds;
  custom panic hooks that suppress the message are out of scope.
