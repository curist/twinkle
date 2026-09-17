# First-Class Tuples — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add first-class tuple syntax — `(a, b)` literals, `(A, B)` types, `._0` access — for arity 2–4, backed by compiler-known prelude records `Tuple2/3/4`, in both the boot compiler and the Rust stage0 bootstrap compiler; then delete and migrate `@std.tuple`.

**Architecture:** Tuples are ordinary generic prelude records (`Tuple2<A,B>` with fields `_0.._3`) — no new structural type kind. `(a, b)` parses to a dedicated `ExprKind.Tuple` node that lowers to a `TupleN` record construction; `(A, B)` parses to `TypeExprKind.Tuple` lowering to `TupleN<…>`. `Eq`/`==` is automatic (record auto-derivation); `Ord`/`Stringify` come from six hand-written generic prelude witness functions. Everything is mirrored in Rust stage0 so `make stage2` reaches a fixed point.

**Design doc:** [tuples.md](tuples.md). Read it first — this plan implements it.

## Global Constraints

- **Two compilers stay in lockstep.** Any `.tw` syntax that appears in stage0-compiled source (the prelude, `boot/compiler/*.tw`, `boot/main.tw`) must be parseable by **both** the boot compiler and Rust stage0 (`src/`), or `make stage2` fails. The prelude records/witnesses are therefore written **sugar-free** (explicit `Tuple2<A,B>` / `Tuple2.{ _0: …, _1: … }` / `._0`) until stage0 gains sugar.
- **Arity is 2–4 only.** `Tuple2`, `Tuple3`, `Tuple4`. No `Tuple1`, no unit `()`, nothing above 4.
- **Access is `._0`.._3** (0-indexed), never `.0`. Fields are literally named `_0.._3`.
- **`(a)` stays grouping; `()` stays a parse error.** Only a top-level comma inside `( )` makes a tuple.
- **Compiler source stays tuple-free.** Do not use tuple *sugar* inside `boot/compiler/*.tw` in this plan — it waits for destructuring (a later effort). Prelude/stdlib/examples/tests may use it once sugar lands.
- **Prelude changes need the full rebuild.** After editing `boot/prelude/*.tw`: `python3 tools/generate_core_lib.py` then `make bundle-cli` (NOT `quick-bundle-cli`). After editing `src/` (stage0) or any `.tw` the bootstrap compiles: `make stage2` (or `make bundle-cli`) and confirm a fixed point.
- **Format + lint** every edited `.tw`: `target/twk fmt <file>`; `target/twk lint boot/main.tw`.
- **Verification gates:** `make boot-test` (boot suite → `Ran N tests: N passed`); `cargo test --release` for stage0 changes; `make stage2` fixed point (`stage3 == stage4`) for anything the bootstrap compiles.
- **Range literals** `0..n` over `range(n)`; **self-rebind sugar** (`acc = acc + x`), no numbered copies. Commit messages: imperative subject, what/why/how, no metrics.
- **Never run `tree-sitter test`** — hand it to the human (project rule).

## File Map

Boot compiler (Twinkle):
- `boot/compiler/ast.tw` — `ExprKind` (line ~153), `TypeExprKind` (line ~94): add `Tuple` variants.
- `boot/compiler/parser.tw` — `.LParen` expression primary (~2152), array-literal `.LBracket` template (~2169), `parse_type_expr_base` (~792): add comma-loop / `(` type case.
- `boot/compiler/fmt/printer.tw` — print `.Tuple` nodes.
- `boot/compiler/lint.tw`, `checker.tw`, `lower_core/*.tw`, and any pass matching `ExprKind`/`TypeExprKind` without `_` — add `.Tuple` arms (exhaustiveness will flag them).
- `boot/compiler/resolver.tw` — register `TupleN` for unqualified resolution if needed (like `Option`/`Cell`).
- `boot/prelude/tuple.tw` + `boot/prelude/tuple/tuple3.tw` + `boot/prelude/tuple/tuple4.tw` (new) — `Tuple2/3/4` records + per-arity witnesses (mirrors `boot/stdlib/tuple.tw` + `tuple/triple.tw`).
- `boot/prelude/vector.tw` — `Vector.pop`.

