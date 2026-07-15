# Ownership Fact Lattice and Transfer Rules

**Status:** Draft subplan

This is the semantic core of the sound-uniqueness analysis: the ownership fact
lattice, the per-op transfer function, the control-flow merge rules, and the
function-summary/specialization model. It is designed against the real ANF shapes
in [worked-examples.md](worked-examples.md) and realizes the analysis the
[architecture.md](../architecture.md) umbrella and [cfg-ownership-ir.md](cfg-ownership-ir.md)
view describe.

Guiding rule throughout: **soundness before coverage.** Every default is the
conservative one (not owned), and any operation whose effect the analysis cannot
prove drops the fact to the persistent path.

## What the lattice tracks

For each ANF local at each program point, the first question is: *is the value
bound to this local uniquely owned here, such that an existing in-place or builder
lowering can consume it without changing observable semantics?* Ownership is a
property of a **value**, which may be referenced by more than one local (Case C:
`L7` and `L8` name the same value), so the model is affine: **a value has at most
one live owner; creating a second live reference demotes it to shared.**

## First implementation domain

The executable first cut should use a deliberately small ownership domain:

```
Unknown  (⊤ — conservative, no optimization)
  │
Shared   (known observable through another reference/publication)
  │
Unique   (static uniqueness proof; existing in-place/builder hooks may consume)
```

- **`Unknown`** — no proof. Default for parameters without a summary, results of
  unknown calls, values read from globals, or any case the analysis has not yet
  modeled. Consumers must use the persistent path.
- **`Shared`** — the value is known observable through another live reference or
  publication boundary. Persistent-only.
- **`Unique`** — the value has a static uniqueness proof at this point. Existing
  in-place helpers and builder lowering may consume it if separate liveness / last
  use facts also allow the operation.

`Moved` is **not** an ownership fact. It is binding-validity information: a local
whose value has been transferred must not be used as a live binding until it is
rebound. The CFG view should provide binding validity, liveness, and last-use
facts alongside ownership, rather than folding them into the ownership lattice.

**Publication is an event, not a lattice element.** When a unique value reaches a
publication point (return, value-carrying `break`, `try` exit, storage in an
escaping aggregate, closure/task capture, channel send, global, unknown call), the
continued local fact becomes **`Shared`**. A locally handled branch or match can
still continue with `Unique` only if every continuing path preserves uniqueness.

The **join** (control-flow merge) is conservative: `Unique ⊔ Unique = Unique`,
`Unique ⊔ Shared = Shared`, and anything joined with `Unknown` is `Unknown`. A
fact survives a merge only if it holds on **every** predecessor path.

## Later precision layers

Recursive ownership shapes, field-sensitive facts, return-path ownership,
variant-payload paths, and persistent-vs-mutable region labels are later
precision layers. The worked examples keep those target cases visible so the CFG
and fact APIs do not paint us into a corner, but the first implementation should
not need them to replace the old uniqueness/liveness pass or drive the existing
local rewrites.

When those layers are added, the positive fact is refined from `Unique` into a
shape such as `Record { shell, fields }`, `Vec { elem }`, or `Dict { val }` for
path-sensitive decisions. That refinement must remain a consumer-visible
extension of the same primitive ownership analysis, not a separate optimizer-local
proof.

## The `AInit` hinge: move vs alias

`let L = init A` is the pivotal node. Given `A = ALocal(S)`:

- **Move** if `S` is dead after this point → `L` takes `S`'s ownership fact; `S`
  becomes invalid as a binding (not a `Moved` ownership fact).
- **Alias** if `S` is still live afterward → the value now has two live
  references, so **both `L` and `S` become `Shared`**.

This single rule decides the worked negatives:

- Case B `L4 = init L9` — `L9` dead after → move → `L4` stays `Unique`.
- Case C `L8 = init L7` — both `L7` and `L8` read later → alias → both `Shared`,
  so the later `call Fn295(L7,…)` sees a shared arg and stays persistent.

`AInit` therefore requires **liveness**: the analysis needs "is the source local
live after this point," which the CFG view supplies.

## The field-projection hinge: transport wrappers

Boot code often returns updated context/state records inside small result
records: `SynthOut`, `CheckOut`, `ExprOut`, `LocalOut`, `FuncIdOut`,
`RewriteResult`, ANF lowering's `FreshResult`/`ExprAccumResult`, query analysis's
`SourceLoad`/`Discovery`/`SingletonResult`, resolver `ResolveResult`, and similar
shapes. The caller then writes `ctx = out.ctx`, `state = out.state`,
`env = out.env`, or projects multiple accumulator fields, while reading sibling
result fields such as `ty`, `expr`, `local`, or `diags`.

