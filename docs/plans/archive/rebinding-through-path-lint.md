# Rebinding-through-path lint

> **Status: report-only rule shipped (L5 `direct-rebinding`).** Teachable
> `twk lint` rule for avoiding temporary copies when Twinkle's field/index
> rebinding syntax can express the update directly. Implemented as a structural
> AST visitor in `boot/compiler/lint.tw`, covered by the `lint_pass` suite.
> Auto-fix (below) remains future work, and dogfooding over `boot/` is pending a
> CLI rebundle (`make bundle-cli`), since `twk lint` runs the baked-in compiler.

## Goal

Teach Twinkle's value model by flagging code that creates an unnecessary alias
for a value, updates the alias, and then returns or assigns the alias back.
Prefer rebinding the original name/path directly.

Twinkle assignment is rebinding, not mutation. These two forms are semantically
aligned, but the direct form better communicates the language model:

```tw
// Avoid
d := reg.by_name
d[internal] = entry
reg.by_name = d

// Prefer
reg.by_name[internal] = entry
```

Likewise for records:

```tw
// Avoid
updated := func
updated.body = body
updated

// Prefer
func.body = body
func
```

This complements the existing record-copy-helper lint. That rule catches full
record reconstruction by field copying; this rule catches the next layer of
unnecessary ceremony: creating a temporary alias solely to update it and thread it
back.

## Rule name

Proposed report-only lint: `direct-rebinding`.

Catalog slot: this is a report-only lint, so it lands as the next `L` rule
(`L5`) in `docs/plans/archive/linter.md`. Note the `L5`/`L6` that an earlier linter
draft *rejected* were different rules (whole-program effect analysis); there is
no clash, but linter.md must be updated so the rule catalog stays the single
source of truth. Name style follows the existing `record-copy-helper`
report-only rule.

Message shape:

```text
`tmp` is only an alias for `<path>`; rebind `<path>` directly
```

For whole-value aliases:

```text
`tmp` is only an alias for `<name>`; rebind `<name>` directly
```

## Covered patterns

### Pure alias accumulator

Detect a temporary introduced only so later statements can rebind that temporary
instead of rebinding the original variable:

```tw
cur := reg

for spec in specs {
  cur = cur.add(spec)
}

cur
```

Prefer:

```tw
for spec in specs {
  reg = reg.add(spec)
}

reg
```

This is the most general form: the alias has no independent meaning, so it makes
readers track two names for one evolving value.

### Field/index temporary copied back to the same path

Detect the straight-line shape:

```tw
tmp := root.field
tmp[k] = v
root.field = tmp
```

Suggest:

```tw
root.field[k] = v
```

Also allow field updates on the temporary:

```tw
tmp := root.field
tmp.x = v
root.field = tmp
```

Suggest:

```tw
root.field.x = v
```

The same applies to deeper paths:

```tw
tmp := state.registry.by_name
tmp[name] = entry
state.registry.by_name = tmp
```

becomes:

```tw
state.registry.by_name[name] = entry
```

### Whole-record temporary returned immediately

Detect helper/update functions where a temporary aliases a function parameter or
local only to update the alias and return it:

```tw
updated := info
updated.repr = repr
updated.wasm_type = wasm_type
updated
```

Suggest:

```tw
info.repr = repr
info.wasm_type = wasm_type
info
```

This is especially useful for small builder-style helpers whose entire purpose is
to return an updated value.

### Whole-record temporary assigned back

Detect:

```tw
updated := state
updated.cache = cache
state = updated
```

Prefer:

```tw
state.cache = cache
```

This form is less common because rebinding the original name is already legal,
but it is the clearest statement of intent when it appears.

## Conservative trigger predicate

The lint should fail closed. Only report when all of these hold:

1. The temporary is bound by a simple identifier binding:
   - `tmp := ident` for pure alias / whole-value forms, or
   - `tmp := path`, where `path` is an identifier followed by field postfixes.

   For the first implementation, source paths are field-only. Indexes are allowed
   in updates rooted at the temporary (`tmp[k] = v`), but not in the source path
   being aliased. This avoids changing evaluation shape/count for arbitrary index
   expressions such as `tmp := reg.by_name[key()]`. A later version may admit
   indexed source paths after defining a "stable path" predicate for literal,
   identifier, and field-only index expressions with no calls, assignments,
   `try`, blocks, or other effects.

   Call results are not aliases: `tmp := reg.by_name[name].unwrap()` is outside
   this rule unless a later, separate extract-update-store-back rule is designed.
