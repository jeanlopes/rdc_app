---
description: "Task list for 002-pdb-stepping-panic — unit + integration test coverage"
---

# Tasks: Debug Engine Completion — Test Coverage

**Input**: Design documents from `specs/002-pdb-stepping-panic/`

**Prerequisites**: plan.md ✅, spec.md ✅, data-model.md ✅, research.md ✅

**Context**: Implementation is complete in `001-mcp-lldb-bridge`. All tasks in this list
are about **writing tests** as specified in `plan.md § Test Coverage`. No production code
changes required.

**Tests**: REQUIRED — this is a test-writing feature. All tasks produce `#[test]` functions.

**Organization**: Tasks grouped by user story. Each user story's tests are independently
runnable with `cargo test -p <crate> <test_name>`.

## Format: `[ID] [P?] [Story?] Description — file path`

- **[P]**: Parallelizable (different files/test modules)
- **[Story]**: US1–US6 per spec.md

## Path conventions

```
crates/runtime-core/src/    ← add #[cfg(test)] mod tests {} to each source file
crates/protocol/src/        ← same
crates/win-debug-bridge/src/ ← same
crates/win-debug-bridge/tests/integration.rs  ← new file for #[ignore] tests
```

---

## Phase 1: Setup

**Purpose**: Add `#[cfg(test)]` module stubs and create the integration test file.

- [X] T001 Add `#[cfg(test)] mod tests {}` block to `crates/runtime-core/src/error.rs` (if absent)
- [X] T002 [P] Add `#[cfg(test)] mod tests {}` block to `crates/runtime-core/src/session.rs` (if absent)
- [X] T003 [P] Add `#[cfg(test)] mod tests {}` block to `crates/runtime-core/src/breakpoint.rs` (if absent)
- [X] T004 [P] Add `#[cfg(test)] mod tests {}` block to `crates/runtime-core/src/probe.rs` (if absent)
- [X] T005 [P] Add `#[cfg(test)] mod tests {}` block to `crates/runtime-core/src/serialization.rs` (if absent)
- [X] T006 [P] Add `#[cfg(test)] mod tests {}` block to `crates/protocol/src/error.rs` (if absent)
- [X] T007 [P] Add `#[cfg(test)] mod tests {}` block to `crates/win-debug-bridge/src/pdb_info.rs` (if absent)
- [X] T008 [P] Add `#[cfg(test)] mod tests {}` block to `crates/win-debug-bridge/src/windows_backend.rs` (if absent)
- [X] T009 Create `crates/win-debug-bridge/tests/integration.rs` with a top-level comment and a placeholder `#[test] #[ignore] fn placeholder() {}` — `crates/win-debug-bridge/tests/integration.rs`

**Checkpoint**: `cargo test --workspace` runs with no compilation errors (even if 0 tests).

---

## Phase 2: Foundational — `runtime-core` error + protocol error mapping

**Purpose**: Core error type tests needed by all user stories' test assertions.
These tests are in `crates/runtime-core/src/error.rs` and `crates/protocol/src/error.rs`.

- [X] T010 Write `error_process_not_found_display` — assert `DebuggerError::ProcessNotFound.to_string()` contains "process not found" — `crates/runtime-core/src/error.rs`
- [X] T011 [P] Write `error_breakpoint_not_found_display` — assert `BreakpointNotFound(5).to_string()` contains "5" — `crates/runtime-core/src/error.rs`
- [X] T012 [P] Write `error_invalid_state_display` — assert `InvalidState { current: "Running".to_string(), required: "Paused" }.to_string()` contains "Running" and "Paused" — `crates/runtime-core/src/error.rs`
- [X] T013 Write `invalid_state_maps_to_minus_32001` — assert `to_mcp_error_code(&DebuggerError::InvalidState { current: "".to_string(), required: "" })` == `-32001` — `crates/protocol/src/error.rs`
- [X] T014 [P] Write `breakpoint_not_found_maps_to_minus_32003` — `to_mcp_error_code(&BreakpointNotFound(1))` == `-32003` — `crates/protocol/src/error.rs`
- [X] T015 [P] Write `thread_not_found_maps_to_minus_32004` — `to_mcp_error_code(&ThreadNotFound(1))` == `-32004` — `crates/protocol/src/error.rs`
- [X] T016 [P] Write `generic_error_maps_to_minus_32000` — `to_mcp_error_code(&ProcessNotFound)` == `-32000` — `crates/protocol/src/error.rs`

