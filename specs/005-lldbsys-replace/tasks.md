# Tasks: Native LLDB Backend via lldb-sys

**Input**: Design documents from `specs/005-lldbsys-replace/`

**Feature**: Replace `crates/lldb-bridge` (DAP + codelldb external process) with `crates/lldb-native` (lldb-safe in-process FFI). Extract `DebugBackend` trait into `runtime-core`. Wire into `apps/mcp-server`.

**Organization**: Tasks are grouped by user story. Each phase is independently buildable and testable.

---

## Phase 1: Setup (Crate Scaffolding)

**Purpose**: Create the `crates/lldb-native` workspace crate with the correct Cargo manifest and module skeleton so that subsequent phases compile.

- [X] T001 Add `"crates/lldb-native"` to the `[workspace] members` array in the root `Cargo.toml`
- [X] T002 Create `crates/lldb-native/Cargo.toml` with: `name = "lldb-native"`, edition/rust-version/license from workspace, deps: `lldb-safe = { path = "../../.." }` (adjust relative path to `c:\workspace\lldb-sys\crates\lldb-safe`), `runtime-core`, `protocol`, `tokio` (workspace, features: ["sync", "rt"]), `tracing` (workspace), `thiserror` (workspace), `async-trait = "0.1"`, and `[target.'cfg(windows)'.dependencies] windows-sys = { version = "0.59", features = ["Win32_System_LibraryLoader"] }`
- [X] T003 [P] Create `crates/lldb-native/src/lib.rs` with module declarations: `pub mod handle; mod thread; mod command; mod mapping;` and `pub use handle::LldbNativeHandle;`
- [X] T004 [P] Create `crates/lldb-native/src/command.rs` with an empty `LldbCommand` enum placeholder: `pub(crate) enum LldbCommand {}`
- [X] T005 [P] Create `crates/lldb-native/src/mapping.rs` as an empty module with a `// type conversions: lldb-safe → runtime-core` comment
- [X] T006 [P] Create `crates/lldb-native/src/thread.rs` with an empty `pub(crate) struct LldbDebugThread;`
- [X] T007 [P] Create `crates/lldb-native/src/handle.rs` with a stub `pub struct LldbNativeHandle;` and `impl LldbNativeHandle { pub fn spawn() -> Result<Self, runtime_core::error::DebuggerError> { todo!() } }`
- [X] T008 Verify `cargo check --package lldb-native` compiles with no errors (stubs are acceptable)

**Checkpoint**: `cargo check --package lldb-native` passes.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Two independent foundations that ALL user stories depend on: (A) `Frame::source_location()` in lldb-safe for source-level stack frames, and (B) `DebugBackend` trait in `runtime-core` for the mcp-server decoupling.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

### A — lldb-safe Extension (SBLineEntry)

- [X] T009 Add C++ wrapper for `SBLineEntry` in `c:\workspace\lldb-sys\crates\lldb-sys\wrapper\include\lldb_c.h`: declare `LLDB_SBFrame_GetLineEntryFile(SBFrameRef frame, char* buf, size_t buf_len)` and `LLDB_SBFrame_GetLineEntryLine(SBFrameRef frame)` returning `uint32_t`; also declare `LLDB_SBFrame_GetVariables`
- [X] T010 Create `c:\workspace\lldb-sys\crates\lldb-sys\wrapper\src\SBLineEntry.cpp` implementing the two functions: call `frame->GetLineEntry().GetFileSpec().GetPath(buf, buf_len)` and `frame->GetLineEntry().GetLine()` on the dereferenced `SBFrameRef`; also implements `LLDB_SBFrame_GetVariables`
- [X] T011 Add the two new function declarations to `c:\workspace\lldb-sys\crates\lldb-sys\wrapper\include\lldb_c.h` (bindgen generates Rust bindings from header — no manual extern "C" block in lib.rs)
- [X] T012 Add `pub fn source_location(&self) -> Option<(String, u32)>` and `pub fn variables(arguments: bool, locals: bool, statics: bool) -> Vec<Value>` to `c:\workspace\lldb-sys\crates\lldb-safe\src\frame.rs`
- [ ] T013 Verify `cargo build --package lldb-safe` (in `c:\workspace\lldb-sys`) compiles without errors

