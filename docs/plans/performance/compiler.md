# Compiler Performance Plan

This document tracks the current performance shape of the self-hosted boot
compiler and the next investigations worth doing. It is the compiler-throughput
side of the performance effort; generated-program runtime performance is tracked
in [compiled-programs.md](compiled-programs.md).

Older measurements from the April compiler are intentionally collapsed into
lessons learned: the compiler, runtime data structures, module graph, and
generated code shape have changed enough that those raw numbers are no longer
useful as baselines.

## How to measure

Build with the bundled CLI and enable compiler timings:

```bash
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/twinkle-boot.wasm
```

For wall-clock checks, run the same build without timing output:

```bash
/usr/bin/time -p target/twk build boot/main.tw -o /tmp/twinkle-boot.wasm
```

Use same-session A/B comparisons for optimization work. Whole-pipeline timings
are noisy enough that a single sample should not justify a change by itself.

## Current baseline: 2026-07-03 (post frontend + link wins)

After the 2026-07-03 session (frontend import/typecheck wins detailed below, plus
the linker `ns_prefix` hoist). Compiling `boot/main.tw` (234 modules) with the
bundled CLI. Representative phase timing (single instrumented run; run-to-run
noise on `lower`/`emit_module` is ±15%):

```text
compile_modules   ~1960ms   (frontend; still the largest bucket)
emit_module        ~465 - 560ms
optimize           ~460ms
prepare_backend    ~358ms
verify             ~330ms
core_link          ~272ms
link               ~227ms
emit_wasm_binary   ~207ms
plan_wasm_types    ~121ms
lower_anf          ~110ms
monomorphize        ~74ms
wasm_dce            ~60ms
closure_convert     ~22ms
```

Frontend sub-timing:

```text
typecheck   ~358ms   (bodies ~252, finalize ~102, setup ~2)
import_merge ~225ms   (module ~62, selective ~126, prelude ~35)
lower       ~215 - 288ms
plan_deps   ~196ms
resolve     ~157ms
load_source ~136ms
parse       ~112ms
publish      ~67ms
env_extend   ~52ms
```

Wall-clock (timing off): **~4.85s**, down from the ~5.06s pre-session baseline.

## Update: 2026-07-09 (post typed-vector family work)

Measured sequentially with the bundled CLI after the typed-vector element-family
work and Bool family follow-ups. Do not run the wall-clock build concurrently
with an instrumented build; doing so inflates the wall number through contention.

Wall-clock checks without timing output:

```text
real 5.06s
real 5.08s
real 5.10s
```

Representative phase timing:

```text
compile_modules   ~1984ms
prepare_backend    ~566ms
emit_module        ~521ms
optimize           ~476ms
verify             ~408ms
core_link          ~279ms
link               ~212ms
emit_wasm_binary   ~209ms
plan_wasm_types    ~125ms
lower_anf          ~115ms
monomorphize        ~75ms
wasm_dce            ~57ms
closure_convert     ~23ms
```

Frontend sub-timing:

```text
typecheck    ~374ms   (bodies ~262, finalize ~107, setup ~2)
lower        ~273ms
import_merge ~242ms   (module ~69, selective ~135, prelude ~36)
plan_deps    ~196ms
resolve      ~164ms
load_source  ~138ms
parse        ~110ms
publish       ~65ms
env_extend    ~53ms
```

Shape check against the 2026-07-03 baseline: the frontend story still mostly
lines up (`compile_modules` remains the dominant bucket, and import/typecheck are
still the important sub-buckets), but the backend tier is heavier now.
`prepare_backend` and `verify` are the clearest drift: both grew after the typed
vector ABI/family work expanded the prepared IR, slot metadata, and verifier
surface. `emit_module` and `optimize` remain in the previous range or close to it.
The current wall-clock is therefore back near the pre-session ~5.06s baseline,
not the post-frontend/link-win ~4.85s low-water mark.

What moved this session (all self-host- and 2960-test-validated), each a
"stop doing unnecessary work / defer until needed" change:

- **import_merge ~485 → ~225ms (~54%)** — lazy origin index + skip identity TypeId
  remaps (below).
- **typecheck ~425 → ~358ms** — Pass 0 `with_functions`-skip (~52→2ms) + finalize
  meta-free zonk skip (below).
- **link ~320 → ~227ms (~29%)** — hoist the O(len²) `ns_prefix` build out of the
  per-instruction rewrite path (below).

