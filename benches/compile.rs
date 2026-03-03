use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/run")
}

/// check_entry: parse + resolve + typecheck only (no lowering)
fn bench_check(c: &mut Criterion) {
    let dir = fixtures_dir();

    let mut group = c.benchmark_group("check");

    let cases = [
        ("hello", "hello.tw"),
        ("iterator_advanced", "iterator_advanced.tw"),
        ("multi_module", "multi_module/main.tw"),
        ("twinkle_typechecker", "twinkle_typechecker.tw"),
    ];

    for (name, rel_path) in cases {
        let path = dir.join(rel_path).to_string_lossy().into_owned();
        group.bench_function(name, |b| {
            b.iter(|| twinkle::module::check_entry(&path).expect("check failed"))
        });
    }

    group.finish();
}

/// check_entry with cold cache: reset query cache before each iteration.
fn bench_check_cold(c: &mut Criterion) {
    let dir = fixtures_dir();

    let mut group = c.benchmark_group("check_cold");

    let cases = [
        ("hello", "hello.tw"),
        ("iterator_advanced", "iterator_advanced.tw"),
        ("multi_module", "multi_module/main.tw"),
        ("twinkle_typechecker", "twinkle_typechecker.tw"),
    ];

    for (name, rel_path) in cases {
        let path = dir.join(rel_path).to_string_lossy().into_owned();
        group.bench_function(name, |b| {
            b.iter(|| {
                twinkle::query::cache::reset_global_cache();
                twinkle::module::check_entry(&path).expect("check failed")
            })
        });
    }

    group.finish();
}

/// compile_entry: full pipeline including lowering to Core IR
fn bench_compile(c: &mut Criterion) {
    let dir = fixtures_dir();

    let mut group = c.benchmark_group("compile");

    let cases = [
        ("hello", "hello.tw"),
        ("iterator_advanced", "iterator_advanced.tw"),
        ("multi_module", "multi_module/main.tw"),
        ("twinkle_typechecker", "twinkle_typechecker.tw"),
    ];

    for (name, rel_path) in cases {
        let path = dir.join(rel_path).to_string_lossy().into_owned();
        group.bench_function(name, |b| {
            b.iter(|| twinkle::module::compile_entry(&path).expect("compile failed"))
        });
    }

    group.finish();
}

/// compile_entry with cold cache: reset query cache before each iteration.
fn bench_compile_cold(c: &mut Criterion) {
    let dir = fixtures_dir();

    let mut group = c.benchmark_group("compile_cold");

    let cases = [
        ("hello", "hello.tw"),
        ("iterator_advanced", "iterator_advanced.tw"),
        ("multi_module", "multi_module/main.tw"),
        ("twinkle_typechecker", "twinkle_typechecker.tw"),
    ];

    for (name, rel_path) in cases {
        let path = dir.join(rel_path).to_string_lossy().into_owned();
        group.bench_function(name, |b| {
            b.iter(|| {
                twinkle::query::cache::reset_global_cache();
                twinkle::module::compile_entry(&path).expect("compile failed")
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_check,
    bench_check_cold,
    bench_compile,
    bench_compile_cold
);
criterion_main!(benches);
