# Typed Dict Representation

**Status: ARCHIVED / NOT PURSUED (Phase 0 gate, 2026-06-12).** Phase 0
microbenchmarks (`boot/bench/dict_*`, `boot/bench/set_*`) measured the actual
cost split and the premise did not hold:

- **Key boxing is already negligible.** `dict_bigint_build` (every key a heap
  `BoxedInt`) is within noise of `dict_int_build` (every key an unboxed i31):
  7.12 vs 7.13 ms @32k. `set` is dominated by HAMT node alloc + path-copy +
  insertion-order append (`build − get` ≈ 80% of build), not key handling. A
  typed `i64` key saves ≈0 — the typed-vector analogy does not transfer.
- **The only real key-typing win is String reads** (~2× int reads:
  `str_has` 2.27 vs `int_has` 1.16 @32k), i.e. `hash_string`+generic `core_eq`
  vs a direct path. String *build* stays alloc-bound. A narrow, modest lever —
  not worth the family-parameterization + routing + boundary machinery this
  plan describes.
- **Surfaced the real structural cost:** `remove` is O(n) per call (insertion-
  order vector rebuild) → bulk remove is O(n²) (10 s @32k). Key typing is
  irrelevant to it. This — and node-allocation cost generally — is the lever
  worth chasing, tracked separately.

The Phase 0 benchmarks and full baseline table are kept as a permanent
regression guard in `boot/bench/README.md` ("Dict / Set benchmark suite"). The
design below is retained for reference only; do not implement without new
evidence that key representation (not allocation/order-tracking) is the
bottleneck.

---

## Goal

Make common `Dict<K, V>` and `Set<K>` workloads faster by using typed runtime
families for monomorphic key types instead of routing every key through
`anyref`. Target the key types that dominate the boot compiler first:

- `Int`
- `String`

This is the `Dict` analogue of the typed-vector representation work: keep the
source-level `Dict<K, V>` abstraction, but let the backend choose a more precise
physical representation when monomorphization and use-site analysis make it
safe.

---

## Motivation

The current runtime dict is a persistent HAMT with insertion-order tracking:

```text
PDict {
  size: i32,
  root: HamtNode?,
  order: PVec,
}

HamtEntry {
  hash: i64,
  key: anyref,
  val: anyref,
}
```

This is semantically solid and already competitive, but each `Dict<Int, V>`
operation pays generic representation costs:

- boxed keys at container boundaries;
- `hash_key(anyref)` dispatch;
- `rt.core.eq` for key comparison;
- generic `Array<anyref>` entry payloads;
- insertion-order keys stored in generic `PVec`.

Recent vector work removed the analogous gap for selected `Vector<Int>` paths by
routing eligible values to `PVecI64` and typed runtime helpers. Dicts should use
the same strategy, starting with typed key families.

---

## Reference benchmark

Current `make bench-compare` compares Twinkle against Clojure persistent
collections. This is a coarse single-run benchmark; use it as directional
context, not as a statistically rigorous result.

Run shape:

```bash
make bench-compare
```

Result from Apple Silicon / local `target/twk` run:

```text
summary (ms; lower is better)
workload               N         twinkle   clojure
---------------------  --------  --------  --------
vector_append_build    1000      0.123     0.301
vector_append_build    5000      0.128     0.573
vector_append_build    10000     0.214     0.697
vector_random_get      1000      0.012     0.873
vector_random_get      5000      0.121     0.739
vector_random_get      10000     0.280     1.027
vector_persistent_set  1000      0.106     0.649
vector_persistent_set  5000      0.580     3.533
vector_persistent_set  10000     1.522     1.281
dict_assoc_build       1000      0.220     0.315
dict_assoc_build       5000      0.966     0.853
dict_assoc_build       10000     1.489     1.448
dict_random_get        1000      0.115     0.258
dict_random_get        5000      0.408     0.627
dict_random_get        10000     0.755     1.083
set_insert_build       1000      0.169     0.235
set_insert_build       5000      0.720     0.744
set_insert_build       10000     1.940     1.319
set_contains           1000      0.062     0.221
set_contains           5000      0.242     0.618
set_contains           10000     0.420     1.281
```

Takeaways:

- Twinkle vectors are generally faster than Clojure in this shape, which is
  consistent with the recent typed-vector work.
- Twinkle dict reads are faster here, but dict build/update is roughly tied with
  Clojure and slightly behind at larger sizes.
