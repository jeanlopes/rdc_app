# Research: Phase 1 — MCP + Windows Debug Bridge

**Feature**: 001-mcp-lldb-bridge
**Date**: 2026-05-20 (revised 2026-05-20 — LLDB approach replaced by Windows Debug API)

---

## 1. Debugger Integration Strategy

### Decision: Windows Debug API via `windows-rs` crate

**Rationale**:
The platform targets Windows exclusively. The Windows Debug API is a native OS capability
available on every Windows 10/11 machine — no installation required beyond a working Rust
toolchain. All integration is expressed as Rust crate dependencies in `Cargo.toml`.

The previous approach (PyO3 + LLDB Python API) was rejected because:
- Required LLVM installed separately on the machine
- Required Python 3.x installed separately
- Required matching Python/LLDB versions
- Was a Linux-native design ported badly to Windows
- Violated Constitution Principle VI (no external runtime dependencies)

**How it works**:

```
CreateProcess(exe, DEBUG_PROCESS | DEBUG_ONLY_THIS_PROCESS)
        ↓
WaitForDebugEvent(DEBUG_EVENT, timeout_ms)   ← event loop on dedicated OS thread
        ↓ match event.dwDebugEventCode
  EXCEPTION_DEBUG_EVENT
    ├── EXCEPTION_BREAKPOINT (0x80000003)    ← INT3 hit
    ├── EXCEPTION_SINGLE_STEP (0x80000004)   ← after TF step
    └── other codes                          ← panics, access violations
  EXIT_PROCESS_DEBUG_EVENT                   ← process terminated
  CREATE_PROCESS_DEBUG_EVENT                 ← process started
        ↓
ContinueDebugEvent(pid, tid, DBG_CONTINUE | DBG_EXCEPTION_NOT_HANDLED)
```

**Setting breakpoints**:
1. `ReadProcessMemory` — save the original byte at target address
2. `WriteProcessMemory` — write `0xCC` (INT3 opcode)
3. On hit: restore original byte, set `EIP/RIP -= 1`, optionally re-arm

**Single-step (step over / step into)**:
1. `GetThreadContext` — read `CONTEXT.EFlags`
2. Set `EFLAGS.TF = 1` (Trap Flag)
3. `SetThreadContext` — apply
4. `ContinueDebugEvent` → next instruction raises `EXCEPTION_SINGLE_STEP`

**Step out** (return to caller):
- Set a temporary breakpoint at the return address found in the stack frame

**Variable reading**:
- `ReadProcessMemory(process, var_address, buffer, size)` — raw bytes
- PDB tells us: address (or register + offset), type size, type name
- Combine both to produce a typed `Variable`

**Crate dependencies**:
```toml
windows = { version = "0.58", features = [
    "Win32_System_Diagnostics_Debug",
    "Win32_System_Threading",
    "Win32_System_Memory",
    "Win32_Foundation",
    "Win32_Storage_FileSystem",
] }
pdb = "0.8"
```

**Alternatives considered**:
- **PyO3 + LLDB Python API**: Requires LLDB + Python installed. Rejected — violates zero-install
  principle. Was the original (wrong) approach in this branch.
- **DbgHelp.dll + SymFromAddr**: Works but requires `dbghelp.dll` loaded and is higher-level.
  Can be used for stack walking alongside the debug API. Not excluded, but `pdb` crate is
  preferred for symbol resolution to stay in pure-Rust territory.
- **WinDbg via subprocess**: Requires WinDbg installed. Rejected — same issue as LLDB.
- **DAP (Debug Adapter Protocol) over subprocess**: Higher-level than needed; requires an
  external DA server. Rejected.

---

## 2. Symbol Resolution

### Decision: `pdb` crate (parse `.pdb` files directly)

**Rationale**:
Rust on Windows compiles with debug information in `.pdb` format (Program Database). The `pdb`
crate parses these files in pure Rust, providing:
- Source file + line number from instruction pointer (address → `PdbAddressMap`)
- Function names from public/global symbols
- Local variable names, types, and locations (register-relative or absolute address)

**Flow**:
```
1. On CreateProcess: find <exe_name>.pdb next to the .exe
2. pdb::PDB::open(pdb_file)
3. On each debug event: look up current RIP in PDB address map → SourceLocation
4. On read_locals: enumerate locals for current function → name + location
5. ReadProcessMemory(location.address) → raw bytes → deserialize to VariableValue
```

**Limitation**: PDB variable type info (`S_LOCAL`, `S_DEFRANGE_REGISTER`) can be complex to
parse fully for deeply nested Rust types. Initial implementation returns scalar types and
opaque summaries for complex structs. Full type reconstruction is a Phase 2+ enhancement.

---

## 3. Async Bridging

### Decision: Same pattern as before — dedicated OS thread + `tokio::sync::mpsc`

**Rationale**:
Windows Debug API calls (`WaitForDebugEvent`, `WriteProcessMemory`, etc.) are synchronous and
MUST be called from the same thread that called `CreateProcess` with `DEBUG_PROCESS`. The
dedicated-thread + mpsc channel pattern is mandatory, not optional.

```
Tokio async handler  →  WindowsDebugHandle (mpsc::Sender<DebugCommand>)
                                  ↓
                     Windows debug OS thread (std::thread::spawn)
                     WaitForDebugEvent loop
                                  ↓
                     WindowsDebugBackend (pure-Rust Windows API calls)
```

No changes to the channel architecture from the previous design.

---

## 4. Semantic Probes

### Decision: Unchanged — `probe!` macro + `ProbeRegistry`

The `probe!` macro, `ProbeRegistry`, and `SemanticAnnotation` types are defined in
`crates/runtime-core` and are independent of the debugger backend. No changes needed.

---

## 5. Variable Serialization

### Decision: Unchanged — `Serializer` in `runtime-core`

The recursive serializer with depth limits, cyclic ref detection, and truncation is backend-
agnostic. No changes needed.

---

## 6. MCP Protocol

### Decision: Unchanged — JSON-RPC 2.0 dispatch loop in `apps/mcp-server`

The MCP server, all handlers, and all protocol types are backend-agnostic. No changes needed.

---

## 7. Platform Target

- **Windows 10/11 x86-64**: Only supported target.
  Windows Debug API is available on all Windows versions since XP.
  PDB format is stable and supported by the Rust MSVC and GNU toolchains on Windows.
  No installation required beyond `rustup` and the MSVC or GNU toolchain.

---

## Summary Table

| Decision | Choice | Reason |
|---|---|---|
| Debugger integration | Windows Debug API (`windows-rs`) | Zero install, Windows-native |
| Symbol resolution | `pdb` crate | Pure Rust, no external tools |
| Async bridge | Dedicated thread + mpsc | Windows debug thread affinity requirement |
| Semantic probes | `probe!` macro (unchanged) | Backend-agnostic |
| Variable serialization | `Serializer` (unchanged) | Backend-agnostic |
| Platform | Windows 10/11 only | Project target |