### B — `DebugBackend` Trait in runtime-core

- [X] T014 Add `async-trait = "0.1"` to `[dependencies]` in `crates/runtime-core/Cargo.toml`
- [X] T015 Create `crates/runtime-core/src/backend.rs` containing the `DebugBackend` trait exactly as specified in `specs/005-lldbsys-replace/contracts/debug-backend-trait.md` (all 14 async methods, `async_trait` attribute, `Send + Sync` supertrait)
- [X] T016 Add `pub mod backend; pub use backend::DebugBackend;` to `crates/runtime-core/src/lib.rs`; also add `pub mod event; pub use event::{ExecutionEvent, ExecutionEventKind};` and move `ExecutionEvent` from `protocol` to `runtime-core::event`
- [ ] T017 Verify `cargo test --package runtime-core` passes

**Checkpoint**: `cargo build --package lldb-safe` and `cargo test --package runtime-core` both pass.

---

## Phase 3: User Story 1 — Launch and Break at Breakpoint (Priority: P1) 🎯 MVP

**Goal**: Start a debug session in-process via lldb-safe, set a source-level breakpoint, launch the binary, and pause at the breakpoint — with no external process spawned.

**Independent Test**: Run `cargo test --package lldb-native launch_and_break` — launches `debug-target-example`, sets a breakpoint at `main.rs:20`, calls `continue_execution()`, asserts the returned `ExecutionEvent` is `BreakpointHit`.

- [X] T018 [US1] Expand `LldbCommand` enum in `crates/lldb-native/src/command.rs` with all variants: `LaunchProcess`, `AttachToPid`, `GetState`, `SetBreakpoint`, `RemoveBreakpoint`, `ListBreakpoints`, `Continue`, `Pause`, `StepOver`, `StepInto`, `StepOut`, `ReadLocals`, `ReadStack`, `EvaluateExpr`, `ListThreads` — each with `oneshot::Sender` reply
- [X] T019 [US1] Implement `mapping::state_to_session_state(state: lldb_safe::State, process: &lldb_safe::Process) -> SessionState` in `crates/lldb-native/src/mapping.rs`
- [X] T020 [US1] Implement `mapping::stop_reason_to_pause_reason`, `mapping::build_execution_event`, `mapping::frame_source_location`, `mapping::frame_to_stack_frame`, `mapping::value_to_variable`, `mapping::value_to_eval_result`, `mapping::thread_info`, `mapping::lldb_bp_to_core` in `crates/lldb-native/src/mapping.rs`
- [X] T021 [US1] Implement `LldbDebugThread` struct in `crates/lldb-native/src/thread.rs`: fields `debugger`, `target`, `process`; `run(rx)` entry point calling `Debugger::initialize()` and message loop
- [X] T022 [US1] Implement `LldbDebugThread::handle_launch_process()` — creates LLDB target, launches process stopped at entry, spin-polls state
- [X] T023 [US1] Implement `LldbDebugThread::handle_set_breakpoint()` — matches on `BreakpointKind`, sets via lldb-safe, returns `CoreBreakpoint`
- [X] T024 [US1] Implement `LldbDebugThread::handle_continue()` — resumes process, spin-polls until stopped, maps to `ExecutionEvent`
- [X] T025 [US1] Implement `LldbNativeHandle` in `crates/lldb-native/src/handle.rs`: `spawn()` creates `mpsc::sync_channel::<LldbCommand>(32)`, spawns `std::thread` for `LldbDebugThread::run(rx)`; all backend methods use oneshot pattern
- [X] T026 [US1] Add `#[async_trait::async_trait] impl DebugBackend for LldbNativeHandle` in `crates/lldb-native/src/handle.rs` — all 14 methods fully implemented
- [ ] T027 [US1] Create `crates/lldb-native/tests/integration.rs` with test `launch_and_break`

