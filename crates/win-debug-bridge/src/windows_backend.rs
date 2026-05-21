//! Windows Debug API backend — full implementation.
//!
//! # Safety policy (Constitution Principle VI)
//! Every `unsafe` block in this file calls a Win32 API. Each site carries:
//!   1. Safety proof comment
//!   2. GitHub issue reference (placeholder: #1)
//!   3. Rejected safe alternative

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::{info, instrument, warn};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::{
    ContinueDebugEvent, DebugBreakProcess, GetThreadContext, ReadProcessMemory,
    SetThreadContext, WaitForDebugEvent, WriteProcessMemory,
    CONTEXT, CONTEXT_FLAGS, DEBUG_EVENT, EXCEPTION_DEBUG_EVENT, EXIT_PROCESS_DEBUG_EVENT,
    CREATE_PROCESS_DEBUG_EVENT,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, THREADENTRY32, TH32CS_SNAPTHREAD,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessA, DEBUG_ONLY_THIS_PROCESS, DEBUG_PROCESS, PROCESS_INFORMATION,
    STARTF_USESTDHANDLES, STARTUPINFOA,
};
use windows::core::PCSTR;

use runtime_core::{
    breakpoint::{Breakpoint, BreakpointId, BreakpointKind, BreakpointLocation},
    error::DebuggerError,
    process::{StackFrame, ThreadId, ThreadInfo, ThreadState},
    session::{DebugTarget, PauseReason, SessionState},
    variable::{EvalResult, ScalarValue, Variable, VariableValue, SemanticAnnotation},
};
use protocol::tools::execution::{ExecutionEvent, ExecutionEventKind};
use crate::pdb_info::{PdbInfo, VarLocation};

const INT3: u8 = 0xCC;

// AMD64 CONTEXT flags (typed as CONTEXT_FLAGS for windows-rs compatibility)
const CONTEXT_FULL: CONTEXT_FLAGS = CONTEXT_FLAGS(0x0010_003F);

// EFLAGS Trap Flag (bit 8) — causes EXCEPTION_SINGLE_STEP after next instruction
const TRAP_FLAG: u32 = 0x0100;

struct PatchedBreakpoint {
    address: u64,
    original_byte: u8,
    bp: Breakpoint,
}

/// Windows Debug API backend.
pub struct WindowsDebugBackend {
    process: Mutex<Option<HANDLE>>,
    pid: Mutex<Option<u32>>,
    state: Mutex<SessionState>,
    breakpoints: Mutex<HashMap<BreakpointId, PatchedBreakpoint>>,
    next_bp_id: Mutex<u32>,
    /// Thread ID of the currently stopped thread.
    stopped_tid: Mutex<u32>,
    /// PDB symbol info (loaded after process starts).
    pdb: Mutex<Option<PdbInfo>>,
    /// Path to the executable.
    exe_path: Mutex<Option<PathBuf>>,
    /// Read end of stderr pipe (for panic message capture).
    stderr_read: Mutex<Option<HANDLE>>,
}

// SAFETY: Used from the single debug OS thread only.
unsafe impl Send for WindowsDebugBackend {}
unsafe impl Sync for WindowsDebugBackend {}

impl WindowsDebugBackend {
    pub fn new() -> Result<Self, DebuggerError> {
        Ok(Self {
            process: Mutex::new(None),
            pid: Mutex::new(None),
            state: Mutex::new(SessionState::Idle),
            breakpoints: Mutex::new(HashMap::new()),
            next_bp_id: Mutex::new(1),
            stopped_tid: Mutex::new(0),
            pdb: Mutex::new(None),
            exe_path: Mutex::new(None),
            stderr_read: Mutex::new(None),
        })
    }

    // ── Session ──────────────────────────────────────────────────────────────