That pattern needs path-sensitive liveness, not only whole-record liveness. A
field projection from an owned result record can transfer ownership of that field
when the projected path is dead through the wrapper afterward, even if sibling
fields are still read. The wrapper shell may remain usable for scalar/read-only
siblings, but publishing the wrapper or reading the same transported field again
would make the projection a borrow/publication instead of a move. Without this
projection hinge, every helper returning `.{ ..., ctx }` / `.{ ..., state }` looks
like it published the threaded value into an aggregate, and the analysis loses the
dominant boot threading path.

## Transfer function (per `AnfOp`)

`AnfOp` is closed, so the table is exhaustive. `let L = op`:

| Op | Effect on facts |
|---|---|
| `ACall(constructor)` — `Dict.new`, `Vector.make`, builder freeze | `L ← Unique` (fresh allocation / builder result) |
| `ARecord(fields)` | First cut: `L ← Unique` for the fresh shell, but reference-typed fields are not deeply unique unless later path-sensitive facts prove it. Each field arg follows the `AInit` move/alias hinge as binding-validity/liveness data |
| `AArrayLit` / `AVariant(args)` | First cut: `L ← Unique` for the fresh outer shell; nested element/payload ownership is a later precision layer |
| `ARecordGet(base, f)` | First cut: borrow/read; base ownership is preserved unless the projection is published. Later path-sensitive mode may move field ownership out when the field path is dead through `base` |
| `AIndex(base, i)` | Borrow/read. Indexed collection reads do not transfer element ownership in the first cut; if the result is stored/returned/captured, publish/demote when modeled |
| `ACall(consuming)` — `dict.set`, `vector.append`, `Vector.set`, summarized wrapper: *consumes p0, returns fresh/alias* | If the accepted decision uses an existing in-place/helper path, the consumed argument's binding becomes invalid and `L ← Unique` for the handed-forward result. If the site falls back to a persistent operation, `L ← Unknown` unless that operation is known to allocate fresh, unshared backing for the relevant mutation scope. **In-place is licensed iff p0 was `Unique` and last-use holds**; otherwise emit the persistent operation |
| `ARecordUpdate(base, f, v, in_place, _)` | First cut: shell in-place is licensed iff base is `Unique` and last-use holds; otherwise persistent record update. Field-sensitive replacement facts are a later precision layer |
| `AAssign(local, A)` | `local ← A.fact` (ownership transfer; the loop-carried rebind) |
| `AInit(A)` | move/alias hinge above |
| `AMakeClosure(_, captured)` | each captured local → `Shared` (published), unless the closure is proven non-escaping (future; see closure-capture.md) |
| store into an **escaping** aggregate | stored local → `Shared` |
| `AGlobalSet(_, A)` | `A → Shared` |
| `ACall(Cell.new / Cell.set / Cell.update)` — store into a `Cell` | **publish** the stored value → `Shared` (a `Cell` is a mutable box, aliasable and readable at arbitrary times). `Cell.update` reads-then-writes, so — like `Cell.get` — the value handed to the update function is `Unknown` |
| `ACall(Cell.get)` | `L ← Unknown` (contents stay aliased through the live cell). `Cell` is not an optimization target — already mutable by design; these rows only keep the analysis sound around it |
| `Return(A)` / `Break(A)` / match-arm body ending in `Return` (`try`) | publish `A` → continuing facts for that value become `Shared` on the exit edge |
| `ACall(extern/host import)` — closed boundary allow-list (`Int/Float/Bool/String/Void`, `ExternRef`/`ExternRef?`, `Vector<Byte>`, `Vector<String>`, `Result<Vector<Byte>,String>`) | **Not a publication sink.** The auto-bridge marshals every GC-typed argument into a host-owned *copy* synchronously and retains no Twinkle reference, so a `Vector<Byte>`/`Vector<String>` arg is a read-only **borrow** (arg fact preserved, *not* `Shared`), and a GC-typed *result* is host-constructed fresh → `L ← Unique`. Scalars are ownership-neutral; `ExternRef` handles are host-owned (neutral). This is stronger than the generic row below and rests on the copying-marshalling contract — see [concurrency-publication.md](concurrency-publication.md). **Staging note:** this borrow/`Unique`-result precision is the *eventual* rule; the first executable analysis ([phase2-design.md](phase2-design.md)) conservatively over-approximates externs as publication (sound, less precise) and delivers this row in **Phase 10** |
| `ACall(unknown/unsummarized)` — a *Twinkle* callee with no summary (not extern) | every reference arg → `Shared`; `L ← Unknown` |
| `ABinOp`/`AUnOp`/scalar ops | no reference-ownership effect |

