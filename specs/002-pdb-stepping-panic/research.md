# Research: Debug Engine Completion — PDB, Stepping & Panic Messages

**Feature**: 002-pdb-stepping-panic
**Date**: 2026-05-20
**Implementation branch**: 001-mcp-lldb-bridge (retroactive documentation)

---

## 1. PDB Symbol Resolution

### Decision: `pdb` crate 0.8, pre-loaded into memory indexes at launch time

**Rationale**:
The `pdb` crate parses Microsoft PDB files in pure Rust — no DbgHelp, no MSVC headers,
no external tools. All data is extracted once at `launch_process` time and stored in
owned `HashMap`/`BTreeMap` structures. All lookups at debug time are O(log n) or O(1)
with no I/O.

**Implementation**:
```
PDB::open(pdb_file)
    ↓ address_map = pdb.address_map()          (RVA conversion)
    ↓ string_table = pdb.string_table()        (file name strings)
    ├── global_symbols.iter() → Public(fn)      → function_starts: Vec<(rva, name)>
    └── debug_information.modules()
            └── module_info.line_program()      → rva_to_source: BTreeMap<rva, (path, line)>
                module_info.symbols()           → locals: HashMap<func_rva, Vec<PdbLocal>>
                    S_LOCAL + S_RegisterRelative → name, type_name, rbp_offset, size
```

**Image base**: captured from `CREATE_PROCESS_DEBUG_EVENT.CreateProcessInfo.lpBaseOfImage`
in `drain_initial_events` immediately after `CreateProcess`.

**VA ↔ RVA conversion**: `va = image_base + rva`, `rva = va - image_base`.

**Source-to-VA lookup**: key is `(lowercase_file_stem, line_number)` → RVA.
Using the file stem (not full path) makes the lookup robust against absolute path
differences between build machine and debug machine.

**VA-to-source lookup**: `BTreeMap::range(..=rva).next_back()` finds the nearest
source entry ≤ the target address. Entries more than 256 bytes away are rejected
to avoid false matches across functions.

**Alternatives considered**:
- **DbgHelp `SymFromAddr`**: requires `dbghelp.dll` loaded via `LoadLibrary` and is
  higher-level than needed. Rejected in favour of pure-Rust `pdb` crate.
- **DWARF parsing**: Rust on Windows with MSVC toolchain uses PDB, not DWARF. Rejected.
- **Runtime PDB lookup on each call**: too slow; pre-indexing is necessary for
  responsiveness. Rejected.

---

## 2. Local Variable Reading

### Decision: PDB `S_RegisterRelative` records + `ReadProcessMemory` at RBP+offset

**Rationale**:
In unoptimised Rust debug builds (`cargo build`), all locals are spilled to the stack.
PDB encodes their locations as `S_RegisterRelative` records (offset from the frame base
pointer, RBP on AMD64). Reading a variable therefore requires:
1. Get RBP from `CONTEXT.Rbp` via `GetThreadContext`
2. Compute `address = rbp + offset` (offset can be negative)
3. `ReadProcessMemory(process, address, buf, size)`
4. Interpret bytes based on PDB type name

**Type interpretation (Phase 1)**: scalar types (bool, i8–i128, u8–u128, f32, f64,
isize, usize) decoded from little-endian bytes. `Vec<T>` decoded as a fat pointer
(ptr, len, cap — 8 bytes each). All other types returned as `Opaque { summary: hex }`.

**Limitation**: PDB type indices (TypeIndex) from `S_LOCAL` records are not yet
decoded to human-readable type names. Phase 1 uses the raw type_index as a placeholder
string (e.g., `"type_0x1234"`). Full type reconstruction via `TypeInformation` is
deferred to a future PR.

---

## 3. Single-Step via EFLAGS.TF

### Decision: Set Trap Flag (bit 8 of EFLAGS) via `GetThreadContext`/`SetThreadContext`

**Rationale**:
The x86-64 Trap Flag causes the CPU to raise `EXCEPTION_SINGLE_STEP (0x80000004)`
after executing exactly one instruction. This is the standard hardware single-step
mechanism used by all debuggers.

**Implementation**:
```
GetThreadContext(thread, &mut ctx)  → ctx.EFlags
ctx.EFlags |= 0x0100               (set TF)
SetThreadContext(thread, &ctx)
ContinueDebugEvent(pid, tid, DBG_CONTINUE)
→ wait for EXCEPTION_SINGLE_STEP in event_loop
```

**Windows-rs feature required**: `Win32_System_Kernel` — `CONTEXT`, `GetThreadContext`,
and `SetThreadContext` are gated behind this feature in windows-rs 0.58 due to
architecture-specific CONTEXT layout.