Stage0 (Rust):
- `src/syntax/ast.rs` — `Expr`/`TypeExpr` enums: add tuple nodes.
- `src/syntax/parser.rs` — `(` expression primary + type parser: comma-loop.
- `src/syntax/pretty.rs` — print tuple nodes (parity with boot fmt is not required, but keep it valid).
- `src/types/resolve.rs`, `src/types/check.rs` — resolve `TupleN`, lower tuple syntax to the record shape.
- stage0 lowering to Core IR — map tuple literal to `TupleN` record construction (mirror boot).

Migration / tooling:
- `boot/stdlib/tuple.tw` (+ `boot/stdlib/tuple/` submodule) — delete.
- `boot/stdlib/regexp/parse.tw`, `boot/tests/suites/*`, `boot/repros/*`, `examples/leetcode/problems/*` — migrate.
- `docs/grammar.ebnf`, `tree-sitter-twinkle/grammar.js`, `docs/API.md`.

---

## Task 1: `Tuple2/3/4` records + witnesses (sugar-free, both compilers green)

Lands the type + contracts with **zero parser change**, using only ordinary generic-record syntax that boot and stage0 already compile. This alone makes `Tuple2.{ _0: 1, _1: 2 }` usable, `==`-able, orderable, and printable.

**Structure — mirror the proven `@std.tuple` layout.** `@std.tuple` puts `Pair` + its `to_string` in `tuple.tw` and `Triple` + its `to_string` in a `tuple/triple.tw` submodule, because a type's inherent methods must live in the type's defining module and two same-named `to_string` cannot share one module scope. Follow the same shape: one module per arity, each owning its `compare`/`to_string`, transparently re-exported into the top prelude tuple module.

**Note on precedent:** `@std.tuple` provides `to_string` (`Stringify`) for `Pair`/`Triple` but **no `compare` (`Ord`) witness** — so the `to_string` half has an exact template to copy, while the `compare` half is new code (the resolution *mechanism*, generic `prove_contract_method` lookup, is the same, but there is nothing to copy line-for-line). Do not assume a `compare` precedent exists.

