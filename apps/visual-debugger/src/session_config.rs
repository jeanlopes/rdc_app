use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionConfig {
    pub breakpoints: Vec<SavedBreakpoint>,
    pub open_directory: Option<PathBuf>,
    pub open_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedBreakpoint {
    pub file: PathBuf,
    pub line: u32,
}

impl SessionConfig {
    fn config_path() -> PathBuf {
        #[cfg(windows)]
        {
            let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
            PathBuf::from(appdata).join("rdc-visual-debugger").join("session.json")
        }
        #[cfg(not(windows))]
        {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".config").join("rdc-visual-debugger").join("session.json")
        }
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(breakpoints: &[SavedBreakpoint], directory: Option<&PathBuf>, file: Option<&PathBuf>) {
        let cfg = Self {
            breakpoints: breakpoints.to_vec(),
            open_directory: directory.cloned(),
            open_file: file.cloned(),
        };
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&cfg) {
            let _ = std::fs::write(&path, json);
        }
    }
}
