// type conversions: lldb-safe → runtime-core
use lldb_safe::{Breakpoint as LldbBreakpoint, Frame, Process, State, StopReason, Thread, Value};
use runtime_core::{
    breakpoint::{Breakpoint as CoreBreakpoint, BreakpointKind},
    event::{ExecutionEvent, ExecutionEventKind},
    process::{SourceLocation, StackFrame, ThreadInfo, ThreadState},
    session::{PauseReason, SessionState},
    variable::{EvalResult, Variable, VariableValue},
};
use crate::pdb_resolver::PdbResolver;

pub fn state_to_session_state(state: State, process: &Process) -> SessionState {
    match state {
        State::Invalid => SessionState::Error("invalid LLDB state".into()),
        State::Unloaded | State::Connected => SessionState::Idle,
        State::Attaching | State::Launching => SessionState::Launching,
        State::Stopped => {
            let reason = process
                .selected_thread()
                .map(|t| stop_reason_to_pause_reason(&t))
                .unwrap_or(PauseReason::UserRequest);
            SessionState::Paused(reason)
        }
        State::Running => SessionState::Running,
        State::Stepping => SessionState::Stepping,
        State::Crashed => SessionState::Paused(PauseReason::Exception("crashed".into())),
        State::Detached => SessionState::Terminated(0),
        State::Exited => SessionState::Terminated(process.exit_status()),
        State::Suspended => SessionState::Paused(PauseReason::UserRequest),
    }
}

pub fn stop_reason_to_pause_reason(thread: &Thread) -> PauseReason {
    match thread.stop_reason() {
        StopReason::Breakpoint => {
            let bp_id = thread.stop_reason_data_at(0) as u32;
            PauseReason::Breakpoint(bp_id)
        }
        StopReason::Trace | StopReason::PlanComplete => PauseReason::Step,
        StopReason::Signal => PauseReason::Signal(thread.stop_description()),
        StopReason::Exception => PauseReason::Exception(thread.stop_description()),
        _ => PauseReason::UserRequest,
    }
}

pub fn build_execution_event(
    thread: &Thread,
    kind: ExecutionEventKind,
    pdb: Option<&PdbResolver>,
) -> ExecutionEvent {
    let thread_id = thread.thread_id();

    // Try LLDB's own source location first; fall back to PDB resolver if None.
    let location = thread.frame_at(0).and_then(|f| {
        frame_source_location(&f).or_else(|| {
            // LLDB symbols missing (NativePDB bug) — resolve via PDB crate.
            pdb.and_then(|r| {
                let pc = f.pc();
                r.va_to_source(pc).map(|(path, line)| SourceLocation {
                    file: path,
                    line,
                    column: None,
                })
            })
        })
    });

    ExecutionEvent { kind, thread_id, location }
}

pub fn frame_source_location(frame: &Frame) -> Option<SourceLocation> {
    let (path, line) = frame.source_location()?;
    if path.is_empty() || line == 0 {
        return None;
    }
    Some(SourceLocation { file: path.into(), line, column: None })
}

pub fn frame_to_stack_frame(frame: &Frame, index: u32) -> StackFrame {
    StackFrame {
        index,
        function_name: frame.function_name().map(|s| s.to_string()),
        module: None,
        source_location: frame_source_location(frame),
        is_inlined: frame.is_inlined(),
    }
}

pub fn value_to_variable(value: &Value) -> Variable {
    let name = value.name().unwrap_or("").to_string();
    let type_name = value.display_type_name().unwrap_or("").to_string();

    let val = if value.num_children() > 0 {
        VariableValue::Struct { fields: vec![] }
    } else {
        let summary = value.value_string().unwrap_or("").to_string();
        if summary.is_empty() {
            VariableValue::Opaque { summary: format!("0x{:x}", value.as_u64()) }
        } else {
            VariableValue::Opaque { summary }
        }
    };

    Variable {
        name,
        type_name,
        value: val,
        address: Some(value.load_address()),
        semantic: None,
    }
}

pub fn value_to_eval_result(expr: String, value: &Value) -> EvalResult {
    let err = value.error();
    let error_msg = if err.fail() { err.message().map(|s| s.to_string()) } else { None };
    EvalResult {
        expression: expr,
        value: VariableValue::Opaque {
            summary: value.value_string().unwrap_or("").to_string(),
        },
        type_name: value.display_type_name().unwrap_or("").to_string(),
        error: error_msg,
    }
}

pub fn thread_info(thread: &Thread) -> ThreadInfo {
    let state = match thread.stop_reason() {
        StopReason::None | StopReason::Invalid => ThreadState::Running,
        _ => ThreadState::Stopped,
    };
    ThreadInfo {
        id: thread.thread_id(),
        name: thread.name().map(|s| s.to_string()),
        state,
        stop_reason: None,
        frame_count: thread.num_frames() as usize,
    }
}

pub fn lldb_bp_to_core(bp: &LldbBreakpoint, kind: BreakpointKind, condition: Option<String>) -> CoreBreakpoint {
    CoreBreakpoint {
        id: bp.id(),
        kind,
        condition,
        hit_count: bp.hit_count(),
        enabled: bp.is_enabled(),
        locations: vec![],
    }
}
