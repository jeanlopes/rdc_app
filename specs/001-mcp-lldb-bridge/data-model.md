# Data Model: Phase 1 — MCP + LLDB Bridge

**Feature**: 001-mcp-lldb-bridge
**Date**: 2026-05-20

---

## Core Entities

### DebugSession

The top-level lifecycle container for a single debugging engagement.

```rust
pub struct DebugSession {
    pub id: SessionId,           // UUID v4
    pub target: DebugTarget,
    pub state: SessionState,
    pub created_at: u64,         // Unix timestamp (ms)
    pub process: Option<ProcessHandle>,
}

pub struct DebugTarget {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub working_dir: Option<PathBuf>,
}

pub type SessionId = uuid::Uuid;
```

**State machine** — only these transitions are valid:

| From | Event | To |
|------|-------|----|
| `Idle` | `launch_process` | `Launching` |
| `Launching` | process started | `Running` |
| `Launching` | launch failed | `Error(reason)` |
| `Running` | breakpoint hit | `Paused(PauseReason::Breakpoint)` |
| `Running` | `pause_execution` | `Paused(PauseReason::UserRequest)` |
| `Running` | panic detected | `Paused(PauseReason::Panic)` |
| `Paused(_)` | `continue_execution` | `Running` |
| `Paused(_)` | `step_over` / `step_into` / `step_out` | `Stepping` |
| `Stepping` | step complete | `Paused(PauseReason::Step)` |
| `Running` / `Paused(_)` | process exits | `Terminated(ExitCode)` |
| Any | unrecoverable LLDB error | `Error(reason)` |

```rust
pub enum SessionState {
    Idle,
    Launching,
    Running,
    Paused(PauseReason),
    Stepping,
    Terminated(i32),        // exit code
    Error(String),
}

pub enum PauseReason {
    Breakpoint(BreakpointId),
    UserRequest,
    Step,
    Panic,
    Signal(String),
    Exception(String),
}
```

---

### ProcessHandle

Wraps the LLDB process object; owned by the LLDB thread.

```rust
pub struct ProcessHandle {
    pub pid: u32,
    pub threads: Vec<ThreadInfo>,
    pub selected_thread: ThreadId,
}

pub type ThreadId = u64;     // LLDB thread index
```

---

### ThreadInfo

Snapshot of a thread's execution state at pause time.

```rust
pub struct ThreadInfo {
    pub id: ThreadId,
    pub name: Option<String>,
    pub state: ThreadState,
    pub stop_reason: Option<StopReason>,
    pub frames: Vec<StackFrame>,   // populated on demand
}

pub enum ThreadState {
    Running,
    Stopped,
    Suspended,
}

pub enum StopReason {
    Breakpoint { id: BreakpointId, location: u32 },
    Step,
    Signal { name: String, number: u32 },
    Exception { description: String },
    PlanComplete,
    None,
}
```

---

### StackFrame

A single frame in a thread's call stack.

```rust
pub struct StackFrame {
    pub index: u32,
    pub function_name: Option<String>,
    pub module: Option<String>,
    pub source_location: Option<SourceLocation>,
    pub is_inlined: bool,
}

pub struct SourceLocation {
    pub file: PathBuf,
    pub line: u32,
    pub column: Option<u32>,
}
```

---

### Breakpoint

Represents a breakpoint in the target program with its full lifecycle.

```rust
pub struct Breakpoint {
    pub id: BreakpointId,
    pub kind: BreakpointKind,
    pub condition: Option<String>,    // LLDB expression
    pub hit_count: u32,
    pub enabled: bool,
    pub locations: Vec<BreakpointLocation>,
}

pub type BreakpointId = u32;

pub enum BreakpointKind {
    SourceLine { file: PathBuf, line: u32 },
    FunctionName { name: String },
    Address { addr: u64 },
    Regex { pattern: String },
}

pub struct BreakpointLocation {
    pub address: u64,
    pub source_location: Option<SourceLocation>,
    pub resolved: bool,
}
```

**Breakpoint lifecycle**:
- `Unresolved` → `Resolved` when the symbol is loaded
- `Resolved` → `Hit` on each hit (hit_count increments)
- `Enabled` ↔ `Disabled` via `enable_breakpoint` / `disable_breakpoint`
- `Active` → `Removed` via `remove_breakpoint`

