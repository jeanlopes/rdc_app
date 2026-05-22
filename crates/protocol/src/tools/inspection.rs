use serde::{Deserialize, Serialize};
use runtime_core::{variable::{Variable, EvalResult}, process::StackFrame, process::ThreadInfo};

#[derive(Debug, Deserialize)]
pub struct ReadLocalsInput {
    pub thread_id: Option<u64>,
    #[serde(default)]
    pub frame_index: u32,
    pub probe_context: Option<String>,
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,
}

fn default_max_depth() -> u32 { 4 }

#[derive(Debug, Serialize)]
pub struct VariableListOutput {
    pub probe_context: Option<String>,
    pub variables: Vec<Variable>,
}

#[derive(Debug, Deserialize)]
pub struct ReadStackInput {
    pub thread_id: Option<u64>,
    #[serde(default = "default_max_frames")]
    pub max_frames: u32,
}

fn default_max_frames() -> u32 { 32 }

#[derive(Debug, Serialize)]
pub struct StackOutput {
    pub thread_id: u64,
    pub frames: Vec<StackFrame>,
}

#[derive(Debug, Deserialize)]
pub struct EvalInput {
    pub expression: String,
    pub thread_id: Option<u64>,
    #[serde(default)]
    pub frame_index: u32,
}

#[derive(Debug, Serialize)]
pub struct EvalOutput {
    pub result: EvalResult,
}

#[derive(Debug, Serialize)]
pub struct ThreadListOutput {
    pub threads: Vec<ThreadInfo>,
    pub selected_thread: u64,
}