Two consequences worth stating explicitly:

- **A consuming persistent op does not automatically produce a unique result.**
  Persistent vectors and dicts may share unchanged backing with their inputs (see
  [persistent-runtime.md](../../../internals/persistent-runtime.md)). On a shared or unknown input, the
  result stays `Unknown` unless the operation guarantees fresh unshared backing
  for the relevant mutation scope, or a later path/node-sensitive proof shows the
  next mutated backing is unshared. Do not conflate "semantic new value" with
  "unique storage."
- **The record quartet eventually carries two independent in-place decisions.**
  For `record_get .f` → consuming call → `record_update .f` → `assign`: the field
  backing in-place is licensed by field `f` being deeply unique; the shell
  in-place is licensed by the record shell being unique. The first implementation
  may handle only the shell/simple-collection side; field-sensitive backing facts
  are a later precision layer (worked-examples Case V).

## Control flow

- **Branch/match join.** Local facts merge by the lattice join. Case V’s
  `if … { record_update .lowlinks; assign L7 } else { }`: the updating arm ends
  with `L7` unique (consumed then reassigned) and the empty arm leaves `L7` unique
  → join `Unique`. If one arm published `L7` and the other did not, the join is
  `Shared`.
- **Loop back-edge.** The loop-header entry fact for a carried local is
  `join(pre-loop fact, back-edge fact)`, iterated to fixpoint. A local stays
  `Unique` across the loop iff every body path ends with it `Unique` (borrow-only
  reads, or consume-then-reassign) and no path publishes/aliases it. Monotone over
  the finite lattice ⇒ terminates.
- **Multi-exit publication.** `Return`, value-carrying `Break`, and `try` error
  arms are all exit edges (worked-examples Case T); each publishes the exiting
  value. A locally handled `Result`/variant `case` is different: ownership can
  flow through payload paths inside each arm and then join normally once that
  later precision layer exists.

## Function summaries

> The summary schema, SCC-ordered computation, variant identity, call-site
> decision, caps, and determinism are specified in full in
> [summary-specialization.md](summary-specialization.md). This section states just
> the fact-lattice-level shape.

The first executable summary subset should stay minimal: consumes parameter,
retains parameter, returns fresh value, returns alias. The path-keyed details
below are the later precision target for transport wrappers and specialization.

Interprocedural facts, computed per function (schema authoritative in
[summary-specialization.md](summary-specialization.md); `ParamRole` has **three**
values, with "returned" factored out into `flows_to_return` + path-keyed
`ReturnOwn` facts):

- **per parameter `base_role`** ∈ { `Borrowed` (read-only, ownership-neutral),
  `Consumed` (mutated/moved; caller’s value dead after if in-place taken),
  `Published` (escapes inside the callee) };
- **`flows_to_return`** — whether the parameter’s value flows into the return;
- **return ownership by path** (`[]`, `[.ctx]`, `Ok[0].state`, etc.) with each
  path's `ReturnOwn` ∈ `OwnedFresh` | `OwnedFromParam(k)` | `Shared`; variants
  and payload records need path segments for locally handled `Result` transport;
- **`in_place_paths`** — the field paths a `Consumed`+`flows_to_return` parameter
  mutates in place if the caller proves them owned (`[]` = shell/whole value).

Worked summaries:

- `set_at` wrapper — `p0: Consumed`, `flows_to_return`, `in_place_paths {[]}`;
  return `[]: OwnedFromParam(0)`.
- `add_type` — `p0: Consumed`, `flows_to_return`, `in_place_paths {[], [.types]}`,
  `p1,p2: Borrowed`; return `[]: OwnedFromParam(0)`.
- `visit` — `p0(State): Consumed`, `flows_to_return`; return
  `[]: OwnedFromParam(0)`; **recursive**, so its summary depends on itself.
- `fresh_meta`/`alloc_local`-style transport helper — `p0(ctx/state): Consumed`,
  `flows_to_return`, `in_place_paths {[] plus any updated reference fields}`;
  return `[]: OwnedFresh record` and `[.ctx]`/`[.state]: OwnedFromParam(0)`.
- `load_source`/`parse_cached`-style `Result` helper — `p0(state): Consumed`,
  `flows_to_return`; return `Ok[0].state: OwnedFromParam(0)` and
  `Err[0].state: OwnedFromParam(0)` when both locally handled arms carry the
  state forward.