    #[instrument(skip(self))]
    pub fn launch_process(&self, target: DebugTarget) -> Result<(u32, SessionState), DebuggerError> {
        let exe = target.executable.clone();
        let exe_str = exe.to_string_lossy().to_string();

        // Create anonymous pipe for stderr capture (panic message extraction).
        let mut stderr_read = HANDLE::default();
        let mut stderr_write = HANDLE::default();
        // SAFETY: CreatePipe with NULL security attributes creates an inheritable pipe.
        // Both handles must be closed. stderr_write is passed to child (inheritable);
        // we close it in the parent immediately after CreateProcess.
        // GitHub issue: #1 — Safe alternative: none.
        unsafe {
            CreatePipe(&mut stderr_read, &mut stderr_write, None, 0)
                .map_err(|e| DebuggerError::DebuggerError(format!("CreatePipe: {}", e)))?;
        }

        // Build command line (null-terminated).
        let mut cmdline = exe_str.clone();
        if !target.args.is_empty() {
            cmdline.push(' ');
            cmdline.push_str(&target.args.join(" "));
        }
        let mut cmdline_bytes: Vec<u8> = cmdline.into_bytes();
        cmdline_bytes.push(0);

        let si = STARTUPINFOA {
            cb: std::mem::size_of::<STARTUPINFOA>() as u32,
            dwFlags: STARTF_USESTDHANDLES,
            hStdInput: HANDLE::default(),
            hStdOutput: HANDLE::default(),
            hStdError: stderr_write,
            ..Default::default()
        };
        let mut pi = PROCESS_INFORMATION::default();

        // SAFETY: CreateProcessA with DEBUG_PROCESS. bInheritHandles = true so stderr
        // pipe is inherited. cmdline_bytes is null-terminated and valid.
        // GitHub issue: #1 — Safe alternative: std::process::Command lacks DEBUG_PROCESS.
        let result = unsafe {
            CreateProcessA(
                PCSTR::null(),
                windows::core::PSTR(cmdline_bytes.as_mut_ptr()),
                None,
                None,
                true,
                DEBUG_PROCESS | DEBUG_ONLY_THIS_PROCESS,
                None,
                PCSTR::null(),
                &si,
                &mut pi,
            )
        };

        // Close the write end in the parent — child holds it now.
        // SAFETY: CloseHandle releases our copy of the write handle.
        // GitHub issue: #1
        unsafe { CloseHandle(stderr_write).ok(); }

        result.map_err(|e| DebuggerError::DebuggerError(format!("CreateProcessA: {}", e)))?;

        let pid = pi.dwProcessId;
        *self.process.lock().unwrap() = Some(pi.hProcess);
        *self.pid.lock().unwrap() = Some(pid);
        *self.stderr_read.lock().unwrap() = Some(stderr_read);
        *self.exe_path.lock().unwrap() = Some(exe.clone());
        *self.state.lock().unwrap() = SessionState::Running;

        // Drain initial events and capture image base, then load PDB.
        let image_base = self.drain_initial_events(pid)?;
        match PdbInfo::load(&exe, image_base) {
            Ok(pdb_info) => {
                info!("PDB loaded successfully");
                *self.pdb.lock().unwrap() = Some(pdb_info);
            }
            Err(e) => {
                warn!("PDB load failed (source-level debugging unavailable): {}", e);
            }
        }

        info!(pid, executable = %exe_str, "process launched");
        Ok((pid, SessionState::Running))
    }

    pub fn get_state(&self) -> Result<SessionState, DebuggerError> {
        Ok(self.state.lock().unwrap().clone())
    }

    // ── Breakpoints ──────────────────────────────────────────────────────────

    #[instrument(skip(self))]
    pub fn set_breakpoint(&self, kind: BreakpointKind, condition: Option<String>) -> Result<Breakpoint, DebuggerError> {
        let address = self.resolve_address(&kind)?;
        let process = self.require_process()?;

        let original_byte = self.read_byte(process, address)?;
        self.write_byte(process, address, INT3)?;

        let id = { let mut g = self.next_bp_id.lock().unwrap(); let id = *g; *g += 1; id };

        let source_location = self.pdb.lock().unwrap()
            .as_ref()
            .and_then(|p| p.va_to_source(address));

        let bp = Breakpoint {
            id,
            kind,
            condition,
            hit_count: 0,
            enabled: true,
            locations: vec![BreakpointLocation { address, source_location, resolved: true }],
        };
        self.breakpoints.lock().unwrap().insert(id, PatchedBreakpoint { address, original_byte, bp: bp.clone() });
        info!(bp_id = id, address = %format!("0x{:x}", address), "breakpoint set");
        Ok(bp)
    }

    pub fn remove_breakpoint(&self, id: BreakpointId) -> Result<(), DebuggerError> {
        let patched = self.breakpoints.lock().unwrap().remove(&id)
            .ok_or(DebuggerError::BreakpointNotFound(id))?;
        let process = self.require_process()?;
        self.write_byte(process, patched.address, patched.original_byte)?;
        info!(bp_id = id, "breakpoint removed");
        Ok(())
    }

    pub fn list_breakpoints(&self) -> Result<Vec<Breakpoint>, DebuggerError> {
        Ok(self.breakpoints.lock().unwrap().values().map(|p| p.bp.clone()).collect())
    }

    // ── Execution control ────────────────────────────────────────────────────

