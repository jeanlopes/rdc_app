use pyo3::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::{info, instrument};
use runtime_core::{
    breakpoint::{Breakpoint, BreakpointId, BreakpointKind, BreakpointLocation},
    error::DebuggerError,
    process::{SourceLocation, StackFrame, ThreadId, ThreadInfo, ThreadState},
    session::{DebugTarget, SessionState, PauseReason},
    variable::{EvalResult, ScalarValue, Variable, VariableValue},
};
use protocol::tools::execution::{ExecutionEvent, ExecutionEventKind};

/// Synchronous LLDB backend using the Python API via PyO3.
///
/// All methods are called from the dedicated LLDB OS thread — never from an
/// async context. The Python GIL is held for the duration of each call.
///
/// # Safety
/// PyO3's `Python::with_gil` acquires the GIL on entry and releases it on
/// return. `PythonBackend` is `!Send` by default due to the raw `PyObject`
/// fields; it is safe to use from a single OS thread.
pub struct PythonBackend {
    /// LLDB SBDebugger instance (Python object).
    debugger: Mutex<Option<PyObject>>,
    /// LLDB SBTarget for the currently loaded executable.
    target: Mutex<Option<PyObject>>,
    /// LLDB SBProcess for the running/paused process.
    process: Mutex<Option<PyObject>>,
    /// Breakpoint index: id → local Breakpoint struct.
    breakpoints: Mutex<HashMap<BreakpointId, Breakpoint>>,
}

// SAFETY: PythonBackend is only ever used from the single LLDB OS thread.
// No Python objects are shared across threads.
unsafe impl Send for PythonBackend {}
unsafe impl Sync for PythonBackend {}

impl PythonBackend {
    /// Initialize the LLDB Python backend.
    ///
    /// Acquires the Python GIL, imports the `lldb` module, and calls
    /// `SBDebugger.Initialize()`. Returns `LLDBError` if the module is not
    /// found or initialization fails.
    pub fn new() -> Result<Self, DebuggerError> {
        Python::with_gil(|py| {
            let lldb = py.import_bound("lldb").map_err(|e| {
                DebuggerError::LLDBError(format!(
                    "failed to import lldb Python module: {}. \
                     Ensure LLDB is installed with Python bindings (apt install python3-lldb).",
                    e
                ))
            })?;
            let debugger_class = lldb.getattr("SBDebugger").map_err(|e| {
                DebuggerError::LLDBError(format!("SBDebugger not found: {}", e))
            })?;
            debugger_class
                .call_method0("Initialize")
                .map_err(|e| DebuggerError::LLDBError(format!("SBDebugger.Initialize() failed: {}", e)))?;
            info!("LLDB Python backend initialized");
            Ok(Self {
                debugger: Mutex::new(None),
                target: Mutex::new(None),
                process: Mutex::new(None),
                breakpoints: Mutex::new(HashMap::new()),
            })
        })
    }

    // ── Session ──────────────────────────────────────────────────────────────

