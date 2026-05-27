# Implementation Plan: Native LLDB Backend via lldb-sys

**Branch**: `005-lldbsys-replace` | **Date**: 2026-05-27 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/005-lldbsys-replace/spec.md`

---

## Summary

Replace `crates/lldb-bridge` (DAP + external codelldb process) with a new `crates/lldb-native`
crate that calls LLDB 19 directly in-process via `lldb-sys` / `lldb-safe`. Extract a
`DebugBackend` trait into `crates/runtime-core` so `apps/mcp-server` can switch between
`lldb-native` and `win-debug-bridge` at startup via a CLI flag. One new C++ wrapper method is
added to `lldb-safe` for `SBFrame::GetLineEntry()` (source-level frame location).

---

## Technical Context

**Language/Version**: Rust stable (MSRV declared in workspace `Cargo.toml`)

**Primary Dependencies**:
- `lldb-safe` (local workspace `c:\workspace\lldb-sys\crates\lldb-safe`) — safe Rust wrappers
  around LLDB's C API: `Debugger`, `Target`, `Process`, `Thread`, `Frame`, `Breakpoint`, `Value`
- `lldb-sys` (local workspace `c:\workspace\lldb-sys\crates\lldb-sys`) — raw FFI to `liblldb.dll`
- `tokio` (sync channel primitives: `mpsc`, `oneshot`)
- `tracing` (structured events for all runtime paths)
- `runtime-core` (all shared entity types + new `DebugBackend` trait)
- `protocol` (MCP execution event types)

**Build prerequisite**: LLVM 19 installed (`scripts/install-llvm.ps1`), `LLDB_SYS_PREFIX` set

**Storage**: N/A

**Testing**: `cargo test --package lldb-native` (integration tests against `debug-target-example`)

**Target Platform**: Windows 10/11 x86-64

**Project Type**: Library crate in `crates/` workspace

**Performance Goals**: Step commands < 500ms; variable list (≤50 locals) < 200ms (spec SC-002/003)

**Constraints**: No external process spawned; in-process DLL only; `unsafe` requires three-proof

**Scale/Scope**: Single session per app instance

---

## Constitution Check *(post-amendment)*

*GATE: Re-checked after constitution v3.0.0 amendment (2026-05-27). All gates pass.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Runtime Intelligence | ✅ Pass | Active in-process debug control, no passive wrapper |
| II. Crate-First Modularity | ✅ Pass | New capability in standalone `crates/lldb-native` |
| III. MCP as AI-Debugger Contract | ✅ Pass | `DebugBackend` trait keeps MCP boundary intact |
| IV. Deterministic Replay | ⚠ Deferred | trace-storage integration is future work |
| V. Autonomous Agent Discipline | ⚠ Deferred | Out of scope |
| VI. Rust Safety First | ✅ Pass | lldb-sys allowed by v3.0.0; `unsafe` per three-proof rule |
| VII. Open Platform Foundation | ✅ Pass | All public items carry `///` docs + usage examples |

---

## Project Structure

### Documentation (this feature)

```text
specs/005-lldbsys-replace/
├── plan.md              ← this file
├── research.md          ← Phase 0 decisions
├── data-model.md        ← entity and dependency graph
├── contracts/
│   └── debug-backend-trait.md   ← DebugBackend trait definition
├── checklists/
│   └── requirements.md
└── tasks.md             ← Phase 2 output (/speckit-tasks)
```

### Source Code Layout

