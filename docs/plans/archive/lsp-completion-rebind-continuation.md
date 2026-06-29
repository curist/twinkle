# LSP completion — meaningful continuation at `LHS = .`

Status: Done (landed on branch `lsp-completion-continuation`) · Boot-only · 2026-06-29

## Goal

Limit completion candidates at a type-constrained position to **meaningful /
logical continuations** — candidates whose result type could actually typecheck
in that position. The concrete instance is the rebinding receiver shorthand:

```tw
xs: Vector<Int> = ...
xs = .<cursor>
```

Today the cursor sits after `=` then `.`, and the classifier routes `= .` to
the catch-all `.Variant` context, which lists *every* variant in scope (plus
`Option`/`Result`) — noisy and mostly wrong. We want `xs = .` to offer only the
continuations that produce `Vector<Int>`: methods whose return unifies with the
target type, plus variant constructors of that type.

## Principle

At a position whose type is fixed by context (the rebind target or the typed
binding's annotation in `LHS = .`), offer only candidates whose **result type
unifies with the expected type A**, restricted to the syntactic forms that are
legal at that position.

| Position | Expected type A | Legal forms |
|---|---|---|
| `a = .` (rebind) | type of `a` | methods returning ≈ A, variants of A |
| `a: T = .` (typed binding) | `T` | variants / literals of `T` (no method shorthand) |
| `state.items = .` (field-path rebind) | type of the field path | methods returning ≈ A, variants of A |

### Why "unifies with A", not "returns exactly A"

`xs = .map(cb)` desugars to `xs = xs.map(cb)`. `map` returns `Vector<B>`; the
rebind only typechecks if `Vector<B> = Vector<Int>`, i.e. the type system forces
`cb` to return `Int`. So `.map` **is** a valid continuation — the rebind doesn't
forbid it, it *constrains* the callback. The correct filter is therefore "the
method's return type can unify with A", treating the method's own generic params
as flexible:

- `.append(x)` → `Vector<Int>` — unifies → shown.
- `.map(cb)` → `Vector<B>` — `B` is the method's own generic ⇒ wildcard ⇒
  `Vector<_>` unifies with `Vector<Int>` → shown.
- `.slice(lo, hi)` → `Vector<Int>` — unifies → shown.
- `.len()` → `Int` — cannot unify with `Vector<Int>` → hidden.

### Why fields are excluded

The rebinding receiver shorthand fires only for `.lowercase(` — a method call
*with parens* (`parser.tw:2551-2556`, `2645-2648`). `a = .x` (bare field, no
paren) does **not** desugar to `a = a.x`; the parser treats `.x` as a normal
expression. Offering a plain record field would insert syntactically invalid
text, so fields are not candidates. Variant constructors (`.Some(x)`,
uppercase) reach the same `= .` cursor position through a different parser path
and *do* construct A directly, so they are kept.

## Design

### 1. Classification (`boot/compiler/query/cursor_context.tw`)

When `prev == .Dot` and the token before the dot is `.Eq`, scan backward over
the current statement to classify the assignment LHS:

- `Ident =` → rebind; expected type = type of that ident; forms = methods + variants.
- `Ident : <type> =` → typed binding; expected type = the annotation; forms = variants only.
- `Ident (. Ident)+ =` → field-path rebind; expected type = the field path; forms = methods + variants.

Emit a new context:

```tw
pub type RebindInfo = .{
  dot_offset: Int,
  target: RebindTarget,   // Ident(name) | FieldPath(Vector<String>) | Typed(name, TypeExpr)
  allow_methods: Bool,    // false for typed bindings
}

pub type CompletionContext = {
  Member(MemberContext),
  General(String),
  Import(ImportPrefix),
  Variant,
  RebindContinuation(RebindInfo),
}
```

If the back-scan cannot classify the LHS, fall back to today's `.Variant` so the
user is never blocked. This module stays syntax-only (token/text inspection,
no semantic lookup) per its existing contract.

### 2. Dispatch + expected-type resolution (`boot/compiler/query/completion.tw`)

Add a `.RebindContinuation(info)` arm to `complete()` calling a new
`rebind_continuation_completions(snap, info, source)`. It resolves A:

- `Ident(name)` → reuse `resolve_receiver_by_name` (name-based, edit-stable),
  then the existing type_map fallback.
- `FieldPath(segments)` → resolve the head ident's type, then walk fields using
  the same substitution logic as `completions_for_type`'s record-field branch.
- `Typed(_, type_expr)` → resolve the annotation `TypeExpr` to a `MonoType`
  through the resolved env.

If A cannot be resolved, fall back to unfiltered candidates (do not block).

### 3. Candidate gathering + filter

- Methods (only when `info.allow_methods`): gather via the existing
  `method_completions_for_type(env, A)`, then keep only entries whose registered
  signature's **return type unifies with A** (predicate below).
- Variants: gather via the existing `variants_for_type(env, A)`. These construct
  A directly, so they pass by construction; kept for all positions.
- Fields: excluded.

### 4. Unify predicate

```tw
fn return_unifies(env, ret_ty: MonoType, method_type_params, a_ty: MonoType) Bool
```

Substitute the method's own generic params in `ret_ty` with wildcards, then
structurally match against the ground `a_ty`:

- wildcard (a method generic) matches anything;
- `Named(tid, args)` vs `Named(tid, args')` — same `tid` and arity, args match pairwise;
- builtin shapes (`Vector`, `Optional`, `Result`, `Dict`, `String`, `Int`, …)
  matched structurally;
- otherwise false.

Examples: `Vector<B>` vs `Vector<Int>` → true; `Int` vs `Vector<Int>` → false.

This is a one-directional matchability check ("could the rebind typecheck"),
deliberately lighter than the checker's `InferCtx`-bound `unify`.

## Touch points

- `boot/compiler/query/cursor_context.tw` — new context + LHS back-scan.
- `boot/compiler/query/completion.tw` — dispatch arm, handler, filter, predicate.
- No change to `boot/lib/lsp/completion.tw` (item shape unchanged).
- No stage0 change (LSP is boot-only).

## Testing (`boot/tests/suites/lsp_completion_suite.tw`)

- `xs = .` on `Vector<Int>` → includes `append`, `map`, `slice`; excludes `len`.
- `o = .` on `Option<Int>` → includes `Some`, `None`.
- `a: T = .` typed binding → **no methods offered** (back-scan detects the
  `:` annotation and bails to existing variant completion). Precise
  variants-of-`T` filtering is deferred; the v1 guarantee is "no method
  suggestions where the receiver shorthand can't fire."
- `state.items = .` field-path target → resolves and filters like a plain rebind.
- Unresolved target type → falls back to unfiltered candidates.
- Plain record field never appears as a `= .` candidate.

Verify with `make boot-test`; exercise the live server with `make bundle-cli`
when validating end to end.

---

# Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `LHS = .` completion offer only continuations whose result type unifies with the target type — methods returning ≈A plus variants of A — instead of the current dump of every in-scope variant.

**Architecture:** A new syntax-only `RebindContinuation` completion context is produced by a backward token scan in `cursor_context.tw`. The completion query resolves the target type, gathers candidate methods/variants, and filters methods through a one-directional "return type unifies with A" predicate. Typed bindings (`a: T = .`) are detected and left to existing variant completion (no methods).

**Tech Stack:** Twinkle (`.tw`), boot self-hosted compiler/LSP, boot test runner. Boot-only — no Rust/stage0 changes.

## Conventions for every task

- Build the test compiler with `make boot-test` (runs `target/twk run boot/tests/main.tw`). To run faster while iterating you may build once and re-run, but the authoritative check is `make boot-test`.
- After editing any `.tw`, run `target/twk fmt <file>` then `target/twk lint <file>`.
- Tests live in `boot/tests/suites/lsp_completion_suite.tw`, registered in `boot/tests/main.tw` (already wired). Use the existing `open_then_complete`, `completion_labels`, `has_label`, `count_label` helpers.
- Commit after each task with a short imperative subject.

---

### Task 1: Add the `RebindContinuation` completion context

**Files:**
- Modify: `boot/compiler/query/cursor_context.tw` (the `CompletionContext` enum near line 21)

- [ ] **Step 1: Add the target + context types**

In `cursor_context.tw`, next to `MemberContext`/`CompletionContext`, add:

```tw
pub type RebindTarget = {
  Ident(String),
  FieldPath(Vector<String>),
}

pub type RebindInfo = .{ dot_offset: Int, target: RebindTarget }
```

And add a case to the existing enum:

```tw
pub type CompletionContext = {
  Member(MemberContext),
  General(String),
  Import(ImportPrefix),
  Variant,
  RebindContinuation(RebindInfo),
}
```

- [ ] **Step 2: Add a stub dispatch arm so the tree stays buildable**

Adding the enum case makes `complete()`'s `case ctx { ... }` in `completion.tw` non-exhaustive (Twinkle requires exhaustive matches). Add a temporary arm now so every commit builds; Task 3 replaces it with the real handler.

In `boot/compiler/query/completion.tw`, in `complete()`:

```tw
    .RebindContinuation(_) => [],
```

- [ ] **Step 3: Verify it compiles**

Run: `target/twk fmt boot/compiler/query/cursor_context.tw && target/twk build boot/main.tw -o /tmp/boot.wasm`
Expected: builds clean.

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/query/cursor_context.tw boot/compiler/query/completion.tw
git commit -m "lsp: add RebindContinuation completion context type"
```

---

### Task 2: Back-scan the LHS to classify `LHS = .`

**Files:**
- Modify: `boot/compiler/query/cursor_context.tw` (`classify_completion`, the `if prev.kind == .Dot` block near line 55; add a new `fn classify_rebind_lhs`)

- [ ] **Step 1: Add the back-scan helper**

Add this function to `cursor_context.tw`:

```tw
fn classify_rebind_lhs(tokens: Vector<Token>, eq_idx: Int, source: String) RebindTarget? {
  if eq_idx < 1 {
    return .None
  }

  j := eq_idx - 1

  // The token immediately before `=` must be the last LHS segment.
  if tokens[j].kind != .Ident {
    return .None
  }

  // Walk back over an `Ident (Dot Ident)*` run to find its head.
  head_idx := j
  k := j - 1

  for k >= 1 and tokens[k].kind == .Dot and tokens[k - 1].kind == .Ident {
    head_idx = k - 1
    k = k - 2
  }

  // Typed binding (`a: T = .`): not a rebind, receiver shorthand can't fire.
  if head_idx >= 1 and tokens[head_idx - 1].kind == .Colon {
    return .None
  }

  // The run must start a statement (or be the very first token, or follow a
  // clear block/stmt opener). Otherwise this `=` is not a rebinding LHS.
  if head_idx > 0 and !tokens[head_idx].preceded_by_newline {
    before := tokens[head_idx - 1]

    if before.kind != .LBrace and before.kind != .FatArrow {
      return .None
    }
  }

  // Collect segment names left-to-right.
  names: Vector<String> = []
  i := head_idx

  for i <= eq_idx - 1 {
    if tokens[i].kind == .Ident {
      names = .append(source.slice(tokens[i].span.start, tokens[i].span.end))
    }

    i = i + 1
  }

  if names.len() == 1 {
    .Some(.Ident(names[0]))
  } else {
    .Some(.FieldPath(names))
  }
}
```

- [ ] **Step 2: Route `.Eq`-before-dot through the back-scan**

In `classify_completion`, replace the existing dot block:

```tw
  if prev.kind == .Dot {
    if cursor_idx >= 2 {
      before_dot := tokens[cursor_idx - 2]

      if before_dot.kind == .LBrace
        or before_dot.kind == .FatArrow
        or before_dot.kind == .Comma
        or before_dot.kind == .ColonEq
        or before_dot.kind == .Eq {
        return .Variant
      }
    } else {
      return .Variant
    }

    dot_offset := prev.span.start

    return .Member(.{ dot_offset, receiver_name: .Some(extract_ident_before(source, dot_offset)) })
  }
```

with:

```tw
  if prev.kind == .Dot {
    if cursor_idx >= 2 {
      before_dot := tokens[cursor_idx - 2]

      if before_dot.kind == .Eq {
        case classify_rebind_lhs(tokens, cursor_idx - 2, source) {
          .Some(target) => return .RebindContinuation(.{ dot_offset: prev.span.start, target }),
          .None => return .Variant,
        }
      }

      if before_dot.kind == .LBrace
        or before_dot.kind == .FatArrow
        or before_dot.kind == .Comma
        or before_dot.kind == .ColonEq {
        return .Variant
      }
    } else {
      return .Variant
    }

    dot_offset := prev.span.start

    return .Member(.{ dot_offset, receiver_name: .Some(extract_ident_before(source, dot_offset)) })
  }
```

- [ ] **Step 3: Format, lint, build**

Run: `target/twk fmt boot/compiler/query/cursor_context.tw && target/twk lint boot/compiler/query/cursor_context.tw && target/twk build boot/main.tw -o /tmp/boot.wasm`
Expected: formats/lints clean; builds (the stub arm from Task 1 keeps the match exhaustive). Behavior is unchanged so far — the new context resolves to `[]`.

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/query/cursor_context.tw
git commit -m "lsp: classify LHS = . as rebind continuation"
```

---

### Task 3: Dispatch + variants-only handler (end-to-end skeleton)

This task wires the new context through with variants only (no method filtering yet), so we get a passing end-to-end test before adding the predicate.

**Files:**
- Modify: `boot/compiler/query/completion.tw` (imports near line 19; `complete()` dispatch near line 37; add handler + `resolve_target_type`/`walk_field_path`/`field_type`)
- Test: `boot/tests/suites/lsp_completion_suite.tw`

- [ ] **Step 1: Write the failing test**

Add to the `.test(...)` chain in `lsp_completion_suite.tw`:

```tw
    .test(
      "rebind continuation on enum shows variants",
      fn() {
        valid := "type Color = { Red, Green }\nc := Color.Red\nc\n"
        incomplete := "type Color = { Red, Green }\nc := Color.Red\nc = .\n"
        step := open_then_complete(valid, incomplete, 2, 6)
        labels := try completion_labels(step)

        if !has_label(labels, "Red") or !has_label(labels, "Green") {
          return .Err("expected 'Red' and 'Green', got: ${labels}")
        }

        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify it fails**

Run: `make boot-test`
Expected: FAIL — the new test runs (build is green from the stub arm) but `Red`/`Green` are absent because the stub returns `[]`.

- [ ] **Step 3: Add imports**

In `completion.tw`, extend the cursor_context import:

```tw
use compiler.query.cursor_context.{ImportPrefix, RebindInfo, RebindTarget}
```

(Keep the existing `use compiler.query.cursor_context` line.)

- [ ] **Step 4: Replace the stub dispatch arm**

In `complete()`'s `case ctx { ... }`, replace `.RebindContinuation(_) => [],` with:

```tw
    .RebindContinuation(info) => rebind_continuation_completions(snap, info, source),
```

- [ ] **Step 5: Add the handler and target resolution**

Add these functions to `completion.tw`:

```tw
// ── Rebind continuation completions ─────────────────────────────────
fn rebind_continuation_completions(snap: SemanticSnapshot, info: RebindInfo, source: String) Vector<
  lsp_completion.CompletionItem,
> {
  typed := case snap.typed {
    .Some(v) => v,
    .None => return [],
  }

  dot_offset := info.dot_offset
  if dot_offset <= 0 {
    return []
  }

  search_offset := dot_offset - 1

  cursor_offset := dot_offset + 1
  lexed := lexer.lex_with_cursor(source, 0, cursor_offset)
  reparsed := parser.parse_tokens(source, 0, lexed.value, lexed.diagnostics)
  reparse_items := reparsed.value.items

  ty := case resolve_target_type(reparse_items, typed, search_offset, info.target) {
    .Some(t) => t,
    .None => return all_variant_completions(snap),
  }

  // Variants of the target type construct it directly.
  variants_for_type(typed.env, ty)
}

fn resolve_target_type(items: Vector<Item>, typed: CheckResult, offset: Int, target: RebindTarget) MonoType? {
  case target {
    .Ident(name) => resolve_receiver_by_name(items, typed, offset, name),
    .FieldPath(segments) => {
      base := try resolve_receiver_by_name(items, typed, offset, segments[0])
      walk_field_path(typed.env, base, segments, 1)
    },
  }
}

fn walk_field_path(env: ResolvedEnv, ty: MonoType, segments: Vector<String>, idx: Int) MonoType? {
  if idx >= segments.len() {
    return .Some(ty)
  }

  next := try field_type(env, ty, segments[idx])
  walk_field_path(env, next, segments, idx + 1)
}

fn field_type(env: ResolvedEnv, ty: MonoType, field_name: String) MonoType? {
  case ty {
    .Named(tid, args) => case env.lookup_type_def(tid) {
      .Some(.Record(_, params, fields)) => {
        for f in fields {
          if f.name == field_name {
            return .Some(type_util.subst_type_params_lenient(f.ty, params, args))
          }
        }

        .None
      },
      _ => .None,
    },
    _ => .None,
  }
}
```

- [ ] **Step 6: Format, lint, run test**

Run: `target/twk fmt boot/compiler/query/completion.tw && target/twk lint boot/compiler/query/completion.tw && make boot-test`
Expected: the new "rebind continuation on enum shows variants" test passes; all other tests stay green.

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/query/completion.tw boot/tests/suites/lsp_completion_suite.tw
git commit -m "lsp: handle rebind continuation context (variants)"
```

---

### Task 4: Return-type unify predicate

**Files:**
- Modify: `boot/compiler/query/completion.tw` (add predicate helpers)

- [ ] **Step 1: Add the predicate helpers**

Add to `completion.tw`:

```tw
// ── Return-type unification (one-directional matchability) ──────────
fn is_wild(t: MonoType) Bool {
  case t {
    .Var(_) => true,
    .MetaVar(_) => true,
    _ => false,
  }
}

fn all_match(xs: Vector<MonoType>, ys: Vector<MonoType>) Bool {
  for i in 0..xs.len() {
    if !types_match_wildcard(xs[i], ys[i]) {
      return false
    }
  }

  true
}

fn types_match_wildcard(a: MonoType, b: MonoType) Bool {
  if is_wild(a) or is_wild(b) {
    return true
  }

  case a {
    .Int => case b { .Int => true, _ => false },
    .Float => case b { .Float => true, _ => false },
    .Bool => case b { .Bool => true, _ => false },
    .Byte => case b { .Byte => true, _ => false },
    .String => case b { .String => true, _ => false },
    .Void => case b { .Void => true, _ => false },
    .Vector(x) => case b { .Vector(y) => types_match_wildcard(x, y), _ => false },
    .Optional(x) => case b { .Optional(y) => types_match_wildcard(x, y), _ => false },
    .Result(xo, xe) => case b {
      .Result(yo, ye) => types_match_wildcard(xo, yo) and types_match_wildcard(xe, ye),
      _ => false,
    },
    .Dict(xk, xv) => case b {
      .Dict(yk, yv) => types_match_wildcard(xk, yk) and types_match_wildcard(xv, yv),
      _ => false,
    },
    .Named(ta, aa) => case b {
      .Named(tb, bb) => ta == tb and aa.len() == bb.len() and all_match(aa, bb),
      _ => false,
    },
    .Function(pa, ra) => case b {
      .Function(pb, rb) => pa.len() == pb.len() and all_match(pa, pb) and types_match_wildcard(ra, rb),
      _ => false,
    },
    _ => false,
  }
}

fn bind_vars(pat: MonoType, ground: MonoType, subst: Dict<String, MonoType>) Dict<String, MonoType> {
  case pat {
    .Var(name) => subst.set(name, ground),
    .Vector(p) => case ground {
      .Vector(g) => bind_vars(p, g, subst),
      _ => subst,
    },
    .Optional(p) => case ground {
      .Optional(g) => bind_vars(p, g, subst),
      _ => subst,
    },
    .Result(po, pe) => case ground {
      .Result(go, ge) => bind_vars(pe, ge, bind_vars(po, go, subst)),
      _ => subst,
    },
    .Dict(pk, pv) => case ground {
      .Dict(gk, gv) => bind_vars(pv, gv, bind_vars(pk, gk, subst)),
      _ => subst,
    },
    .Named(_, pargs) => case ground {
      .Named(_, gargs) => {
        s := subst

        for i in 0..pargs.len() {
          if i < gargs.len() {
            s = bind_vars(pargs[i], gargs[i], s)
          }
        }

        s
      },
      _ => subst,
    },
    _ => subst,
  }
}

fn apply_subst(ty: MonoType, subst: Dict<String, MonoType>) MonoType {
  case ty {
    .Var(name) => case subst[name] {
      .Some(t) => t,
      .None => ty,
    },
    .Vector(e) => .Vector(apply_subst(e, subst)),
    .Optional(e) => .Optional(apply_subst(e, subst)),
    .Result(o, e) => .Result(apply_subst(o, subst), apply_subst(e, subst)),
    .Dict(k, v) => .Dict(apply_subst(k, subst), apply_subst(v, subst)),
    .Named(tid, args) => .Named(tid, collect a in args { apply_subst(a, subst) }),
    .Function(ps, r) => .Function(collect p in ps { apply_subst(p, subst) }, apply_subst(r, subst)),
    _ => ty,
  }
}

fn method_return_unifies(sig: FunctionSig, ty: MonoType) Bool {
  ret := case sig.ret {
    .Some(r) => r,
    .None => return false,
  }

  if sig.params.len() == 0 {
    return false
  }

  recv := sig.params[0]
  subst := bind_vars(recv, ty, Dict.new())
  ret2 := apply_subst(ret, subst)
  types_match_wildcard(ret2, ty)
}
```

- [ ] **Step 2: Build to verify it compiles**

Run: `target/twk fmt boot/compiler/query/completion.tw && target/twk build boot/main.tw -o /tmp/boot.wasm`
Expected: builds clean. (No behavior change yet — predicate unused until Task 5.)

- [ ] **Step 3: Commit**

```bash
git add boot/compiler/query/completion.tw
git commit -m "lsp: add return-type unify predicate for rebind continuation"
```

---

### Task 5: Gather A-returning methods and wire them in

**Files:**
- Modify: `boot/compiler/query/completion.tw` (`rebind_continuation_completions`; add `methods_returning`/`filter_method_entries`)
- Test: `boot/tests/suites/lsp_completion_suite.tw`

- [ ] **Step 1: Write the failing test**

Add to the suite:

```tw
    .test(
      "rebind continuation on vector shows A-returning methods, hides len",
      fn() {
        valid := "xs := [1, 2, 3]\nxs\n"
        incomplete := "xs := [1, 2, 3]\nxs = .\n"
        step := open_then_complete(valid, incomplete, 1, 6)
        labels := try completion_labels(step)

        if !has_label(labels, "append") {
          return .Err("expected 'append', got: ${labels}")
        }

        if !has_label(labels, "map") {
          return .Err("expected 'map', got: ${labels}")
        }

        if has_label(labels, "len") {
          return .Err("did not expect 'len' (returns Int, not Vector), got: ${labels}")
        }

        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify it fails**

Run: `make boot-test`
Expected: FAIL — `append`/`map` missing (handler only returns variants).

- [ ] **Step 3: Add method gathering**

Add to `completion.tw`:

```tw
fn methods_returning(env: ResolvedEnv, ty: MonoType) Vector<lsp_completion.CompletionItem> {
  items: Vector<lsp_completion.CompletionItem> = []

  case ty {
    .Named(tid, _) => case lookup_type_methods(env, tid) {
      .Some(entries) => {
        items = .concat(filter_method_entries(env, entries, ty))
      },
      .None => {},
    },
    _ => {},
  }

  case builtin_type_name(ty) {
    .Some(name) => case env.methods[name] {
      .Some(entries) => {
        items = .concat(filter_method_entries(env, entries, ty))
      },
      .None => {},
    },
    .None => {},
  }

  items
}

fn filter_method_entries(env: ResolvedEnv, entries: Vector<MethodEntry>, ty: MonoType) Vector<
  lsp_completion.CompletionItem,
> {
  result: Vector<lsp_completion.CompletionItem> = []

  for e in entries {
    sig := case env.lookup_registered_function(e.function_name) {
      .Some(s) => s,
      .None => continue,
    }

    if !method_return_unifies(sig, ty) {
      continue
    }

    result = .append(
      lsp_completion.CompletionItem.{
        label: e.method_name,
        kind: .Method,
        detail: .Some(sig_to_detail(env, sig)),
        documentation: sig.doc,
      },
    )
  }

  result
}
```

- [ ] **Step 4: Wire methods into the handler**

In `rebind_continuation_completions`, replace the final line:

```tw
  // Variants of the target type construct it directly.
  variants_for_type(typed.env, ty)
```

with:

```tw
  // Methods whose return unifies with the target type, plus variants of the
  // target type (which construct it directly).
  items := methods_returning(typed.env, ty)
  items.concat(variants_for_type(typed.env, ty))
```

- [ ] **Step 5: Run tests**

Run: `target/twk fmt boot/compiler/query/completion.tw && target/twk lint boot/compiler/query/completion.tw && make boot-test`
Expected: new test passes; enum-variant test from Task 3 still passes; full suite green.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/query/completion.tw boot/tests/suites/lsp_completion_suite.tw
git commit -m "lsp: filter rebind continuation methods by return type"
```

---

### Task 6: Field-path target + typed-binding guard tests

The code for both already exists (field path in Task 3, typed-binding guard in Task 2); this task adds the regression tests that lock the behavior.

**Files:**
- Test: `boot/tests/suites/lsp_completion_suite.tw`

- [ ] **Step 1: Write the field-path test**

```tw
    .test(
      "rebind continuation on field path filters by field type",
      fn() {
        valid := "type S = .{ items: Vector<Int> }\ns := S.{ items: [1] }\ns.items\n"
        incomplete := "type S = .{ items: Vector<Int> }\ns := S.{ items: [1] }\ns.items = .\n"
        step := open_then_complete(valid, incomplete, 2, 11)
        labels := try completion_labels(step)

        if !has_label(labels, "append") {
          return .Err("expected 'append' for Vector<Int> field, got: ${labels}")
        }

        if has_label(labels, "len") {
          return .Err("did not expect 'len', got: ${labels}")
        }

        .Ok({})
      },
    )
```

Note: char index 11 is the column just after the dot in `s.items = .` (`s.items = .` → `.` is at column 10, cursor after it at 11).

- [ ] **Step 2: Write the typed-binding guard test**

```tw
    .test(
      "typed binding does not offer methods",
      fn() {
        valid := "xs: Vector<Int> = [1]\nxs\n"
        incomplete := "xs: Vector<Int> = [1]\nxs: Vector<Int> = .\n"
        step := open_then_complete(valid, incomplete, 1, 18)
        labels := try completion_labels(step)

        if has_label(labels, "append") or has_label(labels, "len") {
          return .Err("typed binding must not offer methods, got: ${labels}")
        }

        .Ok({})
      },
    )
```

Note: in `xs: Vector<Int> = .` the `.` is the final char; column 18 places the cursor just after it.

- [ ] **Step 3: Run tests**

Run: `make boot-test`
Expected: both new tests pass; full suite green. If a column index is off, adjust `line`/`character` to land immediately after the `.` (the harness mirrors a real editor cursor).

- [ ] **Step 4: Commit**

```bash
git add boot/tests/suites/lsp_completion_suite.tw
git commit -m "lsp: cover field-path rebind and typed-binding guard"
```

---

### Task 7: Self-host verification and plan close-out

**Files:**
- Modify: `docs/plans/README.md` (status flip), this doc, `docs/plans/lsp-completion-rebind-continuation.md` → `docs/plans/archive/`

- [ ] **Step 1: Full self-host + test loop**

Run: `make stage2 && make boot-test`
Expected: stage2 rebuilds `target/boot.wasm` (the compiler still compiles itself with the changes), and the full boot suite is green.

- [ ] **Step 2: Optional live-server smoke test**

Run: `make bundle-cli`
Then exercise an editor against `target/twk` if available, typing `xs = .` on a `Vector<Int>` and confirming `append`/`map` appear and `len` does not. (Manual; skip if no editor harness.)

- [ ] **Step 3: Archive the plan**

Per repo convention, move this doc to `docs/plans/archive/` and remove its row from `docs/plans/README.md` (delete the row, do not mark Done).

```bash
git mv docs/plans/lsp-completion-rebind-continuation.md docs/plans/archive/lsp-completion-rebind-continuation.md
```

Edit `docs/plans/README.md` to delete the `LSP completion continuation` row.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "docs: archive LSP completion continuation plan"
```

## Self-review notes

- **Spec coverage:** classification (Task 2), expected-type resolution incl. field path (Task 3), unify predicate (Task 4), method filtering + variants (Task 5), typed-binding guard (Tasks 2/6), fallback-on-unresolved (Task 3 returns `all_variant_completions`), fields-excluded (never gathered). All covered.
- **Type consistency:** `RebindTarget`/`RebindInfo` defined in Task 1 and imported/used identically in Task 3; `method_return_unifies`/`bind_vars`/`apply_subst`/`types_match_wildcard` defined in Task 4 and only consumed in Task 5; `methods_returning`/`filter_method_entries` defined and wired in Task 5.
- **Deferred (YAGNI):** precise variants-of-`T` for typed bindings (v1 leaves them to existing variant completion, guaranteeing only "no methods"); broader expected-type filtering at non-`= .` positions.