    #[instrument(skip(self))]
    pub fn launch_process(&self, target_cfg: DebugTarget) -> Result<(u32, SessionState), DebuggerError> {
        Python::with_gil(|py| {
            let lldb = py.import_bound("lldb").map_err(|e| DebuggerError::LLDBError(e.to_string()))?;

            // Create debugger
            let debugger_class = lldb.getattr("SBDebugger").map_err(|e| DebuggerError::LLDBError(e.to_string()))?;
            let debugger = debugger_class
                .call_method1("Create", (false,))
                .map_err(|e| DebuggerError::LLDBError(format!("SBDebugger.Create: {}", e)))?;
            debugger
                .call_method1("SetAsync", (false,))
                .map_err(|e| DebuggerError::LLDBError(format!("SetAsync: {}", e)))?;

            // Create target
            let exe = target_cfg.executable.to_string_lossy().to_string();
            let sb_target = debugger
                .call_method1("CreateTargetWithFileAndArch", (&exe, ""))
                .map_err(|e| DebuggerError::LLDBError(format!("CreateTarget: {}", e)))?;

            let is_valid: bool = sb_target
                .call_method0("IsValid")
                .and_then(|v| v.extract())
                .unwrap_or(false);
            if !is_valid {
                return Err(DebuggerError::LLDBError(format!("Could not create target for '{}'", exe)));
            }

            // Launch
            let args_list: Vec<&str> = target_cfg.args.iter().map(String::as_str).collect();
            let env_list: Vec<String> = target_cfg
                .env
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            let env_refs: Vec<&str> = env_list.iter().map(String::as_str).collect();
            let cwd = target_cfg
                .working_dir
                .as_deref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            let sb_process = sb_target
                .call_method(
                    "LaunchSimple",
                    (args_list, env_refs, &cwd),
                    None,
                )
                .map_err(|e| DebuggerError::LLDBError(format!("LaunchSimple: {}", e)))?;

            let process_valid: bool = sb_process
                .call_method0("IsValid")
                .and_then(|v| v.extract())
                .unwrap_or(false);
            if !process_valid {
                return Err(DebuggerError::LLDBError("Process failed to launch".into()));
            }

            let pid: u64 = sb_process
                .call_method0("GetProcessID")
                .and_then(|v| v.extract())
                .map_err(|e| DebuggerError::LLDBError(format!("GetProcessID: {}", e)))?;

            *self.debugger.lock().unwrap() = Some(debugger.into());
            *self.target.lock().unwrap() = Some(sb_target.into());
            *self.process.lock().unwrap() = Some(sb_process.into());

            info!(pid, executable = %exe, "process launched");
            Ok((pid as u32, SessionState::Running))
        })
    }

    pub fn get_state(&self) -> Result<SessionState, DebuggerError> {
        Python::with_gil(|py| {
            let guard = self.process.lock().unwrap();
            let proc = guard.as_ref().ok_or(DebuggerError::ProcessNotFound)?;
            let sb_process = proc.bind(py);
            let state_id: u32 = sb_process
                .call_method0("GetState")
                .and_then(|v| v.extract())
                .map_err(|e| DebuggerError::LLDBError(e.to_string()))?;
            Ok(lldb_state_to_session(state_id))
        })
    }

    // ── Breakpoints ──────────────────────────────────────────────────────────

    #[instrument(skip(self))]
    pub fn set_breakpoint(&self, kind: BreakpointKind, condition: Option<String>) -> Result<Breakpoint, DebuggerError> {
        Python::with_gil(|py| {
            let guard = self.target.lock().unwrap();
            let tgt = guard.as_ref().ok_or(DebuggerError::ProcessNotFound)?;
            let sb_target = tgt.bind(py);

            let sb_bp = match &kind {
                BreakpointKind::SourceLine { file, line } => sb_target
                    .call_method1(
                        "BreakpointCreateByLocation",
                        (file.to_string_lossy().as_ref(), *line),
                    )
                    .map_err(|e| DebuggerError::LLDBError(e.to_string()))?,
                BreakpointKind::FunctionName { name } => sb_target
                    .call_method1("BreakpointCreateByName", (name.as_str(),))
                    .map_err(|e| DebuggerError::LLDBError(e.to_string()))?,
                BreakpointKind::Address { addr } => sb_target
                    .call_method1("BreakpointCreateByAddress", (*addr,))
                    .map_err(|e| DebuggerError::LLDBError(e.to_string()))?,
                BreakpointKind::Regex { pattern } => sb_target
                    .call_method1("BreakpointCreateByRegex", (pattern.as_str(),))
                    .map_err(|e| DebuggerError::LLDBError(e.to_string()))?,
            };

            if let Some(cond) = &condition {
                let _ = sb_bp.call_method1("SetCondition", (cond.as_str(),));
            }

            let id: u32 = sb_bp
                .call_method0("GetID")
                .and_then(|v| v.extract())
                .map_err(|e| DebuggerError::LLDBError(e.to_string()))?;

            let num_locs: u32 = sb_bp
                .call_method0("GetNumLocations")
                .and_then(|v| v.extract())
                .unwrap_or(0);

            let mut locations = Vec::new();
            for i in 0..num_locs {
                if let Ok(loc) = sb_bp.call_method1("GetLocationAtIndex", (i,)) {
                    let addr: u64 = loc
                        .call_method0("GetLoadAddress")
                        .and_then(|v| v.extract())
                        .unwrap_or(0);
                    let resolved: bool = loc
                        .call_method0("IsResolved")
                        .and_then(|v| v.extract())
                        .unwrap_or(false);
                    locations.push(BreakpointLocation {
                        address: addr,
                        source_location: None,
                        resolved,
                    });
                }
            }

            let bp = Breakpoint {
                id,
                kind,
                condition,
                hit_count: 0,
                enabled: true,
                locations,
            };
            self.breakpoints.lock().unwrap().insert(id, bp.clone());
            info!(breakpoint_id = id, "breakpoint set");
            Ok(bp)
        })
    }

