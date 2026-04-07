use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use wasmtime::Module;

use twinkle::cli::run_wasm::{build_engine, execute_module};

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/run")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn bench_fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches/tw")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn build_wasm(file_path: &str) -> Vec<u8> {
    let wat = twinkle::cli::build::build_wat(file_path).expect("build_wat failed");
    wat::parse_str(&wat).expect("WAT parse failed")
}

fn bench_wasm_exec(c: &mut Criterion) {
    let engine = build_engine().expect("failed to build Wasmtime engine");

    let cases = [("bench_generic", "bench_generic.tw")];

    for (label, file) in cases {
        let path = fixture(file);
        let bytes = build_wasm(&path);
        let module = Module::new(&engine, &bytes).expect("module");

        let mut group = c.benchmark_group(label);
        group.bench_function(BenchmarkId::new("exec", "typed_closure"), |b| {
            b.iter(|| execute_module(&engine, &module).expect("execution failed"))
        });
        group.finish();
    }
}

fn bench_closure_exec(c: &mut Criterion) {
    let engine = build_engine().expect("failed to build Wasmtime engine");
    let cases = [
        ("bench_closure", "bench_closure.tw"),
        ("bench_closure_stress", "bench_closure_stress.tw"),
    ];

    for (label, file) in cases {
        let path = fixture(file);
        let bytes = build_wasm(&path);
        let module = Module::new(&engine, &bytes).expect("module");

        let mut group = c.benchmark_group(label);
        group.bench_function(BenchmarkId::new("exec", "typed_closure"), |b| {
            b.iter(|| execute_module(&engine, &module).expect("execution failed"))
        });
        group.finish();
    }
}

/// Compare optimized collect lowering (builder path) against a
/// manual persistent push loop that still pays concat-per-append cost.
fn bench_collect_strategy_exec(c: &mut Criterion) {
    let engine = build_engine().expect("failed to build Wasmtime engine");

    let optimized_path = fixture("bench_collect_iterator.tw");
    let manual_path = fixture("bench_manual_push_iterator.tw");

    let optimized_bytes = build_wasm(&optimized_path);
    let manual_bytes = build_wasm(&manual_path);

    let optimized_module = Module::new(&engine, &optimized_bytes).expect("module (collect)");
    let manual_module = Module::new(&engine, &manual_bytes).expect("module (manual push)");

    let mut group = c.benchmark_group("bench_collect_strategy");
    group.bench_function(BenchmarkId::new("exec", "collect_builder"), |b| {
        b.iter(|| execute_module(&engine, &optimized_module).expect("execution failed"))
    });
    group.bench_function(BenchmarkId::new("exec", "manual_push"), |b| {
        b.iter(|| execute_module(&engine, &manual_module).expect("execution failed"))
    });
    group.finish();
}

/// Phase 1 microbenchmarks: measure vector read cost across trie depths.
///
/// Compares indexed reads (`xs[i]`) and iterator traversal (`for x in xs`)
/// at three vector sizes that exercise different code paths:
/// - tiny (len=32): tail-only, no trie descent
/// - shallow (len=1000, shift=5): 1 internal level
/// - deep (len=50000, shift=10): 2 internal levels
fn bench_vector_read_depth(c: &mut Criterion) {
    let engine = build_engine().expect("failed to build Wasmtime engine");

    let indexed_cases = [
        // Core size/depth progression
        ("vector_get_tiny", "vector_get_tiny.tw"),
        ("vector_get_shallow", "vector_get_shallow.tw"),
        (
            "vector_get_shallow_matched",
            "vector_get_shallow_matched.tw",
        ),
        ("vector_get_deep", "vector_get_deep.tw"),
        // Depth boundary: 1024 (depth 1) vs 1025 (depth 2), ~2.5M reads each
        ("vector_get_1024", "vector_get_1024.tw"),
        ("vector_get_1025", "vector_get_1025.tw"),
        // Tail-only reads with 16-element tails at different vector sizes
        ("vector_get_tail_48", "vector_get_tail_48.tw"),
        ("vector_get_deep_tail_only", "vector_get_deep_tail_only.tw"),
        ("vector_get_tail_1040", "vector_get_tail_1040.tw"),
    ];

    let iter_cases = [
        ("vector_iter_tiny", "vector_iter_tiny.tw"),
        ("vector_iter_sum", "vector_iter_sum.tw"),
    ];

    let mut group = c.benchmark_group("vector_read_depth");
    // These benchmarks do significant internal looping, so fewer criterion samples suffice.
    group.sample_size(10);

    for (label, file) in indexed_cases.iter().chain(iter_cases.iter()) {
        let path = bench_fixture(file);
        let bytes = build_wasm(&path);
        let module = Module::new(&engine, &bytes).expect("module");

        group.bench_function(BenchmarkId::new("exec", label), |b| {
            b.iter(|| execute_module(&engine, &module).expect("execution failed"))
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_wasm_exec,
    bench_closure_exec,
    bench_collect_strategy_exec,
    bench_vector_read_depth
);
criterion_main!(benches);