    pub fn continue_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        self.event_loop()
    }

    pub fn pause_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        let process = self.require_process()?;
        // SAFETY: DebugBreakProcess injects a software BP into the target.
        // GitHub issue: #1 — Safe alternative: none.
        unsafe {
            DebugBreakProcess(process)
                .map_err(|e| DebuggerError::DebuggerError(format!("DebugBreakProcess: {}", e)))?;
        }
        self.event_loop()
    }

    pub fn step_over(&self, _thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.single_step()
    }

    pub fn step_into(&self, _thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        // step_into = single step (TF will descend into function calls naturally)
        self.single_step()
    }

    pub fn step_out(&self, _thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        // Read return address from RSP, set a temp BP there, then continue.
        let ret_addr = self.read_return_address()?;
        let process = self.require_process()?;
        let original = self.read_byte(process, ret_addr)?;
        self.write_byte(process, ret_addr, INT3)?;

        // Mark it as a one-shot internal breakpoint (id = u32::MAX)
        let temp_id = u32::MAX;
        let temp_bp = Breakpoint {
            id: temp_id,
            kind: BreakpointKind::Address { addr: ret_addr },
            condition: None,
            hit_count: 0,
            enabled: true,
            locations: vec![],
        };
        self.breakpoints.lock().unwrap().insert(temp_id, PatchedBreakpoint {
            address: ret_addr,
            original_byte: original,
            bp: temp_bp,
        });

        let event = self.event_loop()?;
        // Remove the temp BP regardless of what we hit
        self.breakpoints.lock().unwrap().remove(&temp_id);
        Ok(event)
    }

    // ── Inspection ───────────────────────────────────────────────────────────

    pub fn read_locals(
        &self,
        _thread_id: Option<ThreadId>,
        _frame_index: u32,
        probe_context: Option<String>,
        _max_depth: u32,
    ) -> Result<Vec<Variable>, DebuggerError> {
        let rip = self.get_rip()?;
        let pdb_guard = self.pdb.lock().unwrap();
        let pdb = match pdb_guard.as_ref() {
            Some(p) => p,
            None => return Ok(vec![]),
        };

        let local_defs = pdb.locals_at_va(rip);
        if local_defs.is_empty() { return Ok(vec![]); }

        let rbp = self.get_rbp()?;
        let process = self.require_process()?;

        let mut result = Vec::new();
        for local in &local_defs {
            let var = self.read_local_var(process, local, rbp, probe_context.as_deref())?;
            result.push(var);
        }
        Ok(result)
    }

    pub fn read_stack(&self, _thread_id: Option<ThreadId>, max_frames: u32) -> Result<Vec<StackFrame>, DebuggerError> {
        let rip = self.get_rip()?;
        let pdb_guard = self.pdb.lock().unwrap();

        let (function_name, source_location) = if let Some(pdb) = pdb_guard.as_ref() {
            (pdb.va_to_function_name(rip), pdb.va_to_source(rip))
        } else {
            (None, None)
        };

        let frame0 = StackFrame {
            index: 0,
            function_name,
            module: None,
            source_location,
            is_inlined: false,
        };

        // Phase 1: single frame. Full stack walking (StackWalk64) is next PR.
        let _ = max_frames;
        Ok(vec![frame0])
    }

    pub fn evaluate_expression(
        &self,
        expression: String,
        _thread_id: Option<ThreadId>,
        _frame_index: u32,
    ) -> Result<EvalResult, DebuggerError> {
        // Phase 1: handle arr[N] and simple variable names.
        let value = self.eval_expression(&expression);
        Ok(EvalResult { expression, value, type_name: "unknown".into(), error: None })
    }

    pub fn list_threads(&self) -> Result<Vec<ThreadInfo>, DebuggerError> {
        let pid = self.require_pid()?;
        self.enumerate_threads(pid)
    }

    // ── Private: Win32 primitives ─────────────────────────────────────────────

    fn require_process(&self) -> Result<HANDLE, DebuggerError> {
        self.process.lock().unwrap().ok_or(DebuggerError::ProcessNotFound)
    }

    fn require_pid(&self) -> Result<u32, DebuggerError> {
        self.pid.lock().unwrap().ok_or(DebuggerError::ProcessNotFound)
    }

    fn read_byte(&self, process: HANDLE, address: u64) -> Result<u8, DebuggerError> {
        let mut byte: u8 = 0;
        let mut read = 0usize;
        // SAFETY: ReadProcessMemory, 1 byte. GitHub issue: #1.
        unsafe {
            ReadProcessMemory(process, address as *const _, &mut byte as *mut u8 as *mut _, 1, Some(&mut read))
                .map_err(|e| DebuggerError::DebuggerError(format!("ReadProcessMemory: {}", e)))?;
        }
        Ok(byte)
    }

    fn write_byte(&self, process: HANDLE, address: u64, byte: u8) -> Result<(), DebuggerError> {
        let mut written = 0usize;
        // SAFETY: WriteProcessMemory, 1 byte. GitHub issue: #1.
        unsafe {
            WriteProcessMemory(process, address as *const _, &byte as *const u8 as *const _, 1, Some(&mut written))
                .map_err(|e| DebuggerError::DebuggerError(format!("WriteProcessMemory: {}", e)))?;
        }
        Ok(())
    }

    fn read_bytes(&self, process: HANDLE, address: u64, size: usize) -> Result<Vec<u8>, DebuggerError> {
        let mut buf = vec![0u8; size];
        let mut read = 0usize;
        // SAFETY: ReadProcessMemory into heap buffer. GitHub issue: #1.
        unsafe {
            ReadProcessMemory(process, address as *const _, buf.as_mut_ptr() as *mut _, size, Some(&mut read))
                .map_err(|e| DebuggerError::DebuggerError(format!("ReadProcessMemory({} bytes): {}", size, e)))?;
        }
        buf.truncate(read);
        Ok(buf)
    }

    fn get_thread_context(&self, tid: u32) -> Result<CONTEXT, DebuggerError> {
        use windows::Win32::System::Threading::{OpenThread, THREAD_GET_CONTEXT, THREAD_SUSPEND_RESUME};
        // SAFETY: OpenThread returns a handle to the stopped thread.
        // GitHub issue: #1 — Safe alternative: none.
        let thread_handle = unsafe {
            OpenThread(THREAD_GET_CONTEXT | THREAD_SUSPEND_RESUME, false, tid)
                .map_err(|e| DebuggerError::DebuggerError(format!("OpenThread: {}", e)))?
        };

        let mut ctx = CONTEXT::default();
        ctx.ContextFlags = CONTEXT_FULL;

        // SAFETY: GetThreadContext requires the thread to be in a suspended/debug state.
        // The debug thread is halted at a debug event, which satisfies this.
        // GitHub issue: #1 — Safe alternative: none.
        let result = unsafe { GetThreadContext(thread_handle, &mut ctx) };
        unsafe { CloseHandle(thread_handle).ok(); }
        result.map_err(|e| DebuggerError::DebuggerError(format!("GetThreadContext: {}", e)))?;
        Ok(ctx)
    }

    fn set_thread_context(&self, tid: u32, ctx: &CONTEXT) -> Result<(), DebuggerError> {
        use windows::Win32::System::Threading::{OpenThread, THREAD_SET_CONTEXT, THREAD_SUSPEND_RESUME};
        // SAFETY: OpenThread + SetThreadContext. Thread is in debug-stopped state.
        // GitHub issue: #1 — Safe alternative: none.
        let thread_handle = unsafe {
            OpenThread(THREAD_SET_CONTEXT | THREAD_SUSPEND_RESUME, false, tid)
                .map_err(|e| DebuggerError::DebuggerError(format!("OpenThread(set): {}", e)))?
        };
        let result = unsafe { SetThreadContext(thread_handle, ctx) };
        unsafe { CloseHandle(thread_handle).ok(); }
        result.map_err(|e| DebuggerError::DebuggerError(format!("SetThreadContext: {}", e)))
    }

    fn get_rip(&self) -> Result<u64, DebuggerError> {
        let tid = *self.stopped_tid.lock().unwrap();
        if tid == 0 { return Ok(0); }
        let ctx = self.get_thread_context(tid)?;
        Ok(ctx.Rip)
    }

    fn get_rbp(&self) -> Result<u64, DebuggerError> {
        let tid = *self.stopped_tid.lock().unwrap();
        if tid == 0 { return Ok(0); }
        let ctx = self.get_thread_context(tid)?;
        Ok(ctx.Rbp)
    }

    fn read_return_address(&self) -> Result<u64, DebuggerError> {
        let tid = *self.stopped_tid.lock().unwrap();
        if tid == 0 { return Err(DebuggerError::ProcessNotFound); }
        let ctx = self.get_thread_context(tid)?;
        let rsp = ctx.Rsp;
        let process = self.require_process()?;
        let bytes = self.read_bytes(process, rsp, 8)?;
        if bytes.len() < 8 {
            return Err(DebuggerError::DebuggerError("could not read return address from stack".into()));
        }
        Ok(u64::from_le_bytes(bytes[..8].try_into().unwrap()))
    }

    fn single_step(&self) -> Result<ExecutionEvent, DebuggerError> {
        let tid = *self.stopped_tid.lock().unwrap();
        if tid == 0 {
            return Err(DebuggerError::DebuggerError("no stopped thread for single-step".into()));
        }
        let mut ctx = self.get_thread_context(tid)?;
        // Set EFLAGS.TF = 1 to trigger EXCEPTION_SINGLE_STEP after next instruction.
        ctx.EFlags |= TRAP_FLAG;
        self.set_thread_context(tid, &ctx)?;
        self.event_loop()
    }

    /// Drain the initial CREATE_PROCESS + DLL events and return the image base.
    fn drain_initial_events(&self, pid: u32) -> Result<u64, DebuggerError> {
        let mut image_base = 0u64;
        let mut event = DEBUG_EVENT::default();
        for _ in 0..64 {
            // SAFETY: WaitForDebugEvent from the debug thread. GitHub issue: #1.
            let ok = unsafe { WaitForDebugEvent(&mut event, 200) };
            if ok.is_err() { break; }
            let tid = event.dwThreadId;

            if event.dwDebugEventCode == CREATE_PROCESS_DEBUG_EVENT {
                // Image base is in CreateProcessInfo
                let base = unsafe { event.u.CreateProcessInfo.lpBaseOfImage as u64 };
                if base != 0 { image_base = base; }
                // The first event fires at the process entry point, which is a BP.
                unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
                return Ok(image_base);
            }
            unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
        }
        Ok(image_base)
    }

    /// Main event loop — blocks until the next interesting stop event.
    fn event_loop(&self) -> Result<ExecutionEvent, DebuggerError> {
        let pid = self.require_pid()?;
        let mut event = DEBUG_EVENT::default();
        loop {
            // SAFETY: WaitForDebugEvent from the debug thread. GitHub issue: #1.
            let ok = unsafe { WaitForDebugEvent(&mut event, 30_000) };
            if ok.is_err() {
                return Err(DebuggerError::DebuggerError("WaitForDebugEvent timeout".into()));
            }
            let tid = event.dwThreadId;
            match event.dwDebugEventCode {
                EXCEPTION_DEBUG_EVENT => {
                    let exc = unsafe { &event.u.Exception };
                    let code = exc.ExceptionRecord.ExceptionCode.0 as u32;
                    let addr = exc.ExceptionRecord.ExceptionAddress as u64;
                    let first_chance = exc.dwFirstChance != 0;

                    match code {
                        // EXCEPTION_BREAKPOINT
                        0x80000003 => {
                            let bp_id = self.on_breakpoint_hit(addr)?;
                            *self.stopped_tid.lock().unwrap() = tid;
                            *self.state.lock().unwrap() = SessionState::Paused(PauseReason::Breakpoint(bp_id));
                            unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
                            let pdb_loc = self.pdb.lock().unwrap()
                                .as_ref()
                                .and_then(|p| p.va_to_source(addr.saturating_sub(1)));
                            return Ok(ExecutionEvent {
                                kind: ExecutionEventKind::BreakpointHit,
                                thread_id: tid as u64,
                                location: pdb_loc,
                            });
                        }
                        // EXCEPTION_SINGLE_STEP
                        0x80000004 => {
                            *self.stopped_tid.lock().unwrap() = tid;
                            *self.state.lock().unwrap() = SessionState::Paused(PauseReason::Step);
                            unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
                            let pdb_loc = self.pdb.lock().unwrap()
                                .as_ref()
                                .and_then(|p| p.va_to_source(addr));
                            return Ok(ExecutionEvent {
                                kind: ExecutionEventKind::StepComplete,
                                thread_id: tid as u64,
                                location: pdb_loc,
                            });
                        }
                        // Unhandled exception = Rust panic/abort
                        _ if !first_chance => {
                            let msg = self.read_panic_message().unwrap_or_else(|| {
                                format!("unhandled exception 0x{:08X} at 0x{:x}", code, addr)
                            });
                            warn!(panic = %msg, "panic/unhandled exception");
                            *self.stopped_tid.lock().unwrap() = tid;
                            *self.state.lock().unwrap() = SessionState::Paused(PauseReason::Panic);
                            unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_EXCEPTION_NOT_HANDLED).ok(); }
                            let pdb_loc = self.pdb.lock().unwrap()
                                .as_ref()
                                .and_then(|p| p.va_to_source(addr));
                            return Ok(ExecutionEvent {
                                kind: ExecutionEventKind::PanicDetected { message: msg },
                                thread_id: tid as u64,
                                location: pdb_loc,
                            });
                        }
                        _ => {
                            unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_EXCEPTION_NOT_HANDLED).ok(); }
                        }
                    }
                }
                EXIT_PROCESS_DEBUG_EVENT => {
                    let exit_code = unsafe { event.u.ExitProcess.dwExitCode };
                    // Non-zero exit = likely a Rust panic via abort.
                    let kind = if exit_code != 0 {
                        let msg = self.read_panic_message().unwrap_or_else(|| {
                            format!("process exited with code {}", exit_code)
                        });
                        if msg.contains("panic") || msg.contains("panicked") {
                            ExecutionEventKind::PanicDetected { message: msg }
                        } else {
                            ExecutionEventKind::Terminated { exit_code: exit_code as i32 }
                        }
                    } else {
                        ExecutionEventKind::Terminated { exit_code: 0 }
                    };
                    *self.state.lock().unwrap() = SessionState::Terminated(exit_code as i32);
                    unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
                    return Ok(ExecutionEvent { kind, thread_id: tid as u64, location: None });
                }
                _ => {
                    unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
                }
            }
        }
    }

    fn on_breakpoint_hit(&self, rip_after_int3: u64) -> Result<BreakpointId, DebuggerError> {
        let patch_addr = rip_after_int3.saturating_sub(1);
        let bps = self.breakpoints.lock().unwrap();
        if let Some((&id, p)) = bps.iter().find(|(_, p)| p.address == patch_addr) {
            let original = p.original_byte;
            drop(bps);
            let process = self.require_process()?;
            self.write_byte(process, patch_addr, original)?;
            self.breakpoints.lock().unwrap().get_mut(&id).map(|p| p.bp.increment_hit_count());
            Ok(id)
        } else {
            Ok(0) // system/injected breakpoint
        }
    }

    fn resolve_address(&self, kind: &BreakpointKind) -> Result<u64, DebuggerError> {
        match kind {
            BreakpointKind::Address { addr } => Ok(*addr),
            BreakpointKind::SourceLine { file, line } => {
                self.pdb.lock().unwrap()
                    .as_ref()
                    .and_then(|p| p.source_to_va(file, *line))
                    .ok_or_else(|| DebuggerError::DebuggerError(
                        format!("no address for {}:{} in PDB", file.display(), line)
                    ))
            }
            BreakpointKind::FunctionName { name } => {
                self.pdb.lock().unwrap()
                    .as_ref()
                    .and_then(|p| p.function_name_to_va(name))
                    .ok_or_else(|| DebuggerError::DebuggerError(
                        format!("function '{}' not found in PDB", name)
                    ))
            }
            BreakpointKind::Regex { .. } =>
                Err(DebuggerError::DebuggerError("regex breakpoints not yet supported".into())),
        }
    }

    // ── Local variable reading ────────────────────────────────────────────────

    fn read_local_var(
        &self,
        process: HANDLE,
        local: &crate::pdb_info::PdbLocal,
        rbp: u64,
        probe_context: Option<&str>,
    ) -> Result<Variable, DebuggerError> {
        let address = match local.location {
            VarLocation::FramePointerRelative(offset) => {
                (rbp as i64 + offset as i64) as u64
            }
            VarLocation::Register(_) => {
                // Register variables: skip for now (require GetThreadContext per-register mapping)
                return Ok(Variable {
                    name: local.name.clone(),
                    type_name: local.type_name.clone(),
                    value: VariableValue::Opaque { summary: "<register variable>".into() },
                    address: None,
                    semantic: probe_context.map(|ctx| SemanticAnnotation {
                        context: ctx.to_string(),
                        qualified_name: format!("{}.{}", ctx, local.name),
                        description: None,
                    }),
                });
            }
        };

        let size = if local.size > 0 { local.size } else { 8 }; // default to pointer size
        let bytes = self.read_bytes(process, address, size).unwrap_or_default();

        let value = bytes_to_value(&bytes, &local.type_name, address);

        Ok(Variable {
            name: local.name.clone(),
            type_name: local.type_name.clone(),
            value,
            address: Some(address),
            semantic: probe_context.map(|ctx| SemanticAnnotation {
                context: ctx.to_string(),
                qualified_name: format!("{}.{}", ctx, local.name),
                description: None,
            }),
        })
    }

    // ── Expression evaluation ─────────────────────────────────────────────────

    fn eval_expression(&self, expr: &str) -> VariableValue {
        let expr = expr.trim();

        // Handle arr[N] style indexing
        if let Some((var_name, rest)) = expr.split_once('[') {
            if let Some(idx_str) = rest.strip_suffix(']') {
                if let Ok(idx) = idx_str.trim().parse::<usize>() {
                    return self.eval_array_index(var_name.trim(), idx);
                }
            }
        }

        // Simple variable lookup
        self.eval_variable(expr)
    }

    fn eval_array_index(&self, var_name: &str, idx: usize) -> VariableValue {
        let process = match self.require_process() { Ok(p) => p, Err(_) => return VariableValue::Opaque { summary: "no process".into() } };
        let rip = match self.get_rip() { Ok(r) => r, Err(_) => return VariableValue::Opaque { summary: "no rip".into() } };
        let rbp = match self.get_rbp() { Ok(r) => r, Err(_) => return VariableValue::Opaque { summary: "no rbp".into() } };

        let pdb_guard = self.pdb.lock().unwrap();
        let pdb = match pdb_guard.as_ref() { Some(p) => p, None => return VariableValue::Opaque { summary: "no PDB".into() } };
        let locals = pdb.locals_at_va(rip);

        let local = locals.iter().find(|l| l.name == var_name);
        let local = match local { Some(l) => l, None => return VariableValue::Opaque { summary: format!("variable {} not found", var_name) } };

        // Vec<T> layout on x64: { ptr: *T, len: usize, cap: usize } (each 8 bytes)
        let base_addr = match local.location {
            VarLocation::FramePointerRelative(off) => (rbp as i64 + off as i64) as u64,
            _ => return VariableValue::Opaque { summary: "register-based array not supported".into() },
        };

        // Read Vec<T> fat pointer: [data_ptr (8 bytes), len (8 bytes), cap (8 bytes)]
        let vec_bytes = match self.read_bytes(process, base_addr, 24) { Ok(b) => b, Err(e) => return VariableValue::Error { message: e.to_string() } };
        if vec_bytes.len() < 24 { return VariableValue::Opaque { summary: "truncated vec read".into() }; }

        let data_ptr = u64::from_le_bytes(vec_bytes[0..8].try_into().unwrap());
        let len = u64::from_le_bytes(vec_bytes[8..16].try_into().unwrap()) as usize;

        if idx >= len {
            return VariableValue::Error { message: format!("index {} out of bounds (len={})", idx, len) };
        }

        // For Vec<i32>: each element is 4 bytes
        let elem_addr = data_ptr + (idx as u64 * 4);
        let elem_bytes = match self.read_bytes(process, elem_addr, 4) { Ok(b) => b, Err(e) => return VariableValue::Error { message: e.to_string() } };
        if elem_bytes.len() < 4 { return VariableValue::Opaque { summary: "short element read".into() }; }
        let val = i32::from_le_bytes(elem_bytes[..4].try_into().unwrap());
        VariableValue::Scalar(ScalarValue::Int(val as i128))
    }

    fn eval_variable(&self, name: &str) -> VariableValue {
        let process = match self.require_process() { Ok(p) => p, Err(_) => return VariableValue::Opaque { summary: "no process".into() } };
        let rip = match self.get_rip() { Ok(r) => r, Err(_) => return VariableValue::Opaque { summary: "no rip".into() } };
        let rbp = match self.get_rbp() { Ok(r) => r, Err(_) => return VariableValue::Opaque { summary: "no rbp".into() } };

        let pdb_guard = self.pdb.lock().unwrap();
        let pdb = match pdb_guard.as_ref() { Some(p) => p, None => return VariableValue::Opaque { summary: "no PDB".into() } };
        let locals = pdb.locals_at_va(rip);
        let local = match locals.iter().find(|l| l.name == name) {
            Some(l) => l,
            None => return VariableValue::Opaque { summary: format!("'{}' not found in scope", name) },
        };

        let addr = match local.location {
            VarLocation::FramePointerRelative(off) => (rbp as i64 + off as i64) as u64,
            _ => return VariableValue::Opaque { summary: "register variable".into() },
        };
        let size = if local.size > 0 { local.size } else { 8 };
        let bytes = match self.read_bytes(process, addr, size) { Ok(b) => b, Err(e) => return VariableValue::Error { message: e.to_string() } };
        bytes_to_value(&bytes, &local.type_name, addr)
    }

    // ── Panic message capture via stderr pipe ─────────────────────────────────

    fn read_panic_message(&self) -> Option<String> {
        use windows::Win32::System::Pipes::PeekNamedPipe;
        use std::io::Read;
        use std::mem::ManuallyDrop;
        use std::os::windows::io::FromRawHandle;

        let handle = self.stderr_read.lock().unwrap().as_ref().copied()?;

        // Peek without blocking to see how many bytes the child wrote.
        // SAFETY: PeekNamedPipe doesn't consume bytes; safe to call anytime.
        // GitHub issue: #1 — Safe alternative: none.
        let mut available: u32 = 0;
        let ok = unsafe { PeekNamedPipe(handle, None, 0, None, Some(&mut available), None) };
        if ok.is_err() || available == 0 { return None; }

        // Wrap the raw HANDLE in a std::fs::File for safe, idiomatic reading.
        // ManuallyDrop prevents File from closing the handle on drop (we manage it).
        // SAFETY: handle is a valid, open pipe read-end. ManuallyDrop prevents double-close.
        // GitHub issue: #1
        let mut file = ManuallyDrop::new(unsafe {
            std::fs::File::from_raw_handle(handle.0 as *mut std::ffi::c_void)
        });

        let mut buf = vec![0u8; available.min(8192) as usize];
        let n = file.read(&mut buf).unwrap_or(0);

        let text = String::from_utf8_lossy(&buf[..n]).to_string();
        extract_panic_message(&text)
    }

    // ── Thread enumeration ────────────────────────────────────────────────────

    fn enumerate_threads(&self, pid: u32) -> Result<Vec<ThreadInfo>, DebuggerError> {
        let mut threads = Vec::new();
        // SAFETY: Toolhelp snapshot of threads. GitHub issue: #1.
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
            .map_err(|e| DebuggerError::DebuggerError(format!("CreateToolhelp32Snapshot: {}", e)))?;

        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        // SAFETY: Thread32First/Next iterate the snapshot. GitHub issue: #1.
        let mut ok = unsafe { Thread32First(snap, &mut entry) };
        while ok.is_ok() {
            if entry.th32OwnerProcessID == pid {
                threads.push(ThreadInfo {
                    id: entry.th32ThreadID as u64,
                    name: None,
                    state: ThreadState::Stopped,
                    stop_reason: None,
                    frame_count: 1,
                });
            }
            ok = unsafe { Thread32Next(snap, &mut entry) };
        }
        // SAFETY: Close the snapshot handle. GitHub issue: #1.
        unsafe { CloseHandle(snap).ok(); }
        Ok(threads)
    }
}

