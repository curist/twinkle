# sort-bench

A collection of **performance probes** for Twinkle.

Unlike `leetcode` and `dataframe`, this is not a test suite — there is no
`main.tw`. Each `*_probe.tw` / `*_micro.tw` is a standalone, self-timing
investigation into one performance question, written to attribute cost in a hot
path and to compare against a reference implementation. Each file's header
comment states what it measures; most trace back to specific compiler work
(e.g. `sort_by` comparator mechanics, the typed-vector representation, persistent
`Dict` throughput).

The `*.clj` (and other-language) files are cross-language baselines — Clojure's
persistent collections, etc. — for the same workload, so a Twinkle number can be
read against a known reference.

These are kept as evidence and as ready-made re-measurement tools; they are
expected to grow and churn as performance work continues, rather than being a
stable, maintained corpus.

## Run

Run a probe directly (build `target/twk` first via `make bundle-cli`):

```bash
target/twk run examples/sort-bench/value_sort_micro.tw
```

Baselines run with their own toolchains, e.g.:

```bash
clojure -M examples/sort-bench/value_sort_clojure.clj
```
