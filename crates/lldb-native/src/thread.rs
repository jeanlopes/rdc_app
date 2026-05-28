use std::cell::Cell;
use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use lldb_safe::{Debugger, Listener, Process, State, Target};
use tracing::{error, info};

use runtime_core::{
    breakpoint::{Breakpoint as CoreBreakpoint, BreakpointKind},
    error::DebuggerError,
    event::{ExecutionEvent, ExecutionEventKind},
    process::{StackFrame, ThreadId, ThreadInfo},
    session::{DebugTarget, SessionState},
    variable::{EvalResult, Variable},
};

use crate::command::LldbCommand;
use crate::mapping;
use crate::pdb_resolver::PdbResolver;

pub(crate) struct LldbDebugThread {
    debugger: Debugger,
    /// Listener for async process events.  Present whenever the debugger runs in
    /// async mode (always, after `Debugger::create(true)`).
    listener: Option<Listener>,
    target: Option<Target>,
    process: Option<Process>,
    /// PDB resolver used when LLDB's built-in NativePDB reader fails (LLDB 19.1.7 bug).
    pdb_resolver: Option<PdbResolver>,
    /// Maps breakpoint id → virtual address for breakpoints set by address.
    breakpoint_addresses: HashMap<u32, u64>,
    /// Set when a step-over times out (process blocked waiting for I/O).
    /// All subsequent step commands return an error immediately until Continue clears this.
    blocked_waiting_for_input: Cell<bool>,
}

impl LldbDebugThread {
    fn new(debugger: Debugger, listener: Option<Listener>) -> Self {
        Self {
            debugger,
            listener,
            target: None,
            process: None,
            pdb_resolver: None,
            breakpoint_addresses: HashMap::new(),
            blocked_waiting_for_input: Cell::new(false),
        }
    }

    /// Entry point for the dedicated LLDB OS thread.
    pub(crate) fn run(rx: mpsc::Receiver<LldbCommand>) {
        let dll_dir = std::env::var("LLDB_DLL_DIR").unwrap_or_default();
        if !dll_dir.is_empty() {
            lldb_safe::set_lldb_dll_dir(std::path::Path::new(&dll_dir));
        }

        Debugger::initialize();
        info!("LLDB initialized");

        // Async mode: LLDB delivers process events through its listener rather than
        // blocking each SB call.  This lets us use WaitForEvent + GetRestartedFromEvent
        // to distinguish user-visible stops from internal LLDB stops (DLL loads, TLS, etc.).
        let debugger = match Debugger::create(true) {
            Some(d) => d,
            None => {
                error!("Failed to create LLDB Debugger");
                Debugger::terminate();
                return;
            }
        };

        let listener = debugger.get_listener();
        if listener.is_none() {
            error!("Failed to obtain LLDB default listener — event-based waiting will not work");
        }

        let mut worker = LldbDebugThread::new(debugger, listener);

        loop {
            let cmd = match rx.recv() {
                Ok(c) => c,
                Err(_) => break,
            };
            match cmd {
                LldbCommand::LaunchProcess { target, reply } => {
                    let _ = reply.send(worker.handle_launch_process(target));
                }
                LldbCommand::AttachToPid { pid, reply } => {
                    let _ = reply.send(worker.handle_attach_to_pid(pid));
                }
                LldbCommand::GetState { reply } => {
                    let _ = reply.send(worker.handle_get_state());
                }
                LldbCommand::SetBreakpoint { kind, condition, reply } => {
                    let _ = reply.send(worker.handle_set_breakpoint(kind, condition));
                }
                LldbCommand::RemoveBreakpoint { id, reply } => {
                    let _ = reply.send(worker.handle_remove_breakpoint(id));
                }
                LldbCommand::ListBreakpoints { reply } => {
                    let _ = reply.send(worker.handle_list_breakpoints());
                }
                LldbCommand::Continue { reply } => {
                    let _ = reply.send(worker.handle_continue());
                }
                LldbCommand::Pause { reply } => {
                    let _ = reply.send(worker.handle_pause());
                }
                LldbCommand::StepOver { thread_id, reply } => {
                    let _ = reply.send(worker.handle_step(StepKind::Over, thread_id));
                }
                LldbCommand::StepInto { thread_id, reply } => {
                    let _ = reply.send(worker.handle_step(StepKind::Into, thread_id));
                }
                LldbCommand::StepOut { thread_id, reply } => {
                    let _ = reply.send(worker.handle_step(StepKind::Out, thread_id));
                }
                LldbCommand::ReadLocals { thread_id, frame_index, probe_context: _, max_depth: _, reply } => {
                    let _ = reply.send(worker.handle_read_locals(thread_id, frame_index));
                }
                LldbCommand::ReadStack { thread_id, max_frames, reply } => {
                    let _ = reply.send(worker.handle_read_stack(thread_id, max_frames));
                }
                LldbCommand::EvaluateExpr { expression, thread_id, frame_index, reply } => {
                    let _ = reply.send(worker.handle_evaluate_expression(expression, thread_id, frame_index));
                }
                LldbCommand::ListThreads { reply } => {
                    let _ = reply.send(worker.handle_list_threads());
                }
            }
        }

        info!("LLDB thread shutting down");
        drop(worker);
        Debugger::terminate();
    }

