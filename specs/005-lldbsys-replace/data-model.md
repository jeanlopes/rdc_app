# Data Model: Native LLDB Backend

## Overview

This feature introduces one new workspace crate (`crates/lldb-native`) and one new trait
(`DebugBackend`) in `crates/runtime-core`. All other entity types are reused from
`runtime-core` without modification.

---

## New Trait: `DebugBackend` (in `crates/runtime-core`)

Defines the async contract that any debug backend must satisfy. Extracted from the shared method
signatures of `LldbDebugHandle` (lldb-bridge) and `WindowsDebugHandle` (win-debug-bridge).

```
DebugBackend
├── launch_process(target: DebugTarget) → (u32, SessionState)
├── attach_to_pid(pid: u64)             → (u64, SessionState)
├── get_state()                         → SessionState
├── set_breakpoint(kind, condition)     → Breakpoint
├── remove_breakpoint(id)               → ()
├── list_breakpoints()                  → Vec<Breakpoint>
├── continue_execution()                → ExecutionEvent
├── pause_execution()                   → ExecutionEvent
├── step_over(thread_id?)               → ExecutionEvent
├── step_into(thread_id?)               → ExecutionEvent
├── step_out(thread_id?)                → ExecutionEvent
├── read_locals(thread_id?, frame, ctx, depth) → Vec<Variable>
├── read_stack(thread_id?, max_frames)  → Vec<StackFrame>
├── evaluate_expression(expr, thread_id?, frame) → EvalResult
└── list_threads()                      → Vec<ThreadInfo>
```

**Consumed by**: `apps/mcp-server` (via `Arc<dyn DebugBackend + Send + Sync>`)
**Implemented by**: `crates/lldb-native`, `crates/win-debug-bridge`

---

## New Crate: `crates/lldb-native`

### Public API

```
lldb_native
└── LldbNativeHandle       (Clone, Send, Sync)
    ├── spawn() → Result<Self, DebuggerError>
    └── impl DebugBackend  (all methods are async)
```

### Internal Structure

```
lldb_native (crate)
├── handle.rs      — LldbNativeHandle: public async handle (mpsc sender)
├── thread.rs      — LldbDebugThread: spawns the LLDB OS thread, owns SBDebugger
├── command.rs     — LldbCommand enum: all operations as message variants
├── mapping.rs     — Type conversions: lldb-safe types → runtime-core types
└── lib.rs         — pub use handle::LldbNativeHandle
```

### LldbCommand enum (internal)

Each variant carries the input data and a `oneshot::Sender` for the result:

```
LldbCommand
├── LaunchProcess   { target: DebugTarget, reply: oneshot::Sender<Result<(u32, SessionState)>> }
├── AttachToPid     { pid: u64,            reply: oneshot::Sender<Result<(u64, SessionState)>> }
├── GetState        {                      reply: oneshot::Sender<Result<SessionState>> }
├── SetBreakpoint   { kind, condition,     reply: oneshot::Sender<Result<Breakpoint>> }
├── RemoveBreakpoint{ id: BreakpointId,    reply: oneshot::Sender<Result<()>> }
├── ListBreakpoints {                      reply: oneshot::Sender<Result<Vec<Breakpoint>>> }
├── Continue        {                      reply: oneshot::Sender<Result<ExecutionEvent>> }
├── Pause           {                      reply: oneshot::Sender<Result<ExecutionEvent>> }
├── StepOver        { thread_id?,          reply: oneshot::Sender<Result<ExecutionEvent>> }
├── StepInto        { thread_id?,          reply: oneshot::Sender<Result<ExecutionEvent>> }
├── StepOut         { thread_id?,          reply: oneshot::Sender<Result<ExecutionEvent>> }
├── ReadLocals      { thread_id?, frame, ctx, depth, reply: ... }
├── ReadStack       { thread_id?, max_frames,        reply: ... }
├── EvaluateExpr    { expr, thread_id?, frame,       reply: ... }
└── ListThreads     {                      reply: oneshot::Sender<Result<Vec<ThreadInfo>>> }
```

### LldbDebugThread state (internal, lives on the LLDB OS thread)

```
LldbDebugThread
├── debugger: Debugger              — SBDebugger instance (owns the session)
├── target:   Option<Target>        — current debug target
├── process:  Option<Process>       — live process (set after launch/attach)
└── state:    SessionState          — cached process state
```

---

## Extended Entity: `StackFrame` (in `crates/runtime-core`)

`source_location: Option<SourceLocation>` already exists. The `lldb-safe` extension needed:
`Frame::source_location()` → returns file path + line number from `SBLineEntry`.

No changes to `StackFrame`'s Rust struct — only to how it is populated.

---

## Extended crate: `lldb-safe` (in `c:\workspace\lldb-sys`)

One new method added to `Frame`:

```rust
pub fn source_location(&self) -> Option<(String, u32)>  // (file_path, line)
```

This wraps `SBFrame::GetLineEntry()` → `SBLineEntry::GetFileSpec()` + `GetLine()`.
A new C++ wrapper file `wrapper/src/SBLineEntry.cpp` is added to `lldb-sys`.

---

## Entities Reused Unchanged from `runtime-core`

| Entity | Source | Used by lldb-native |
|---|---|---|
| `DebugTarget` | `session.rs` | `LaunchProcess` command input |
| `SessionState` / `PauseReason` | `session.rs` | All state transitions |
| `Breakpoint` / `BreakpointKind` / `BreakpointId` | `breakpoint.rs` | Set/remove/list |
| `BreakpointLocation` | `breakpoint.rs` | Populated from `lldb_safe::Breakpoint` |
| `StackFrame` | `process.rs` | `ReadStack` output |
| `SourceLocation` | `process.rs` | Frame source file + line |
| `ThreadInfo` / `ThreadId` / `ThreadState` | `process.rs` | `ListThreads` output |
| `Variable` / `VariableValue` / `ScalarValue` | `variable.rs` | `ReadLocals` output |
| `EvalResult` | `variable.rs` | `EvaluateExpr` output |
| `DebuggerError` | `error.rs` | All error variants |

---

## Dependency Graph

```
apps/mcp-server
  └── runtime-core (trait DebugBackend)
  └── lldb-native
        └── runtime-core
        └── lldb-safe (c:\workspace\lldb-sys\crates\lldb-safe)
              └── lldb-sys (c:\workspace\lldb-sys\crates\lldb-sys → liblldb.dll)

apps/mcp-server
  └── [win-debug-bridge kept as alternative, not removed]
```

`apps/mcp-server` selects the backend at startup (via a CLI flag or env var). Both backends
implement `DebugBackend`. The handler code is unchanged.
