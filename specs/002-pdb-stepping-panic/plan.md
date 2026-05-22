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

**Testing**: `cargo test --workspace` for unit tests; `cargo test --workspace -- --ignored` for integration tests requiring a built binary

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

## Test Coverage

### Strategy

Tests are split into two tiers:

- **Unit tests** — `#[test]`, no Windows infrastructure, no file I/O. Run with
  `cargo test --workspace`. These cover all pure logic: state machines, serializers,
  free functions, and pre-built data structures.
- **Integration tests** — `#[test] #[ignore]`, require `debug-target-example.exe`
  and its `.pdb` to exist. Run with `cargo test --workspace -- --ignored`. These
  verify real PDB parsing against a known binary.

---

### crates/runtime-core — Phase 1 coverage

#### `src/error.rs`

| Test | Assertion |
|------|-----------|
| `error_process_not_found_display` | `DebuggerError::ProcessNotFound.to_string()` contains "process not found" |
| `error_breakpoint_not_found_display` | `BreakpointNotFound(5).to_string()` contains "5" |
| `error_invalid_state_display` | `InvalidState { current: "Running", required: "Paused" }.to_string()` contains both values |

#### `src/session.rs` — state machine

| Test | Assertion |
|------|-----------|
| `session_new_starts_idle` | `DebugSession::new(target).state == SessionState::Idle` |
| `transition_idle_to_launching` | `session.transition(Launching)` → Ok |
| `transition_idle_to_running_fails` | `session.transition(Running)` → Err(InvalidState) |
| `transition_running_to_paused` | Ok |
| `transition_paused_to_running` | Ok |
| `transition_paused_to_stepping` | Ok |
| `transition_stepping_to_paused` | Ok |
| `transition_terminated_is_terminal` | `session.transition(Running)` after Terminated → Err |
| `full_session_lifecycle` | Idle→Launching→Running→Paused→Running→Terminated all Ok |

#### `src/breakpoint.rs`

| Test | Assertion |
|------|-----------|
| `breakpoint_hit_count_increments` | `bp.increment_hit_count()` → `bp.hit_count == 1` |
| `breakpoint_toggle_enabled` | false→true→false |

#### `src/probe.rs`

| Test | Assertion |
|------|-----------|
| `probe_registry_register_lookup` | `register("ctx", ["a","b"])` then `lookup("ctx")` == `["a","b"]` |
| `probe_registry_unknown_context` | `lookup("missing")` == None |
| `probe_macro_returns_context_and_vars` | `probe!("ctx", x, y)` returns `("ctx", ["x","y"])` |

#### `src/serialization.rs`

| Test | Assertion |
|------|-----------|
| `serialize_bool_true` | `Scalar(Bool(true))` → json `true` |
| `serialize_bool_false` | `Scalar(Bool(false))` → json `false` |
| `serialize_int` | `Scalar(Int(-12))` → json `-12` |
| `serialize_string_within_limit` | value < limit → no `$truncated` |
| `serialize_string_over_limit` | value > limit → `$truncated: true`, `total_bytes` set |
| `serialize_array_within_limit` | len ≤ max → no `$truncated` |
| `serialize_array_over_limit` | len > max → `$truncated: true`, `shown` and `total` set |
| `serialize_depth_limit` | struct nested > max_depth → `$depth_limit: true` |
| `serialize_cyclic_ref` | same address visited twice → `$ref: "0x..."` |
| `serialize_null_pointer` | `Pointer { address: 0 }` → `null: true` |

---

### crates/protocol — Phase 1 coverage

#### `src/error.rs`

| Test | Assertion |
|------|-----------|
| `invalid_state_maps_to_minus_32001` | `to_mcp_error_code(InvalidState{..})` == `-32001` |
| `breakpoint_not_found_maps_to_minus_32003` | `to_mcp_error_code(BreakpointNotFound(1))` == `-32003` |
| `thread_not_found_maps_to_minus_32004` | `to_mcp_error_code(ThreadNotFound(1))` == `-32004` |
| `generic_error_maps_to_minus_32000` | `to_mcp_error_code(ProcessNotFound)` == `-32000` |

---

### crates/win-debug-bridge — Phase 2 coverage

#### `src/pdb_info.rs` — free functions (no PDB file)