    pub fn remove_breakpoint(&self, id: BreakpointId) -> Result<(), DebuggerError> {
        Python::with_gil(|py| {
            let guard = self.target.lock().unwrap();
            let tgt = guard.as_ref().ok_or(DebuggerError::ProcessNotFound)?;
            tgt.bind(py)
                .call_method1("BreakpointDelete", (id,))
                .map_err(|e| DebuggerError::LLDBError(e.to_string()))?;
            self.breakpoints.lock().unwrap().remove(&id);
            info!(breakpoint_id = id, "breakpoint removed");
            Ok(())
        })
    }

    pub fn list_breakpoints(&self) -> Result<Vec<Breakpoint>, DebuggerError> {
        Ok(self.breakpoints.lock().unwrap().values().cloned().collect())
    }

    // ── Execution control ────────────────────────────────────────────────────

    #[instrument(skip(self))]
    pub fn continue_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        Python::with_gil(|py| {
            let guard = self.process.lock().unwrap();
            let proc = guard.as_ref().ok_or(DebuggerError::ProcessNotFound)?;
            let sb_process = proc.bind(py);

            sb_process
                .call_method0("Continue")
                .map_err(|e| DebuggerError::LLDBError(format!("Continue: {}", e)))?;

            // Wait synchronously for next stop event
            self.wait_for_stop(py, &sb_process)
        })
    }

    pub fn pause_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        Python::with_gil(|py| {
            let guard = self.process.lock().unwrap();
            let proc = guard.as_ref().ok_or(DebuggerError::ProcessNotFound)?;
            let sb_process = proc.bind(py);
            sb_process
                .call_method0("Stop")
                .map_err(|e| DebuggerError::LLDBError(format!("Stop: {}", e)))?;
            self.wait_for_stop(py, &sb_process)
        })
    }

    pub fn step_over(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.step_thread(thread_id, "StepOver")
    }

    pub fn step_into(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.step_thread(thread_id, "StepInto")
    }

    pub fn step_out(&self, thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.step_thread(thread_id, "StepOut")
    }

    fn step_thread(&self, thread_id: Option<ThreadId>, method: &str) -> Result<ExecutionEvent, DebuggerError> {
        Python::with_gil(|py| {
            let proc_guard = self.process.lock().unwrap();
            let proc = proc_guard.as_ref().ok_or(DebuggerError::ProcessNotFound)?;
            let sb_process = proc.bind(py);
            let thread = self.get_thread(py, sb_process, thread_id)?;
            thread
                .call_method0(method)
                .map_err(|e| DebuggerError::LLDBError(format!("{}: {}", method, e)))?;
            // Wait for the step to complete
            let proc2 = proc.bind(py);
            self.wait_for_stop(py, &proc2)
        })
    }

    // ── Inspection ───────────────────────────────────────────────────────────

    #[instrument(skip(self))]
    pub fn read_locals(
        &self,
        thread_id: Option<ThreadId>,
        frame_index: u32,
        probe_context: Option<String>,
        max_depth: u32,
    ) -> Result<Vec<Variable>, DebuggerError> {
        Python::with_gil(|py| {
            let proc_guard = self.process.lock().unwrap();
            let proc = proc_guard.as_ref().ok_or(DebuggerError::ProcessNotFound)?;
            let sb_process = proc.bind(py);
            let thread = self.get_thread(py, sb_process, thread_id)?;
            let frame = thread
                .call_method1("GetFrameAtIndex", (frame_index,))
                .map_err(|e| DebuggerError::LLDBError(e.to_string()))?;

            let variables = frame
                .call_method0("GetVariables")
                .or_else(|_| {
                    // fallback: GetAllVariables(args, locals, statics, in_scope)
                    frame.call_method1("GetVariables", (false, true, false, true))
                })
                .map_err(|e| DebuggerError::LLDBError(format!("GetVariables: {}", e)))?;

            let num: usize = variables
                .call_method0("GetSize")
                .and_then(|v| v.extract())
                .unwrap_or(0);

            let mut result = Vec::with_capacity(num);
            for i in 0..num {
                if let Ok(sb_val) = variables.call_method1("GetValueAtIndex", (i,)) {
                    let var = sb_value_to_variable(py, &sb_val, probe_context.as_deref(), max_depth, 0);
                    result.push(var);
                }
            }
            Ok(result)
        })
    }

    #[instrument(skip(self))]
    pub fn read_stack(&self, thread_id: Option<ThreadId>, max_frames: u32) -> Result<Vec<StackFrame>, DebuggerError> {
        Python::with_gil(|py| {
            let proc_guard = self.process.lock().unwrap();
            let proc = proc_guard.as_ref().ok_or(DebuggerError::ProcessNotFound)?;
            let sb_process = proc.bind(py);
            let thread = self.get_thread(py, sb_process, thread_id)?;
            let num_frames: u32 = thread
                .call_method0("GetNumFrames")
                .and_then(|v| v.extract())
                .unwrap_or(0);
            let count = num_frames.min(max_frames);
            let mut frames = Vec::with_capacity(count as usize);
            for i in 0..count {
                if let Ok(frame) = thread.call_method1("GetFrameAtIndex", (i,)) {
                    frames.push(sb_frame_to_stack_frame(py, &frame, i));
                }
            }
            Ok(frames)
        })
    }

    #[instrument(skip(self, expression))]
    pub fn evaluate_expression(
        &self,
        expression: String,
        thread_id: Option<ThreadId>,
        frame_index: u32,
    ) -> Result<EvalResult, DebuggerError> {
        Python::with_gil(|py| {
            let proc_guard = self.process.lock().unwrap();
            let proc = proc_guard.as_ref().ok_or(DebuggerError::ProcessNotFound)?;
            let sb_process = proc.bind(py);
            let thread = self.get_thread(py, sb_process, thread_id)?;
            let frame = thread
                .call_method1("GetFrameAtIndex", (frame_index,))
                .map_err(|e| DebuggerError::LLDBError(e.to_string()))?;
            let result_val = frame
                .call_method1("EvaluateExpression", (expression.as_str(),))
                .map_err(|e| DebuggerError::LLDBError(format!("EvaluateExpression: {}", e)))?;

            let error_obj = result_val.call_method0("GetError").ok();
            let has_error = error_obj
                .as_ref()
                .and_then(|e| e.call_method0("Fail").ok())
                .and_then(|v| v.extract::<bool>().ok())
                .unwrap_or(false);

            let error_msg = if has_error {
                error_obj
                    .and_then(|e| e.call_method0("GetCString").ok())
                    .and_then(|v| v.extract::<String>().ok())
            } else {
                None
            };

            let type_name: String = result_val
                .call_method0("GetTypeName")
                .and_then(|v| v.extract())
                .unwrap_or_else(|_| "unknown".to_string());

            let value = sb_value_to_variable_value(py, &result_val, 0, 4);

            Ok(EvalResult {
                expression,
                value,
                type_name,
                error: error_msg,
            })
        })
    }

    pub fn list_threads(&self) -> Result<Vec<ThreadInfo>, DebuggerError> {
        Python::with_gil(|py| {
            let guard = self.process.lock().unwrap();
            let proc = guard.as_ref().ok_or(DebuggerError::ProcessNotFound)?;
            let sb_process = proc.bind(py);
            let num: u32 = sb_process
                .call_method0("GetNumThreads")
                .and_then(|v| v.extract())
                .unwrap_or(0);
            let mut threads = Vec::with_capacity(num as usize);
            for i in 0..num {
                if let Ok(t) = sb_process.call_method1("GetThreadAtIndex", (i,)) {
                    let id: u64 = t.call_method0("GetThreadID").and_then(|v| v.extract()).unwrap_or(0);
                    let name: Option<String> = t.call_method0("GetName").and_then(|v| v.extract()).ok();
                    let frame_count: usize = t.call_method0("GetNumFrames").and_then(|v| v.extract()).unwrap_or(0);
                    threads.push(ThreadInfo {
                        id,
                        name,
                        state: ThreadState::Stopped,
                        stop_reason: None,
                        frame_count,
                    });
                }
            }
            Ok(threads)
        })
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn get_thread<'py>(
        &self,
        py: Python<'py>,
        sb_process: &Bound<'py, PyAny>,
        thread_id: Option<ThreadId>,
    ) -> Result<Bound<'py, PyAny>, DebuggerError> {
        match thread_id {
            Some(id) => sb_process
                .call_method1("GetThreadByID", (id,))
                .map_err(|_| DebuggerError::ThreadNotFound(id)),
            None => sb_process
                .call_method0("GetSelectedThread")
                .map_err(|e| DebuggerError::LLDBError(e.to_string())),
        }
    }

    #[allow(unused_variables)]
    fn wait_for_stop<'py>(
        &self,
        py: Python<'py>,
        sb_process: &Bound<'py, PyAny>,
    ) -> Result<ExecutionEvent, DebuggerError> {
        // Poll state until stopped (synchronous mode; LLDB already stopped before returning from step/continue)
        let state_id: u32 = sb_process
            .call_method0("GetState")
            .and_then(|v| v.extract())
            .unwrap_or(0);

        let thread = sb_process
            .call_method0("GetSelectedThread")
            .map_err(|e| DebuggerError::LLDBError(e.to_string()))?;

        let thread_id: u64 = thread
            .call_method0("GetThreadID")
            .and_then(|v| v.extract())
            .unwrap_or(0);

        let stop_reason_id: u32 = thread
            .call_method0("GetStopReason")
            .and_then(|v| v.extract())
            .unwrap_or(0);

        let location = get_thread_location(py, &thread);

        // Map stop reason ID to ExecutionEventKind
        // LLDB stop reason constants: eStopReasonBreakpoint=3, eStopReasonPlanComplete=12,
        //   eStopReasonSignal=5, eStopReasonExec=8
        let kind = match stop_reason_id {
            3 => ExecutionEventKind::BreakpointHit,
            12 => ExecutionEventKind::StepComplete,
            5 => {
                // Check for panic signal (SIGABRT = 6)
                let signal_num: u64 = thread
                    .call_method1("GetStopReasonDataAtIndex", (0u32,))
                    .and_then(|v| v.extract())
                    .unwrap_or(0);
                if signal_num == 6 {
                    let desc: String = thread
                        .call_method0("GetStopDescription")
                        .and_then(|v| v.extract())
                        .unwrap_or_else(|_| "panic".to_string());
                    ExecutionEventKind::PanicDetected { message: desc }
                } else {
                    ExecutionEventKind::Paused
                }
            }
            _ => {
                // Check for process exit
                if state_id == 10 {
                    // eStateStopped=5, eStateExited=10
                    let exit_code: i32 = sb_process
                        .call_method0("GetExitStatus")
                        .and_then(|v| v.extract())
                        .unwrap_or(0);
                    ExecutionEventKind::Terminated { exit_code }
                } else {
                    ExecutionEventKind::Paused
                }
            }
        };

        Ok(ExecutionEvent { kind, thread_id, location })
    }
}

