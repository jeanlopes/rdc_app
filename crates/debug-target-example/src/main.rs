/// RDC debug-target-example
///
/// A Windows debug target for validating the mcp-server acceptance criteria.
/// Uses bubble sort as the algorithm — simple, loop-heavy, easy to step through.
///
/// Modes:
///   (no args)  — bubble sort with known array, prints sorted result
///   panic      — index out of bounds panic for PanicDetected testing
use std::env;

// ── Bubble sort ───────────────────────────────────────────────────────────────

/// Sort `arr` in-place using bubble sort.
///
/// Set a breakpoint inside this function to inspect `pass`, `i`, `arr`, `swapped`.
/// Use `probe_context: "sort_pass"` to read `sort_pass.pass` and `sort_pass.swapped`.
fn bubble_sort(arr: &mut Vec<i32>) {
    let n = arr.len();
    for pass in 0..n {
        let mut swapped = false;                     // BP: read_locals here
        for i in 0..(n - 1 - pass) {
            if arr[i] > arr[i + 1] {
                arr.swap(i, i + 1);
                swapped = true;
            }
        }
        if !swapped {
            break; // already sorted
        }
    }
}

// ── Statistics ────────────────────────────────────────────────────────────────

#[allow(dead_code)]
struct Stats {
    min: i32,
    max: i32,
    sum: i32,
    count: usize,
}

/// Compute basic statistics over a sorted slice.
/// Set a breakpoint here to inspect the `Stats` struct at depth 1.
fn compute_stats(arr: &[i32]) -> Stats {
    let min = arr[0];                                // BP: inspect nested struct
    let max = arr[arr.len() - 1];
    let sum: i32 = arr.iter().sum();
    let count = arr.len();
    Stats { min, max, sum, count }
}

// ── Panic path ────────────────────────────────────────────────────────────────

fn panic_path() {
    let data: Vec<i32> = vec![10, 20, 30];
    println!("[panic] attempting out-of-bounds access...");
    let _value = data[99];                           // BP: PanicDetected expected here
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("sort");

    match mode {
        "panic" => {
            panic_path();
        }
        _ => {
            // Known input — sorting this produces a deterministic sequence for
            // step-by-step validation: [64, 34, 25, 12, 22, 11, 90] → [11, 12, 22, 25, 34, 64, 90]
            let mut arr = vec![64, 34, 25, 12, 22, 11, 90];
            println!("before: {:?}", arr);

            bubble_sort(&mut arr);                   // BP: step_into goes here

            println!("after:  {:?}", arr);

            let stats = compute_stats(&arr);
            println!(
                "stats: min={} max={} sum={} count={}",
                stats.min, stats.max, stats.sum, stats.count
            );
        }
    }
}
