// Integration tests for win-debug-bridge.
// These require a pre-built debug-target-example binary and its .pdb file.
// Run with: cargo build -p debug-target-example && cargo test -p win-debug-bridge -- --ignored

use win_debug_bridge::pdb_info::PdbInfo;

const KNOWN_LINE: u32 = 20;

fn exe_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/debug-target-example.exe")
}

#[test]
#[ignore]
fn pdb_load_succeeds() {
    let path = exe_path();
    let info = PdbInfo::load(&path, 0x140000000).expect("PDB load failed");
    let _ = info;
}

#[test]
#[ignore]
fn pdb_source_to_va_main_line() {
    let path = exe_path();
    let info = PdbInfo::load(&path, 0x140000000).unwrap();
    let va = info.source_to_va(std::path::Path::new("main.rs"), KNOWN_LINE);
    assert!(va.is_some(), "source_to_va returned None for main.rs:{}", KNOWN_LINE);
    assert_ne!(va.unwrap(), 0);
}

#[test]
#[ignore]
fn pdb_va_to_source_round_trip() {
    let path = exe_path();
    let info = PdbInfo::load(&path, 0x140000000).unwrap();
    let va = info.source_to_va(std::path::Path::new("main.rs"), KNOWN_LINE).unwrap();
    let loc = info.va_to_source(va).unwrap();
    assert!(loc.file.to_string_lossy().contains("main"));
    assert_eq!(loc.line, KNOWN_LINE);
}

#[test]
#[ignore]
fn pdb_function_bubble_sort_found() {
    let path = exe_path();
    let info = PdbInfo::load(&path, 0x140000000).unwrap();
    assert!(info.function_name_to_va("bubble_sort").is_some());
}

#[test]
#[ignore]
fn pdb_locals_contain_pass() {
    let path = exe_path();
    let info = PdbInfo::load(&path, 0x140000000).unwrap();
    let va = info.function_name_to_va("bubble_sort").unwrap();
    let locals = info.locals_at_va(va);
    assert!(locals.iter().any(|l| l.name == "pass"), "no 'pass' local found");
}