| Test | Assertion |
|------|-----------|
| `short_name_strips_hash` | `short_name("bubble_sort::h1a2b3c4d")` == `"bubble_sort"` |
| `short_name_takes_last_segment` | `short_name("std::vec::Vec::push")` == `"push"` |
| `short_name_simple` | `short_name("main")` == `"main"` |
| `primitive_size_bool` | `primitive_size_from_type_name("bool")` == 1 |
| `primitive_size_i32` | `primitive_size_from_type_name("i32")` == 4 |
| `primitive_size_usize` | `primitive_size_from_type_name("usize")` == 8 |
| `primitive_size_unknown` | `primitive_size_from_type_name("MyStruct")` == 0 |

#### `src/pdb_info.rs` — PdbInfo math methods

| Test | Assertion |
|------|-----------|
| `rva_to_va_adds_base` | `PdbInfo { image_base: 0x140000000 }.rva_to_va(0x1234)` == `0x140001234` |
| `va_to_rva_subtracts_base` | `.va_to_rva(0x140001234)` == `Some(0x1234)` |
| `va_to_rva_underflow` | `.va_to_rva(0x100)` with base `0x140000000` == `None` |

#### `src/pdb_info.rs` — PdbInfo lookups with synthetic data

Build a `PdbInfo` directly (bypassing PDB loading) and test query methods:

| Test | Assertion |
|------|-----------|
| `va_to_source_exact_hit` | RVA in map → returns matching `SourceLocation` |
| `va_to_source_nearest` | RVA + 50 bytes (within 256) → returns nearest entry |
| `va_to_source_too_far` | RVA + 300 bytes → None |
| `va_to_function_name_found` | Address inside function range → name returned |
| `va_to_function_name_not_found` | Address before first function → None |

#### `src/windows_backend.rs` — `bytes_to_value` (free function, no Win32)

| Test | Bytes | Type | Expected |
|------|-------|------|----------|
| `bytes_bool_false` | `[0x00]` | "bool" | `Scalar(Bool(false))` |
| `bytes_bool_true` | `[0x01]` | "bool" | `Scalar(Bool(true))` |
| `bytes_i32_positive` | `[0x2c,0x00,0x00,0x00]` | "i32" | `Scalar(Int(44))` |
| `bytes_i32_negative` | `[0xff,0xff,0xff,0xff]` | "i32" | `Scalar(Int(-1))` |
| `bytes_u32` | `[0x05,0x00,0x00,0x00]` | "u32" | `Scalar(UInt(5))` |
| `bytes_usize` | `[0x08,0x00,0x00,0x00,0x00,0x00,0x00,0x00]` | "usize" | `Scalar(UInt(8))` |
| `bytes_f32` | IEEE-754 bytes for 1.0f32 | "f32" | `Scalar(Float(1.0))` |
| `bytes_unknown_type` | any | "SomeStruct" | `Opaque { summary: contains "bytes" }` |

#### `src/windows_backend.rs` — `extract_panic_message` (free function, no Win32)

| Test | Input | Expected |
|------|-------|----------|
| `panic_new_format` | `"thread 'main' panicked at src\\main.rs:58:22:\nindex out of bounds: the len is 3 but the index is 99\n"` | `Some("index out of bounds: the len is 3 but the index is 99")` |
| `panic_old_format` | `"thread 'main' panicked at 'index out of bounds: ...', src\\main.rs:58:22\n"` | `Some("index out of bounds: ...")` |
| `panic_empty_string` | `""` | `None` |
| `panic_no_panic_text` | `"some other stderr output\n"` | `None` (or Some with fallback) |
| `panic_unwrap_none` | new-format output for `unwrap()` | `Some` containing "called \`Option::unwrap()\`" |

---

### Integration tests — require `debug-target-example.pdb`

These tests are `#[ignore]` by default. Run with:
```powershell
cargo build -p debug-target-example
cargo test -p win-debug-bridge -- --ignored
```

| Test | Assertion |
|------|-----------|
| `pdb_load_succeeds` | `PdbInfo::load("target/debug/debug-target-example.exe", base)` → Ok |
| `pdb_source_to_va_bubble_sort_line` | `source_to_va("main.rs", BP_LINE)` → Some(non-zero va) |
| `pdb_va_to_source_round_trip` | `va_to_source(source_to_va("main.rs", L).unwrap())` → file contains "main", line == L |
| `pdb_function_lookup_bubble_sort` | `function_name_to_va("bubble_sort")` → Some |
| `pdb_locals_at_bubble_sort` | `locals_at_va(bubble_sort_va)` → list contains entry with name "pass" |

---

### How to run

```powershell
# Unit tests only (no binary required)
cargo test --workspace

# Unit + integration tests
cargo build -p debug-target-example
cargo test --workspace -- --ignored

# Single crate
cargo test -p runtime-core
cargo test -p win-debug-bridge
```

---

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