- Twinkle set contains is faster here, but set insert is behind at larger sizes.
- Since Twinkle dicts preserve insertion order and Clojure's ordinary persistent
  map/set do not, Twinkle is already paying for extra semantics in update-heavy
  workloads.

---

## Boot compiler usage signal

Static `Dict<K, V>` annotations in `boot/compiler` are strongly biased toward
`Int` keys, with `String` keys as the second major group:

```text
total Dict annotations: 809

Int keys:     511
String keys:  294
other:          4
```

Top pairs:

```text
Dict<Int, Bool>
Dict<String, Bool>
Dict<Int, MonoType>
Dict<Int, Int>
Dict<String, Int>
Dict<String, MonoType>
Dict<String, FuncId>
Dict<String, String>
Dict<Int, String>
Dict<Int, TypeId>
```

`Int`-key dicts are concentrated in compiler analyses and backend maps:

- ownership/liveness/use-count analysis;
- slot assignment and verification;
- typed-vector routing;
- codegen planning/emission context;
- ID-to-type/function/local lookup tables.

`String`-key dicts are concentrated in name/module-oriented code:

- resolver scopes;
- monomorphization caches;
- linker/module maps;
- codegen symbol maps;
- unused-import and query/document analysis.

So the highest-value target is not a single narrow `Dict<Int, Int>` case. It is
specialized key families:

```text
Dict<Int, V>
Dict<String, V>
Set<Int>
Set<String>
```

---

## Design direction

Specialize by key type first and keep values erased initially.

### Runtime families

Start with these physical families:

```text
PDictI64KeyAny
PDictStringKeyAny
```

Possible entry layouts:

```text
HamtEntryI64KeyAny {
  hash: i64,
  key: i64,
  val: anyref,
}

HamtEntryStringKeyAny {
  hash: i64,
  key: String,
  val: anyref,
}
```

The HAMT node/collision shapes can mirror the existing generic dict, but use
entry/collision structs specific to the family.

For `Int` keys:

- call `hash_i64` directly;
- compare keys with `i64.eq`;
- store insertion order as `PVecI64` where possible.

For `String` keys:

- call `hash_string` directly;
- compare strings with a direct string-equality helper rather than generic
  `rt.core.eq`;
- store insertion order as ordinary `PVec`, unless/until there is a typed string
  vector family.

Values remain `anyref` in this phase. That avoids the hardest part of a full
`Dict<Int, Int>` specialization: returning absence plus an unboxed value.

---

## Why key specialization first

Key specialization attacks the costs that every dict operation pays:

- hash dispatch;
- key boxing/unboxing;
- equality dispatch;
- collision scans;
- order-vector key storage.

It also benefits `Set<K>` immediately, since `Set` is backed by `Dict<K, Void>`.
A value-specialized dict only helps operations that read/write primitive values,
whereas key specialization helps `has`, `get`, `set`, `remove`, and set ops.

---

## Semantics and representation boundaries

The source-level semantics do not change:

- dicts remain persistent values;
- insertion order is preserved;
- `keys()` preserves insertion order;
- equality remains structural and order-independent;
- generic `Dict<K, V>` APIs keep working.

Typed dicts need representation-boundary rules like typed vectors:

1. A monomorphic local can stay typed while all operations are representation-aware.
2. Passing to representation-polymorphic code erases to generic `PDict` unless a
   specialized clone of that code is generated.
3. Returning a typed dict through an ordinary ABI either preserves typed ABI when
   caller/callee agree or boxes/erases at the boundary.

Initial implementation should be conservative and erase at uncertain boundaries.

---

## Compiler routing

Add a routing pass analogous to `route_typed_vec.tw`, tentatively:

```text
boot/compiler/backend/route_typed_dict.tw
```

The pass should run after representation assignment, while prepared slots still
carry enough monomorphic type information to identify:

```text
Dict<Int, V>
Dict<String, V>
Set<Int>
Set<String>
```

It rewrites builtin calls when the receiver slot is eligible:

```text
dict$new             -> dict$new_i64_key_any / dict$new_string_key_any
dict$set             -> dict$set_i64_key_any / dict$set_string_key_any
dict$set_in_place    -> typed variants
dict$get             -> typed variants
dict$get_unsafe      -> typed variants
dict$has             -> typed variants
dict$remove          -> typed variants
dict$remove_in_place -> typed variants
dict$keys            -> typed keys variant or erasing adapter
```

For `Set<K>`, routing can happen either through the existing `Dict<K, Void>` calls
or through explicit set-specialized builtins if that becomes clearer.

Eligibility should start conservative:

