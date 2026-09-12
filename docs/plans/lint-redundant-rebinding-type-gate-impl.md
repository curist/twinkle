# Type-aware `redundant-rebinding` Auto-Fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the `redundant-rebinding` computed-advance auto-fix (`acc2 := step(acc)` → `acc = step(acc)`) safe for type-inferred (`:=`) bindings by proving `typeof(acc2) == typeof(acc)` from the checker's inferred types instead of byte-identical source annotations.

**Architecture:** The checker already publishes `CheckResult.type_map: Dict<Int, MonoType>` (keyed by `Expr.id`, zonked at finalize). Thread it into `lint_module` alongside the `env` the linter already receives, and replace the syntactic `ann_ok` gate in `redundant_rebinding_edits` with a `mono_eq` comparison of the temp's and base's result types, failing closed on missing/error/meta types.

**Tech Stack:** Twinkle boot compiler (`boot/*.tw`). `target/twk test` compiles the suite from current source, so edits to `boot/compiler/*.tw` are exercised without rebundling. Heavy verification via `make stage2` / `make boot-test`.

Design spec: [lint-redundant-rebinding-type-gate.md](lint-redundant-rebinding-type-gate.md).

## Global Constraints

- Boot-only change. No Rust/stage0 edits expected.
- Analysis-only: the self-host fixed point (`stage3 == stage4`, via `make stage2`) MUST still hold — codegen must not change.
- The rule stays fail-closed: never emit an unproven rewrite. A finding with no safe proof carries empty `edits` (report-only).
- `FixEdit = .{ start: Int, end: Int, replacement: String }` (in `boot/lib/source/report.tw`).
- After editing any `.tw` file, run `target/twk fmt <file>` (idempotent; expect no second-pass churn).
- Commit trailers for this session:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01HWB8aAe5AQWwznH3N7snST
  ```

---

## File Structure

- `boot/compiler/mono_type.tw` — **new home** for the structural `mono_eq`/`mono_vec_eq` and `contains_meta` predicates (beside `ty_to_string`).
- `boot/compiler/backend/verify_common.tw` — drops its local `mono_eq`/`mono_vec_eq` (moved to `mono_type`).
- `boot/compiler/backend/verify_expr.tw`, `boot/compiler/backend/verify_slots.tw` — repoint their `mono_eq` import to `compiler.mono_type`.
- `boot/compiler/lint.tw` — thread `type_map` through the redundant-rebinding walk; replace the `ann_ok` gate with the type comparison; delete now-dead `same_source_type`.
- `boot/compiler/query/analyze.tw` — pass `checked.type_map` at the `lint_module` call site.
- `boot/tests/suites/lint_pass_suite.tw` — add a checker-backed `rr_findings_typed` helper; migrate the computed-advance edit tests to type-complete snippets; add the unlock + soundness + fail-closed tests.

**Out of scope (deliberate):** `closure_convert.tw`'s private `mono_eq` copy; `checker.tw`'s private `contains_meta` (14 internal call sites in a hot path — not worth the self-host risk for pure dedup). Both keep their own copies.

---

## Task 1: Relocate `mono_eq` + add `contains_meta` to `compiler.mono_type`

Pure refactor. No behavior change; verified by compiling the whole boot compiler (which runs the backend verifier that uses `mono_eq`) and the existing suites.

**Files:**
- Modify: `boot/compiler/mono_type.tw` (add three functions)
- Modify: `boot/compiler/backend/verify_common.tw` (remove two functions)
- Modify: `boot/compiler/backend/verify_expr.tw:13-14,23` (imports)
- Modify: `boot/compiler/backend/verify_slots.tw:5-6` (imports)

**Interfaces:**
- Produces: `pub fn mono_eq(a: MonoType, b: MonoType) Bool` and `pub fn contains_meta(ty: MonoType) Bool` in `compiler.mono_type`.

- [ ] **Step 1: Add the three predicates to `boot/compiler/mono_type.tw`**

Append after `ty_to_string` (end of file). `mono_vec_eq` stays module-private; `mono_eq` and `contains_meta` are `pub`.

```tw
pub fn mono_eq(a: MonoType, b: MonoType) Bool {
  case a {
    .Int => case b {
      .Int => true,
      _ => false,
    },
    .Float => case b {
      .Float => true,
      _ => false,
    },
    .Bool => case b {
      .Bool => true,
      _ => false,
    },
    .Byte => case b {
      .Byte => true,
      _ => false,
    },
    .String => case b {
      .String => true,
      _ => false,
    },
    .Void => case b {
      .Void => true,
      _ => false,
    },
    .Anyref_ => case b {
      .Anyref_ => true,
      _ => false,
    },
    .Var(name_a) => case b {
      .Var(name_b) => name_a == name_b,
      _ => false,
    },
    .Named(tid_a, args_a) => case b {
      .Named(tid_b, args_b) => tid_a.id == tid_b.id and mono_vec_eq(args_a, args_b),
      _ => false,
    },
    .Vector(inner_a) => case b {
      .Vector(inner_b) => mono_eq(inner_a, inner_b),
      _ => false,
    },
    .Dict(ka, va) => case b {
      .Dict(kb, vb) => mono_eq(ka, kb) and mono_eq(va, vb),
      _ => false,
    },
    .Function(pa, ra) => case b {
      .Function(pb, rb) => mono_vec_eq(pa, pb) and mono_eq(ra, rb),
      _ => false,
    },
    .Optional(inner_a) => case b {
      .Optional(inner_b) => mono_eq(inner_a, inner_b),
      _ => false,
    },
    .ExternRef(tid_a) => case b {
      .ExternRef(tid_b) => tid_a.id == tid_b.id,
      _ => false,
    },
    .Result(ok_a, err_a) => case b {
      .Result(ok_b, err_b) => mono_eq(ok_a, ok_b) and mono_eq(err_a, err_b),
      _ => false,
    },
    .MetaVar(id_a) => case b {
      .MetaVar(id_b) => id_a == id_b,
      _ => false,
    },
    .Never => case b {
      .Never => true,
      _ => false,
    },
    .ErrorType => case b {
      .ErrorType => true,
      _ => false,
    },
  }
}