**Calling convention**: `GetThreadContext`/`SetThreadContext` are `unsafe extern "system"`.
Thread must be in a suspended/debug-stopped state (satisfied by the debug event model).

**step_over vs step_into**: both use EFLAGS.TF. The CPU naturally steps one instruction;
stepping "over" a CALL instruction at the source level requires additional logic
(setting a BP at the return address, which is the same as `step_out`). Phase 1
implements step_over as single-step — both descend into calls. True step_over is a
future enhancement.

**step_out**: reads the return address from `RSP` via `ReadProcessMemory(process, ctx.Rsp, 8)`,
installs a temporary INT3 breakpoint at that address (id = `u32::MAX`), continues, and
removes the temp BP after the first stop event.

---

## 4. Multi-Frame Stack Walking

### Decision: `StackWalk64` from DbgHelp with `SymInitialize` + `SymFunctionTableAccess64`

**Rationale**:
`StackWalk64` is the Windows-native stack walking API that handles frame pointer
omission, inlined frames, and exception handling correctly. It requires:
- `SymInitialize(process, NULL, TRUE)` called once after `CreateProcess`
  (`fInvadeProcess=TRUE` auto-loads symbols for all loaded modules)
- `SymFunctionTableAccess64` and `SymGetModuleBase64` as callbacks for unwind data

**ABI wrapper issue**: windows-rs 0.58 exposes `SymFunctionTableAccess64` and
`SymGetModuleBase64` as generic `unsafe fn` items. `StackWalk64` expects
`Option<unsafe extern "system" fn(...)>` callbacks. Solution: explicit
`unsafe extern "system"` wrapper functions that delegate to the windows-rs items.

```rust
unsafe extern "system" fn sym_func_table_access(h: HANDLE, base: u64) -> *mut c_void {
    SymFunctionTableAccess64(h, base)
}
```

**Frame resolution**: each frame's `AddrPC.Offset` (the instruction pointer) is
resolved via the PDB `va_to_function_name` and `va_to_source` lookups to produce
a `StackFrame { function_name, source_location }`.

---

## 5. Panic Message Extraction

### Decision: Anonymous stderr pipe + `PeekNamedPipe` + `ManuallyDrop<File>::read()`

**Rationale**:
Rust panics write their message to stderr synchronously (via the default panic hook)
before calling `abort()`. Capturing stderr via an anonymous pipe gives us the message
without needing to parse process memory or set breakpoints on internal Rust symbols.

**Implementation**:
```
CreatePipe(&read_end, &write_end, NULL, 0)
STARTUPINFOA { hStdError: write_end, dwFlags: STARTF_USESTDHANDLES, ... }
CreateProcessA(..., bInheritHandles: TRUE, ...)
CloseHandle(write_end)              // parent doesn't need write end
                    ↓
On EXIT_PROCESS_DEBUG_EVENT (exit_code ≠ 0)  or  unhandled exception:
PeekNamedPipe(read_end, available_bytes)
ManuallyDrop<File>::read(&mut buf)  // std::io::Read on raw HANDLE
extract_panic_message(stderr_text)
```

**Message parsing**: supports both Rust panic output formats:
- **New (Rust 1.65+)**: `thread 'main' panicked at file:line:\n<message>`
- **Old**: `thread 'main' panicked at '<message>', file:line`

**`ManuallyDrop<File>` rationale**: `std::fs::File::from_raw_handle` is used for
idiomatic Rust I/O. `ManuallyDrop` prevents `File::drop` from closing the HANDLE
(we close it explicitly in `WindowsDebugBackend::drop`).

**Why not `ReadFile` from windows-rs**: `ReadFile` is gated behind `Win32_Storage_FileSystem`
which introduces a heavyweight dependency tree. `std::io::Read` on a raw handle is
simpler and avoids the extra feature.

---

## Summary Table

| Decision | Choice | Key Reason |
|---|---|---|
| PDB loading | Pre-index at launch | O(log n) lookups, no runtime I/O |
| Local var locations | S_RegisterRelative (RBP-relative) | Debug builds always spill to stack |
| Single-step | EFLAGS.TF via GetThreadContext | Hardware mechanism, no polling |
| step_out | Temp INT3 at return address | Minimal; reuses existing BP machinery |
| Stack walking | StackWalk64 + SymInitialize | Windows-native, handles FPO/exceptions |
| Panic message | stderr pipe capture | Rust writes message before abort() |
| PDB crate | `pdb` 0.8 (pure Rust) | Zero external deps (Constitution VI) |
