# Rebinding-Ceremony Auto-Fix Lint Rules Plan

**Status: placeholder / skeleton.** Captures three low-ceremony rewrites we
want as `twk lint` / `twk fix` rules. No execution plan yet — this doc scopes
the patterns, detection approach, safety gates, and open questions so a later
spec can pick one up. Sequenced by risk.

## Goal

Turn the recurring "tidy up" rewrites we apply by hand into machine-applicable
lint fixes, so canonical low-ceremony style is enforced instead of remembered.
The motivating sample is commit `c2410fa4` (a manual tidy-up of
`repr_assign.tw` / `route_typed_vec.tw`), which mixed four distinct patterns.
One of the four (field-shorthand punning) is already handled by `twk fmt` and
is out of scope here.

## Current Baseline

Auto-fix rules live in `boot/compiler/lint.tw`: each rule walks the parsed AST
(`Module` / `Expr` / `Stmt` with `Span`s) plus `ResolvedEnv`, and an
auto-fixable finding carries `edits: Vector<FixEdit>` where
`FixEdit = .{ start, end, replacement }` — source byte-range rewrites.
Suggestions are rendered by slicing source spans, not by re-printing AST.
Rule rationale strings live in `boot/compiler/lint_rules.tw` (`describe`).

Relevant existing rules:

* `direct-rebinding` (report-only) — a temporary that only aliases a value,
  gets rebinding-updates, then is copied back; rebind the path directly.
* `record-copy-helper` (report-only) — a `with_*`-style fn returning a record
  literal that copies most fields from a same-type param.
* `inline-record-copy` — a local binding whose record literal copies a majority
  of its fields from one source. **Auto-fix today only handles self-rebinds**
  (`st = St.{…st…}`, via `--fix-inline-record-copy`).

"Detectable" here means: the pattern is recognizable in the AST, and the
rewrite is expressible as safe byte-range edits.

---

## Pattern A — Named constructor → contextual anonymous `.{ }` (new rule)

**Rewrite.** `TypeName.{ … }` → `.{ … }` where the expected record type is
already known. The edit is trivial: delete the `TypeName` prefix span, keep
`.{ … }`.

```tw
// Before                                         // After
ReprWasmResult.{ repr, wasm_type: .Anyref, cache }  .{ repr, wasm_type: .Anyref, cache }
```

**Why this is the low-risk one.** The rewrite itself is mechanical. The only
hazard is the safety gate: the anonymous form is legal *only* where an expected
record type is known. That is a semantic condition, but a **syntactically
provable subset** covers most real cases without full type inference:

* annotated binding — `let x: T = T.{ … }`
* return position of a fn whose declared return type is `T`
* `case` / `if` arm in one of the above positions
* record field value whose field type is `T`
* argument to a param whose declared type is `T` (needs callee lookup — may
  defer)

**Open questions.**
* How much of the "expected type known" gate to prove syntactically vs. pull
  from `ResolvedEnv` type info — do we have per-expression inferred types at
  lint time, or only the resolved env?
* Do we require `TypeName` to *match* the expected type exactly before
  stripping (it always should, or it wouldn't typecheck — but confirm we never
  strip in a spot where the prefix was load-bearing, e.g. a variant-path
  disambiguation)?
* Interaction with `twk fmt`: confirm fmt does not re-add the prefix.

**Relationship to existing rules.** None — genuinely new.

---

## Pattern B — Drop numbered accumulators, rebind in place (new rule)

**Rewrite.** Collapse a freshly-named intermediate (`acc2`, `acc3`, `cur`,
`cur_acc`, `cache2`) that only threads a value forward into a rebind of the
original name. Two shapes seen in `c2410fa4`:

```tw
// straight-line thread
acc2 := f(op, acc)               →   acc = f(op, acc)
g(rest, acc2)                        g(rest, acc)

// loop accumulator
cur := acc                           for arm in arms {
for arm in arms {                =>    acc = h(arm.body, acc)
  cur = h(arm.body, cur)             }
}                                    acc
cur
```

