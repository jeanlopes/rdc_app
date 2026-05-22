# Data Model: Debug Engine Completion

**Feature**: 002-pdb-stepping-panic
**Date**: 2026-05-20

All types are in `crates/win-debug-bridge/src/pdb_info.rs` and
`crates/win-debug-bridge/src/windows_backend.rs` unless noted otherwise.

---

## PdbInfo

Pre-parsed PDB data. Loaded once at `launch_process`, stored in owned structures
with no lifetime dependency on the PDB file handle.

```rust
pub struct PdbInfo {
    image_base: u64,
    // RVA → (absolute path, line number) — BTreeMap for range lookups
    rva_to_source: BTreeMap<u32, (PathBuf, u32)>,
    // (lowercase file stem, line) → RVA — for source-line breakpoints
    line_to_rva: HashMap<(String, u32), u32>,
    // sorted (start_rva, name) — for function-name lookups
    function_starts: Vec<(u32, String)>,
    // function start_rva → local variable list
    locals: HashMap<u32, Vec<PdbLocal>>,
    // short/full name → start RVA — for function-name breakpoints
    name_to_rva: HashMap<String, u32>,
}
```

**Key queries**:
| Method | Input | Output |
|--------|-------|--------|
| `va_to_source(va)` | virtual address | `Option<SourceLocation>` |
| `source_to_va(file, line)` | path + line | `Option<u64>` (VA) |
| `va_to_function_name(va)` | virtual address | `Option<String>` |
| `function_name_to_va(name)` | name | `Option<u64>` |
| `locals_at_va(va)` | virtual address | `Vec<PdbLocal>` |

---

## PdbLocal

Metadata about a local variable extracted from PDB `S_LOCAL` + `S_RegisterRelative`
symbol records.

```rust
pub struct PdbLocal {
    pub name: String,
    pub type_name: String,     // e.g. "i32", "bool", "type_0x1234"
    pub location: VarLocation,
    pub size: usize,           // 0 = unknown; default to 8 (pointer size)
}

pub enum VarLocation {
    /// Offset from RBP. Negative = below RBP (typical for locals).
    FramePointerRelative(i32),
    /// Register number — variable lives entirely in a register.
    Register(u16),
}
```

---

## WindowsDebugBackend (extended fields)

New fields added to support PDB + stepping + panic capture:

```rust
pub struct WindowsDebugBackend {
    // existing fields...
    stopped_tid: Mutex<u32>,          // TID of the currently stopped thread
    pdb: Mutex<Option<PdbInfo>>,      // loaded after launch_process
    exe_path: Mutex<Option<PathBuf>>, // needed to locate .pdb file
    stderr_read: Mutex<Option<HANDLE>>, // read end of panic capture pipe
}
```

---

## StackWalk64 Frame Resolution

`StackWalk64` produces a sequence of `STACKFRAME64` values. Each is resolved to
a `runtime_core::process::StackFrame` using PDB lookups:

```
STACKFRAME64.AddrPC.Offset (u64 virtual address)
    ↓
PdbInfo::va_to_function_name(rip) → Option<String>
PdbInfo::va_to_source(rip)        → Option<SourceLocation>
    ↓
StackFrame { index, function_name, source_location, is_inlined: false }
```

---

## ExecutionEvent (extended)

`PanicDetected` was added to `ExecutionEventKind` in `crates/protocol/src/tools/execution.rs`:

```rust
pub enum ExecutionEventKind {
    BreakpointHit,
    StepComplete,
    Paused,
    Terminated { exit_code: i32 },
    PanicDetected { message: String },  // ← added in 001; carries full stderr text
}
```

The `message` field contains the Rust panic message extracted from the process's stderr
pipe, e.g. `"index out of bounds: the len is 3 but the index is 99"`.

---

## Variable Value Types (Phase 1 support)

`bytes_to_value()` in `windows_backend.rs` maps PDB type names to `VariableValue`:

| Rust type | Bytes | VariableValue variant |
|-----------|-------|-----------------------|
| `bool` | 1 | `Scalar(Bool)` |
| `i8`–`i128` | 1–16 | `Scalar(Int)` |
| `u8`–`u128` | 1–16 | `Scalar(UInt)` |
| `f32` | 4 | `Scalar(Float)` |
| `f64` | 8 | `Scalar(Float)` |
| `isize`/`usize` | 8 | `Scalar(Int/UInt)` |
| `Vec<i32>` (arr eval) | 24 | fat ptr → element read |
| other | any | `Opaque { summary: hex }` |
