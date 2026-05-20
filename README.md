# RDC — Runtime Debugger Console

> An AI-native platform for runtime introspection and autonomous debugging.

RDC is **not** an IDE plugin. It is **not** a debugger wrapper.

It is a **Runtime Intelligence System** — a platform where an AI agent observes program
execution, understands state, correlates runtime behavior with source code, formulates
hypotheses, and executes autonomous debugging loops.

```
AI Agent
   ↓
MCP Protocol Layer
   ↓
Runtime Rust Debugger
   ├── LLDB Integration
   ├── Runtime Event Stream
   ├── UI Introspection (egui)
   ├── Deterministic Replay
   ├── Semantic Runtime Graph
   └── Autonomous Debug Engine
```

---

## Why RDC?

Traditional debuggers are **passive tools**. You set a breakpoint, you inspect variables, you
think about what it means. The debugger does nothing unless you ask.

RDC flips this model. The AI:

- **Observes** execution continuously
- **Understands** program state semantically (not raw `{ "x": 88 }` but
  `{ "measure_layout.remaining_width": -12, "overflow_detected": true }`)
- **Correlates** runtime events with source code
- **Formulates** falsifiable hypotheses (`"overflow occurs after justification pass"`)
- **Tests** hypotheses by controlling execution
- **Validates** patches by re-running

---

## Architecture

### Workspace Layout

```
apps/
├── desktop-ui/         # egui desktop application
├── mcp-server/         # MCP protocol server (AI gateway)
├── replay-engine/      # Deterministic trace replay
└── autonomous-agent/   # AI-driven debug orchestrator

crates/
├── runtime-core/       # Core abstractions: session, entities, state machine
├── lldb-bridge/        # LLDB integration (PyO3 + Python API)
├── event-stream/       # Runtime event capture and dispatch
├── semantic-runtime/   # Semantic graph over runtime state
├── egui-introspection/ # UI introspection widgets
├── replay-core/        # Replay primitives and execution engine
├── trace-storage/      # Trace persistence and querying
├── test-automation/    # Automated test execution harness
├── ai-planning/        # Hypothesis formulation and planning
└── protocol/           # MCP protocol types and codec
```

### How It Works

```
┌─────────────────────────────────────┐
│  AI Agent (Claude, GPT, etc.)        │
│  "Set breakpoint at layout.rs:412"  │
└───────────────┬─────────────────────┘
                │ MCP tools (JSON-RPC)
                ▼
┌─────────────────────────────────────┐
│  apps/mcp-server                     │
│  13 tools: set_breakpoint,           │
│  read_locals, continue_execution...  │
└───────────────┬─────────────────────┘
                │ LLDBCommand (mpsc channel)
                ▼
┌─────────────────────────────────────┐
│  crates/lldb-bridge (dedicated OS   │
│  thread, PyO3 + LLDB Python API)     │
└───────────────┬─────────────────────┘
                │
                ▼
         Target binary
         (running under LLDB)
```

---

## Semantic Probes — The Key Innovation

Standard debuggers give you raw values. RDC gives you **meaning**.

**Without RDC:**
```json
{ "x": 88, "w": -12, "flag": true }
```

**With RDC semantic probes:**
```json
{
  "measure_layout.current_x": 88,
  "measure_layout.remaining_width": -12,
  "measure_layout.overflow_detected": true
}
```

You define probes in your code:
```rust
probe!("measure_layout", current_x, remaining_width, overflow_detected);
```

The AI now understands that `remaining_width` is negative — and that this means an overflow
condition — without needing to know your codebase.

---

## Roadmap

| Phase | Feature | Status |
|-------|---------|--------|
| 1 | MCP + LLDB Bridge — AI↔debugger foundation | 🔧 In progress |
| 2 | Runtime Event Stream — live observable execution | 📋 Planned |
| 3 | egui UI Introspection — semantic widget tree | 📋 Planned |
| 4 | Deterministic Replay — time-travel debugging | 📋 Planned |
| 5 | Autonomous Debug Agent — self-driving investigation | 📋 Planned |

### Phase 1 (current): MCP + LLDB Bridge

Delivers 13 MCP tools over the LLDB Python API:

| Tool | Description |
|------|-------------|
| `launch_process` | Start a binary under LLDB |
| `set_breakpoint` | Set source-line or function breakpoint |
| `remove_breakpoint` | Remove by ID |
| `continue_execution` | Resume; returns next stop event |
| `pause_execution` | Interrupt a running process |
| `step_over` | Advance one source line |
| `step_into` | Descend into a function call |
| `step_out` | Return to call site |
| `read_locals` | Read frame locals (with semantic probe context) |
| `read_stack` | Read the full call stack |
| `evaluate_expression` | Evaluate a Rust expression in frame context |
| `list_threads` | List all threads with stop reasons |
| `get_session_state` | Query current session state |

---

## Getting Started

### Prerequisites

- Rust stable toolchain (`rustup default stable`)
- LLDB 14+ with Python bindings
  - Ubuntu/Debian: `sudo apt install lldb python3-lldb`
  - macOS: `xcode-select --install`
- An MCP-compatible AI client (e.g., Claude Desktop)

### Build

```bash
cargo build --workspace
```

### Run

```bash
./target/debug/mcp-server --executable ./path/to/your/binary
```

### Connect to Claude Desktop

Add to `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "rdc": {
      "command": "/path/to/rdc_app/target/debug/mcp-server",
      "args": ["--executable", "/path/to/your/binary"]
    }
  }
}
```

See [`specs/001-mcp-lldb-bridge/quickstart.md`](specs/001-mcp-lldb-bridge/quickstart.md) for
a full validation walkthrough.

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust (stable, MSRV 1.75) |
| Debugger | LLDB via PyO3 + Python API |
| AI Protocol | MCP (Model Context Protocol) via `rmcp` |
| Async Runtime | Tokio |
| UI | egui |
| Serialization | serde + serde_json |
| Observability | tracing |

---

## Project Governance

This project follows a **spec-driven development** workflow:

```
/speckit-specify → /speckit-clarify → /speckit-plan → /speckit-tasks → /speckit-implement
```

Design decisions and non-negotiable architectural rules are encoded in the
[Project Constitution](.specify/memory/constitution.md) (7 principles, ratified 2026-05-20).

Key principles:
1. **Runtime Intelligence** — the AI observes and acts; no passive wrapping
2. **Crate-First Modularity** — every capability is a standalone, testable crate
3. **MCP as the AI-Debugger Contract** — no direct AI↔LLDB coupling, ever
4. **Deterministic Replay** — all traces are reproducible
5. **Autonomous Agent Discipline** — every AI conclusion needs execution evidence
6. **Rust Safety First** — `unsafe` requires documented proof
7. **Open Platform Foundation** — stable public APIs, documented, versioned

---

## License

MIT
