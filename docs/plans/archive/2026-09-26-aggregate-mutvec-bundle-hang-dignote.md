# Dig note — aggregate-MutVec bundle hang at "Build bridge module via stage1"

## RESOLVED (2026-09-26)

Root cause was NOT any of the prioritized suspects (build_ctx widening, a
superlinear pass, a runtime miscompile). It was a **non-terminating compile-time
fixpoint** introduced by commit `eec5b40c` ("track vector field lineage through
record reconstruction"), tripping in `variant_specialize`'s `summary_table`
phase.

- `collect_field_flow_with_live` (`boot/compiler/ownership.tw`) runs an
  **uncapped** `for changed { … }` dataflow fixpoint over the CFG. Pre-`eec5b40c`
  its lattice was `FlowFact = { origin, dirty }`, a proper monotone meet
  (`origin` descends to −1, `dirty` unions toward a finite ceiling), so the
  uncapped loop always converged.
- `eec5b40c` enriched `FlowFact` with `fields`/`fields_known`/`source_field`/
  `exact` and made `join_fact` compute
  `fields_known = a.fields_known and b.fields_known and a.fields == b.fields`.
  The `a.fields == b.fields` equality is **not a monotone meet**: across a loop
  back-edge the joined fact oscillates, so the uncapped loop spins forever.
- Localized with an iteration cap that `error()`s: it fired on function `export`
  (gen_bridge), **a single self-looping block** — 73 rounds where a monotone
  single block converges in ≤3. Method that worked: `./target/release/twk build
  boot/main.tw -o target/boot-stage1.wasm` then `BOOT_WASM=… deno run …
  deno_main.mjs build boot/tests/gen_bridge_wasm.tw` reproduces the hang natively
  in seconds on a fast machine (the sandbox's >280s figure was sandbox-specific).

**Fix:** bound the flow fixpoint and, on non-convergence, return a conservative
`FieldFlowResult` (`conservative_field_flow` — every return path `ff_none`, no
field-lineage ownership certified), so the offending function falls back to the
generic path. Mirrors the SCC summary fixpoint's existing cap+fallback safety
net. Sound (conservative) and terminating; converging functions are unaffected.

Deeper follow-up (not done, low priority): make the field-flow lattice properly
monotone so the optimization applies even to these functions. The subtlety is
that a reconstructed field's origin can differ from its own field id (field
permutation), so the exact-map guard cannot simply be dropped — it needs an
explicit-ambiguous-entry normalization.

Verified: gen_bridge compiles + runs; 3736/3736 boot tests pass; `make
bundle-cli` completes in ~1m35s with self-host fixed point (stage3 == stage4).

---

Branch `recursive-mutvec-abi`. The aggregate-MutVec work (session 2) made
`make bundle-cli` hang at the `deno run … run boot/tests/gen_bridge_wasm.tw`
step (stage1 = boot compiled by rust stage0, run under deno). Clean HEAD bundle
completes in ~1–2 min. All 3736 boot tests pass; the hang only shows in the
self-host (stage1-under-deno) path, which is ~30–60× slower per op than native,
so it amplifies any superlinear per-compile work the boot tests don't feel.

## Method gotchas (cost me hours — read first)
- `timeout N CMD | tail; echo $?` reports **tail's** exit (0), MASKING a
  timeout (124). Capture exit WITHOUT a pipe: `CMD > f 2>&1; echo $?`.
- `target/twk build/run/wat <x>` uses the PREBUILT binary (no session-2 code).
  `target/twk run boot/main.tw -- <cmd>` interprets current source (fast for
  small inputs; **>200s for boot/gen_bridge** — interp overhead, not a bug).
- eprintln BUFFERS; output is LOST when the process is killed on timeout — do
  not rely on eprintln to localize a hang. Use `TWINKLE_TIMINGS=1` (per-phase),
  a file-marker with flush, or an iteration cap that `error()`s.
- This sandbox CANNOT run the bundle at all: clean-HEAD gen_bridge-via-stage1 is
  >280s here. Localize on a machine where clean HEAD = 1–2 min.

## Already fixed this session (rebuild stage1 to include them; rule these out)
1. `detect_aggregate_regions` was O(routes × functions × body): `find_func`
   per-route + `find_entry` per-candidate each scanned every function. It was a
   one-off in `--census` but session 2 put it in the hot compile path
   (`run_aggregate` runs it EVERY compile). → precompute `DetectCtx` (funcs +
   call-site→entry maps) in ONE pass; lookups are O(1)/O(route_sites).
2. `build_detect_ctx` first used `collect_ops(f.body)` per function, and
   `collect_ops` is O(body²) (repeated `.concat`) → O(functions × body²). →
   rewrote as a direct O(body) walk (`index_call_sites`).
3. `trace_builder` (mutvec_region) recursed `AInit` alias chains with NO cycle
   guard → infinite recursion on a cyclic chain. → added `if aliases.has(cur)
   return .None`.
4. `flatten_helper` (mutvec_propagate) memoized AFTER recursing → self/mutual
   recursive helper = infinite recursion. → in-progress `-1` sentinel; cycle →
   bail (persistent).
5. `run_aggregate` rebuilt the whole functions vector per accepted region
   (O(R×F)) → batched into ONE final rebuild.

## If STILL hanging — localize
### Step 1: compile vs execute
The step RUNS gen_bridge (compile + execute). Test compile alone:
```
BOOT_WASM=target/boot-stage1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/tests/gen_bridge_wasm.tw -o /tmp/gb.wasm \
  > /tmp/gb.log 2>&1 ; echo "exit=$?"
```
- exit=0 fast, but `run` hangs → generated wasm loops at RUNTIME = **miscompile** (§3).
- hangs → compiler hangs COMPILING (§2).

### Step 2: which phase (compile hang)
```
TWINKLE_TIMINGS=1 BOOT_WASM=… deno run … build boot/tests/gen_bridge_wasm.tw -o /tmp/gb.wasm \
  > /tmp/gb.log 2>&1 ; grep -a '\[time' /tmp/gb.log
```
Last `[time:phase]` before the hang = culprit phase (run_mutvec_call,
variant_specialize, produce_mutable, prepare_backend, closure_conversion, …).

### Step 3: bisect session-2 changes (detect fix already in)
Each: edit source → rebuild stage1 natively (`./target/release/twk build
boot/main.tw -o target/boot-stage1.wasm`, ~30s) → re-run Step 1. Toggle ONE:
- (a) `run_aggregate` → early-return the base decision (no aggregate work).
- (b) `build_ctx` widening → restore `producer_source` (drop `collect_freeze_temp`)
      in `mutvec_region` CollectSeed.
- (c) `mutvec_repr.upgrade_func` → `git checkout` that file.
Whichever revert restores 1–2 min = culprit.

## Remaining suspects (prioritized)
1. **`build_ctx` widening** (`mutvec_region.collect_freeze_temp`) — the ONLY
   change touching the BARE S2 path on EVERY module. It now flattens bare
   regions previously detected-but-bailed (freeze-result-direct handle). Compile
   hang via `trace_builder` is now guarded (§fix 3); a RUNTIME loop from a
   mis-flattened bare region is still possible (§1 execute-phase symptom).
   **SAFEST MITIGATION:** revert this shared change and confine freeze-direct
   handling to the aggregate caller path only (`mutvec_aggregate_phase
   .rewrite_caller`: normalize the freeze-direct handle to an `init`-of-freeze
   alias before `rewrite_regions_in_func`, or thaw it inline), leaving the bare
   S2 path byte-identical to HEAD.
2. **Another superlinear pass** — re-audit `run_aggregate`, `build_aggregate`,
   `mutvec_propagate.flatten_helper`, and `mutvec_repr.upgrade_func` for any
   per-function/per-route full-module scan or O(body²) op collection (the two
   already found were `detect`'s scans and `collect_ops`).
3. **Runtime miscompile** of a decomposed `$mvagg`/flat sibling or a widened
   bare region (only if §1 shows the execute phase hangs). Compare `twk wat` of
   the suspect function against HEAD.

## Fast pre-bundle confirmation the perf regression is gone
On a native binary of the new code: `time target/twk ir boot/main.tw --census`
(runs `detect` once). Should be ~HEAD's ~4s, not slower. If detect is fast but
the bundle still hangs, the cause is NOT detect (look at build_ctx / a runtime
miscompile).

## Note on scope
boot & gen_bridge have ~0 ACCEPTED aggregate regions (census; carriers there
mostly have 0 or >1 collection fields), so `build_aggregate`/`flatten_helper`
barely run on them — the regression is in code that runs regardless of
acceptance (`detect`, and the shared `build_ctx`). Keep any new per-compile
analysis strictly linear in module size.
