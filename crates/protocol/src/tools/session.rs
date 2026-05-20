use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use runtime_core::session::SessionState;

#[derive(Debug, Deserialize)]
pub struct LaunchInput {
    pub executable: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub working_dir: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct LaunchOutput {
    pub session_id: String,
    pub pid: u32,
    pub state: SessionState,
}

#[derive(Debug, Serialize)]
pub struct SessionStateOutput {
    pub session_id: String,
    pub state: SessionState,
    pub pid: Option<u32>,
    pub selected_thread: Option<u64>,
}
