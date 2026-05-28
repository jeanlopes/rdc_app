# Feature Specification: Native LLDB Backend via lldb-sys

**Feature Branch**: `005-lldbsys-replace`

**Created**: 2026-05-27

**Status**: Draft

**Input**: Refactor the debugger backend to use the lldb-sys and lldb-safe crates directly instead of going through the Debug Adapter Protocol (DAP) and the VS Code codelldb extension.

---

## Context

The existing debugger backend communicates with codelldb via the DAP protocol — an external process that must be spawned and kept alive. Although the protocol itself works correctly, this architecture introduces an external process boundary that is vulnerable to antivirus interference (Bitdefender Endpoint Security has been observed killing the debugged process before breakpoints are reached).

The `lldb-sys` workspace provides native Rust bindings to the LLDB C API. Using these bindings directly eliminates the external DAP process, removes the dependency on the VS Code codelldb extension, and gives the application full control over the debug lifecycle in-process.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Launch and Break at Breakpoint (Priority: P1)

A developer selects a binary to debug, sets a breakpoint at a source location, and launches the session. Execution pauses at the breakpoint, showing the current file and line in the visual debugger. No external debugger adapter or IDE extension is required.

**Why this priority**: This is the fundamental use case. Every other feature (stepping, variable inspection) depends on successfully launching a process and stopping at a known point.

**Independent Test**: Point the debugger at the `debug-target-example` binary, set a breakpoint at `main.rs:20`, launch, and confirm execution pauses at that line with the source viewer highlighting it and the process state showing "Stopped".

**Acceptance Scenarios**:

1. **Given** a valid binary and a source-level breakpoint, **When** the session is launched, **Then** the process starts and halts at the breakpoint without requiring any external adapter process.
2. **Given** a running session paused at a breakpoint, **When** the user reads the current state, **Then** the active file, active line number, and stop reason ("Breakpoint") are all reported correctly.
3. **Given** the debugger is launched with antivirus software active, **When** execution reaches a breakpoint, **Then** the session is not terminated by the antivirus (no external process spawned to trigger behavioural monitoring).

---

### User Story 2 — Step Through Code (Priority: P2)

While paused at a breakpoint, a developer uses step-over, step-into, and step-out controls. After each step, the current execution position updates in the visual debugger, showing the new file and line.

**Why this priority**: Stepping is the core interaction loop of any debugger; it depends on P1 (the process must first be paused).

**Independent Test**: Pause at a breakpoint, press Step Over three times, and confirm the yellow line highlight advances one source line per step. Press Step Into a function call and confirm the viewer switches to the callee's source file at line 1 of the function body.

**Acceptance Scenarios**:

1. **Given** execution is paused, **When** Step Over is activated, **Then** execution advances exactly one source line and the process returns to Stopped state.
2. **Given** execution is paused at a function call, **When** Step Into is activated, **Then** execution moves into the called function and the active frame reflects the new location.
3. **Given** execution is inside a function, **When** Step Out is activated, **Then** execution returns to the call site and the active frame reflects the caller's location.
4. **Given** execution is paused, **When** Continue is activated, **Then** the process runs until the next breakpoint or termination.

---

### User Story 3 — Inspect Variables (Priority: P3)

While paused, a developer inspects the values of local variables and function arguments in the current stack frame. Compound types (structs, arrays) can be expanded to show their fields.

**Why this priority**: Variable inspection is the primary observability mechanism; it requires a paused session (P1, P2).

**Independent Test**: Pause inside a function that has a local `Vec<i32>`. Confirm the variable panel lists the variable by name, shows its type, and allows expanding it to reveal individual elements with their values.

**Acceptance Scenarios**:

1. **Given** execution is paused in a frame, **When** the variable list is requested, **Then** all in-scope local variables are returned with their names, types, and current values.
2. **Given** a compound variable (struct or array), **When** the user expands it, **Then** child fields or elements are listed with their names, types, and values.
3. **Given** a pointer variable, **When** dereferenced, **Then** the value at the pointed-to address is returned.

---

### User Story 4 — Manage Breakpoints Dynamically (Priority: P4)

A developer adds and removes breakpoints during a live debug session, without restarting. New breakpoints take effect on the next pass through the relevant code; removed breakpoints no longer pause execution.

**Why this priority**: Convenience during an active session. Depends on P1.

**Independent Test**: With a session running, add a breakpoint at a line not yet reached. Continue execution and confirm the process pauses at the new breakpoint. Remove the breakpoint and continue; confirm execution does not pause at that line again.

**Acceptance Scenarios**:

1. **Given** a live session, **When** a breakpoint is added at a source location, **Then** the next execution of that line pauses the process.
2. **Given** a live session with an active breakpoint, **When** that breakpoint is removed, **Then** subsequent execution passes through that line without stopping.
3. **Given** a breakpoint is set on a line with no executable code, **When** the session is active, **Then** the breakpoint is reported as unresolved and execution is not affected.

