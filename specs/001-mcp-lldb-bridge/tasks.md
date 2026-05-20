---
description: "Task list for Phase 1 — MCP + LLDB Bridge"
---

# Tasks: Phase 1 — MCP + LLDB Bridge

**Input**: Design documents from `specs/001-mcp-lldb-bridge/`

**Prerequisites**: plan.md ✅, spec.md ✅, data-model.md ✅, research.md ✅, contracts/ ✅, quickstart.md ✅

**Tests**: Not requested — test tasks omitted. Add integration test binary in Polish phase.

**Organization**: Tasks are grouped by user story to enable independent implementation and
testing of each story.

## Format: `[ID] [P?] [Story?] Description — file path`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: US1/US2/US3/US4 — maps to user stories in spec.md

## Path Conventions

Workspace root layout per plan.md:
```
Cargo.toml            # workspace manifest
apps/mcp-server/      # binary
crates/lldb-bridge/   # LLDB PyO3 backend
crates/runtime-core/  # session, entities, serialization
crates/protocol/      # MCP tool input/output types
```

---

## Phase 1: Setup

**Purpose**: Workspace and crate scaffolding — no logic yet.

- [X] T001 Create workspace `Cargo.toml` declaring members `["apps/mcp-server", "crates/runtime-core", "crates/lldb-bridge", "crates/protocol"]` with shared `[workspace.package]` (edition 2021, rust-version "1.75", license "MIT") — `Cargo.toml`
- [X] T002 [P] Create `crates/runtime-core/Cargo.toml` with dependencies: `thiserror`, `serde` (features = ["derive"]), `serde_json`, `tracing`, `uuid` (features = ["v4"]), `tokio` (features = ["sync"]) — `crates/runtime-core/Cargo.toml`
- [X] T003 [P] Create `crates/lldb-bridge/Cargo.toml` with dependencies: `pyo3` (features = ["auto-initialize"]), `tokio` (features = ["sync", "rt"]), `tracing`, `runtime-core` (path = "../runtime-core") — `crates/lldb-bridge/Cargo.toml`
- [X] T004 [P] Create `crates/protocol/Cargo.toml` with dependencies: `serde`, `serde_json`, `rmcp`, `tracing`, `runtime-core` (path = "../runtime-core") — `crates/protocol/Cargo.toml`
- [X] T005 Create `apps/mcp-server/Cargo.toml` with dependencies: `tokio` (features = ["full"]), `tracing`, `tracing-subscriber`, `clap` (features = ["derive"]), `rmcp`, `lldb-bridge`, `runtime-core`, `protocol` — `apps/mcp-server/Cargo.toml`
- [X] T006 [P] Create empty `src/lib.rs` stubs for all three crates and `src/main.rs` for mcp-server so `cargo check --workspace` passes — `crates/*/src/lib.rs`, `apps/mcp-server/src/main.rs`

**Checkpoint**: `cargo check --workspace` compiles with zero errors.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core types, traits, and async bridge that ALL user stories depend on.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

### Error Types

- [X] T007 Implement `DebuggerError` enum (`InvalidState`, `LLDBError`, `ProcessNotFound`, `BreakpointNotFound`, `ThreadNotFound`, `EvalError`, `SerializationError`, `ProtocolError`) using `thiserror` — `crates/runtime-core/src/error.rs`

### Core Entity Types

- [X] T008 [P] Implement `DebugTarget`, `SessionId` (uuid alias), `SessionState` enum (`Idle`, `Launching`, `Running`, `Paused(PauseReason)`, `Stepping`, `Terminated(i32)`, `Error(String)`), and `PauseReason` enum — `crates/runtime-core/src/session.rs`
- [X] T009 [P] Implement `DebugSession` struct (id, target, state, created_at, process) and `new()` constructor — `crates/runtime-core/src/session.rs`
- [X] T010 [P] Implement `DebugTarget`, `ProcessHandle` (pid, threads, selected_thread), `ThreadId` type alias — `crates/runtime-core/src/process.rs`
- [X] T011 [P] Implement `ThreadInfo` (id, name, state, stop_reason, frames), `ThreadState` enum, `StopReason` enum — `crates/runtime-core/src/process.rs`
- [X] T012 [P] Implement `StackFrame` (index, function_name, module, source_location, is_inlined) and `SourceLocation` (file, line, column) — `crates/runtime-core/src/process.rs`