Backend phases (`emit_module`, `optimize`, `prepare_backend`, `verify`) are now
the largest remaining tier; they transform IR and are more correctness-sensitive,
so treat them as measure-first rather than obvious wins.

### Linker: hoist `ns_prefix` (landed)

The wasm linker's Phase 4 rewrites every instruction of every function to qualify
local symbols. `qualify(ns, sym)` recomputed `ns_prefix(ns)` on every renamed
`Call` / `GlobalGet` / `RefFunc` / type / artifact — and `ns_prefix` is an
O(len²) char-by-char string build (`out = "${out}${ch}"` per char), so it rebuilt
the same per-module prefix hundreds of thousands of times. Compute it once per
module in each phase loop and thread the prefix through
`qualify` / `rename_func` / `rewrite_instrs`. Identical output, computed once.
`link` ~320 → ~227ms.

### LSP interactive latency: skip unused occurrence index (landed)

Same "defer until needed" pattern on the interactive path. Every editor snapshot
(`workspace_snapshot` / `editor_snapshot`) eagerly built the file's occurrence
index (`build_occurrences_cached`), which walks the whole module AST on a cache
miss — and that miss happens on every keystroke edit, exactly when completion and
signature help fire. But hover, completion, and signature help never read
`snap.occurrences`; only definition, references, document-highlight, rename, and
semantic-tokens do. Added a `with_occurrences` gate + `workspace_snapshot_lite` /
`workspace_snapshot_cached_lite` paths and routed every occurrence-free request
through them: hover, completion, signature-help, document-symbol, folding-range,
inlay-hint, and workspace-symbol. This is an interactive-latency win (not a
batch-build metric), so it's not in the phase table above; validated by the LSP
test suites. The occurrence-consuming handlers (definition, type-definition,
references, prepare-rename, rename, document-highlight, semantic-tokens) keep the
full path.

## Update: 2026-07-11 (prepare_backend: typed-vector analysis scope filter)

`prepare_backend` had grown to the second-largest backend phase (~565–589ms) after
the typed-vector family work, with no sub-timing. Added a `[time:prepare]`
breakdown (kept, alongside `[time:check]` / `[time:imports]`) over its six stages:

```text
insert_boundaries   ~98ms
assign_slots       ~133ms
assign_repr         ~74ms
tailify             ~1ms
analyze_typed_repr ~226ms   (~40% of prepare_backend)
route_typed_vectors ~26ms
```

`analyze_typed_repr` (the joint typed-field/payload/param/return/capture fixpoint
in `backend/typed_param_abi.tw`) was the clear hotspot. Instrumentation showed it
converges in **1 primary round + 0 capture rounds** — cost is per-pass, not
iteration count. Each pass runs `analyze_typed_payloads` + `analyze_typed_params`
+ `analyze_typed_fields` over **all 3339 functions**, and the field scan does
~4 whole-body walks *per element family* per function (`build_copy_map`,
`collect_candidates`, the producer collectors, `scan_consumers`). `analyze_typed_fields`
alone was ~109ms.

### Scope filter (landed)

Every producer, consumer, field-store, param, return, payload, and capture the
analysis can classify is backed by a slot whose **MonoType** is `Vector<Int>` /
`Vector<Bool>` (`elem_family_of` over mono, not repr): `collect_candidates`,
`field_store_sites` (`atom_in_family`), `scan_consumers` (result slot in family),
and the param/return/capture predicates all gate on a family slot. So a function
with **no** family-typed slot contributes nothing to any of the fixpoint dicts.

**Landed**: filter `funcs` to the vector-relevant subset once at the top of
`analyze_typed_repr` (`func_has_family_slot`) and run every internal pass over that
subset. In the boot build only **155 of 3340** functions are vector-relevant
(~4.6%), so the per-function whole-body walks now run over the small subset instead
of the whole program. Output dicts are identical (skipped funcs contribute nothing),
proven by the self-host byte-identical fixed point plus all 2982 boot tests and the
dataframe typed-vector suite (42 tests) green.

Result: **analyze_typed_repr ~226 → ~39ms**; **prepare_backend ~565 → ~403ms**.
Wall-clock moved from the ~5.09–5.35s session-start range to a steadier ~5.01–5.08s
(the ~180ms phase win is a few percent of a noisy whole-build number, but the phase
drop is repeatable). Remaining `prepare_backend` cost is now `assign_slots` (~133ms)
and `insert_boundaries` (~98ms) / `assign_repr` (~74ms) — the genuine per-function
slot/boundary work, measure-first before touching.

