# Phase 6 Implementation Design — Ownership-Specialization Decision Facts

**Status:** Draft design (scoping pass; feeds a later Phase 6 execution plan)

This is the implementation design for **Phase 6** (architecture **1E**) of the
sound-uniqueness track: the **final analysis phase**. It extends the return-path
summary layer (Phase 5) with **parameter-side ownership** — per-parameter
`in_place_paths`, an *owned-entry* re-analysis, and per-call-site **variant
selection** — and produces the ownership-**specialization decision facts**:
per-function preconditions/postconditions and, at each call site, which callers
may use an owned-specialized callee and which stay generic. It closes the one
Phase 5 deferral (param-threaded transport recovery).

It remains **analysis-only**: it *proves and prints* what the specialized
variants must be. **No cloned variants are emitted** — variant *generation* and
call-site routing are codegen (Phase 2A / Phases 7–8). This is the same discipline
as every earlier analysis phase: print facts before rewriting.

Canonical semantics:
[summary-specialization.md](summary-specialization.md) (the summary schema,
`in_place_paths`, `UniqueKey`/`VariantId`, the call-site decision, cap and
determinism), [worked-examples.md](worked-examples.md) (Cases **A**/**B**/**V**
and the **B ∩ C** "one callee, two caller shapes" example), and
[fact-lattice.md](fact-lattice.md) (the transfer function re-run under `Unique`
entry). It implements the Phase 6 bullets in [README.md](README.md). On any genuine
conflict the canonical docs win and this doc is corrected.

## Current state (verified against the branch)

Established before designing, because it fixes what Phase 6 extends and what it
must not disturb:

- **The summary schema is whole-parameter + whole-return + return-paths.**
  `Summary = .{ params: Vector<ParamSummary>, ret: ReturnEffect, ret_paths:
  Vector<ReturnPathOwn> }` (`ownership.tw:53`). `ParamSummary = .{ escape:
  EscapeEffect, capability: ParamCapability }` (`ownership.tw:43`), with `EscapeEffect
  = { Borrowed, Retained }` (`ownership.tw:39`) and `ParamCapability = { NoCap,
  Consumed }` (`ownership.tw:41`). There is **no `in_place_paths`, no per-path
  `Consumed`, and no variant/`UniqueKey` concept** anywhere.
- **`ParamCapability.Consumed` is recorded but never consulted for a caller
  decision.** It is compared by `param_summary_eq` (`summary.tw:93`) and printed by
  `render_summary` (`summary.tw:474`), but `transfer_summarized_call` does **not**
  read it and invalidates no binding (phase5-design.md Decision 6). Phase 6 is the
  first consumer that acts on it.
- **`summarize_function` runs one *generic* pass** (`ownership.tw:3255`): every
  reference parameter enters `Unknown`, yielding the caller-visible/conservative
  summary. There is **no owned-entry pass** — no re-run with a param entering
  `Unique`.
- **The caller gate is whole-value and rejects params** (`transfer_summarized_call`
  `ownership.tw:1112`). It snapshots `arg_unique[k] = own_is_unique(pre_own, id) and
  is_last_use(last, id)` (`ownership.tw:1125`), then recovers an `OwnedFromParam(k)`
  return path only when `arg_unique[k]` (`ownership.tw:1174`); otherwise it
  publishes on failure. **A parameter argument enters `Unknown`, so `arg_unique[k]`
  is false ⇒ publish-on-fail. This single line is the entire Phase 5 deferral.**
- **The SCC summary driver already exists.** `order_sccs` (`summary.tw:294`) emits
  call-graph SCCs callee-first (reverse-topo); `run_scc` (`summary.tw:380`) is a
  monotone worklist fixpoint over one SCC, re-summarizing a member only when an
  in-SCC callee changed; `same_summary` (`summary.tw:207`) is the stop condition and
  already compares `ret_paths` canonical-sorted. **The recursion machinery Phase 6
  needs is in place** — Phase 6 adds a *variant* axis to it, not a new driver.
- **`ret_paths` already carries `OwnedFromParam(k)`.** The return side of the
  consume-produce story exists; Phase 6 supplies the *parameter* side
  (`in_place_paths`) and the *call-site* proof that an argument satisfies it.
- **Builtin COW ops already model per-op consumption.** `CallSemantics.cow_base_arg`
  (`ownership.tw:895`) names the consumed base argument for builtins; user summaries
  do not yet have the analogue. Phase 6's `in_place_paths` is the user-function
  generalization.
- **Census is 0 in-place** (`twk ir --census`), and must stay 0 — Phase 6 changes
  no codegen.

## Scope

**In scope (Phase 6):**

