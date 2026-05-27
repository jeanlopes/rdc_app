# Research: Native LLDB Backend via lldb-sys

## 1. API Coverage — lldb-safe vs Required Operations

**Decision**: `lldb-safe` covers all operations required by the spec without any new C++ wrapper code.

| Required Operation | lldb-safe API | Notes |
|---|---|---|
| Initialize / teardown | `Debugger::initialize()` / `Debugger::terminate()` | One-time global calls |
| Create debug target | `Debugger::create_target_simple(path)` | Auto-detects triple |
| Launch process | `Target::launch(argv, envp, cwd, stop_at_entry)` | Returns `Process` |
| Attach by PID | `Target::attach_to_pid(pid)` | Returns `Process` |
| Breakpoint by source line | `Target::breakpoint_by_location(file, line)` | Returns `Breakpoint` |
| Breakpoint by name | `Target::breakpoint_by_name(name, module)` | Returns `Breakpoint` |
| Breakpoint by address | `Target::breakpoint_by_address(addr)` | Returns `Breakpoint` |
| Remove breakpoint | `Target::delete_breakpoint(id)` | Returns `bool` |
| List breakpoints | `Target::num_breakpoints()` / `Target::breakpoint_at(idx)` | Iterator pattern |
| Resume | `Process::resume()` | Returns `Result<(), Error>` |
| Stop (pause) | `Process::stop()` | Returns `Result<(), Error>` |
| Kill | `Process::kill()` | Returns `Result<(), Error>` |
| Detach | `Process::detach()` | Returns `Result<(), Error>` |
| Process state | `Process::state()` | Returns `lldb_safe::State` |
| List threads | `Process::num_threads()` / `Process::thread_at(idx)` | Iterator pattern |
| Get selected thread | `Process::selected_thread()` | Returns `Option<Thread>` |
| Thread stop reason | `Thread::stop_reason()` | Returns `lldb_safe::StopReason` |
| Call stack | `Thread::num_frames()` / `Thread::frame_at(idx)` | Iterator pattern |
| Step over | `Thread::step_over()` | Blocking (sync) |
| Step into | `Thread::step_into()` | Blocking (sync) |
| Step out | `Thread::step_out()` | Blocking (sync) |
| Frame PC / function | `Frame::pc()` / `Frame::function_name()` | |
| Local variables | `Frame::find_variable(name)` | Returns `Option<Value>` |
| Expression evaluation | `Frame::evaluate_expression(expr)` | Returns `Option<Value>` |
| Value inspection | `Value::name()`, `Value::type_name()`, `Value::value_string()` | |
| Compound type children | `Value::num_children()` / `Value::child_at(idx)` | |
| Memory read/write | `Process::read_memory()` / `Process::write_memory()` | |

**Gap identified**: `lldb-safe` does not expose `SBListener` / event polling yet. For the async
event model (waiting for the process to stop after a `resume()`), we need one of:
- Option A: Use `lldb-safe` in **sync mode** (`Debugger::create(false)`) — `step_over()` etc.
  block until the next stop. This works for the step-command model but requires a dedicated
  thread so the Tokio executor is not blocked.
- Option B: Add `SBListener::wait_for_event()` to lldb-safe and use async mode.

**Decision**: Option A (sync mode + dedicated thread). The lldb-safe crate already uses sync
blocking calls (`Thread::step_over()` blocks). A dedicated OS thread with a Tokio `oneshot`
channel per command matches the pattern already used in `crates/win-debug-bridge`.

**Rationale**: Option B requires extending lldb-sys with SBListener bindings (new C++ wrapper
code). Option A reuses the existing sync API and matches the proven win-debug-bridge thread model.

---

## 2. Threading Model

**Decision**: Single dedicated OS thread owns all LLDB state. Async callers communicate via
`mpsc::Sender<LldbCommand>` + per-command `oneshot::Sender<Result<...>>`.

`lldb_safe::Debugger` wraps `SBDebuggerRef` which is `!Send + !Sync`. The `Debugger` struct must
not cross thread boundaries. The dedicated thread is the sole owner.

Architecture (mirrors `win-debug-bridge`):
```
async caller  →  LldbNativeHandle (mpsc::Sender<LldbCommand>)
                       ↓
         LLDB OS thread (std::thread — owns SBDebugger, SBTarget, SBProcess)
                       ↓
         lldb-safe API calls (blocking, in-process, no external process)
```

**Rationale**: `win-debug-bridge` proves this pattern works. No new threading primitives needed.

---

## 3. DLL Loading (Windows)

**Decision**: Call `lldb_safe::set_lldb_dll_dir(&dll_dir)` at crate startup, before
`Debugger::initialize()`. The `LLDB_DLL_DIR` env var is set by `lldb-sys`'s `build.rs` at
compile time to point to the directory containing `liblldb.dll`.