// ── Free helper functions ─────────────────────────────────────────────────────

fn lldb_state_to_session(state_id: u32) -> SessionState {
    match state_id {
        1 => SessionState::Launching,     // eStateAttaching
        3 => SessionState::Launching,     // eStateLaunching
        5 => SessionState::Paused(PauseReason::UserRequest), // eStateStopped
        6 => SessionState::Running,       // eStateRunning
        7 => SessionState::Stepping,      // eStateStepping
        10 => SessionState::Terminated(0), // eStateExited
        _ => SessionState::Idle,
    }
}

fn get_thread_location<'py>(_py: Python<'py>, thread: &Bound<'py, PyAny>) -> Option<SourceLocation> {
    let frame = thread.call_method1("GetFrameAtIndex", (0u32,)).ok()?;
    let line_entry = frame.call_method0("GetLineEntry").ok()?;
    let file_spec = line_entry.call_method0("GetFileSpec").ok()?;
    let dir: String = file_spec.call_method0("GetDirectory").and_then(|v| v.extract()).ok()?;
    let filename: String = file_spec.call_method0("GetFilename").and_then(|v| v.extract()).ok()?;
    let line: u32 = line_entry.call_method0("GetLine").and_then(|v| v.extract()).ok()?;
    let col: Option<u32> = line_entry.call_method0("GetColumn").and_then(|v| v.extract()).ok();
    Some(SourceLocation {
        file: PathBuf::from(dir).join(filename),
        line,
        column: col,
    })
}