### Session State Machine

- [X] T013 Implement `DebugSession::transition(event)` enforcing valid state transitions from data-model.md; return `DebuggerError::InvalidState` for illegal transitions — `crates/runtime-core/src/session.rs`

### LLDB Bridge Infrastructure

- [X] T014 Define `DebuggerBackend` trait in `crates/lldb-bridge/src/lib.rs` with async method signatures for all 13 operations: `launch_process`, `get_state`, `set_breakpoint`, `remove_breakpoint`, `list_breakpoints`, `continue_execution`, `pause_execution`, `step_over`, `step_into`, `step_out`, `read_locals`, `read_stack`, `evaluate_expression`, `list_threads` — `crates/lldb-bridge/src/lib.rs`
- [X] T015 Implement `LLDBCommand` enum (one variant per backend method, each carrying a `tokio::sync::oneshot::Sender<LLDBResult>` and input payload) and `LLDBResult` type — `crates/lldb-bridge/src/thread.rs`
- [X] T016 Implement `LLDBThread::spawn(mpsc::Receiver<LLDBCommand>)` — OS thread (via `std::thread::spawn`) that loops on the channel, dispatches to PyO3 backend, sends results back via oneshot — `crates/lldb-bridge/src/thread.rs`
- [X] T017 Implement `LLDBHandle::send(cmd)` async helper that sends a `LLDBCommand` to the thread and `await`s the oneshot response; add tracing spans for each operation — `crates/lldb-bridge/src/thread.rs`
- [X] T018 Implement `PythonBackend::new()` — acquires the Python GIL, imports the `lldb` module, asserts `lldb.SBDebugger.Initialize()` succeeds; return `LLDBError` on failure — `crates/lldb-bridge/src/python_backend.rs`

### Protocol Types

- [X] T019 [P] Implement all session tool input/output types: `LaunchInput`, `LaunchOutput`, `SessionStateOutput` — `crates/protocol/src/tools/session.rs`
- [X] T020 [P] Implement all breakpoint tool input/output types: `SetBreakpointInput`, `BreakpointOutput`, `RemoveBreakpointInput`, `ListBreakpointsOutput` — `crates/protocol/src/tools/breakpoints.rs`
- [X] T021 [P] Implement all execution tool input/output types: `StepInput`, `ExecutionEvent`, `ExecutionEventKind`, `PauseOutput` — `crates/protocol/src/tools/execution.rs`
- [X] T022 [P] Implement all inspection tool input/output types: `ReadLocalsInput`, `VariableOutput`, `ReadStackInput`, `StackOutput`, `EvalInput`, `EvalOutput`, `ThreadListOutput` — `crates/protocol/src/tools/inspection.rs`
- [X] T023 Implement `DebuggerError` → MCP JSON-RPC error code mapping (codes -32000 to -32005 per contracts/mcp-tools.md) — `crates/protocol/src/error.rs`

**Checkpoint**: `cargo check --workspace` still compiles. Foundation is complete — user story work can now begin in parallel.

---

## Phase 3: User Story 1 — Process Launch & Session Lifecycle (Priority: P1) 🎯 MVP

**Goal**: AI agent can launch a Rust binary under LLDB and query session state via MCP.

**Independent Test**: Run `mcp-server --executable ./target/debug/hello_world`, call
`launch_process {executable: ...}`, verify response has `state: "Running"` and non-zero PID.
Call `get_session_state`, verify it reflects running state.

### LLDB Backend for US1