Summaries are computed **bottom-up over call-graph SCCs** (the compiler already
has Tarjan SCC). Within a recursive SCC, iterate summaries to fixpoint with two
separate defaults: caller-visible escape/return facts start conservative
(parameters may be `Published`, returns may alias/`Shared`), while optimization
capabilities start empty. A recursive SCC may add in-place capability only when
every recursive path supports it under the current summaries; if iteration is cut
short, expose the conservative safety facts and no speculative in-place paths.

## Interprocedural specialization (later)

After the core CFG ownership engine and minimal summaries are stable, a callee
with in-place-capable parameter `k` can get mutable-specialized variants. A call
site emits the **mutable-specialized variant** iff, at that site, the argument’s
fact is `Unique` (or later path-refined unique) **and** the old version is not
observed after the call; otherwise it emits the **persistent variant**. This is
exactly Cases B ∩ C: `add_type` gets a mutable variant at `build_env`'s unique
call sites and the persistent variant at `branch_env`'s shared call site.

- **Specialization key** = the subset of the callee's in-place `(parameter,
  field-path)` requirements that are `Unique` at the site (the shell path `[]` and
  each mutated field independently — so a shared sibling field does not block a
  field's in-place update). Read-only/`Borrowed` parameters and fields never
  participate. Full schema in [summary-specialization.md](summary-specialization.md).
- **Cap.** Bound the number of ownership variants per function (composing with
  type-monomorphization, per architecture.md); on overflow, fall back to the
  persistent variant. The census suggests most functions need at most one extra
  variant (uniform caller shapes, e.g. `visit`), so the cap should rarely bind.
- No runtime uniqueness test: the caller statically selects the variant.

## Validation trace (against the anchors)

- **Case B `build_env`:** `Dict.new ⇒ Unique`; `record Env ⇒ Unique shell` with
  later field precision for unique fields; `init L9 ⇒ move`; each `call Fn295 ⇒`
  unique arg + summary in-place-capable ⇒ mutable variant; `assign` transfers the
  result back; `return ⇒` publish once. **Verdict: unique/in-place add_type once
  field precision/specialization is enabled.** ✓
- **Case C `branch_env`:** `init L7 ⇒ alias` (both `Shared`); `call Fn295(L7)`
  sees `Shared` arg ⇒ persistent variant; later `record_get L8 .types` reads the
  old version — consistent with the demotion. **Verdict: persistent.** ✓
- **Case A `sieve`:** builder freeze ⇒ `Unique`; `index` ⇒ borrow (ownership
  preserved); `call Fn297` with unique arg + wrapper summary ⇒ in-place; `assign`
  back across the back-edge; loop fixpoint holds `Unique`; never published.
  **Verdict: unique/in-place.** ✓
- **Case V `visit`:** quartet keeps `L7` unique across each field update;
  match-arm join merges to `Unique`; recursion resolved by the SCC summary
  fixpoint (all callers unique ⇒ single owned variant); nested `components` write
  moves a unique `Vector` into the field once field precision is enabled.
  **Verdict: unique/in-place, both in-place decisions per quartet.** ✓
- **Case W transport wrappers:** helper returns `[]: OwnedFresh` plus transported
  field paths such as `[.ctx]: OwnedFromParam(0)` or `[.state]:
  OwnedFromParam(0)`; caller projection moves the field when that path is dead
  through `out`; sibling reads do not publish it. **Verdict: owned handoff through
  returned field.** ✓
- **Case R Result-wrapped transport:** helper returns payload paths such as
  `Ok[0].state: OwnedFromParam(0)` and `Err[0].state: OwnedFromParam(0)`; locally
  handled match arms project and rejoin state ownership. **Verdict: owned handoff
  through variant payload field.** ✓

## Open questions

- Exact representation of `shape` for records vs nested collections — inline
  recursive fact, or interned shape ids to bound size?
- Element/value ownership for nested collections: track precisely, or default
  `Unknown` and only refine on freshly-introduced-and-never-shared contents?
- Recursive-SCC summary fixpoint direction and termination proof — confirm the
  "start conservative, promote monotonically" schedule is both sound and precise
  enough for `visit`.
- How much liveness precision does the `AInit` hinge need (per-local last-use vs
  full liveness), and can the CFG view supply it cheaply?

## Non-goals

- No runtime uniqueness flags, refcounts, or COW checks — soundness is static.
- No speculative "may be shared" mutation.
- No lattice element that permits mutation without a proof on **every** path.
- No abandoning conservative defaults for coverage.