```text
crates/
├── runtime-core/
│   └── src/
│       ├── backend.rs           ← NEW: DebugBackend trait (async_trait)
│       └── [existing files unchanged]
│
├── lldb-native/                 ← NEW crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs               ← pub use handle::LldbNativeHandle
│       ├── handle.rs            ← LldbNativeHandle (Clone, Send, Sync)
│       ├── thread.rs            ← LldbDebugThread (owns SBDebugger on dedicated thread)
│       ├── command.rs           ← LldbCommand enum (mpsc message variants)
│       └── mapping.rs           ← lldb-safe types → runtime-core types
│
├── lldb-bridge/                 ← DEPRECATED (kept as dead stub; removed in follow-up)
│   └── [unchanged]
│
└── win-debug-bridge/
    └── [unchanged — alternative backend]

apps/
└── mcp-server/
    ├── Cargo.toml               ← ADD lldb-native dep; REMOVE lldb-bridge dep
    └── src/
        ├── main.rs              ← ADD --backend CLI flag; construct Arc<dyn DebugBackend>
        └── handlers/
            └── session.rs       ← CHANGE: LldbHandle → Arc<dyn DebugBackend>

[external workspace]
c:\workspace\lldb-sys/
└── crates/
    ├── lldb-safe/src/
    │   ├── frame.rs             ← ADD source_location() method
    │   └── [rest unchanged]
    └── lldb-sys/wrapper/src/
        └── SBLineEntry.cpp      ← NEW C++ wrapper for SBLineEntry
```

---

## Implementation Phases

### Phase A — lldb-sys extension (prerequisite)

1. Add `SBLineEntry.cpp` to `lldb-sys/wrapper/src/`
2. Expose `Frame::source_location() -> Option<(String, u32)>` in `lldb-safe/src/frame.rs`
3. Confirm `cargo build --package lldb-safe` compiles

### Phase B — `DebugBackend` trait in runtime-core

1. Create `crates/runtime-core/src/backend.rs` with the trait definition
2. Add `async-trait` to `runtime-core/Cargo.toml` dependencies
3. `pub mod backend; pub use backend::DebugBackend;` in `runtime-core/src/lib.rs`
4. Implement `DebugBackend` for `WindowsDebugHandle` in `win-debug-bridge`
   (method signatures already match; `impl` block is mechanical)
5. `cargo test --package runtime-core` passes

### Phase C — `crates/lldb-native` (new crate)

1. Create `crates/lldb-native/Cargo.toml`:
   - Depends on `lldb-safe` (path), `runtime-core`, `protocol`, `tokio`, `tracing`, `thiserror`
   - `[target.'cfg(windows)'.dependencies]` for `windows-sys` (DLL directory API)
2. Implement `command.rs` (LldbCommand enum)
3. Implement `thread.rs` (LldbDebugThread):
   - `spawn()` starts a `std::thread`, calls `set_lldb_dll_dir`, `Debugger::initialize()`
   - Message loop: `recv()` from mpsc, execute LLDB call, send result via oneshot
   - Teardown: `Debugger::terminate()` on thread exit
4. Implement `mapping.rs` (all type conversions)
5. Implement `handle.rs` (LldbNativeHandle):
   - `spawn()` → creates mpsc channel, starts LldbDebugThread, returns handle
   - Each async method: creates oneshot pair, sends LldbCommand, awaits reply
   - `impl DebugBackend for LldbNativeHandle`
6. Integration test: launch `debug-target-example`, set breakpoint, continue, verify stop
7. `cargo test --package lldb-native` passes

### Phase D — Wire into mcp-server

1. `apps/mcp-server/Cargo.toml`: add `lldb-native`; keep `lldb-bridge` temporarily
2. Add `--backend lldb-native | win-debug-bridge` CLI flag (default: `lldb-native`)
3. `SessionContext.handle: LldbDebugHandle` → `backend: Arc<dyn DebugBackend>`
4. Update all handler imports
5. End-to-end test: start mcp-server with `--backend lldb-native`, run MCP session

### Phase E — Cleanup

1. Remove `lldb-bridge` dependency from `mcp-server/Cargo.toml`
2. Mark `crates/lldb-bridge` as deprecated in its `Cargo.toml` description
3. Update workspace root `Cargo.toml` member list to keep `lldb-bridge` or remove it

---

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| LLVM system install (build prereq) | lldb-sys links liblldb.dll; DLL is ~100MB with transitive LLVM dependencies — cannot be embedded | Bundling the DLL is not practical at this size; win-debug-bridge lacks DWARF/type-system depth needed for future AI agent capabilities |
| New C++ wrapper (SBLineEntry.cpp) | `SBFrame::GetLineEntry()` is required for accurate source location on stack frames | Deriving source location via `evaluate_expression("__FILE__")` is fragile and unreliable |