- local dicts built and consumed inside one function;
- direct calls to known dict builtins;
- no generic boundary use unless an erasing adapter is inserted;
- no `keys()` typed result unless the result representation is handled.

After correctness is stable, broaden eligibility to params, returns, and record
fields using the same whole-program field analysis pattern used by typed vectors.

---

## Runtime API additions

Add builtin registry entries and runtime exports for the typed-key families.

For `Int` keys:

```text
dict$new_i64_key_any() -> PDictI64KeyAny
dict$set_i64_key_any(PDictI64KeyAny?, i64, anyref) -> PDictI64KeyAny
dict$set_i64_key_any_in_place(PDictI64KeyAny?, i64, anyref) -> PDictI64KeyAny
dict$get_i64_key_any(PDictI64KeyAny?, i64) -> Variant / Option-erased helper
dict$get_unsafe_i64_key_any(PDictI64KeyAny?, i64) -> anyref
dict$has_i64_key_any(PDictI64KeyAny?, i64) -> i32
dict$remove_i64_key_any(PDictI64KeyAny?, i64) -> PDictI64KeyAny
dict$remove_i64_key_any_in_place(PDictI64KeyAny?, i64) -> PDictI64KeyAny
dict$keys_i64_key_any(PDictI64KeyAny?) -> PVecI64 or PVec
```

For `String` keys:

```text
dict$new_string_key_any() -> PDictStringKeyAny
dict$set_string_key_any(PDictStringKeyAny?, String?, anyref) -> PDictStringKeyAny
dict$get_string_key_any(...)
dict$has_string_key_any(...)
dict$remove_string_key_any(...)
dict$keys_string_key_any(...) -> PVec
```

Names can be shortened before implementation. The important distinction is the
physical key family, not the exact spelling.

---

## `get` and `Option<V>` handling

Values stay erased until Phase 4, so typed-key `get` can preserve the current
null-on-miss internal ABI:

```text
get_i64_key_any(dict, key) -> anyref?  // null means missing
```

Codegen then performs the same concrete `Option<V>` construction it does today:

```text
null -> None
value anyref -> unbox/egress V -> Some(value)
```

This avoids introducing multi-value `(found, value)` returns until value
specialization is needed.

---

## Insertion-order keys

Twinkle dicts preserve insertion order, unlike Clojure's ordinary persistent map.
Typed-key dicts must keep that behavior.

For `Int` keys, prefer storing order as `PVecI64`:

```text
PDictI64KeyAny.order: PVecI64
```

This avoids reintroducing key boxing during `set` and makes `keys()` cheaper for
`Dict<Int, V>`. If `keys()` must return the ordinary source-level `Vector<Int>`
ABI, codegen can keep the typed result where representation-known or box through
`PVecI64 -> PVec` at a boundary.

For `String` keys, ordinary `PVec` is acceptable because strings are already
references and do not suffer primitive boxing.

---

## Equality

Generic dict equality currently relies on runtime structural equality. Typed dict
equality needs either:

1. erase both typed dicts to generic `PDict` before equality; or
2. add typed equality helpers; or
3. route equality based on physical representation.

Use erasure first if equality is uncommon in hot paths. Add typed equality once
benchmarks show it matters.

---

## Implementation phases

### Phase 0 — measurement and baselines

- Keep `tools/bench_persistent_compare.py` as the Clojure comparison harness.
- Add Twinkle-only dict/set microbenchmarks under `boot/bench/` for:
  - `Dict<Int, Bool>` build/has/remove;
  - `Dict<Int, Int>` build/get/set;
  - `Dict<String, Bool>` build/has/remove;
  - `Dict<String, Int>` build/get/set.
- Use these as regression guards while runtime families land.

### Phase 1a — parameterize the existing emitter (behavior-preserving)

The runtime dict lives in `boot/compiler/codegen/runtime/dict.tw` as `FuncDef`
builders that emit Wasm instruction arrays — not ordinary HAMT source. Before
adding any new family, refactor those builders to take a family descriptor as
their first argument, then instantiate it with an `anyref`-key descriptor that
reproduces today's output. The key-type-specific surface is narrow and local
(see `node_get`): the key param type, the key-compare op (`core_eq` →
inlinable), the hash op (`hash_key` dispatch → inline `hash_i64`/`hash_string`),
the entry struct name, the insertion-order vector op, and the name prefix on
self/cross calls. Everything else — bitmap fragment math, `popcount`, slot
indexing, the `RefTest(entry/node/collision)` dispatch, the structural recursion
— is key-agnostic.