fn sb_frame_to_stack_frame<'py>(_py: Python<'py>, frame: &Bound<'py, PyAny>, index: u32) -> StackFrame {
    let function_name: Option<String> = frame.call_method0("GetFunctionName").and_then(|v| v.extract()).ok();
    let module: Option<String> = frame
        .call_method0("GetModule")
        .ok()
        .and_then(|m| m.call_method0("GetFileSpec").ok())
        .and_then(|fs| fs.call_method0("GetFilename").ok())
        .and_then(|v| v.extract().ok());
    let source_location: Option<SourceLocation> = (|| -> Option<SourceLocation> {
        let le = frame.call_method0("GetLineEntry").ok()?;
        let fs = le.call_method0("GetFileSpec").ok()?;
        let dir: String = fs.call_method0("GetDirectory").and_then(|v| v.extract()).ok()?;
        let name: String = fs.call_method0("GetFilename").and_then(|v| v.extract()).ok()?;
        let line: u32 = le.call_method0("GetLine").and_then(|v| v.extract()).ok()?;
        Some(SourceLocation { file: PathBuf::from(dir).join(name), line, column: None })
    })();
    let is_inlined: bool = frame.call_method0("IsInlined").and_then(|v| v.extract()).unwrap_or(false);
    StackFrame { index, function_name, module, source_location, is_inlined }
}

