use std::sync::mpsc;
use std::time::Duration;

use lldb_safe::{Debugger, Process, State, Target};
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
    target: Option<Target>,
    process: Option<Process>,
    /// PDB resolver used when LLDB's built-in NativePDB reader fails (LLDB 19.1.7 bug).
    pdb_resolver: Option<PdbResolver>,
}

impl LldbDebugThread {
    fn new(debugger: Debugger) -> Self {
        Self { debugger, target: None, process: None, pdb_resolver: None }
    }

    /// Entry point for the dedicated LLDB OS thread.
    pub(crate) fn run(rx: mpsc::Receiver<LldbCommand>) {
        let dll_dir = std::env::var("LLDB_DLL_DIR").unwrap_or_default();
        if !dll_dir.is_empty() {
            lldb_safe::set_lldb_dll_dir(std::path::Path::new(&dll_dir));
        }

        Debugger::initialize();
        info!("LLDB initialized");

        let debugger = match Debugger::create(false) {
            Some(d) => d,
            None => {
                error!("Failed to create LLDB Debugger");
                Debugger::terminate();
                return;
            }
        };

        let mut worker = LldbDebugThread::new(debugger);

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

    // After calling resume(), the process state briefly remains Stopped before
    // LLDB transitions it through Invalid → Running.  If wait_for_stop() is called
    // immediately, it sees the stale Stopped state and returns before the process
    // has actually started, causing the next resume() to fail with "process still running".
    // This function spins until the state is actually Running/Stepping or terminal.
    fn wait_for_running(process: &Process) {
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        while std::time::Instant::now() < deadline {
            match process.state() {
                State::Running | State::Stepping => return,
                State::Exited | State::Detached | State::Crashed => return,
                _ => std::thread::sleep(Duration::from_millis(2)),
            }
        }
    }

    fn wait_for_stop(process: &Process) -> State {
        // Timeout after 30 s to avoid spinning forever if LLDB gets wedged.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut last_state = State::Invalid;
        loop {
            let state = process.state();
            if state != last_state {
                info!(state = ?state, "wait_for_stop: state transition");
                last_state = state;
            }
            // Only exit when we know the process is truly done or stopped.
            // State::Invalid must NOT exit the loop — on Windows, Continue() may be
            // non-blocking and the state is briefly Invalid before transitioning to Running.
            if state.is_stopped() || matches!(state, State::Exited | State::Detached) {
                return state;
            }
            if std::time::Instant::now() >= deadline {
                info!(state = ?state, "wait_for_stop: timed out after 30s");
                return state;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn handle_launch_process(&mut self, target: DebugTarget) -> Result<(u32, SessionState), DebuggerError> {
        let exe = target.executable.to_string_lossy().to_string();
        info!(exe, "LLDB {} — creating target", lldb_safe::Debugger::version_string());

        let lldb_target = self.debugger.create_target_simple(&exe)
            .ok_or_else(|| DebuggerError::DebuggerError(format!("failed to create target for {}", exe)))?;

        // LLDB 19.1.7 on Windows has a broken SymbolFileNativePDB reader that
        // never loads PDB symbols (UUID comparison bug). We work around this by
        // parsing the PDB ourselves with the `pdb` crate and resolving source-line
        // breakpoints by virtual address instead of by source location.
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

        let _ = Self::wait_for_stop(&process);
        let pid = process.pid() as u32;
        let state = mapping::state_to_session_state(process.state(), &process);

        let modules = self.debugger.handle_command("image list");
        info!("image list:\n{}", modules.trim());

        // Apply the actual module load base so the PDB resolver can compute VAs.
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
        let _ = Self::wait_for_stop(&process);
        let state = mapping::state_to_session_state(process.state(), &process);
        self.target = Some(lldb_target);
        self.process = Some(process);
        Ok((pid, state))
    }

    fn handle_get_state(&self) -> Result<SessionState, DebuggerError> {
        let process = self.process()?;
        Ok(mapping::state_to_session_state(process.state(), process))
    }

    fn handle_set_breakpoint(&self, kind: BreakpointKind, condition: Option<String>) -> Result<CoreBreakpoint, DebuggerError> {
        let target = self.target()?;
        let lldb_bp = match &kind {
            BreakpointKind::SourceLine { file, line } => {
                let filename = file.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(file.to_str().unwrap_or(""));
                let full_path = file.to_str().unwrap_or(filename);
                info!(filename, line, full_path, "setting source-line breakpoint");

                // Prefer PDB-based address resolution (LLDB 19.1.7 NativePDB is broken).
                if let Some(va) = self.pdb_resolver.as_ref().and_then(|r| r.source_to_va(filename, *line)) {
                    info!(filename, line, va = format!("{va:#x}"), "resolved via PDB resolver — setting by address");
                    target.breakpoint_by_address(va)
                } else {
                    // Fallback: let LLDB try (will likely return 0 locations).
                    target.breakpoint_by_location(full_path, *line)
                }
            }
            BreakpointKind::FunctionName { name } => {
                info!(name, "setting function breakpoint");
                target.breakpoint_by_name(name, None)
            }
            BreakpointKind::Address { addr } => {
                info!(addr, "setting address breakpoint");
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
                // Verbose lookup shows compile unit and symbol info if PDB is parsed.
                let cmd = format!("image lookup -verbose -file {} -line {}", filename, line);
                let lookup = self.debugger.handle_command(&cmd);
                info!("image lookup verbose:\n{}", lookup.trim());
                // Also check what LLDB sees in line tables for this file.
                let cmd2 = format!("image dump line-table {}", filename);
                let lt = self.debugger.handle_command(&cmd2);
                info!("image dump line-table:\n{}", lt.trim());
            }
        } else {
            info!(bp_id = lldb_bp.id(), num_locations, "breakpoint set and resolved");
        }
        Ok(mapping::lldb_bp_to_core(&lldb_bp, kind, condition))
    }

    fn handle_remove_breakpoint(&self, id: u32) -> Result<(), DebuggerError> {
        let target = self.target()?;
        if target.delete_breakpoint(id) {
            info!(bp_id = id, "breakpoint removed");
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
        let process = self.process()?;

        // On Windows, LLDB generates multiple internal Stopped events (DLL loads,
        // TLS callbacks, initial thread-create stops) before reaching a user breakpoint.
        // Each stop has no meaningful thread stop reason.  We auto-continue through
        // those internal stops until we see a user-visible stop, the process exits,
        // or 60 seconds elapse.
        let deadline = std::time::Instant::now() + Duration::from_secs(60);

        let (final_state, thread) = loop {
            process.resume().map_err(|e| DebuggerError::DebuggerError(e.to_string()))?;
            Self::wait_for_running(process);  // wait until state leaves Stopped
            let state = Self::wait_for_stop(process);

            info!(state = ?state, num_threads = process.num_threads(), "process stopped");

            if matches!(state, State::Exited | State::Detached | State::Crashed) {
                break (state, None);
            }

            // Find the first thread with a meaningful stop reason.
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

            if let Some(t) = thread {
                info!(thread_id = t.thread_id(), stop_reason = ?t.stop_reason(), "user-visible stop");
                break (state, Some(t));
            }

            if std::time::Instant::now() >= deadline {
                info!("continue timed out after 60s — surfacing as Paused");
                break (state, None);
            }
            info!("internal stop (no thread reason) — auto-continuing");
        };

        let kind = match final_state {
            State::Exited => ExecutionEventKind::Terminated { exit_code: process.exit_status() },
            State::Crashed => ExecutionEventKind::Paused,
            _ => {
                if let Some(ref t) = thread {
                    match t.stop_reason() {
                        lldb_safe::StopReason::Breakpoint => ExecutionEventKind::BreakpointHit,
                        lldb_safe::StopReason::Trace | lldb_safe::StopReason::PlanComplete => ExecutionEventKind::StepComplete,
                        _ => ExecutionEventKind::Paused,
                    }
                } else {
                    ExecutionEventKind::Paused
                }
            }
        };

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
        let thread = process.selected_thread()
            .ok_or_else(|| DebuggerError::DebuggerError("no selected thread".into()))?;

        match kind {
            StepKind::Over => thread.step_over(),
            StepKind::Into => thread.step_into(),
            StepKind::Out => thread.step_out(),
        }

        let _ = Self::wait_for_stop(process);

        let thread2 = process.selected_thread();
        let event = match &thread2 {
            Some(t) => mapping::build_execution_event(t, ExecutionEventKind::StepComplete, self.pdb_resolver.as_ref()),
            None => ExecutionEvent { kind: ExecutionEventKind::StepComplete, thread_id: 0, location: None },
        };
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
}

enum StepKind { Over, Into, Out }
