# Phase 0 — Baseline and Safety Rails

**Status:** Draft subplan (design)

The Phase 0 preconditions from the [README](README.md): the two safety rails that
gate every later phase. Both are **latent today** — with the previous optimizer
passes removed on this branch nothing lowers to in-place, so the guard tests all
pass on the persistent path and the census reads an all-COW floor. Their value is
the signal they carry once Phase 5 codegen starts converting sites: a guard test
flips red on an unsound in-place rewrite, and the census in-place column climbs
from zero. Building them now lays the tracks before the trains run.

Neither rail contains any ownership analysis. They read what ANF already carries
(the `ARecordUpdate` `in_place` slot, candidate op FuncIds); ownership verdicts and
rejection reasons are Phase 1+, explicitly out of scope here.

## Component 1 — Negative-aliasing guard suite

**File:** `boot/tests/suites/uniqueness_guard_suite.tw`, wired into
`boot/tests/main.tw`.

Each test runs one aliasing/publication pattern and asserts the **observable
persistent result** — the value that would be corrupted if a future in-place
rewrite mutated a still-live alias. Green today (everything persistent); a test
flips red exactly when a Phase 5+ in-place lowering is unsound for that pattern.

Each test is **named for and commented with the worked-example case and the
fact-lattice rule it guards**, so the suite doubles as an executable index of the
soundness model ([worked-examples.md](worked-examples.md),
[fact-lattice.md](fact-lattice.md)).

Coverage (one test per required negative in [sound-analysis.md](sound-analysis.md)):

- `case_c_alias_old_version_observable` — Case C, the `AInit` alias hinge
- `slice_concat_view_sharing` — shared backing via `slice`/`concat`/`View`
- `stored_in_record_before_update` / `stored_in_variant_before_update` /
  `stored_in_dict_before_update` — one guard per aggregate storage kind
- `case_cell_publish_and_get_unknown` — Case Cell (`cell$set` publishes,
  `cell$get` yields `Unknown`)