**Checkpoint**: `cargo test -p runtime-core error` and `cargo test -p protocol error` pass.

---

## Phase 3: User Story 1 — Source-Line Breakpoints (P1) 🎯 MVP

**Goal**: Tests that verify the PDB source-line resolution logic used by `set_breakpoint`.

**Independent Test**: `cargo test -p win-debug-bridge pdb_info::tests` — all pass without any PDB file.

### `pdb_info.rs` — free functions

- [X] T017 [P] [US1] Write `short_name_strips_hash` — `short_name("bubble_sort::h1a2b3c4d")` == `"bubble_sort"` — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T018 [P] [US1] Write `short_name_takes_last_segment` — `short_name("std::vec::Vec::push")` == `"push"` — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T019 [P] [US1] Write `short_name_simple` — `short_name("main")` == `"main"` — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T020 [P] [US1] Write `primitive_size_bool` — `primitive_size_from_type_name("bool")` == `1` — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T021 [P] [US1] Write `primitive_size_i32` — `primitive_size_from_type_name("i32")` == `4` — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T022 [P] [US1] Write `primitive_size_usize` — `primitive_size_from_type_name("usize")` == `8` — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T023 [P] [US1] Write `primitive_size_unknown` — `primitive_size_from_type_name("MyStruct")` == `0` — `crates/win-debug-bridge/src/pdb_info.rs`

### `pdb_info.rs` — VA↔RVA math

- [X] T024 [P] [US1] Write `rva_to_va_adds_base` — construct `PdbInfo` with `image_base = 0x140000000`; assert `rva_to_va(0x1234)` == `0x140001234` — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T025 [P] [US1] Write `va_to_rva_subtracts_base` — assert `va_to_rva(0x140001234)` == `Some(0x1234)` — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T026 [P] [US1] Write `va_to_rva_below_base_returns_none` — assert `va_to_rva(0x100)` with base `0x140000000` == `None` — `crates/win-debug-bridge/src/pdb_info.rs`

### `pdb_info.rs` — lookups with synthetic data

Build a `PdbInfo` directly by populating its internal maps (make fields `pub(crate)` if needed).

- [X] T027 [US1] Write `va_to_source_exact_hit` — insert `rva=0x1000 → ("main.rs", 42)`; assert `va_to_source(image_base + 0x1000)` == `Some(SourceLocation { line: 42, .. })` — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T028 [P] [US1] Write `va_to_source_nearest_within_range` — insert rva `0x1000`; assert `va_to_source(image_base + 0x1000 + 50)` (within 256 bytes) returns the entry — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T029 [P] [US1] Write `va_to_source_too_far_returns_none` — 300 bytes past nearest entry → None — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T030 [P] [US1] Write `va_to_function_name_found` — insert function at rva `0x2000`; assert `va_to_function_name(image_base + 0x2000)` returns the name — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T031 [P] [US1] Write `va_to_function_name_not_found` — address before any function → None — `crates/win-debug-bridge/src/pdb_info.rs`

**Checkpoint**: `cargo test -p win-debug-bridge pdb` — all pass.

---

## Phase 4: User Story 2 — Local Variable Inspection (P2)

**Goal**: Tests that verify `bytes_to_value` and the serializer used by `read_locals`.

**Independent Test**: `cargo test -p win-debug-bridge bytes` — all pass.

### `windows_backend.rs` — `bytes_to_value`