## Current baseline: 2026-06-28

Measured on the `scc-module-groups` branch after the SCC frontend landed, using
`target/twk build boot/main.tw`. Two same-session timing runs plus one wall-clock
run with timing disabled.

Representative phase timing (range across the two runs):

```text
compile_modules    ~2002 - 2017ms
emit_module         ~444 - 445ms
optimize            ~423 - 449ms
prepare_backend     ~327 - 328ms
verify              ~303 - 305ms
core_link           ~247 - 250ms
emit_wasm_binary    ~191 - 219ms
link                ~202 - 204ms
plan_wasm_types     ~110 - 111ms
lower_anf           ~104 - 106ms
monomorphize         ~71 - 72ms
wasm_dce             ~53 - 55ms
closure_convert      ~21 - 22ms
```

Wall-clock check without timing output:

```text
real 4.67s
user 7.61s
sys  0.47s
```

Frontend subphase timing currently has an instrumentation gap after the SCC
driver swap: `dep_hashes`, `env_extend`, `import_merge`, and the detailed import
edge counters report zero because the old recursive `analyze_dependencies`
instrumentation no longer owns import-env construction. The non-zero frontend
buckets from the same runs were:

```text
load_source       ~122ms
parse              ~96 - 97ms
plan_deps         ~177 - 189ms
resolve           ~142 - 144ms
typecheck         ~376 - 379ms
publish            ~55 - 60ms
unused_imports     ~17 - 18ms
lower             ~257 - 260ms
```

Interpretation: the backend shape is still close to the June 25 recovered
baseline. The frontend bucket grew after the SCC driver landed, but the missing
import-env sub-timings mean the old “import merge dominates” claim cannot be
revalidated from this run. Restoring import/env/deps-hash timing in the SCC path
is the next observability task before drawing fine-grained frontend conclusions.

Optimizer subphase shape:

```text
funcs=3051  total_rounds=6518  avg_rounds=2.14  at_cap=25

dead_let       ~126 - 137ms
copy_prop      ~117 - 120ms
uniqueness      ~89 - 93ms
defer_elim      ~7 - 8ms
const_fold      ~7 - 9ms
branch_simp     ~7ms
```

Backend planning and verification details:

```text
plan_wasm_types: 125308 slot registration calls, 1070 unique types
verify:          122257 slots; expr_walk ~196 - 201ms dominates slot_checks ~103 - 105ms
```

## Update: 2026-07-02 (post awfy-c5 in-place work)

Measured on `codegen-void-elim` after the awfy-c5 in-place `set_at` work landed
(uniqueness alias-invalidation + method-form `set_at` loop rewrite). Compiling
`boot/main.tw` (234 modules) with the bundled CLI.

```text
compile_modules   ~2209ms   (still dominates: ~44% of wall-clock)
emit_module        ~476ms
optimize           ~448 - 462ms
prepare_backend    ~376ms
verify             ~328ms
core_link          ~276ms
emit_wasm_binary   ~270ms
link               ~214ms
plan_wasm_types    ~121ms
lower_anf          ~115ms
monomorphize        ~73ms
wasm_dce            ~60ms
closure_convert     ~22ms
```

Wall-clock: `real ~5.06s`. Shape is unchanged from the 2026-06-28 baseline —
`compile_modules` still dominates, then `emit_module`/`optimize`. That frontend
bucket remains the only lever worth chasing for compile speed.

**The awfy-c5 in-place work is perf-neutral for self-compilation.** Same-session
A/B on identical input (both compilers building the same `boot/main.tw`):
current `~5.06s` vs the pre-1a compiler `~5.26s` — a ~1–4% edge within noise, not
a real speedup and not a regression. The `set_at`→in-place lever that gave
user programs sieve ~7× / bounce ~9× does not apply here: the compiler's hot
loops accumulate via `Vector.append` (builder) and `Dict`, and it has only ~9
`.set_at` sites total, all in the regexp stdlib — off the compile hot path. The
1a alias-invalidation added per-op optimizer work but `optimize` is unchanged
(the added work is offset/within noise).

## Update: 2026-07-03 (SCC frontend sub-timings restored)