- [X] T024 [US1] Implement `PythonBackend::launch_process(target: DebugTarget)` — create `SBDebugger`, load target, call `SBTarget.LaunchSimple()`, capture PID, start event listener thread, store `SBProcess`; return `LaunchOutput` — `crates/lldb-bridge/src/python_backend.rs`
- [X] T025 [US1] Implement process exit listener inside `PythonBackend` that detects `eBroadcastBitStateChanged` LLDB events and transitions `SessionState` to `Terminated(exit_code)` — `crates/lldb-bridge/src/python_backend.rs`
- [X] T026 [US1] Implement `PythonBackend::get_state()` — read current `SBProcess` state, map to `SessionState`, return `SessionStateOutput` — `crates/lldb-bridge/src/python_backend.rs`

### MCP Server for US1

- [X] T027 [US1] Implement `apps/mcp-server/src/main.rs` — parse CLI args (`--executable`, `--args`, `--log-level`, `--transport`), initialize `tracing_subscriber`, construct `LLDBHandle`, start rmcp server — `apps/mcp-server/src/main.rs`
- [X] T028 [US1] Implement rmcp tool registration in `apps/mcp-server/src/server.rs` — register all 13 MCP tools with their handlers; wire `LLDBHandle` into handler context — `apps/mcp-server/src/server.rs`
- [X] T029 [US1] Implement `handle_launch_process` MCP handler — deserialize `LaunchInput`, call `LLDBHandle::launch_process`, serialize `LaunchOutput`, map errors via `protocol::error` — `apps/mcp-server/src/handlers/session.rs`
- [X] T030 [US1] Implement `handle_get_session_state` MCP handler — call `LLDBHandle::get_state`, serialize `SessionStateOutput` — `apps/mcp-server/src/handlers/session.rs`
- [X] T031 [US1] Add `tracing::info!` and `tracing::error!` spans for all session lifecycle events (launch attempt, process start, process exit, error) — `apps/mcp-server/src/handlers/session.rs`

**Checkpoint**: User Story 1 is fully functional and independently testable. The AI can open a process and confirm it is running.

---

## Phase 4: User Story 2 — Breakpoint Management & Execution Control (Priority: P2)

**Goal**: AI agent can set breakpoints, resume execution, hit breakpoints, and step through code.

**Independent Test**: Set breakpoint at a known line, call `continue_execution`, verify
`BreakpointHit` event at the expected location. Call `step_over`, verify line advances by 1.

### Breakpoint Entity

- [X] T032 [P] [US2] Implement `Breakpoint`, `BreakpointId` (u32 alias), `BreakpointKind` (`SourceLine`, `FunctionName`, `Address`, `Regex`), `BreakpointLocation` in `crates/runtime-core/src/breakpoint.rs`
- [X] T033 [US2] Implement breakpoint lifecycle state in `crates/runtime-core/src/breakpoint.rs` — track `enabled`, `hit_count`, `locations`; add `increment_hit_count()` and `toggle_enabled()` methods

### LLDB Backend for US2

- [X] T034 [US2] Implement `PythonBackend::set_breakpoint(kind, condition)` — call `SBTarget.BreakpointCreateByLocation()` or `SBTarget.BreakpointCreateByName()`, apply condition if set, return resolved locations — `crates/lldb-bridge/src/python_backend.rs`
- [X] T035 [P] [US2] Implement `PythonBackend::remove_breakpoint(id)` — call `SBTarget.BreakpointDelete(id)`; return `BreakpointNotFound` if ID unknown — `crates/lldb-bridge/src/python_backend.rs`
- [X] T036 [P] [US2] Implement `PythonBackend::list_breakpoints()` — iterate `SBTarget.breakpoints`, map to `Breakpoint` structs — `crates/lldb-bridge/src/python_backend.rs`
- [X] T037 [US2] Implement `PythonBackend::continue_execution()` — call `SBProcess.Continue()`, block on LLDB event loop until next stop event, map stop reason to `ExecutionEvent`, update `SessionState` — `crates/lldb-bridge/src/python_backend.rs`
- [X] T038 [P] [US2] Implement `PythonBackend::pause_execution()` — call `SBProcess.Stop()`, wait for stop event, return `ExecutionEvent` with `UserRequest` reason — `crates/lldb-bridge/src/python_backend.rs`
- [X] T039 [P] [US2] Implement `PythonBackend::step_over(thread_id)` — call `SBThread.StepOver()`, wait for stop, return `StepComplete` event — `crates/lldb-bridge/src/python_backend.rs`
- [X] T040 [P] [US2] Implement `PythonBackend::step_into(thread_id)` — call `SBThread.StepInto()`, wait for stop, return `StepComplete` event — `crates/lldb-bridge/src/python_backend.rs`
- [X] T041 [P] [US2] Implement `PythonBackend::step_out(thread_id)` — call `SBThread.StepOut()`, wait for stop, return `StepComplete` event — `crates/lldb-bridge/src/python_backend.rs`