- [X] T032 [P] [US2] Write `bytes_bool_false` — `bytes_to_value(&[0x00], "bool", 0)` == `Scalar(Bool(false))` — `crates/win-debug-bridge/src/windows_backend.rs`
- [X] T033 [P] [US2] Write `bytes_bool_true` — `bytes_to_value(&[0x01], "bool", 0)` == `Scalar(Bool(true))` — `crates/win-debug-bridge/src/windows_backend.rs`
- [X] T034 [P] [US2] Write `bytes_i32_positive` — `bytes_to_value(&[0x2c,0x00,0x00,0x00], "i32", 0)` == `Scalar(Int(44))` — `crates/win-debug-bridge/src/windows_backend.rs`
- [X] T035 [P] [US2] Write `bytes_i32_negative` — `bytes_to_value(&[0xff,0xff,0xff,0xff], "i32", 0)` == `Scalar(Int(-1))` — `crates/win-debug-bridge/src/windows_backend.rs`
- [X] T036 [P] [US2] Write `bytes_u32` — `bytes_to_value(&[0x05,0x00,0x00,0x00], "u32", 0)` == `Scalar(UInt(5))` — `crates/win-debug-bridge/src/windows_backend.rs`
- [X] T037 [P] [US2] Write `bytes_usize` — 8 zero bytes + `0x08` for "usize" → `Scalar(UInt(8))` — `crates/win-debug-bridge/src/windows_backend.rs`
- [X] T038 [P] [US2] Write `bytes_f32` — IEEE-754 bytes for `1.0f32` → `Scalar(Float(1.0))` — `crates/win-debug-bridge/src/windows_backend.rs`
- [X] T039 [P] [US2] Write `bytes_unknown_type_returns_opaque` — any bytes with type "SomeStruct" → `Opaque { summary: contains "bytes" }` — `crates/win-debug-bridge/src/windows_backend.rs`

### `runtime-core/src/serialization.rs` — Serializer (linked to read_locals output)

- [X] T040 [P] [US2] Write `serialize_bool_true` — `Scalar(Bool(true))` serializes to JSON `true` — `crates/runtime-core/src/serialization.rs`
- [X] T041 [P] [US2] Write `serialize_int_negative` — `Scalar(Int(-12))` serializes to JSON `-12` — `crates/runtime-core/src/serialization.rs`
- [X] T042 [P] [US2] Write `serialize_string_within_limit` — string length < `max_string_bytes` → no `$truncated` key — `crates/runtime-core/src/serialization.rs`
- [X] T043 [P] [US2] Write `serialize_string_over_limit` — string length > limit → `$truncated: true` and `total_bytes` present — `crates/runtime-core/src/serialization.rs`
- [X] T044 [US2] Write `serialize_depth_limit` — struct nested deeper than `max_depth` → inner level has `$depth_limit: true` — `crates/runtime-core/src/serialization.rs`
- [X] T045 [P] [US2] Write `serialize_array_within_limit` — array length ≤ `max_array_elements` → no `$truncated` — `crates/runtime-core/src/serialization.rs`
- [X] T046 [P] [US2] Write `serialize_array_over_limit` — array length > limit → `$truncated: true`, `shown` and `total` set — `crates/runtime-core/src/serialization.rs`
- [X] T047 [P] [US2] Write `serialize_cyclic_ref` — same pointer address visited twice → second occurrence has `$ref: "0x..."` — `crates/runtime-core/src/serialization.rs`
- [X] T048 [P] [US2] Write `serialize_null_pointer` — `Pointer { address: 0, .. }` → JSON has `null: true` — `crates/runtime-core/src/serialization.rs`

**Checkpoint**: `cargo test -p runtime-core serial` and `cargo test -p win-debug-bridge bytes` — all pass.

---

## Phase 5: User Story 3 — Execution Stepping (P3)

**Goal**: Tests that verify the session state machine correctly tracks stepping transitions.

**Independent Test**: `cargo test -p runtime-core session` — all pass.

