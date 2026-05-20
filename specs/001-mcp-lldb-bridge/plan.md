# Implementation Plan: Phase 1 — MCP + Windows Debug Bridge

**Branch**: `001-mcp-lldb-bridge` | **Date**: 2026-05-20 | **Spec**: specs/001-mcp-lldb-bridge/

**Input**: User description — Phase 1 of the RDC AI-native Runtime Intelligence Platform

> **Revision note (2026-05-20)**: Original plan used PyO3 + LLDB Python API.
> Replaced by Windows Debug API (`windows-rs` + `pdb` crate) per Constitution v2.0.0.
> The `lldb-bridge` crate is being renamed and rewritten as `win-debug-bridge`.

## Summary

Build the foundational AI↔debugger bridge: a Rust workspace providing an MCP server that exposes
13 Windows Debug API-backed tools (breakpoints, execution control, stack inspection, variable
reading, expression evaluation) to AI agents. All integration is expressed as Rust crate
dependencies — zero external installation required beyond `rustup`.

Three core crates: `win-debug-bridge` (Windows Debug API + PDB parsing), `runtime-core`
(session entities, state machine, serialization), `protocol` (MCP types). One app: `mcp-server`.

The key innovation is the `probe!` macro: the AI receives
`{ "bubble_sort.pass": 2, "bubble_sort.swapped": true }` instead of `{ "x": 2, "flag": true }`.

## Technical Context

**Language/Version**: Rust stable (MSRV 1.75)

**Primary Dependencies**:
- `windows` 0.58 (`Win32_System_Diagnostics_Debug`, `Win32_System_Threading`, `Win32_System_Memory`, `Win32_Foundation`) — Windows Debug API
- `pdb` 0.8 — pure-Rust PDB symbol file parser
- `tokio` (full features) — async runtime
- `serde` + `serde_json` — serialization
- `tracing` + `tracing-subscriber` — structured logging
- `uuid` — session IDs
- `thiserror` — error types
- `clap` — CLI
- `anyhow` — app-level error handling

**Removed from original plan**:
- `pyo3` — Python interop (violated zero-install principle)
- `rmcp` — MCP SDK (replaced by hand-rolled JSON-RPC 2.0 dispatch; revisit when stable)

**Storage**: N/A for Phase 1

**Testing**: `cargo test --workspace`; integration test binary: `crates/debug-target-example`

**Target Platform**: Windows 10/11 x86-64 exclusively

**Performance Goals**:
- Debug API event round-trip: < 50ms p95
- `read_locals` with 20 variables: < 100ms
- Server startup to ready: < 2s

**Constraints**:
- Windows Debug API MUST be called from the same OS thread that called `CreateProcess`
- INT3 patching MUST save + restore original bytes atomically
- `unsafe` in `win-debug-bridge` MUST carry three-condition proof (Constitution Principle VI)
- `cargo build` on a clean Windows machine with Rust MUST succeed with no other prerequisites

**Scale/Scope**: Single debug session per `mcp-server` instance (MVP)

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Runtime Intelligence | ✅ PASS | Semantic probes elevate raw memory to structured meaning |
| II. Crate-First Modularity | ✅ PASS | `win-debug-bridge`, `runtime-core`, `protocol` are distinct crates |
| III. MCP as AI-Debugger Contract | ✅ PASS | Core deliverable; `mcp-server` is the sole integration point |
| IV. Deterministic Replay | ⚠ DEFERRED | Phase 4 — no event capture yet; not a violation |
| V. Autonomous Agent Discipline | ⚠ DEFERRED | Phase 5 — not applicable to bridge layer |
| VI. Rust Safety First — No External Deps | ✅ PASS | `windows-rs` + `pdb` are Rust crates; zero external install |
| VII. Open Platform Foundation | ✅ PASS | All `pub` APIs carry `///` docs |

**Gate result**: PASS

## Project Structure

### Documentation (this feature)

```text
specs/001-mcp-lldb-bridge/
├── plan.md                              # This file
├── research.md                          # Windows Debug API decision
├── data-model.md                        # Entity definitions
├── quickstart.md                        # Windows validation guide
├── contracts/
│   ├── mcp-tools.md                     # 13 MCP tool schemas
│   └── variable-serialization.md        # Variable JSON format
└── tasks.md                             # Task list
```

