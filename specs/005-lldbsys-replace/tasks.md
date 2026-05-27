# Tasks: Native LLDB Backend via lldb-sys

**Input**: Design documents from `specs/005-lldbsys-replace/`

**Feature**: Replace `crates/lldb-bridge` (DAP + codelldb external process) with `crates/lldb-native` (lldb-safe in-process FFI). Extract `DebugBackend` trait into `runtime-core`. Wire into `apps/mcp-server`.

**Organization**: Tasks are grouped by user story. Each phase is independently buildable and testable.

---

## Phase 1: Setup (Crate Scaffolding)

**Purpose**: Create the `crates/lldb-native` workspace crate with the correct Cargo manifest and module skeleton so that subsequent phases compile.

- [ ] T001 Add `"crates/lldb-native"` to the `[workspace] members` array in the root `Cargo.toml`
- [ ] T002 Create `crates/lldb-native/Cargo.toml` with: `name = "lldb-native"`, edition/rust-version/license from workspace, deps: `lldb-safe = { path = "../../.." }` (adjust relative path to `c:\workspace\lldb-sys\crates\lldb-safe`), `runtime-core`, `protocol`, `tokio` (workspace, features: ["sync", "rt"]), `tracing` (workspace), `thiserror` (workspace), `async-trait = "0.1"`, and `[target.'cfg(windows)'.dependencies] windows-sys = { version = "0.59", features = ["Win32_System_LibraryLoader"] }`
- [ ] T003 [P] Create `crates/lldb-native/src/lib.rs` with module declarations: `pub mod handle; mod thread; mod command; mod mapping;` and `pub use handle::LldbNativeHandle;`
- [ ] T004 [P] Create `crates/lldb-native/src/command.rs` with an empty `LldbCommand` enum placeholder: `pub(crate) enum LldbCommand {}`
- [ ] T005 [P] Create `crates/lldb-native/src/mapping.rs` as an empty module with a `// type conversions: lldb-safe → runtime-core` comment
- [ ] T006 [P] Create `crates/lldb-native/src/thread.rs` with an empty `pub(crate) struct LldbDebugThread;`
- [ ] T007 [P] Create `crates/lldb-native/src/handle.rs` with a stub `pub struct LldbNativeHandle;` and `impl LldbNativeHandle { pub fn spawn() -> Result<Self, runtime_core::error::DebuggerError> { todo!() } }`
- [ ] T008 Verify `cargo check --package lldb-native` compiles with no errors (stubs are acceptable)

**Checkpoint**: `cargo check --package lldb-native` passes.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Two independent foundations that ALL user stories depend on: (A) `Frame::source_location()` in lldb-safe for source-level stack frames, and (B) `DebugBackend` trait in `runtime-core` for the mcp-server decoupling.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

### A — lldb-safe Extension (SBLineEntry)

- [ ] T009 Add C++ wrapper for `SBLineEntry` in `c:\workspace\lldb-sys\crates\lldb-sys\wrapper\include\lldb_c.h`: declare `LLDB_SBFrame_GetLineEntryFile(SBFrameRef frame, char* buf, size_t buf_len)` and `LLDB_SBFrame_GetLineEntryLine(SBFrameRef frame)` returning `uint32_t`
- [ ] T010 Create `c:\workspace\lldb-sys\crates\lldb-sys\wrapper\src\SBLineEntry.cpp` implementing the two functions: call `frame->GetLineEntry().GetFileSpec().GetPath(buf, buf_len)` and `frame->GetLineEntry().GetLine()` on the dereferenced `SBFrameRef`
- [ ] T011 Add the two new function declarations to `c:\workspace\lldb-sys\crates\lldb-sys\src\lib.rs` under the existing `extern "C"` block: `pub fn LLDB_SBFrame_GetLineEntryFile(frame: SBFrameRef, buf: *mut i8, buf_len: usize); pub fn LLDB_SBFrame_GetLineEntryLine(frame: SBFrameRef) -> u32;`
- [ ] T012 Add `pub fn source_location(&self) -> Option<(String, u32)>` to `c:\workspace\lldb-sys\crates\lldb-safe\src\frame.rs`: allocate a 1024-byte buffer, call `LLDB_SBFrame_GetLineEntryFile`, convert to `String`, call `LLDB_SBFrame_GetLineEntryLine`, return `Some((path, line))` only if path is non-empty and line > 0
- [ ] T013 Verify `cargo build --package lldb-safe` (in `c:\workspace\lldb-sys`) compiles without errors

