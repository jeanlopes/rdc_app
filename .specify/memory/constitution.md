<!--
SYNC IMPACT REPORT
==================
Version change: 2.0.0 → 3.0.0 (MAJOR)
Rationale for MAJOR bump: re-introduction of LLDB as a debugger integration option via the
`lldb-sys` / `lldb-safe` crates (native Rust FFI bindings to liblldb.dll), replacing the
DAP+codelldb external-process approach. The `crates/win-debug-bridge` (Win32 Debug API) remains
as an alternative backend. The external-process boundary that triggered antivirus interference is
eliminated by both options; the choice of backend is now configuration-level.

Motivation for reverting the v2.0.0 "no LLDB" constraint:
  - The Win32 Debug API backend (`win-debug-bridge`) lacks LLDB's symbol-resolution depth (DWARF,
    type system, expression evaluation) which limits future autonomous-agent capabilities.
  - The v2.0.0 prohibition was aimed at the external codelldb *process* (DAP adapter), not at
    linking liblldb.dll in-process. lldb-sys/lldb-safe links the DLL directly — no external
    process is spawned, so antivirus behavioural hooks are not triggered.
  - The deployment requirement (LLVM installation) is accepted and must be explicitly documented.

Modified principles:
  - VI. Rust Safety First: LLDB via lldb-sys is explicitly allowed; system LLVM install
    is accepted as a documented build prerequisite; Python and PyO3 remain forbidden.

Modified sections:
  - Technology Stack & Architecture Constraints:
    * Debugger integration: dual-backend — lldb-sys (primary) + win-debug-bridge (alternative)
    * Build prerequisite added: LLVM 19 system installation (LLDB_SYS_PREFIX env var)
    * crates/lldb-native added to workspace layout (replaces crates/lldb-bridge)

Templates requiring updates:
  - .specify/templates/plan-template.md  ⚠ pending
  - .specify/templates/spec-template.md  ⚠ pending
  - README.md                            ⚠ pending — document LLVM install prerequisite

Follow-up TODOs:
  - TODO(README): Document LLVM 19 installation as a build prerequisite
  - TODO(MAINTAINERS): Define initial maintainer list for governance amendment approval
-->

# RDC App Constitution

## Core Principles

### I. Runtime Intelligence, Not Tool Wrapping

The RDC platform MUST function as an active intelligence layer over program execution, not a
passive debugger wrapper or log viewer. The AI component MUST observe live execution, understand
program state, correlate runtime behavior with source code, formulate falsifiable hypotheses, and
execute hypothesis-test loops autonomously. Passive observation is insufficient; the system MUST
reason and act.

**Rationale**: A mere debugger wrapper is replaceable by existing tools (VS Code debugger,
WinDbg). The platform's value is autonomous reasoning over runtime state — that is the
non-negotiable differentiator.

### II. Crate-First Modularity (NON-NEGOTIABLE)

Every discrete capability MUST be a standalone Rust crate in the `crates/` workspace. Each crate
MUST:
- Have a single, clearly stated responsibility
- Be independently compilable and testable via `cargo test --package <crate>`
- Expose a public API with documented guarantees
- Carry no circular dependencies on other workspace crates

`apps/` (desktop-ui, mcp-server, replay-engine, autonomous-agent) MUST depend on `crates/`, never
the reverse. No capability may be siloed inside an `apps/` binary without a corresponding library
crate in `crates/`.

**Rationale**: Enforces independent testability, prevents monolithic binaries, and enables future
embedding of individual crates into third-party tooling without pulling in the entire platform.

### III. MCP as the AI-Debugger Contract (NON-NEGOTIABLE)

All communication between AI agents and the debugger runtime MUST transit the MCP protocol layer
(`apps/mcp-server`, `crates/protocol`). Direct in-process coupling between `autonomous-agent` and
`win-debug-bridge` or `runtime-core` is FORBIDDEN. The protocol boundary MUST be the single
integration point for all AI-to-debugger interactions.

**Rationale**: Decouples AI reasoning from debugger internals. Enables backend replacement
(e.g., swapping `win-debug-bridge` for a future remote-debug backend), protocol versioning, and
multi-agent topologies without modifying the runtime core.

### IV. Deterministic Replay

