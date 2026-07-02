# AWFY C5 — in-place vector `set_at` (Phase 0 findings)

**Status:** Phase 0 (root-cause + guard design) complete. No code changed yet.
**Parent:** [awfy-codegen-gaps.md](awfy-codegen-gaps.md) attack vector C5.
**Branch:** `codegen-void-elim` (investigation only).

## Reframe: C5 is two independent levers

The parent doc lumps "typed representation" as the sieve/bounce fix. That is
wrong. The two write-heavy benchmarks store **non-numeric** elements:

- `sieve`: `Vector<Bool>` — hot op `flags = .set_at(k, false)`.
- `bounce`: `Vector<Ball>` where `Ball` is a **record (reference)** — hot op
  `balls = .set_at(j, res.ball)`.

Neither is helped by unboxed `PVecI64`/`PVecF64` leaves. Their cost is the
**persistent trie path-copy allocation** on every `set_at`. So:

- **Lever A — in-place `set_at`** (mutate a uniquely-owned vector, no path copy):
  the actual sieve/bounce lever, and the subject of this doc.
- **Lever B — typed unboxed leaves** (`PVecI64` etc.): a *separate* track that
  helps read-heavy numeric vectors (nbody internals, dataframe columns), already
  partially landed for read-only `Vector<Int>` (`route_typed_vec.tw`). Out of
  scope here.

## What already exists

The in-place machinery is built and *works in simple cases*:

- `vector$set_in_place` builtin → `rt_arr__set_in_place` runtime op.
- `xs[i]=v` lowers to `vector$set_unsafe`, registered in the optimizer semantics
  with `in_place_equivalent = vector$set_in_place`.
- The uniqueness optimizer rewrites the COW op to the in-place op when the base
  is proven deep-owned (`opt/uniqueness.tw`).

`rt_arr__set_in_place` navigates to the leaf via `get_leaf` and does a **raw
`array.set` — no copy-on-write, no per-leaf ownership check**. So *all*
soundness rests on the optimizer only firing when the leaf is uniquely owned.

## Phase 0 experiments (minimal repros)

In-place firing by pattern (checked via `rt_arr__set_in_place` vs `rt_arr__set`
in emitted WAT):

| pattern | in-place fires? | correct? |
|---|---|---|
| straight-line `xs[i]=v` on fresh `xs` | ✅ | ✅ |
| loop `xs[k]=v`, no read of `xs` in loop | ✅ | ✅ |
| loop `xs[k]=v` **+ interleaved `xs[k]` read** (sieve/bounce shape) | ❌ COW | ✅ |
| `xs.set_at(i,v)` **method form** (sieve/bounce use this) | ❌ COW | ✅ |
| `zs := xs.slice(..); xs[i]=v` | ✅ | ❌ **corrupts `zs`** |
| `b := Box.{v: xs}; xs[i]=v` | ✅ | ❌ **corrupts `b.v`** |
| `peek := fn() { xs.at(i) }; xs[i]=v` (closure capture) | ✅ | ❌ **corrupts capture** |
| `sink(xs); xs[i]=v` (arg to an unregistered call) | ✅ | ❌ unsound if `xs` escapes |
| `ys := xs; xs[i]=v` (direct copy-bind) | ❌ COW | ✅ |

**Reproduction notes (important — these tripped up the first pass):**

- The corruption rows fire **only inside a function body**. Top-level init
  statements do not run through the uniqueness optimizer the same way, so a
  top-level repro looks (falsely) sound. Wrap the repro in an `fn`.
- Slice's **boundary leaf is copied** (`trim_left`/`trim_right` copy the boundary
  spine), so mutating `xs` at an index that lands in the copied boundary leaf
  does *not* visibly corrupt the slice. Pick an index in a genuinely **shared
  interior leaf** (e.g. `xs := collect i in range(128) …; zs := xs.slice(0,100);
  xs[5]=999` → `zs.at(5)` returns `999`). An index near the slice boundary hides
  the bug.

### Perf gaps (why sieve/bounce stay on COW)

1. **Method form is a non-inlined wrapper.** `xs.set_at(i,v)` compiles to a call
   to the prelude `set_at` function (`set_at<A>(xs,i,v){ xs[i]=v; xs }`), emitted
   as its own function. The real `vector$set_unsafe` lives *inside* it, where the
   base is a **parameter** — and the optimizer's `.Call` in-place path requires
   `local_is_deep_owned(base)`, which a parameter is not. The bracket form
   `xs[i]=v` puts the COW op directly at the caller where the base is fresh, so it
   fires. `set_at`'s body is literally the bracket op, so **inlining it removes
   the gap**.
2. **Interleaved element read defeats it.** Even in bracket form, a read of the
   same vector in the loop body (`if xs[k] == 0 { xs[k] = v }`) drops the rewrite
   back to COW. This is exactly sieve (`if flags[i] … flags[k]=false`) and bounce
   (`balls[j].bounce_step() … balls=.set_at(j,…)`). A non-escaping element read
   is a borrow and is sound to allow before/after an in-place write to the same
   unique vector; the analysis is over-conservative here.

### Soundness bug (pre-existing, critical) — must fix first

The "corrupts" rows are a **live correctness bug** in the shipped optimizer,
demonstrated with data corruption (`zs.at(5)` returns the mutated `999` instead
of `5`; the record and closure cases likewise). Root cause, precisely:

- `ownership_of` (`opt/uniqueness.tw`) treats a local as owned when
  `!(tainted && !refreshed)` — i.e. a **`refreshed` flag unconditionally
  overrides the (conservative, whole-function) taint set**.
