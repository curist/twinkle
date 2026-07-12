# Compiled-Program Performance

This is the umbrella track for making programs compiled by Twinkle run faster.
Compiler throughput has its own track in [compiler.md](compiler.md), although
runtime improvements often help the compiler too because the self-hosted compiler
is itself a Twinkle program.

## Current thesis

The biggest remaining runtime wins are structural representation wins, not local
micro-optimizations. The recurring pattern across the benchmark docs is that hot
programs become slow when concrete values cross erased or copying boundaries:

- primitive vectors stored as `anyref` payloads;
- repeated boxed PVec reads inside sort comparators;
- universal runtime helper APIs that box, cast, or dispatch through erased shapes;
- byte data crossing between `Vector<Byte>`, `String`, and linear memory;
- substring allocation for comparisons or scans.

The long-term direction is therefore: keep source-level value semantics, but use
more precise backend representations and helper families for concrete
monomorphized code.

## Active priorities

### 1. Typed vectors and `anyref` elimination

Lead docs:

- [backend-anyref-elimination.md](backend-anyref-elimination.md) — the
  architecture-parent plan for this priority (typed container/helper families,
  representation-boundary policy)
- [vector/typed-vector-representation.md](vector/typed-vector-representation.md)
  — the concrete `Vector<Int>` family delivering the first piece of it

`Vector<Int>` typed storage is the current master lever for numeric/dataframe
workloads. The immediate runtime unlock is carrying typed vector representation
through realistic boundaries, especially variant payloads used by dataframe
columns. The broader target — making `anyref` exceptional rather than the
default backend representation — is owned by the architecture-parent plan above,
whose Phase 1 (declare the representation-boundary policy) is the current gate.

### 2. Vector reads, `sort_by`, and dataframe `order_by`

Lead docs:

- [vector/README.md](vector/README.md)
- [vector/generic-sort-by-vector-read-perf.md](vector/generic-sort-by-vector-read-perf.md)
- [dataframe/README.md](dataframe/README.md)

Generic comparator mechanics have already seen meaningful wins, but key-index
sorts remain dominated by random vector reads. Persistent-only dense merge work
and comparator-shape recognition are secondary unless typed storage reaches the
values being read.

### 3. Persistent collection runtime cleanup

Reference docs:

- [../archive/pvec-performance-enhancements.md](../archive/pvec-performance-enhancements.md)
- [../archive/dict-performance-enhancements.md](../archive/dict-performance-enhancements.md)
- [../sound-uniqueness/](../sound-uniqueness/)
- [../archive/static-uniqueness-plan.md](../archive/static-uniqueness-plan.md) — historical notes from the older optimizer line

Useful incremental work includes cheaper PVec builders and bulk conversions,
HAMT `popcnt`, avoiding double dict traversals, and carefully gated in-place
updates. These should stay compatible with the typed-container direction rather
than entrenching erased payload storage.

### 4. Generated-code quality and numeric loops

Reference docs:

- [../archive/awfy-codegen-gaps.md](../archive/awfy-codegen-gaps.md)
- [../archive/awfy-c5-inplace-vector.md](../archive/awfy-c5-inplace-vector.md)

AWFY remains the broad generated-code signal. Several intuitive codegen levers
are already resolved or rejected: native float intrinsics and dead `Void`
elimination landed; generic local peepholes, broad record-allocation elimination,
and naive field caching did not justify themselves under V8.

### 5. Bytes, strings, Buffer, and IO

Reference docs:

- [../archive/buffer-linear-memory.md](../archive/buffer-linear-memory.md)
- [../archive/crypto-perf.md](../archive/crypto-perf.md)
- [../archive/slice-performance.md](../archive/slice-performance.md)

Linear-memory `Buffer` is a strong fit for dense byte codecs and IO-originated
bytes. It is not a general replacement for `Vector`, and it only helps when it
removes a boundary rather than adding a copy. String scanning should move toward
allocation-free region checks and views rather than repeated substring
allocation.

## Current “now / next / later” queue

### Now

- Extend typed `Vector<Int>` routing through variant payloads so dataframe
  `IntCol(Vector<Int>)` can keep typed storage.
- Add probes that verify variant-held column reads use typed helpers rather than
  boxed PVec reads.
- Keep the vector/order-by benchmark gate current.

### Next

- Route the surrounding operations that matter for dataframe/order-by: typed
  `len`, indexed reads, gather, sort-support paths, and boundary coercions.
- Revisit typed-buffer merge only after typed storage reaches the hot values.
- Pick small PVec/Dict runtime cleanups that are measurable and low risk.

### Later

- Broaden typed container/helper families beyond `Vector<Int>`.
- Add typed arithmetic paths for i32-heavy code such as hash functions.
- Explore Buffer-native IO and byte-codec paths.
- Reassess numeric-loop scalar residency only where benchmarks show V8 is not
  already recovering the shape.

## Benchmark map

| Workload | Purpose | Location |
|---------|---------|----------|
| AWFY | Broad generated-code/runtime signal | `examples/performance/awfy/` |
| Vector/sort probes | Component breakdown for sort, reads, comparator costs | `examples/performance/sort-bench/` |
| Dataframe | App-scale `order_by`, gather, group/join context | `examples/performance/dataframe/` |
| Crypto/base64 | Byte/string/buffer boundary costs | `examples/performance/crypto-bench/` |
| Buffer codec probes | Dense byte-indexing validation | archived Buffer docs |

## Ruled-out or deprioritized levers

- Comparator micro-optimizations as the path to dataframe parity.
- Persistent-vector-only flat-buffer merge as a standalone change.
- Opaque `anyref` scratch buffers for generic sorting.
- Broad local peephole coalescing as a runtime optimization.
- Retrying host `Math` FFI fixes for operations that Wasm cannot express.
- In-place vector `set_at` as a self-compilation speed lever.
