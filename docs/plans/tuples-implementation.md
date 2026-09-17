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

**Two investigations before writing code (the design's flagged unknowns):**
1. **Global resolvability.** For `(a, b)` sugar to resolve to `Tuple2` with no import, the `TupleN` types must be in scope everywhere — like `Option`/`Result`/`Cell`. Check how those are registered (builtin `TypeId` registration vs prelude auto-import in the resolver, `boot/compiler/resolver.tw`) and register `TupleN` the same way. If they are ordinary auto-imported prelude records that resolve unqualified, no builtin `TypeId` is needed; if they need builtin registration, add it.
2. **Per-arity witness resolution.** Confirm the per-arity-module + re-export pattern makes `t.compare(u)` / `t.to_string()` resolve for each `TupleN` (exactly as `@std.tuple`'s `Triple.to_string` resolves today). If prelude subdirectories are not supported, fall back to distinct top-level prelude files.

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

`boot/prelude/tuple/tuple4.tw` follows the same shape with a fourth field. `boot/prelude/tuple.tw` holds `Tuple2` and re-exports the others (mirror `boot/stdlib/tuple.tw`'s `use .tuple.triple as triple_mod` + `pub type Triple<…> = triple_mod.Triple<…>`):

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

Then apply the two investigations above: ensure `Tuple2/3/4` resolve unqualified everywhere (register like `Option`/`Cell` if needed), and confirm each arity's `compare`/`to_string` resolves as an inherent method. `==` needs nothing — record `Eq` auto-derivation covers it.

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
- Modify: `boot/compiler/ast.tw` (`ExprKind`), `boot/compiler/parser.tw` (`.LParen` primary), lowering (`boot/compiler/lower_core/*.tw`), and every exhaustive `ExprKind` match the compiler flags (`fmt/printer.tw`, `lint.tw`, `checker.tw`, `summary.tw`, `cfg.tw`, etc.)
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

- [ ] **Step 5: Thread `.Tuple` through exhaustive matches + lowering**

Build (`make bundle-cli`) and let exhaustiveness errors guide you: add a `.Tuple(elems)` arm everywhere the compiler flags a non-exhaustive `ExprKind` match. Key ones: lowering (`lower_core/*.tw`) maps `.Tuple(elems)` to a `TupleN` `NamedRecord` construction with fields `_0.._n` (`Tuple2`/`Tuple3`/`Tuple4` by element count); `fmt/printer.tw` is handled in Task 4; `lint.tw`/`checker.tw`/others recurse into the element expressions.

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
      "tuple type annotation resolves",
      fn() {
        p: (Int, String) = (7, "z")
        try assert.equal(p._0, 7)
        make := fn(a: Int, b: Int) (Int, Int) { (a, b) }
        try assert.equal(make(3, 4)._1, 4)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify it fails**

Run: `make bundle-cli && make boot-test` → FAIL (`(Int, String)` type unparsed).

- [ ] **Step 3: AST + parse**

Add `Tuple(Vector<TypeExpr>)` to `TypeExprKind` in `ast.tw`. In `parse_type_expr_base`, add a `.LParen` case: parse a comma-separated type list, require 2–4, build `.Tuple(elems)`. (There is no existing `(` type case, so nothing conflicts; `fn(...)` types are handled under the `fn` keyword path.)

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

## Task 5: Stage0 (Rust) tuple parity

Mirror Tasks 2–4 in `src/` so `make stage2` reaches a fixed point with tuple syntax present in stage0-compiled source. Use the boot implementation as the reference spec. Happy path only — no diagnostic polish.

**Files:**
- Modify: `src/syntax/ast.rs`, `src/syntax/parser.rs`, `src/syntax/pretty.rs`, `src/types/resolve.rs`, `src/types/check.rs`, stage0 Core-IR lowering.
- Test: `cargo test --release` (add unit tests near existing parser/type tests); `make stage2`.

**Interfaces:**
- Produces: stage0 parses `(a, b)` / `(A, B)` / `._n`, resolves to the same `TupleN` record shape, and lowers identically to boot.

- [ ] **Step 1: Write failing Rust tests**

Add unit tests in `src/syntax/mod.rs` (parser) and `src/types/` asserting `(1, 2)` parses to a tuple expr, `(Int, String)` to a tuple type, and both lower/check without error. Run `cargo test --release <names>` → FAIL.

- [ ] **Step 2: AST nodes**

Add tuple variants to `Expr`/`TypeExpr` in `src/syntax/ast.rs` (match the boot `ExprKind.Tuple`/`TypeExprKind.Tuple` shape: a vector of children).

- [ ] **Step 3: Parser**

In `src/syntax/parser.rs`, extend the `(` expression primary (find the grouping case) with a comma-loop → tuple; add a `(` case to the type parser. Enforce arity 2–4.

- [ ] **Step 4: Resolve + lower**

In `src/types/resolve.rs`/`check.rs`, resolve tuple types to `TupleN<…>` and tuple literals to `TupleN` record construction, mirroring `Option`/`Result` handling; ensure `pretty.rs` can render the nodes (validity, not byte-parity with boot fmt).

- [ ] **Step 5: Verify parity**

Run: `cargo test --release` (green) then `make stage2`. Expected: `Fixed point reached: stage3 == stage4`. This is the acceptance gate — stage0 and boot now produce identical Wasm for tuple-bearing source.

- [ ] **Step 6: Commit**

```bash
git add src/
git commit -m "feat(tuple): stage0 parity for tuple literals and types"
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
- **Open uncertainty carried from the design:** whether same-named `compare`/`to_string` witnesses overload cleanly per `TupleN` receiver, and whether `Ord`/`Stringify` conditional satisfaction needs a `try_builtin_container_contract` entry. Task 1 Step 3 resolves both before any parser work.
