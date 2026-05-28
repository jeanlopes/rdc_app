# Contract: `DebugBackend` Trait

**Location**: `crates/runtime-core/src/backend.rs`
**Visibility**: `pub trait DebugBackend`
**Required bounds**: `Send + Sync`

---

## Purpose

A backend-agnostic async contract that `apps/mcp-server` uses to drive a debug session.
Decouples the MCP handler layer from any specific debug technology (LLDB, Win32 Debug API, etc.).

---

## Trait Definition (authoritative)

```rust
use async_trait::async_trait;

#[async_trait]
pub trait DebugBackend: Send + Sync {
    // ── Session ──────────────────────────────────────────────────────────

    /// Launch a binary as a new debugged process.
    /// Returns (pid, initial_state). The process is stopped at the entry point
    /// so that breakpoints can be set before execution begins.
    async fn launch_process(
        &self,
        target: DebugTarget,
    ) -> Result<(u32, SessionState), DebuggerError>;

    /// Attach the debugger to an already-running process by PID.
    /// Returns (pid, initial_state).
    async fn attach_to_pid(
        &self,
        pid: u64,
    ) -> Result<(u64, SessionState), DebuggerError>;

    /// Return the current session / process state.
    async fn get_state(&self) -> Result<SessionState, DebuggerError>;

    // ── Breakpoints ──────────────────────────────────────────────────────

    /// Set a breakpoint. Returns the created Breakpoint with its assigned ID.
    async fn set_breakpoint(
        &self,
        kind: BreakpointKind,
        condition: Option<String>,
    ) -> Result<Breakpoint, DebuggerError>;

    /// Remove a breakpoint by ID.
    async fn remove_breakpoint(&self, id: BreakpointId) -> Result<(), DebuggerError>;

    /// Return all active breakpoints.
    async fn list_breakpoints(&self) -> Result<Vec<Breakpoint>, DebuggerError>;

    // ── Execution control ────────────────────────────────────────────────

    /// Resume execution. Blocks until the next stop event.
    async fn continue_execution(&self) -> Result<ExecutionEvent, DebuggerError>;

    /// Interrupt a running process and pause it.
    async fn pause_execution(&self) -> Result<ExecutionEvent, DebuggerError>;

    /// Step over the current source line. Blocks until the process stops again.
    async fn step_over(
        &self,
        thread_id: Option<ThreadId>,
    ) -> Result<ExecutionEvent, DebuggerError>;

    /// Step into the current source line. Blocks until the process stops again.
    async fn step_into(
        &self,
        thread_id: Option<ThreadId>,
    ) -> Result<ExecutionEvent, DebuggerError>;

    /// Step out of the current function. Blocks until the process stops again.
    async fn step_out(
        &self,
        thread_id: Option<ThreadId>,
    ) -> Result<ExecutionEvent, DebuggerError>;

    // ── Inspection ───────────────────────────────────────────────────────

    /// Return in-scope local variables for the given frame.
    async fn read_locals(
        &self,
        thread_id: Option<ThreadId>,
        frame_index: u32,
        probe_context: Option<String>,
        max_depth: u32,
    ) -> Result<Vec<Variable>, DebuggerError>;

    /// Return the call stack for the given thread.
    async fn read_stack(
        &self,
        thread_id: Option<ThreadId>,
        max_frames: u32,
    ) -> Result<Vec<StackFrame>, DebuggerError>;

    /// Evaluate an expression in the context of the given frame.
    async fn evaluate_expression(
        &self,
        expression: String,
        thread_id: Option<ThreadId>,
        frame_index: u32,
    ) -> Result<EvalResult, DebuggerError>;

    /// List all threads in the current process.
    async fn list_threads(&self) -> Result<Vec<ThreadInfo>, DebuggerError>;
}
```

---

## Invariants

1. All methods MUST return `Err(DebuggerError::ProcessNotFound)` when called before a successful
   `launch_process` or `attach_to_pid`.
2. `continue_execution`, `step_over`, `step_into`, `step_out` MUST NOT return until the process
   has reached a new stopped state or terminated.
3. `set_breakpoint` MUST assign a unique `BreakpointId` that is stable for the lifetime of the
   breakpoint.
4. `list_breakpoints` MUST return all breakpoints previously set via `set_breakpoint` that have
   not been removed.
5. Implementations MUST be `Send + Sync` — they are shared across Tokio tasks via `Arc`.

---

## Usage in `apps/mcp-server`

```rust
// In SessionContext
pub struct SessionContext {
    pub backend: Arc<dyn DebugBackend>,
    // ...
}

// Backend selection at startup (CLI flag --backend lldb-native | win-debug-bridge)
let backend: Arc<dyn DebugBackend> = match args.backend {
    Backend::LldbNative    => Arc::new(LldbNativeHandle::spawn()?),
    Backend::WinDebugBridge => Arc::new(WindowsDebugHandle::spawn()?),
};
```

---

## Implementations

| Crate | Handle type | Technology |
|---|---|---|
| `crates/lldb-native` | `LldbNativeHandle` | LLDB 19 via lldb-safe FFI (in-process) |
| `crates/win-debug-bridge` | `WindowsDebugHandle` | Win32 Debug API (CreateProcess + WaitForDebugEvent) |
