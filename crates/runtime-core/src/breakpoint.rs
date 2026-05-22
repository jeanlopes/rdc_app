// T032, T033 — stub
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::process::SourceLocation;

pub type BreakpointId = u32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BreakpointKind {
    SourceLine { file: PathBuf, line: u32 },
    FunctionName { name: String },
    Address { addr: u64 },
    Regex { pattern: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakpointLocation {
    pub address: u64,
    pub source_location: Option<SourceLocation>,
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breakpoint {
    pub id: BreakpointId,
    pub kind: BreakpointKind,
    pub condition: Option<String>,
    pub hit_count: u32,
    pub enabled: bool,
    pub locations: Vec<BreakpointLocation>,
}

impl Breakpoint {
    pub fn increment_hit_count(&mut self) {
        self.hit_count += 1;
    }

    pub fn toggle_enabled(&mut self) {
        self.enabled = !self.enabled;
    }
}
