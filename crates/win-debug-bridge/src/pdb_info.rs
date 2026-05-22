//! PDB symbol file loader.
//!
//! Pre-parses the .pdb file into in-memory indexes so all lookups are O(log n)
//! or O(1) at debug time with no I/O.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use pdb::{FallibleIterator, PDB, SymbolData};
use runtime_core::{error::DebuggerError, process::SourceLocation};

/// A local variable's storage location within a stack frame.
#[derive(Debug, Clone)]
pub enum VarLocation {
    /// Offset from the frame base pointer (RBP). Negative = below RBP, positive = above.
    FramePointerRelative(i32),
    /// Stored entirely in a register (register number per AMD64 convention).
    Register(u16),
}

/// Metadata about a local variable from the PDB.
#[derive(Debug, Clone)]
pub struct PdbLocal {
    pub name: String,
    pub type_name: String,
    pub location: VarLocation,
    /// Size in bytes. 0 = unknown (treat as opaque).
    pub size: usize,
}

/// Pre-parsed PDB data. All fields are owned; no lifetime dependency on the PDB file.
pub struct PdbInfo {
    image_base: u64,
    /// RVA → (absolute path, line number) — used for address-to-source lookups.
    rva_to_source: BTreeMap<u32, (PathBuf, u32)>,
    /// (lowercase file stem, line) → RVA — used for source-to-address lookups.
    line_to_rva: HashMap<(String, u32), u32>,
    /// Sorted list of (start_rva, name) — used for address-to-function-name.
    function_starts: Vec<(u32, String)>,
    /// function start_rva → local variables.
    locals: HashMap<u32, Vec<PdbLocal>>,
    /// Demangled/short function name → start RVA (for function-name breakpoints).
    name_to_rva: HashMap<String, u32>,
}

