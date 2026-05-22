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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bp() -> Breakpoint {
        Breakpoint {
            id: 1,
            kind: BreakpointKind::Address { addr: 0x1000 },
            condition: None,
            hit_count: 0,
            enabled: true,
            locations: vec![],
        }
    }

    #[test]
    fn breakpoint_hit_count_increments() {
        let mut bp = make_bp();
        bp.increment_hit_count();
        assert_eq!(bp.hit_count, 1);
        bp.increment_hit_count();
        assert_eq!(bp.hit_count, 2);
    }

    #[test]
    fn breakpoint_toggle_enabled() {
        let mut bp = make_bp();
        assert!(bp.enabled);
        bp.toggle_enabled();
        assert!(!bp.enabled);
        bp.toggle_enabled();
        assert!(bp.enabled);
    }
}