The frontend import/env/deps instrumentation lost in the SCC driver swap is back.
The old recursive analyzer timed import-env construction inside
`analyze_module_impl`; under the SCC driver that work moved into
`build_import_env` and `dependency_hashes` (called from `resolve_singleton` /
`resolve_group`), which were uninstrumented, so `import_merge`, `env_extend`,
`dep_hashes`, and every `[time:imports]` counter reported zero. `build_import_env`
now threads `AnalysisState` back out and accumulates the same buckets the old path
did (env extend, per-kind merge time, edge/export-entry counts, `[time:imports:top]`
attribution); the two `dependency_hashes` call sites are wrapped for `dep_hashes`.
Group (cyclic-SCC) sibling merges inside steps B/D are left untimed — that path is
off the boot compile hot path.

Representative frontend timing (single instrumented run, `boot/main.tw`, 234
modules):

```text
import_merge      ~470ms   (module ~93, selective ~157, prelude ~216)
typecheck         ~434ms
lower             ~218ms
plan_deps         ~205ms
resolve           ~165ms
load_source       ~122ms
parse             ~100ms
publish           ~65ms
env_extend        ~57ms
unused_imports    ~17ms
dep_hashes        ~6ms
```

```text
import edges:       3671   (module 410, selective 753, prelude 2508)
export entries processed while merging: ~157817
```

Interpretation: the old "import merge dominates the frontend" claim is
revalidated — `import_merge` (~470ms) is again the single largest frontend bucket,
now just ahead of `typecheck` (~434ms). Within import merge the cost is cumulative
across many tiny edges (largest individual edge is single-digit microseconds), and
the prelude bucket (2508 edges, ~216ms) is the biggest sub-share because the
prelude surface is imported into nearly every module. Selective imports (~157ms)
still register the full imported interface before binding only selected names.

Best next optimization target: **import merge**, specifically the prelude and
selective sub-buckets. Because no single edge dominates, the lever is a
representation change (cache/remap an imported interface view per `(dependency,
alias, kind/items)` within a session, or shrink prelude re-registration), not a
local edge tweak. `typecheck` is the co-equal runner-up and now has its own
sub-counters (below).

### Typecheck sub-timings (`[time:check]`)

`checker.check` now stamps a per-module `CheckTiming` onto `CheckResult`
(pass-boundary `date.now()` samples, always on — the six samples/module are
negligible). The frontend driver folds these into aggregate buckets on cache
misses only (a hit did no work), printed as `[time:check]`. Representative run:

```text
setup      ~52ms    Pass 0: fresh-meta assignment + env.with_functions rebuild
toplevel   ~1ms     Pass 1 + Pass 3 top-level lets/statements
bodies     ~260ms   Pass 2 (+ conditional Pass 4): function-body inference
finalize   ~111ms   whole-type_map zonk sweep + fn-ret zonk + pub-value zonk

subst_entries      ~5855     total |subst| summed across modules
type_map_entries   ~157120   total entries zonked in the finalize sweep
```

Interpretation:

- **bodies (~260ms, ~61% of typecheck)** is the irreducible core: bidirectional
  inference walking every function body. No cheap structural win here — it scales
  with the amount of code checked.
- **finalize (~114ms → ~101ms, ~26%)** was the clearest lever. It zonks ~157k
  `type_map` entries at end of each module, and `zonk_with_meta` fully
  deconstructs and *rebuilds* every type tree even when nothing resolves. Two
  candidate cheap wins were measured:
  - **Empty-`subst` fast path inside `zonk_with_meta`** (skip the rebuild when the
    substitution has no bindings) — **measured-and-rejected**: perf-neutral for
    self-compilation. `subst_entries` averages ~25/module but finalize cost
    concentrates in the *meta-bearing* modules (non-empty subst), which the fast
    path does not accelerate; it only adds a branch to the hottest recursive
    function for no gain.
  - **Per-entry no-meta guard at the finalize sweep** (skip `zonk` entirely for a
    `type_map` entry that contains no `MetaVar`, since a meta-free type is
    unaffected by any substitution) — **landed**: ~11% off finalize (~114→~101ms).
    Most final `type_map` entries are already-concrete types, so this avoids the
    bulk of the rebuild allocation, and it leaves the inference-path `zonk`
    untouched. Modest in whole-build terms (~0.25%) but correct, localized, and
    risk-free.
  A larger remaining lever is subtree sharing inside `zonk_with_meta` itself
  (reuse an unchanged child instead of reallocating), which would also help the
  meta-bearing entries the finalize guard still fully zonks — deferred as it needs
  a change-tracking return shape, not a one-line guard.