fn sb_value_to_variable<'py>(
    py: Python<'py>,
    sb_val: &Bound<'py, PyAny>,
    probe_context: Option<&str>,
    max_depth: u32,
    depth: u32,
) -> Variable {
    let name: String = sb_val.call_method0("GetName").and_then(|v| v.extract()).unwrap_or_default();
    let type_name: String = sb_val.call_method0("GetTypeName").and_then(|v| v.extract()).unwrap_or_default();
    let address: Option<u64> = sb_val.call_method0("GetLoadAddress").and_then(|v| v.extract()).ok();
    let value = sb_value_to_variable_value(py, sb_val, depth, max_depth);

    let semantic = probe_context.map(|ctx| runtime_core::variable::SemanticAnnotation {
        context: ctx.to_string(),
        qualified_name: format!("{}.{}", ctx, name),
        description: None,
    });

    Variable { name, type_name, value, address, semantic }
}

fn sb_value_to_variable_value<'py>(
    py: Python<'py>,
    sb_val: &Bound<'py, PyAny>,
    depth: u32,
    max_depth: u32,
) -> VariableValue {
    if depth >= max_depth {
        let summary: String = sb_val.call_method0("GetSummary").and_then(|v| v.extract())
            .or_else(|_| sb_val.call_method0("GetValue").and_then(|v| v.extract()))
            .unwrap_or_else(|_| "<depth limit>".to_string());
        return VariableValue::Opaque { summary };
    }

    let num_children: u32 = sb_val.call_method0("GetNumChildren").and_then(|v| v.extract()).unwrap_or(0);

    if num_children > 0 {
        let fields: Vec<Variable> = (0..num_children.min(256))
            .filter_map(|i| sb_val.call_method1("GetChildAtIndex", (i,)).ok())
            .map(|child| sb_value_to_variable(py, &child, None, max_depth, depth + 1))
            .collect();
        return VariableValue::Struct { fields };
    }

    let raw_value: Option<String> = sb_val.call_method0("GetValue").and_then(|v| v.extract()).ok();

    // Try to parse as common scalar types
    if let Some(ref v) = raw_value {
        if v == "true" { return VariableValue::Scalar(ScalarValue::Bool(true)); }
        if v == "false" { return VariableValue::Scalar(ScalarValue::Bool(false)); }
        if let Ok(n) = v.parse::<i128>() { return VariableValue::Scalar(ScalarValue::Int(n)); }
        if let Ok(n) = v.parse::<f64>() { return VariableValue::Scalar(ScalarValue::Float(n)); }
    }

    let summary: String = raw_value
        .or_else(|| sb_val.call_method0("GetSummary").and_then(|v| v.extract()).ok())
        .unwrap_or_else(|| "<unknown>".to_string());
    VariableValue::Opaque { summary }
}