- `module_global_publishes` — value published into a module-global `Cell`
  (Twinkle's only mutable global), the module/global publication sink
- `closure_capture_publishes`
- `task_capture_publishes` / `channel_send_publishes` (Task and Channel both exist
  on this branch, so concurrency sinks are expressible now — nothing stubbed)
- `unknown_call_boundary_publishes` — the callee **retains** the value (a read-only
  callee returning a scalar would be a vacuous guard)
- `nested_collection_inner_shared` — `Vector<Vector<T>>`, `Dict<K, Vector<V>>`
- `case_t_try_early_return_publishes` — Case T

Plus two **positive anchors** as contrast partners, so later diffing has an owned
baseline and not only negatives:

- `case_b_owned_threading` — owned record + dict threaded through `add_type`
- `case_v_record_quartet` — the `visit` shell/field quartet

## Component 2 — `twk ir --census` flag

The counting logic lives in a **reusable boot function** — new
`boot/compiler/census.tw`, taking a lowered `AnfModule` plus the optimizer
semantics (built from the `BuiltinRegistry`) and returning a tally struct — so the
flag and the gate test call the same code, not two parallel implementations.

- Registered in `boot/main.tw` next to `--anf`; handled in
  `boot/commands/ir.tw` via `pipeline.compile_entry_path` → `artifacts.opt` (with
  `artifacts.builtins` supplying the registry).
- Counts over the **codegen-bound (optimized) ANF** (`artifacts.opt`), so the
  in-place column reflects real decisions. With the optimizer stripped on this
  branch it equals the all-COW floor (0 in-place).
- **Identifies candidates via the surviving `OptimizerSemantics`
  (`boot/compiler/opt/semantics.tw`), not hardcoded FuncIds** — matching
  architecture Phase-1A's "model operation effects through optimizer semantics
  rather than hardcoded source names." A candidate is an `ACall` whose
  `CallSemantics.effect` is `.Update` (dict.set/remove, vector append/index-set,
  …) plus every `ARecordUpdate`; the builder families (`.Allocate`) are tallied the
  same way. The `ReusableUpdateKind { Call, RecordUpdate }` and
  `CowConfig`/`CowOpEntry{base_arg, in_place_id}` types already model this
  candidate/in-place split.
- **In-place shows up in two forms, both counted:** the `ARecordUpdate.in_place`
  bit set, and an update call rewritten to its `CowOpEntry.in_place_id` variant.
  Both are zero on this branch.
- **Default:** a population table (total candidates + in-place count per family),
  matching the stage0 census table in [worked-examples.md](worked-examples.md).
- **`--sites` modifier:** adds a per-site listing (function + op) for debugging
  *which* sites did or did not convert once Phase 5 lands.
- Deterministic: a pure structural walk in ANF/source order (no hash-map
  iteration). The count inherits the optimized ANF's determinism, which the plan
  already mandates for the optimizer — the census adds no nondeterminism of its own.

## Component 3 — Census gate and wide reference

- **Asserted gate (tight):** `boot/tests/suites/uniqueness_census_suite.tw`
  compiles a small fixture corpus in-process via `pipeline.compile_source` and
  asserts the exact per-family counts through the Component 2 census function.
  Corpus = **inline worked-example snippets** (the record / dict / vector shapes
  and Cases B/V), each a self-contained source string in the suite — filesystem-
  independent and hand-verifiable. Stable — each count traces to a documented
  case, and it will not false-alarm on unrelated boot-source growth.
- **Wide reference (loose):** `twk ir boot/main.tw --census` (and, run by hand, the
  AWFY programs `sieve` / `bounce` / `nbody`) as **documented manual commands**,
  not CI assertions — the "how much of the real compiler is covered" signal. This
  mirrors how the stage0 `tests/cow_analysis.rs` census is an `--ignored` reference
  distribution rather than an exact gate, and it absorbs the absolute-count drift
  that tracking `boot/main.tw` as a hard gate would cause.

## Verified enablers (main technical risk retired)

The gate test needs to lower fixture source → optimized ANF in-process. Confirmed
this is a proven, exposed pattern on this branch:

- `pipeline.compile_source(src)` runs the full pipeline (including
  `optimize_module` at `pipeline.tw:148`) and returns `PipelineArtifacts` with
  `.anf` (unoptimized), `.opt` (optimized, codegen-bound — the same field codegen
  consumes), `.env`, and `.builtins`.
- `boot/tests/suites/anf_lower_suite.tw` already compiles source strings to
  `AnfModule` inside a boot suite (parse → resolve → check → lower_core →
  monomorphize → lower_anf), so the idiom is established.
- The `AnfOp.ARecordUpdate(Atom, FieldId, Atom, Bool, TypeId)` node carries the
  `in_place` bit (the `Bool`) the census reads (`boot/compiler/anf.tw`).
- The `OptimizerSemantics` layer (`opt/semantics.tw`) survived the pass removal, so
  candidate identification rides `EffectKind`/`CowConfig` rather than numeric
  FuncIds.
- `task_suite.tw` and `channel_suite.tw` already run Task/Channel in the boot
  harness, so the `task_capture` / `channel_send` guard tests are runnable there —
  no concurrency case needs stubbing.

So Component 3's asserted gate compiles fixtures in-process; no Rust harness
fallback is required.

## Relationship to the stage0 census

This boot-side census is **distinct** from `tests/cow_analysis.rs`
([worked-examples.md](worked-examples.md) "Census baseline"):

- the stage0 census measures **stage0** compiling `boot/main.tw`, is
  non-deterministic (optimizer jitter), and is a loose reference distribution;
- this Phase 0 census measures the **boot pipeline's** optimized ANF, is
  deterministic (a static op count), and provides the asserted fixture gate.

They answer different questions and both stay. The boot-side census is the
regression gate this project judges itself against.

## Non-goals

- No ownership verdicts, rejection reasons, or CFG/ownership-fact printing (Phase 1
  — [cfg-ownership-ir.md](cfg-ownership-ir.md)).
- No codegen changes: the census reads existing ANF; the guard suite asserts
  existing persistent behavior.
- No golden ANF snapshots — the worked-examples doc already notes FnIDs drift, so
  snapshots would churn without adding soundness signal.

## Resolved during implementation

- The asserted gate pins the exact per-family counts the current lowering produces
  for each inline fixture (record/dict/vector shapes + Cases B/V); those counts
  live in `uniqueness_census_suite.tw` and are reconciled against
  `twk ir <fixture> --anf` whenever the lowering shifts them.
- The wide `boot/main.tw` census stays **informational** (no CI assertion) — the
  inline gate carries the deterministic regression signal. (The current wide read
  is all-COW: candidates across every family, zero in-place.)