fn mono_vec_eq(a: Vector<MonoType>, b: Vector<MonoType>) Bool {
  if a.len() != b.len() {
    return false
  }

  for item, i in a {
    if !mono_eq(item, b[i]) {
      return false
    }
  }

  true
}

pub fn contains_meta(ty: MonoType) Bool {
  case ty {
    .MetaVar(_) => true,
    .Vector(inner) => contains_meta(inner),
    .Dict(k, v) => contains_meta(k) or contains_meta(v),
    .Function(params, ret) => {
      for p in params {
        if contains_meta(p) {
          return true
        }
      }

      contains_meta(ret)
    },
    .Optional(inner) => contains_meta(inner),
    .Result(ok, err) => contains_meta(ok) or contains_meta(err),
    .Named(_, args) => {
      for a in args {
        if contains_meta(a) {
          return true
        }
      }

      false
    },
    _ => false,
  }
}
```

- [ ] **Step 2: Remove `mono_eq` + `mono_vec_eq` from `boot/compiler/backend/verify_common.tw`**

Delete the `pub fn mono_eq(...)` block and the `fn mono_vec_eq(...)` block (`mono_vec_eq` is private and only used by that `mono_eq`). Leave every other function (`heap_type_name`, etc.) untouched.

- [ ] **Step 3: Repoint the `mono_eq` import in `boot/compiler/backend/verify_expr.tw`**

Remove `mono_eq,` from the `use compiler.backend.verify_common.{ … }` list (around line 14), and add `mono_eq` to the existing `use compiler.mono_type.{MonoType}` line (around line 23):

```tw
use compiler.mono_type.{MonoType, mono_eq}
```

- [ ] **Step 4: Repoint the `mono_eq` import in `boot/compiler/backend/verify_slots.tw`**

Remove `mono_eq,` from the `use compiler.backend.verify_common.{ … }` list (around line 6). This file has no `compiler.mono_type` import yet — add one near the other `use` lines:

```tw
use compiler.mono_type.{mono_eq}
```

- [ ] **Step 5: Format the touched files**

Run: `target/twk fmt boot/compiler/mono_type.tw boot/compiler/backend/verify_common.tw boot/compiler/backend/verify_expr.tw boot/compiler/backend/verify_slots.tw`

- [ ] **Step 6: Verify the relocation compiles + verifier still works**

Run: `target/twk build boot/main.tw -o /tmp/reloc-check.wasm`
Expected: builds with no error (compiles the whole compiler and runs the backend verifier, which uses `mono_eq`).

- [ ] **Step 7: Run the type/verify-touching suites**

Run: `target/twk test --filter "checker"`
Expected: all pass (0 failures).

- [ ] **Step 8: Commit**

```bash
git add boot/compiler/mono_type.tw boot/compiler/backend/verify_common.tw boot/compiler/backend/verify_expr.tw boot/compiler/backend/verify_slots.tw
git commit -m "refactor(boot): move mono_eq/contains_meta to compiler.mono_type