### MCP Handlers for US2

- [X] T042 [P] [US2] Implement `handle_set_breakpoint`, `handle_remove_breakpoint`, `handle_list_breakpoints` MCP handlers — `apps/mcp-server/src/handlers/breakpoints.rs`
- [X] T043 [US2] Implement `handle_continue_execution` MCP handler — must await LLDB stop event (potentially long-running); use `timeout` configurable per session — `apps/mcp-server/src/handlers/execution.rs`
- [X] T044 [P] [US2] Implement `handle_pause_execution`, `handle_step_over`, `handle_step_into`, `handle_step_out` MCP handlers — `apps/mcp-server/src/handlers/execution.rs`
- [X] T045 [US2] Add tracing spans for all breakpoint events (set, hit, removed) and execution control transitions — `apps/mcp-server/src/handlers/breakpoints.rs`, `apps/mcp-server/src/handlers/execution.rs`

**Checkpoint**: User Stories 1 AND 2 are independently functional. The AI can set breakpoints and navigate execution.

---

## Phase 5: User Story 3 — Runtime State Inspection (Priority: P3)

**Goal**: AI agent can read locals with semantic context, inspect the call stack, evaluate
expressions, and list threads.

**Independent Test**: At a breakpoint where `remaining_width` is negative, call `read_locals`
with `probe_context: "measure_layout"`. Verify `measure_layout.remaining_width` appears in the
response with a negative value.

### Variable Entity & Serialization

- [X] T046 [P] [US3] Implement `Variable`, `VariableValue`, `ScalarValue`, `SemanticAnnotation` per data-model.md — `crates/runtime-core/src/variable.rs`
- [X] T047 [P] [US3] Implement `EvalResult` (expression, value, type_name, error) — `crates/runtime-core/src/variable.rs`
- [X] T048 [US3] Implement `Serializer::serialize(value, opts)` in `crates/runtime-core/src/serialization.rs` — recursive descent with depth limit, array truncation at `max_array_elements`, string truncation at `max_string_bytes`, cyclic ref detection via address `HashSet` — `crates/runtime-core/src/serialization.rs`
- [X] T049 [US3] Implement `probe!` declarative macro — takes `(context, var1, var2, ...)` and expands to a `SemanticProbe { context, variables }` filter spec; stored in `ProbeRegistry` keyed by context name — `crates/runtime-core/src/probe.rs`
- [X] T050 [US3] Implement `ProbeRegistry` — in-memory map from context name to list of variable names; `register(context, vars)` and `lookup(context) -> Option<&[String]>` — `crates/runtime-core/src/probe.rs`

### LLDB Backend for US3