### B — `DebugBackend` Trait in runtime-core

- [ ] T014 Add `async-trait = "0.1"` to `[dependencies]` in `crates/runtime-core/Cargo.toml`
- [ ] T015 Create `crates/runtime-core/src/backend.rs` containing the `DebugBackend` trait exactly as specified in `specs/005-lldbsys-replace/contracts/debug-backend-trait.md` (all 14 async methods, `async_trait` attribute, `Send + Sync` supertrait)
- [ ] T016 Add `pub mod backend; pub use backend::DebugBackend;` to `crates/runtime-core/src/lib.rs`
- [ ] T017 Verify `cargo test --package runtime-core` passes

**Checkpoint**: `cargo build --package lldb-safe` and `cargo test --package runtime-core` both pass.

---

## Phase 3: User Story 1 — Launch and Break at Breakpoint (Priority: P1) 🎯 MVP

**Goal**: Start a debug session in-process via lldb-safe, set a source-level breakpoint, launch the binary, and pause at the breakpoint — with no external process spawned.

**Independent Test**: Run `cargo test --package lldb-native launch_and_break` — launches `debug-target-example`, sets a breakpoint at `main.rs:20`, calls `continue_execution()`, asserts the returned `ExecutionEvent` is `BreakpointHit`.

- [ ] T018 [US1] Expand `LldbCommand` enum in `crates/lldb-native/src/command.rs` with variants for US1: `LaunchProcess { target: DebugTarget, reply: tokio::sync::oneshot::Sender<Result<(u32, SessionState), DebuggerError>> }`, `GetState { reply: ... }`, `SetBreakpoint { kind: BreakpointKind, condition: Option<String>, reply: ... }`, `Continue { reply: tokio::sync::oneshot::Sender<Result<ExecutionEvent, DebuggerError>> }`, `ListBreakpoints { reply: ... }`, and `Shutdown`
- [ ] T019 [US1] Implement `mapping::state_to_session_state(state: lldb_safe::State, process: &lldb_safe::Process) -> SessionState` in `crates/lldb-native/src/mapping.rs` using the mapping table in `specs/005-lldbsys-replace/data-model.md` (Exited → call `process.exit_status()` for the exit code)
- [ ] T020 [US1] Implement `mapping::stop_reason_to_pause_reason(thread: &lldb_safe::Thread) -> PauseReason` in `crates/lldb-native/src/mapping.rs`: for `StopReason::Breakpoint`, read `thread.stop_reason_data_at(0)` as the breakpoint ID; for `StopReason::Trace`/`PlanComplete` → `Step`; others as per data-model.md
- [ ] T021 [US1] Implement `LldbDebugThread` struct in `crates/lldb-native/src/thread.rs`: fields `debugger: lldb_safe::Debugger`, `target: Option<lldb_safe::Target>`, `process: Option<lldb_safe::Process>`, `state: SessionState`; plus `pub(crate) fn run(rx: mpsc::Receiver<LldbCommand>)` which calls `set_lldb_dll_dir` (from `LLDB_DLL_DIR` env var), then `Debugger::initialize()`, then loops on `rx.recv()` dispatching each variant, and calls `Debugger::terminate()` on exit
- [ ] T022 [US1] Implement `LldbDebugThread::handle_launch_process()` in `crates/lldb-native/src/thread.rs`: call `self.debugger.create_target_simple(&exe_str)`, store in `self.target`; call `target.launch(&[], &[], cwd, true)` (stop at entry), store in `self.process`; wait for first stop event by calling `process.state()` in a spin loop until `state.is_stopped()`; update `self.state` to `Paused(Breakpoint(0))`; return `(pid, state)` where pid is `process.pid() as u32`
- [ ] T023 [US1] Implement `LldbDebugThread::handle_set_breakpoint()` in `crates/lldb-native/src/thread.rs`: match on `BreakpointKind` — for `SourceLine { file, line }` call `target.breakpoint_by_location(file.to_str().unwrap(), line)`, for `FunctionName { name }` call `target.breakpoint_by_name(name, None)`, for `Address { addr }` call `target.breakpoint_by_address(addr)`; build and return `runtime_core::breakpoint::Breakpoint` with the LLDB breakpoint's `id()` as the id
- [ ] T024 [US1] Implement `LldbDebugThread::handle_continue()` in `crates/lldb-native/src/thread.rs`: call `process.resume()`, then spin on `process.state()` until `state.is_stopped()` or `Exited`; map the resulting state and selected thread's stop reason to `ExecutionEvent { kind, thread_id, location: None }`; update `self.state`
- [ ] T025 [US1] Implement `LldbNativeHandle` in `crates/lldb-native/src/handle.rs`: `spawn()` creates a `mpsc::channel::<LldbCommand>(32)`, spawns `std::thread::spawn(move || LldbDebugThread::run(rx))`, stores the sender; `async fn launch_process()` creates a `oneshot` pair, sends `LldbCommand::LaunchProcess`, awaits the receiver; same pattern for `get_state()`, `set_breakpoint()`, `continue_execution()`, `list_breakpoints()`
- [ ] T026 [US1] Add `#[async_trait::async_trait] impl DebugBackend for LldbNativeHandle` in `crates/lldb-native/src/handle.rs` delegating `launch_process`, `get_state`, `set_breakpoint`, `continue_execution`, `list_breakpoints` to the corresponding `self.*()` methods; stub remaining methods with `Err(DebuggerError::DebuggerError("not yet implemented".into()))`
- [ ] T027 [US1] Create `crates/lldb-native/tests/integration.rs` with test `launch_and_break`: build path to `debug-target-example` binary from `CARGO_MANIFEST_DIR`, call `LldbNativeHandle::spawn()`, `set_breakpoint(BreakpointKind::SourceLine { file: "main.rs".into(), line: 20 }, None)`, `launch_process(target)`, `continue_execution()`, assert `event.kind == ExecutionEventKind::BreakpointHit`