**Checkpoint**: `cargo test --package lldb-native launch_and_break` passes. Session launches, stops at breakpoint, no external codelldb process running.

---

## Phase 4: User Story 2 — Step Through Code (Priority: P2)

**Goal**: From a paused session, call step-over / step-into / step-out and observe the source position advancing correctly.

**Independent Test**: After `launch_and_break` succeeds, call `step_over()` three times and assert each `ExecutionEvent` is `StepComplete` with a different source line number.

- [X] T028 [P] [US2] `StepOver`, `StepInto`, `StepOut` variants added to `LldbCommand` (done as part of T018)
- [X] T029 [US2] Implement `mapping::frame_to_stack_frame` in `crates/lldb-native/src/mapping.rs` (done as part of T020)
- [X] T030 [US2] Implement `LldbDebugThread::handle_step()` in `crates/lldb-native/src/thread.rs` — dispatches Over/Into/Out on selected thread, spin-polls, returns `StepComplete` event
- [X] T031 [US2] `LldbNativeHandle::step_over()`, `step_into()`, `step_out()` implemented via `send()` helper
- [X] T032 [US2] `impl DebugBackend for LldbNativeHandle` — `step_over`, `step_into`, `step_out` fully delegated
- [ ] T033 [US2] Extend `crates/lldb-native/tests/integration.rs` with test `step_three_times`

**Checkpoint**: `cargo test --package lldb-native step_three_times` passes.

---

## Phase 5: User Story 3 — Inspect Variables (Priority: P3)

**Goal**: After pausing, retrieve local variable names, types, and values; expand compound types; evaluate expressions.

**Independent Test**: After pausing at a breakpoint inside a function with a `Vec<i32>` local, call `read_locals()` and assert the variable is listed with a non-empty type name and display value.

- [X] T034 [P] [US3] `ReadLocals`, `ReadStack`, `EvaluateExpr`, `ListThreads` variants in `LldbCommand` (done as part of T018)
- [X] T035 [US3] Implement `mapping::value_to_variable` in `crates/lldb-native/src/mapping.rs` (done as part of T020)
- [X] T036 [US3] Implement `LldbDebugThread::handle_read_locals()` using `frame.variables(true, true, false)` → map each `Value` via `mapping::value_to_variable`
- [X] T037 [US3] Add C++ wrapper `LLDB_SBFrame_GetVariables` to `lldb-sys` and `Frame::variables()` to `lldb-safe/src/frame.rs`
- [X] T038 [US3] `LldbDebugThread::handle_read_locals()` implemented in `crates/lldb-native/src/thread.rs`
- [X] T039 [US3] `LldbDebugThread::handle_read_stack()` implemented in `crates/lldb-native/src/thread.rs`
- [X] T040 [P] [US3] `LldbDebugThread::handle_evaluate_expression()` and `handle_list_threads()` implemented
- [X] T041 [US3] `LldbNativeHandle::read_locals()`, `read_stack()`, `evaluate_expression()`, `list_threads()` implemented via mpsc + oneshot
- [X] T042 [US3] `impl DebugBackend for LldbNativeHandle` — all inspection methods fully delegated
- [ ] T043 [US3] Extend `crates/lldb-native/tests/integration.rs` with test `read_locals_at_breakpoint`

**Checkpoint**: `cargo test --package lldb-native read_locals_at_breakpoint` passes.

---

## Phase 6: User Story 4 — Dynamic Breakpoints (Priority: P4)

**Goal**: Add and remove breakpoints during a live session; verify new breakpoints halt execution and removed ones do not.

**Independent Test**: Pause at BP1, add BP2 on a line not yet reached, continue (process pauses at BP2), remove BP2, continue again — process does not pause at BP2's line.