---

### Edge Cases

- Binary path does not exist or is not a valid executable — a clear error is reported and no session is started.
- Debugged process exits unexpectedly (crash or normal exit) — the backend transitions to an "Exited" state and surfaces the exit code; it does not hang waiting for the next event.
- Breakpoint set inside a module not yet loaded — breakpoint is deferred and resolved once the module loads.
- Step Over across a multi-line expression — execution advances to the next statement, not the next physical line.
- Process is stopped by a signal (e.g., access violation) — the stop reason is reported as "Exception" with the signal name; the user can inspect state before terminating.
- Multiple threads hit breakpoints simultaneously — the backend selects the first stopped thread and surfaces it; the user can switch threads explicitly.
- Session is killed (Stop Debugging) while the process is running — the process is cleanly terminated, all resources released, and the backend returns to idle state.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The backend MUST launch a specified binary as a debugged process without spawning any external adapter process or requiring any VS Code extension.
- **FR-002**: The backend MUST allow breakpoints to be set by source file and line number before and during a live session.
- **FR-003**: The backend MUST pause execution at confirmed breakpoints and report the stop reason, active thread, active frame, source file, and line number.
- **FR-004**: The backend MUST support the following stepping commands: Step Over, Step Into, Step Out, and Continue.
- **FR-005**: The backend MUST expose the list of in-scope local variables and function arguments for the current frame, including name, type, and value.
- **FR-006**: The backend MUST support expanding compound variables (structs, arrays, pointers) to reveal child values.
- **FR-007**: The backend MUST allow breakpoints to be added and removed during a live session; changes MUST take effect without restarting.
- **FR-008**: The backend MUST report process state transitions (Launching, Running, Stopped, Exited, Crashed) to the visual debugger UI so that the UI can update accordingly.
- **FR-009**: The backend MUST allow the user to attach to an already-running process by PID.
- **FR-010**: The backend MUST cleanly detach from or kill the debugged process when the session is ended, releasing all OS resources.
- **FR-011**: The backend MUST surface the full call stack (list of frames with function name, source file, and line number) when execution is paused.
- **FR-012**: The visual debugger UI (feature 004) MUST continue to function with the new backend without changes to its event model; the backend MUST emit the same session-state events the UI already consumes.

### Key Entities

- **DebugSession**: Represents one active debug session — binary path, process state, active thread, active frame, list of active breakpoints.
- **BreakpointSpec**: A requested breakpoint location — source file, line number, and resolved/unresolved status.
- **FrameInfo**: A single stack frame — frame index, function name, source file, line number, PC address.
- **VariableInfo**: A variable or value in scope — name, type name, display value, child count (for compound types).
- **SessionEvent**: A state-change notification emitted by the backend — event type (stopped, continued, exited, breakpoint-resolved), payload (stop reason, active frame, exit code).

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A debug session reaches a source-level breakpoint in 100% of runs on the target machine, with no session termination caused by antivirus interference.
- **SC-002**: Each step command (Step Over, Step Into, Step Out) completes and returns a new paused state within 500ms on the target machine.
- **SC-003**: The variable list for a frame with up to 50 local variables is returned within 200ms of the process pausing.
- **SC-004**: Breakpoints added or removed during a live session take effect on the next execution pass through that location without requiring a restart.
- **SC-005**: The visual debugger UI reflects each state change (paused, stepped, breakpoint added/removed) within one rendered frame (< 16ms) of the backend emitting the event.
- **SC-006**: Session teardown (Stop Debugging) completes within 1 second of the user activating it, with the debugged process fully terminated and no zombie processes remaining.
- **SC-007**: All existing visual debugger UI features (toolbar, source view, gutter breakpoints) work identically with the new backend as they did with the DAP backend.

---

## Assumptions

- The target platform is Windows 10/11 x86-64; the `lldb-sys` workspace already builds and links correctly on this platform.
- The `lldb-safe` crate provides sufficient coverage of LLDB's API for the required operations; no new C++ wrapper code is needed for core scenarios.
- The visual debugger UI (feature 004) communicates with the backend through an in-process shared state or event channel; no IPC or network protocol is required between UI and backend.
- Debug symbols (PDB or DWARF) are present in the binary being debugged; source-level stepping without symbols is out of scope.
- Thread-level operations (switching threads, per-thread stepping) are limited to a single selected thread for this feature; a full multi-thread debugger UI is out of scope.
- Watchpoints, conditional breakpoints, and expression evaluation at breakpoints are out of scope for this feature's initial delivery.
- The `Debugger::initialize()` / `Debugger::terminate()` lifecycle is managed by the application; the spec does not prescribe when these are called relative to the app lifecycle.