**Checkpoint**: `cargo test --package lldb-native launch_and_break` passes. Session launches, stops at breakpoint, no external codelldb process running.

---

## Phase 4: User Story 2 — Step Through Code (Priority: P2)

**Goal**: From a paused session, call step-over / step-into / step-out and observe the source position advancing correctly.

**Independent Test**: After `launch_and_break` succeeds, call `step_over()` three times and assert each `ExecutionEvent` is `StepComplete` with a different source line number.

- [ ] T028 [P] [US2] Add `StepOver`, `StepInto`, `StepOut` variants (each with `thread_id: Option<ThreadId>` and `reply: oneshot::Sender<Result<ExecutionEvent, DebuggerError>>`) to `LldbCommand` in `crates/lldb-native/src/command.rs`
- [ ] T029 [US2] Implement `mapping::frame_to_stack_frame(frame: &lldb_safe::Frame, index: u32) -> StackFrame` in `crates/lldb-native/src/mapping.rs`: call `frame.function_name()`, `frame.source_location()` (the new method from T012), `frame.is_inlined()`, map to `runtime_core::process::StackFrame`
- [ ] T030 [US2] Implement `LldbDebugThread::handle_step_over/into/out()` in `crates/lldb-native/src/thread.rs`: get selected thread from `process.selected_thread()`, call `thread.step_over()`/`step_into()`/`step_out()`, then spin on `process.state()` until stopped; build `ExecutionEvent { kind: StepComplete, thread_id, location }` using `mapping::frame_to_stack_frame` on the thread's selected frame
- [ ] T031 [US2] Implement `LldbNativeHandle::step_over()`, `step_into()`, `step_out()` in `crates/lldb-native/src/handle.rs` via mpsc + oneshot (same pattern as T025)
- [ ] T032 [US2] Extend `impl DebugBackend for LldbNativeHandle` in `crates/lldb-native/src/handle.rs` replacing the stubs for `step_over`, `step_into`, `step_out` with real delegation
- [ ] T033 [US2] Extend `crates/lldb-native/tests/integration.rs` with test `step_three_times`: reuse session from `launch_and_break` setup, call `step_over()` three times, collect the three `ExecutionEvent`s, assert all are `StepComplete`, assert `location.line` increments (or at least differs between calls)

**Checkpoint**: `cargo test --package lldb-native step_three_times` passes.

---

## Phase 5: User Story 3 — Inspect Variables (Priority: P3)

**Goal**: After pausing, retrieve local variable names, types, and values; expand compound types; evaluate expressions.

