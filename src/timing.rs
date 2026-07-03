//! Lightweight stage timing, gated by the `TWINKLE_TIMINGS` environment
//! variable. Mirrors the boot compiler's `[time:...]` output so the two
//! compilers can be compared stage-by-stage.
//!
//! When `TWINKLE_TIMINGS` is unset the guards compile down to a cheap env
//! lookup (cached) and an `Instant::now()` that is never read, so leaving the
//! instrumentation in place has negligible cost on normal builds.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("TWINKLE_TIMINGS").is_some())
}

fn accumulators() -> &'static Mutex<HashMap<&'static str, (Duration, u64)>> {
    static ACC: OnceLock<Mutex<HashMap<&'static str, (Duration, u64)>>> = OnceLock::new();
    ACC.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Run `f`, accumulating its elapsed time under `label` (summed across every
/// call). Nothing is printed until [`dump_accumulated`] runs. Used for stages
/// that execute once per module, where a per-call print would be noise.
pub fn timed_acc<T>(label: &'static str, f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();
    let mut acc = accumulators().lock().expect("timing accumulator poisoned");
    let entry = acc.entry(label).or_insert((Duration::ZERO, 0));
    entry.0 += elapsed;
    entry.1 += 1;
    result
}

/// Print all accumulated per-stage totals (label, summed ms, call count),
/// sorted slowest first. No-op unless `TWINKLE_TIMINGS` is set.
pub fn dump_accumulated() {
    if !enabled() {
        return;
    }
    let acc = accumulators().lock().expect("timing accumulator poisoned");
    let mut entries: Vec<_> = acc.iter().collect();
    entries.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (label, (total, calls)) in entries {
        eprintln!(
            "[time:{label}] {:.1}ms ({calls} calls)",
            total.as_secs_f64() * 1000.0
        );
    }
}

/// Run `f`, and if `TWINKLE_TIMINGS` is set print `[time:<label>] <ms>ms` to
/// stderr. Returns whatever `f` returns.
pub fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let start = Instant::now();
    let result = f();
    eprintln!("[time:{label}] {:.1}ms", start.elapsed().as_secs_f64() * 1000.0);
    result
}