- [X] T051 [US3] Implement `PythonBackend::read_locals(thread_id, frame_index, probe_context, max_depth)` — call `SBFrame.variables`, convert each `SBValue` to `Variable` using `Serializer`; if `probe_context` set, filter by `ProbeRegistry` and set `SemanticAnnotation` — `crates/lldb-bridge/src/python_backend.rs`
- [X] T052 [P] [US3] Implement `PythonBackend::read_stack(thread_id, max_frames)` — iterate `SBThread.frames`, map each to `StackFrame` with `SBFrame.GetFunctionName()`, `SBFrame.GetLineEntry()` — `crates/lldb-bridge/src/python_backend.rs`
- [X] T053 [P] [US3] Implement `PythonBackend::evaluate_expression(expression, thread_id, frame_index)` — call `SBFrame.EvaluateExpression()`, convert result `SBValue` to `EvalResult`; map LLDB error to `EvalError` — `crates/lldb-bridge/src/python_backend.rs`
- [X] T054 [P] [US3] Implement `PythonBackend::list_threads()` — iterate `SBProcess.threads`, map each to `ThreadInfo` with name, state, stop reason, frame count — `crates/lldb-bridge/src/python_backend.rs`

### MCP Handlers for US3

- [X] T055 [US3] Implement `handle_read_locals` MCP handler — deserialize `ReadLocalsInput` (thread_id, frame_index, probe_context, max_depth), call backend, serialize `VariableOutput` list — `apps/mcp-server/src/handlers/inspection.rs`
- [X] T056 [P] [US3] Implement `handle_read_stack` MCP handler — deserialize `ReadStackInput`, call backend, serialize `StackOutput` — `apps/mcp-server/src/handlers/inspection.rs`
- [X] T057 [P] [US3] Implement `handle_evaluate_expression` MCP handler — `apps/mcp-server/src/handlers/inspection.rs`
- [X] T058 [P] [US3] Implement `handle_list_threads` MCP handler — `apps/mcp-server/src/handlers/inspection.rs`
- [X] T059 [US3] Add tracing spans for all inspection calls (variable count, probe_context used, eval expression text) — `apps/mcp-server/src/handlers/inspection.rs`

**Checkpoint**: User Stories 1, 2, and 3 are all independently functional. The AI can inspect full runtime state with semantic context.

---

## Phase 6: User Story 4 — Panic Detection (Priority: P4)

**Goal**: AI agent receives a structured `PanicDetected` event with message and location when
the target binary panics.

**Independent Test**: Run a binary that panics with `index out of bounds`, call
`continue_execution`, verify response event kind is `PanicDetected` with a `message` containing
"index out of bounds" and a valid source location.

- [X] T060 [US4] Add `PanicDetected { message: String }` variant to `ExecutionEventKind` in `crates/protocol/src/tools/execution.rs`
- [X] T061 [US4] Implement LLDB panic detection in `PythonBackend::continue_execution()` — detect `SIGABRT` stop reason and/or Rust `__rust_begin_short_backtrace` function breakpoint; extract panic message from `std::panicking::begin_panic` argument or LLDB stop description; return `PanicDetected` event — `crates/lldb-bridge/src/python_backend.rs`
- [X] T062 [US4] Propagate `PanicDetected` correctly through `handle_continue_execution` MCP handler — ensure panic message and source location are serialized into the response per `contracts/mcp-tools.md` — `apps/mcp-server/src/handlers/execution.rs`
- [X] T063 [US4] Verify that `read_locals` and `read_stack` remain callable after `PanicDetected` (session state remains `Paused(PauseReason::Panic)`) — `crates/runtime-core/src/session.rs`
- [X] T064 [US4] Add tracing event for every panic detected (binary name, panic message, location) — `crates/lldb-bridge/src/python_backend.rs`

**Checkpoint**: All four user stories are independently functional.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Improvements that affect all user stories.