---

### Variable

A runtime variable value, optionally annotated with semantic context.

```rust
pub struct Variable {
    pub name: String,
    pub type_name: String,
    pub value: VariableValue,
    pub address: Option<u64>,
    pub semantic: Option<SemanticAnnotation>,
}

pub enum VariableValue {
    Scalar(ScalarValue),
    Struct { fields: Vec<Variable> },
    Array { elements: Vec<VariableValue>, truncated: bool, total: usize },
    Pointer { address: u64, dereferenced: Option<Box<VariableValue>> },
    String { value: String, truncated: bool },
    CyclicRef { address: u64 },
    Opaque { summary: String },      // LLDB summary provider output
    Error { message: String },
}

pub enum ScalarValue {
    Bool(bool),
    Int(i128),
    UInt(u128),
    Float(f64),
    Char(char),
    Unit,
}

pub struct SemanticAnnotation {
    pub context: String,             // e.g., "measure_layout"
    pub qualified_name: String,      // e.g., "measure_layout.remaining_width"
    pub description: Option<String>, // human-readable meaning
}
```

---

### SemanticProbe

A named group of variables captured together with shared semantic context.

```rust
pub struct SemanticProbe {
    pub context: String,               // e.g., "measure_layout"
    pub variables: Vec<Variable>,      // each has SemanticAnnotation set
    pub source_location: SourceLocation,
    pub timestamp_ms: u64,
}
```

The `probe!` macro expands to a `read_locals` call filtered to the listed variable names, with
`SemanticAnnotation.context` set to the macro's first argument and `qualified_name` set to
`"{context}.{variable_name}"`.

---

### MCP Tool Schemas

See `contracts/mcp-tools.md` for full input/output schemas.

Summary of MCP tools and their return types:

| Tool | Input | Output |
|------|-------|--------|
| `set_breakpoint` | `SetBreakpointInput` | `BreakpointId` |
| `remove_breakpoint` | `BreakpointId` | `()` |
| `continue_execution` | `()` | `ExecutionEvent` |
| `pause_execution` | `()` | `()` |
| `step_over` | `StepInput` | `ExecutionEvent` |
| `step_into` | `StepInput` | `ExecutionEvent` |
| `step_out` | `()` | `ExecutionEvent` |
| `read_locals` | `ReadLocalsInput` | `Vec<Variable>` |
| `read_stack` | `ReadStackInput` | `Vec<StackFrame>` |
| `evaluate_expression` | `EvalInput` | `EvalResult` |
| `list_threads` | `()` | `Vec<ThreadInfo>` |

```rust
pub struct ExecutionEvent {
    pub kind: ExecutionEventKind,
    pub thread_id: ThreadId,
    pub stop_reason: Option<StopReason>,
    pub location: Option<SourceLocation>,
}

pub enum ExecutionEventKind {
    BreakpointHit,
    StepComplete,
    Paused,
    Terminated { exit_code: i32 },
    PanicDetected { message: String },
}

pub struct EvalResult {
    pub expression: String,
    pub value: VariableValue,
    pub type_name: String,
    pub error: Option<String>,
}
```

---

## Error Types

```rust
pub enum DebuggerError {
    InvalidState { current: SessionState, required: &'static str },
    LLDBError(String),
    ProcessNotFound,
    BreakpointNotFound(BreakpointId),
    ThreadNotFound(ThreadId),
    EvalError { expression: String, message: String },
    SerializationError(String),
    ProtocolError(String),
}
```

---

## Crate Ownership

| Entity | Defined in |
|--------|-----------|
| `DebugSession`, `SessionState`, `ProcessHandle` | `crates/runtime-core` |
| `DebugTarget`, `BreakpointKind`, `Variable`, `SemanticProbe` | `crates/runtime-core` |
| `StackFrame`, `SourceLocation`, `ThreadInfo` | `crates/runtime-core` |
| `DebuggerError` | `crates/runtime-core` |
| MCP tool input/output types | `crates/protocol` |
| `ExecutionEvent`, `EvalResult` | `crates/protocol` |
| LLDB Python bridge implementation | `crates/lldb-bridge` |
