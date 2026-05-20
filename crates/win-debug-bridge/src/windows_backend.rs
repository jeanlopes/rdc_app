//! Windows Debug API backend.
//!
//! Uses Win32 `CreateProcess(DEBUG_PROCESS)` + `WaitForDebugEvent` loop.
//! No LLDB, no Python, no external tools — only `windows-rs` + `pdb` crates.
//!
//! # Safety policy (Constitution Principle VI)
//! Every `unsafe` block calls a Win32 API. Each site carries:
//!   1. A safety proof comment
//!   2. A GitHub issue reference (placeholder: #1)
//!   3. The rejected safe alternative

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::{info, instrument, warn};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::{
    ContinueDebugEvent, DebugBreakProcess, ReadProcessMemory, WaitForDebugEvent,
    WriteProcessMemory, DEBUG_EVENT, EXCEPTION_DEBUG_EVENT, EXIT_PROCESS_DEBUG_EVENT,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next,
    THREADENTRY32, TH32CS_SNAPTHREAD,
};
use windows::Win32::System::Threading::{
    CreateProcessA, DEBUG_ONLY_THIS_PROCESS, DEBUG_PROCESS,
    PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, STARTUPINFOA,
};
use windows::core::PCSTR;

use runtime_core::{
    breakpoint::{Breakpoint, BreakpointId, BreakpointKind, BreakpointLocation},
    error::DebuggerError,
    process::{SourceLocation, StackFrame, ThreadId, ThreadInfo, ThreadState},
    session::{DebugTarget, PauseReason, SessionState},
    variable::{EvalResult, Variable, VariableValue},
};
use protocol::tools::execution::{ExecutionEvent, ExecutionEventKind};

/// INT3 opcode — software breakpoint on x86/x64.
const INT3: u8 = 0xCC;

struct PatchedBreakpoint {
    address: u64,
    original_byte: u8,
    bp: Breakpoint,
}

/// Windows Debug API backend (synchronous — runs on the dedicated debug OS thread).
pub struct WindowsDebugBackend {
    process: Mutex<Option<HANDLE>>,
    pid: Mutex<Option<u32>>,
    state: Mutex<SessionState>,
    breakpoints: Mutex<HashMap<BreakpointId, PatchedBreakpoint>>,
    next_bp_id: Mutex<u32>,
}

// SAFETY: WindowsDebugBackend is only ever used from the single debug OS thread.
// HANDLE values are never shared across threads.
unsafe impl Send for WindowsDebugBackend {}
unsafe impl Sync for WindowsDebugBackend {}

impl WindowsDebugBackend {
    /// Create a new backend in `Idle` state.
    pub fn new() -> Result<Self, DebuggerError> {
        Ok(Self {
            process: Mutex::new(None),
            pid: Mutex::new(None),
            state: Mutex::new(SessionState::Idle),
            breakpoints: Mutex::new(HashMap::new()),
            next_bp_id: Mutex::new(1),
        })
    }

    // ── Session ──────────────────────────────────────────────────────────────

    #[instrument(skip(self))]
    pub fn launch_process(&self, target: DebugTarget) -> Result<(u32, SessionState), DebuggerError> {
        let exe = target.executable.to_string_lossy().to_string();
        let mut cmdline = exe.clone();
        if !target.args.is_empty() {
            cmdline.push(' ');
            cmdline.push_str(&target.args.join(" "));
        }
        // Null-terminate for the Win32 API.
        let mut cmdline_bytes: Vec<u8> = cmdline.into_bytes();
        cmdline_bytes.push(0);

        let mut si = STARTUPINFOA {
            cb: std::mem::size_of::<STARTUPINFOA>() as u32,
            ..Default::default()
        };
        let mut pi = PROCESS_INFORMATION::default();

        // SAFETY: CreateProcessA launches the child process with the DEBUG_PROCESS flag.
        // cmdline_bytes is null-terminated and outlives this call.
        // si.cb is set correctly; all other fields are zero-initialized (valid defaults).
        // The returned pi.hProcess MUST be closed in Drop.
        // GitHub issue: #1
        // Safe alternative: std::process::Command — does not expose DEBUG_PROCESS flag.
        let result = unsafe {
            CreateProcessA(
                PCSTR::null(),
                windows::core::PSTR(cmdline_bytes.as_mut_ptr()),
                None,
                None,
                false,
                DEBUG_PROCESS | DEBUG_ONLY_THIS_PROCESS,
                None,
                PCSTR::null(),
                &si,
                &mut pi,
            )
        };

        result.map_err(|e| DebuggerError::DebuggerError(format!("CreateProcessA failed: {}", e)))?;

        let pid = pi.dwProcessId;
        *self.process.lock().unwrap() = Some(pi.hProcess);
        *self.pid.lock().unwrap() = Some(pid);
        *self.state.lock().unwrap() = SessionState::Running;

        // Consume the initial CREATE_PROCESS_DEBUG_EVENT break-in.
        let _ = self.drain_initial_events(pid);

        info!(pid, executable = %exe, "process launched under Windows debug");
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

        let id = {
            let mut g = self.next_bp_id.lock().unwrap();
            let id = *g;
            *g += 1;
            id
        };

        let bp = Breakpoint {
            id,
            kind,
            condition,
            hit_count: 0,
            enabled: true,
            locations: vec![BreakpointLocation {
                address,
                source_location: self.pdb_address_to_source(address),
                resolved: true,
            }],
        };
        self.breakpoints.lock().unwrap().insert(id, PatchedBreakpoint { address, original_byte, bp: bp.clone() });
        info!(bp_id = id, address = %format!("0x{:x}", address), "breakpoint set");
        Ok(bp)
    }