All runtime events captured during a debug session MUST be persisted via `crates/trace-storage`
and replayable via `apps/replay-engine`. The replay MUST be deterministic: re-executing a recorded
trace MUST produce the same observable sequence of events. Non-deterministic inputs (wall-clock
timestamps, random seeds, external I/O) MUST be recorded as part of the trace at capture time and
injected — not regenerated — during replay.

**Rationale**: Reproducibility is the foundation of trustworthy debugging. Without deterministic
replay, autonomous agent hypotheses cannot be falsified against a stable reference execution.

### V. Autonomous Agent Discipline

The autonomous debugging agent (`apps/autonomous-agent`, `crates/ai-planning`) MUST:
- Express every debugging action as a falsifiable hypothesis with a stated expected observable
  outcome
- Validate each hypothesis by executing against the replay engine or a live debug session, not by
  inference alone
- Implement bounded hypothesis-test loops; maximum iteration count MUST be configurable at runtime
- Persist each hypothesis, test result, and reasoning step to a structured, queryable log

The agent MUST NOT emit a debugging conclusion without supporting execution evidence recorded in
`crates/trace-storage`.

**Rationale**: Unbounded or unverifiable AI reasoning erodes user trust. Grounding every conclusion
in observable runtime evidence makes the agent auditable and its behavior improvable over time.

### VI. Rust Safety First — No External Runtime Dependencies

All production code MUST be written in Rust (stable toolchain). The debugger integration MUST use
only Rust crates — no external language runtimes and no language interpreters (Python, PyO3, etc.).
`unsafe` blocks are FORBIDDEN unless all three conditions are met:
1. An inline comment provides a complete safety proof at the `unsafe` block
2. A GitHub issue tracks the unsafe usage with a link in the comment
3. A safe alternative was considered and rejected with a written rationale in the same comment

`unsafe` is expected and accepted in:
- `crates/win-debug-bridge`: Windows API calls (`ReadProcessMemory`, `WriteProcessMemory`,
  `CreateProcess`, etc.)
- `crates/lldb-native`: LLDB C API FFI calls via `lldb-sys` / `lldb-safe`

Each `unsafe` call site in both crates MUST carry the three-condition proof above.

**LLDB via lldb-sys is explicitly permitted** as a debugger integration backend, subject to:
- Linking occurs in-process via `liblldb.dll` — no external debugger process may be spawned.
- LLVM 19 must be installed on the development/build machine and `LLDB_SYS_PREFIX` set.
  This is an accepted and documented build prerequisite, not a runtime dependency.
- Python, PyO3, and any Python-based LLDB scripting bridges remain FORBIDDEN.

Async code MUST use Tokio as the sole async runtime. No competing runtimes (async-std, smol) may
be introduced. The MSRV MUST be declared in `workspace.package` in the root `Cargo.toml`.

**Rationale**: Python/PyO3 and external debugger *processes* create invisible deployment failures
and antivirus surface area. Pure-Rust crates plus an explicitly documented LLVM build prerequisite
are acceptable; spawning a foreign process (codelldb.exe, lldb.exe) is not.

### VII. Open Platform Foundation

All public crate APIs (`pub` items at each crate root) MUST carry `///` doc comments and include
at least one usage example. Once a crate reaches 1.0, breaking changes MUST trigger a MAJOR semver
bump in that crate's `Cargo.toml` and an accompanying migration guide in `docs/migrations/`. Pre-1.0
crates MAY introduce breaking changes on MINOR version bumps but MUST update all workspace callers
atomically in the same commit.

**Rationale**: RDC is designed as an open-source platform foundation. Third-party integrators
depend on stable, documented contracts. Undocumented or silent breakage destroys ecosystem trust.

## Technology Stack & Architecture Constraints

- **Primary language**: Rust (stable toolchain; MSRV declared in workspace `Cargo.toml`)
- **Target platform**: Windows 10/11 (x86-64). No Linux or macOS support is planned or claimed.
- **UI framework**: egui via `crates/egui-introspection` and `apps/desktop-ui`
- **Debugger integration**: Dual-backend architecture.
  - **Primary**: LLDB 19 via `lldb-sys` / `lldb-safe` crates, linking `liblldb.dll` in-process
    (`crates/lldb-native`). Build prerequisite: LLVM 19 installed, `LLDB_SYS_PREFIX` set.
  - **Alternative**: Windows Debug API via `windows-rs` (`crates/win-debug-bridge`), zero system
    dependencies, symbol resolution via the `pdb` crate.
  - **Forbidden**: External debugger processes (codelldb.exe, lldb.exe, gdb.exe), Python/PyO3
    scripting bridges, any GDB-based backend.