- [X] T049 [P] [US3] Write `session_new_starts_idle` — `DebugSession::new(target).state` == `SessionState::Idle` — `crates/runtime-core/src/session.rs`
- [X] T050 [P] [US3] Write `transition_idle_to_launching` — `session.transition(Launching)` → Ok — `crates/runtime-core/src/session.rs`
- [X] T051 [P] [US3] Write `transition_idle_to_running_fails` — `session.transition(Running)` → Err(InvalidState) — `crates/runtime-core/src/session.rs`
- [X] T052 [P] [US3] Write `transition_running_to_paused` — Ok — `crates/runtime-core/src/session.rs`
- [X] T053 [P] [US3] Write `transition_paused_to_stepping` — Ok — `crates/runtime-core/src/session.rs`
- [X] T054 [P] [US3] Write `transition_stepping_to_paused` — Ok — `crates/runtime-core/src/session.rs`
- [X] T055 [P] [US3] Write `transition_paused_to_running` — Ok — `crates/runtime-core/src/session.rs`
- [X] T056 [P] [US3] Write `transition_terminated_is_terminal` — `session.transition(Running)` after `Terminated(0)` → Err(InvalidState) — `crates/runtime-core/src/session.rs`
- [X] T057 [US3] Write `full_session_lifecycle` — Idle→Launching→Running→Paused(Breakpoint)→Stepping→Paused(Step)→Running→Terminated — all transitions Ok — `crates/runtime-core/src/session.rs`

**Checkpoint**: `cargo test -p runtime-core session` — all 9 pass.

---

## Phase 6: User Story 4 — Call Stack Inspection (P4)

**Goal**: Tests for the PDB address→function-name lookups that populate `read_stack` frames.

**Independent Test**: `cargo test -p win-debug-bridge source` — all pass.

- [X] T058 [P] [US4] Write `source_to_va_round_trip` — insert `(stem, line) → rva` entry; assert `source_to_va(file, line)` returns the correct VA — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T059 [P] [US4] Write `source_to_va_unknown_returns_none` — file/line not in map → None — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T060 [P] [US4] Write `function_name_to_va_exact` — assert `function_name_to_va("bubble_sort")` returns the inserted RVA as VA — `crates/win-debug-bridge/src/pdb_info.rs`
- [X] T061 [P] [US4] Write `function_name_to_va_missing` — name not in map → None — `crates/win-debug-bridge/src/pdb_info.rs`

**Checkpoint**: `cargo test -p win-debug-bridge source` and `cargo test -p win-debug-bridge function` — all pass.

---

## Phase 7: User Story 5 — Expression Evaluation (P5)

**Goal**: Tests for probe registry and semantic annotation used by `evaluate_expression` and `read_locals`.

**Independent Test**: `cargo test -p runtime-core probe` — all pass.

- [X] T062 [P] [US5] Write `probe_registry_register_lookup` — `register("ctx", vec!["a","b"])` then `lookup("ctx")` == `Some(&["a","b"][..])` — `crates/runtime-core/src/probe.rs`
- [X] T063 [P] [US5] Write `probe_registry_unknown_returns_none` — `lookup("missing")` == None — `crates/runtime-core/src/probe.rs`
- [X] T064 [P] [US5] Write `probe_macro_returns_context_and_vars` — `probe!("ctx", x, y)` returns `("ctx".to_string(), vec!["x","y"])` — `crates/runtime-core/src/probe.rs`

**Checkpoint**: `cargo test -p runtime-core probe` — all 3 pass.

---

## Phase 8: User Story 6 — Panic Message Extraction (P6)

**Goal**: Tests for `extract_panic_message` — the free function that parses Rust panic output from stderr.

**Independent Test**: `cargo test -p win-debug-bridge panic` — all pass.

- [X] T065 [P] [US6] Write `panic_new_format` — input: `"thread 'main' panicked at src\\main.rs:58:22:\nindex out of bounds: the len is 3 but the index is 99\n"` → `Some("index out of bounds: the len is 3 but the index is 99")` — `crates/win-debug-bridge/src/windows_backend.rs`
- [X] T066 [P] [US6] Write `panic_old_format` — input with `panicked at 'message', file:line` → `Some("message")` — `crates/win-debug-bridge/src/windows_backend.rs`
- [X] T067 [P] [US6] Write `panic_empty_string_returns_none` — `extract_panic_message("")` == None — `crates/win-debug-bridge/src/windows_backend.rs`
- [X] T068 [P] [US6] Write `panic_unrelated_text_returns_none` — `extract_panic_message("hello world\n")` == None — `crates/win-debug-bridge/src/windows_backend.rs`
- [X] T069 [P] [US6] Write `panic_unwrap_on_none` — new-format input for `Option::unwrap()` → `Some` containing "called `Option::unwrap()` on a `None` value" — `crates/win-debug-bridge/src/windows_backend.rs`

