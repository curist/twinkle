# View (generic stdlib type) method-not-found diagnostic

## Problem

Calling a method that does **not** exist on a `View<C>` receiver crashes the
compiler instead of producing a clean "no method" diagnostic. The crash is
internal (lowering / backend verifier), with no source location.

### Minimal repro (3 lines)

```tw
use @std.view
v := view.from([1])
x := v.reverse()      // backend verifier: AMakeClosure result must have
                      //   ClosureRef repr, got OpaqueAnyref
```

The trigger is the conjunction of **two** conditions:

1. the receiver is a **bound local** whose `View` backing parameter `C` is still
   an **unresolved metavar** (`View<?>`) — i.e. an un-annotated `:=` binding —
   and
2. the absent method name resolves to a **closure-shaped** signature somewhere
   (`reverse`, `map`, `filter`, …).

Either condition alone is fine, which is why the two obvious variations report
cleanly:

| Form | Result |
|---|---|
| `view.from([1]).reverse()` (inline receiver) | clean — `type View<Vector<Int>> has no method 'reverse'` (check) |
| `v: View<Vector<Int>> = view.from([1]); v.reverse()` (annotated) | clean — same check-stage error |
| `v := view.from([1]); x := v.append(4)` (non-closure name) | clean — `cannot resolve method value: append` (lower) |
| `v := view.from([1]); x := v.bogus_method(42)` | clean — `cannot resolve method value` (lower) |
| **`v := view.from([1]); x := v.reverse()`** | **crash** — `AMakeClosure ... OpaqueAnyref` |
| **`v := view.from([1,2,3]); x := v.map(fn(n){n*2})`** | **crash** — `op_kind_from: unexpected type Void` |

Annotating the binding (`v: View<Vector<Int>>`) or calling inline both resolve
`C` before the method-call check and yield the clean error — the difference is
entirely whether `C` is resolved at that point.

This surfaced while shrinking the `View` API (`map`/`filter`/`flat_map`/
`zip_with` were removed in favour of `iter()`), but it is **not** caused by that
change: `reverse` was never a `View` method and crashes the same way. It is a
pre-existing defect in how absent methods are diagnosed when the receiver's
generic parameter is unresolved.

## Root cause

Two layers conspire.

### 1. Checker leaks absent methods to lowering when the receiver param is unresolved

`boot/compiler/checker.tw:2218-2241` — when `lookup_method_sig(base_ty,
method_name)` returns `.None`, a clean `.NoMethod` diagnostic is emitted **only
if** `method_owner_name(base_ty)` is `.Some` (true for records, `Dict`,
builtins, and a fully-resolved `View<Vector<Int>>`). When `base_ty` is
`View<?>` — an un-annotated `:=` binding whose backing `C` is still a metavar —
`method_owner_name` returns `.None` (it can't name the owner module from an
unresolved head), so the checker `return .None` — "this isn't a method call I
recognize" — and the call falls through to the method-*value* lowering path
instead of being rejected at check time.

The likely fix point is right here: zonk `base_ty` before the owner lookup, or
resolve the owner from the head type constructor (`View`) regardless of whether
its parameters are resolved — the owner of a method on `View<_>` does not depend
on `C`. Either makes the un-annotated case report `.NoMethod` like the
annotated/inline cases already do.

### 2. Lowering's not-found recovery returns a bare `Void`

`boot/compiler/lower_core/records.tw:246-252` — `lower_method_value` emits
`cannot resolve method value: <name>` and returns `void_expr(s)` (type `Void`).
That recovery is fine when the result is discarded, but when the failed call has
closure arguments (`map`/`filter`) or feeds a typed operation, the `Void` poison
propagates:

- `op_kind_from` (`boot/compiler/lower_anf.tw:53-60`) traps on the `Void` type
  (`op_kind_from: unexpected type Void`), and/or
- the closure argument lowers to an `AMakeClosure` whose repr the backend
  verifier rejects (`boot/compiler/backend/verify_expr.tw:193`,
  `... got OpaqueAnyref`).

So the user sees an internal crash rather than the `cannot resolve method value`
error that was already emitted.

## Fix options

**(A) Fix at the checker (preferred).** Make `method_owner_name` resolve the
owner from the receiver's head constructor even when its type parameters are
unresolved (`View<?>`), or zonk `base_ty` before the lookup — see root cause #1.
Then an absent method on an un-annotated `View` binding is rejected with
`.NoMethod` at check time, exactly like the annotated/inline cases and like
`record`/`Dict`. This removes the leak to lowering entirely and gives the best
message (`type View<...> has no method 'map'`, with a span).

**(B) Harden the lowering recovery (defense in depth).** When method-value
resolution fails, the recovery expression must not be a bare `Void` that
downstream passes treat as a real value. Emit the diagnostic and return a
typed/abort poison node so `op_kind_from` and the backend verifier never choke,
guaranteeing that *no* unresolved method can ever crash the backend — regardless
of receiver type.

Recommendation: do **both**. (A) is the correct-layer fix and matches the
established record/Dict behaviour; (B) is a cheap safety net that turns any
future "leaked to lowering" case into a clean error instead of a crash.

## Test plan (TDD)

Add checker/lowering tests asserting clean diagnostics (no crash). Use the
**un-annotated bound-local** form — that is the shape that currently crashes:

- `v := view.from([1]); x := v.reverse()` → `type View<...> has no method
  'reverse'`, no verifier crash (primary repro)
- `v := view.from([1]); x := v.map(fn(n){n})` → no-method diagnostic
- regression guards that already pass and must stay clean: the inline
  (`view.from([1]).reverse()`) and annotated
  (`v: View<Vector<Int>> = ...; v.reverse()`) forms, and a non-closure absent
  name (`v.append(4)`)
- a non-`View` generic stdlib type (e.g. `@std.queue` `Queue`) bound
  un-annotated and called with an absent closure-taking method → clean
  diagnostic (confirms the fix generalizes beyond `View`)

Each crashing test should fail first by crashing the compiler (current
behaviour), then pass with a clean diagnostic.

## Notes

- The removed `View.map`/`filter`/`flat_map`/`zip_with` make this reachable for
  anyone upgrading who still calls them; until fixed, the migration path is
  `v.iter().map(f).filter(g).to_vector()`.
- This is the unfinished diagnostic half of treating contract-satisfier stdlib
  types as first-class method owners; see the access-contract design in
  [design/contracts.md](../design/contracts.md).
