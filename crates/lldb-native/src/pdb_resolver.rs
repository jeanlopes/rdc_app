/// PDB-based source-line resolver.
///
/// LLDB 19.1.7 on Windows has a bug in `SymbolFileNativePDB` that prevents it
/// from loading PDB symbols automatically (UUID comparison logic fails even when
/// GUIDs match). This resolver parses the PDB directly using the `pdb` crate and
/// maintains two lookup tables:
///
/// * source_file (basename) + line_number → RVA (for setting breakpoints by address)
/// * RVA → (source_file, line_number)      (for resolving stop location)
///
/// RVAs are converted to actual virtual addresses by adding the module's load
/// base, which is set from the `image list` output after the process launches.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use pdb::FallibleIterator;
use tracing::{info, warn};

pub struct PdbResolver {
    /// (normalised_basename_lowercase, line_start) → rva
    line_to_rva: HashMap<(String, u32), u32>,
    /// rva → (full_path, line_start) — BTreeMap for nearest-match lookups
    rva_to_line: BTreeMap<u32, (String, u32)>,
    /// Actual module load base after process launch.  None until set.
    module_base: Option<u64>,
}

impl PdbResolver {
    /// Open and parse `pdb_path`, returning None on any error.
    pub fn open(pdb_path: &Path) -> Option<Self> {
        let file = std::fs::File::open(pdb_path)
            .map_err(|e| warn!("PDB open failed: {e}"))
            .ok()?;

        let mut pdb = pdb::PDB::open(file)
            .map_err(|e| warn!("PDB parse failed: {e}"))
            .ok()?;

        let addr_map = pdb.address_map()
            .map_err(|e| warn!("PDB address_map failed: {e}"))
            .ok()?;

        // Each of these takes &mut pdb temporarily but releases the borrow when it returns.
        // The returned values own their data (lifetime 's = file source), so they coexist.
        let string_table = pdb.string_table()
            .map_err(|e| warn!("PDB string_table failed: {e}"))
            .ok()?;

        let dbi = pdb.debug_information()
            .map_err(|e| warn!("PDB debug_information failed: {e}"))
            .ok()?;

        let mut line_to_rva: HashMap<(String, u32), u32> = HashMap::new();
        let mut rva_to_line: BTreeMap<u32, (String, u32)> = BTreeMap::new();

        let mut modules = match dbi.modules() {
            Ok(m) => m,
            Err(e) => { warn!("PDB modules() failed: {e}"); return None; }
        };

        let mut total_lines: usize = 0;

        loop {
            let module = match modules.next() {
                Ok(Some(m)) => m,
                Ok(None) => break,
                Err(e) => { warn!("PDB module iteration error: {e}"); break; }
            };

            let module_info = match pdb.module_info(&module) {
                Ok(Some(info)) => info,
                Ok(None) => continue,
                Err(e) => { warn!("PDB module_info error: {e}"); continue; }
            };

            let line_program = match module_info.line_program() {
                Ok(prog) => prog,
                Err(_) => continue,
            };

            let mut lines = line_program.lines();
            loop {
                let line_info = match lines.next() {
                    Ok(Some(l)) => l,
                    Ok(None) => break,
                    Err(_) => break,
                };

                let rva = match line_info.offset.to_rva(&addr_map) {
                    Some(r) => r.0,
                    None => continue,
                };

                let file_info = match line_program.get_file_info(line_info.file_index) {
                    Ok(f) => f,
                    Err(_) => continue,
                };

                let file_name = match string_table.get(file_info.name) {
                    Ok(raw) => raw.to_string().into_owned(),
                    Err(_) => continue,
                };
                let line_num = line_info.line_start;

                // Index by basename (lowercase) so callers can pass just the filename.
                let basename = Path::new(&file_name)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&file_name)
                    .to_lowercase();

                // For source → VA: store the LOWEST rva for each (file, line) pair
                // so the first instruction of that line is targeted.
                line_to_rva
                    .entry((basename, line_num))
                    .and_modify(|existing| {
                        if rva < *existing {
                            *existing = rva;
                        }
                    })
                    .or_insert(rva);

                // For VA → source: store full path (prefer first entry per RVA).
                rva_to_line.entry(rva).or_insert_with(|| (file_name.clone(), line_num));

                total_lines += 1;
            }
        }

        info!(
            pdb = %pdb_path.display(),
            total_lines,
            unique_source_locations = line_to_rva.len(),
            "PDB parsed successfully"
        );

