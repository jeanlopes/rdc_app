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
use tracing::{error, info, instrument, warn};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::{
    ContinueDebugEvent, DebugBreakProcess, GetThreadContext, ReadProcessMemory,
    SetThreadContext, StackWalk64, SymFunctionTableAccess64, SymGetModuleBase64,
    SymInitialize, WaitForDebugEvent, WriteProcessMemory,
    ADDRESS64, ADDRESS_MODE, CONTEXT, CONTEXT_FLAGS, DEBUG_EVENT,
    EXCEPTION_DEBUG_EVENT, EXIT_PROCESS_DEBUG_EVENT, CREATE_PROCESS_DEBUG_EVENT,
    STACKFRAME64,
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
    /// Address that needs INT3 re-patched after the original instruction is single-stepped.
    pending_repatch: Mutex<Option<u64>>,
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
            pending_repatch: Mutex::new(None),
        })
    }

    // ── Session ──────────────────────────────────────────────────────────────

    #[instrument(skip(self))]
    pub fn launch_process(&self, target: DebugTarget) -> Result<(u32, SessionState), DebuggerError> {
        let exe = target.executable.clone();
        let exe_str = exe.to_string_lossy().to_string();
        info!(executable = %exe_str, "LAUNCH_PROCESS start");

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
        info!(pid, h_process = ?pi.hProcess, "CreateProcessA succeeded");
        *self.process.lock().unwrap() = Some(pi.hProcess);
        *self.pid.lock().unwrap() = Some(pid);
        *self.stderr_read.lock().unwrap() = Some(stderr_read);
        *self.exe_path.lock().unwrap() = Some(exe.clone());
        // Process starts as Paused because we stop at the entry-point breakpoint
        // so the caller can set breakpoints before real execution.
        *self.state.lock().unwrap() = SessionState::Paused(PauseReason::Breakpoint(0));

        // Drain initial events and capture image base.
        info!(pid, "draining initial debug events...");
        let image_base = self.drain_initial_events(pid)?;
        info!(pid, image_base = %format!("0x{:x}", image_base), "initial events drained — process PAUSED at entry point");

        // Initialise DbgHelp for this process — required by StackWalk64.
        // fInvadeProcess=true auto-loads symbols for all loaded modules.
        // SAFETY: SymInitialize must be called once per process handle before StackWalk64.
        // GitHub issue: #1 — Safe alternative: none.
        let process_handle = self.require_process()?;
        unsafe {
            let _ = SymInitialize(process_handle, None, true);
        }

        // Load PDB for source-level symbol resolution.
        match PdbInfo::load(&exe, image_base) {
            Ok(pdb_info) => {
                info!("PDB loaded successfully");
                *self.pdb.lock().unwrap() = Some(pdb_info);
            }
            Err(e) => {
                warn!("PDB load failed (source-level debugging unavailable): {}", e);
            }
        }

        info!(pid, executable = %exe_str, "LAUNCH_PROCESS complete — returning Paused (entry point)");
        Ok((pid, SessionState::Paused(PauseReason::Breakpoint(0))))
    }

    pub fn get_state(&self) -> Result<SessionState, DebuggerError> {
        Ok(self.state.lock().unwrap().clone())
    }

    // ── Breakpoints ──────────────────────────────────────────────────────────

    #[instrument(skip(self))]
    pub fn set_breakpoint(&self, kind: BreakpointKind, condition: Option<String>) -> Result<Breakpoint, DebuggerError> {
        info!(?kind, "SET_BREAKPOINT start");
        let address = self.resolve_address(&kind)?;
        info!(address = %format!("0x{:x}", address), "address resolved");
        let process = self.require_process()?;
        info!(?process, "got process handle");

        info!(address = %format!("0x{:x}", address), "reading original byte...");
        let original_byte = self.read_byte(process, address)?;
        info!(original_byte = %format!("0x{:02x}", original_byte), "original byte read");

        info!(address = %format!("0x{:x}", address), "writing INT3...");
        self.write_byte(process, address, INT3)?;
        info!("INT3 written");

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
        info!(bp_id = id, address = %format!("0x{:x}", address), original_byte = %format!("0x{:02x}", original_byte), "SET_BREAKPOINT complete");
        Ok(bp)
    }

    pub fn remove_breakpoint(&self, id: BreakpointId) -> Result<(), DebuggerError> {
        info!(bp_id = id, "REMOVE_BREAKPOINT start");
        let patched = self.breakpoints.lock().unwrap().remove(&id)
            .ok_or(DebuggerError::BreakpointNotFound(id))?;
        info!(address = %format!("0x{:x}", patched.address), original_byte = %format!("0x{:02x}", patched.original_byte), "found breakpoint to remove");
        let process = self.require_process()?;
        self.write_byte(process, patched.address, patched.original_byte)?;
        info!(bp_id = id, "REMOVE_BREAKPOINT complete");
        Ok(())
    }

    pub fn list_breakpoints(&self) -> Result<Vec<Breakpoint>, DebuggerError> {
        Ok(self.breakpoints.lock().unwrap().values().map(|p| p.bp.clone()).collect())
    }

    // ── Execution control ────────────────────────────────────────────────────

    pub fn continue_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        info!("CONTINUE_EXECUTION called");
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

    pub fn read_stack(&self, thread_id: Option<ThreadId>, max_frames: u32) -> Result<Vec<StackFrame>, DebuggerError> {
        use windows::Win32::System::Threading::{OpenThread, THREAD_GET_CONTEXT, THREAD_SUSPEND_RESUME};

        let tid = thread_id
            .map(|id| id as u32)
            .unwrap_or_else(|| *self.stopped_tid.lock().unwrap());

        if tid == 0 {
            return Ok(vec![]);
        }

        let process = self.require_process()?;
        let mut ctx = self.get_thread_context(tid)?;

        // SAFETY: OpenThread for context reading. Thread is halted at a debug event.
        // GitHub issue: #1 — Safe alternative: none.
        let thread_handle = unsafe {
            OpenThread(THREAD_GET_CONTEXT | THREAD_SUSPEND_RESUME, false, tid)
                .map_err(|e| DebuggerError::DebuggerError(format!("OpenThread(stack): {}", e)))?
        };

        // AMD64 flat mode = 3
        let flat = ADDRESS_MODE(3);
        let mut frame = STACKFRAME64 {
            AddrPC:    ADDRESS64 { Offset: ctx.Rip, Segment: 0, Mode: flat },
            AddrStack: ADDRESS64 { Offset: ctx.Rsp, Segment: 0, Mode: flat },
            AddrFrame: ADDRESS64 { Offset: ctx.Rbp, Segment: 0, Mode: flat },
            ..Default::default()
        };

        const IMAGE_FILE_MACHINE_AMD64: u32 = 0x8664;

        let mut frames = Vec::new();
        let pdb_guard = self.pdb.lock().unwrap();

        for _ in 0..max_frames.min(64) {
            // SAFETY: StackWalk64 must be called from a loop. ctx is updated each iteration.
            // process and thread_handle are valid open handles.
            // SymFunctionTableAccess64 / SymGetModuleBase64 are DbgHelp callbacks
            // that require a prior SymInitialize (called in launch_process).
            // GitHub issue: #1 — Safe alternative: none.
            let ok = unsafe {
                StackWalk64(
                    IMAGE_FILE_MACHINE_AMD64,
                    process,
                    thread_handle,
                    &mut frame,
                    &mut ctx as *mut CONTEXT as *mut _,
                    None,
                    Some(sym_func_table_access),
                    Some(sym_get_module_base),
                    None,
                )
            };

            if !ok.as_bool() || frame.AddrPC.Offset == 0 {
                break;
            }

            let rip = frame.AddrPC.Offset;
            let (function_name, source_location) = if let Some(pdb) = pdb_guard.as_ref() {
                (pdb.va_to_function_name(rip), pdb.va_to_source(rip))
            } else {
                (None, None)
            };

            frames.push(StackFrame {
                index: frames.len() as u32,
                function_name,
                module: None,
                source_location,
                is_inlined: false,
            });
        }

        // SAFETY: CloseHandle releases the thread handle. GitHub issue: #1.
        unsafe { CloseHandle(thread_handle).ok(); }
        Ok(frames)
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
        info!(?process, address = %format!("0x{:x}", address), "READ_BYTE");
        // SAFETY: ReadProcessMemory, 1 byte. GitHub issue: #1.
        let result = unsafe {
            ReadProcessMemory(process, address as *const _, &mut byte as *mut u8 as *mut _, 1, Some(&mut read))
        };
        match result {
            Ok(_) => {
                info!(byte = %format!("0x{:02x}", byte), read, "READ_BYTE success");
                Ok(byte)
            }
            Err(e) => {
                error!(address = %format!("0x{:x}", address), error = %e, "READ_BYTE FAILED");
                Err(DebuggerError::DebuggerError(format!("ReadProcessMemory: {}", e)))
            }
        }
    }

    fn write_byte(&self, process: HANDLE, address: u64, byte: u8) -> Result<(), DebuggerError> {
        let mut written = 0usize;
        info!(?process, address = %format!("0x{:x}", address), byte = %format!("0x{:02x}", byte), "WRITE_BYTE");
        // SAFETY: WriteProcessMemory, 1 byte. GitHub issue: #1.
        let result = unsafe {
            WriteProcessMemory(process, address as *const _, &byte as *const u8 as *const _, 1, Some(&mut written))
        };
        match result {
            Ok(_) => {
                info!(written, "WRITE_BYTE success");
                Ok(())
            }
            Err(e) => {
                error!(address = %format!("0x{:x}", address), error = %e, "WRITE_BYTE FAILED");
                Err(DebuggerError::DebuggerError(format!("WriteProcessMemory: {}", e)))
            }
        }
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
        for i in 0..64 {
            // SAFETY: WaitForDebugEvent from the debug thread. GitHub issue: #1.
            let ok = unsafe { WaitForDebugEvent(&mut event, 200) };
            if ok.is_err() {
                info!(iteration = i, "WaitForDebugEvent timeout/errored in drain_initial_events");
                break;
            }
            let tid = event.dwThreadId;
            let code = event.dwDebugEventCode;
            info!(iteration = i, tid, code = code.0, "drain_initial_events received event");

            if code == CREATE_PROCESS_DEBUG_EVENT {
                let base = unsafe { event.u.CreateProcessInfo.lpBaseOfImage as u64 };
                if base != 0 { image_base = base; }
                info!(tid, base = %format!("0x{:x}", base), "CREATE_PROCESS_DEBUG_EVENT — continuing");
                // MUST continue this event so the process can proceed to the entry-point breakpoint
                unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
                continue;
            }

            // The very first exception after CREATE_PROCESS is the entry-point breakpoint.
            // Leave the process STOPPED here so breakpoints can be set before real execution.
            if code == EXCEPTION_DEBUG_EVENT {
                let exc = unsafe { &event.u.Exception };
                let exc_code = exc.ExceptionRecord.ExceptionCode.0 as u32;
                info!(tid, exc_code = %format!("0x{:08x}", exc_code), "EXCEPTION at process startup — leaving STOPPED");
                *self.stopped_tid.lock().unwrap() = tid;
                return Ok(image_base);
            }

            // Any other event: just continue
            unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
        }
        info!(image_base = %format!("0x{:x}", image_base), "drain_initial_events finished (no entry breakpoint?)");
        Ok(image_base)
    }

    /// Main event loop — blocks until the next interesting stop event.
    fn event_loop(&self) -> Result<ExecutionEvent, DebuggerError> {
        let pid = self.require_pid()?;
        info!(pid, "EVENT_LOOP started");
        let mut event = DEBUG_EVENT::default();
        loop {
            // SAFETY: WaitForDebugEvent from the debug thread. GitHub issue: #1.
            let ok = unsafe { WaitForDebugEvent(&mut event, 30_000) };
            if ok.is_err() {
                error!(pid, "WaitForDebugEvent timeout — returning error");
                return Err(DebuggerError::DebuggerError("WaitForDebugEvent timeout".into()));
            }
            let tid = event.dwThreadId;
            let code = event.dwDebugEventCode;
            info!(pid, tid, code = code.0, "EVENT_LOOP received debug event");

            match code {
                EXCEPTION_DEBUG_EVENT => {
                    let exc = unsafe { &event.u.Exception };
                    let exc_code = exc.ExceptionRecord.ExceptionCode.0 as u32;
                    let addr = exc.ExceptionRecord.ExceptionAddress as u64;
                    let first_chance = exc.dwFirstChance != 0;
                    info!(pid, tid, exc_code = %format!("0x{:08x}", exc_code), addr = %format!("0x{:x}", addr), first_chance, "EXCEPTION_DEBUG_EVENT");

                    match exc_code {
                        // EXCEPTION_BREAKPOINT
                        0x80000003 => {
                            info!(addr = %format!("0x{:x}", addr), "EXCEPTION_BREAKPOINT");
                            let bp_id = self.on_breakpoint_hit(addr, tid)?;
                            info!(bp_id, "breakpoint hit handled");
                            *self.stopped_tid.lock().unwrap() = tid;
                            *self.state.lock().unwrap() = SessionState::Paused(PauseReason::Breakpoint(bp_id));
                            unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
                            let pdb_loc = self.pdb.lock().unwrap()
                                .as_ref()
                                .and_then(|p| p.va_to_source(addr.saturating_sub(1)));
                            info!(?pdb_loc, "PDB lookup for breakpoint location");
                            return Ok(ExecutionEvent {
                                kind: ExecutionEventKind::BreakpointHit,
                                thread_id: tid as u64,
                                location: pdb_loc,
                            });
                        }
                        // EXCEPTION_SINGLE_STEP
                        0x80000004 => {
                            info!("EXCEPTION_SINGLE_STEP");
                            if let Some(patch_addr) = self.pending_repatch.lock().unwrap().take() {
                                info!(patch_addr = %format!("0x{:x}", patch_addr), "re-patching INT3 after single-step");
                                if let Err(e) = self.require_process().and_then(|p| self.write_byte(p, patch_addr, INT3)) {
                                    warn!("failed to re-patch breakpoint at 0x{:x}: {}", patch_addr, e);
                                } else {
                                    info!("re-patch success");
                                }
                            }
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
                                format!("unhandled exception 0x{:08X} at 0x{:x}", exc_code, addr)
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
                            info!(exc_code = %format!("0x{:08x}", exc_code), "ignoring first-chance exception");
                            unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_EXCEPTION_NOT_HANDLED).ok(); }
                        }
                    }
                }
                EXIT_PROCESS_DEBUG_EVENT => {
                    let exit_code = unsafe { event.u.ExitProcess.dwExitCode };
                    info!(pid, tid, exit_code, "EXIT_PROCESS_DEBUG_EVENT");
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
                    info!(code = code.0, "ignoring non-exception debug event");
                    unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
                }
            }
        }
    }

    fn on_breakpoint_hit(&self, rip_after_int3: u64, tid: u32) -> Result<BreakpointId, DebuggerError> {
        let patch_addr = rip_after_int3.saturating_sub(1);
        info!(rip = %format!("0x{:x}", rip_after_int3), patch_addr = %format!("0x{:x}", patch_addr), "ON_BREAKPOINT_HIT");
        let bps = self.breakpoints.lock().unwrap();
        if let Some((&id, p)) = bps.iter().find(|(_, p)| p.address == patch_addr) {
            info!(bp_id = id, "user breakpoint hit");
            let original = p.original_byte;
            drop(bps);
            let process = self.require_process()?;
            info!("restoring original byte before single-step");
            self.write_byte(process, patch_addr, original)?;
            self.breakpoints.lock().unwrap().get_mut(&id).map(|p| p.bp.increment_hit_count());
            // Schedule re-patch after we single-step the original instruction
            *self.pending_repatch.lock().unwrap() = Some(patch_addr);
            // Set trap flag so the CPU stops again right after the original instruction
            info!(tid, "setting TRAP_FLAG for single-step");
            let mut ctx = self.get_thread_context(tid)?;
            ctx.EFlags |= TRAP_FLAG;
            self.set_thread_context(tid, &ctx)?;
            Ok(id)
        } else {
            info!("system/injected breakpoint (id=0)");
            Ok(0)
        }
    }

    fn resolve_address(&self, kind: &BreakpointKind) -> Result<u64, DebuggerError> {
        info!(?kind, "RESOLVE_ADDRESS");
        let result = match kind {
            BreakpointKind::Address { addr } => Ok(*addr),
            BreakpointKind::SourceLine { file, line } => {
                let pdb = self.pdb.lock().unwrap();
                let maybe_addr = pdb.as_ref().and_then(|p| {
                    let exact = p.source_to_va(file, *line);
                    if exact.is_some() {
                        info!("found exact line mapping");
                    } else {
                        info!("exact line not found, trying nearest...");
                    }
                    exact.or_else(|| p.source_to_va_nearest(file, *line, 20))
                });
                info!(resolved = ?maybe_addr, "source line resolution result");
                maybe_addr.ok_or_else(|| DebuggerError::DebuggerError(
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
        };
        info!(address = ?result.as_ref().map(|a| format!("0x{:x}", a)), "RESOLVE_ADDRESS done");
        result
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

// ── StackWalk64 callback wrappers ────────────────────────────────────────────
// windows-rs exposes SymFunctionTableAccess64 / SymGetModuleBase64 as generic
// `unsafe fn` items. StackWalk64 expects `unsafe extern "system" fn` pointers,
// so we wrap them here with the correct ABI.

unsafe extern "system" fn sym_func_table_access(
    h: HANDLE,
    base: u64,
) -> *mut core::ffi::c_void {
    SymFunctionTableAccess64(h, base)
}

unsafe extern "system" fn sym_get_module_base(h: HANDLE, addr: u64) -> u64 {
    SymGetModuleBase64(h, addr)
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
            if let Some(message) = message_start.lines().next() {
                let trimmed = message.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
        // Try old format: "panicked at 'message', ..."
        if let Some(rest) = after.strip_prefix("panicked at '") {
            if let Some(end) = rest.find("', ") {
                return Some(rest[..end].to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_core::variable::{ScalarValue, VariableValue};

    #[test]
    fn bytes_bool_false() {
        assert!(matches!(bytes_to_value(&[0x00], "bool", 0), VariableValue::Scalar(ScalarValue::Bool(false))));
    }

    #[test]
    fn bytes_bool_true() {
        assert!(matches!(bytes_to_value(&[0x01], "bool", 0), VariableValue::Scalar(ScalarValue::Bool(true))));
    }

    #[test]
    fn bytes_i32_positive() {
        assert!(matches!(bytes_to_value(&[0x2c, 0x00, 0x00, 0x00], "i32", 0), VariableValue::Scalar(ScalarValue::Int(44))));
    }

    #[test]
    fn bytes_i32_negative() {
        assert!(matches!(bytes_to_value(&[0xff, 0xff, 0xff, 0xff], "i32", 0), VariableValue::Scalar(ScalarValue::Int(-1))));
    }

    #[test]
    fn bytes_u32() {
        assert!(matches!(bytes_to_value(&[0x05, 0x00, 0x00, 0x00], "u32", 0), VariableValue::Scalar(ScalarValue::UInt(5))));
    }

    #[test]
    fn bytes_usize() {
        assert!(matches!(
            bytes_to_value(&[0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], "usize", 0),
            VariableValue::Scalar(ScalarValue::UInt(8))
        ));
    }

    #[test]
    fn bytes_f32() {
        let bytes = 1.0f32.to_le_bytes();
        if let VariableValue::Scalar(ScalarValue::Float(f)) = bytes_to_value(&bytes, "f32", 0) {
            assert!((f - 1.0).abs() < f64::EPSILON);
        } else {
            panic!("expected Scalar(Float)");
        }
    }

    #[test]
    fn bytes_unknown_type_returns_opaque() {
        if let VariableValue::Opaque { summary } = bytes_to_value(&[0x01, 0x02, 0x03, 0x04], "SomeStruct", 0) {
            assert!(summary.contains("bytes"));
        } else {
            panic!("expected Opaque");
        }
    }

    #[test]
    fn panic_new_format() {
        let input = "thread 'main' panicked at src\\main.rs:58:22:\nindex out of bounds: the len is 3 but the index is 99\n";
        assert_eq!(
            extract_panic_message(input),
            Some("index out of bounds: the len is 3 but the index is 99".to_string())
        );
    }

    #[test]
    fn panic_old_format() {
        let input = "thread 'main' panicked at 'index out of bounds: the len is 3', src\\main.rs:58:22\n";
        assert_eq!(
            extract_panic_message(input),
            Some("index out of bounds: the len is 3".to_string())
        );
    }

    #[test]
    fn panic_empty_string_returns_none() {
        assert_eq!(extract_panic_message(""), None);
    }

    #[test]
    fn panic_unrelated_text_returns_none() {
        assert_eq!(extract_panic_message("hello world\n"), None);
    }

    #[test]
    fn panic_unwrap_on_none() {
        let input = "thread 'main' panicked at src\\main.rs:3:5:\ncalled `Option::unwrap()` on a `None` value\n";
        let msg = extract_panic_message(input).unwrap();
        assert!(msg.contains("called `Option::unwrap()` on a `None` value"));
    }
}