impl PdbInfo {
    /// Load and index a PDB file located next to `exe_path`.
    pub fn load(exe_path: &Path, image_base: u64) -> Result<Self, DebuggerError> {
        let pdb_path = exe_path.with_extension("pdb");
        // Rust on Windows names .pdb with underscores even when the binary uses hyphens.
        let pdb_path = if !pdb_path.exists() {
            let stem = exe_path.file_stem().unwrap_or_default().to_string_lossy().replace('-', "_");
            let alt = exe_path.with_file_name(format!("{}.pdb", stem));
            if alt.exists() { alt } else { pdb_path }
        } else {
            pdb_path
        };
        if !pdb_path.exists() {
            return Err(DebuggerError::DebuggerError(format!(
                "PDB not found at {}. Compile with `cargo build` (not --release).",
                pdb_path.display()
            )));
        }

        let file = std::fs::File::open(&pdb_path)
            .map_err(|e| DebuggerError::DebuggerError(format!("open PDB: {}", e)))?;
        let mut pdb = PDB::open(file)
            .map_err(|e| DebuggerError::DebuggerError(format!("parse PDB: {}", e)))?;

        let address_map = pdb.address_map()
            .map_err(|e| DebuggerError::DebuggerError(format!("PDB address map: {}", e)))?;

        let mut info = PdbInfo {
            image_base,
            rva_to_source: BTreeMap::new(),
            line_to_rva: HashMap::new(),
            function_starts: Vec::new(),
            locals: HashMap::new(),
            name_to_rva: HashMap::new(),
        };

        // ── Public symbols (function names + addresses) ───────────────────────
        {
            let global = pdb.global_symbols()
                .map_err(|e| DebuggerError::DebuggerError(format!("PDB global symbols: {}", e)))?;
            let mut iter = global.iter();
            while let Some(sym) = iter.next()
                .map_err(|e| DebuggerError::DebuggerError(format!("symbol iter: {}", e)))?
            {
                if let Ok(SymbolData::Public(data)) = sym.parse() {
                    if data.function {
                        if let Some(rva) = data.offset.to_rva(&address_map) {
                            let full_name = data.name.to_string().into_owned();
                            // Extract short name (last :: segment, demangled)
                            let short = short_name(&full_name);
                            info.function_starts.push((rva.0, full_name.clone()));
                            info.name_to_rva.entry(full_name).or_insert(rva.0);
                            info.name_to_rva.entry(short).or_insert(rva.0);
                        }
                    }
                }
            }
        }
        info.function_starts.sort_by_key(|f| f.0);
        info.function_starts.dedup_by_key(|f| f.0);

        // ── Line numbers + local variables ────────────────────────────────────
        {
            let string_table = pdb.string_table()
                .map_err(|e| DebuggerError::DebuggerError(format!("PDB string table: {}", e)))?;

            let dbi = pdb.debug_information()
                .map_err(|e| DebuggerError::DebuggerError(format!("PDB debug info: {}", e)))?;

            let mut modules = dbi.modules()
                .map_err(|e| DebuggerError::DebuggerError(format!("PDB modules: {}", e)))?;

            while let Some(module) = modules.next()
                .map_err(|e| DebuggerError::DebuggerError(format!("module iter: {}", e)))?
            {
                let module_info = match pdb.module_info(&module)
                    .map_err(|e| DebuggerError::DebuggerError(format!("module info: {}", e)))?
                {
                    Some(m) => m,
                    None => continue,
                };

                // Line numbers
                if let Ok(line_program) = module_info.line_program() {
                    let mut lines = line_program.lines();
                    while let Ok(Some(line)) = lines.next() {
                        if let Some(rva) = line.offset.to_rva(&address_map) {
                            if let Ok(file_info) = line_program.get_file_info(line.file_index) {
                                if let Ok(raw) = string_table.get(file_info.name) {
                                    let file_path = PathBuf::from(raw.to_string().into_owned());
                                    let stem = file_path
                                        .file_stem()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .to_lowercase();
                                    let line_num = line.line_start;
                                    info.rva_to_source.insert(rva.0, (file_path.clone(), line_num));
                                    info.line_to_rva
                                        .entry((stem, line_num))
                                        .or_insert(rva.0);
                                }
                            }
                        }
                    }
                }

                // Local variables (procedure symbols)
                if let Ok(mut symbols) = module_info.symbols() {
                    let mut current_proc_rva: Option<u32> = None;
                    let mut current_locals: Vec<PdbLocal> = Vec::new();
                    let mut pending_name: Option<(String, String)> = None; // (name, type_name)

                    // S_DEFRANGE_FRAMEPOINTER_REL / _FULL_SCOPE symbol kinds —
                    // pdb 0.8 does not parse these, so we read the offset from raw bytes.
                    const S_DEFRANGE_FRAMEPOINTER_REL: u16 = 0x1142;
                    const S_DEFRANGE_FRAMEPOINTER_REL_FULL_SCOPE: u16 = 0x1144;

                    while let Ok(Some(sym)) = symbols.next() {
                        let raw_kind = sym.raw_kind();
                        // Handle S_DEFRANGE_* before parse() — pdb 0.8 returns Error for these.
                        if raw_kind == S_DEFRANGE_FRAMEPOINTER_REL
                            || raw_kind == S_DEFRANGE_FRAMEPOINTER_REL_FULL_SCOPE
                        {
                            if let Some((name, type_name)) = pending_name.take() {
                                // Bytes layout: [kind: u16][offset: i32][...]
                                let raw = sym.raw_bytes();
                                if raw.len() >= 6 {
                                    let offset = i32::from_le_bytes([raw[2], raw[3], raw[4], raw[5]]);
                                    let sz = primitive_size_from_type_name(&type_name);
                                    current_locals.push(PdbLocal {
                                        name,
                                        type_name,
                                        location: VarLocation::FramePointerRelative(offset),
                                        size: sz,
                                    });
                                }
                            }
                            continue;
                        }

                        match sym.parse() {
                            Ok(SymbolData::Procedure(proc)) => {
                                // Save previous procedure's locals
                                if let Some(rva) = current_proc_rva {
                                    if !current_locals.is_empty() {
                                        info.locals.insert(rva, std::mem::take(&mut current_locals));
                                    }
                                }
                                current_proc_rva = proc.offset.to_rva(&address_map).map(|r| r.0);
                                // Index private functions not captured by global_symbols()
                                if let Some(rva) = current_proc_rva {
                                    let full_name = proc.name.to_string().into_owned();
                                    let short = short_name(&full_name);
                                    info.function_starts.push((rva, full_name.clone()));
                                    info.name_to_rva.entry(full_name).or_insert(rva);
                                    info.name_to_rva.entry(short).or_insert(rva);
                                }
                                current_locals.clear();
                                pending_name = None;
                            }
                            Ok(SymbolData::Local(local)) => {
                                let name = local.name.to_string().into_owned();
                                let type_name = format!("type_{}", local.type_index.0);
                                pending_name = Some((name, type_name));
                            }
                            Ok(SymbolData::RegisterRelative(reg)) => {
                                // S_REGREL32 (old format): name is in the record itself.
                                let name = reg.name.to_string().into_owned();
                                let type_name = format!("type_{}", reg.type_index.0);
                                let sz = primitive_size_from_type_name(&type_name);
                                current_locals.push(PdbLocal {
                                    name,
                                    type_name,
                                    location: VarLocation::FramePointerRelative(reg.offset),
                                    size: sz,
                                });
                                pending_name = None;
                            }
                            _ => {
                                // Clear pending_name only on non-defrange unrecognised symbols.
                                // Defrange records are handled before parse() above.
                                pending_name = None;
                            }
                        }
                    }

                    // Save last procedure
                    if let Some(rva) = current_proc_rva {
                        if !current_locals.is_empty() {
                            info.locals.insert(rva, current_locals);
                        }
                    }
                }
            }
        }

        // Re-sort after module procedure symbols were added
        info.function_starts.sort_by_key(|f| f.0);
        info.function_starts.dedup_by_key(|f| f.0);

        tracing::info!(
            functions = info.function_starts.len(),
            source_lines = info.rva_to_source.len(),
            "PDB loaded"
        );

        Ok(info)
    }