2. The temporary has no reads except:
   - as the value being rebound (`tmp = ...`),
   - as the receiver/base of a rebinding expression (`tmp.x = v`, `tmp[k] = v`),
   - as the receiver of a call only when the enclosing expression is self-rebinding
     (`tmp = tmp.method(...)`, or an equivalent expression rooted at `tmp`),
   - as the tail expression of the current block/function, or
   - as the right-hand side of assignment back to the original path.

   MVP completion is tail-expression-only. Explicit `return tmp` is out of scope
   for the first version; early-return handling can be added later if needed.
3. The original identifier/path is not read independently between alias creation
   and completion. Otherwise direct rebinding can change which version later code
   observes. For example, `cur := reg; cur = cur.add(a); cur = cur.merge(reg)`
   must not report because `reg` means the old value. **The completion region
   includes loop bodies that the scan descends into:** a read of the source
   inside the loop that carries the alias rebind invalidates the candidate, e.g.
   `cur := reg; for x in xs { cur = cur.add(x); sink(reg) }; cur` must not report
   even though the only alias rebind is the rewrite-equivalent one. The
   straight-line `merge` example shows the same hazard outside a loop; the
   loop-body variant is the one that is easy to miss.
4. For path-copy-back candidates, every assignment to the temporary is a
   field/index rebinding rooted at the temporary (`tmp.x = v`, `tmp[k] = v`,
   `tmp.x[k] = v`). Independent reads of the source path, or of any prefix/root
   whose observed value would differ after direct rebinding, also invalidate the
   candidate. For example, `log(reg.by_name.len())` between `tmp := reg.by_name`
   and `reg.by_name = tmp` must not report; neither should a read of `reg` or a
   relevant prefix such as `state.registry` for `tmp := state.registry.by_name`.
5. For pure alias candidates, each `tmp = expr` can be rewritten by replacing
   `tmp` with the original identifier in both the assignment target and uses
   rooted at `tmp` inside `expr`.
6. The original root/path is not rebound between the temporary binding and the
   copy-back/final return, except by the rewrite-equivalent updates themselves.
7. Scope must match exactly. Fail closed if either the temporary or the source
   root is shadowed by a binding, loop pattern/index, closure parameter, or any
   other local declaration in the scanned region.
8. The rule only considers a single straight-line region at first. It may look
   inside loop bodies for uses of the alias, but should not cross `if`, `case`,
   `cond`, closures, or early returns in the initial implementation.
9. Do not report if the temporary is used to preserve an old value for another
   computation, appears in a closure, is passed to a function other than as an
   obvious method receiver in a self-rebinding assignment, or is used more
   generally as a value. This restriction is about the *temporary*. A closure
   that captured the *source* before the alias is created is not a hazard:
   Twinkle closures capture by value over immutable bindings, so rebinding the
   source in place leaves the already-captured value untouched — exactly as the
   alias form does. We do not need to invalidate on source-captured closures,
   and should not, or the rule would rarely fire.

These restrictions make the lint teach the common idiom without trying to prove
large program equivalences.

## Auto-fix policy

Start report-only.

Many cases are mechanically fixable, but an auto-fix requires careful source
splicing across multiple statements and trivia. It should not be part of the
first implementation. Once the report-only rule proves useful, add a rewrite
variant only for the simplest contiguous forms:

```tw
tmp := path
tmp[expr] = value
path = tmp
```

and

```tw
tmp := ident
tmp.field = value
tmp
```

Even then, preserve comments conservatively: skip auto-fix when comments or blank
line-sensitive trivia appear inside the candidate range. For the final-expression
form, the fix must remove the alias binding, rewrite every update root, and
rewrite the tail expression from `tmp` to the original identifier/path; do not
attempt this unless the statements are contiguous and the AST shape is exactly
one of the narrow supported forms.

## Implementation sketch

Home: structural lint visitor in `boot/compiler/lint.tw`, alongside the existing
record-copy-helper and unreachable-code rules.

The checker is not needed; this is syntax/dataflow over one block.

Suggested helpers:

- `is_path_expr(expr) Bool` — identifier plus field/index postfixes.
- `path_with_root(path, new_root) Expr` — conceptual helper for comparing the
  temporary-rooted update path to the original path.
- `collect_tmp_uses(block, tmp)` — classify uses as update, copy-back, final
  expression, independent source read, shadowing, or general use.
