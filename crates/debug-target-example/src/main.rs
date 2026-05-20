/// RDC debug-target-example
///
/// A purpose-built binary for validating the mcp-server acceptance criteria
/// from specs/001-mcp-lldb-bridge/quickstart.md.
///
/// Sections (each mapped to a quickstart checklist item):
///   - layout::measure      — breakpoint + semantic probe
///   - panic_path           — panic detection
///   - multi_thread         — list_threads with multiple threads
///   - nested_struct        — read_locals with nested structs at depth 3+
use std::thread;
use std::time::Duration;

// ── Layout module — breakpoint + semantic probe target ────────────────────────

mod layout {
    /// Simulates a layout measurement pass.
    /// Set a breakpoint at the `// BREAKPOINT` line to hit it with known locals.
    pub fn measure(available_width: i32) -> i32 {
        let content_width: i32 = 100;
        let padding: i32 = 8;
        let current_x: i32 = content_width + padding;                   // 108
        let remaining_width: i32 = available_width - current_x;         // e.g. -12
        let overflow: bool = remaining_width < 0;                       // BREAKPOINT

        if overflow {
            eprintln!(
                "[layout] overflow: current_x={} remaining_width={}",
                current_x, remaining_width
            );
        }
        current_x
    }

    /// Multi-step layout for step_over / step_into testing.
    pub fn layout_pass(width: i32) -> i32 {
        let a = measure(width);           // step_into goes here
        let b = measure(width + 50);
        a + b
    }
}

// ── Nested struct — tests read_locals at depth ≥ 3 ───────────────────────────

#[allow(dead_code)]
#[derive(Debug)]
struct Padding {
    top: i32,
    right: i32,
    bottom: i32,
    left: i32,
}

#[allow(dead_code)]
#[derive(Debug)]
struct Style {
    font_size: f32,
    color: u32,
    padding: Padding,
}

#[allow(dead_code)]
#[derive(Debug)]
struct Widget {
    id: u32,
    label: String,
    style: Style,
    children: Vec<u32>,
}

fn inspect_nested() {
    let widget = Widget {
        id: 42,
        label: "Submit".to_string(),
        style: Style {
            font_size: 14.0,
            color: 0xFF5733,
            padding: Padding { top: 8, right: 16, bottom: 8, left: 16 },
        },
        children: vec![1, 2, 3],
    };
    // BREAKPOINT: read_locals here returns widget at depth 3 (Widget→Style→Padding)
    println!("[nested] widget id={} label={}", widget.id, widget.label);
}

// ── Panic path — panic detection ─────────────────────────────────────────────

fn panic_path() {
    let data: Vec<i32> = vec![1, 2, 3];
    // BREAKPOINT (panic): continue_execution should return PanicDetected
    let _value = data[99]; // index out of bounds
}

// ── Multi-threaded path — list_threads ───────────────────────────────────────

fn multi_thread() {
    let handle = thread::spawn(|| {
        thread::sleep(Duration::from_millis(200));
        42_i32
    });
    // BREAKPOINT: list_threads should show at least 2 threads here
    let result = handle.join().unwrap();
    println!("[multi_thread] result={}", result);
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("layout");

    match mode {
        "layout" => {
            println!("[main] running layout mode");
            let result = layout::layout_pass(96); // available=96, overflow expected
            println!("[main] layout result={}", result);
        }
        "nested" => {
            println!("[main] running nested struct mode");
            inspect_nested();
        }
        "panic" => {
            println!("[main] running panic mode");
            panic_path();
        }
        "threads" => {
            println!("[main] running multi-thread mode");
            multi_thread();
        }
        "all" => {
            println!("[main] running all modes");
            let _ = layout::layout_pass(96);
            inspect_nested();
            multi_thread();
            // panic last — it terminates
            panic_path();
        }
        unknown => {
            eprintln!("unknown mode: '{}'. use: layout | nested | panic | threads | all", unknown);
            std::process::exit(1);
        }
    }
}
