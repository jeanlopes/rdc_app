//! File tree panel with lazy directory expansion.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// One node in the file tree.
#[derive(Debug, Clone)]
/// One node in the file tree.
pub struct FileTreeNode {
    pub path: PathBuf,
    pub name: String,
    pub kind: FileTreeKind,
    pub children: Vec<FileTreeNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Kind of file tree entry.
pub enum FileTreeKind {
    Directory,
    File,
}

/// File tree widget with background scanning.
#[derive(Debug)]
/// File tree widget with background scanning.
pub struct FileTree {
    pub root: Arc<Mutex<Vec<FileTreeNode>>>,
    pub expanded: HashSet<PathBuf>,
    pub scanned: HashSet<PathBuf>,
    pub root_path: Option<PathBuf>,
    /// Set to true when the user clicks "open folder"; cleared by app.rs after handling.
    pub open_folder_requested: bool,
}

impl Default for FileTree {
    fn default() -> Self {
        Self {
            root: Arc::new(Mutex::new(Vec::new())),
            expanded: HashSet::new(),
            scanned: HashSet::new(),
            root_path: None,
            open_folder_requested: false,
        }
    }
}

impl FileTree {
    /// Start background scan of the given directory.
    pub fn scan_directory(&mut self, dir: PathBuf) {
        let root = Arc::clone(&self.root);
        std::thread::spawn(move || {
            let entries = walkdir::WalkDir::new(&dir)
                .max_depth(1)
                .into_iter()
                .filter_map(|e| e.ok())
                .skip(1) // skip the root directory itself
                .map(|entry| {
                    let path = entry.path().to_path_buf();
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let kind = if entry.file_type().is_dir() {
                        FileTreeKind::Directory
                    } else {
                        FileTreeKind::File
                    };
                    FileTreeNode {
                        path,
                        name,
                        kind,
                        children: Vec::new(),
                    }
                })
                .collect::<Vec<_>>();

            let mut guard = root.lock().unwrap();
            *guard = entries;
        });
    }

    /// Change the root directory, resetting all tree state.
    pub fn change_root(&mut self, dir: PathBuf) {
        *self.root.lock().unwrap() = Vec::new();
        self.expanded.clear();
        self.scanned.clear();
        self.root_path = Some(dir.clone());
        self.scan_directory(dir);
    }

    /// Scan children of a specific directory node lazily.
    pub fn scan_children(&mut self, dir: &PathBuf) {
        if self.scanned.contains(dir) {
            return;
        }
        self.scanned.insert(dir.clone());

        let root = Arc::clone(&self.root);
        let dir = dir.clone();
        std::thread::spawn(move || {
            let entries = walkdir::WalkDir::new(&dir)
                .max_depth(1)
                .into_iter()
                .filter_map(|e| e.ok())
                .skip(1)
                .map(|entry| {
                    let path = entry.path().to_path_buf();
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let kind = if entry.file_type().is_dir() {
                        FileTreeKind::Directory
                    } else {
                        FileTreeKind::File
                    };
                    FileTreeNode {
                        path,
                        name,
                        kind,
                        children: Vec::new(),
                    }
                })
                .collect::<Vec<_>>();

            let mut guard = root.lock().unwrap();
            if let Some(parent) = find_node_mut(&mut *guard, &dir) {
                parent.children = entries;
            }
        });
    }

    /// Render the file tree and return the selected file path, if any.
    pub fn render(&mut self, ui: &mut egui::Ui, active_file: &Option<PathBuf>) -> Option<PathBuf> {
        // Folder header
        ui.horizontal(|ui| {
            let folder_name = self.root_path.as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_uppercase())
                .unwrap_or_else(|| "EXPLORER".to_string());
            ui.label(egui::RichText::new(folder_name).small().strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let btn = egui::Button::new(egui::RichText::new("📂").small()).frame(false);
                if ui.add(btn).on_hover_text("Abrir outra pasta…").clicked() {
                    self.open_folder_requested = true;
                }
            });
        });

        ui.separator();

        let mut selected = None;
        let mut to_scan: Vec<PathBuf> = Vec::new();
        let root = {
            let guard = self.root.lock().unwrap();
            guard.clone()
        };
        for node in &root {
            Self::render_node(ui, node, active_file, &mut self.expanded, &self.scanned, &mut to_scan, &mut selected);
        }
        if !to_scan.is_empty() {
            for dir in to_scan {
                self.scan_children(&dir);
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        selected
    }

    fn render_node(
        ui: &mut egui::Ui,
        node: &FileTreeNode,
        active_file: &Option<PathBuf>,
        expanded: &mut HashSet<PathBuf>,
        scanned: &HashSet<PathBuf>,
        to_scan: &mut Vec<PathBuf>,
        selected: &mut Option<PathBuf>,
    ) {
        let is_active = active_file.as_ref() == Some(&node.path);
        let label = if node.kind == FileTreeKind::Directory {
            format!("📁 {}", node.name)
        } else {
            format!("📄 {}", node.name)
        };

        if node.kind == FileTreeKind::Directory {
            let is_expanded = expanded.contains(&node.path);
            let header = egui::CollapsingHeader::new(&label)
                .id_salt(&node.path)
                .default_open(is_expanded)
                .show(ui, |ui| {
                    for child in &node.children {
                        Self::render_node(ui, child, active_file, expanded, scanned, to_scan, selected);
                    }
                });
            if header.header_response.clicked() {
                if is_expanded {
                    expanded.remove(&node.path);
                } else {
                    expanded.insert(node.path.clone());
                }
            }
            if expanded.contains(&node.path) && node.children.is_empty() && !scanned.contains(&node.path) {
                to_scan.push(node.path.clone());
            }
        } else {
            let response = ui.selectable_label(is_active, label);
            if response.clicked() {
                *selected = Some(node.path.clone());
            }
        }
    }
}

fn find_node_mut<'a>(nodes: &'a mut [FileTreeNode], path: &PathBuf) -> Option<&'a mut FileTreeNode> {
    for node in nodes.iter_mut() {
        if &node.path == path {
            return Some(node);
        }
        if let Some(found) = find_node_mut(&mut node.children, path) {
            return Some(found);
        }
    }
    None
}
