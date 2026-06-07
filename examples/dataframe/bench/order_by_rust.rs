//! Native Rust comparison for the dataframe `order_by("amount", Asc)` path.
//!
//! Run from the repository root:
//!
//!   rustc -O examples/dataframe/bench/order_by_rust.rs -o /tmp/order_by_rust
//!   /tmp/order_by_rust
//!
//! This mirrors `examples/dataframe/bench/main.tw` for order_by only:
//! - generate the same deterministic columns (`key`, `amount`, `score`)
//! - sort a row-index vector by the `amount` column
//! - gather all columns by the sorted indices
//! - report `nrows` as the checksum, matching the Twinkle benchmark
//!
//! It prints two variants:
//! - `native`: Rust's in-place `sort_unstable_by` over row indices
//! - `merge`: a deliberately Twinkle-like recursive merge sort over row indices

use std::time::{Duration, Instant};

#[derive(Clone)]
struct Table {
    keys: Vec<String>,
    amounts: Vec<i64>,
    scores: Vec<f64>,
}

fn next_seed(seed: i64) -> i64 {
    (seed * 1_664_525 + 1_013_904_223) % 2_147_483_648
}

fn gen_table(n: usize, key_cardinality: i64) -> Table {
    let mut keys = Vec::with_capacity(n);
    let mut amounts = Vec::with_capacity(n);
    let mut scores = Vec::with_capacity(n);
    let mut seed = 12_345_i64;

    for _ in 0..n {
        seed = next_seed(seed);
        keys.push(format!("k{}", seed % key_cardinality));

        seed = next_seed(seed);
        amounts.push(seed % 1000);

        seed = next_seed(seed);
        scores.push((seed % 10_000) as f64 / 100.0);
    }

    Table {
        keys,
        amounts,
        scores,
    }
}

fn gather(t: &Table, idx: &[usize]) -> Table {
    let mut keys = Vec::with_capacity(idx.len());
    let mut amounts = Vec::with_capacity(idx.len());
    let mut scores = Vec::with_capacity(idx.len());

    for &i in idx {
        keys.push(t.keys[i].clone());
        amounts.push(t.amounts[i]);
        scores.push(t.scores[i]);
    }

    Table {
        keys,
        amounts,
        scores,
    }
}

fn order_by_amount_native(t: &Table) -> (Table, Duration, Duration) {
    let mut idx: Vec<usize> = (0..t.amounts.len()).collect();

    let sort_start = Instant::now();
    idx.sort_unstable_by(|&a, &b| t.amounts[a].cmp(&t.amounts[b]));
    let sort_elapsed = sort_start.elapsed();

    let gather_start = Instant::now();
    let out = gather(t, &idx);
    let gather_elapsed = gather_start.elapsed();

    (out, sort_elapsed, gather_elapsed)
}

fn merge_sorted(left: Vec<usize>, right: Vec<usize>, amounts: &[i64]) -> Vec<usize> {
    let mut out = Vec::with_capacity(left.len() + right.len());
    let mut i = 0;
    let mut j = 0;

    while i < left.len() && j < right.len() {
        if amounts[left[i]] <= amounts[right[j]] {
            out.push(left[i]);
            i += 1;
        } else {
            out.push(right[j]);
            j += 1;
        }
    }

    out.extend_from_slice(&left[i..]);
    out.extend_from_slice(&right[j..]);
    out
}

fn merge_sort_indices(idx: &[usize], amounts: &[i64]) -> Vec<usize> {
    match idx.len() {
        0 => Vec::new(),
        1 => vec![idx[0]],
        n => {
            let mid = n / 2;
            let left = merge_sort_indices(&idx[..mid], amounts);
            let right = merge_sort_indices(&idx[mid..], amounts);
            merge_sorted(left, right, amounts)
        }
    }
}

fn order_by_amount_merge(t: &Table) -> (Table, Duration, Duration) {
    let idx: Vec<usize> = (0..t.amounts.len()).collect();

    let sort_start = Instant::now();
    let sorted = merge_sort_indices(&idx, &t.amounts);
    let sort_elapsed = sort_start.elapsed();

    let gather_start = Instant::now();
    let out = gather(t, &sorted);
    let gather_elapsed = gather_start.elapsed();

    (out, sort_elapsed, gather_elapsed)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn print_result(label: &str, n: usize, total: Duration, sort: Duration, gather: Duration, checksum: usize) {
    println!(
        "N={:<8} {:<6} total: {:>8.2}ms  sort: {:>8.2}ms  gather: {:>8.2}ms  checksum {}",
        n,
        label,
        ms(total),
        ms(sort),
        ms(gather),
        checksum
    );
}

fn bench_n(n: usize) {
    let base = gen_table(n, 64);

    let start = Instant::now();
    let (sorted, sort_elapsed, gather_elapsed) = order_by_amount_native(&base);
    print_result("native", n, start.elapsed(), sort_elapsed, gather_elapsed, sorted.amounts.len());

    let start = Instant::now();
    let (sorted, sort_elapsed, gather_elapsed) = order_by_amount_merge(&base);
    print_result("merge", n, start.elapsed(), sort_elapsed, gather_elapsed, sorted.amounts.len());
}

fn main() {
    for n in [10_000_usize, 100_000, 1_000_000] {
        bench_n(n);
    }
}
