# Generic `sort_by` and Vector Read Performance Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** active performance direction for dataframe `order_by`. This supersedes treating native key-index argsort recognition as the primary fix. Transparent argsort recognition can remain an optional fast path later, but the main goal is to make idiomatic callback sorting and indexed vector reads fast enough without requiring a specialized API.

**Reprioritized 2026-06-09 after measurement.** The realistic key-index path is ~7× slower than Clojure (~2.37s vs ~0.34s at N=1M), and that gap is dominated by **vector read cost** and **persistent-merge allocation** — both structural — not by comparator mechanics. Comparator micro-opts (closure representation, enum/`Order` allocation) were measured and are real but small (~6% of the gap combined). Tracks below are reordered accordingly: read path first, flat-buffer merge second, comparator mechanics last. See [Measured decomposition](#measured-decomposition-2026-06-09).

**Goal:** Improve the normal Twinkle path:

```tw
idx.sort_by(fn(a, b) {
  // arbitrary comparator code, including metrics/debug side effects
  Int.compare(keys[a], keys[b])
})
```

The comparator must still run with Twinkle's ordinary callback semantics. Programs that count comparator calls, log comparisons, or otherwise observe callback execution must remain correct. The optimization target is therefore the cost around and inside the generic path: sort mechanics, closure calls, `Order` handling, and repeated `Vector[index]` reads.

## Why this plan exists

The earlier native typed value-sort work proved that dense typed kernels can be very fast for `xs.sort()` over primitive values, but dataframe `order_by` uses `idx.sort_by(comparator)`. A narrow recognizer could lower a known pure dataframe comparator to internal argsort, but that does not help side-effecting or merely unrecognized idiomatic callbacks.

Clojure's persistent-vector comparator benchmark remains far ahead even when the comparator increments an atom. That means avoiding callbacks entirely is not the only viable path; Twinkle's generic comparator path itself is too expensive.

## Current measurements and component breakdown

Measured locally with:

```bash
target/twk run examples/sort-bench/sort_by_component_probe.tw
target/twk run examples/dataframe/bench/sort_by_costs.tw
target/twk run examples/dataframe/bench/order_by_breakdown.tw
clojure /tmp/order_by_clj_side_effect.clj
```

`sort_by_component_probe.tw` is the clean component probe. Representative N = 1,000,000:

| probe | result | what it isolates |
|---|---:|---|
| native `xs.sort()` over the same shuffled Int input | ~106 ms | typed native value-sort lower bound for this data |
| generic `sort_by(fn(a,b){ Int.compare(a,b) })` | ~747–753 ms | generic sort mechanics + comparator calls, no external key reads |
| generic `sort_by` + `Cell.update` counter | ~836–857 ms | same path with observable side-effecting comparator |
| observed comparator calls | ~16.8M | call count for the generic merge on this input |
| closure call + `Int.compare`, same call scale | ~122 ms | approximate direct closure-call/comparison floor |
| `Cell.update`, same call scale | ~90 ms | approximate counter side-effect floor |
| random `Vector<Int>` reads, same scale | ~265 ms | indexed read cost outside sorting |
| building chunked vectors via `.append`, same scale | ~543 ms | rough builder/append allocation cost scale |
| key-index `sort_by(fn(a,b){ Int.compare(keys[a], keys[b]) })` | ~2.36 s | generic sort plus repeated key reads inside comparator |

The earlier ad-hoc generic `sort_by(Int.compare)` readings around ~3.2–3.5s were not stable in the clean probe and appear sensitive to harness shape, memory/GC pressure, or cold-run context. Keep both kinds of probes: a clean component probe for decomposition and the dataframe benchmark for end-to-end reality.

Current interpretation:

- Generic `sort_by` mechanics are not free: even the no-key-read comparator path is far above native typed value sort.
- External vector reads inside the comparator are the bigger multiplier for dataframe/key-index sort: adding `keys[a]` / `keys[b]` lifts the clean probe from ~0.75s to ~2.36s.
- Side-effecting comparator instrumentation is real but not catastrophic: adding a `Cell.update` counter to the simple comparator adds roughly the same order as the standalone counter probe, and still preserves semantics.

## Measured decomposition (2026-06-09)

We measured the comparator-path micro-costs directly and ran the Clojure reference, to decide where the time actually is. This section replaces the earlier guess that comparator mechanics were the primary lever.

### The gap vs Clojure

At N = 1M, key-index sort phase:

| path | sort phase |
|---|---:|
| Clojure persistent-vector key-index sort (`order_by_clojure_persistent.clj`) | ~340 ms |
| Twinkle `sort_by(fn(a,b){ Int.compare(keys[a], keys[b]) })` | ~2367 ms |
| Twinkle generic `sort_by(Int.compare)`, no key reads | ~743 ms |

The realistic key-index gap is **~7×**.

### Where Twinkle's 2367 ms goes

- **Key reads inside the comparator: ~1624 ms** (2367 − 743). That is 2 × `keys[…]` per comparison × ~16.8M comparisons ≈ ~34M persistent-vector reads. These are *random* reads (the row id `a` is shuffled), so they hit the cache-hostile ~16 ns/read regime of the random-read probe.
- **Generic merge mechanics: ~720 ms** — and this alone is already **~2× Clojure's entire sort** with zero key reads.

So the realistic path is **read-bound (~69% reads, ~31% merge mechanics)**, and even the mechanics half is 2× Clojure on its own.

### Attributing the ~720 ms mechanics half

`examples/sort-bench/merge_attribution_probe.tw` ablates the real recursive merge sort (validated: its full variant reproduces `sort_by`, ~720 vs ~731 ms). At N = 1M:

| layer | time | component priced |
|---|---:|---|
| R — reads + compares + recursion, no allocation | ~561 ms | what a flat-buffer in-place merge still pays |
| S — R + base-case singleton `[xs[lo]]` allocation | ~570 ms | singleton allocation |
| B — real merge building output via `.append` | ~720 ms | append + output-vector allocation |

Deltas:

- **Singleton allocation (S − R) ≈ ~8–14 ms → negligible.** My earlier concern that ~n `[xs[lo]]` singletons dominated was wrong; they are cheap.
- **Append + output-vector allocation (B − S) ≈ ~150 ms (~21%).** This is the only part a flat-buffer redesign over *persistent* vectors removes.
- **Total allocation overhead (B − R) ≈ ~155 ms.** A flat-buffer merge that stays on persistent storage caps out here ≈ **~6.5% of the 2367 ms key-index path.**

The dominant cost of the mechanics half is the **~561 ms floor of reads + comparator calls + recursion** — i.e. reads again, not allocation. R still reads through the PVec (sequential, so cheaper than the random key reads), so a *typed flat buffer* (Track 1) could push the merge below R, not merely shave the ~155 ms.

**Synthesis: reads dominate everywhere — random key reads (~1624 ms) and the sequential merge-floor reads (much of ~561 ms).** The master lever is typed flat `Vector<Int>` storage: it makes random key reads cheap *and* lets the merge run over a native buffer (cheap sequential reads + no per-level allocation) in one change. A persistent-only flat-buffer merge is a ~6.5% lever on its own and is therefore folded into the typed-storage work rather than pursued separately.

### Re-measure under tiering control (2026-06-10): the gap is structural, not warm-up

All earlier probes were single-shot cold runs while the Clojure reference was warmed,
leaving open that part of the 7× was V8 tier-up (Liftoff → TurboFan) rather than
structure. `examples/sort-bench/sort_repeat_probe.tw` runs each phase three times
in-process; we also ran it under forced `--v8-flags=--no-liftoff,--no-wasm-lazy-compilation`
(via `deno run -A --v8-flags=… tools/js_runtime/deno_main.mjs run …`). At N = 1M:

| path | run 1 | runs 2–3 | forced TurboFan |
|---|---:|---:|---:|
| native `xs.sort()` | ~102 ms | **~58–63 ms** | ~57–75 ms |
| generic `sort_by(Int.compare)` | ~734 ms | ~704–746 ms | ~754–776 ms |
| key-index `sort_by` | ~2280 ms | ~2240–2280 ms | ~2360–2400 ms |

Conclusions:

- **The generic and key-index numbers are tier-stable.** The 7× key-index gap and the
  ~720 ms mechanics half are structural; no re-prioritization needed on warm-up grounds.
- **Only the native kernel tier-warms**, settling at ~58–63 ms (the recorded ~106–115 ms
  value-sort figures are cold first-run). Warmed-vs-warmed, the native kernel is ~3×
  faster than Clojure's ~192 ms value sort.
- Benchmark hygiene: numbers degrade several-fold under background CPU load — check
  system load before trusting a run.

### Cached-cursor merge landed (2026-06-10): reads were ~half the mechanics floor, not most of it

The prelude `merge_sorted` now hoists `a.len()`/`b.len()` out of the loop and caches the
two cursor values across compare and append, so each merge step costs ~1 vector read
(refresh the advanced side) instead of 3 reads + 2 `len` calls. This was T2.1's
"cache the current left/right value" item, landed standalone — it needed no typed buffer.

Measured (userland A/B mirroring old vs new shape, N = 1M, two stable in-process passes):
old shape ~728–739 ms vs cached ~645–647 ms — **~90 ms, ~12% of the mechanics half**;
the key-index path moved within noise (random key reads still dominate it). Verified
post-land in the real prelude on a quiet machine: generic `sort_by(Int.compare)`
~642–654 ms across three in-process runs (`sort_repeat_probe.tw`).

This corrects the earlier attribution that the ~561 ms R floor was "mostly reads."
Removing ~2/3 of the main-loop reads saved only ~90 ms, so merge-context reads cost
~4–5 ns each (most merge levels operate on small sub-vectors that hit the ≤32 tail-only
fast path in `get`), and reads are roughly **half** of the mechanics floor — the random
~16 ns/read regime applies to the key-index reads into the big `keys` vector, not to the
merge's own reads. The other half of the floor is per-element call/branch work: the
closure boundary (~122 ms at call scale), `Order` allocation (~68 ms), recursion, and
branchy append logic. Two implications:

1. **Track 1 (typed storage) is still the master lever for the key-index path** — the
   ~1624 ms of random boxed key reads is untouched by the merge change.
2. **For the mechanics half, comparator mechanics (Track 3) are a relatively larger
   share than the earlier ~21% framing suggested** — with reads cheapened, closure
   boundary + `Order` allocation + recursion dominate what remains (~650 ms).

### Comparator micro-costs are small (enum/`Order` allocation, closure boundary)

`examples/sort-bench/enum_alloc_probe.tw` isolates producing+consuming a comparison result as an enum vs a bare `Int`, at the ~17M-comparison scale, with identical branch structure. Stable results:

| probe | time | delta |
|---|---:|---:|
| int return, direct | ~145 ms | floor |
| `Order` return, direct | ~163 ms | +18 ms |
| custom nullary enum `Cmp3`, direct | ~164 ms | +19 ms |
| int return via closure | ~233 ms | +88 ms vs direct int |
| `Order` return via closure (the `sort_by` shape) | ~301 ms | **+68 ms vs int-via-closure** |
| `Option<Int>` payload via closure | ~334 ms | +101 ms vs int-via-closure |

Findings:

- **Enum allocation is real but modest, and it is enums in general, not `Order`-specific.** Every payload-free variant literal emits a `StructNew` (`codegen/emit/variants.tw`); sum types are a tagged GC struct. The user-defined `Cmp3` tracks `Order` exactly. The cost across the closure boundary (the real sort path) is ~68 ms / 17M ≈ **~9% of the 743 ms generic baseline, ~3% of the 2367 ms key-index path**.
- **The allocation is already elided when the producer is inlined.** Direct calls (`Order`/`Cmp3`) cost only +18 ms because the optimizer fuses construct-then-match. It only bites when the comparator is an opaque closure value — which is exactly `sort_by`. A representation fix (payload-free variants as i32 tag or cached singletons) removes it unconditionally and helps all enum-heavy code, but the payoff for sort specifically is ~9%.
- **The closure boundary itself (~88 ms, ~12%) is a slightly larger lever than the enum alloc**, and is independent of it. Together, closure + enum ≈ 21% of the 743 ms mechanics baseline ≈ **~155 ms ≈ ~6% of the 7× gap**. They cannot get sort into Clojure's league.

### The reframing: Clojure does not cache keys either

The Clojure reference `(sort-by #(nth amounts %) idx)` **re-invokes the key function on every comparison** — it sorts a flattened array with a comparator and does not memoize keys. So Clojure performs the same ~34M persistent-vector `nth` calls and ~n log n comparisons Twinkle does, and still finishes in ~340 ms. Two consequences:

1. The 7× gap is **constant-factor / structural**, not algorithmic cleverness: Clojure sorts a flat `Object[]` **in place** (no per-level allocation, O(1) access) and its `nth` JITs to far less than 16 ns. These map onto the read path and a flat-buffer merge — not onto comparator micro-opts.
2. **Transparent argsort / key-caching recognition is not required to reach Clojure's league**, because Clojure itself does not cache keys. The plan's instinct to keep the comparator generic is sound; it just needs a flat-buffer merge and a fast `Vector<Int>` read, not key recognition.

### Priority order (evidence-based)

1. **Typed flat `Vector<Int>` storage / read path (was Track B).** The master lever, by a wide margin. Fixes the ~1624 ms of random key reads *and* enables a native-buffer merge (cheap sequential reads + no per-level allocation). The realistic path is read-bound (~69%), and even the mechanics floor (~561 ms) is mostly reads + compares.
2. **Flat-buffer / bottom-up merge — folded into (1).** On persistent storage alone it removes only the ~155 ms allocation overhead (~6.5% of the key-index path), so it is not worth pursuing independently. It becomes valuable when it runs over the typed buffer from (1).
3. **Comparator mechanics (was A2 + enum/`Order` alloc).** Real but caps near ~20% of the mechanics half ≈ ~6% of the gap. Do the enum-representation fix opportunistically (broad, low-risk); do not expect parity from it.

The mechanics attribution is now measured (see above): singleton allocation is negligible (~8–14 ms), append/output allocation is ~150 ms, and the ~561 ms remainder is reads + compares + recursion.

## Scope

**In scope:**

- Preserve public APIs and callback semantics for `Vector.sort_by`.
- Improve generic `sort_by` mechanics while still invoking the comparator.
- Improve `Vector[index]` / runtime PVec read performance, especially primitive reads.
- Keep and extend microbenchmarks that isolate closure calls, vector reads, generic sort mechanics, dataframe comparator shape, and side-effecting comparators.
- Stage0 parity for compiler/runtime changes that affect bootstrapping.

**Out of scope:**

- Requiring users to call `argsort`, `sort_by_key`, or dataframe-specific escape hatches for competitive performance.
- Optimizing side-effecting comparators by skipping callback execution.
- Broad comparator-shape recognition as the primary solution. Keep it as an optional later fast path after generic performance improves.
- Fully replacing `Vector<T>` representation for all types in one step. The broader representation work remains in [typed-vector-representation.md](typed-vector-representation.md).

## Track 0: Probes and attribution (shared)

### T0.1 Establish stable probes

**Files:**

- Keep/create: `examples/dataframe/bench/sort_by_costs.tw`
- Keep: `examples/sort-bench/sort_by_component_probe.tw`, `examples/sort-bench/enum_alloc_probe.tw`
- Modify: `docs/plans/wasm-native-sort.md` once results stabilize

- [x] Keep probes for:
  - generic `sort_by(Int.compare)` over shuffled Int row ids;
  - generic `sort_by` with `Cell.update` counter;
  - key-index comparator `Int.compare(keys[a], keys[b])`;
  - closure-call loop without sorting;
  - vector-read loop without sorting;
  - append/building loop without sorting.
- [x] Enum/`Order` allocation isolated (`enum_alloc_probe.tw`): direct vs closure-boundary, nullary vs payload, user enum vs `Order`.
- [ ] Add/keep probes for the full dataframe null-aware comparator shape in the clean component harness.
- [ ] Ensure the probes avoid already-ascending early-return unless that is the thing being measured.
- [ ] Add a Clojure side-effect comparator reference script under `examples/dataframe/bench/` if we want the comparison to be repeatable.

### T0.2 Attribute the ~720 ms generic-mechanics half — done

`examples/sort-bench/merge_attribution_probe.tw` ablates the merge (R = reads+compare+recursion, S = +singletons, B = +append/alloc; B validated against real `sort_by`). Result at N = 1M:

- [x] Singleton allocation priced: **negligible (~8–14 ms)**.
- [x] Append + output-vector allocation priced: **~150 ms (~21% of the mechanics half)**.
- [x] Reads + compares + recursion floor: **~561 ms (~79%)** — reads dominate the mechanics half too.

Conclusion recorded in [Measured decomposition](#measured-decomposition-2026-06-09): allocation is a minor lever; typed flat storage (Track 1) is the master fix.

## Track 1 (lead): Vector indexed read performance

Largest single lever: the realistic key-index path is ~69% reads (~1624 ms of 2367 ms at N = 1M). Formerly framed as long-term/out-of-scope; the measured decomposition makes it the lead track. Clojure's `nth` over the same persistent-vector family does this in a fraction of the time, so there is constant-factor headroom even before typed representation.

### T1.1 Establish vector read baselines

**Files:**

- Keep/create: `examples/dataframe/bench/sort_by_costs.tw`
- Optionally create: `examples/vector-bench/read_path.tw`

- [ ] Measure random reads from `Vector<Int>`, `Vector<Float>`, `Vector<Bool>`, and `Vector<String>`.
- [ ] Separate loop arithmetic from read cost.
- [ ] Measure sequential reads as a control.
- [ ] Compare with Clojure persistent-vector `nth` probes.

### T1.2 Inspect current PVec read path

**Files:**

- `boot/compiler/codegen/runtime/arr.tw`
- `src/runtime/arr.rs`
- `boot/compiler/codegen/emit.tw` / relevant index lowering paths
- `docs/plans/typed-vector-representation.md`

- [ ] Trace `xs[i]` for `Vector<Int>` in WAT: trie traversal, casts, boxed-int extraction, bounds checks.
- [ ] Identify avoidable repeated work: length checks, leaf wrapper casts, generic `anyref` casts, helper calls not inlined, branch structure.
- [ ] Compare boot and stage0 implementations for parity and obvious drift.

### T1.3 Low-risk PVec read-path improvements

Candidate improvements before full typed vector representation:

- Inline hot `get` helper pieces in generated code where practical.
- Reduce redundant casts or bounds checks when index validity is already known in a loop.
- Simplify leaf/node layout checks if current representation forces extra wrapper loads.
- Add a monomorphic `Vector<Int>` read helper that returns raw `i64` internally when the result is consumed as `Int` immediately.

- [ ] Pick one low-risk improvement from T1.2.
- [ ] Implement in boot first if it does not break stage0; mirror in stage0 as required.
- [ ] Gate on random-read probes and dataframe `order_by_breakdown`.

### T1.4 Typed vector representation bridge

If low-risk read-path cleanup is insufficient, continue into the broader representation plan:

- `Vector<Int>` physical storage that avoids boxed `anyref` leaves;
- typed primitive leaf arrays;
- typed builders and typed gather/read operations;
- representation-aware monomorphized codegen.

This is larger than this plan but, given that the realistic path is read-bound, it is the most direct route to Clojure-class numeric/dataframe performance. Tracked in [typed-vector-representation.md](typed-vector-representation.md).

## Track 2: Flat-buffer merge mechanics (fold into Track 1)

**Demoted after T0.2.** On persistent storage alone, removing all per-level allocation saves only the measured **~155 ms** (append + output-vector allocation; singleton allocation is negligible) — ~21% of the mechanics half, ~6.5% of the key-index path. Not worth pursuing as a standalone change. Its value is realized only when the merge runs over the **typed flat buffer from Track 1**: then it gets cheap sequential native reads (pushing below the ~561 ms PVec read floor) *and* zero per-level allocation in the same redesign. So treat the flat-buffer/bottom-up merge as a deliverable *of* Track 1, gated below.

**Files to inspect:**

- `boot/prelude/vector.tw` (`sort_by`, `sort_by_range`, `merge_sorted`)
- PVec builder/append runtime paths in `boot/compiler/codegen/runtime/arr.tw`

### T2.1 Typed-buffer bottom-up merge (after Track 1 lands typed storage)

- [ ] Prototype a bottom-up merge that copies `xs` into a typed flat buffer once, ping-pongs between two buffers, and copies back once — 2 allocations total instead of O(n log n), with native sequential `array.get/set`.
- [ ] Keep the comparator invoked on real elements so every call, side effect, trap, and call order is preserved.
- [x] Cache the current left/right value across compare and append to remove the duplicate read in the current `cmp(a[i], b[j])`-then-append shape. **Landed standalone 2026-06-10** (prelude `merge_sorted`, no typed buffer needed): ~12% off the mechanics half; see [Cached-cursor merge landed](#cached-cursor-merge-landed-2026-06-10-reads-were-half-the-mechanics-floor-not-most-of-it).
- [ ] Gate on generic `sort_by(Int.compare)` and key-index `sort_by`; target below the ~561 ms PVec read floor, not merely the ~155 ms allocation saving.

> Do **not** ship a persistent-vector-only version of this; per T0.2 the allocation-only saving is ~155 ms (~6.5%) and not worth the prelude churn.

### T2.2 Dense buffer only if element access is inlined

Approach C failed because dense scratch access was opaque: per-element runtime calls plus `anyref` casts outweighed persistent-vector merge savings. The lesson is "inline the array ops," not "abandon dense buffers." Safe variants:

- Generate the dense merge loop in the specialized `sort_by` instance so scratch `array.get/set` is inlined (no `scratch_get` calls).
- Keep elements as `anyref` only if access is direct Wasm array ops.
- For primitive `Vector<Int>` / `Vector<Float>` with a callback comparator, consider typed value buffers only if comparator arguments are reboxed exactly as required for callback semantics.

- [ ] Prototype the dense/flat merge with inlined access; reject any shape that reintroduces per-element opaque calls.
- [ ] Gate with `sort_by(Int.compare)` and side-effecting comparator probes.

## Track 3 (lowest priority): Comparator mechanics

Real but small per the measured decomposition: closure boundary (~12%) plus enum/`Order` allocation (~9%) ≈ ~21% of the 743 ms mechanics half ≈ ~6% of the 7× gap. Do the enum-representation fix opportunistically because it is broad and low-risk, but do not expect parity from this track.

### T3.1 Payload-free variant representation (enums in general, not just `Order`) — DONE (singleton globals, 2026-06-10)

Every payload-free variant literal currently emits `StructNew` (`boot/compiler/codegen/emit/variants.tw`); sum types are a tagged GC struct. The cost is only paid across an opaque closure boundary (the `sort_by` shape) — the optimizer already fuses construct-then-match when the producer is inlined. `enum_alloc_probe.tw` confirms a user-defined `Cmp3` behaves identically to `Order`, so any fix must target payload-free variants generally.

**Landed via the module-global-singleton variant** (commit `codegen: hoist payload-free variant literals to shared globals`), not the i32-tag representation change. Rationale: caching one immutable instance per nullary variant is a pure backend change with no blast radius into pattern-match conditions, repr, or `insert_boundaries.tw`, and no stage0 parity work (the logic lives in boot's Twinkle source, which stage0 simply compiles — like the string-builder rewrite). The i32-tag alternative would touch all of those and was not needed.

- [x] Cache nullary variants as immutable module-global singletons (one `StructNew` at init, reused via `global.get`). The planner demand-collects constructed nullary variants (second-pass slot-mono walk in `wasm_plan_scan.tw`, mirroring the closure-mono pass) and registers one global per eligible `(sum, variant)`; emission diverts to `global.get`.
- [x] Eligibility gate: every sibling payload slot must have a *constant* default (`variant_singleton_eligible` in `wasm_layout.tw`). A non-nullable ref default needs `ref.as_non_null`, which is not a constant expression — those sums keep the inline `StructNew`. Covers `Order` (no payloads) and `Option`/enums whose other variants carry primitives or nullable refs.
- [x] Blast radius avoided, not managed: no i32-tag repr change, so pattern-match conditions / default-payload emission / `insert_boundaries.tw` / stage0 are untouched. Demand collection (not eager enumeration) keeps the boot module free of dead globals.
- [x] Gated on `enum_alloc_probe.tw` and `sort_repeat_probe.tw`. Measured at N=1M: generic `sort_by(Int.compare)` ~645 → ~610 ms; key-index ~2260 → ~2200 ms; native `xs.sort()` unchanged (constructs no `Order`). The broader payoff is every `.None`/nullary variant in user *and* compiler code.

Not pursued: the i32-tag representation for payload-free-only enums. The singleton approach captured the allocation win with far less risk; revisit i32-tag only if profiling shows the remaining tagged-struct loads (vs a bare i32) matter.

### T3.2 Closure call representation — DONE (typed funcref for non-tail calls, 2026-06-10)

**What the WAT probe found** (`xs.sort_by(fn(a,b){ Int.compare(a,b) })`): the comparator was called through the **universal erased path** on every comparison — box both `i64` args into `BoxedInt`, `array.new_fixed` a 2-element args array, `call_ref` the universal funcref (anyref params/result), then `ref.cast Variant` + `variant_to_sum_helper` to recover the `Order`. ~3 allocations + a result conversion per comparison, ~17M times.

The concrete closure struct (`$closure_fn_i64_i64_t7`, a subtype of `rt_types__Closure`) already carries a **typed funcref in field 2** taking unboxed `i64 i64 → Order` directly. But only *tail-position* closure calls (`emit_closure_tail_call`) used it; the comparator call isn't in tail position (its result feeds a `case`), so it never hit the typed path.

**Fix (no monomorphization-on-function-value needed):** generalize the typed path to value-producing calls. `emit_closure_call` now tries `try_emit_typed_closure_call` (`boot/compiler/codegen/emit/closures.tw`): `ref.test` for the typed struct and, on the hot path, push env (field 1) + raw args + typed funcref (field 2), `call_ref` the typed functype, use the result directly. The universal erased call stays as the `else` branch for closures lacking the typed struct (builtins). `register_layout_type_def` always declares both the typed func type and the 3-field struct for any `.Closure` layout, so the cast/call validate. This is conservative and preserves every comparator invocation, so side-effecting/observing comparators stay correct — no recognition or comparator-shape analysis required.

- [x] WAT probe + trace: universal path boxes args + allocates args array + converts result per comparison; the typed funcref existed but was tail-call-only.
- [x] Monomorphization specializes `sort_by` by element type only; the comparator stays an opaque closure value. **Not changed** — the typed funcref already gives the unboxed fast path via a runtime `ref.test`, so no specialization-on-comparator-value was needed.
- [x] Typed-funcref fast path for non-tail closure calls, value-returning only (Void/Never keep universal). Preserves call count, order, env, and side effects — verified with a captured-state `Cell.update` comparator.
- [x] Gated on `sort_repeat_probe.tw` and a side-effecting comparator. N=1M: generic `sort_by(Int.compare)` ~610 → ~495 ms (~19%); key-index ~2200 → ~2010 ms; native `xs.sort()` unchanged.

**Cumulative across the three sort-mechanics wins on this branch:** generic `sort_by(Int.compare)` ~743 → ~495 ms (~33%); key-index `sort_by` ~2367 → ~2010 ms (~15%). The remaining key-index gap is now almost entirely the ~1.6 s of random boxed key reads — i.e. the master typed-`Vector<Int>`-storage track, nothing left in comparator mechanics.

Possible follow-on (not pursued): hoist the per-comparison `ref.test` out of the merge loop (the closure value is loop-invariant), but the backend has no LICM and V8 likely folds the always-true test; not worth the complexity now.

## Relationship to transparent argsort recognition

[native-key-index-argsort.md](native-key-index-argsort.md) remains useful as an optional fast path, but should not be the only way to get good `order_by` performance. Comparator-shape recognition is necessarily conservative and must reject side effects like:

```tw
times := Cell.new(0)
foo.sort_by(fn(a, b) {
  times.update(fn(x) { x + 1 })
  Foo.compare(a, b)
})
```

This plan attacks the path that still runs such code. If generic `sort_by` and vector reads become competitive, transparent argsort recognition can be reserved for extra wins on pure key-index shapes instead of being the baseline performance story.

**Why generic is enough:** the Clojure reference itself does **not** cache keys — `(sort-by #(nth amounts %) idx)` re-invokes the key function on every comparison and sorts a flattened array with the resulting comparator. It performs the same ~n log n comparisons and ~34M persistent-vector reads Twinkle does, yet finishes in ~340 ms. So key-caching/argsort recognition is not what closes the gap; a flat-buffer merge (Track 2) and a fast `Vector<Int>` read (Track 1) are. This vindicates keeping the comparator generic.

## Benchmark gate

Primary commands:

```bash
target/twk run examples/dataframe/bench/sort_by_costs.tw
target/twk run examples/dataframe/bench/order_by_breakdown.tw
target/twk run examples/dataframe/bench/main.tw
clojure examples/dataframe/bench/order_by_clojure_persistent.clj
```

Optional comparisons:

```bash
target/twk run examples/sort-bench/value_sort_micro.tw
clojure examples/sort-bench/value_sort_clojure.clj
```

Success is incremental:

- `sort_by(Int.compare)` over shuffled Ints moves materially toward native `xs.sort()` rather than multi-second runtime.
- key-index `sort_by(fn(a,b){ Int.compare(keys[a], keys[b]) })` improves without changing source.
- dataframe `order_by` sort phase drops materially from the current ~2.1–2.2s range.
- side-effecting comparator benchmarks improve while still reporting a valid callback count.
- vector random-read probes improve, and the improvement is visible in dataframe comparator probes.

## Notes and cautions

- Do not optimize by skipping comparator calls unless a separate transparent-recognition fast path proves the comparator is pure and equivalent to a supported key-index sort.
- Preserve stable sort semantics.
- Avoid reintroducing Approach C's failed shape: opaque scratch operations inside the hot loop are not enough.
- Keep measurements in tree and repeatable; avoid relying on `/tmp` scripts for conclusions that guide implementation.
