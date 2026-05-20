<!--
SYNC IMPACT REPORT
==================
Version change: [unset] → 1.0.0
Modified principles: N/A (initial ratification — all principles are new)
Added sections:
  - Core Principles (7 principles)
  - Technology Stack & Architecture Constraints
  - Development Workflow
  - Governance
Removed sections: N/A (initial ratification)
Templates requiring updates:
  - .specify/templates/plan-template.md  ✅ aligned (Constitution Check section present; gates derive from principles)
  - .specify/templates/spec-template.md  ✅ aligned (no constitution-specific mandatory sections absent)
  - .specify/templates/tasks-template.md ✅ aligned (task phases map to crate-first and test-first principles)
  - .specify/templates/commands/         ⚠ pending — no command files found at path; validate when populated
  - README.md                            ⚠ pending — no root README.md found; create to document platform vision
Follow-up TODOs:
  - TODO(ROOT_README): Create /README.md at project root documenting platform vision and pointing to this constitution
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

**Rationale**: A mere debugger wrapper is replaceable by existing tools (LLDB CLI, VS Code). The
platform's value is autonomous reasoning over runtime state — that is the non-negotiable
differentiator.

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
`lldb-bridge` or `runtime-core` is FORBIDDEN. The protocol boundary MUST be the single integration
point for all AI-to-debugger interactions.

**Rationale**: Decouples AI reasoning from debugger internals. Enables agent replacement, protocol
versioning, and multi-agent topologies without modifying the runtime core.

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

### VI. Rust Safety First

All production code MUST be written in Rust (stable toolchain). `unsafe` blocks are FORBIDDEN
unless all three conditions are met:
1. An inline comment provides a complete safety proof at the `unsafe` block
2. A GitHub issue tracks the unsafe usage with a link in the comment
3. A safe alternative was considered and rejected with a written rationale in the same comment

Async code MUST use Tokio as the sole async runtime. No competing runtimes (async-std, smol) may
be introduced. The MSRV (Minimum Supported Rust Version) MUST be declared in `workspace.package`
in the root `Cargo.toml` and updated deliberately via a PR.

**Rationale**: Rust's safety guarantees prevent classes of runtime bugs that would undermine the
platform's role as a trusted runtime observer. Allowing ad-hoc unsafe use or runtime mixing
removes those guarantees.

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
- **UI framework**: egui via `crates/egui-introspection` and `apps/desktop-ui`
- **Debugger integration**: LLDB via `crates/lldb-bridge` (no GDB coupling in initial releases)
- **AI protocol**: MCP (Model Context Protocol) via `crates/protocol` and `apps/mcp-server`
- **Async runtime**: Tokio only — no mixing with async-std or smol
- **Serialization**: `serde` with `serde_json` or `postcard` for wire formats; no ad-hoc
  serialization logic outside of `crates/protocol`
- **Workspace layout** (authoritative):
  ```
  apps/
    desktop-ui/         # egui desktop application
    mcp-server/         # MCP protocol server
    replay-engine/      # deterministic trace replay
    autonomous-agent/   # AI-driven debug orchestrator

  crates/
    runtime-core/       # core runtime abstractions
    lldb-bridge/        # LLDB FFI integration
    event-stream/       # runtime event capture and dispatch
    semantic-runtime/   # semantic graph over runtime state
    egui-introspection/ # UI introspection widgets
    replay-core/        # replay primitives and execution engine
    trace-storage/      # trace persistence and querying
    test-automation/    # automated test execution harness
    ai-planning/        # hypothesis formulation and planning
    protocol/           # MCP protocol types and codec
  ```
- **Target platforms**: Linux (primary), macOS (secondary), Windows (best-effort)
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

## Governance

This constitution supersedes all other project practices, style guides, and informal conventions.
Amendments MUST follow this procedure:
1. Open a GitHub issue proposing the amendment, referencing the affected principle(s)
2. Obtain approval from at least one project maintainer (documented in the issue)
3. Include a migration plan for any in-flight specs or implementations affected
4. Increment `CONSTITUTION_VERSION` per the versioning policy below
5. Update `Last Amended` to the date of the merge commit

**Versioning policy**:
- MAJOR: Removal or backward-incompatible redefinition of a Core Principle
- MINOR: New principle or section added, or materially expanded guidance
- PATCH: Clarifications, wording fixes, non-semantic refinements

**Compliance review**: All pull requests MUST include a self-assessment of compliance with the
Core Principles in the PR description. Reviewers MUST reject PRs that violate principles without
an approved exception documented in this Governance section.

**Authoritative guidance**: This file (`.specify/memory/constitution.md`) is the single source of
truth for project governance. In any conflict between this document and other guidance files, this
constitution prevails.

**Version**: 1.0.0 | **Ratified**: 2026-05-20 | **Last Amended**: 2026-05-20