```tw
type DictFamily = .{
  prefix: String,        // "" | "i64_key_any" | "string_key_any"
  key_ty: ValType,       // .Anyref | .I64 | .Ref(String)
  entry_ty: String,      // entry struct holding the typed key
  eq_op: Vector<Instr>,  // [.Call("core_eq")] | [.I64Eq] | [.Call("string_eq")]
  hash_op: Vector<Instr>,
  order: OrderRepr,      // PVec | PVecI64
}
```

Validation gate: regenerate the generic family through the parameterized builder
and diff the emitted WAT against current `make stage2` output. It must be
**byte-identical** before any new family is added. This de-risks everything
downstream and drops the marginal cost of each new key family to roughly one
descriptor record plus one typed entry struct.

Share, don't parameterize, the genuinely key-agnostic helpers (`popcount`,
`arr_insert_at`/`replace_at`/`remove_at`, fragment math) the way `arr.tw` shares
its trie-build helpers across the boxed and `PVecI64` families. The difference
from vectors: vector trie nodes treat leaves as opaque `ref eq`, so the whole
core is shared untouched and only thin typed wrappers are cloned. HAMT traversal
reaches *into* the entry to compare the key, so the comparison-bearing functions
(`node_get`/`node_set`/`node_remove`/`collision_*`) cannot be shared verbatim —
they are exactly what the descriptor specializes (eq/hash inlined per family).

### Phase 1b — instantiate `PDictI64KeyAny`

- Add typed runtime structs in `boot/compiler/codegen/runtime/types.tw`.
- Instantiate the parameterized builders with the `i64`-key descriptor
  (`hash_i64`, inline `i64.eq`, typed entry struct).
- Store insertion order as `PVecI64` if practical; otherwise start with generic
  `PVec` and follow up.
- Add builtin entries and direct codegen emission for typed-key operations.
- Add conservative routing for local `Dict<Int, V>` values.

Because Phase 1a makes a new family cheap, the i64 family can be stood up
speculatively to feed the Phase 0 measurements rather than being a large up-front
bet — the benchmark numbers, not the build cost, decide whether it ships.

### Phase 2 — `PDictStringKeyAny`

- Add string-key runtime family.
- Use direct `hash_string`.
- Add or expose a direct string equality helper.
- Keep insertion order as generic `PVec`.
- Add routing for local `Dict<String, V>` values.

### Phase 3 — broaden routing

- Route params/returns when caller and callee agree on representation.
- Add record-field analysis for typed dict fields, following the typed-vector
  field approach.
- Add boundary erasure adapters where needed.

### Phase 4 — value specialization, only if justified

Consider value-specialized families after typed keys are measured:

```text
PDictI64KeyI64
PDictStringKeyI64
```

These require a different `get` ABI because raw primitive values cannot use null
as a missing sentinel:

```text
get_i64_key_i64(dict, key) -> (found: i32, value: i64)
```

Do not start here unless benchmarks show key specialization leaves a major value
boxing hotspot.

---

## Risks and non-goals

### Risk: runtime duplication

A naive port would clone the non-trivial HAMT emitter per family. Avoid that:
parameterize the existing `dict.tw` builders by a `DictFamily` descriptor up
front (Phase 1a) instead of cloning, and prove the refactor by reproducing the
generic family bit-identical. A new family then costs one descriptor plus one
typed entry struct. Note this is *more* involved than the `arr.tw` typed-vector
work: there, opaque `ref eq` leaves let the trie core be shared untouched, so
only thin wrappers were cloned. The HAMT's traversal inspects the key, so the
comparison-bearing functions must be specialized (eq/hash inlined) per family —
sharing alone does not cover them.

### Risk: representation-boundary bugs

Typed vectors already showed that boundary handling is the hard part. Keep early
routing local and conservative, then expand deliberately.

### Risk: too many representation families

Do not build a full `K × V` matrix. Start with typed keys and erased values:

```text
Int key + any value
String key + any value
```

Only add value-specialized families based on benchmark evidence.

### Non-goal: changing dict semantics

Do not drop insertion-order tracking to match Clojure performance. Twinkle's
ordered keys are part of the current behavior and should remain.

---

## Success criteria

- `Dict<Int, V>` and `Set<Int>` update/read microbenchmarks improve without
  regressing generic dict behavior.
- `Dict<String, V>` benchmarks improve or at least avoid regressions while using
  direct hash/equality paths.
- Boot compiler workloads that are heavy in analysis/backend maps show improved
  run time.
- Typed dict routing remains invisible at the language level and preserves all
  existing dict/set API behavior.