- **Parameter-side `in_place_paths`**: per reference parameter, the set of
  field-paths the callee would mutate **in place if the caller proves them owned**
  (`[]` = the shell / whole collection; `[.f]` = field `f`'s backing). Downward-closed
  under the shell.
- **A `ParamRole` reconciliation** (D9): fold the current `escape × capability` pair
  into the three-way `{ Borrowed, Consumed, Published }`, add `flows_to_return`, and
  populate `in_place_paths` only for a `Consumed` param that flows to the return.
- **An owned-entry re-analysis**: re-run the transfer with keyed `(param, path)`
  slots entering `Unique` instead of `Unknown`, producing the **specialized
  summary** for a `VariantId`. This is what turns candidate `in_place_paths` into
  accepted in-place decisions and turns `OwnedFromParam(k)` into a real unique
  hand-off — and, by feeding a param in as `Unique`, is exactly what closes the
  param-threaded gate **without changing the gate**.
- **`UniqueKey`/`VariantId` identity + the per-call-site decision**: at each user
  call, reduce the `Unique` candidate paths to `selected_key` by the key-level
  `consume_dead` fixed point (D4/D6); select `VariantId{f, selected_key}` (or generic
  if empty / over-cap). Post-call: partially invalidate the consumed paths of `a_k`;
  the result takes the specialized return-path facts.
- **SCC variant fixpoint**: demand-driven variants keyed by caller argument facts;
  within-SCC recursive calls matching an in-progress key tie the knot (Case V's
  self-referential `visit`).
- **Cap + fallback**: a per-`(mono-instance, func)` variant cap with a sound
  persistent fallback.
- **Rendering**: per-function preconditions/postconditions and, per call site, the
  selected variant with its licensing proof, in `twk ir --cfg`.

**Out of scope (deferred):**

- **No variant *generation*.** Phase 6 records the decision (`VariantId` + call-site
  selection + specialized summary); *cloning* the ANF function and *routing* callers
  is codegen (Phase 2A). Nothing this phase emits changes generated code.
- **No in-place emission / decision records consumed by the backend** → Phases 7–8.
- **No specialization keyed on `Borrowed` or `Published` parameters** (D5).
- **No runtime uniqueness test / dynamic dispatch** — the caller selects statically.
- **No deeper parameter requirements in Phase 6:** `UniqueReq.path` accepts only
  `[]` and direct record fields `[f]`. The `Vector<Int>` representation leaves room
  for future field chains, but Phase 6 key building does **not** create a requirement
  for an unsupported deeper mutation. It may only record a direct ancestor when that
  ancestor is itself an independently supported mutation site. Return paths keep the
  Phase 5 depth cap: one record field or one field under a variant payload.
- **No representation commitment (clone vs annotation)** — Phase 6 stays
  representation-neutral (D8).

Guiding rule (unchanged): **soundness before coverage.** A call site takes an owned
variant only with a static proof (`Unique`@path **and** the path-aware consume
condition); any doubt selects the generic persistent callee — always sound.

**Destructive-update theorem.** Phase 6's call-site decision is not proving that a
source variable is merely "used once." It proves the semantic condition needed for
compiler-private mutation: the **pre-update logical version** of the consumed region
has no observable continuation except producing the post-update value. `Unique` names
the single-handle proof; `consume_dead` is the old-version-observability check; a
failed proof selects the generic persistent callee.

Coverage rule (this phase's design bias): **prefer complete
[worked-examples.md](worked-examples.md) coverage over implementation simplicity,
provided no super-linear growth is introduced once the variant cap and lattice
height are treated as constants.** Concretely, the intended bound is: total analysis
work **linear in program size** (Σ function body sizes), and SCC variant iterations
bounded by `lattice_height × cap` — a constant per SCC member. Bounded-constant work
per function/variant is fine; anything that grows super-linearly in program size (or
uncaps the ownership axis) is not. Scope creep in service of the worked-examples goal
is acceptable; if a decision's complete option genuinely balloons the phase, split
the execution plan rather than dropping coverage.

## Worked target — what "decide correctly" means

**Cases B ∩ C (one callee, two caller shapes):**

```tw
fn add_type(env, name, id) { env.types = env.types.set(name, id); env }   // p0 Consumed {[],[.types]}

fn build_env() {            // Case B: fresh env threaded, consume-dead at each call
  env := Env.{ types: Dict.new(), values: Dict.new() }
  env = add_type(env, "A", 1)   // a0 Unique incl .types, consume-dead -> add_type[unique:0,.types]
  env = add_type(env, "B", 2)   //                                       -> add_type[unique:0,.types]
  env
}

fn branch_env(env) {        // Case C: old env kept observable
  before := env
  after  := add_type(env, "A", 1)   // a0 Shared (aliased by before, read later) -> add_type[generic]
  if before.types.has("A") { 0 } else if after.types.has("A") { 1 } else { 2 }
}
```

Phase 6 prints, for `add_type`, `p0=Consumed paths{[],[.types]} p1,p2=Borrowed ->
[]=OwnedFromParam(0)`; and per call site `build_env#… -> add_type[unique:0,.types]`
vs `branch_env#… -> add_type[generic]`, each with the argument facts that licensed
it. **`add_type` materializes exactly two decisions** (one specialized key, one
generic) — no combinatorial blow-up. No code is emitted.

**Mixed-ownership record (path-aware gate, D6):** a caller with a **fresh `.types`
but shared `.values`** still selects `add_type[unique:0,.types]` — a downward-closed
partial key — and a read of `a0.values` *after* the call stays legal, because only
`.types` is consumed. This is the case whole-binding last-use would wrongly forbid.

**Param-threaded transport (closes the Phase 5 deferral, Case W with a param):** the
`case_w_param_fixture` guard in `cfg_return_paths_suite.tw` currently asserts a
param-threaded caller **publishes** its arg. Under Phase 6 the caller that proves an
owned arg selects the callee's **owned variant** (whose param entered `Unique`, so
Phase 5's recovery gate fires inside it); the callee's **generic** variant still
publishes. The guard is updated to assert *per-variant* behavior, **not** relaxed to
"params are implicitly unique."

## Design decisions

Every decision below is settled (forced by soundness, the canonical docs, the
coverage rule, or resolved in review). Where a decision chose between real
alternatives, the rejected option is named with the reason — not left open.

| # | Decision | Choice / rationale |
|---|---|---|
| D1 | Analysis-only; no variants emitted | Phase 6 produces `VariantId` decisions + specialized summaries + per-call-site selection. Cloning/routing is codegen (2A). `twk ir --census` stays **0 in-place** |
| D2 | Reuse the existing SCC driver | Build the variant fixpoint **on** `order_sccs`/`run_scc`/`same_summary`; do not add a parallel driver. Callee-first order + within-SCC worklist are the substrate |
| D3 | `in_place_paths` downward-closed under the shell | A `(k, [.f])` requirement implies `(k, [])`; a `UniqueKey` is downward-closed, so a partially-satisfied key (shell unique, one field shared) yields the shell-reuse-only variant, never an ill-formed key |
| D4 | Licensing is a **key-level** fixed point, not an independent per-path test | `candidate_key = downward_close({ (k,p) ∈ in_place_paths : fact(a_k)@p Unique })`; `selected_key` = the greatest subset of `candidate_key` for which `consume_dead(a_k, selected_key)` holds (drop violating paths, re-close downward, repeat; else generic). `Unique` alone is insufficient — a still-observable consumed region must not be mutated. Full spec: [Key selection and `consume_dead`](#key-selection-and-consume_dead-d4d6) (D6) |
| D5 | No key on `Borrowed`/`Published` params | Read-only reference params and leaked (`Published`) params never enter a `UniqueKey`; a `Published` param carries **empty** `in_place_paths` (a unique value cannot help a callee that leaks it) |
| D6 | **Key-level consume liveness; record shell `[]` ≠ whole binding** *(review)* | `consume_dead` is evaluated over the whole `selected_key` (obligations interact across paths), and `[]` splits by receiver: `[]` on a **record** = the **shell storage** (field-pointer slots) — a consumed shell forbids future whole-record use/publish of `a_k` or any alias and reads of the **updated** fields, but **allows disjoint sibling projections** (`a_k.values` when only `.types` is overwritten); `[]` on a **collection** = the whole backing region (no future use/publish); `[.f]` = field `f`'s region (no future read/publish through `a_k`/aliases). Post-call only the consumed paths are invalidated; a partially-invalid record binding serves only as a carrier for statically-proven disjoint sibling projections. This is what makes `add_type[unique:0,.types]` (consumes shell `[]` + field `[.types]`) coexist with a later `.values` read. Whole-binding last-use is only the collection-`[]` / whole-record-use case |
| D7 | Cap + conservative fallback are mandatory, coverage-sized | A per-`(mono-instance, func)` variant cap; on overflow the call site selects generic (always sound). The cap is the **asymptotic guard** that keeps the ownership axis constant (coverage rule) — **not** a coverage limiter: set it from measured worked-examples demand. **Default cap = 4** ownership variants per `(mono-instance, func)`; every current worked example fits within **2** (add_type / set_at / visit each need one specialized key + the generic), so the cap never binds on them — it is the asymptotic guard, not a coverage limit. Budget composes with type-mono: total = type clones × ownership variants; the cap is on the ownership axis per type-instance. Perf-aware fallback selection is post-codegen tuning. (Uncapped all-combinations was rejected — exponential, violates the coverage ceiling) |
| D8 | Representation-neutral decision record | Record an abstract `VariantId{f, UniqueKey}` + per-call-site selection + a specialized `Summary`; codegen (2A) later chooses clone-vs-annotation. The key + specialized summary is exactly the interface codegen needs, whichever shape it picks — so Phase 6 does not resolve that open question. (Committing to cloned-ANF now was rejected: it bakes a codegen decision into an analysis phase and churns if annotation wins) |
| D9 | Unified `ParamRole` + `flows_to_return`, escape wins precedence *(review)* | `ParamRole = { Borrowed, Consumed, Published }`; `ParamSummary` also carries `flows_to_return: Bool`. **Precedence:** any escaping/retained param maps to `Published` with **empty** `in_place_paths`, *regardless* of a consumed capability (so a leaked value never specializes); `Consumed` only when the param is mutated, not published, and `flows_to_return`; else `Borrowed`. Matches the canonical `base_role / in_place_paths / flows_to_return` schema. **Load-bearing dependency (review #5):** "escaping/retained ⇒ Published" is only safe because a **returned or transport-threaded param classifies `escape=Borrowed`, not `Retained`** — Phase 5 made `Return` non-publishing (`ownership.tw:1942`) and `ARecord` construction uses path-granular `field_store`, not `publish_atom` (`ownership.tw:1429`). If a later change makes `Return` or aggregate construction publish, D9 would silently over-`Published` every transport param and the central worked target (`add_type`/`fresh_meta`/`load_source` rows) collapses. Guarded by a test. (The old two-axis `escape × capability` was rejected: three overlapping fields left `Retained`-but-not-`Published` ambiguous) |
| D10 | **Demand-driven owned-entry re-analysis, exact per key** *(coverage rule)* | `summarize_variant(f, key)` re-runs the transfer with the keyed `(param,path)` slots seeded `Unique`, **only** for keys a call site actually selects; memoized. Each specialized summary is *exactly* sound for its key — no over-claim, no independence check — so interacting-path / mixed-ownership records classify correctly. Cost is sub-polynomial: demanded keys per function are bounded by `min(cap, #call-site-shapes)` (D7), each is one linear re-run, iterations bounded by lattice height. (The cheaper single owned-max pass + subset-intersect was rejected: it over-claims on interacting paths and needs an independence check that itself loses coverage — a constant-factor speedup we don't need) |
| D11 | Param-threaded gate closed by precondition, no gate change | The owned variant is analyzed with keyed params entering `Unique`; the existing Phase 5 recovery gate (`ownership.tw:1174`) fires unchanged inside it. The caller selects that variant only when it proves the owned arg (D4). Sound by construction; preserves the guard-test invariant ("params unique only under a proven precondition, per variant"). (A lighter owned-param fact propagation without variant identity was rejected — a second, weaker specialization path, i.e. the split-brain the architecture forbids) |
| D12 | **Variant memo is an ascending iterated cell with a two-component discipline** *(review #1)* | The `VariantId → Summary` memo stores a **stable cell** dirty-queued until `same_summary` stabilizes, **not** a one-shot transfer. The cell is a whole `Summary` but its two components differ: (a) **in-place capability** is **monotone/ascending** — starts at the generic bottom, a within-SCC recursive call reads the **previous iteration's approximant** (not a blanket generic — that would keep Case V from bootstrapping), growing only when every recursive path supports it; (b) **`ret_paths`** are **non-monotone** and ride the **existing Phase 5 `suppress`** unchanged — read as empty in-SCC (`ownership.tw:1108`), published only at the fixed point. So the ascending argument covers only (a); (b) is order-independent by suppression. Termination is by finite lattice height, **not** an iteration cap (D7's cap is a separate variant-count limit, review #2). (A one-shot memo was rejected: it goes stale under recursion) |
| D13 | Owned-entry is a *re-run of the same transfer* | A specialized summary is the callee re-analyzed with keyed slots entering `Unique` — ownership monomorphization, structurally analogous to type monomorphization. No second, independent proof engine (no split-brain) |
| D14 | Determinism | `UniqueKey` = canonical-sorted `(param, path)` reqs; call sites visited in ANF/source order; `VariantId`s assigned in creation order via a deterministic worklist; within-SCC recursive calls matching an in-progress key **reuse** it (tie the knot) rather than spawning a new variant |

### Data model

**Phase 6 Blocker labels.** The `(Blocker N)` tags below name Phase 6's own
implementation obstacles and are **distinct from the Phase 5 `Blocker 1–5` set**
in [phase5-design.md](phase5-design.md) (same numbers, different meanings — do not
cross-reference): **Blocker 1** = parameter in-place paths are *field-only*
(`[]`/`[.f]` chains), never payload/`Elem`/`Val` segments; **Blocker 2** =
`in_place_paths` are collected from *mutation sites only* (a pure transport param
that never mutates stays empty); **Blocker 3** = *partial path-validity* — a
consumed arg needs a per-path consumed-set that whole-binding `binding_valid`
(cfg.tw) cannot express; **Blocker 5** = the *per-call-site decision* record
(`SpecializationFacts`) plus its module ownership (`ownership.tw` produces each
decision, `summary.tw` assembles the facts, `cfg.tw`/CLI only read them). There is
no Phase 6 Blocker 4.

```tw
// ownership.tw — D9 reconciliation (field names match canonical base_role/…)
pub type ParamRole = { Borrowed, Consumed, Published }

// A parameter in-place path is FIELD-ONLY (canonical: "Parameter in-place paths
// remain field-only AccessPaths"): the shell [] or a direct record field [f] in
// executable Phase 6. The Vector shape reserves future field chains, but Stage-2 key
// building does not create a requirement for unsupported deeper mutations; it may
// only record a direct ancestor for an independently supported mutation site.
// Payload / Elem / Val segments are return-path / read-fact only and are REJECTED here.
pub type ParamPath = Vector<Int>               // [] = shell/whole collection; [f] = direct record field
pub type ParamSummary = .{
  base_role: ParamRole,                        // was escape × capability (canonical name)
  in_place_paths: Vector<ParamPath>,           // NEW: paths the callee MUTATES if the arg is owned
  flows_to_return: Bool,                       // NEW: does the param's region reach the return?
}
// Invariant (D9 precedence): base_role == Published  =>  in_place_paths == []
//                            in_place_paths != []     =>  base_role == Consumed && flows_to_return

// ownership.tw — variant identity (canonical-sorted, downward-closed under the shell)
pub type UniqueReq = .{ param: Int, path: ParamPath }   // field-only path (Blocker 1)
pub type UniqueKey = Vector<UniqueReq>           // [] ⇒ generic variant
pub type VariantId = .{ func: Int, unique: UniqueKey }

// Partial path-validity for consumed args (Blocker 3): whole-binding `binding_valid`
// (cfg.tw) cannot express "a.types consumed but a.values still readable". Track the
// consumed paths per local so the three D6 read rules are checkable.
type ConsumedPaths = Dict<Int, Vector<ParamPath>>   // local -> paths invalidated by an owned call

// Per-call-site decision + whole-program specialization facts (Blocker 5).
pub type CallDecision = .{
  site_func: Int, site_local: Int,             // caller FuncId + the call's ANF result local
  variant: VariantId,                          // unique == [] ⇒ the generic variant
  proof: String,                               // rendered licensing reason
}
pub type SpecializationFacts = .{
  variants: Dict<Int, Summary>,                // variant_key(VariantId) -> specialized (iterated-cell) Summary
  decisions: Dict<Int, CallDecision>,          // site_key(site_func, site_local) -> decision
}
// The specialized Summary is re-derived under Unique entry (D10); memoized as an
// ITERATED cell in the SCC driver (D12); representation-neutral (D8).
//
// Int-key encoding (load-bearing for determinism, acceptance #14). Both Dicts are
// keyed by an Int, so a `VariantId` and a `(site_func, site_local)` pair each
// collapse to a canonical Int:
//   - site_key(site_func, site_local) = a reversible pairing of two FuncId/LocalId
//     ints (both are already dense, non-negative, per-mono-instance stable).
//   - variant_key(VariantId) = hash/serialize the func id + the CANONICAL-SORTED
//     UniqueKey (D14) so equal keys map to one Int and byte-identical across builds;
//     the empty UniqueKey ([] ⇒ generic) is never stored (the generic path reuses
//     today's SummaryTable). The exact codec is an execution-plan detail, but it
//     MUST be a pure function of the sorted key — no allocation-order or visit-order
//     input — or `VariantId` numbering (D14) drifts between builds.
```

Invariants:

1. `in_place_paths` are downward-closed under the shell (D3), **field-only** and
   Phase-6-depth-limited to `[]` / direct `[f]` (D9/Blocker 1); a `Published` param
   has **empty** `in_place_paths` (D5/D9).
2. A `selected_key` is the greatest downward-closed subset of the `Unique` candidate
   paths for which the **key-level** `consume_dead(a_k, selected_key)` holds (D4/D6);
   dropping an unmet `(k, [])` drops every `(k, [.f])` under it, and dropping a
   later-observed path re-runs the downward closure until stable.
3. The in-place capability lattice **ascends**: a variant's specialized summary starts
   at the generic (empty-capability) bottom and only **grows** as the SCC fixpoint
   proves paths (D12). It is not a discover-then-retract fact — within an SCC the
   recursive call reads the **previous iteration's approximant**, and a capability is
   committed at the fixed point (`same_summary` stable). Its `ret_paths` component is
   non-monotone and is suppressed in-SCC (Phase 5 `suppress`), published only at the
   fixed point (D12). Termination is by finite lattice height; the D7 variant-**count**
   cap is separate (over-budget *new* keys route to generic at the call site, not by
   stripping a converging cell).

## Module layout

- **`ownership.tw`** (extend):
  - `ParamSummary` gains `in_place_paths` + `flows_to_return`; `ParamRole` folds
    `escape`/`capability` (D9).
  - New variant types beside `ReturnPathOwn`: `UniqueReq`, `UniqueKey`, `VariantId`.
  - **Requirement collection** (generic pass): `in_place_paths` come **only from
    mutation sites** (Blocker 2) — consuming collection updates on a param path,
    record-update candidates on a param shell/field, and the transitive case (passing
    a param path to a summarized callee that consumes it). An `OwnedFromParam(k)`
    return handoff sets **`flows_to_return`**, it does **not** by itself add an
    `in_place_path` (a pure transport helper that never mutates has empty
    `in_place_paths`). A *requirements* readout, not proof.
  - **Owned-entry re-analysis** (D10): `summarize_variant(f, key)` re-runs
    `forward_block`/`summarize_function` with the keyed `(param, path)` slots seeded
    `Unique` (+ `path_prov` naming param `k`) instead of `Unknown`, yielding the
    specialized `Summary` (accepted `in_place_paths`, `OwnedFromParam` becomes a real
    unique hand-off).
  - **Call-site decision** in `transfer_summarized_call` (or a sibling): form
    `candidate_key` from the pre-call per-path facts, reduce to `selected_key` by the
    key-level `consume_dead` fixed point (D4/D6), select `VariantId` (or generic under
    D7), apply the specialized return-path facts to the result, and invalidate the
    consumed paths of `a_k` (D6). The generic path is exactly today's behavior.
- **`summary.tw`** (extend):
  - `conservative_summary` (`summary.tw:58`) seeds `in_place_paths: []`,
    `flows_to_return: false`, and the reconciled `ParamRole`; `param_summary_eq`
    (`summary.tw:92`) and `same_summary` (`summary.tw:207`) compare the new fields
    (canonical-sorted paths) so the fixpoint terminates.
  - **Variant fixpoint** (D10/D12): the driver returns `SpecializationFacts` (the
    `VariantId → Summary` variant memo + the `CallDecision` table), not just today's
    `SummaryTable`. Variant cells are **iterated** (ascending from the generic bottom):
    a within-SCC recursive call reads the **previous iteration's approximant** for its
    `VariantId`, dirty-queued until `same_summary` stabilizes (terminates by finite
    lattice height); `ret_paths` suppressed in-SCC per Phase 5; a *newly demanded* key
    over the D7 count cap routes that call site to generic (no cell stripping);
    `VariantId`s in creation order (D14).
  - `render_summary` (`summary.tw:467`) prints preconditions/postconditions +
    `in_place_paths`.
- **`field_facts.tw`** — no schema change; `PathKey` reused for encoding, but
  `UniqueReq.path` is the **field-only** `ParamPath` (Blocker 1), **not** the full
  `ff.AccessPath` — payload/`Elem`/`Val` segments are rejected when building a key.
  Still a leaf module.
- **`cfg.tw`** — `render_view` prints the per-call-site variant choice + licensing
  proof by reading the `CallDecision` table passed **as data** (`variant` id + `proof`
  string), preserving cfg.tw's no-`ownership.tw`-dependency rule (like the ownership
  Int tags already are). No structural change.
- **Module boundaries (Blocker 5):** `ownership.tw` produces each `CallDecision` during
  its existing forward pass (it already runs there and holds the per-path facts);
  `summary.tw` (which imports `ownership.tw`) owns the variant memo/fixpoint and
  assembles `SpecializationFacts`; `cfg.tw`/CLI only **read** it for rendering. No new
  import cycle — the existing `cfg ← ownership ← summary` direction is unchanged.
- **CLI** — `twk ir --cfg` renders the specialization story; no new flag.

## Analysis semantics

### Key selection and `consume_dead` (D4/D6)

Selection at a call site is a **fixed point**, not an independent per-path test,
because the consume obligations interact and `[]` denotes different storage on a
record versus a collection:

```
candidate_key = downward_close({ (k, p) ∈ f.in_place_paths : fact(a_k)@p is Unique })
selected_key  = greatest subset of candidate_key with consume_dead(a_k, selected_key) true
                (remove any path whose region is observed later, re-close downward,
                 repeat until stable; if no non-empty key survives ⇒ generic)
```

Removing a path from the key only removes obligations, so the greatest satisfying
subset is unique and reached by dropping violating paths until stable.
`consume_dead(a, key)` is **key-level**:

- **consumed record shell `[]`** (shell reuse / field-slot overwrite): no future
  **whole-record** use or publish of `a` or any alias to that shell, and no future
  read/publish of the **fields this variant updates**; **disjoint sibling
  projections are allowed** — the untouched slots still hold their original
  pointers, so `a.values` after a `.types`-only update reads the correct value.
- **consumed collection `[]`** (whole backing): no future use or publish of that
  collection region through `a` **or any alias**. **Alias-completeness (review #6):**
  unlike the record-shell case — where pre-captured field aliases keep *old* pointers
  and are naturally safe — an in-place `set_at` mutates the shared backing, so a
  pre-captured alias (`w = flags` read after the call) *would* observe the write.
  Rule 1 below is about uses of `a` itself and does **not** cover `w`. Therefore a
  collection-`[]` consume is licensed **only if the alias set for that backing is
  complete** (every live reference to the region is tracked by `prov`/`field_own`); if
  any alias of a collection-`[]` arg is untracked/unknown, **fall to generic** (sound).
- **consumed field path `[.f]`**: no future read or publish of field `f`'s region
  through `a` or aliases (same alias-completeness requirement for the field's
  collection backing).

**The `[]` split is the crux:** `[]` on a record is the record **shell storage**,
**not** the entire logical binding. Treating it as the whole binding would make
`add_type[unique:0,.types]` (which is downward-closed to `{[], [.types]}`) forbid
the later `.values` read and the mixed-ownership target would stay contradictory. A
partially-invalidated record binding may afterwards be used **only** as a carrier
for statically-proven disjoint sibling projections; any whole-value use of it drops
the call to generic.

**Data model for partial validity (Blocker 3).** `cfg.tw`'s `BlockFacts` today has
only whole-binding `binding_valid` (`cfg.tw:37`) and per-path `field_own`
(ownership, not read-validity) — neither can express "`a.types` consumed, `a.values`
still readable." Phase 6 adds a per-local **consumed-path set** to `ForwardState`
(`ConsumedPaths = local → Vector<ParamPath>`), populated post-call for a selected
key's `(k, p)` members. Both the forward `consume_dead` check and the post-call
invalidation read it via three rules on a later use of `a`:

1. **whole-value use** of `a` (pass / store / return / publish, or any read not
   provably a disjoint sibling projection) → **illegal** if `ConsumedPaths[a]` is
   non-empty (drop the call to generic);
2. **read/publish of a consumed path** `p ∈ ConsumedPaths[a]` (through `a` or a known
   alias) → **illegal**;
3. **projection at a path disjoint from every consumed path** → **legal** (the
   `a.values`-after-`.types` case), and yields a normal, fully-valid sub-value.

Aliases are the ones the lattice already tracks (`prov`/`field_own`); an unknown
alias forces rule 1. This set is analysis-only debug/decision state (printed in the
verdict); it is **not** consumed by codegen in Phase 6.

**Alias-completeness gate.** Phase 6 does not add a new points-to analysis. It asks
whether the existing facts are complete enough for the selected region:

- shell/whole ownership must be `Unique` in `own` and have provenance precise enough
  to name a single fresh/param origin, not absent or multi-origin unknown;
- a field path `[f]` must be `Unique` in `field_own` and have matching `path_prov`
  for `[f]` (absence is unknown, never fresh);
- any pre-call alias that remains live must already be represented by the same
  `prov`/`path_prov` origin and checked by `consume_dead`; if an alias may exist but
  is not represented in those facts, the selected key is dropped and the call falls
  back to generic.

For collection `[]` and collection-valued `[f]`, this gate is load-bearing: a
pre-captured alias would observe in-place backing mutation. For record-shell `[]`,
pre-captured aliases only observe old field-pointer slots; they still block
whole-record use/publish and updated-field reads, while disjoint sibling projections
remain legal under D6.

**ConsumedPaths transfer and merge rules.** `ConsumedPaths` is part of the forward
analysis state, not the public CFG facts:

- selecting an owned variant adds the selected consumed paths for the argument local;
- rebinding a local to a fresh post-call result clears that local's old consumed set;
- moving/initing a partially-consumed carrier is legal only when the move itself is a
  permitted disjoint-carrier use; otherwise the earlier call must select generic;
- branch joins and loop back-edges merge consumed-path sets by **union** for each
  carried local, because a path consumed on any predecessor is unsafe to read after
  the join;
- assigning a carried block parameter from a fully-rebound value uses the incoming
  value's consumed set, so normal rebind flow clears old consumed paths when every
  incoming edge supplies the new value.

### Requirement collection (generic pass)

During the existing generic `summarize_function` pass, additionally record, per
reference param, the paths that *would* become in-place ops under `Unique` entry —
**mutation sites only** (Blocker 2): consuming collection updates on the param (or a
param-derived local at a known path), record-update candidates on a param shell/direct
field, and the **transitive** case (passing a param path to a summarized callee whose
summary consumes it). This is a **syntactic/effect** requirements set — it does
**not** assert the generic call is in-place-capable (the generic call is not). It is
the candidate set the owned-entry pass validates. In executable Phase 6, parameter
requirements are restricted to `[]` and direct `[f]`; deeper field chains,
`Elem`/`Val`, and payload segments do not create an `in_place_path` requirement
unless a supported direct ancestor is also independently mutated.

`flows_to_return` is **derived, not independently computed** (nit): it is set iff
param `k` appears in the whole-return classification (`MayAliasParams(k)`) or in a
`ret_path` `OwnedFromParam(k)` — a projection of facts Phase 5 already produces.

Because the transitive case reads callee summaries, requirement collection **also
participates in the SCC fixpoint** (a member's candidate set can grow when an in-SCC
callee's summary gains an `in_place_path`); it ascends alongside the owned-entry
capability under the same previous-approximant schedule (D12).

### Owned-entry re-analysis (`summarize_variant`, D10)

For a **demanded** `key`, re-run the transfer over the callee body with each `(param
k, path p) ∈ key` seeded `Unique` at entry (and `path_prov` naming param `k`),
leaving non-keyed reference params `Unknown`. The re-run reuses the **same** transfer
function (D13): the owned entry is what turns a candidate `record_update[in_place=
false]` / consuming call at path `p` into an accepted in-place decision, and turns
`OwnedFromParam(k)` into a unique hand-off — including recovering a **param-threaded**
transport arg, because the param now enters `Unique` and Phase 5's recovery gate
(`ownership.tw:1174`) fires unchanged (D11). The result is the specialized `Summary`
for `VariantId{f, key}`, stored in its iterated cell (D12).

Because it is demand-driven per key, each specialized summary is exactly sound for
its key with no independence check; the number of re-runs per function is bounded by
the variant cap (D7), keeping the cost sub-polynomial per the coverage rule.

### Call-site decision

At `let L = call f(a0, a1, …)`, form `candidate_key` from the **pre-call** per-path
facts (the snapshot the Phase 5 gate already captures) and reduce it to
`selected_key` by the key-level fixed point of [Key selection and
`consume_dead`](#key-selection-and-consume_dead-d4d6) (D4/D6): the greatest
downward-closed subset of the `Unique` candidate paths for which `consume_dead`
holds. If `selected_key` is non-empty and within the cap (D7), select
`VariantId{f, selected_key}` and its specialized summary; else select generic.
Post-call:

- **`(k, ·) ∈ key`:** invalidate only `a_k`'s **consumed** paths (D6) — sibling
  untouched paths stay readable; the result `L` takes the specialized return-path
  facts (a `[]`-return `OwnedFromParam(k)` makes `L` unique as a whole; a `[.ctx]` /
  `Ok[0].state` return path makes that projected path own the handed-off region until
  projected or published).
- **generic:** `Borrowed` args unchanged, `Consumed` args untouched (persistent
  update does not mutate them), `L` follows the generic return-path facts — exactly
  today's behavior.

### SCC variant fixpoint (D12)

Variants are demanded by call sites and processed in the existing callee-first SCC
order (`order_sccs`). Within an SCC, the variant memo is iterated to a fixpoint. The
cell is a whole `Summary`, but its two components are governed by **different
disciplines** — this split is essential (review #1):

- **In-place capability** (accepted `in_place_paths` / owned-entry facts) is a
  **monotone/ascending** lattice. The cell starts at the **bottom = the generic
  summary** (capability empty); a within-SCC recursive call that matches an in-progress
  `VariantId` **reuses that cell** (ties the knot, D14) and reads the **previous
  iteration's approximant** — not a blanket generic. Round 1 the recursive call sees
  empty capability; a capability is added only when **every** relevant recursive path
  supports it under the current approximant (canonical "Recursion within an SCC"), so
  the ascent is sound and each approximant is a safe under-claim.
- **`ret_paths`** are **non-monotone** (discover-then-retract) and ride the **existing
  Phase 5 `suppress` mechanism unchanged**: during in-SCC iteration a co-member's
  `ret_paths` are read as **empty** (`ownership.tw:1108-1111`), so no speculative
  `ret_path` leaks across a recursive edge, and `ret_paths` are **published only at the
  fixed point**. The ascending argument therefore covers *only* the in-place
  component; the `ret_paths` component is order-independent by suppression, exactly as
  in Phase 5. The variant cell does **not** invent a new discipline for `ret_paths`.

Because in-place capability only grows and no speculative `ret_path` is read across a
recursive edge (suppression governs how a co-member *reads* a callee's `ret_paths`,
not what `same_summary` compares), the stored `ret_paths` converge with the same
termination property as Phase 5's generic fixpoint, and the combined cell reaches a
**least fixed point** with a stable `same_summary`. `visit` resolves here: round 1 its
recursive call assumes no owned hand-off, round 2 it observes the round-1 owned
`.indices/.lowlinks/…` capability and confirms it, and the fixpoint closes with the
owned variant.

**Termination is by finite lattice height, not an iteration cap** (review #2): the
in-place capability lattice is finite (bounded by the callee's candidate paths), so
the Kleene ascent always converges — no round budget is needed and no *converging*
cell is ever discarded. The D7 cap is a separate, **variant-count** limit: if a
**newly demanded** key would exceed the per-`(mono-instance, func)` budget, **that
call site selects generic** (the variant is simply not created). The two never
interact — a cell already ascending to its fixed point is never "stripped."

## Determinism, fixpoint, rendering

- **Determinism (D14):** `UniqueKey` canonical-sorted by `(param, path)` (paths
  compared field-id/segment-wise); call sites visited in ANF/source order;
  `VariantId`s in creation order via a deterministic worklist; equal keys dedup;
  recursive matches reuse. All rendered output sorted.
- **Fixpoint (D12):** `same_summary` compares the reconciled `ParamSummary`
  (`base_role` + `flows_to_return` + sorted `in_place_paths`) so the generic loop
  terminates; the variant memo's in-place component is an **ascending Kleene
  fixpoint** — cells start at the generic bottom, a recursive call reads the previous
  iteration's approximant, the capability only grows, and iteration stops at
  `same_summary`, terminating by **finite lattice height** (no iteration cap). Its
  `ret_paths` component is non-monotone and suppressed in-SCC (Phase 5 `suppress`),
  published at the fixed point. The D7 variant-count cap is separate and never strips a
  converging cell. This is the load-bearing termination argument — lock it with the
  recursive fixture.
- **Rendering:** the summary header gains `in_place_paths` and the reconciled
  `base_role`,
  e.g. `summary add_type: p0=Consumed paths{[],[.types]} p1=Borrowed p2=Borrowed ->
  []=OwnedFromParam(0)`; per call site the selected variant + proof, e.g.
  `call build_env#L10 -> add_type[unique:0,.types] (a0=L4 Unique incl .types,
  consume-dead)` / `call branch_env#L10 -> add_type[generic] (a0=L7 Shared: aliased
  by L8, read later)`. Never a silent bail — every generic-fallback prints its reason.

## Testing

New/extended fixtures in `boot/tests/suites/cfg_summary_suite.tw` and
`cfg_return_paths_suite.tw`, TDD, each checked against `twk ir <fixture>
--opt`/`--cfg` so the tested shape survives optimization. Cross-function by nature.

- **Case A — loop-carried `set_at[unique:0]`** (`worked-examples.md` Case A, sieve):
  a loop body `flags = flags.set_at(k, false)` calling the `set_at` wrapper selects
  `set_at[unique:0]` because `flags` is loop-carried `Unique` + consume-dead across the
  back-edge; the wrapper's summary is `p0=Consumed paths{[]} -> []=OwnedFromParam(0)`.
  (Vector `[]` = whole backing, so the collection-`[]` consume rule applies.)
- **Cases B ∩ C:** `add_type` prints `paths{[],[.types]}`; `build_env` selects
  `add_type[unique:0,.types]` at both calls (one variant, two sites), `branch_env`
  selects generic; each verdict names the licensing argument facts.
- **Case R — `Result`-payload param scrutinee** (`worked-examples.md` Case R; closes
  the README:181/202 gap): a caller whose **param** `state` is threaded through a
  `Result`-returning helper (`case load(state) { .Ok(v) => …v.state…, .Err(e) =>
  return … }`) recovers `v.state` `Unique` **in the caller's owned variant** (its
  `state` param entered `Unique`, so the Phase 5 `Ok[0].state`/`Err[0].state` recovery
  gate fires), while the caller's generic variant publishes. Distinct from Case W in
  that the scrutinee is a variant payload, not a record field.
- **Mixed-ownership record (D6):** `add_type` with `.types` fresh but `.values`
  shared selects `add_type[unique:0,.types]` (partial, downward-closed key), **and a
  read of `a0.values` after the call stays legal** (only `.types` consumed). This is
  the case whole-binding last-use would wrongly reject.
- **Param-threaded transport (closes the Phase 5 deferral):** update the
  `case_w_param_fixture` guard — the owned-variant caller recovers the param-threaded
  state, the generic-variant caller still publishes. Assert **per-variant**, not
  "params are implicitly unique."
- **Consume gate — whole-value use blocks (D6-i):** a `Unique` arg whose **whole
  value** is used after the call (passed/stored/returned) selects generic — the
  path-aware move still forbids whole-value escape.
- **Consume gate — consumed-path read blocks (D6-ii):** a `Unique` arg whose
  **consumed** path (`.types`) is read after the call selects generic; the disjoint
  sibling read alone does not.
- **`Published` param excluded (D5/D9):** a callee that leaks its param carries empty
  `in_place_paths` and `base_role=Published` even if it also mutates the param; no call
  site ever keys on it (precedence test).
- **Transport param stays `Borrowed` (review #5 guard):** a helper that only returns
  its param through a record/`Result` wrapper (`fresh_meta`/`load_source` shape)
  classifies `base_role=Borrowed` (⇒ `Consumed` when also mutated), **not**
  `Published` — pinning the `Return`-non-publish + `field_store` dependency so a
  regression there is caught here.
- **Recursive convergence (Case V, D12):** a self-referential `visit`-shaped helper
  converges with a single specialized variant via the ascending previous-approximant
  schedule; no speculative `ret_path` is observed across the recursive edge (Phase 5
  suppression); termination is by lattice height, confirmed by inspection.
- **Late cross-member demand (review #4):** an SCC where member A's owned variant is
  first demanded only after member B's facts sharpen in a later outer round — the
  late cell starts at bottom and completes its own nested ascent inside the outer
  loop; final decisions match a demand-from-round-1 run.
- **Variant-count cap vs generic (review #2):** with the cap set below demand, a
  newly demanded over-budget key routes **that call site** to generic while an
  already-converging variant at another site is untouched (no cell stripping).
- **Cap + fallback (D7):** past the cap, further call-site shapes select generic
  (sound); the choice is deterministic and source-order.
- **Determinism (D14):** `twk ir --cfg` (summaries + per-call-site decisions)
  byte-identical across two builds; `VariantId` numbering stable.
- **No codegen regression:** `twk ir --census` still **0 in-place**.

## Non-goals

- No variant *generation*, call-site *routing*, or in-place *emission* (Phases 7–8).
- No runtime variant dispatch or uniqueness test — the caller selects statically.
- No unbounded ownership monomorphization — the cap + conservative fallback are
  mandatory (D7).
- No in-place licensing from `Unique` alone — the path-aware consume condition is
  required (D4/D6).
- No specialization keyed on `Borrowed` or `Published` parameters (D5).
- No representation commitment (clone vs annotation) — D8.
- No return/param paths deeper than the Phase 5 depth cap.
- No `Moved` lattice element; no runtime uniqueness flags / refcounts / COW checks.

## Acceptance criteria

All via the boot suite unless noted:

1. **Parameter `in_place_paths` + `flows_to_return`** — `add_type` prints
   `p0=Consumed paths{[],[.types]}` with `.values` **absent** and `flows_to_return`
   set; `set_at`'s vector param prints `paths{[]}`. A pure (non-mutating) transport
   helper prints **empty** `in_place_paths` with `flows_to_return` set (Blocker 2).
2. **Case A — loop-carried decision** — the sieve-shaped loop calling the `set_at`
   wrapper selects `set_at[unique:0]` across the back-edge; a variant that instead
   keeps a prior version observable stays generic.
3. **Cases B ∩ C decision** — `build_env` selects the owned variant at both calls
   (one `VariantId`), `branch_env` selects generic; each verdict names the argument
   facts.
4. **Case R — `Result`-payload param scrutinee** — the param-threaded `Result`
   caller recovers `v.state` `Unique` in its owned variant; the generic variant
   publishes (closes the README:181/202 Case-R gap).
5. **Mixed-ownership record (D6)** — a partially-owned record selects the
   downward-closed partial key (shell + owned field), and a post-call read of the
   shared sibling path is accepted.
6. **Param-threaded transport closed** — the owned-variant caller recovers the
   param-threaded state; the generic variant still publishes; the guard asserts
   per-variant behavior.
7. **Path-aware consume gate** — whole-value use *or* a consumed-path read after the
   call forces generic; a disjoint sibling read alone does not (D4/D6).
8. **Collection-`[]` alias completeness (review #6)** — a pre-captured alias
   (`w = flags`) read after an in-place `set_at` forces generic unless the backing's
   alias set is provably complete.
9. **Transport param stays `Borrowed` (review #5)** — a return-only/transport helper
   classifies `Borrowed`/`Consumed`, never `Published` (pins the Return-non-publish
   dependency).
10. **`Published` precedence (D9)** — a param that is both mutated and leaked is
    `Published` with empty `in_place_paths`; no call site keys on it.
11. **Recursive convergence** — a recursive transport/`visit` helper converges to a
    single specialized variant via ascending approximants (iterated cell), no
    speculative `ret_path` observed across the recursive edge; termination by lattice
    height (D12).
12. **Late cross-member demand** — a variant first demanded in a later outer SCC round
    completes its nested ascent; decisions match a demand-from-round-1 run (review #4).
13. **Variant-count cap + fallback** — a newly demanded over-budget key routes that
    call site to generic (no cell stripping); the choice is sound and deterministic.
14. **Determinism** — `twk ir --cfg` byte-identical across two builds; stable
    `VariantId` numbering.
15. **No regression to in-place** — `twk ir --census` still **0 in-place** (Phase 6
    changes no codegen).
16. **Full verification** — `make boot-test` green and `make stage2` reaches the
    self-host fixed point (Phase 6 is boot-only and adds no stage0-parity construct),
    including any re-baselined `cfg_summary_suite` expectations for the `ParamRole`
    reconciliation.

## Deferrals and tracking

| Item | Home |
|---|---|
| Variant *generation* + call-site routing (clone vs annotation resolved) | Phase 2A ([../codegen/README.md](../codegen/README.md)) |
| In-place emission from the decisions | Phases 7–8 ([../codegen/README.md](../codegen/README.md)) |
| **Cross-phase obligation on shell-reuse codegen (review #3):** the D6 disjoint-sibling read license (`a.values` legal after an in-place `.types` update) is sound **only if** in-place shell reuse overwrites *exactly* the updated field slots and leaves sibling slots holding their original pointers. Phase 6 licenses reads on this future behavior; a later shell-reuse implementation **must not** relocate/clear other slots. | Phase 2A/7–8 codegen constraint ([../codegen/README.md](../codegen/README.md)) |
| Perf-aware cap/fallback selection (hotness) | Post-codegen tuning (README "Analysis deferrals") |
| Return/param paths deeper than the Phase 5 depth cap | Post-codegen follow-up (sound under-claim without it) |
| Non-escaping closure recovery; concurrency copy/share refinement | Post-codegen follow-ups (conservative-by-default) |

## Determinism-sensitive spots (lock with tests)

- **`UniqueKey` canonicalization** — sorted `(param, path)`, downward-closed; equal
  keys dedup; round-trip stable across builds. `variant_key(VariantId)` and
  `site_key(site_func, site_local)` (the `SpecializationFacts` Dict keys) must each
  be a **pure function of their canonical inputs** — the sorted `UniqueKey` + func
  id, and the two ids respectively — with no allocation/visit-order input, so the
  memo and the rendered `VariantId` numbering are byte-identical across builds.
- **`VariantId` numbering** — creation-order via a deterministic worklist; recursive
  matches reuse the in-progress id (no numbering drift under reordering).
- **Ascending variant fixpoint** — a within-SCC recursive read returns the **previous
  iteration's approximant** of that `VariantId`'s cell (bottom = generic), and the
  in-place capability only grows; the cell is dirty-queued and re-run until
  `same_summary` stabilizes (D12); its non-monotone `ret_paths` half is suppressed
  in-SCC (Phase 5 `suppress`) and published at the fixed point. Order-independence
  follows from round-robin-to-fixed-point over sorted members (Kleene ascent reaches
  the same least fixed point regardless of visit order). The D7 variant-count cap
  routes over-budget *new* keys to generic at the call site; it never strips a
  converging cell.
- **Owned-entry pass** — a pure function of the callee body + the seed key (D10);
  off the generic fixpoint's monotone path; memoized per `VariantId` so a given key is
  re-derived only when its dependencies change, order-independently.