- The taint scan **already taints every aliasing sink correctly**
  (`scan_tainted_op` taints record-field values, variant/array-lit args, closure
  free-vars, and *all* args of an unregistered call). So the taint set is not the
  problem — the blanket `refreshed` override is.
- Where does `refreshed` come from? **Not** directly at fresh producers as one
  might assume — `classify_producer_ownership` sets *ownership*, never
  `refreshed`. `refreshed` is set only in `transfer_owned_local` and
  `with_consumed_update_result`, each **gated on the target being tainted**. A
  `collect`/array-lit result (a `fresh_result` value) transferred into the user
  local via `AInit`/`AAssign` therefore lands `refreshed` whenever that local is
  also tainted — and the tainting sink (slice base, record field, closure
  capture, escaping call arg) is exactly what makes it tainted. The result:
  fresh-then-aliased locals are simultaneously tainted *and* refreshed, and the
  override lets in-place fire on the now-shared structure, corrupting the alias.
- The forward pass only *clears* `refreshed`/ownership on a **direct copy-bind**
  (`alias_or_transfer_owned`, the "copy-bind liveness guard"). Every other
  aliasing sink leaves `refreshed` intact.

Empirically, taint alone does **not** block in-place: `sink(xs); xs[i]=v` still
emits `set_in_place` even though `sink` is unregistered and `xs` stays live.

This is masked in practice because idiomatic code uses the method form (which
never fires in-place) and rarely slices-then-bracket-mutates the original — but
it is unsound, and it is in the self-hosting compiler's own codegen path.

## Guard design

The invariant `set_in_place` needs: **the target PVec's leaves are uniquely
owned at the mutation point.** The bug is not a missing taint — taint already
covers every sink — it is the blanket `refreshed`-over-taint override in
`ownership_of`. So fix the interaction, not by re-enumerating sinks in an
ad-hoc list (that list already drifted — closure capture was missed), but by
deriving the clear-set from the **same operand-tainting cases the taint scan
uses**, so the two cannot diverge:

1. **Suppress the `refreshed` override once a still-live value is aliased (the
   soundness fix).** In the forward pass, any owned/`refreshed` local consumed by
   an operand-tainting sink — record field value, variant payload, array-lit
   element, **closure free-var (`AMakeClosure`)**, base of a sharing op
   (`slice`/`concat`), **`AGlobalSet` value**, or arg to an unregistered/
   allocating call — that remains live afterward must have its ownership **and
   `refreshed`** cleared (mirror `alias_or_transfer_owned`'s live-source branch).
   Enumerate these by mirroring `scan_tainted_op`'s tainting cases so no sink is
   missed. This makes slice/record/closure/global cases fall back to COW.
2. **Register `slice`/`concat` (and any leaf-sharing producer) in the optimizer
   semantics** so the forward pass can *recognize* them as sharing ops for #1.
   Note this is for recognition, **not** to stop an ownership leak: unregistered
   calls already taint all their args, so slice/concat already de-own via taint
   today — the gap is purely the `refreshed` override, which #1 closes.
3. Keep `set_in_place`'s raw contract; do **not** add per-leaf COW there (it
   would defeat the point). The optimizer is the sole guarantor.

## Phased plan

- **Phase 1a — fix the soundness bug (independent, lands first). DONE.** Alias
  invalidation (#1) + slice/concat recognition (#2). Success = **all** corruption
  repros (slice, record, closure capture) return the un-mutated value; existing
  straight-line/simple-loop in-place still fires; self-host + full test suite +
  AWFY checksum green. This is a correctness fix and should arguably ship
  regardless of the perf work.
- **Phase 1b — sieve (expand coverage). DONE.** Rather than an inliner, (i)
  recognize the monomorphized thin `set_at` wrapper by body shape
  (`op_aliasing_source_locals` sibling `is_thin_set_wrapper` in `uniqueness.tw`)
  and register each as a COW op whose in-place equivalent is `vector$set_in_place`
  — the existing loop rewrite then swaps the *wrapper call* to in-place at unique
  sites (no inliner; the wrapper is DCE'd). The swap is a plain callee change:
  emit's runtime-arg coercion boxes the value against `set_in_place`'s `anyref`
  ABI, so no argument reshaping is needed (the anticipated boxing hole did not
  materialize). (ii) allow a non-escaping element read (`base[i]`) of the same
  unique vector in the loop body (`op_is_index_read`) without dropping the
  rewrite. Detection tolerates trailing dead scaffolding bindings because it runs
  before per-function simplification. **Result: sieve 35.5 ms → 4.9 ms (~7×),
  bounce 1927 ms → 208 ms (~9×), both checksums unchanged.**
- **Phase 2 — bounce (deeper).** In-place `set_at` removed the vector path-copy
  and already recovered ~9× on bounce (better than expected). bounce *also*
  allocates a fresh `Ball` record per step (which `bounce_mut` avoids via a
  raw-int buffer); it remains ~2.7× off its `_mut` floor. Fully closing needs
  record-allocation elimination or a struct-of-arrays layout — a separate item.
- **Lever B (separate track):** extend `route_typed_vec` beyond read-only
  `Vector<Int>` to `set_at` and to `Float`/`Bool`; add `PVecF64`.

## Risks

- **Self-host correctness is paramount** — a wrong in-place mutation corrupts the
  compiler compiling itself. Phase 1a is the gate; expanding coverage (1b) on top
  of the current unsound base would widen the bug's blast radius.
- Phase 1a will (correctly) turn some currently-in-place-but-unsound sites back
  into COW; that is a correctness win, not a regression, but watch for any
  measurable slowdown in self-host compile time from the added conservatism.

## Recommendation

Do **Phase 1a first as a standalone soundness fix** (it stands on its own merit),
then **Phase 1b (sieve)**. Bounce and Lever B are follow-ons.
