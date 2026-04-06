use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::Parser;
use wasmtime::Module;

use twinkle::cli::run_wasm::{build_engine, execute_module};

#[derive(Parser)]
#[command(name = "bench_exec")]
#[command(about = "Build a Twinkle program once, then benchmark Wasm execution only")]
struct Cli {
    /// Path to the benchmark .tw file
    file: String,
    /// Number of measured execution runs
    #[arg(long, default_value_t = 10)]
    runs: usize,
    /// Number of warmup executions before measuring
    #[arg(long, default_value_t = 1)]
    warmup: usize,
}

fn build_wasm(file_path: &str) -> Result<Vec<u8>> {
    let wat = twinkle::cli::build::build_wat(file_path).context("build_wat failed")?;
    wat::parse_str(&wat).context("WAT parse failed")
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.runs == 0 {
        bail!("--runs must be > 0");
    }

    let path = PathBuf::from(&cli.file);
    let engine = build_engine().context("failed to build Wasmtime engine")?;
    let bytes = build_wasm(
        path.to_str()
            .ok_or_else(|| anyhow::anyhow!("non-utf8 benchmark path"))?,
    )?;
    let module = Module::new(&engine, &bytes).context("failed to compile Wasm module")?;

    for _ in 0..cli.warmup {
        execute_module(&engine, &module).context("warmup execution failed")?;
    }

    let mut samples = Vec::with_capacity(cli.runs);
    for _ in 0..cli.runs {
        let start = Instant::now();
        execute_module(&engine, &module).context("measured execution failed")?;
        samples.push(start.elapsed().as_secs_f64());
    }

    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[samples.len() / 2];
    let min = samples[0];
    let max = samples[samples.len() - 1];

    println!("file={}", path.display());
    println!("runs={}", cli.runs);
    println!("median_seconds={median:.6}");
    println!("min_seconds={min:.6}");
    println!("max_seconds={max:.6}");
    println!(
        "samples_seconds={}",
        samples
            .iter()
            .map(|s| format!("{s:.6}"))
            .collect::<Vec<_>>()
            .join(",")
    );

    Ok(())
}