- **AI protocol**: MCP (Model Context Protocol) via `crates/protocol` and `apps/mcp-server`
- **Async runtime**: Tokio only — no mixing with async-std or smol
- **Serialization**: `serde` with `serde_json` or `postcard` for wire formats; no ad-hoc
  serialization logic outside of `crates/protocol`
- **External dependencies policy**: ZERO runtime dependencies outside of Rust crates.
  `cargo build` on a fresh Windows machine with Rust installed MUST produce a working binary.
- **Workspace layout** (authoritative):
  ```
  apps/
    desktop-ui/           # egui desktop application
    mcp-server/           # MCP protocol server
    replay-engine/        # deterministic trace replay
    autonomous-agent/     # AI-driven debug orchestrator

  crates/
    runtime-core/         # core abstractions: session, entities, state machine
    lldb-native/          # LLDB 19 in-process backend (lldb-sys + lldb-safe FFI)
    win-debug-bridge/     # Windows Debug API integration (windows-rs + pdb) — alternative backend
    event-stream/         # runtime event capture and dispatch
    semantic-runtime/     # semantic graph over runtime state
    egui-introspection/   # UI introspection widgets
    replay-core/          # replay primitives and execution engine
    trace-storage/        # trace persistence and querying
    test-automation/      # automated test execution harness
    ai-planning/          # hypothesis formulation and planning
    protocol/             # MCP protocol types and codec
  ```
- **Observability**: All crates processing runtime events MUST emit structured log events via the
  `tracing` crate. Silent failure is FORBIDDEN.

## Development Workflow

- **Spec-Driven**: Every feature MUST have a spec (`spec.md`) and implementation plan (`plan.md`)
  before any code is written. The mandated workflow is:
  `/speckit-specify` → `/speckit-clarify` → `/speckit-plan` → `/speckit-tasks` →
  `/speckit-implement`
- **Test-First**: For any new crate capability, tests MUST be written and confirmed failing before
  implementation begins. The Red-Green-Refactor cycle is strictly enforced.
- **Crate PR Scope**: Pull requests MUST NOT span more than two workspace crates unless the change
  is an atomic refactor updating all callers. Cross-crate feature work MUST be broken into
  sequential crate-scoped PRs reviewed independently.
- **Constitution Check in Plans**: Every `plan.md` MUST include a Constitution Check section
  verifying compliance with all seven Core Principles before Phase 0 research proceeds.
  Non-compliant plans MUST NOT advance to task generation.
- **Observability gate**: Any PR adding a new runtime-event-handling code path that lacks
  `tracing` instrumentation MUST be rejected at review.
- **Zero-install gate**: Any PR that introduces a dependency on a system-installed tool, runtime,
  or interpreter MUST be rejected. All dependencies MUST be expressible in `Cargo.toml`.

## Governance

This constitution supersedes all other project practices, style guides, and informal conventions.
Amendments MUST follow this procedure:
1. Open a GitHub issue proposing the amendment, referencing the affected principle(s)
2. Obtain approval from at least one project maintainer (documented in the issue)
3. Include a migration plan for any in-flight specs or implementations affected
4. Increment `CONSTITUTION_VERSION` per the versioning policy below
5. Update `Last Amended` to the date of the merge commit

**Versioning policy**:
- MAJOR: Removal or backward-incompatible redefinition of a Core Principle or Technology Stack
- MINOR: New principle or section added, or materially expanded guidance
- PATCH: Clarifications, wording fixes, non-semantic refinements

**Compliance review**: All pull requests MUST include a self-assessment of compliance with the
Core Principles in the PR description. Reviewers MUST reject PRs that violate principles without
an approved exception documented in this Governance section.

**Authoritative guidance**: This file (`.specify/memory/constitution.md`) is the single source of
truth for project governance. In any conflict between this document and other guidance files, this
constitution prevails.

**Version**: 3.0.0 | **Ratified**: 2026-05-20 | **Last Amended**: 2026-05-27