        Some(Self {
            line_to_rva,
            rva_to_line,
            module_base: None,
        })
    }

    /// Called after process launch to set the actual module load base from `image list` output.
    ///
    /// `exe_name` is just the filename part (e.g. "rust_app_example.exe").
    pub fn apply_module_base_from_image_list(&mut self, image_list_output: &str, exe_name: &str) {
        let exe_lower = exe_name.to_lowercase();
        for line in image_list_output.lines() {
            if line.to_lowercase().contains(&exe_lower) {
                // Format: "[ 0] UUID 0xBASEADDR /path/to/exe"
                for word in line.split_whitespace() {
                    let hex = if let Some(h) = word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")) {
                        h
                    } else {
                        continue;
                    };
                    if let Ok(base) = u64::from_str_radix(hex, 16) {
                        self.module_base = Some(base);
                        info!(module_base = format!("{base:#x}"), "module base set from image list");
                        return;
                    }
                }
            }
        }
        warn!(exe_name, "could not parse module base from image list");
    }

    /// Force-set the module base directly (alternative to `apply_module_base_from_image_list`).
    pub fn set_module_base(&mut self, base: u64) {
        self.module_base = Some(base);
    }

    /// Resolve (source_file_basename, line) → virtual address.
    ///
    /// Returns None if the file/line is not in the PDB or base address is unknown.
    pub fn source_to_va(&self, file: &str, line: u32) -> Option<u64> {
        let module_base = self.module_base?;

        let basename = Path::new(file)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(file)
            .to_lowercase();

        let rva = self.line_to_rva.get(&(basename, line))?;
        Some(module_base + *rva as u64)
    }

    /// Resolve a virtual address → (source_file_path, line_number).
    ///
    /// Uses the nearest-below RVA in the line table, matching how debuggers
    /// map PCs to source lines.
    pub fn va_to_source(&self, va: u64) -> Option<(PathBuf, u32)> {
        let module_base = self.module_base?;
        if va < module_base {
            return None;
        }
        let rva = (va - module_base) as u32;

        // Addresses in other modules (e.g. ntdll) that happen to sit at higher
        // load addresses than our exe produce enormous RVAs when subtracted from
        // module_base.  Without this guard, next_back() would return the last
        // entry in the PDB (CRT startup code) for any ntdll/kernel address.
        let max_rva = self.rva_to_line.keys().next_back().copied().unwrap_or(0);
        if rva > max_rva.saturating_add(0x1000) {
            return None;
        }

        // Find the greatest RVA ≤ query RVA.
        let (_, (path, line)) = self.rva_to_line.range(..=rva).next_back()?;
        Some((PathBuf::from(path), *line))
    }

    pub fn has_module_base(&self) -> bool {
        self.module_base.is_some()
    }

    /// Return the VA of the first instruction of the NEXT source line after
    /// `current_va` within the same file.
    ///
    /// Used by `handle_step` to implement step_over via a temp breakpoint:
    /// LLDB 19.1.7 on Windows only advances one instruction with `step_over()`
    /// because SymbolFileNativePDB is broken and it doesn't know line boundaries.
    pub fn next_source_line_va(
        &self,
        current_va: u64,
        current_file_basename: &str,
        current_line: u32,
    ) -> Option<u64> {
        let module_base = self.module_base?;
        if current_va < module_base {
            return None;
        }
        let current_rva = (current_va - module_base) as u32;
        let start_rva = current_rva.checked_add(1)?;

        let basename_lower = Path::new(current_file_basename)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(current_file_basename)
            .to_lowercase();

        // Walk RVA entries in ascending order from start_rva.
        // Skip entries in other files or still on the same source line.
        self.rva_to_line
            .range(start_rva..)
            .find(|(_, (file, line))| {
                let entry_basename = Path::new(file.as_str())
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(file.as_str())
                    .to_lowercase();
                // Require a strictly higher line number: LLVM sometimes emits
                // inlined code from earlier lines at higher addresses, so
                // accepting any different line would pick a backward reference.
                entry_basename == basename_lower && *line > current_line
            })
            .map(|(rva, _)| module_base + u64::from(*rva))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PDB: &str = r"C:\workspace\rust_app_example\target\debug\rust_app_example.pdb";

    #[test]
    fn opens_and_parses() {
        if !Path::new(PDB).exists() {
            eprintln!("SKIP: PDB not found at {PDB}");
            return;
        }
        let resolver = PdbResolver::open(Path::new(PDB))
            .expect("PdbResolver::open must succeed for a valid PDB");
        assert!(!resolver.line_to_rva.is_empty(), "must have parsed some line entries");
        println!("line_to_rva entries: {}", resolver.line_to_rva.len());
    }

    #[test]
    fn resolves_main_rs_after_base_set() {
        if !Path::new(PDB).exists() {
            eprintln!("SKIP: PDB not found at {PDB}");
            return;
        }
        let mut resolver = PdbResolver::open(Path::new(PDB))
            .expect("PdbResolver::open must succeed");

        resolver.set_module_base(0x0000000140000000);

        // Print first 30 entries for diagnostics.
        println!("First 30 PDB source locations:");
        for ((file, line), rva) in resolver.line_to_rva.iter().take(30) {
            println!("  {file}:{line} → rva={rva:#x}");
        }

        // Print all entries that contain "main" in the filename.
        println!("\nEntries with 'main' in filename:");
        for ((file, line), rva) in resolver.line_to_rva.iter().filter(|((f, _), _)| f.contains("main")) {
            println!("  {file}:{line} → rva={rva:#x}");
        }

        let mut found = false;
        for line in 1..=200u32 {
            if let Some(va) = resolver.source_to_va("main.rs", line) {
                println!("main.rs:{line} → va={va:#x}");
                found = true;
                if let Some((file, rl)) = resolver.va_to_source(va) {
                    println!("  reverse → {}:{rl}", file.display());
                }
                break;
            }
        }
        assert!(found, "at least one line of main.rs should be resolvable in the PDB");
    }
}