    fn process(&self) -> Result<&Process, DebuggerError> {
        self.process.as_ref().ok_or(DebuggerError::ProcessNotFound)
    }

    fn target(&self) -> Result<&Target, DebuggerError> {
        self.target.as_ref().ok_or(DebuggerError::ProcessNotFound)
    }

    /// Block on the event listener until the process reaches a user-visible stop
    /// (breakpoint, step complete, crash, exit) or `timeout_secs` elapses.
    ///
    /// Internal LLDB stops (DLL loads, TLS callbacks, thread-create events) are
    /// identified by `GetRestartedFromEvent() == true` and are skipped transparently —
    /// LLDB already auto-continued the process for those.
    fn wait_for_user_stop(listener: &Listener, timeout_secs: u32) -> State {
        let deadline = Instant::now() + Duration::from_secs(u64::from(timeout_secs));
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                info!("wait_for_user_stop: timed out after {}s", timeout_secs);
                return State::Invalid;
            }
            // Poll in 5-second slices so we can recheck the deadline.
            let slice = remaining.as_secs().min(5).max(1) as u32;
            let event = match listener.wait_for_event(slice) {
                Some(e) => e,
                None => continue,
            };
            if !Process::is_process_event(&event) {
                continue;
            }
            let state = Process::state_from_event(&event);
            let restarted = Process::restarted_from_event(&event);
            info!(state = ?state, restarted, "process event");
            match state {
                // User-visible stop: breakpoint, step, manual pause, entry-point stop.
                State::Stopped if !restarted => return State::Stopped,
                // Terminal states always surface immediately.
                State::Exited | State::Detached | State::Crashed => return state,
                // Internal stops (restarted=true), Running, Launching, etc. — loop.
                _ => continue,
            }
        }
    }

    fn handle_launch_process(&mut self, target: DebugTarget) -> Result<(u32, SessionState), DebuggerError> {
        let exe = target.executable.to_string_lossy().to_string();
        info!(exe, "LLDB {} — creating target", lldb_safe::Debugger::version_string());

        let lldb_target = self.debugger.create_target_simple(&exe)
            .ok_or_else(|| DebuggerError::DebuggerError(format!("failed to create target for {}", exe)))?;

        // LLDB 19.1.7 on Windows has a broken SymbolFileNativePDB reader.  Work around
        // by parsing PDB ourselves with the `pdb` crate and resolving breakpoints by VA.
        let exe_path = std::path::Path::new(&exe);
        let pdb_path = exe_path.with_extension("pdb");
        if pdb_path.exists() {
            match PdbResolver::open(&pdb_path) {
                Some(resolver) => {
                    info!(pdb = %pdb_path.display(), "PDB opened with pdb-crate resolver");
                    self.pdb_resolver = Some(resolver);
                }
                None => {
                    info!(pdb = %pdb_path.display(), "PDB resolver failed to open");
                }
            }
        } else {
            info!(pdb = %pdb_path.display(), "PDB not found at expected path");
        }

        let argv: Vec<&str> = target.args.iter().map(|s| s.as_str()).collect();
        let cwd = target.working_dir.as_ref().and_then(|p| p.to_str()).map(|s| s.to_string());

        let process = lldb_target
            .launch(&argv, &[], cwd.as_deref(), true)
            .map_err(|e| DebuggerError::DebuggerError(e.to_string()))?;

        // Subscribe the debugger's listener to this process's state-change events.
        // eBroadcastBitStateChanged = 0x01
        if let Some(ref listener) = self.listener {
            if let Some(broadcaster) = process.get_broadcaster() {
                let subscribed = broadcaster.add_listener(listener, 0x01);
                info!(subscribed, "subscribed listener to process broadcaster");
            }
        }

        // Wait for the initial entry-point stop.
        let final_state = if let Some(ref listener) = self.listener {
            Self::wait_for_user_stop(listener, 30)
        } else {
            // Fallback to polling if we somehow have no listener.
            Self::poll_for_stop(&process, 30)
        };

        let pid = process.pid() as u32;
        let state = mapping::state_to_session_state(final_state, &process);

        let modules = self.debugger.handle_command("image list");
        info!("image list:\n{}", modules.trim());

        if let Some(ref mut resolver) = self.pdb_resolver {
            let exe_name = exe_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&exe);
            resolver.apply_module_base_from_image_list(&modules, exe_name);
        }

        self.target = Some(lldb_target);
        self.process = Some(process);

        info!(pid, "process launched and stopped at entry");
        Ok((pid, state))
    }

    fn handle_attach_to_pid(&mut self, pid: u64) -> Result<(u64, SessionState), DebuggerError> {
        let lldb_target = self.debugger.create_target_simple("")
            .ok_or_else(|| DebuggerError::DebuggerError("failed to create target for attach".into()))?;
        let process = lldb_target.attach_to_pid(pid)
            .map_err(|e| DebuggerError::DebuggerError(e.to_string()))?;

        if let Some(ref listener) = self.listener {
            if let Some(broadcaster) = process.get_broadcaster() {
                broadcaster.add_listener(listener, 0x01);
            }
        }

        let final_state = if let Some(ref listener) = self.listener {
            Self::wait_for_user_stop(listener, 30)
        } else {
            Self::poll_for_stop(&process, 30)
        };

        let state = mapping::state_to_session_state(final_state, &process);
        self.target = Some(lldb_target);
        self.process = Some(process);
        Ok((pid, state))
    }

    fn handle_get_state(&self) -> Result<SessionState, DebuggerError> {
        let process = self.process()?;
        Ok(mapping::state_to_session_state(process.state(), process))
    }

    fn handle_set_breakpoint(&mut self, kind: BreakpointKind, condition: Option<String>) -> Result<CoreBreakpoint, DebuggerError> {
        let target = self.target()?;
        let mut bp_va: Option<u64> = None;

        let lldb_bp = match &kind {
            BreakpointKind::SourceLine { file, line } => {
                let filename = file.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(file.to_str().unwrap_or(""));
                let full_path = file.to_str().unwrap_or(filename);
                info!(filename, line, full_path, "setting source-line breakpoint");

                if let Some(va) = self.pdb_resolver.as_ref().and_then(|r| r.source_to_va(filename, *line)) {
                    info!(filename, line, va = format!("{va:#x}"), "resolved via PDB resolver — setting by address");
                    bp_va = Some(va);
                    target.breakpoint_by_address(va)
                } else {
                    target.breakpoint_by_location(full_path, *line)
                }
            }
            BreakpointKind::FunctionName { name } => {
                info!(name, "setting function breakpoint");
                target.breakpoint_by_name(name, None)
            }
            BreakpointKind::Address { addr } => {
                info!(addr, "setting address breakpoint");
                bp_va = Some(*addr);
                target.breakpoint_by_address(*addr)
            }
            BreakpointKind::Regex { pattern } => {
                info!(pattern, "setting regex breakpoint");
                target.breakpoint_by_name(pattern, None)
            }
        }.ok_or_else(|| DebuggerError::DebuggerError("failed to set breakpoint".into()))?;

        if let Some(cond) = &condition {
            lldb_bp.set_condition(cond);
        }

        let num_locations = lldb_bp.num_locations();
        if num_locations == 0 {
            info!(bp_id = lldb_bp.id(), "breakpoint resolved 0 locations — will not trigger");
            if let BreakpointKind::SourceLine { file, line } = &kind {
                let filename = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let cmd = format!("image lookup -verbose -file {} -line {}", filename, line);
                let lookup = self.debugger.handle_command(&cmd);
                info!("image lookup verbose:\n{}", lookup.trim());
            }
        } else {
            info!(bp_id = lldb_bp.id(), num_locations, "breakpoint set and resolved");
        }

        if let Some(va) = bp_va {
            info!(bp_id = lldb_bp.id(), va = format!("{va:#x}"), "tracking breakpoint address");
            self.breakpoint_addresses.insert(lldb_bp.id(), va);
        }

        Ok(mapping::lldb_bp_to_core(&lldb_bp, kind, condition))
    }

    fn handle_remove_breakpoint(&mut self, id: u32) -> Result<(), DebuggerError> {
        let target = self.target()?;
        if target.delete_breakpoint(id) {
            if let Some(va) = self.breakpoint_addresses.remove(&id) {
                info!(bp_id = id, va = format!("{va:#x}"), "breakpoint removed");
            } else {
                info!(bp_id = id, "breakpoint removed");
            }
            Ok(())
        } else {
            Err(DebuggerError::BreakpointNotFound(id))
        }
    }

    fn handle_list_breakpoints(&self) -> Result<Vec<CoreBreakpoint>, DebuggerError> {
        let target = self.target()?;
        let n = target.num_breakpoints();
        let mut bps = Vec::with_capacity(n as usize);
        for i in 0..n {
            if let Some(bp) = target.breakpoint_at(i) {
                bps.push(mapping::lldb_bp_to_core(
                    &bp,
                    BreakpointKind::Address { addr: 0 },
                    None,
                ));
            }
        }
        Ok(bps)
    }

    fn handle_continue(&self) -> Result<ExecutionEvent, DebuggerError> {
        self.blocked_waiting_for_input.set(false);
        let process = self.process()?;

        process.resume().map_err(|e| DebuggerError::DebuggerError(e.to_string()))?;

        // Wait indefinitely for a user-visible stop (breakpoint, crash, exit).
        // Use short sub-timeouts so we can log a heartbeat — but never give up:
        // the process may be an interactive program waiting on stdin for minutes.
        let final_state = loop {
            let state = if let Some(ref listener) = self.listener {
                Self::wait_for_user_stop(listener, 30)
            } else {
                Self::poll_for_stop(process, 30)
            };
            if !matches!(state, State::Invalid) {
                break state;
            }
            // Timed out — process still running.  Keep waiting.
            info!("continue: process still running — waiting for stop...");
        };

        info!(state = ?final_state, num_threads = process.num_threads(), "user-visible stop");

        let kind = match final_state {
            State::Exited => ExecutionEventKind::Terminated { exit_code: process.exit_status() },
            State::Crashed => ExecutionEventKind::Paused,
            _ => {
                let thread = process.selected_thread()
                    .and_then(|t| {
                        if matches!(t.stop_reason(), lldb_safe::StopReason::None | lldb_safe::StopReason::Invalid) {
                            None
                        } else {
                            Some(t)
                        }
                    })
                    .or_else(|| {
                        (0..process.num_threads() as usize)
                            .filter_map(|i| process.thread_at(i))
                            .find(|t| !matches!(
                                t.stop_reason(),
                                lldb_safe::StopReason::None | lldb_safe::StopReason::Invalid
                            ))
                    });

                if let Some(ref t) = thread {
                    match t.stop_reason() {
                        lldb_safe::StopReason::Breakpoint => ExecutionEventKind::BreakpointHit,
                        lldb_safe::StopReason::Trace | lldb_safe::StopReason::PlanComplete => ExecutionEventKind::StepComplete,
                        _ => {
                            // Fallback: LLDB 19.1.7 may not set StopReason for address BPs.
                            let pc = t.frame_at(0).map(|f| f.pc()).unwrap_or(0);
                            if self.breakpoint_addresses.values().any(|&va| va == pc) {
                                info!(pc = format!("{pc:#x}"), "breakpoint hit via PC match (LLDB StopReason missing)");
                                ExecutionEventKind::BreakpointHit
                            } else {
                                ExecutionEventKind::Paused
                            }
                        }
                    }
                } else {
                    // No thread with a stop reason.  Check all threads' PCs.
                    let pc_hit = (0..process.num_threads() as usize)
                        .filter_map(|i| process.thread_at(i))
                        .any(|t| {
                            t.frame_at(0)
                                .map(|f| self.breakpoint_addresses.values().any(|&va| va == f.pc()))
                                .unwrap_or(false)
                        });
                    if pc_hit {
                        ExecutionEventKind::BreakpointHit
                    } else {
                        ExecutionEventKind::Paused
                    }
                }
            }
        };

        let thread = process.selected_thread();
        let event = match &thread {
            Some(t) => mapping::build_execution_event(t, kind, self.pdb_resolver.as_ref()),
            None => ExecutionEvent { kind, thread_id: 0, location: None },
        };
        Ok(event)
    }

    fn handle_pause(&self) -> Result<ExecutionEvent, DebuggerError> {
        let process = self.process()?;
        process.stop().map_err(|e| DebuggerError::DebuggerError(e.to_string()))?;
        let thread = process.selected_thread();
        let event = match &thread {
            Some(t) => mapping::build_execution_event(t, ExecutionEventKind::Paused, self.pdb_resolver.as_ref()),
            None => ExecutionEvent { kind: ExecutionEventKind::Paused, thread_id: 0, location: None },
        };
        Ok(event)
    }

    fn handle_step(&self, kind: StepKind, _thread_id: Option<ThreadId>) -> Result<ExecutionEvent, DebuggerError> {
        let process = self.process()?;
        let stepped_thread = process.selected_thread()
            .ok_or_else(|| DebuggerError::DebuggerError("no selected thread".into()))?;

        // If a previous step timed out (process was waiting for I/O), refuse all
        // further steps until the user presses Continue.  This avoids calling any
        // LLDB APIs on a thread that may be stopped inside a blocking kernel call,
        // which would either hang or step through unrelated native code.
        if self.blocked_waiting_for_input.get() {
            info!("step blocked: process is waiting for user input (blocked_waiting_for_input=true)");
            return Err(DebuggerError::DebuggerError(
                "Cannot step: process is waiting for user input. \
                 Provide input in the target terminal, then press Continue."
                    .into(),
            ));
        }

        // StepOver via temp BP: LLDB 19.1.7 on Windows can't step by source line
        // (SymbolFileNativePDB is broken), so step_over() only advances one instruction.
        // Instead, look up the next source line's VA via PDB, set a temp breakpoint
        // there, and resume — this correctly steps over function calls too.
        if matches!(kind, StepKind::Over) {
            let next_va = self.pdb_resolver.as_ref().and_then(|r| {
                let frame = stepped_thread.frame_at(0)?;
                let pc = frame.pc();
                let (file, line) = r.va_to_source(pc)?;
                let basename = file.file_name()?.to_str()?.to_owned();
                let va = r.next_source_line_va(pc, &basename, line);
                info!(
                    current_line = line,
                    next_va = va.map(|v| format!("{v:#x}")).as_deref().unwrap_or("None"),
                    "step_over: PDB next-line lookup"
                );
                va
            });

            if let Some(next_va) = next_va {
                // Diagnostic: what source location does the temp BP address resolve to?
                if let Some(ref r) = self.pdb_resolver {
                    let resolved = r.va_to_source(next_va);
                    info!(next_va = format!("{next_va:#x}"), resolved = ?resolved, "step_over: temp BP target");
                }

                let target = self.target()?;
                let temp_bp = target.breakpoint_by_address(next_va)
                    .ok_or_else(|| DebuggerError::DebuggerError(
                        format!("step_over: failed to set temp BP at {next_va:#x}")
                    ))?;
                let temp_id = temp_bp.id();
                info!(temp_id, next_va = format!("{next_va:#x}"), "step_over: temp BP set");

                if let Err(e) = process.resume() {
                    target.delete_breakpoint(temp_id);
                    return Err(DebuggerError::DebuggerError(e.to_string()));
                }

                let state = if let Some(ref listener) = self.listener {
                    Self::wait_for_user_stop(listener, 5)
                } else {
                    Self::poll_for_stop(process, 5)
                };
                info!(state = ?state, "step_over (temp BP) completed");
                target.delete_breakpoint(temp_id);

                // If timed out: the process is likely blocked waiting for user input.
                // Interrupt it to restore Stopped state, then return an error so the UI
                // shows a clear warning without navigating to foreign (kernel/CRT) code.
                if matches!(state, State::Invalid) {
                    info!("step_over timed out — process likely waiting for user input; interrupting");
                    let _ = process.stop();
                    let recovered = if let Some(ref listener) = self.listener {
                        Self::wait_for_user_stop(listener, 10)
                    } else {
                        Self::poll_for_stop(process, 10)
                    };
                    info!(recovered = ?recovered, "process state after interrupt");
                    self.blocked_waiting_for_input.set(true);
                    return Err(DebuggerError::DebuggerError(
                        "Step over cancelled: process is waiting for user input. \
                         Provide input in the target terminal, then press Continue."
                            .into(),
                    ));
                }

                let post_thread = process.selected_thread()
                    .and_then(|t| if t.num_frames() > 0 { Some(t) } else { None })
                    .or(Some(stepped_thread));

                if let Some(ref t) = post_thread {
                    if let Some(f) = t.frame_at(0) {
                        info!(thread_id = t.thread_id(), pc = format!("{:#x}", f.pc()), "step_over post-thread");
                    }
                }

                let event = match state {
                    State::Exited => ExecutionEvent {
                        kind: ExecutionEventKind::Terminated { exit_code: process.exit_status() },
                        thread_id: 0,
                        location: None,
                    },
                    _ => match &post_thread {
                        Some(t) => mapping::build_execution_event(t, ExecutionEventKind::StepComplete, self.pdb_resolver.as_ref()),
                        None => ExecutionEvent { kind: ExecutionEventKind::StepComplete, thread_id: 0, location: None },
                    },
                };
                info!(location = ?event.location, "step_over event built");
                return Ok(event);
            }

            info!("step_over: no PDB next-line VA — falling back to LLDB native step_over");
        }

        // Native LLDB stepping (StepInto, StepOut, or StepOver without PDB next-line)
        match kind {
            StepKind::Over => stepped_thread.step_over(),
            StepKind::Into => stepped_thread.step_into(),
            StepKind::Out => stepped_thread.step_out(),
        }

        let state = if let Some(ref listener) = self.listener {
            Self::wait_for_user_stop(listener, 30)
        } else {
            Self::poll_for_stop(process, 30)
        };
        info!(state = ?state, "step completed");

        let post_thread = process.selected_thread()
            .and_then(|t| if t.num_frames() > 0 { Some(t) } else { None })
            .or(Some(stepped_thread));

        if let Some(ref t) = post_thread {
            if let Some(f) = t.frame_at(0) {
                info!(thread_id = t.thread_id(), pc = format!("{:#x}", f.pc()), "step post-thread");
            }
        }

        let event = match &post_thread {
            Some(t) => mapping::build_execution_event(t, ExecutionEventKind::StepComplete, self.pdb_resolver.as_ref()),
            None => ExecutionEvent { kind: ExecutionEventKind::StepComplete, thread_id: 0, location: None },
        };
        info!(location = ?event.location, "step event built");
        Ok(event)
    }

    fn handle_read_locals(&self, _thread_id: Option<ThreadId>, frame_index: u32) -> Result<Vec<Variable>, DebuggerError> {
        let process = self.process()?;
        let thread = process.selected_thread()
            .ok_or_else(|| DebuggerError::DebuggerError("no selected thread".into()))?;
        let frame = thread.frame_at(frame_index)
            .ok_or_else(|| DebuggerError::DebuggerError(format!("frame {} not found", frame_index)))?;

        let vars: Vec<Variable> = frame.variables(true, true, false)
            .iter()
            .map(mapping::value_to_variable)
            .collect();
        Ok(vars)
    }

    fn handle_read_stack(&self, _thread_id: Option<ThreadId>, max_frames: u32) -> Result<Vec<StackFrame>, DebuggerError> {
        let process = self.process()?;
        let thread = process.selected_thread()
            .ok_or_else(|| DebuggerError::DebuggerError("no selected thread".into()))?;

        let n = thread.num_frames().min(max_frames);
        let frames: Vec<StackFrame> = (0..n)
            .filter_map(|i| thread.frame_at(i).map(|f| mapping::frame_to_stack_frame(&f, i)))
            .collect();
        Ok(frames)
    }

    fn handle_evaluate_expression(&self, expr: String, _thread_id: Option<ThreadId>, frame_index: u32) -> Result<EvalResult, DebuggerError> {
        let process = self.process()?;
        let thread = process.selected_thread()
            .ok_or_else(|| DebuggerError::DebuggerError("no selected thread".into()))?;
        let frame = thread.frame_at(frame_index)
            .ok_or_else(|| DebuggerError::DebuggerError(format!("frame {} not found", frame_index)))?;

        let value = frame.evaluate_expression(&expr)
            .ok_or_else(|| DebuggerError::EvalError { expression: expr.clone(), message: "expression evaluation failed".into() })?;
        Ok(mapping::value_to_eval_result(expr, &value))
    }

    fn handle_list_threads(&self) -> Result<Vec<ThreadInfo>, DebuggerError> {
        let process = self.process()?;
        let n = process.num_threads();
        let threads: Vec<ThreadInfo> = (0..n as usize)
            .filter_map(|i| process.thread_at(i).map(|t| mapping::thread_info(&t)))
            .collect();
        Ok(threads)
    }

    /// Polling fallback used only if the debugger listener is unavailable.
    fn poll_for_stop(process: &Process, timeout_secs: u32) -> State {
        let deadline = Instant::now() + Duration::from_secs(u64::from(timeout_secs));
        loop {
            let state = process.state();
            if state.is_stopped() || matches!(state, State::Exited | State::Detached) {
                return state;
            }
            if Instant::now() >= deadline {
                return state;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

enum StepKind { Over, Into, Out }