    pub fn remove_breakpoint(&self, id: BreakpointId) -> Result<(), DebuggerError> {
        let patched = self.breakpoints.lock().unwrap()
            .remove(&id)
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

    #[instrument(skip(self))]
    pub fn continue_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        self.event_loop(false)
    }

    pub fn pause_execution(&self) -> Result<ExecutionEvent, DebuggerError> {
        let process = self.require_process()?;
        // SAFETY: DebugBreakProcess injects a software breakpoint into the target.
        // process handle is valid (checked above). GitHub issue: #1
        // Safe alternative: none — Win32 has no other way to interrupt a running debuggee.
        unsafe {
            DebugBreakProcess(process)
                .map_err(|e| DebuggerError::DebuggerError(format!("DebugBreakProcess: {}", e)))?;
        }
        self.event_loop(false)
    }

    pub fn step_over(&self, _thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.single_step()
    }

    pub fn step_into(&self, _thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        self.single_step()
    }

    pub fn step_out(&self, _thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        // TODO: full step_out sets a temp BP at the return address from the stack.
        // Phase 1: falls back to single-step.
        self.single_step()
    }

    // ── Inspection ───────────────────────────────────────────────────────────

    pub fn read_locals(
        &self,
        _thread_id: Option<ThreadId>,
        _frame_index: u32,
        probe_context: Option<String>,
        _max_depth: u32,
    ) -> Result<Vec<Variable>, DebuggerError> {
        // Phase 1: PDB local variable enumeration (S_LOCAL records) — stub.
        // Full implementation: enumerate locals for current function via pdb crate,
        // then ReadProcessMemory for each variable address.
        let _ = probe_context;
        Ok(vec![])
    }

    pub fn read_stack(&self, _thread_id: Option<ThreadId>, _max_frames: u32) -> Result<Vec<StackFrame>, DebuggerError> {
        // Phase 1: single frame from current RIP. Full stack walking uses StackWalk64.
        Ok(vec![StackFrame {
            index: 0,
            function_name: None,
            module: None,
            source_location: None,
            is_inlined: false,
        }])
    }

    pub fn evaluate_expression(
        &self,
        expression: String,
        _thread_id: Option<ThreadId>,
        _frame_index: u32,
    ) -> Result<EvalResult, DebuggerError> {
        // Phase 1 stub. Full implementation: parse expression, look up variable
        // addresses from PDB, ReadProcessMemory for value.
        Ok(EvalResult {
            expression,
            value: VariableValue::Opaque { summary: "expression eval: Phase 2".into() },
            type_name: "unknown".into(),
            error: None,
        })
    }

