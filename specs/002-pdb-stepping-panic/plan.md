# Implementation Plan: Debug Engine Completion — PDB, Stepping & Panic Messages

**Branch**: `002-pdb-stepping-panic` | **Date**: 2026-05-20 | **Spec**: specs/002-pdb-stepping-panic/

> **Note**: This plan is retroactive documentation. All implementation was delivered
> in branch `001-mcp-lldb-bridge` as part of completing that branch's acceptance criteria.
> The spec was created before the implementation; the plan documents the actual technical
> decisions made.

## Summary

Complete the three remaining work pieces that were stubbed during the `lldb-bridge` →
`win-debug-bridge` refactoring:

1. **PDB Integration** — parse `.pdb` files to resolve source lines ↔ addresses and
   enumerate local variables. Blocks: `set_breakpoint` by line, `read_locals`, `read_stack`,
   `evaluate_expression`.
2. **Single-step via EFLAGS.TF** — set CPU Trap Flag to advance one instruction at a time.
   Blocks: `step_over`, `step_into`, `step_out`.
3. **Panic message extraction** — capture child process stderr via anonymous pipe and parse
   Rust panic output. Completes: `PanicDetected { message }`.

All code lives in `crates/win-debug-bridge`. No other crates are modified.

## Technical Context

**Language/Version**: Rust stable (MSRV 1.75)

**Primary Dependencies**:
- `pdb` 0.8 — pure-Rust PDB symbol file parser (new)
- `windows` 0.58 with `Win32_System_Kernel` (new feature — required for `CONTEXT`,
  `GetThreadContext`, `SetThreadContext`)
- `windows` 0.58 with `Win32_System_Pipes` (new feature — for `CreatePipe`)
- All other dependencies unchanged from branch 001

**Storage**: N/A

**Testing**: `cargo build --workspace`; manual validation against `debug-target-example.exe`

**Target Platform**: Windows 10/11 x86-64

**Performance Goals**:
- PDB load at startup: < 500ms for typical Rust debug binary
- `read_locals` round-trip: < 100ms for ≤ 20 variables
- `read_stack` with 32 frames: < 50ms

**Constraints**:
- PDB file MUST be in the same directory as the `.exe` (standard Cargo output layout)
- `GetThreadContext`/`SetThreadContext` MUST be called from the debug OS thread with the
  thread in a suspended/debug-stopped state
- `StackWalk64` MUST be called after `SymInitialize` on the same process handle
- `unsafe` blocks MUST carry three-condition proof (Constitution Principle VI)

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Runtime Intelligence | ✅ PASS | PDB gives AI semantically meaningful variable names + source locations |
| II. Crate-First Modularity | ✅ PASS | All changes in `crates/win-debug-bridge`; no cross-crate contamination |
| III. MCP as AI-Debugger Contract | ✅ PASS | Existing MCP tool surface unchanged; implementation is backend-only |
| IV. Deterministic Replay | ⚠ DEFERRED | Phase 4 — not applicable |
| V. Autonomous Agent Discipline | ⚠ DEFERRED | Phase 5 — not applicable |
| VI. Rust Safety First — No External Deps | ✅ PASS | `pdb` crate = pure Rust; Windows API via `windows-rs`; all `unsafe` blocks have three-condition proofs |
| VII. Open Platform Foundation | ✅ PASS | `PdbInfo` and `PdbLocal` carry `///` doc comments |

**Gate result**: PASS

## Project Structure

### New files (all in `crates/win-debug-bridge/src/`)

```text
crates/win-debug-bridge/src/
├── lib.rs                 (updated: added pub mod pdb_info)
├── pdb_info.rs            (NEW) — PdbInfo, PdbLocal, VarLocation
├── thread.rs              (unchanged)
└── windows_backend.rs     (updated) — SymInitialize, read_stack, single_step,
                                       read_local_var, eval_expression, read_panic_message
```

### No new spec-level contracts

The existing `contracts/mcp-tools.md` and `contracts/variable-serialization.md` from
`specs/001-mcp-lldb-bridge/` remain authoritative. This feature fulfils the contracts
that were previously marked as stubs — it does not change their schemas.

## Phase 0: Research (Complete)

All decisions documented in `research.md`. Summary:

| Work piece | Decision | Key reason |
|------------|----------|------------|
| PDB loading | Pre-index at launch (BTreeMap + HashMap) | No runtime I/O on debug events |
| Source → VA | `LineProgram.lines()` + `address_map.to_rva()` + image_base | Standard pdb crate API |
| VA → source | `BTreeMap::range(..=rva).next_back()` | Nearest-RVA lookup, O(log n) |
| Local vars | `S_RegisterRelative` records, RBP-relative | Debug builds always spill to stack |
| Single-step | EFLAGS.TF bit (0x100) via `GetThreadContext`/`SetThreadContext` | CPU hardware mechanism |
| step_out | Temp INT3 at return address (read from RSP) | Reuses existing BP machinery |
| Stack walk | `StackWalk64` + `SymInitialize(fInvadeProcess=TRUE)` | Windows-native, handles FPO |
| Panic message | Anonymous stderr pipe + `PeekNamedPipe` + `ManuallyDrop<File>` | Rust writes to stderr before abort |

## Phase 1: Design (Complete)

All design artifacts generated:

- `research.md` — seven technical decisions with rationale and alternatives
- `data-model.md` — `PdbInfo`, `PdbLocal`, `VarLocation`, extended `WindowsDebugBackend`
- No new contracts (existing contracts fulfilled, schemas unchanged)

## Acceptance Criteria

All 12 criteria from `specs/001-mcp-lldb-bridge/plan.md` — verified via MCP tools
against `debug-target-example.exe` (bubble sort binary):

| # | Criterion | Implementation |
|---|-----------|----------------|
| 2 | `set_breakpoint` by source line → resolved | `PdbInfo::source_to_va` + `LineProgram.lines()` |
| 4 | `read_locals` → correct variable values | `S_RegisterRelative` + `ReadProcessMemory` |
| 5 | `probe_context` → qualified names | `SemanticAnnotation` applied in `read_local_var` |
| 6 | `step_over` → line +1 | `EFLAGS.TF` → `EXCEPTION_SINGLE_STEP` |
| 7 | `step_into` → enters function | `EFLAGS.TF` (same mechanism) |
| 8 | `step_out` → back to caller | Temp BP at return address (from RSP) |
| 9 | `evaluate_expression arr[0]` | Vec fat pointer read via `ReadProcessMemory` |
| 11 | `read_stack` → `bubble_sort → main` | `StackWalk64` loop + PDB frame resolution |
| 12 | `PanicDetected { message }` | stderr pipe + `extract_panic_message()` parser |