impl Drop for WindowsDebugBackend {
    fn drop(&mut self) {
        // SAFETY: Close process and stderr-read handles on drop. GitHub issue: #1.
        if let Some(h) = *self.process.lock().unwrap() { unsafe { CloseHandle(h).ok(); } }
        if let Some(h) = *self.stderr_read.lock().unwrap() { unsafe { CloseHandle(h).ok(); } }
    }
}

// ── Free helpers ──────────────────────────────────────────────────────────────

/// Interpret raw bytes as a typed VariableValue based on the type name.
fn bytes_to_value(bytes: &[u8], type_name: &str, _address: u64) -> VariableValue {
    match (type_name, bytes.len()) {
        ("bool", 1) => VariableValue::Scalar(ScalarValue::Bool(bytes[0] != 0)),
        ("i8",  1) => VariableValue::Scalar(ScalarValue::Int(bytes[0] as i8 as i128)),
        ("u8",  1) => VariableValue::Scalar(ScalarValue::UInt(bytes[0] as u128)),
        ("i16", 2) => VariableValue::Scalar(ScalarValue::Int(i16::from_le_bytes(bytes[..2].try_into().unwrap()) as i128)),
        ("u16", 2) => VariableValue::Scalar(ScalarValue::UInt(u16::from_le_bytes(bytes[..2].try_into().unwrap()) as u128)),
        ("i32", 4) | (_, 4) if type_name.starts_with("i32") =>
            VariableValue::Scalar(ScalarValue::Int(i32::from_le_bytes(bytes[..4].try_into().unwrap()) as i128)),
        ("u32", 4) =>
            VariableValue::Scalar(ScalarValue::UInt(u32::from_le_bytes(bytes[..4].try_into().unwrap()) as u128)),
        ("f32", 4) =>
            VariableValue::Scalar(ScalarValue::Float(f32::from_le_bytes(bytes[..4].try_into().unwrap()) as f64)),
        ("i64", 8) | ("isize", 8) =>
            VariableValue::Scalar(ScalarValue::Int(i64::from_le_bytes(bytes[..8].try_into().unwrap()) as i128)),
        ("u64", 8) | ("usize", 8) =>
            VariableValue::Scalar(ScalarValue::UInt(u64::from_le_bytes(bytes[..8].try_into().unwrap()) as u128)),
        ("f64", 8) =>
            VariableValue::Scalar(ScalarValue::Float(f64::from_le_bytes(bytes[..8].try_into().unwrap()))),
        _ => {
            let hex: String = bytes.iter().take(16).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
            VariableValue::Opaque { summary: format!("[{} bytes: {}{}]", bytes.len(), hex, if bytes.len() > 16 { " ..." } else { "" }) }
        }
    }
}

/// Parse the Rust panic message from stderr output.
fn extract_panic_message(stderr: &str) -> Option<String> {
    // New format (Rust 1.65+):
    // thread 'main' panicked at src\main.rs:58:22:
    // index out of bounds: the len is 3 but the index is 99
    //
    // Old format:
    // thread 'main' panicked at 'index out of bounds: ...', src\main.rs:58:22

    // New format: line after "panicked at <location>:\n"
    if let Some(pos) = stderr.find("panicked at ") {
        let after = &stderr[pos..];
        // Try new format: "panicked at file:line:\nmessage"
        if let Some(newline_pos) = after.find('\n') {
            let message_start = &after[newline_pos + 1..];
            let message = message_start.lines().next()?.trim();
            if !message.is_empty() {
                return Some(message.to_string());
            }
        }
        // Try old format: "panicked at 'message', ..."
        if let Some(rest) = after.strip_prefix("panicked at '") {
            let end = rest.find("', ")?;
            return Some(rest[..end].to_string());
        }
    }

    // Fallback: return the whole stderr trimmed
    let trimmed = stderr.trim();
    if !trimmed.is_empty() { Some(trimmed.to_string()) } else { None }
}