- **setup (~52ms → ~2ms, was ~12%)** was Pass 0 calling `with_functions` per
  module, which rebuilds `func_index` and re-filters `function_bindings` /
  `function_origins` over *every* visible function (thousands, imports included).
  But Pass 0 only mutates function `ret` types, and none of the index / bindings /
  origins depend on `ret` — so the rebuild was pure waste. **Landed**: when
  `function_bindings` is already populated (the common case after resolve), swap
  the ret-updated vector in directly and skip the rebuild; the empty-bindings case
  still routes through `with_functions` so its `bind_all_when_empty` seeding is
  preserved. ~50ms off typecheck (~96% off setup), self-host + all boot tests
  green.

Net effect of the finalize + setup wins: typecheck ~425ms → ~367ms. The remaining
typecheck cost is now clearly **bodies (~260ms)** — the irreducible inference walk —
and **finalize (~103ms)**, whose deeper subtree-sharing lever is noted above.

Recommended next: import-merge representation work (the largest single frontend
bucket), then the subtree-sharing `zonk_with_meta` rewrite if finalize is revisited.

### Import merge: lazy origin index (landed)

`plan_export_type_ids` runs once per import edge (all 3671 of them) and rebuilt
`build_type_origin_index` — a full inverted `origin → TypeId` Dict over the env's
*entire* `type_origins` map — eagerly every time, even though that index is only
consulted when an exported type is **not** already registered by name but carries
an origin. That miss case is the minority: re-merged types (especially the 2508
prelude edges) resolve by name against shared/already-merged state and never touch
the index. Building it eagerly was ~700k+ throwaway string-keyed inserts.

**Landed**: build the index lazily on the first name-lookup miss and reuse it
within the call. Provably identical results (the env is not mutated inside
`plan_export_type_ids`, so a deferred build has the same contents), zero external
changes. Import merge dropped **~485ms → ~330ms (~32%)**, concentrated in the
prelude sub-bucket (**~216ms → ~49ms**) since prelude types resolve by name.

Post-change per-kind shape:

```text
import_merge  ~330ms   (module ~87, selective ~171, prelude ~49)
```

`selective` (~171ms, 753 edges) was then the largest import sub-bucket. It
registers the *full* imported interface before binding only the selected names
(`merge_selective_via_registration`), so `use module.{a, b}` pays whole-interface
registration cost — a probe measured **753 selective edges selecting 1732 items
but registering ~85k entries (~49×)**, dominated by ~30.8k support functions and
~48.8k types.