- [X] T044 [P] [US4] `RemoveBreakpoint`, `ListBreakpoints` variants in `LldbCommand` (done as part of T018)
- [X] T045 [US4] `LldbDebugThread::handle_remove_breakpoint()` implemented in `crates/lldb-native/src/thread.rs`
- [X] T046 [US4] `LldbDebugThread::handle_list_breakpoints()` implemented in `crates/lldb-native/src/thread.rs`
- [X] T047 [US4] `LldbNativeHandle::remove_breakpoint()` and `list_breakpoints()` implemented via mpsc + oneshot
- [X] T048 [US4] `impl DebugBackend for LldbNativeHandle` — `remove_breakpoint`, `list_breakpoints` fully delegated
- [ ] T049 [US4] Extend `crates/lldb-native/tests/integration.rs` with test `dynamic_breakpoints`

**Checkpoint**: `cargo test --package lldb-native dynamic_breakpoints` passes.

---

## Phase 7: mcp-server Wiring

**Purpose**: Replace the hard `lldb-bridge` dependency in `apps/mcp-server` with `Arc<dyn DebugBackend>`, add a `--backend` CLI flag, and add `WindowsDebugHandle: DebugBackend` for the win-debug-bridge alternative.

- [X] T050 Add `#[async_trait::async_trait] impl DebugBackend for WindowsDebugHandle` in `crates/win-debug-bridge/src/thread.rs`; add `async-trait = "0.1"` to `crates/win-debug-bridge/Cargo.toml`
- [X] T051 Add `lldb-native = { path = "../../crates/lldb-native" }` to `apps/mcp-server/Cargo.toml`; remove `lldb-bridge.workspace = true`; add `win-debug-bridge.workspace = true`
- [X] T052 Refactor `apps/mcp-server/src/handlers/session.rs`: `SessionContext.backend: Arc<dyn DebugBackend>`; update all handler calls
- [X] T053 Update `apps/mcp-server/src/handlers/breakpoints.rs`, `execution.rs`, `inspection.rs`: all functions take `&dyn DebugBackend`
- [X] T054 Add `#[derive(clap::ValueEnum)] enum BackendChoice { LldbNative, WinDebugBridge }` and `--backend` arg to `apps/mcp-server/src/main.rs`
- [X] T055 Construct `Arc<dyn DebugBackend>` based on `--backend` choice in `apps/mcp-server/src/main.rs`
- [X] T056 Verify `cargo check --package mcp-server` compiles with no errors

**Checkpoint**: `cargo build --package mcp-server` passes. Running with `--backend lldb-native` launches without error.

---

## Phase 7B: visual-debugger Migration

**Purpose**: Migrate `apps/visual-debugger` from `lldb-bridge::LldbDebugHandle` to `Arc<dyn DebugBackend>`.

- [X] Update `apps/visual-debugger/Cargo.toml`: replace `lldb-bridge` with `lldb-native` and `win-debug-bridge`
- [X] Update `apps/visual-debugger/src/app.rs`: `debug_handle` type changed to `Arc<Mutex<Option<Arc<dyn DebugBackend>>>>`, import from `runtime_core::backend::DebugBackend`, `LldbDebugHandle::spawn()` → `lldb_native::LldbNativeHandle::spawn()`
- [X] Update `apps/visual-debugger/src/main.rs`: same spawn change, `mcp_server::run_with_view` call updated

**Checkpoint**: `cargo check --package visual-debugger` passes (verified).

---

## Phase 8: Polish & Cross-Cutting Concerns

- [ ] T057 [P] Add `///` doc comments to all `pub` items in `crates/lldb-native/src/handle.rs` and `crates/lldb-native/src/lib.rs`; add a usage example in the crate-level doc comment showing `spawn()` → `set_breakpoint()` → `launch_process()` → `continue_execution()`
- [ ] T058 [P] Add `///` doc comments and usage example to `DebugBackend` trait in `crates/runtime-core/src/backend.rs`
- [X] T059 Mark `crates/lldb-bridge/Cargo.toml` description as `"DEPRECATED: use lldb-native crate instead"` and add `#[deprecated]` to `LldbDebugHandle::spawn()` in `crates/lldb-bridge/src/handle.rs`
- [ ] T060 [P] Add `tracing::instrument` attributes and `info!` / `error!` events to all public methods of `LldbDebugThread` in `crates/lldb-native/src/thread.rs`
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