```rust
let dll_dir = std::env::var("LLDB_DLL_DIR")
    .expect("LLDB_DLL_DIR not set — run scripts/install-llvm.ps1 first");
lldb_safe::set_lldb_dll_dir(std::path::Path::new(&dll_dir));
```

This must happen in the LLDB thread before `Debugger::initialize()`, not in `main()`, to avoid
confusion about which thread owns the DLL search path.

**Alternatives considered**:
- Embedding `liblldb.dll` in the binary: Not viable — the DLL is ~100MB and has its own
  transitive dependencies (LLVM libraries). Bundling is not practical.
- Shipping a trimmed LLDB build: Future work, out of scope.

---

## 4. Type Mapping — lldb-safe → runtime-core

### State → SessionState

| `lldb_safe::State` | `runtime_core::session::SessionState` |
|---|---|
| `Invalid` | `Error("invalid LLDB state")` |
| `Unloaded` / `Connected` | `Idle` |
| `Attaching` / `Launching` | `Launching` |
| `Stopped` | `Paused(PauseReason::...)` |
| `Running` | `Running` |
| `Stepping` | `Stepping` |
| `Crashed` | `Paused(PauseReason::Exception("crashed"))` |
| `Detached` | `Terminated(0)` |
| `Exited` | `Terminated(exit_code)` |
| `Suspended` | `Paused(PauseReason::UserRequest)` |

### StopReason → PauseReason

| `lldb_safe::StopReason` | `runtime_core::session::PauseReason` |
|---|---|
| `Breakpoint` | `PauseReason::Breakpoint(bp_id)` — id from `Thread::stop_reason_data_at(0)` |
| `Trace` / `PlanComplete` | `PauseReason::Step` |
| `Signal` | `PauseReason::Signal(signal_name)` |
| `Exception` | `PauseReason::Exception(description)` |
| `None` / `Invalid` | `PauseReason::UserRequest` |

### Frame → StackFrame

`Frame::pc()` + `Frame::function_name()` + `Frame::is_inlined()` map directly to
`runtime_core::process::StackFrame { index, function_name, source_location, is_inlined }`.
Source location (file + line) requires LLDB's `SBLineEntry` — this is not currently exposed in
`lldb-safe`. Two options:
- Option A: Derive source location from `Frame::evaluate_expression("__FILE__")` / `("__LINE__")`.
  Fragile — depends on debug info.
- Option B: Add `SBFrame::GetLineEntry()` to lldb-safe (one new C++ wrapper method).

**Decision**: Option B — add `Frame::source_location() -> Option<SourceLocation>` to lldb-safe.
This is the correct API and requires a single new C++ wrapper method. It will be implemented as
part of this feature in the `lldb-sys` workspace (separate PR or sub-task).

**Rationale**: Option A is not reliable. The `SBLineEntry` approach is what LLDB itself uses for
source-level debugging. One new wrapper method is a minimal change.

### Value → Variable

`lldb_safe::Value` maps to `runtime_core::variable::Variable`:
- `Value::name()` → `Variable.name`
- `Value::display_type_name()` → `Variable.type_name`
- `Value::value_string()` → `Variable.value` (via `VariableValue::Opaque { summary }`)
- `Value::num_children()` > 0 → compound type → children fetched lazily
- `Value::as_i64()` / `Value::as_u64()` → `VariableValue::Scalar`

---

## 5. Backend Trait — Eliminating the mcp-server's Hard Dependency on lldb-bridge

**Decision**: Extract a `DebugBackend` trait into `crates/runtime-core`. Both `lldb-native` and
`win-debug-bridge` implement it. `apps/mcp-server` uses `Arc<dyn DebugBackend + Send + Sync>`.

This is the correct constitution-compliant change: it makes the backend swappable at startup
without changing MCP handler code. It is a prerequisite for replacing `lldb-bridge` with
`lldb-native` in the mcp-server without a significant rewrite of the handler layer.

**Rationale**: `mcp-server/Cargo.toml` currently depends on `lldb-bridge` directly. If we
simply swap the dependency to `lldb-native`, the handlers continue to work because the method
signatures are identical. However, extracting a trait in `runtime-core` is architecturally
cleaner and avoids future coupling.

**Alternative considered**: Just swap the `Cargo.toml` dependency (`lldb-bridge` → `lldb-native`)
and keep the concrete type. Simpler but leaves the mcp-server hard-coupled to the backend choice.
The trait approach is better and the cost is low (one trait definition, two `impl` blocks).

---

## 6. Crate Naming

**Decision**: New crate is `crates/lldb-native` (Rust crate name: `lldb-native`).

The old `crates/lldb-bridge` is renamed/superseded but kept in the workspace temporarily as a
deprecated stub until all callers are removed, to avoid a large blast radius.

**Rationale**: `lldb-bridge` is a reasonable name for the DAP-based approach; `lldb-native`
communicates that this is the direct, in-process binding.
