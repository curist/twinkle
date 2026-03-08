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

criterion_group!(
    benches,
    bench_wasm_exec,
    bench_closure_exec,
    bench_collect_strategy_exec
);
criterion_main!(benches);