**Two investigations before writing code (the design's flagged unknowns), plus a reserved-name guard:**
1. **Global resolvability.** For `(a, b)` sugar to resolve to `Tuple2` with no import, the `TupleN` types must resolve unqualified everywhere — like `Cell`/`Range`/`Iterator`/`Order`. Verified by the plan review: those are *not* dedicated `MonoType` variants (unlike `Vector`/`Dict`/`Option`/`Result` at `resolver.tw:2640-2665`); they resolve as ordinary auto-imported prelude named types. So `TupleN` needs **no** `MonoType` variant. **Do** add `Tuple2`/`Tuple3`/`Tuple4` to `is_reserved_type_name` (`resolver.tw:~1317`) to block user shadowing — this is the one required resolver edit.
2. **Per-arity witness resolution.** The per-arity-module + re-export pattern makes `t.compare(u)` / `t.to_string()` resolve via `resolver.tw` `method_source_tid` (~185-201), which follows alias chains — the same mechanism `@std.tuple`'s re-exported `Triple.to_string` uses. Prelude subdirectories are supported (`generate_core_lib.py` recurses; both loaders filter by `.tw` and resolve relative paths directory-agnostically). This is verified by symmetry with stdlib but **untested for the prelude specifically** — the Tuple2 checkpoint below is where you confirm it for real.

**Files:**
- Create: `boot/prelude/tuple.tw` (Tuple2 + witnesses), `boot/prelude/tuple/tuple3.tw`, `boot/prelude/tuple/tuple4.tw` (Tuple3/Tuple4 + witnesses), re-exported into `tuple.tw` — mirroring `boot/stdlib/tuple.tw` + `boot/stdlib/tuple/triple.tw`
- Modify: resolver registration for `TupleN` if investigation (1) shows it is needed
- Create: `boot/tests/suites/tuple_suite.tw`; register in `boot/tests/main.tw`

**Interfaces:**
- Produces: globally-resolvable types `Tuple2<A,B>`, `Tuple3<A,B,C>`, `Tuple4<A,B,C,D>` (fields `_0.._3`); per-arity `compare`/`to_string` witnesses.

- [ ] **Step 1: Write the failing test**

Create `boot/tests/suites/tuple_suite.tw` (register `tuple_suite.suite()` in `boot/tests/main.tw` alongside the other `api_*` suites):

```tw
use @std.testing.assert as assert
use @std.testing as runner

pub fn suite() runner.Suite {
  runner
    .suite("tuple")
    .test(
      "Tuple2 equality, ordering, and stringify",
      fn() {
        a := Tuple2.{ _0: 1, _1: "x" }
        b := Tuple2.{ _0: 1, _1: "x" }
        c := Tuple2.{ _0: 2, _1: "x" }
        try assert.ok(a == b, "equal tuples")
        try assert.ok(a != c, "unequal tuples")
        try assert.ok(a.compare(c) == .Lt, "lexicographic compare")
        try assert.equal(a.to_string(), "(1, x)")
        try assert.equal("${a}", "(1, x)")
        .Ok({})
      },
    )
    .test(
      "Tuple3 and Tuple4 witnesses",
      fn() {
        t3 := Tuple3.{ _0: 1, _1: 2, _2: 3 }
        t4 := Tuple4.{ _0: 1, _1: 2, _2: 3, _3: 4 }
        try assert.equal(t3.to_string(), "(1, 2, 3)")
        try assert.equal(t4.to_string(), "(1, 2, 3, 4)")
        try assert.ok(t3 == Tuple3.{ _0: 1, _1: 2, _2: 3 }, "t3 eq")
        try assert.ok(t4.compare(Tuple4.{ _0: 1, _1: 2, _2: 3, _3: 5 }) == .Lt, "t4 compare")
        .Ok({})
      },
    )
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `make bundle-cli && make boot-test`
Expected: FAIL — `Tuple2` unresolved.

- [ ] **Step 3: Implement the per-arity prelude modules**

Each arity's type owns its witnesses in its own module (sugar-free so stage0 compiles them as plain generic records). `boot/prelude/tuple/tuple3.tw`:

```tw
pub type Tuple3<A, B, C> = .{ _0: A, _1: B, _2: C }

pub fn compare<A: Ord, B: Ord, C: Ord>(x: Tuple3<A, B, C>, y: Tuple3<A, B, C>) Order {
  case x._0.compare(y._0) {
    .Eq => case x._1.compare(y._1) {
      .Eq => x._2.compare(y._2),
      other => other,
    },
    other => other,
  }
}

pub fn to_string<A: Stringify, B: Stringify, C: Stringify>(t: Tuple3<A, B, C>) String {
  "(${t._0}, ${t._1}, ${t._2})"
}
```

`boot/prelude/tuple/tuple4.tw` follows the same shape with a fourth field.

**Do `Tuple2` first as a checkpoint (the design's Tuple2-only spike).** Land only `boot/prelude/tuple.tw` with `Tuple2` + its witnesses (no submodules yet), add it to `is_reserved_type_name`, and run Step 4's verification — this confirms the prelude-record resolution, the witness resolution, and the bootstrap all work on one arity **before** you replicate into the `tuple3`/`tuple4` submodules and confirm the subdirectory re-export path. If the prelude-subdirectory or witness pattern has a wrinkle, you find it at 1× cost, not 3×.

`boot/prelude/tuple.tw` holds `Tuple2` and (once the checkpoint passes) re-exports the others (mirror `boot/stdlib/tuple.tw`'s `use .tuple.triple as triple_mod` + `pub type Triple<…> = triple_mod.Triple<…>`):

```tw
use .tuple.tuple3 as tuple3_mod
use .tuple.tuple4 as tuple4_mod

pub type Tuple2<A, B> = .{ _0: A, _1: B }
pub type Tuple3<A, B, C> = tuple3_mod.Tuple3<A, B, C>
pub type Tuple4<A, B, C, D> = tuple4_mod.Tuple4<A, B, C, D>

pub fn compare<A: Ord, B: Ord>(x: Tuple2<A, B>, y: Tuple2<A, B>) Order {
  case x._0.compare(y._0) {
    .Eq => x._1.compare(y._1),
    other => other,
  }
}

pub fn to_string<A: Stringify, B: Stringify>(t: Tuple2<A, B>) String {
  "(${t._0}, ${t._1})"
}
```

Add `Tuple2`/`Tuple3`/`Tuple4` to `is_reserved_type_name` (`resolver.tw`). No `MonoType` variant is needed (they resolve as ordinary prelude named types, like `Cell`/`Order`). `==` needs nothing — record `Eq` auto-derivation covers it. Confirm each arity's `compare`/`to_string` resolves as an inherent method at the checkpoint.

- [ ] **Step 4: Verify witnesses resolve and bootstrap stays green**

Run: `target/twk fmt boot/prelude/tuple.tw && python3 tools/generate_core_lib.py && make bundle-cli && make boot-test`
Expected: PASS (tuple suite green). Then `make stage2` → confirm `stage3 == stage4` (stage0 compiled the new prelude records unchanged).

- [ ] **Step 5: Commit**

```bash
git add boot/prelude/tuple.tw boot/tests/suites/tuple_suite.tw boot/tests/main.tw
git commit -m "feat(tuple): add Tuple2/3/4 records and Ord/Stringify witnesses"
```

---

## Task 2: Boot — `(a, b)` literal parsing → `ExprKind.Tuple` → `TupleN` lowering

**Files:**
- Modify: `boot/compiler/ast.tw` (`ExprKind`), `boot/compiler/parser.tw` (`.LParen` primary), `boot/compiler/lower_core.tw` + `lower_core/records.tw`, `boot/compiler/checker.tw`, plus every pass matching `ExprKind` — `fmt/printer.tw`, `lint.tw`, `summary.tw`, `cfg.tw`, and **`boot/compiler/query/*.tw`** (`occurrence_build.tw`, `ast_walk.tw`, `ast_path.tw`, `definition.tw`, `folding_range.tw`, `signature_help.tw` — several are exhaustive and were omitted from the first draft).
- Test: `boot/tests/suites/tuple_suite.tw` (add literal tests)

**Interfaces:**
- Consumes: `Tuple2/3/4` from Task 1.
- Produces: `ExprKind.Tuple(Vector<Expr>)`; parsing `(a, b, …)` (2–4 elems) yields it; lowering emits `TupleN` record construction. `t._0` access works (field access, already supported).

- [ ] **Step 1: Write the failing test**

Add to `boot/tests/suites/tuple_suite.tw`:

```tw
    .test(
      "tuple literal constructs and accesses via ._n",
      fn() {
        t := (1, "x")
        try assert.equal(t._0, 1)
        try assert.equal(t._1, "x")
        try assert.equal((1, 2, 3)._2, 3)
        try assert.equal((1, 2, 3, 4)._3, 4)
        try assert.equal("${(1, 2)}", "(1, 2)")
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify it fails**

Run: `make bundle-cli && make boot-test`
Expected: FAIL — `(1, "x")` is a parse error ("expected ')'").

- [ ] **Step 3: Add the AST variant**

In `boot/compiler/ast.tw`, add to `ExprKind` (after `Array(Vector<Expr>)`):

```tw
  Tuple(Vector<Expr>),
```

- [ ] **Step 4: Parse the literal**

In `boot/compiler/parser.tw`, extend the `.LParen` primary case (~2152). After parsing the first inner expression, if the next token is `.Comma`, loop collecting elements (mirror the array-literal `.LBracket` case at ~2169, but close on `.RParen` and error message "expected ',' or ')' in tuple"). Build `Expr.{ kind: .Tuple(elems), … }` when there is at least one comma; keep the existing single-expression grouping path when there is no comma. Reject arity > 4 with a diagnostic ("tuples support 2–4 elements; use a record").

- [ ] **Step 5: Thread `.Tuple` through the matches — including the non-exhaustive ones**

Build (`make bundle-cli`) and let exhaustiveness errors guide you for the **exhaustive** dispatchers (`checker.tw` `synth_expr` ~2549, fmt, lint recursion). But **two consequential dispatchers end in a `_ =>` wildcard and will NOT be flagged** — you must add `.Tuple` arms to them by hand or get silently wrong behavior:

- **`checker.tw` `check_expr` (~2932, wildcard `_ =>` synth+unify fallback at ~3016).** This is where expected-type pushdown lives (e.g. `.Array` pushes element type for `Byte`-literal coercion at ~2980). Without a `.Tuple` arm, `t: (Byte, Byte) = (1, 2)` and nested-literal elements type-check wrong with no diagnostic. Add a `.Tuple` arm that pushes the expected `TupleN`'s element types into each element (model it on the `.Array` arm).
- **`lower_core.tw` `lower_expr` (~46, wildcard `_ => emit_error("unsupported expression kind")`).** A missing arm here traps at runtime, caught only by the Task-2 test. Add the arm.

For lowering, reuse the existing named-record machinery directly: **`lower_core/records.tw` `lower_named_record` / `lower_record_fields` (~19-58)** already take a type-name string + `Vector<RecordEntry>` — build `RecordEntry`s `_0.._n` and call it with `"Tuple2"`/`"Tuple3"`/`"Tuple4"` by element count. For the checker synth arm, **`checker.tw` `synth_named_record` (~2721) + `check_record_lit` (~2771)** are the near-exact template (fresh metas per type param → `MonoType.Named(entry.id, args)` → `check_record_lit` → bounds check).

- [ ] **Step 6: Run to verify it passes**

Run: `target/twk fmt boot/compiler/ast.tw boot/compiler/parser.tw && make bundle-cli && make boot-test`
Expected: PASS (new literal test green). `make stage2` still reaches a fixed point (boot's own source contains no tuple *syntax* yet — only the new enum variant, which stage0 handles as data).

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/ boot/tests/suites/tuple_suite.tw
git commit -m "feat(tuple): parse (a, b) literals to ExprKind.Tuple in boot"
```

---

## Task 3: Boot — `(A, B)` type parsing → `TypeExprKind.Tuple` → `TupleN<…>`

**Files:**
- Modify: `boot/compiler/ast.tw` (`TypeExprKind`), `boot/compiler/parser.tw` (`parse_type_expr_base`), type resolution, and exhaustive `TypeExprKind` matches.
- Test: `boot/tests/suites/tuple_suite.tw`

**Interfaces:**
- Produces: `TypeExprKind.Tuple(Vector<TypeExpr>)`; `(A, B)` in a type position resolves to `TupleN<A, B>`.

- [ ] **Step 1: Write the failing test**

```tw
    .test(
      "tuple type annotation resolves, including optional and nested",
      fn() {
        p: (Int, String) = (7, "z")
        try assert.equal(p._0, 7)
        make := fn(a: Int, b: Int) (Int, Int) { (a, b) }
        try assert.equal(make(3, 4)._1, 4)
        // (A, B)? must parse as Option<Tuple2<A,B>> — load-bearing for Vector.pop
        maybe: (Int, Int)? = .Some((1, 2))
        try assert.equal(maybe.unwrap()._1, 2)
        // (A, B)!E must parse as Result<Tuple2<A,B>, E>
        res: (Int, String)!String = .Ok((5, "q"))
        try assert.equal(res.unwrap()._0, 5)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify it fails**

Run: `make bundle-cli && make boot-test` → FAIL (`(Int, String)` type unparsed).

- [ ] **Step 3: AST + parse**

Add `Tuple(Vector<TypeExpr>)` to `TypeExprKind` in `ast.tw`. In `parse_type_expr_base`, add a `.LParen` case: parse a comma-separated type list, require 2–4, build `.Tuple(elems)`. (There is no existing `(` type case, so nothing conflicts; `fn(...)` types are handled under the `fn` keyword path.)

**Both type postfixes must wrap the tuple** — verified against the parser:
- `?` (Optional) is applied **per-branch** inside `parse_type_expr_base` (e.g. lines 916–919 for the Path branch — there is no shared tail). So the new tuple branch must append its **own** trailing-`.Question` check, copying the 916–919 pattern, or `(A, B)?` won't become `Option<Tuple2<…>>` (it must not become `Tuple2<A, B?>`).
- `!` / `!E` (Result) is handled by the outer `parse_type_expr` wrapper (762–789) *after* calling the base, so it wraps the tuple automatically — `(A, B)!E` → `Result<Tuple2<…>, E>` needs no tuple-specific code.

Both are asserted in the Step 1 test. `Vector.pop` (Task 7, `(T, Vector<T>)?`) depends on the `?` case. The stage0 mirror (Task 5) must preserve both: put the tuple desugar in the base type parse so the existing `?`/`!` postfix logic wraps the resulting `Type::Named("TupleN", …)`.

- [ ] **Step 4: Resolve + thread**

Resolve `TypeExprKind.Tuple([t0, t1, …])` to `TupleN<…>` (the `Tuple2/3/4` type by arity) — mirror how `TypeExprKind.Optional`/`Result` resolve to `Option`/`Result`. Add `.Tuple` arms to exhaustive `TypeExprKind` matches (exhaustiveness will flag them).

- [ ] **Step 5: Verify**

Run: `target/twk fmt boot/compiler/... && make bundle-cli && make boot-test` → PASS. `make stage2` fixed point holds.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/ boot/tests/suites/tuple_suite.tw
git commit -m "feat(tuple): parse (A, B) tuple types in boot"
```

---

## Task 4: Boot — formatter prints `.Tuple` and round-trips

**Files:**
- Modify: `boot/compiler/fmt/printer.tw`
- Test: `boot/tests/suites/fmt_suite.tw` (or the tuple suite)

**Interfaces:**
- Produces: `fmt` renders `ExprKind.Tuple`/`TypeExprKind.Tuple` back as `(a, b)` / `(A, B)`; idempotent.

- [ ] **Step 1: Write the failing test**

Add an fmt round-trip assertion (follow existing `fmt_suite.tw` patterns): formatting `t := (1, 2)` and a `(Int, String)` annotation yields exactly `(1, 2)` / `(Int, String)`, and formatting twice is stable.

- [ ] **Step 2: Run to verify it fails / renders wrong**

Run: `make bundle-cli && make boot-test` → FAIL (printer emits placeholder or errors on `.Tuple`).

- [ ] **Step 3: Implement printing**

In `fmt/printer.tw`, print `.Tuple(xs)` as `"(" + xs.map(print).join(", ") + ")"` for both the expr and type printers (mirror the `.Array` / `.Result` sugar-preserving cases).

- [ ] **Step 4: Verify idempotence**

Run: `target/twk fmt boot/compiler/fmt/printer.tw && make bundle-cli && make boot-test`; also `target/twk fmt <a scratch .tw with tuples>` twice and diff — no change on the second run.

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/fmt/printer.tw boot/tests/suites/
git commit -m "feat(tuple): format tuple literals and types, idempotent round-trip"
```

---

## Task 5: Stage0 (Rust) tuple parity — parse-time desugar

Give stage0 enough tuple support that `make stage2` reaches a fixed point with tuple syntax in stage0-compiled source (the prelude witness modules from Task 1 are sugar-free, but `Vector.pop`'s `(T, Vector<T>)?` signature in Task 7 forces stage0 to parse tuple *types*). **Do NOT mirror boot's dedicated-AST-node approach** — stage0 has no fmt-idempotence requirement (Global Constraints: happy path only), and it already has the cheaper precedent:

- stage0 desugars `T?` and `!E`/`T!E` **at parse time** into `Type::Named { name: "Option"/"Result", args }` with **no** dedicated `Type` variant (`src/syntax/parser.rs:2354-2400`, `parse_type`). Do the same for tuples: `(A, B)` → `Type::Named { name: "Tuple2", args: [A, B] }`.
- `ExprKind::RecordLit { name: Option<String>, fields: Vec<(String, Expr)> }` already exists (`src/syntax/ast.rs:222-225`) and is handled by every downstream pass (`check.rs`, `ir/lower.rs`, `monomorphize.rs`, `dce.rs`). Desugar `(a, b)` → `RecordLit { name: Some("Tuple2"), fields: [("_0", a), ("_1", b)] }` at parse time.

This touches essentially just `src/syntax/parser.rs` (no new AST variants, no ~12-file thread). stage0's Wasm need not byte-match boot's — the fixed point is compared between boot-compiled stages (`stage3 == stage4`), so stage0 only needs to compile the tuple-bearing prelude correctly.

**Files:**
- Modify: `src/syntax/parser.rs` (expression `(` primary + `parse_type` `(` case, both desugaring). Add `Tuple2/3/4` to stage0's reserved-type-name set if it maintains one (parallel to boot's `is_reserved_type_name`).
- Test: `cargo test --release` (parser unit tests near existing ones); `make stage2`.

**Interfaces:**
- Produces: stage0 parses `(a, b)` → `RecordLit("Tuple2"|"Tuple3"|"Tuple4", _0.._n)` and `(A, B)` → `Type::Named("TupleN", …)`, arity 2–4.

- [ ] **Step 1: Write failing Rust tests**

Add unit tests in `src/syntax/mod.rs` asserting `(1, 2)` parses to `RecordLit{name: Some("Tuple2"), …}`, `(Int, String)` to `Type::Named{name: "Tuple2", …}`, `(Int, String)?` to `Type::Named{name: "Option", args: [Tuple2 …]}` (load-bearing for `Vector.pop`), and `(Int, String)!String` to `Type::Named{name: "Result", args: [Tuple2 …, String]}`. Run `cargo test --release <names>` → FAIL.

- [ ] **Step 2: Desugar in the parser**

In `src/syntax/parser.rs`: extend the `(` expression primary with a comma-loop building a `RecordLit` with `_0.._n` field names; add a `(` case to `parse_type` building `Type::Named("TupleN", args)`, ensuring the existing `T?` postfix wraps it (so `(A, B)?` = `Option<Tuple2<…>>`). Enforce arity 2–4 with an error.

- [ ] **Step 3: Verify parity**

Run: `cargo test --release` (green) then `make stage2`. Expected: `Fixed point reached: stage3 == stage4`. This is the acceptance gate.

- [ ] **Step 4: Commit**

```bash
git add src/
git commit -m "feat(tuple): stage0 tuple parity via parse-time desugar to RecordLit/Named"
```

---

## Task 6: Migrate and delete `@std.tuple`

**Files:**
- Delete: `boot/stdlib/tuple.tw`, `boot/stdlib/tuple/` submodule
- Modify: `boot/stdlib/regexp/parse.tw`, `boot/tests/suites/stdlib_tuple_suite.tw` (fold useful cases into `tuple_suite.tw`, then delete), `boot/tests/suites/lsp_semantic_tokens_suite.tw`, `boot/repros/*`, `examples/leetcode/problems/*` (the four using `@std.tuple`)

**Interfaces:** none new — behavior-preserving rewrite.

- [ ] **Step 1: Find every site**

Run: `grep -rn 'use @std.tuple\|tuple.pair\|tuple.triple\|Pair<\|Triple<\|\.first\b\|\.second\b\|\.third\b' boot/ examples/ | grep -v 'boot/stdlib/tuple'`
Record the list; note that `Pair<`/`.first` may also match unrelated records — inspect each.

- [ ] **Step 2: Rewrite each site**

`tuple.pair(a, b)` → `(a, b)`; `Pair<A,B>` → `(A, B)`; `.first/.second/.third` → `._0/._1/._2`; drop `.swap()` (rewrite `p.swap()` → `(p._1, p._0)`); remove `use @std.tuple` lines. Delete `boot/stdlib/tuple.tw` and the submodule.

- [ ] **Step 3: Regenerate + verify**

Run: `python3 tools/generate_core_lib.py && make bundle-cli && make boot-test && make stage2`
Expected: all green; fixed point holds; no remaining reference to `@std.tuple` (`grep -rn '@std.tuple' boot/ examples/` is empty except this plan/design docs).

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor(tuple): migrate @std.tuple call sites to tuple syntax and delete the library"
```

---

## Task 7: Payoff API — `Vector.pop`

**Files:**
- Modify: `boot/prelude/vector.tw`
- Test: `boot/tests/suites/api_vector_suite.tw`

**Interfaces:**
- Produces: `Vector.pop(xs: Vector<T>) (T, Vector<T>)?` — `.Some((last, rest))` or `.None` when empty. (Uses tuple *type* syntax in a prelude signature, which is why it lands after stage0 parity.)

- [ ] **Step 1: Write the failing test**

```tw
    .test(
      "pop returns the last element and the rest",
      fn() {
        empty: Vector<Int> = []
        try assert.equal(empty.pop(), .None)
        case [1, 2, 3].pop() {
          .Some(p) => {
            try assert.equal(p._0, 3)
            try assert.equal(p._1, [1, 2])
          },
          .None => return assert.fail("expected Some"),
        }
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify it fails**

Run: `make bundle-cli && make boot-test` → FAIL (`pop` unresolved).

- [ ] **Step 3: Implement**

Add to `boot/prelude/vector.tw`:

```tw
/// The last element and the vector without it, or `.None` when empty.
pub fn pop<T>(xs: Vector<T>) (T, Vector<T>)? {
  if xs.is_empty() {
    .None
  } else {
    .Some((xs[xs.len() - 1], xs.drop_last()))
  }
}
```

- [ ] **Step 4: Verify (both compilers)**

Run: `target/twk fmt boot/prelude/vector.tw && python3 tools/generate_core_lib.py && make bundle-cli && make boot-test && make stage2`
Expected: PASS; fixed point holds (stage0 parses the tuple-typed prelude signature).

- [ ] **Step 5: Commit**

```bash
git add boot/prelude/vector.tw boot/tests/suites/api_vector_suite.tw
git commit -m "feat(vector): add Vector.pop returning (last, rest) via tuples"
```

---

## Task 8: Grammar docs + tree-sitter

**Files:**
- Modify: `docs/grammar.ebnf`, `tree-sitter-twinkle/grammar.js`, regenerated `tree-sitter-twinkle/src/*`, `tree-sitter-twinkle/tree-sitter-twinkle.wasm`

**Interfaces:** none (tooling).

- [ ] **Step 1: Update the EBNF**

In `docs/grammar.ebnf`: add tuple literal to `PrimaryExpr` (`| "(" Expr "," Expr { "," Expr } ")"`) and a tuple type production to the type grammar, noting the 2–4 arity bound. Keep `"(" Expr ")"` grouping.

- [ ] **Step 2: Update tree-sitter grammar**

Edit `tree-sitter-twinkle/grammar.js` to add tuple literal and tuple type rules (parenthesized comma-separated, disambiguated from grouping by the comma). Then:

```bash
cd tree-sitter-twinkle && npx tree-sitter generate && npx tree-sitter build --wasm
```

- [ ] **Step 3: Hand tests to the human**

Do **not** run `tree-sitter test`. Ask the human to run it and confirm the corpus passes. Commit `grammar.js`, regenerated `src/`, and the wasm together once confirmed.

- [ ] **Step 4: Commit**

```bash
git add docs/grammar.ebnf tree-sitter-twinkle/
git commit -m "feat(tuple): grammar.ebnf and tree-sitter rules for tuple syntax"
```

---

## Task 9: Documentation

**Files:**
- Modify: `docs/API.md`

**Interfaces:** none.

- [ ] **Step 1: Replace the `@std.tuple` section**

Replace the `### @std.tuple` section with a tuples section: `(a, b)` … `(a, b, c, d)` literals, `(A, B)` … types, `._0.._3` access; note `==`/`compare`/`to_string` are provided; document the arity-4 cap and the **"name it when the fields have meaningful names — use a record for coordinates/colors/parser state"** guidance. Add `Vector.pop` to the `Vector<T>` table. Remove `@std.tuple` from any module lists.

- [ ] **Step 2: Verify no stale references**

Run: `grep -rn '@std.tuple\|tuple.pair\|Pair<' docs/API.md` → empty (aside from none).

- [ ] **Step 3: Commit**

```bash
git add docs/API.md
git commit -m "docs(tuple): document tuple syntax and Vector.pop; drop @std.tuple"
```

---

## Finalization

- [ ] Full gate: `make bundle-cli && make boot-test` (green), `cargo test --release` (green), `make stage2` (`stage3 == stage4`).
- [ ] `target/twk lint boot/main.tw` — resolve/justify findings on edited files.
- [ ] Confirm `@std.tuple` is gone everywhere except the design/plan docs: `grep -rn '@std.tuple' boot/ src/ examples/`.
- [ ] Move both `docs/plans/tuples.md` and `docs/plans/tuples-implementation.md` to `docs/plans/archive/` and remove the plan-index row (per the plan lifecycle). Note the deferred **destructuring** fast-follow (its own future plan) in the commit so it is not lost.

## Notes / deferred

- **Destructuring** (`(a, b) :=`, `case (a, b) =>`) is the immediate fast-follow, in both compilers — its own plan. Compiler source (`boot/compiler/*.tw`) adopts tuples only after it lands.
- **Pair-returning stdlib** (`split_once`, `zip`, `partition`) becomes natural once destructuring exists — revisit then.
- **Monomorphization** of `TupleN` is assumed to fall through the existing generic-record path (no special handling). The Task 2 literal test already exercises multiple instantiations (`Tuple2<Int,String>`, `Tuple2<Int,Int>`, `Tuple3`, `Tuple4`) in one function; if a mono issue surfaces, add a generic helper (`fn fst<A,B>(t: (A,B)) A { t._0 }`) used at two instantiations.
- **Linter:** check whether `t._0 = x` rebinding sugar (if legal on a positionally-built record) needs `direct-rebinding`/`record-copy-helper` rule awareness, and whether `lint.tw`'s `ExprKind` walks are exhaustive or wildcarded (Task 2 must cover the walks either way).

**Assumptions verified by plan review (evidence in the reviews):** `Eq`/`==` auto-derives for `TupleN` records; `Ord`/`Stringify` need no `try_builtin_container_contract` entry (resolve via generic `prove_contract_method`); `TupleN` needs no `MonoType` variant; per-arity submodule witness re-export resolves via `method_source_tid`; the boot parser line citations (`.LParen` ~2152, `.LBracket` ~2169, `parse_type_expr_base` ~792) are accurate. The two dispatchers that are **not** exhaustiveness-guarded — `checker.tw` `check_expr` (~2932) and `lower_core.tw` `lower_expr` (~46) — are called out in Task 2 Step 5.