    // ── Public query methods ──────────────────────────────────────────────────

    /// Convert a virtual address to a source location.
    pub fn va_to_source(&self, va: u64) -> Option<SourceLocation> {
        let rva = self.va_to_rva(va)?;
        let (&entry_rva, (path, line)) = self.rva_to_source.range(..=rva).next_back()?;
        // Only use if we're within 256 bytes of the entry (avoid false matches)
        if rva - entry_rva > 256 { return None; }
        Some(SourceLocation { file: path.clone(), line: *line, column: None })
    }

    /// Convert a source file stem (lowercase) + line to a virtual address.
    pub fn source_to_va(&self, file: &Path, line: u32) -> Option<u64> {
        let stem = file.file_stem()?.to_string_lossy().to_lowercase();
        let rva = *self.line_to_rva.get(&(stem, line))?;
        Some(self.rva_to_va(rva))
    }

    /// Get the function name containing the given virtual address.
    pub fn va_to_function_name(&self, va: u64) -> Option<String> {
        let rva = self.va_to_rva(va)?;
        let idx = self.function_starts.partition_point(|f| f.0 <= rva);
        if idx == 0 { return None; }
        Some(self.function_starts[idx - 1].1.clone())
    }

    /// Get the virtual address of a function by name (exact or suffix match).
    pub fn function_name_to_va(&self, name: &str) -> Option<u64> {
        let rva = self.name_to_rva.get(name).copied()
            .or_else(|| {
                // Try suffix match for demangled names
                self.function_starts.iter()
                    .find(|(_, n)| n.ends_with(&format!("::{}", name)))
                    .map(|(rva, _)| *rva)
            })?;
        Some(self.rva_to_va(rva))
    }

    /// Get local variables for the function containing `va`.
    pub fn locals_at_va(&self, va: u64) -> Vec<PdbLocal> {
        let rva = match self.va_to_rva(va) { Some(r) => r, None => return vec![] };
        let idx = self.function_starts.partition_point(|f| f.0 <= rva);
        if idx == 0 { return vec![]; }
        let func_rva = self.function_starts[idx - 1].0;
        self.locals.get(&func_rva).cloned().unwrap_or_default()
    }

    pub fn rva_to_va(&self, rva: u32) -> u64 {
        self.image_base + rva as u64
    }