**Independent Test**: After pausing at a breakpoint inside a function with a `Vec<i32>` local, call `read_locals()` and assert the variable is listed with a non-empty type name and display value.

- [ ] T034 [P] [US3] Add `ReadLocals`, `ReadStack`, `EvaluateExpression`, `ListThreads` variants to `LldbCommand` in `crates/lldb-native/src/command.rs` (each with appropriate fields and `oneshot::Sender` reply)
- [ ] T035 [US3] Implement `mapping::value_to_variable(value: &lldb_safe::Value) -> Variable` in `crates/lldb-native/src/mapping.rs`: use `value.name()` → `Variable.name`, `value.display_type_name()` → `Variable.type_name`, `value.value_string()` → `VariableValue::Opaque { summary }` for leaf values; for compound types (`value.num_children() > 0`), build `VariableValue::Composite { fields: vec![] }` (children loaded on demand; empty here)
- [ ] T036 [US3] Implement `LldbDebugThread::handle_read_locals()` in `crates/lldb-native/src/thread.rs`: get selected thread → selected frame → iterate variables by calling `frame.find_variable(name)` is not enough; instead call `frame.evaluate_expression("x")` is also not the right approach. Use the correct LLDB approach: get `SBFrame::GetVariables` if exposed, or call `frame.find_variable` for each known name. **Note**: `lldb-safe` does not yet expose `GetVariables`. For this implementation, call `Frame::evaluate_expression` with each variable name from a list of local variable names obtained by inspecting the frame's symbol context. If `SBFrame::GetVariables` is not exposed, add a `Frame::variables() -> Vec<Value>` method to `lldb-safe` first (add to `c:\workspace\lldb-sys\crates\lldb-safe\src\frame.rs`, wrapping `SBFrame::GetVariables(true, false, false, false)` via a new C++ wrapper)
- [ ] T037 [US3] Add C++ wrapper `LLDB_SBFrame_GetVariables(SBFrameRef frame, bool arguments, bool locals, bool statics, bool in_scope)` returning a new `SBValueListRef` handle type to `c:\workspace\lldb-sys\crates\lldb-sys\wrapper\`, and expose `Frame::variables(arguments: bool, locals: bool, statics: bool) -> Vec<Value>` in `c:\workspace\lldb-sys\crates\lldb-safe\src\frame.rs`; verify `cargo build --package lldb-safe` passes
- [ ] T038 [US3] Implement `LldbDebugThread::handle_read_locals()` in `crates/lldb-native/src/thread.rs` using `frame.variables(true, true, false)` → map each `Value` via `mapping::value_to_variable`
- [ ] T039 [US3] Implement `LldbDebugThread::handle_read_stack()` in `crates/lldb-native/src/thread.rs`: get selected thread → iterate `thread.num_frames()` frames calling `thread.frame_at(i)` → map each via `mapping::frame_to_stack_frame`; limit to `max_frames`
- [ ] T040 [P] [US3] Implement `LldbDebugThread::handle_evaluate_expression()` and `handle_list_threads()` in `crates/lldb-native/src/thread.rs`: `evaluate_expression` → `frame.evaluate_expression(expr)` → `mapping::value_to_variable` → return as `EvalResult`; `list_threads` → iterate `process.num_threads()` → return `Vec<ThreadInfo>` with `id`, `name`, `state`
- [ ] T041 [US3] Implement `LldbNativeHandle::read_locals()`, `read_stack()`, `evaluate_expression()`, `list_threads()` in `crates/lldb-native/src/handle.rs` via mpsc + oneshot
- [ ] T042 [US3] Extend `impl DebugBackend for LldbNativeHandle` replacing stubs for `read_locals`, `read_stack`, `evaluate_expression`, `list_threads` with real delegation in `crates/lldb-native/src/handle.rs`
- [ ] T043 [US3] Extend `crates/lldb-native/tests/integration.rs` with test `read_locals_at_breakpoint`: pause at a breakpoint inside a function that has at least one local variable, call `read_locals(None, 0, None, 1)`, assert the result contains at least one `Variable` with a non-empty name and type_name

**Checkpoint**: `cargo test --package lldb-native read_locals_at_breakpoint` passes.

---

## Phase 6: User Story 4 — Dynamic Breakpoints (Priority: P4)

**Goal**: Add and remove breakpoints during a live session; verify new breakpoints halt execution and removed ones do not.

**Independent Test**: Pause at BP1, add BP2 on a line not yet reached, continue (process pauses at BP2), remove BP2, continue again — process does not pause at BP2's line.

- [ ] T044 [P] [US4] Add `RemoveBreakpoint { id: BreakpointId, reply: oneshot::Sender<Result<(), DebuggerError>> }` and `ListBreakpoints { reply: oneshot::Sender<Result<Vec<Breakpoint>, DebuggerError>> }` to `LldbCommand` in `crates/lldb-native/src/command.rs` (these may already be present from T018; verify and add if missing)
- [ ] T045 [US4] Implement `LldbDebugThread::handle_remove_breakpoint()` in `crates/lldb-native/src/thread.rs`: call `self.target.as_ref()?.delete_breakpoint(id as u32)`, return `Ok(())` on success or `Err(BreakpointNotFound(id))` if false
- [ ] T046 [US4] Implement `LldbDebugThread::handle_list_breakpoints()` in `crates/lldb-native/src/thread.rs`: iterate `target.num_breakpoints()` and `target.breakpoint_at(i)`, map each to `runtime_core::breakpoint::Breakpoint`
- [ ] T047 [US4] Implement `LldbNativeHandle::remove_breakpoint()` and `list_breakpoints()` in `crates/lldb-native/src/handle.rs` via mpsc + oneshot
- [ ] T048 [US4] Extend `impl DebugBackend for LldbNativeHandle` replacing stubs for `remove_breakpoint` and `list_breakpoints` in `crates/lldb-native/src/handle.rs`
- [ ] T049 [US4] Extend `crates/lldb-native/tests/integration.rs` with test `dynamic_breakpoints`: launch session, set BP1, continue to BP1, set BP2 at a later line, continue (assert BP2 hit), remove BP2, continue (assert process does not hit BP2 again — it terminates or hits another bp)

**Checkpoint**: `cargo test --package lldb-native dynamic_breakpoints` passes.

---

## Phase 7: mcp-server Wiring

**Purpose**: Replace the hard `lldb-bridge` dependency in `apps/mcp-server` with `Arc<dyn DebugBackend>`, add a `--backend` CLI flag, and add `WindowsDebugHandle: DebugBackend` for the win-debug-bridge alternative.

- [ ] T050 Add `#[async_trait::async_trait] impl DebugBackend for WindowsDebugHandle` in `crates/win-debug-bridge/src/thread.rs`: delegate each method to the existing `self.launch_process()` etc. methods (the signatures already match — this is a mechanical `impl` wrapper); add `async-trait = "0.1"` to `crates/win-debug-bridge/Cargo.toml`
- [ ] T051 Add `lldb-native.workspace = true` to `[dependencies]` in `apps/mcp-server/Cargo.toml`; remove `lldb-bridge.workspace = true`
- [ ] T052 Refactor `apps/mcp-server/src/handlers/session.rs`: change `SessionContext.handle: LldbDebugHandle` to `backend: Arc<dyn DebugBackend>` (import `runtime_core::DebugBackend`); update `SessionContext::new()` parameter accordingly; replace `ctx.handle.*()` calls with `ctx.backend.*()` throughout the file
- [ ] T053 Update `apps/mcp-server/src/handlers/breakpoints.rs`, `execution.rs`, `inspection.rs`: replace `ctx.handle.*()` with `ctx.backend.*()` for any handler that calls backend methods directly
- [ ] T054 Add `#[derive(clap::ValueEnum)] enum BackendChoice { LldbNative, WinDebugBridge }` and `--backend <BACKEND>` arg to the CLI struct in `apps/mcp-server/src/main.rs` (default: `LldbNative`)
- [ ] T055 In `apps/mcp-server/src/main.rs`, construct `let backend: Arc<dyn DebugBackend> = match args.backend { LldbNative => Arc::new(LldbNativeHandle::spawn()?), WinDebugBridge => Arc::new(WindowsDebugHandle::spawn()?) };` and pass it to `SessionContext::new(backend, view)`
- [ ] T056 Verify `cargo build --package mcp-server` compiles with no errors