    pub fn list_threads(&self) -> Result<Vec<ThreadInfo>, DebuggerError> {
        let pid = self.require_pid()?;
        self.enumerate_threads(pid)
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    fn require_process(&self) -> Result<HANDLE, DebuggerError> {
        self.process.lock().unwrap().ok_or(DebuggerError::ProcessNotFound)
    }

    fn require_pid(&self) -> Result<u32, DebuggerError> {
        self.pid.lock().unwrap().ok_or(DebuggerError::ProcessNotFound)
    }

    fn read_byte(&self, process: HANDLE, address: u64) -> Result<u8, DebuggerError> {
        let mut byte: u8 = 0;
        let mut read = 0usize;
        // SAFETY: ReadProcessMemory reads 1 byte from the target process address space.
        // process handle and address are valid. buffer is 1 byte on the stack.
        // GitHub issue: #1 — Safe alternative: none.
        unsafe {
            ReadProcessMemory(
                process,
                address as *const _,
                &mut byte as *mut u8 as *mut _,
                1,
                Some(&mut read),
            ).map_err(|e| DebuggerError::DebuggerError(format!("ReadProcessMemory: {}", e)))?;
        }
        Ok(byte)
    }

    fn write_byte(&self, process: HANDLE, address: u64, byte: u8) -> Result<(), DebuggerError> {
        let mut written = 0usize;
        // SAFETY: WriteProcessMemory writes 1 byte to the target process address space.
        // process handle is valid; address must be writable (verified by caller for INT3 patching).
        // GitHub issue: #1 — Safe alternative: none.
        unsafe {
            WriteProcessMemory(
                process,
                address as *const _,
                &byte as *const u8 as *const _,
                1,
                Some(&mut written),
            ).map_err(|e| DebuggerError::DebuggerError(format!("WriteProcessMemory: {}", e)))?;
        }
        Ok(())
    }

    fn single_step(&self) -> Result<ExecutionEvent, DebuggerError> {
        // Set EFLAGS.TF=1 via SetThreadContext, then continue.
        // Phase 1: simplified — just continues and waits for any stop event.
        // Full implementation: GetThreadContext → set TF → SetThreadContext → event loop.
        self.event_loop(true)
    }

    /// Drain the initial CREATE_PROCESS + DLL load events emitted right after CreateProcess.
    fn drain_initial_events(&self, pid: u32) -> Result<(), DebuggerError> {
        let mut event = DEBUG_EVENT::default();
        for _ in 0..32 {
            // SAFETY: WaitForDebugEvent must be called from the CreateProcess thread.
            // This method is only called from the dedicated debug OS thread.
            // GitHub issue: #1 — Safe alternative: none.
            let ok = unsafe { WaitForDebugEvent(&mut event, 100) };
            if ok.is_err() {
                break;
            }
            let tid = event.dwThreadId;
            if event.dwDebugEventCode == EXCEPTION_DEBUG_EVENT {
                // First break-in (system breakpoint) — this is where the process is first paused.
                unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
                *self.state.lock().unwrap() = SessionState::Running;
                return Ok(());
            }
            unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
        }
        Ok(())
    }

    /// Main event loop — blocks until the next interesting stop event.
    fn event_loop(&self, is_step: bool) -> Result<ExecutionEvent, DebuggerError> {
        let pid = self.require_pid()?;
        let mut event = DEBUG_EVENT::default();
        loop {
            // SAFETY: WaitForDebugEvent must be called from the CreateProcess thread.
            // timeout = 10000ms to avoid infinite hangs.
            // GitHub issue: #1 — Safe alternative: none.
            let ok = unsafe { WaitForDebugEvent(&mut event, 10_000) };
            if ok.is_err() {
                return Err(DebuggerError::DebuggerError("WaitForDebugEvent timed out or failed".into()));
            }
            let tid = event.dwThreadId;
            match event.dwDebugEventCode {
                EXCEPTION_DEBUG_EVENT => {
                    let exc = unsafe { &event.u.Exception };
                    let code = exc.ExceptionRecord.ExceptionCode.0 as u32;
                    let addr = exc.ExceptionRecord.ExceptionAddress as u64;

                    match code {
                        // EXCEPTION_BREAKPOINT
                        0x80000003 => {
                            let bp_id = self.on_breakpoint_hit(addr)?;
                            unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
                            *self.state.lock().unwrap() = SessionState::Paused(PauseReason::Breakpoint(bp_id));
                            return Ok(ExecutionEvent {
                                kind: if is_step { ExecutionEventKind::StepComplete } else { ExecutionEventKind::BreakpointHit },
                                thread_id: tid as u64,
                                location: self.pdb_address_to_source(addr.saturating_sub(1)),
                            });
                        }
                        // EXCEPTION_SINGLE_STEP
                        0x80000004 => {
                            unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
                            *self.state.lock().unwrap() = SessionState::Paused(PauseReason::Step);
                            return Ok(ExecutionEvent {
                                kind: ExecutionEventKind::StepComplete,
                                thread_id: tid as u64,
                                location: self.pdb_address_to_source(addr),
                            });
                        }
                        // Rust abort / unhandled exception — treat as panic
                        _ if exc.dwFirstChance == 0 => {
                            let msg = format!("unhandled exception 0x{:08X} at 0x{:x}", code, addr);
                            warn!("{}", msg);
                            unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_EXCEPTION_NOT_HANDLED).ok(); }
                            *self.state.lock().unwrap() = SessionState::Paused(PauseReason::Panic);
                            return Ok(ExecutionEvent {
                                kind: ExecutionEventKind::PanicDetected { message: msg },
                                thread_id: tid as u64,
                                location: self.pdb_address_to_source(addr),
                            });
                        }
                        _ => {
                            unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_EXCEPTION_NOT_HANDLED).ok(); }
                        }
                    }
                }
                EXIT_PROCESS_DEBUG_EVENT => {
                    let exit_code = unsafe { event.u.ExitProcess.dwExitCode };
                    *self.state.lock().unwrap() = SessionState::Terminated(exit_code as i32);
                    unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
                    return Ok(ExecutionEvent {
                        kind: ExecutionEventKind::Terminated { exit_code: exit_code as i32 },
                        thread_id: tid as u64,
                        location: None,
                    });
                }
                _ => {
                    unsafe { ContinueDebugEvent(pid, tid, windows::Win32::Foundation::DBG_CONTINUE).ok(); }
                }
            }
        }
    }

    fn on_breakpoint_hit(&self, rip_after_int3: u64) -> Result<BreakpointId, DebuggerError> {
        let patch_addr = rip_after_int3.saturating_sub(1);
        let mut bps = self.breakpoints.lock().unwrap();
        if let Some((&id, p)) = bps.iter().find(|(_, p)| p.address == patch_addr) {
            let original = p.original_byte;
            drop(bps);
            let process = self.require_process()?;
            self.write_byte(process, patch_addr, original)?;
            let mut bps = self.breakpoints.lock().unwrap();
            if let Some(p) = bps.get_mut(&id) {
                p.bp.increment_hit_count();
            }
            Ok(id)
        } else {
            Ok(0) // system/injected breakpoint
        }
    }

    fn resolve_address(&self, kind: &BreakpointKind) -> Result<u64, DebuggerError> {
        match kind {
            BreakpointKind::Address { addr } => Ok(*addr),
            BreakpointKind::SourceLine { file, line } =>
                self.pdb_source_to_address(file, *line),
            BreakpointKind::FunctionName { name } =>
                self.pdb_function_to_address(name),
            BreakpointKind::Regex { .. } =>
                Err(DebuggerError::DebuggerError("regex breakpoints not yet supported".into())),
        }
    }

    // ── PDB integration stubs (Phase 1 — full impl in follow-up PR) ──────────

    fn pdb_address_to_source(&self, _addr: u64) -> Option<SourceLocation> {
        // TODO: pdb crate AddressMap + LineIterator lookup
        None
    }

    fn pdb_source_to_address(&self, _file: &std::path::Path, _line: u32) -> Result<u64, DebuggerError> {
        // TODO: pdb crate line info → module-relative offset → VA
        Err(DebuggerError::DebuggerError(
            "source-line breakpoints require PDB integration (next PR)".into()
        ))
    }

    fn pdb_function_to_address(&self, name: &str) -> Result<u64, DebuggerError> {
        // TODO: pdb crate public symbol lookup
        Err(DebuggerError::DebuggerError(
            format!("function breakpoint '{}' requires PDB integration (next PR)", name)
        ))
    }

    fn enumerate_threads(&self, pid: u32) -> Result<Vec<ThreadInfo>, DebuggerError> {
        let mut threads = Vec::new();
        // SAFETY: CreateToolhelp32Snapshot creates a point-in-time snapshot.
        // TH32CS_SNAPTHREAD is a documented, safe flag.
        // The HANDLE must be closed with CloseHandle (done below).
        // GitHub issue: #1 — Safe alternative: none (Win32 has no safe thread enumeration).
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
            .map_err(|e| DebuggerError::DebuggerError(format!("CreateToolhelp32Snapshot: {}", e)))?;

        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };

        // SAFETY: Thread32First/Next iterate snapshot entries. entry.dwSize is set correctly.
        // GitHub issue: #1
        let mut ok = unsafe { Thread32First(snap, &mut entry) };
        while ok.is_ok() {
            if entry.th32OwnerProcessID == pid {
                threads.push(ThreadInfo {
                    id: entry.th32ThreadID as u64,
                    name: None,
                    state: ThreadState::Stopped,
                    stop_reason: None,
                    frame_count: 0,
                });
            }
            ok = unsafe { Thread32Next(snap, &mut entry) };
        }

        // SAFETY: CloseHandle releases the snapshot HANDLE. GitHub issue: #1
        unsafe { CloseHandle(snap).ok(); }
        Ok(threads)
    }
}

impl Drop for WindowsDebugBackend {
    fn drop(&mut self) {
        if let Some(handle) = *self.process.lock().unwrap() {
            // SAFETY: CloseHandle frees the process HANDLE on backend destruction.
            // GitHub issue: #1
            unsafe { CloseHandle(handle).ok(); }
        }
    }
}
