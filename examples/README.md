# Examples

Two kinds of examples live here.

## Single-file demos

The loose `*.tw` files each showcase one feature or idiom — records, enums,
closures, pattern matching, immutability, FFI, and so on. Run any of them
directly:

```bash
target/twk run examples/<name>.tw
```

They are intentionally small; browse the directory to see what is available.

## Stress-test projects

Self-contained projects (each with its own `twinkle.toml`) that exercise
Twinkle at scale to probe **API ergonomics** and **performance**, and to drive
improvements back into the compiler and standard library:

- **`leetcode/`** — API ergonomics on small, self-contained algorithm problems.
- **`dataframe/`** — a columnar query engine; ergonomics *and* performance at
  application scale.
- **`sort-bench/`** — focused performance probes (sorting, dict, typed vector
  reads) with cross-language baselines.

See each project's own README for details. They share the `assert.tw` /
`runner.tw` test harness via symlinks to `boot/tests/`.