### Source Code

```text
Cargo.toml                               # workspace root

apps/
└── mcp-server/
    └── src/
        ├── main.rs                      # CLI, tracing, LLDBHandle spawn
        ├── server.rs                    # JSON-RPC dispatch loop
        └── handlers/
            ├── session.rs
            ├── breakpoints.rs
            ├── execution.rs
            └── inspection.rs

crates/
├── win-debug-bridge/                    # ← renamed from lldb-bridge
│   └── src/
│       ├── lib.rs                       # DebuggerBackend trait
│       ├── thread.rs                    # DebugCommand channel + WindowsDebugHandle
│       └── windows_backend.rs          # Windows Debug API implementation
│           ├── process mgmt            # CreateProcess, WaitForDebugEvent loop
│           ├── breakpoints             # INT3 patch/restore
│           ├── stepping                # EFLAGS.TF single-step
│           ├── memory                  # ReadProcessMemory
│           └── symbols                 # pdb crate integration
├── runtime-core/                        # Unchanged
├── protocol/                            # Unchanged
└── debug-target-example/               # Bubble sort + panic test binary
```

## Refactoring Plan (lldb-bridge → win-debug-bridge)

### What changes

| Item | Before | After |
|------|--------|-------|
| Crate name | `lldb-bridge` | `win-debug-bridge` |
| Implementation file | `python_backend.rs` (PyO3) | `windows_backend.rs` (windows-rs) |
| Dependencies | `pyo3` | `windows`, `pdb` |
| Backend struct | `PythonBackend` | `WindowsDebugBackend` |
| Channel struct | `LLDBHandle` | `WindowsDebugHandle` |

### What stays the same

- `DebuggerBackend` trait (unchanged — this abstraction is working)
- `DebugCommand` enum structure (renamed variants only)
- `runtime-core` entities — all unchanged
- `protocol` types — all unchanged
- `mcp-server` handlers — import path update only
- `apps/mcp-server/src/main.rs` — import path update only

### Migration steps

1. Create `crates/win-debug-bridge/` with new `Cargo.toml`
2. Copy `thread.rs` → update `LLDBHandle` → `WindowsDebugHandle`, `LLDBCommand` → `DebugCommand`
3. Write `windows_backend.rs` implementing `DebuggerBackend` with Windows Debug API
4. Update workspace `Cargo.toml` members + `apps/mcp-server/Cargo.toml` deps
5. Delete `crates/lldb-bridge/`
6. `cargo build --workspace` — verify zero errors

## Acceptance Criteria

Via MCP tools against `debug-target-example.exe` (bubble sort binary):

1. `launch_process` → state `Running`, non-zero PID
2. `set_breakpoint` at `bubble_sort` inner loop line → `resolved: true`
3. `continue_execution` → `BreakpointHit` at that line
4. `read_locals` → `pass`, `i`, `swapped`, `arr` with correct values
5. `read_locals` with `probe_context: "bubble_sort"` → `bubble_sort.pass`, `bubble_sort.swapped`
6. `step_over` → line advances by 1
7. `step_into` `bubble_sort` from `main` → enters function
8. `step_out` → returns to `main`
9. `evaluate_expression` `arr[0]` → current value of first element
10. `list_threads` → at least one thread
11. `read_stack` → `bubble_sort` → `main`
12. Panic mode: `continue_execution` → `PanicDetected { message: "index out of bounds..." }`

---

## Roadmap Context (Future Phases)

This plan covers Phase 1 only. Future feature branches:

| Branch | Phase | Description |
|--------|-------|-------------|
| `002-runtime-event-stream` | Phase 2 | Streaming `RuntimeEvent` bus; live timeline; `event-stream` crate |
| `003-egui-introspection` | Phase 3 | `UiSnapshot`, `WidgetNode` graph; `egui-introspection` crate |
| `004-deterministic-replay` | Phase 4 | `RuntimeFrame`, `replay-engine`, `trace-storage` crates |
| `005-autonomous-agent` | Phase 5 | Hypothesis engine, autonomous debug loops; `ai-planning` crate |