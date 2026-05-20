# Research: Phase 1 — MCP + LLDB Bridge

**Feature**: 001-mcp-lldb-bridge
**Date**: 2026-05-20

---

## 1. LLDB Integration Strategy

### Decision: Python API via PyO3 (initial)

**Rationale**:
- LLDB ships its Python bindings (`lldb` Python module) on all major platforms as part of the
  standard LLDB installation. No additional headers or compilation against LLDB sources required.
- PyO3 provides safe, ergonomic Rust↔Python interop with well-maintained crate support.
- Reduces `unsafe` surface area dramatically compared to direct C++ FFI, satisfying
  Constitution Principle VI.
- Sufficient performance for interactive debugging (round-trip < 100ms target is met; Python GIL
  is held only during LLDB calls, not in the async path).

**Alternatives considered**:
- **LLDB C++ API via bindgen**: Maximum performance and no Python runtime dependency. Rejected
  for Phase 1 because bindgen output requires platform-specific LLDB headers, generates large
  `unsafe` surfaces, and significantly increases onboarding complexity. Viable for Phase 3+.
- **lldb-rs crate (unmaintained)**: Provides Rust bindings but last updated 2020, does not cover
  the full LLDB API surface needed. Rejected.
- **DAP (Debug Adapter Protocol) over stdio**: Higher-level than needed; DAP does not expose
  enough raw runtime state for semantic probes. Rejected.

**Migration path**: The `lldb-bridge` crate interface will be designed as a pure Rust trait
(`DebuggerBackend`), so the PyO3 implementation can be swapped for a C++ bindgen implementation
without changing callers.

---

## 2. MCP Protocol SDK

### Decision: `rmcp` crate (official Rust MCP SDK)

**Rationale**:
- `rmcp` is the official Rust SDK for the Model Context Protocol, maintained by the MCP
  organization. It provides server-side tool registration, JSON-RPC 2.0 message handling, and
  stdio/HTTP transport out of the box.
- Using an official SDK guarantees spec compliance and reduces protocol implementation risk.
- The crate supports `async` tool handlers natively (Tokio-based), aligning with
  Constitution Principle VI.

**Alternatives considered**:
- **Manual JSON-RPC 2.0 implementation**: Full control but significant boilerplate, risk of
  spec drift. Rejected in favor of `rmcp`; revisit if `rmcp` proves unstable.
- **jsonrpc crate**: Protocol-level only, does not handle MCP tool/resource abstractions.
  Rejected as too low-level.

**Fallback**: If `rmcp` API surface is insufficient or unstable at implementation time, implement
`crates/protocol` as a thin JSON-RPC 2.0 layer over `tokio-tungstenite` (HTTP/SSE) or `tokio`
stdio. The `crates/protocol` abstraction boundary isolates this decision from callers.

---

## 3. Async Bridging: LLDB (Synchronous) + Tokio (Async)

### Decision: Dedicated OS thread + `tokio::sync::mpsc` command channel

**Rationale**:
LLDB Python API is synchronous and GIL-bound. Calling it directly from an async Tokio task would
block the executor thread. The correct pattern is:

```
┌─────────────────────────────┐
│  Tokio runtime               │
│  ┌──────────────────────┐   │
│  │  MCP tool handler    │   │
│  │  (async fn)          │   │
│  │  sends LLDBCommand   │──────────────────┐
│  │  via mpsc::Sender    │   │              │
│  └──────────────────────┘   │              ▼
│                              │   ┌────────────────────┐
│                              │   │  LLDB Thread        │
│                              │   │  (std::thread::spawn│
│                              │   │  + Python GIL)      │
│                              │   │  executes command   │
│                              │   │  sends result via   │
│                              │   │  oneshot::Sender    │
└─────────────────────────────┘   └────────────────────┘
```

- `LLDBCommand` enum: one variant per debugger operation
- Each command carries a `tokio::sync::oneshot::Sender<LLDBResult>` for the reply
- LLDB thread loops on `mpsc::Receiver<LLDBCommand>`, executes synchronously, sends result back
- Tokio tasks `await` on the `oneshot::Receiver`

**Alternatives considered**:
- `tokio::task::spawn_blocking`: Simpler but spins up a new thread per call, causing GIL
  contention and Python interpreter re-acquisition overhead. Rejected.
- Async LLDB wrapper (none exist): No production-quality async LLDB crate exists. Rejected.

---

## 4. Semantic Probes

### Decision: Declarative `probe!` macro emitting `SemanticVariable` records

**Rationale**:
Raw variable inspection returns opaque values (`{ "x": 88 }`). A semantic probe attaches a named
context to a group of related variables, enabling the AI to reason about meaning rather than raw
state:

```rust
probe!("measure_layout", remaining_width, current_x, overflow);
// Emits: { "measure_layout.remaining_width": -12, "measure_layout.current_x": 88,
//          "measure_layout.overflow": true }
```

Implementation: A `macro_rules!` macro in `crates/runtime-core` that wraps the `read_locals`
LLDB call, applies a namespace prefix to matching variable names, and serializes to
`Vec<SemanticVariable>`. The MCP `read_locals` tool accepts an optional `probe_context` argument
to trigger semantic annotation.

**Alternatives considered**:
- **Post-hoc renaming in AI agent**: Requires the agent to know variable naming conventions per
  codebase. Not scalable; rejected.
- **Proc macro with type information**: More powerful but requires compile-time integration with
  the target binary. Deferred to Phase 2 (Runtime Semantics Layer).

---

## 5. Session State Management

### Decision: Explicit state machine in `runtime-core` with typed transitions

State enum and transitions (see `data-model.md` for full specification):

```
Idle → Launching → Running ⇆ Paused → Terminated
                    ↓                      ↑
                  Error ──────────────────┘
```

Illegal transitions (e.g., `step_over` from `Running`) return a typed `DebuggerError::InvalidState`
rather than panicking. The state machine is protected by a `tokio::sync::Mutex<SessionState>`.

---

## 6. Variable Serialization

### Decision: Recursive serialization with depth limit and cyclic reference detection

- Max depth: configurable, default 8 levels
- Cyclic refs: detected via address-based visited set; replaced with `{ "$ref": "<addr>" }`
- Large arrays: truncated at configurable limit (default 256 elements) with
  `{ "$truncated": true, "total": N }` metadata
- Large strings: truncated at 4096 bytes with `{ "$truncated": true }` marker

See `contracts/variable-serialization.md` for the full schema.

---

## 7. Platform Target

- **Windows**: Primary and only supported target.
  LLDB available via LLVM for Windows (`winget install LLVM.LLVM`).
  Python 3.x required separately (`winget install Python.Python.3`).
  Python path configured via `.cargo/config.toml` → `PYO3_PYTHON`.

---

## Summary Table

| Decision | Choice | Key Reason |
|---|---|---|
| LLDB binding | PyO3 + Python API | Minimal unsafe, ships with LLDB |
| MCP SDK | `rmcp` crate | Official SDK, async-native |
| Async bridge | Dedicated thread + mpsc | GIL safety, no blocking executor |
| Semantic probes | `probe!` macro | AI-consumable structured context |
| Session state | Explicit state machine | Typed transitions, no illegal ops |
| Variable serialization | Recursive + depth limit | Handles cyclic refs, large objects |
| Target platform | Windows | Only supported target |