**Checkpoint**: `cargo test -p win-debug-bridge panic` — all 5 pass.

---

## Phase 9: Polish & Cross-Cutting

**Purpose**: Remaining unit tests + integration tests with real binary.

### Breakpoint lifecycle (runtime-core, applies to all stories)

- [X] T070 [P] Write `breakpoint_hit_count_increments` — `bp.increment_hit_count()` → `bp.hit_count == 1`; call twice → `hit_count == 2` — `crates/runtime-core/src/breakpoint.rs`
- [X] T071 [P] Write `breakpoint_toggle_enabled` — starts `true`; toggle → `false`; toggle → `true` — `crates/runtime-core/src/breakpoint.rs`

### Integration tests (require `debug-target-example.pdb`)

- [X] T072 Write `pdb_load_succeeds` — `#[ignore]` — `PdbInfo::load("target/debug/debug-target-example.exe", base)` → Ok, functions count > 0 — `crates/win-debug-bridge/tests/integration.rs`
- [X] T073 [P] Write `pdb_source_to_va_main_line` — `#[ignore]` — `source_to_va("main.rs", KNOWN_LINE)` → Some(non-zero) — `crates/win-debug-bridge/tests/integration.rs`
- [X] T074 [P] Write `pdb_va_to_source_round_trip` — `#[ignore]` — `va_to_source(source_to_va("main.rs", L).unwrap())` → file contains "main", line == L — `crates/win-debug-bridge/tests/integration.rs`
- [X] T075 [P] Write `pdb_function_bubble_sort_found` — `#[ignore]` — `function_name_to_va("bubble_sort")` → Some — `crates/win-debug-bridge/tests/integration.rs`
- [X] T076 Write `pdb_locals_contain_pass` — `#[ignore]` — `locals_at_va(bubble_sort_va)` → list contains a `PdbLocal` with `name == "pass"` — `crates/win-debug-bridge/tests/integration.rs`
- [X] T077 Write `serialize_bool_false` — `Scalar(Bool(false))` serializes to JSON `false` — `crates/runtime-core/src/serialization.rs`

### Final validation

- [X] T078 Run `cargo test --workspace` and confirm all non-ignored tests pass — verify zero failures
- [ ] T079 [P] Run `cargo test --workspace -- --ignored` (requires `cargo build -p debug-target-example` first) — verify integration tests pass

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No deps — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 module stubs
- **US1–US6 (Phases 3–8)**: Depend on Phase 1 stubs; can run in parallel after that
- **Polish (Phase 9)**: Depends on all US phases

### Parallel Opportunities

- After Phase 1 completes: all user story test phases (3–8) can be written in parallel
- Within each phase: all `[P]`-marked tasks write to different test functions in the same file — safe to write concurrently

---

## Implementation Strategy

### Fastest path to `cargo test --workspace` going green

1. Phase 1: Setup stubs (T001–T009)
2. Phase 2: Error tests (T010–T016) — simplest, confirm compilation
3. Phase 8: extract_panic_message (T065–T069) — pure string logic, easiest wins
4. Phase 3: PDB free functions (T017–T031) — no PDB file needed
5. Phases 4–7 in parallel
6. Phase 9: Breakpoint tests + integration tests

---

## Notes

- All unit tests are `#[test]` with no `#[ignore]` — they run on `cargo test --workspace`
- Integration tests (T072–T076) are `#[test] #[ignore]` — require a pre-built binary
- `[P]` = different functions in the same file, safe to write in parallel
- `bytes_to_value` and `extract_panic_message` are free functions — make them `pub(crate)` if not already, to call from `#[cfg(test)]` modules
- For synthetic `PdbInfo` tests (T027–T031, T058–T061): add a `#[cfg(test)] impl PdbInfo { pub fn test_new(...) -> Self { ... } }` constructor or make fields `pub(crate)`