**Checkpoint**: `cargo build --package mcp-server` passes. Running with `--backend lldb-native` launches without error.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [ ] T057 [P] Add `///` doc comments to all `pub` items in `crates/lldb-native/src/handle.rs` and `crates/lldb-native/src/lib.rs`; add a usage example in the crate-level doc comment showing `spawn()` → `set_breakpoint()` → `launch_process()` → `continue_execution()`
- [ ] T058 [P] Add `///` doc comments and usage example to `DebugBackend` trait in `crates/runtime-core/src/backend.rs`
- [ ] T059 Mark `crates/lldb-bridge/Cargo.toml` description as `"DEPRECATED: use lldb-native"` and add `#[deprecated(note = "use LldbNativeHandle from the lldb-native crate")]` to `LldbDebugHandle::spawn()` in `crates/lldb-bridge/src/handle.rs`
- [ ] T060 [P] Add `tracing::instrument` attributes and `info!` / `error!` events to all public methods of `LldbDebugThread` in `crates/lldb-native/src/thread.rs` following the pattern used in `crates/win-debug-bridge/src/windows_backend.rs`
- [ ] T061 Run `cargo test --workspace` and resolve any compilation errors or test failures

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS all user story phases**
- **US1 (Phase 3)**: Depends on Foundational
- **US2 (Phase 4)**: Depends on US1 (stepping requires a running session)
- **US3 (Phase 5)**: Depends on US1; can overlap with US2 after T027 passes
- **US4 (Phase 6)**: Depends on US1; can overlap with US2/US3
- **mcp-server wiring (Phase 7)**: Depends on all user story phases (full `DebugBackend` impl)
- **Polish (Phase 8)**: Depends on Phase 7