Relocate the structural MonoType equality (mono_eq/mono_vec_eq) out of
backend/verify_common into compiler.mono_type, its natural home beside
ty_to_string, and add a pub contains_meta there. Repoints the verify_expr /
verify_slots imports; the linter will consume both from mono_type without
importing from backend/. Pure relocation; no behavior change.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HWB8aAe5AQWwznH3N7snST"
```

---

## Task 2: Thread `type_map`, replace the annotation gate, migrate the annotated tests

Changing `lint_module`'s signature immediately breaks the two existing *annotated* edit-asserting tests (they run parse-only and now have no `type_map`). To keep this task green, it also adds the checker-backed helper and migrates those two tests. Task 3 is then purely additive.

**Files:**
- Modify: `boot/compiler/lint.tw` (imports; `LintCtx`; `empty_lint_ctx`; `lint_module`; `lint_block`; `lint_redundant_rebinding`; `shape_one_redundant_finding`; `redundant_rebinding_edits`; delete `same_source_type`)
- Modify: `boot/compiler/query/analyze.tw:960` (pass `checked.type_map`)
- Modify: `boot/tests/suites/lint_pass_suite.tw` (parse-only callers pass empty `type_map`; add `rr_findings_typed`; migrate the two annotated edit tests)

**Interfaces:**
- Consumes: `mono_eq`, `contains_meta`, `MonoType` from `compiler.mono_type` (Task 1); `checker.check(module, env, lint_mode) CheckResult`; `base_env.builtin_env().resolve(module)`.
- Produces: `pub fn lint_module(module: Module, env: ResolvedEnv, type_map: Dict<Int, MonoType>, source: String) Vector<LintFinding>` (new signature every caller matches); `rr_findings_typed(src) Vector<lint.LintFinding>` test helper (consumed by Task 3).

- [ ] **Step 1: Update imports in `boot/compiler/lint.tw`**

Add after `use compiler.resolver.{ResolvedEnv}`:

```tw
use compiler.mono_type.{MonoType, contains_meta, mono_eq}
```

- [ ] **Step 2: Add `type_map` to `LintCtx` and `empty_lint_ctx`**

`LintCtx` (around line 30):

```tw
type LintCtx = .{
  types: Dict<String, String>,
  values: Dict<String, Bool>,
  source: String,
  env: ResolvedEnv,
  type_map: Dict<Int, MonoType>,
}
```

`empty_lint_ctx` (around line 40):

```tw
fn empty_lint_ctx(env: ResolvedEnv, type_map: Dict<Int, MonoType>, source: String) LintCtx {
  .{ types: Dict.new(), values: Dict.new(), source, env, type_map }
}
```

- [ ] **Step 3: Update `lint_module` signature and its two `LintCtx` constructions**

Signature (line 56) and the two ctx builds (lines 59, 69):

```tw
pub fn lint_module(
  module: Module,
  env: ResolvedEnv,
  type_map: Dict<Int, MonoType>,
  source: String,
) Vector<LintFinding> {
  out: Vector<LintFinding> = []

  cur := empty_lint_ctx(env, type_map, source)
```

The per-function-body ctx literal:

```tw
          lint_block(
            decl.body,
            .{ types: param_types(decl), values: param_values(decl), source, env, type_map },
          ),
```

- [ ] **Step 4: Pass `ctx.type_map` into `lint_redundant_rebinding`**

In `lint_block` (around line 423):

```tw
  out = .concat(lint_redundant_rebinding(block, ctx.source, ctx.type_map))
```

- [ ] **Step 5: Thread `type_map` through `lint_redundant_rebinding` and `shape_one_redundant_finding`**

`lint_redundant_rebinding` (line 2058):

```tw
fn lint_redundant_rebinding(
  block: Block,
  source: String,
  type_map: Dict<Int, MonoType>,
) Vector<LintFinding> {
  out: Vector<LintFinding> = []

  for stmt, i in block.stmts {
    case stmt {
      .Let(ls) => {
        case shape_one_redundant_finding(block, i, ls, source, type_map) {
          .Some(finding) => out = .append(finding),
          .None => {},
        }
        case shape_two_redundant_finding(block, i, ls) {
          .Some(finding) => out = .append(finding),
          .None => {},
        }
      },
      _ => {},
    }
  }

  out
}
```

`shape_one_redundant_finding` (line 1890): add the param and forward it:

```tw
fn shape_one_redundant_finding(
  block: Block,
  i: Int,
  ls: LetStmt,
  source: String,
  type_map: Dict<Int, MonoType>,
) LintFinding? {
```

and near the end of that function:

```tw
  edits := redundant_rebinding_edits(ls, base, base_decl, forward, source, type_map)
```

- [ ] **Step 6: Replace the `ann_ok` gate in `redundant_rebinding_edits`**

Replace the function (around lines 1826-1866) with:

```tw
fn redundant_rebinding_edits(
  ls: LetStmt,
  base: String,
  base_decl: LetStmt,
  forward: Vector<Span>,
  source: String,
  type_map: Dict<Int, MonoType>,
) Vector<FixEdit> {
  is_alias := case ls.value.kind {
    .Ident(n) => n == base,
    _ => false,
  }

  binding_edit: FixEdit = if is_alias {
    .{ start: ls.span.start, end: ls.span.end, replacement: "" }
  } else {
    // Type gate: rebinding `base` in place is only safe when the advance's
    // result type equals `base`'s type. Compare the checker's zonked inferred
    // types (keyed by Expr.id) instead of requiring matching source
    // annotations; fail closed when a type is missing, an error, or still
    // holds a meta-var (see docs/plans/lint-redundant-rebinding-type-gate.md).
    temp_ty := case type_map[ls.value.id] {
      .Some(t) => t,
      .None => return [],
    }
    base_ty := case type_map[base_decl.value.id] {
      .Some(t) => t,
      .None => return [],
    }
    if !comparable_mono(temp_ty) or !comparable_mono(base_ty) or !mono_eq(temp_ty, base_ty) {
      return []
    }

    prefix := source.slice(ls.name_span.start, ls.value.span.start)
    if prefix.contains("//") {
      return []
    }

    .{ start: ls.name_span.start, end: ls.value.span.start, replacement: "${base} = " }
  }

  edits: Vector<FixEdit> = [binding_edit]
  for sp in forward {
    edits = .append(FixEdit.{ start: sp.start, end: sp.end, replacement: base })
  }

  edits
}

/// A MonoType is safe to compare for the rebinding gate only when it is fully
/// resolved: not an error sentinel (two `ErrorType`s are `mono_eq` but prove
/// nothing) and free of unresolved meta-vars.
fn comparable_mono(ty: MonoType) Bool {
  case ty {
    .ErrorType => false,
    _ => !contains_meta(ty),
  }
}
```

- [ ] **Step 7: Delete the now-dead `same_source_type`**

Remove `fn same_source_type(a: TypeExpr, b: TypeExpr, source: String) Bool { … }` (around line 1802) — its only caller was the `ann_ok` branch just removed. If `TypeExpr` is now unused in the `use compiler.ast.{…}` import list, remove it from that list too.

- [ ] **Step 8: Update the production call site `boot/compiler/query/analyze.tw:960`**

```tw
    all_findings := lint.lint_module(parsed.module, checked.env, checked.type_map, source.text).concat(checked.lints)
```

- [ ] **Step 9: Update the parse-only test callers in `boot/tests/suites/lint_pass_suite.tw`**

The `findings` helper (line 13):

```tw
fn findings(src: String) Vector<lint.LintFinding> {
  parsed := parser.parse(src, 0)
  lint.lint_module(parsed.value, base_env.builtin_env(), Dict.new(), src)
}
```

The public-temporary test (line 799):

```tw
              fs := lint.lint_module(module, base_env.builtin_env(), Dict.new(), src)
```

- [ ] **Step 10: Add checker/resolver imports + the `rr_findings_typed` helper**

At the top of `boot/tests/suites/lint_pass_suite.tw`, add alongside the existing `use compiler.*` lines:

```tw
use compiler.checker
use compiler.resolver
```

Add beside `rr_findings` (around line 83):

```tw
// Like `rr_findings`, but runs resolve + check first so lint_module receives a
// real (zonked) type_map. Snippets passed here MUST be type-complete — every
// called helper defined — or the computed-advance gate fails closed on the
// resulting ErrorType.
fn rr_findings_typed(src: String) Vector<lint.LintFinding> {
  parsed := parser.parse(src, 0)
  resolved := base_env.builtin_env().resolve(parsed.value)
  checked := checker.check(parsed.value, resolved.env, false)
  all := lint.lint_module(parsed.value, checked.env, checked.type_map, src)

  collect f in all {
    if f.rule == "redundant-rebinding" {
      f
    } else {
      continue
    }
  }
}
```

- [ ] **Step 11: Migrate "rewrites a computed candidate with matching annotations" to the typed helper**

Replace that test's body (around line 563) with a type-complete snippet run through `rr_findings_typed`:

```tw
    .test(
      "redundant-rebinding: rewrites a computed candidate with matching annotations",
      fn() {
        src := "fn step(x: Int) Int {\n  x\n}\nfn consume(x: Int) Int {\n  x\n}\nfn f() Int {\n  acc: Int = 0\n  acc2: Int = step(acc)\n  consume(acc2)\n}\n"
        fs := rr_findings_typed(src)
        try assert.equal(fs.len(), 1)
        try assert.equal(fs[0].edits.len(), 2)
        fixed := lintcmd.apply_edits(src, fs[0].edits)
        try assert.is_false(fixed.contains("acc2"))
        try assert.is_true(fixed.contains("acc = step(acc)"))
        try assert.is_true(fixed.contains("consume(acc)"))
        .Ok({})
      },
    )
```

- [ ] **Step 12: Migrate "renames every forward occurrence" to the typed helper**

Replace that test's body (around line 577):

```tw
    .test(
      "redundant-rebinding: renames every forward occurrence",
      fn() {
        src := "fn step(x: Int) Int {\n  x\n}\nfn combine(a: Int, b: Int) Int {\n  a\n}\nfn f() Int {\n  state: Int = 0\n  state_next: Int = step(state)\n  combine(state_next, state_next)\n}\n"
        fs := rr_findings_typed(src)
        try assert.equal(fs.len(), 1)

        // Binding rewrite plus one edit per forward read.
        try assert.equal(fs[0].edits.len(), 3)
        fixed := lintcmd.apply_edits(src, fs[0].edits)
        try assert.is_false(fixed.contains("state_next"))
        try assert.is_true(fixed.contains("combine(state, state)"))
        .Ok({})
      },
    )
```

- [ ] **Step 13: Format touched files**

Run: `target/twk fmt boot/compiler/lint.tw boot/compiler/query/analyze.tw boot/tests/suites/lint_pass_suite.tw`

- [ ] **Step 14: Run the redundant-rebinding suite (must be GREEN)**

Run: `target/twk test --filter "redundant-rebinding"`
Expected: all pass. The migrated annotated tests now get a real `type_map`; parse-only computed-advance tests take the empty-`type_map` fail-closed path (they already asserted report-only); pure-alias and Shape 2 tests are unaffected. If anything fails, stop and investigate.

- [ ] **Step 15: Commit**

```bash
git add boot/compiler/lint.tw boot/compiler/query/analyze.tw boot/tests/suites/lint_pass_suite.tw
git commit -m "feat(lint): gate redundant-rebinding fix on inferred types

Thread the checker's zonked CheckResult.type_map into lint_module and replace
the byte-identical-annotation gate in redundant_rebinding_edits with a mono_eq
comparison of the temp's and base's inferred result types, failing closed on
missing/error/meta types. Parse-only callers pass an empty type_map. Adds an
rr_findings_typed test helper and migrates the annotated edit tests onto it.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HWB8aAe5AQWwznH3N7snST"
```

---

## Task 3: New type-gated test coverage (additive)

Adds the behavior this change unlocks and its soundness guard; reworks the two "report-only" tests whose intent the type gate changes.

**Files:**
- Modify: `boot/tests/suites/lint_pass_suite.tw`

**Interfaces:**
- Consumes: `rr_findings_typed` and `rr_findings` (Task 2).

- [ ] **Step 1: Add the unlock test (inferred, matching types → fixable)**

```tw
    .test(
      "redundant-rebinding: inferred computed advance with matching types is fixable",
      fn() {
        src := "fn step(x: Int) Int {\n  x\n}\nfn consume(x: Int) Int {\n  x\n}\nfn f() Int {\n  acc := 0\n  acc2 := step(acc)\n  consume(acc2)\n}\n"
        fs := rr_findings_typed(src)
        try assert.equal(fs.len(), 1)

        // Binding rewrite (`acc2 := ` -> `acc = `) plus one forward rename.
        try assert.equal(fs[0].edits.len(), 2)
        fixed := lintcmd.apply_edits(src, fs[0].edits)
        try assert.is_true(fixed.contains("acc = step(acc)"))
        try assert.is_true(fixed.contains("consume(acc)"))
        .Ok({})
      },
    )
```

- [ ] **Step 2: Add the soundness guard (differing types → declines)**

```tw
    .test(
      "redundant-rebinding: a computed advance that changes type declines the fix",
      fn() {
        // `acc2` is a numbered sibling of `acc` and reads it, so the rule fires;
        // but `to_s` returns String while `acc` is Int, so rebinding `acc` would
        // be a type error. The fix must decline (report-only).
        src := "fn to_s(x: Int) String {\n  \"s\"\n}\nfn use_s(x: String) Int {\n  0\n}\nfn f() Int {\n  acc := 0\n  acc2 := to_s(acc)\n  use_s(acc2)\n}\n"
        fs := rr_findings_typed(src)
        try assert.equal(fs.len(), 1)
        try assert.equal(fs[0].edits.len(), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 3: Rework the two "report-only" tests**

Replace "computed unannotated candidates stay report-only" (around line 624) so it documents the **fail-closed-without-types** path on the parse-only helper:

```tw
    .test(
      "redundant-rebinding: declines a computed advance when no type info is available",
      fn() {
        // Parse-only (no checker): the type gate can't prove the rebind is safe,
        // so the finding stays report-only.
        fs := rr_findings(
          "fn f() Int {\n  cache := initial()\n  cache2 := step(cache)\n  consume(cache2)\n}\n",
        )
        try assert.equal(fs.len(), 1)
        try assert.equal(fs[0].edits.len(), 0)
        .Ok({})
      },
    )
```

Delete the "differing annotations stay report-only" test (around line 634-643) — its `Other` type is undefined and its intent (different type declines) is now covered by Step 2's type-complete mismatch test.

- [ ] **Step 4: Migrate "a comment in the binding prefix stays report-only" to the typed helper**

Its point is that the comment guard declines even when types match — so the snippet must type-check. Replace its body (around line 646):

```tw
    .test(
      "redundant-rebinding: a comment in the binding prefix stays report-only",
      fn() {
        src := "fn step(x: Int) Int {\n  x\n}\nfn consume(x: Int) Int {\n  x\n}\nfn f() Int {\n  cache: Int = 0\n  cache2: Int // preserve this\n    = step(cache)\n  consume(cache2)\n}\n"
        fs := rr_findings_typed(src)
        try assert.equal(fs.len(), 1)
        try assert.equal(fs[0].edits.len(), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 5: Run the redundant-rebinding suite (GREEN)**

Run: `target/twk test --filter "redundant-rebinding"`
Expected: all pass, including the new unlock/soundness tests.

- [ ] **Step 6: Format touched file**

Run: `target/twk fmt boot/tests/suites/lint_pass_suite.tw`

- [ ] **Step 7: Commit**

```bash
git add boot/tests/suites/lint_pass_suite.tw
git commit -m "test(lint): cover type-gated redundant-rebinding fix

An inferred computed advance with matching types is now auto-fixable; a
type-changing advance (Int -> String) declines; parse-only is the fail-closed
case. Reworks the former annotation-based report-only tests accordingly.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HWB8aAe5AQWwznH3N7snST"
```

---

## Final Verification

Run sequentially (never concurrently).

- [ ] **Full boot suite:** `make boot-test` → expect `… tests: N passed` (0 failed).
- [ ] **Self-host fixed point:** `make stage2` → expect `Fixed point reached: stage3 == stage4` (analysis-only; codegen must be unchanged).
- [ ] **Rebuild CLI + re-run suite:** `make quick-bundle-cli` then `make boot-test` → confirm the rebuilt compiler is green.
- [ ] **Dogfood:** `target/twk lint boot/main.tw` → confirm no new findings. (Optional) reintroduce one `acc2 := step(acc)` locally to confirm `--fix-redundant-rebinding` now rewrites it, then revert.

---

## Self-Review

- **Spec coverage:** mechanism (type_map reuse) → Task 2; threading touch points → Task 2 Steps 1-9; type gate + fail-closed guards → Task 2 Step 6; `mono_eq` relocation → Task 1; tests (unlock, soundness, annotated regression, fail-closed) → Task 2 Steps 11-12 + Task 3 Steps 1-4; verification → Final Verification. All spec sections covered.
- **Placeholder scan:** none — every step has exact code or an exact command.
- **Type consistency:** `lint_module(module, env, type_map, source)` used identically at all three call sites (analyze.tw:960, findings:13, public-temporary:799); `redundant_rebinding_edits(ls, base, base_decl, forward, source, type_map)` matches its single caller in `shape_one_redundant_finding`; `comparable_mono`/`mono_eq`/`contains_meta` signatures consistent; `rr_findings_typed` returns `Vector<lint.LintFinding>` like `rr_findings`.
- **Green at each task:** Task 1 ends on a clean build + checker suite; Task 2 ends on a green redundant-rebinding suite (annotated tests migrated in-task); Task 3 is purely additive and ends green.
