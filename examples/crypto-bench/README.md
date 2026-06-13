# Crypto microbenchmarks

Small cross-language benchmarks for `@std.crypto`.

The Twinkle implementation is pure Twinkle compiled to Wasm GC. The Node,
Python, and Go baselines use their standard-library crypto/encoding packages,
which are native and heavily optimized. Treat them as reference ecosystem
baselines, not apples-to-apples pure-language implementations.

## Run

```bash
examples/crypto-bench/run.sh
```

Output columns:

```text
lang    bench    iters    ms    sink    us_per_op
```

`sink` consumes output bytes so the timed work cannot be trivially eliminated.

Benchmarks:

- `*_small`: digest a short fixed message.
- `*_4k`: digest a reused 4 KiB byte buffer.
- `hmac_sha256_small`: HMAC over a short fixed key/message.
- `base64_roundtrip_4k`: Base64 encode then decode the 4 KiB buffer.