    pub fn va_to_rva(&self, va: u64) -> Option<u32> {
        va.checked_sub(self.image_base).map(|r| r as u32)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract the last `::` segment of a potentially mangled Rust symbol name.
fn short_name(full: &str) -> String {
    // Strip hash suffix (e.g., `bubble_sort::h1234abcd`)
    let without_hash = full.rsplit("::h").next().map(|_| {
        let parts: Vec<&str> = full.splitn(2, "::h").collect();
        parts[0]
    }).unwrap_or(full);
    // Take last segment
    without_hash.rsplit("::").next().unwrap_or(without_hash).to_string()
}

/// Best-effort size in bytes for a PDB type name. Returns 0 if unknown.
fn primitive_size_from_type_name(name: &str) -> usize {
    match name {
        "bool" => 1,
        "i8" | "u8" => 1,
        "i16" | "u16" => 2,
        "i32" | "u32" | "f32" => 4,
        "i64" | "u64" | "f64" | "isize" | "usize" => 8,
        "i128" | "u128" => 16,
        _ => 0,
    }
}

#[cfg(test)]
impl PdbInfo {
    pub fn test_new(
        image_base: u64,
        rva_to_source: BTreeMap<u32, (PathBuf, u32)>,
        line_to_rva: HashMap<(String, u32), u32>,
        function_starts: Vec<(u32, String)>,
        name_to_rva: HashMap<String, u32>,
    ) -> Self {
        PdbInfo { image_base, rva_to_source, line_to_rva, function_starts, locals: HashMap::new(), name_to_rva }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u64 = 0x140000000;

    fn empty_pdb() -> PdbInfo {
        PdbInfo::test_new(BASE, BTreeMap::new(), HashMap::new(), vec![], HashMap::new())
    }

    #[test]
    fn short_name_strips_hash() {
        assert_eq!(short_name("bubble_sort::h1a2b3c4d"), "bubble_sort");
    }

    #[test]
    fn short_name_takes_last_segment() {
        assert_eq!(short_name("std::vec::Vec::push"), "push");
    }

    #[test]
    fn short_name_simple() {
        assert_eq!(short_name("main"), "main");
    }

    #[test]
    fn primitive_size_bool() {
        assert_eq!(primitive_size_from_type_name("bool"), 1);
    }

    #[test]
    fn primitive_size_i32() {
        assert_eq!(primitive_size_from_type_name("i32"), 4);
    }

    #[test]
    fn primitive_size_usize() {
        assert_eq!(primitive_size_from_type_name("usize"), 8);
    }

    #[test]
    fn primitive_size_unknown() {
        assert_eq!(primitive_size_from_type_name("MyStruct"), 0);
    }

    #[test]
    fn rva_to_va_adds_base() {
        let pdb = empty_pdb();
        assert_eq!(pdb.rva_to_va(0x1234), BASE + 0x1234);
    }

    #[test]
    fn va_to_rva_subtracts_base() {
        let pdb = empty_pdb();
        assert_eq!(pdb.va_to_rva(BASE + 0x1234), Some(0x1234));
    }

    #[test]
    fn va_to_rva_below_base_returns_none() {
        let pdb = empty_pdb();
        assert_eq!(pdb.va_to_rva(0x100), None);
    }

    #[test]
    fn va_to_source_exact_hit() {
        let mut rva_to_source = BTreeMap::new();
        rva_to_source.insert(0x1000u32, (PathBuf::from("main.rs"), 42u32));
        let pdb = PdbInfo::test_new(BASE, rva_to_source, HashMap::new(), vec![], HashMap::new());
        let loc = pdb.va_to_source(BASE + 0x1000).unwrap();
        assert_eq!(loc.line, 42);
    }

    #[test]
    fn va_to_source_nearest_within_range() {
        let mut rva_to_source = BTreeMap::new();
        rva_to_source.insert(0x1000u32, (PathBuf::from("main.rs"), 42u32));
        let pdb = PdbInfo::test_new(BASE, rva_to_source, HashMap::new(), vec![], HashMap::new());
        let loc = pdb.va_to_source(BASE + 0x1000 + 50).unwrap();
        assert_eq!(loc.line, 42);
    }

    #[test]
    fn va_to_source_too_far_returns_none() {
        let mut rva_to_source = BTreeMap::new();
        rva_to_source.insert(0x1000u32, (PathBuf::from("main.rs"), 42u32));
        let pdb = PdbInfo::test_new(BASE, rva_to_source, HashMap::new(), vec![], HashMap::new());
        assert!(pdb.va_to_source(BASE + 0x1000 + 300).is_none());
    }

    #[test]
    fn va_to_function_name_found() {
        let function_starts = vec![(0x2000u32, "my_func".to_string())];
        let pdb = PdbInfo::test_new(BASE, BTreeMap::new(), HashMap::new(), function_starts, HashMap::new());
        let name = pdb.va_to_function_name(BASE + 0x2000).unwrap();
        assert_eq!(name, "my_func");
    }

    #[test]
    fn va_to_function_name_not_found() {
        let function_starts = vec![(0x2000u32, "my_func".to_string())];
        let pdb = PdbInfo::test_new(BASE, BTreeMap::new(), HashMap::new(), function_starts, HashMap::new());
        assert!(pdb.va_to_function_name(BASE + 0x100).is_none());
    }

    #[test]
    fn source_to_va_round_trip() {
        let mut line_to_rva = HashMap::new();
        line_to_rva.insert(("main".to_string(), 42u32), 0x1000u32);
        let pdb = PdbInfo::test_new(BASE, BTreeMap::new(), line_to_rva, vec![], HashMap::new());
        assert_eq!(pdb.source_to_va(std::path::Path::new("main.rs"), 42), Some(BASE + 0x1000));
    }

    #[test]
    fn source_to_va_unknown_returns_none() {
        assert!(empty_pdb().source_to_va(std::path::Path::new("unknown.rs"), 1).is_none());
    }

    #[test]
    fn function_name_to_va_exact() {
        let mut name_to_rva = HashMap::new();
        name_to_rva.insert("bubble_sort".to_string(), 0x3000u32);
        let pdb = PdbInfo::test_new(BASE, BTreeMap::new(), HashMap::new(), vec![], name_to_rva);
        assert_eq!(pdb.function_name_to_va("bubble_sort"), Some(BASE + 0x3000));
    }

    #[test]
    fn function_name_to_va_missing() {
        assert!(empty_pdb().function_name_to_va("nonexistent").is_none());
    }
}
