use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use runtime_core::breakpoint::{Breakpoint, BreakpointKind};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SetBreakpointInput {
    SourceLine { file: PathBuf, line: u32, condition: Option<String> },
    FunctionName { name: String, condition: Option<String> },
}

impl SetBreakpointInput {
    pub fn into_kind_and_condition(self) -> (BreakpointKind, Option<String>) {
        match self {
            Self::SourceLine { file, line, condition } => (BreakpointKind::SourceLine { file, line }, condition),
            Self::FunctionName { name, condition } => (BreakpointKind::FunctionName { name }, condition),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RemoveBreakpointInput {
    pub id: u32,
}

#[derive(Debug, Serialize)]
pub struct BreakpointOutput {
    pub breakpoint: Breakpoint,
}

#[derive(Debug, Serialize)]
pub struct ListBreakpointsOutput {
    pub breakpoints: Vec<Breakpoint>,
}