- `same_path(a, b) Bool` — compare path shape without relying on source text.
  Must compare index segments structurally (not just field names), since the
  deferred indexed-source work will reuse this helper for paths like
  `reg.by_name[name]`.
- `contains_independent_read(expr, source_path, tmp) Bool` — finds reads of the
  original path/root that are not the rewritten target/root.

Rebinding spans **two distinct AST node kinds**, and the classifier must handle
both:

- **Bare-identifier rebind** (`cur = cur.add(x)`, `source = tmp`) parses as
  `Stmt.Let { is_rebind: true }` — see `parse_binding_stmt` / `parse_stmt` in
  `boot/compiler/parser.tw`, where an `Ident` followed by `=` becomes a binding
  statement. The pure-alias form (predicate #5) and the whole-record copy-back
  form (`state = updated`) both flow through this node, not through an
  assignment expression.
- **Path-LHS rebind** (`reg.by_name = tmp`, `tmp[k] = v`, `tmp.x = v`) parses as
  an `ExprStmt` wrapping `ExprKind.Binary(.Assign, lhs, rhs)`.

So the candidate-introducing binding (`tmp := source`) is a
`Stmt.Let { is_rebind: false }`; later updates and copy-backs are a mix of
`Stmt.Let { is_rebind: true }` and `Binary(.Assign)` expr-statements.
Classification must inspect both — do not only match `Binary(.Assign)`, and do
not only match top-level statement nodes (path rebinds can also appear nested in
expression position). Recursive scans must distinguish target occurrences from
read occurrences: an assignment/rebind LHS is not an ordinary read of that path,
though subexpressions of future indexed LHS forms may need their own
stability/read checks.

Initial algorithm per block:

1. Scan statements for `tmp := source` candidates — a `Stmt.Let` with
   `is_rebind: false` whose value is a bare identifier or a field-only path.
2. Walk following statements in the same block until the candidate is completed
   or invalidated.
3. Accumulate updates rooted at `tmp`, including simple `tmp = ...` self-rebinding
   assignments for pure aliases.
4. Complete when either:
   - a statement assigns `source = tmp`, or
   - the block's final expression is `tmp` for the pure-alias / whole-record-copy
     form.
5. Emit a lint if there is at least one accumulated update and no invalidating
   use.

## Validation

Beyond unit examples, dogfood the rule over `boot/` itself once it runs. The
field-rebinding idiom is already mandated in `CLAUDE.md` and was applied
manually (see the "Use field rebinding for record updates" work), so the boot
tree is a realistic corpus: it both validates precision (no false positives on
intentional alias-preservation) and is likely to surface genuine findings.

## Examples to cover

Report:

```tw
cur := reg
for spec in specs {
  cur = cur.add(spec)
}
cur
```

```tw
slots := pf.slots
slots[id] = info
pf.slots = slots
```

```tw
rewritten := anf
rewritten.functions = functions
rewritten
```

Do not report:

```tw
old := state.cache
state.cache = new_cache
log(old.len())
```

```tw
tmp := reg.by_name
tmp[k] = v
other(tmp)
reg.by_name = tmp
```

```tw
cur := reg
cur = cur.add(a)
cur = cur.merge(reg)
cur
```

```tw
tmp := reg.by_name
tmp[k] = v
log(reg.by_name.len())
reg.by_name = tmp
```

```tw
tmp := state.registry.by_name
tmp[k] = v
log(state.registry)
state.registry.by_name = tmp
```

```tw
tmp := reg.by_name[key()]
tmp.value = v
reg.by_name[key()] = tmp
```

```tw
entry := reg.by_name[name].unwrap()
entry.canonical_name = .Some(canonical)
reg.by_name[name] = entry
```

```tw
tmp := state.config
if enabled {
  tmp.debug = true
}
state.config = tmp
```

The last example may become lintable later, but the first version should stay
straight-line only. The `unwrap()` example is an extracted value, not an alias for
an assignable path; it needs a separate, more complex rule if we ever want it.

## Relationship to existing lints

- `record-copy-helper` catches full record reconstruction by field copying:
  `Foo.{ a: foo.a, b: new_b }`.
- `direct-rebinding` catches unnecessary aliases around rebinding:
  `tmp := foo; tmp.b = new_b; tmp` and `cur := acc; cur = cur.step(); cur`.

Together they teach the same principle: update values by rebinding the field or
index path you mean, instead of manually copying surrounding structure.