### User Story Dependencies

- **US1 (P1)**: Can start after Phase 2 — no story dependencies
- **US2 (P2)**: Depends on US1 (needs a working `continue_execution` + paused session)
- **US3 (P3)**: Depends on US1 (needs paused session); independent of US2
- **US4 (P4)**: Depends on US1 (needs live session); independent of US2/US3

### Parallel Opportunities Within Each Phase

| Phase | Parallelizable tasks |
|---|---|
| Phase 1 | T003, T004, T005, T006, T007 (all separate files) |
| Phase 2A | T009, T010 (separate files in lldb-sys) |
| Phase 2B | T014, T015, T016 (after T014) |
| Phase 3 | T019, T020 (mapping functions, separate from thread impl) |
| Phase 4 | T028, T029 (command variant + mapping, separate files) |
| Phase 5 | T034, T040 (command variants + evaluate/threads, separate files) |
| Phase 6 | T044 (verify/add command variants — minimal work) |
| Phase 8 | T057, T058, T060 (doc comments on different files) |

---

## Parallel Execution Examples

```bash
# Phase 1 — scaffold all files in one pass:
T003: crates/lldb-native/src/lib.rs
T004: crates/lldb-native/src/command.rs
T005: crates/lldb-native/src/mapping.rs
T006: crates/lldb-native/src/thread.rs
T007: crates/lldb-native/src/handle.rs

# Phase 3 — mapping and thread impl can overlap:
T019: mapping::state_to_session_state + stop_reason_to_pause_reason
T020: thread::handle_launch_process  (different file sections)
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1 (Setup) — ~1 session
2. Complete Phase 2 (Foundational) — requires lldb-sys C++ work
3. Complete Phase 3 (US1: launch + break) — core loop working
4. **STOP and VALIDATE**: `cargo test --package lldb-native launch_and_break` must pass
5. No Bitdefender issue (no codelldb.exe spawned) — validate manually

### Incremental Delivery

1. Setup + Foundational → crate scaffolded, trait ready
2. US1 → launch, break, continue — minimal working debugger (MVP)
3. US2 → stepping — interactive debugging possible
4. US3 → variable inspection — full inspection loop
5. US4 → dynamic breakpoints — production-quality session management
6. Phase 7 → mcp-server live on new backend
7. Phase 8 → production polish

---

## Notes

- All `unsafe` blocks in `crates/lldb-native` MUST follow the three-condition proof (Constitution Principle VI): safety comment, GitHub issue link, rejected safe alternative
- The `LldbDebugThread` MUST own all LLDB state — nothing LLDB-related crosses thread boundaries
- The integration tests require LLVM installed (`LLDB_DLL_DIR` set) — they will be skipped in CI without it unless the CI environment has LLVM
- `lldb-bridge` is intentionally left in the workspace (deprecated, not deleted) to avoid a forced migration of any remaining callers; deletion is a separate cleanup PR
