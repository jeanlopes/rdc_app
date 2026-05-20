# Quickstart: MCP + LLDB Bridge (Windows)

**Feature**: 001-mcp-lldb-bridge
**Platform**: Windows 10/11

---

## Prerequisites

- Rust stable toolchain (`rustup default stable`)
- LLVM for Windows (includes LLDB):
  ```powershell
  winget install LLVM.LLVM
  ```
- Python 3.x:
  ```powershell
  winget install Python.Python.3
  ```
- Python path configured in `.cargo\config.toml` (already done):
  ```toml
  [env]
  PYO3_PYTHON = "C:\\Python311\\python.exe"
  ```

---

## Step 1: Build

```powershell
cargo build --workspace
```

Binaries land in `target\debug\`.

---

## Step 2: Verify the debug target runs

```powershell
# Default mode — bubble sort with known values
.\target\debug\debug-target-example.exe

# Panic mode
.\target\debug\debug-target-example.exe panic
```

---

## Step 3: Launch the MCP server

```powershell
.\target\debug\mcp-server.exe `
  --executable .\target\debug\debug-target-example.exe `
  --log-level debug
```

The server reads JSON-RPC 2.0 from **stdin** and writes to **stdout**.

---

## Step 4: Connect Claude Desktop

Add to `%APPDATA%\Claude\claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "rdc": {
      "command": "C:\\workspace\\rdc_app\\target\\debug\\mcp-server.exe",
      "args": ["--executable", "C:\\workspace\\rdc_app\\target\\debug\\debug-target-example.exe"]
    }
  }
}
```

---

## Validation Checklist

### Static (confirmed by build ✅)

- [X] `mcp-server.exe` builds — `cargo build --workspace` succeeds
- [X] `debug-target-example.exe` runs — bubble sort completes, panic path fires OOB
- [X] All 13 MCP tool handlers compile and wire to LLDB backend

### Runtime (requires LLDB Python bindings — run manually)

- [ ] `launch_process` returns `state: "Running"` with a non-zero PID
- [ ] `set_breakpoint` on `debug-target-example\src\main.rs` at a `// BP` comment resolves `resolved: true`
- [ ] `continue_execution` returns `BreakpointHit` at the expected line
- [ ] `read_locals` returns `pass`, `arr`, `n` with correct values at the sort breakpoint
- [ ] `step_over` advances one source line
- [ ] `step_into` descends into `bubble_sort`
- [ ] `step_out` returns to `main`
- [ ] `evaluate_expression` on `arr[0]` returns the current first element
- [ ] `list_threads` shows at least one thread
- [ ] `read_stack` shows `bubble_sort` → `main`
- [ ] Panic mode: `continue_execution` returns `PanicDetected` with "index out of bounds"
- [ ] `read_locals` with `probe_context: "sort_pass"` returns `sort_pass.pass`, `sort_pass.swapped`

---

## Troubleshooting

**LLDB Python module not found**

Verify the `lldb` Python module is importable:
```powershell
python -c "import lldb; print(lldb.__file__)"
```

If it fails, LLVM was not installed with Python bindings. Reinstall:
```powershell
winget uninstall LLVM.LLVM
winget install LLVM.LLVM
```

Then update `.cargo\config.toml` with the correct Python path if it differs from `C:\Python311\python.exe`.

**Port already in use (HTTP mode)**

```powershell
.\target\debug\mcp-server.exe --executable ... --transport http --port 3001
```