- [X] T065 [P] Add `--log-level` CLI argument wiring to `tracing_subscriber::EnvFilter` in `apps/mcp-server/src/main.rs`
- [X] T066 [P] Add `--transport http --port <N>` option to enable HTTP/SSE transport alongside stdio in `apps/mcp-server/src/main.rs`
- [X] T067 Create integration test binary `crates/debug-target-example/` — a Rust binary with known breakpoints, a panic path, a multi-threaded path, and a struct with nested fields — `crates/debug-target-example/src/main.rs`
- [X] T068 Run the quickstart.md validation checklist manually against `debug-target-example`; mark all items checked in `specs/001-mcp-lldb-bridge/quickstart.md`
- [X] T069 [P] Add `///` doc comments with usage examples to all `pub` items in `crates/runtime-core/src/` (Constitution Principle VII)
- [X] T070 [P] Add `///` doc comments with usage examples to all `pub` items in `crates/protocol/src/` (Constitution Principle VII)
- [X] T071 [P] Add `///` doc comments with usage examples to all `pub` items in `crates/lldb-bridge/src/lib.rs` (Constitution Principle VII)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — BLOCKS all user stories
- **US1 (Phase 3)**: Depends on Foundational — first user story; unlocks MVP
- **US2 (Phase 4)**: Depends on Foundational — can start in parallel with US1 if staffed
- **US3 (Phase 5)**: Depends on Foundational — can start in parallel with US1/US2 if staffed
- **US4 (Phase 6)**: Depends on US2 (`continue_execution` must exist) — otherwise independent
- **Polish (Phase 7)**: Depends on all desired user stories being complete

### User Story Dependencies

- **US1 (P1)**: Independent after Foundational
- **US2 (P2)**: Independent after Foundational (does not need US1 complete)
- **US3 (P3)**: Independent after Foundational (does not need US1 or US2 complete)
- **US4 (P4)**: Depends on US2 (`continue_execution` backend must exist — T037)

### Within Each User Story

- LLDB backend methods before MCP handlers
- Entity types before backend implementations
- Serialization before `read_locals` backend
- `probe.rs` before `read_locals` with `probe_context`

### Parallel Opportunities

- All Setup tasks T002–T006 can run in parallel
- Foundational entity tasks T008–T012 can run in parallel
- Foundational protocol types T019–T022 can run in parallel
- Once Foundational is done: US1, US2, US3 backend tasks can start in parallel
- Within US2: LLDB step methods T039–T041 can run in parallel
- Within US3: `read_stack`, `evaluate_expression`, `list_threads` backends T052–T054 can run in parallel
- Polish tasks T065–T071: all parallelizable

---

## Parallel Execution Examples

### Foundational Phase Parallel Launch

```
Parallel group A (entity types):
  T008: DebugTarget, ProcessHandle, ThreadId
  T009: DebugSession, new() constructor
  T010: ProcessHandle, ThreadId
  T011: ThreadInfo, ThreadState, StopReason
  T012: StackFrame, SourceLocation

Parallel group B (protocol types):
  T019: session tool types
  T020: breakpoint tool types
  T021: execution tool types
  T022: inspection tool types
```

### US3 Parallel Launch (after T048, T049, T050 complete)

```
Parallel group:
  T052: PythonBackend::read_stack
  T053: PythonBackend::evaluate_expression
  T054: PythonBackend::list_threads
  T056: handle_read_stack MCP handler
  T057: handle_evaluate_expression MCP handler
  T058: handle_list_threads MCP handler
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (T001–T006)
2. Complete Phase 2: Foundational (T007–T023) — CRITICAL, blocks everything
3. Complete Phase 3: US1 (T024–T031)
4. **STOP AND VALIDATE**: `launch_process` returns running state, `get_session_state` reflects it
5. Ship MVP — AI can open a process

### Incremental Delivery

1. Setup + Foundational → workspace compiles
2. US1 → AI can launch process (MVP)
3. US2 → AI can navigate execution
4. US3 → AI can inspect state with semantic probes ← primary value delivery
5. US4 → AI detects panics autonomously
6. Polish → production-quality server

---

## Notes

- `[P]` = parallelizable (different files, no unmet dependencies)
- `[US#]` = user story label for traceability
- LLDB backend methods are in `crates/lldb-bridge/src/python_backend.rs` throughout
- All LLDB backend methods dispatch through `LLDBHandle` mpsc channel (never called directly from async context)
- `tracing` instrumentation is mandatory per Constitution Principle: all runtime-event-handling code paths MUST emit structured log events
- `unsafe` blocks required by PyO3 GIL handling MUST carry inline safety proof + GH issue reference (Constitution Principle VI)