The obvious follow-up — a per-selected-item support closure — turned out **not**
to be a clean win: the exporter's `support_functions` are, by construction
(`extract_exports_for_module`'s method fixpoint), exactly the method-functions of
support types, all genuinely needed for method resolution on inferred values. So a
selective fast path would have to re-run that fixpoint per edge at import time
(complex, correctness-critical for method resolution, and self-offsetting in cost).
Deferred; would be cleaner as an exporter-side per-visible-export closure.

### Import merge: skip identity TypeId remaps (landed)

The probe instead surfaced a safe, broadly-applicable lever. Because TypeIds are
globally unique across modules, `plan_export_type_ids` almost always maps an
exported type's id **to itself**, yet `remap_function_sig` / `remap_type_def` still
walked and reallocated every signature and type definition to apply those no-op
remaps — on every registered function and type across all three merge kinds.

**Landed** (transparent — every consumer already treats a missing `type_ids` entry
as "keep the original id"): omit identity mappings in `plan_export_type_ids`, and
short-circuit `remap_function_sig` / `remap_type_def` when `type_ids` is empty
(set the name, skip the tree walk). Import merge dropped **~330ms → ~226ms**,
across all kinds: selective ~171→~126ms, module ~87→~62ms, prelude ~49→~35ms.
Cumulative with the lazy origin index, import merge fell **~485ms → ~226ms (~53%)**
over the session.

A fully-synced reverse `origin → TypeId` index field on the env was considered and
set aside: `type_origins` has external write sites (e.g. `inject_group_member_types`),
so keeping a field in sync is correctness-risky in this TypeId-dedup-critical path
for no gain over the lazy build.

## Previous baseline: 2026-06-25

Measured compiling `boot/main.tw` (222 modules / 3029 functions), self-hosted
boot compiler. Two same-session timing runs. These backend numbers were taken
via the `deno` runtime driving the freshly built `target/boot.wasm`
(`BOOT_WASM=target/boot.wasm deno run … tools/js_runtime/deno_main.mjs build
boot/main.tw …`); the internal `[time]` phase numbers are harness-independent and
comparable to earlier snapshots, but wall-clock under this runner (~4.5s) carries
more startup/runtime overhead than the compiled standalone `target/twk` used for
the June 20 wall-clock (~4.1s), so do not compare the two wall-clock figures
directly.

Representative phase timing (range across the two runs):

```text
compile_modules    ~1740 - 1770ms
emit_module         ~448 - 511ms
optimize            ~426 - 459ms
prepare_backend     ~330 - 345ms
verify              ~303 - 308ms
emit_wasm_binary    ~190 - 200ms
core_link           ~252 - 270ms
link                ~208 - 230ms
plan_wasm_types     ~108 - 110ms
lower_anf           ~107 - 122ms
monomorphize         ~70 - 90ms
wasm_dce             ~53 - 66ms
closure_convert      ~20 - 22ms
```

The frontend (`compile_modules`) remains the largest bucket and was not affected
by the backend regression/recovery described below; its subphase shape is
unchanged from June 20 (import merge and typecheck still dominate).

### Deep-IR stack-safety regression (f1a80dd3) and recovery

Between June 20 and 25, `f1a80dd3` ("compiler: improve stack safety for deep IR")
converted many backend/optimizer IR traversals from native recursion to explicit
`Vector`-backed worklists, to stop deeply-nested IR from overflowing the host
stack. The worklists box every child node into a GC `Vector<anyref>` and pay an
`append`/`drop_last` per node — roughly 2-9× slower per node than native call
frames — and several ran unconditionally on every function. This regressed the
post-monomorphize phases sharply while leaving the frontend untouched:

```text
                  June 20    regressed   recovered
optimize           ~350       ~687        ~430
  defer_elim       ~18        ~112        ~7.7
closure_convert    ~22        ~155        ~20
prepare_backend    ~327       ~796        ~330
plan_wasm_types    ~105       ~148        ~110
emit_module        ~398       ~557        ~448
```

The recovery (committed in this branch) replaces those worklists with a single
shape: **walk the deep direction — the linear `Let` spine — iteratively, and
recurse only into control-flow branch bodies (`if`/`match`/`loop`/`defer`), whose
nesting is shallow.** This keeps native-call speed without per-node GC boxing
while staying stack-safe on long `Let` chains. Touched: `opt/defer_elim`,
`opt/use_count`, `backend/closure_convert`, `backend/route_typed_vec`,
`backend/prepare`, `codegen/wasm_plan_scan`, `codegen/insert_boundaries`.

`backend/slot_assign`'s `lower_expr` is the exception: it must build the
`PreparedExpr` and handle deep **else-if chains** (the genuine deep-recursion
case), so it uses a **depth-gated hybrid** — the fast iterative-spine path for
the common shallow case, falling back to the original work-stack walk past a
depth limit (256) so deeply-nested IR still cannot overflow. Each change was
gated on the self-host fixed point (`stage3 == stage4`, byte-identical output).

Durable lesson: a `Vector` worklist over IR nodes is a real per-node tax; prefer
iterating the unbounded *spine* and recursing only the bounded *nesting*, and
reserve an explicit worklist (or a depth-gated fallback) for the one or two walks
where nesting itself is genuinely unbounded.

Residual gap to the June 20 backend numbers is small and lives in the not-yet-
converted worklists (`lower_anf`, `anf_analysis`, `core_linker/dce`,
`codegen/emit`, `codegen/emit/helper_collectors`, `opt/pipeline`,
`backend/verify_expr`), worth ~100ms in aggregate if a follow-up wants them.

Frontend subphase timing (range across the two runs):

```text
import_merge      ~408 - 443ms
typecheck         ~369 - 406ms
lower             ~248 - 257ms
plan_deps         ~189 - 204ms
resolve           ~130 - 144ms
load_source       ~123 - 139ms
parse              ~93 - 106ms
publish            ~56 - 62ms
env_extend         ~17 - 20ms
unused_imports     ~14 - 17ms
dep_hashes          ~6ms
```

Import/interface merging is still the standout frontend cost, but `typecheck`
(~168ms → ~390ms) and `lower` (~143ms → ~250ms) have grown the most in absolute
terms and are now firmly in the same tier. The instrumented frontend buckets
account for nearly all of `compile_modules`.

Deeper import timing shows this is cumulative rather than one pathological edge:

```text
import edges:       3426
module imports:      357, ~77ms total
selective imports:   717, ~180ms total
prelude imports:    2352, ~180ms total
export entries processed while merging: ~144900
```

Selective and prelude imports are now tied as the largest import-merge buckets.
Prelude edges grew the most (1440 → 2352) as the prelude surface widened, so
their cumulative cost has caught up to selective imports despite each prelude
edge being individually tiny. The selective path still registers the full
imported interface first, then binds only selected names, so many `use
module.{...}` edges still pay full-interface registration cost. No single import
edge dominates; the largest observed edges were only a few microseconds, so this
remains cumulative modular overhead rather than an isolated pathological
dependency.

Optimizer subphase shape:

```text
funcs=2939  total_rounds=6271  avg_rounds=2.13  at_cap=24

uniqueness     ~96ms
dead_let       ~90ms
copy_prop      ~90ms
defer_elim     ~18ms
const_fold     ~12ms
branch_simp    ~11ms
```

Backend planning and verification details:

```text
plan_wasm_types: ~120358 slot registration calls, 1022 unique types
verify:          ~117419 slots; expr_walk ~194ms dominates slot_checks ~99ms
```

## What changed since the old plan

The old investigation started from a much slower compiler where associative-list
`Dict`, flat copy-on-write vectors, repeated layout derivation, and temporary
code-section copies dominated large parts of the pipeline. Those specific
bottlenecks have already been addressed or made less central by later compiler
changes.

Important historical lessons that still apply:

- Replacing the linear `Dict` with a persistent HAMT changed the shape of nearly
  every phase by removing O(n) environment and symbol-table lookups.
- Accumulator-style emission helped where code repeatedly built small temporary
  vectors and concatenated them into larger buffers.
- Reusing per-pass facts was often better than structural rewrites:
  - emission reuses layout caches instead of repeatedly deriving record/sum
    layouts;
  - repr assignment caches mono-derived representation, value-type, and layout
    facts;
  - wasm code-section emission caches name-to-index lookups and writes sections
    directly into the final output buffer.
- The most reliable optimization workflow has been: instrument the hot subphase,
  identify repeated derivation or copying, then remove that repeated work with a
  small targeted cache or accumulator change.

## Current interpretation

The bottleneck has moved back to the frontend, but the frontend profile is now
mostly many small reasonable costs across a large module graph rather than one
obvious runaway stage. `compile_modules` is larger than any single backend
phase, yet its main buckets are spread over 222 modules and thousands of import
edges.

The next tier is broad rather than a single obvious hotspot: optimization,
module emission, backend preparation, wasm binary emission, linking, and
verification are all close enough that local sub-timings matter. `emit_wasm_binary`
serializes the ~3.0 MiB compiler payload in roughly 190ms on the normal
Buffer-backed `.wasm` output path, dominated by code section encoding; this is
worth keeping efficient but is not a large enough fraction of the build to be a
primary speed lever.

The current module graph (222 modules / 3029 functions) is much larger than both
the historical 84-module workload and the May 174-module snapshot, so older
absolute timings should not be used for regressions. Treat this snapshot as the
active baseline.

## Plan

### 1. Frontend: `compile_modules`

This remains the largest whole-pipeline bucket, but the latest sub-timing makes
it less likely that there is a simple broad frontend win. The main cost is not
parsing or name resolution; it is cumulative import/interface merging.

Current frontend timing shape:

```text
import_merge      ~408 - 443ms
typecheck         ~369 - 406ms
lower             ~248 - 257ms
plan_deps         ~189 - 204ms
resolve           ~130 - 144ms
load_source       ~123 - 139ms
parse              ~93 - 106ms
publish            ~56 - 62ms
unused_imports     ~14 - 17ms
env_extend         ~17 - 20ms
dep_hashes          ~6ms
```

Interpretation:

- Import merging is the best-understood frontend hotspot, but the measured cost
  is distributed across many small edges. A meaningful improvement probably
  requires a broader interface/environment representation change rather than a
  local tweak.
- Typecheck has grown faster than the module count (~1.7ms/module now, up from
  ~1ms in May) and has joined import merging as a top frontend bucket; it is now
  worth its own subphase instrumentation. Lower, source loading, and dependency
  planning are each still around one millisecond or less per module. Further
  digging may find small fast paths, but outside typecheck they should not be
  expected to produce a large structural speedup.

Possible future probes, if frontend work resumes:

- split selective import registration internally into type registration,
  function registration, value registration, method registration, and final
  binding work;
- prototype a selective-import fast path only if we are willing to compute the
  needed support-entry closure for selected exports;
- consider caching/remapping an imported interface view per `(dependency,
  alias, import kind/items)` within one compilation session;
- add typechecker counters for empty substitution, alias expansion, and zonk;
- measure whether `load_source` is real file I/O cost or source hashing / path
  canonicalization / overlay lookup overhead.

Prefer small repeated-work eliminations over parser or checker rewrites unless
instrumentation proves the structural cost is real.

### 2. Optimizer: `optimize`

The optimizer remains a top-tier phase, but its cost is spread across a few
passes rather than one runaway pass.

Next checks:

- `dead_let`, `copy_prop`, and `uniqueness` should each get direct subphase A/B
  timing before optimization work.
- Look for repeated traversals over the same ANF body that can be fused without
  making pass behavior harder to reason about.
- Check whether use-count, free-variable, purity, or uniqueness facts can be
  shared within one optimization round.
- Investigate the functions hitting the optimization round cap; confirm whether
  they represent real missed simplification or just harmless churn.

Avoid broad optimizer restructuring until a specific repeated traversal or fact
recomputation is identified.

### 3. Code generation and wasm emission

`emit_module`, `prepare_backend`, `emit_wasm_binary`, and `link` are now in a
similar range. Work here should be driven by sub-timings, not by the old
assumption that code-section encoding is always the only target.

Areas to probe:

- `emit_module`: residual layout/type/value-type lookup churn, helper discovery,
  and instruction-vector building in large functions.
- `prepare_backend`: remaining slot/repr assignment scans and repeated
  mono-derived facts not covered by the existing cache.
- `emit_wasm_binary`: code-section body encoding is still the largest wasm
  subphase. The serializer now writes into `@std.buffer.Buffer` and the build
  command writes that buffer directly via `fs.write_buffer`, avoiding the old
  final `Vector<Byte>` materialization on the normal `.wasm` output path. The
  internal `code_section` timing drops substantially and total binary emission
  is now around 190ms in same-session checks. A `TWINKLE_WRITE_BYTES_FALLBACK=1`
  escape hatch keeps the first bootstrap generation working when it was emitted
  by an older compiler that did not export the buffer linear memory. The whole
  binary emission phase is only a modest share of the build, so even a strong
  local win here is useful but not transformative.
- `link`: current timings are higher than the older post-HAMT snapshots; measure
  symbol resolution, map merges, and final module assembly separately.

### 4. Verification and wasm type planning

These are not the first targets, but they are large enough to watch for obvious
repeated work.

Checks:

- `verify` is dominated by expression walking; look for avoidable rewalking of
  unchanged bodies or repeated slot-entry lookups.
- `plan_wasm_types` performs many slot registration calls for a much smaller set
  of unique types; confirm whether repeated registrations are cheap cache hits or
  still doing unnecessary work.

### 5. Runtime data-structure follow-ups

The compiler now runs on the erased persistent `PVec` runtime described in
[../archive/persistent-vector.md](../archive/persistent-vector.md). Keep measuring
vector-heavy compiler paths before changing vector layout.

Potential runtime investigations:

- typed vector families to reduce `anyref` traffic in hot homogeneous vectors;
- RRB-style concat/slice improvements if instruction-buffer concatenation still
  appears in profiles;
- CHAMP-style HAMT layout improvements if dictionary allocation or iteration
  locality shows up again.

These should be justified by compiler profiles rather than implemented as
standalone runtime cleanups.

## Working rules for future updates

- Keep only the current baseline plus durable lessons in this file.
- Move obsolete raw snapshots out of the main narrative instead of appending a
  long timeline.
- Record ranges or representative same-session A/B results, not isolated single
  numbers.
- State what changed, why it matters, and what the next measurement should prove.
