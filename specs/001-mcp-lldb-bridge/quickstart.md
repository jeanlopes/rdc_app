# Quickstart: MCP + LLDB Bridge

**Feature**: 001-mcp-lldb-bridge

Get the `mcp-server` running against a Rust binary in under 5 minutes.

---

## Prerequisites

- Rust stable toolchain (`rustup default stable`)
- LLDB 14+ with Python bindings installed
  - Ubuntu/Debian: `sudo apt install lldb python3-lldb`
  - macOS: ships with Xcode command-line tools (`xcode-select --install`)
- Python 3.8+ (required by the LLDB Python API)
- An AI client that supports MCP (e.g., Claude Desktop, any MCP-compatible client)

---

## Step 1: Build the workspace

```bash
cargo build --workspace
```

The `mcp-server` binary will be at `target/debug/mcp-server`.

---

## Step 2: Prepare a debug target

You need a Rust binary compiled with debug symbols:

```bash
# Example: build a target binary with debug symbols (default for `cargo build`)
cargo build --manifest-path /path/to/your/project/Cargo.toml
```

Or use the example target in this workspace (once created):
```bash
cargo build -p debug-target-example
```

---

## Step 3: Launch the MCP server

```bash
./target/debug/mcp-server \
  --executable target/debug/my_binary \
  --args "--flag value" \
  --log-level debug
```

The server starts listening on **stdio** (default transport for MCP).

For HTTP/SSE transport:
```bash
./target/debug/mcp-server \
  --executable target/debug/my_binary \
  --transport http \
  --port 3000
```

---

## Step 4: Connect an AI client

### Claude Desktop (stdio)

Add to `claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "rdc-debugger": {
      "command": "/path/to/rdc_app/target/debug/mcp-server",
      "args": ["--executable", "/path/to/your/binary"]
    }
  }
}
```

### Programmatic (MCP client)

```rust
// Using rmcp crate
let client = McpClient::connect_stdio("./target/debug/mcp-server", &["--executable", binary_path]).await?;
```

---

## Step 5: Run the basic debugging workflow

Once connected, the AI can:

**1. Launch the process**
```
Tool: launch_process
Input: { "executable": "/path/to/binary", "args": [] }
```

**2. Set a breakpoint**
```
Tool: set_breakpoint
Input: { "kind": "source_line", "file": "src/main.rs", "line": 42 }
```

**3. Continue to the breakpoint**
```
Tool: continue_execution
Input: {}
```

**4. Read local variables with semantic context**
```
Tool: read_locals
Input: { "frame_index": 0, "probe_context": "my_function" }
```

**5. Step through code**
```
Tool: step_over
Input: {}
```

---

## Validation Checklist

Run through these scenarios to verify Phase 1 acceptance criteria:

- [ ] `launch_process` starts the binary and returns state `Running`
- [ ] `set_breakpoint` on a known line resolves with `resolved: true`
- [ ] `continue_execution` stops at the breakpoint and returns `BreakpointHit`
- [ ] `read_locals` returns variables with correct types and values
- [ ] `step_over` advances by one source line
- [ ] `step_into` descends into a function call
- [ ] `step_out` returns to the call site
- [ ] `evaluate_expression` evaluates `1 + 1` and returns `2`
- [ ] `list_threads` shows at least one thread
- [ ] `read_stack` shows the current call stack with source locations
- [ ] Panic in target binary detected: `continue_execution` returns `PanicDetected` event
- [ ] `probe_context` in `read_locals` returns qualified variable names (`context.variable`)

---

## Troubleshooting

**LLDB Python module not found**

```
Error: LLDB Python bindings not found
```

Ensure `lldb` Python module is importable:
```bash
python3 -c "import lldb; print(lldb.__file__)"
```

If not found, set `LLDB_PYTHON_PATH`:
```bash
export LLDB_PYTHON_PATH=/usr/lib/python3/dist-packages
./target/debug/mcp-server ...
```

**Process fails to launch**

Check that the binary exists and has execute permissions. Ensure no existing LLDB session is
attached to the same PID.

**MCP client not receiving responses**

Ensure the client speaks MCP protocol version 2024-11-05 or later. Check server logs with
`--log-level trace` for protocol-level diagnostics.