**Why this needs the careful plan.** This is a genuine dataflow refactor, not a
syntactic one. Safety requires liveness reasoning:

* the original name must be **dead** after the shadow point (no later read of
  the pre-rebind value),
* the collapse must not cross a scope where the two names have distinct meaning,
* rebinding a function parameter is legal but must not clobber a value the rest
  of the body still needs.

Getting liveness subtly wrong here silently changes behavior, so this rule is
the highest-risk of the three and likely wants report-only first, auto-fix
gated behind a conservative "obviously safe" shape (single forward use,
tail-return of the temp).

**Open questions.**
* Do we scope the auto-fix to only the two canonical shapes above (immediate
  single-use thread; `cur := acc` loop seed returned at tail), refusing
  anything else?
* Liveness: reuse any existing dataflow/liveness pass, or a local
  self-contained "is name read after span" check per binding?
* Naming heuristic — do we only fire when the temp is a numbered/`cur_`-prefixed
  sibling of an in-scope name, or on any single-use forward-threaded temp?
* Confirm no capture hazard (temp closed over by a later closure).

**Relationship to existing rules.** Spiritually the `direct-rebinding` rule
generalized from field/index paths to threaded accumulators; may share the
report-only detector.

---

## Pattern C — Full-record reconstruction → single-field rebind (extend `inline-record-copy`)

**Rewrite.** A record literal that copies all-but-a-few fields verbatim from one
source record becomes a field-rebind of that source. Already the intent of
`inline-record-copy`, but today's auto-fix only covers self-rebinds. The
`c2410fa4` cases go further:

```tw
// non-self return (biggest win: stops re-listing every unchanged field)
ReprAssignResult.{ func: PreparedFunc.{ …11 fields, slots: new_slots… }, cache }
  →   pf.slots = new_slots
      .{ func: pf, cache }

// block-expression synthesis for a copy-with-one-change in expression position
cache: ReprAssignCache.{ …, layout_cache: cache.layout_cache.set(k, v) }
  →   cache: { cache.layout_cache = .set(k, v); cache }
```

**Why the harder variants stay hard.** The self-rebind case is edit-friendly.
The broader cases need:

* proving N-1 fields are identity copies (`r.fieldK`) of the **same** base `r`,
* that base being a rebindable name in scope,
* synthesizing the block-expression form (`{ r.f = …; r }`) when the
  reconstruction is in expression position — the least edit-friendly output,
* handling nested reconstruction (a record built inside another record's field).

**Open questions.**
* How many variants beyond self-rebind to take on — just the "return a record
  copying fields from a param" (the `record-copy-helper` shape, currently
  report-only), or also the in-expression block-synthesis form?
* Threshold for "majority of fields copied" — reuse `inline-record-copy`'s
  existing heuristic or tighten it.
* Block-expression synthesis: is emitting `{ r.f = …; r }` via byte edits
  worth it, or leave those report-only and let humans apply?
* Leading-dot path reuse (`r.f = .set(k, v)` for `r.f = r.f.set(k, v)`) — does
  fmt already normalize this, or does the fixer emit it directly?

**Relationship to existing rules.** Extends `inline-record-copy`; overlaps
`record-copy-helper` (the return-position variant).

---

## Sequencing

1. **Pattern A** — smallest, self-contained, mechanical rewrite behind a
   syntactic gate. Good first spec / quick win.
2. **Pattern C** — extend an existing rule incrementally; take the easy
   variants (return-position copy) before block-expression synthesis.
3. **Pattern B** — last; needs liveness and the most conservative gating.

Each pattern gets its own spec → plan → implementation cycle when picked up.

## Out of scope

* Field-shorthand punning (`cache: cache2` → `cache`) — handled by `twk fmt`.
* Any rewrite that changes observable behavior.
