# Tuples via `@std.tuple` — plan, blocked on transparent type-alias re-export

Status: **done**. Supersedes the open questions in [`tuple.md`](./tuple.md), which
remains the design rationale (why a library `Pair`, why not `(a, b)` syntax).
Part A shipped three resolver fixes (the two below plus inherent-method
re-export, found while wiring Part B); Part B shipped `@std.tuple` with
`Pair`/`Triple`, `.first`/`.second`/`.third` fields, and the `tuple.pair` /
`tuple.triple` constructors.

This plan settles the structure for shipping `Pair` **and** `Triple` as a single
`@std.tuple` module, and documents a compiler blocker discovered while
prototyping it: **cross-module transparent type-alias re-export is broken.** That
blocker must be fixed first; it is a general language capability, not
tuple-specific.

## Decisions (settled)

- **Ship `Triple` alongside `Pair`** in one module, `@std.tuple`.
- **Keep terse constructors** `tuple.pair(a, b)` and `tuple.triple(a, b, c)` —
  the constructor ergonomics are the point.
- **Include `swap`** for `Pair` despite no current call site: it is trivial and
  obvious. (This is a deliberate exception to the "omit until a caller needs it"
  stance in `tuple.md`; `map_first`/`map_second` stay omitted.)
- **Single import surface:** `use @std.tuple` for constructors and
  `use @std.tuple.{Pair, Triple}` for the type names — mirroring the documented
  `@std.view` two-line shape. The user must not need to know `Triple` physically
  lives in a submodule.

### Why this needs a compiler fix

The hard constraint (confirmed in code): **one value namespace per module, no
overloads.** `Stringify` requires an inherent method literally named
`to_string`. `Pair` and `Triple` each need their own `to_string`, so two
`pub fn to_string` cannot coexist in one module. Therefore `Pair` and `Triple`
must live in **separate modules**, each owning its `to_string`.

But we still want one `@std.tuple.{Pair, Triple}` surface. `pub use` re-export is
explicitly rejected (`docs/design/module.md`). The remaining mechanism is a
**transparent type-alias re-export**:

```tw
// boot/stdlib/tuple/triple.tw   (module `triple` — the real nominal home)
pub type Triple<A, B, C> = .{ first: A, second: B, third: C }
pub fn to_string<A: Stringify, B: Stringify, C: Stringify>(t: Triple<A, B, C>) String { ... }

// boot/stdlib/tuple.tw          (module `tuple` — primary surface)
use .tuple.triple as triple_mod
pub type Triple<A, B, C> = triple_mod.Triple<A, B, C>   // transparent re-export
pub type Pair<A, B>      = .{ first: A, second: B }
pub fn pair<A, B>(first: A, second: B) Pair<A, B> { .{ first, second } }
pub fn triple<A, B, C>(first: A, second: B, third: C) Triple<A, B, C> { .{ first, second, third } }
pub fn swap<A, B>(p: Pair<A, B>) Pair<B, A> { .{ first: p.second, second: p.first } }
pub fn to_string<A: Stringify, B: Stringify>(p: Pair<A, B>) String { ... }
```

Because "type aliases don't create distinct nominal types" (spec), a value typed
`tuple.Triple` is identically the nominal `triple.Triple`, so `t.to_string()`
dispatches to `triple.to_string` (inherent methods resolve via the type's home
module) — sidestepping the same-module `to_string` collision while keeping the
`@std.tuple.{Pair, Triple}` surface.

This pattern does not currently work. Two bugs block it.

## Part A — Compiler fix: transparent type-alias re-export

Aliases are spec'd transparent ("type aliases don't create distinct nominal
types"), but the implementation only partially honors that. Prototyping the
re-export surfaced two independent bugs, reproduced minimally below. Note both
reproduce with **same-module** generic aliases too — this is not specific to
cross-module or to tuples.

### Bug 1 — false "circular type alias" when alias name == target name

```tw
// triple.tw
pub type Triple<A, B, C> = .{ first: A, second: B, third: C }
// tuple.tw
use .triple as triple_mod
pub type Triple<A, B, C> = triple_mod.Triple<A, B, C>   // error: circular type alias `Triple`
```

Renaming the alias (`Foo = triple_mod.Triple`) compiles, proving the qualified
reference resolves correctly — the cycle report is spurious.

**Root cause.** `detect_circular_aliases` / `is_circular_alias` /
`mono_contains_circular` (resolver.tw ~2609–2687) track visited types by **name**
(`find_type_name(tid)` then string compare against a `visited` dict). The alias
`Triple` and its distinct-TypeId target `Triple` share a name, so the walk sees
`Triple` "again" and reports a self-cycle.

**Fix.** Key cycle tracking by **TypeId identity**, not name. Real cycles
(`type A = B; type B = A`, or `type A = A`) are still caught because they revisit
the same tid. Distinct types that merely share a name no longer collide.

### Bug 2 — alias TypeId leaks past the checker → backend mismatch / dangling FuncId

```tw
type Base<A, B, C> = .{ first: A, second: B, third: C }
pub type Tri<A, B, C> = Base<A, B, C>
fn label<A, B, C>(t: Base<A, B, C>) String { "base" }
// calling label with a Tri-typed value:
//   non-generic alias -> backend verifier: "arg 0 has mono Named(T18), expected Named(T17)"
//   generic alias     -> "lookup_func_sym: unknown FuncId NNN"
```

Construction and field access through an alias already work (the checker's
`expand_alias` runs during unification and field synthesis). Passing an
alias-typed value into a position expecting the **underlying** type fails.

**Root cause.** `resolve_single_name` (resolver.tw:2378–2387) returns
`MonoType.Named(entry.id, args)` using the **alias's own TypeId**, never checking
whether `entry.def` is `.Alias`. The checker only expands aliases lazily during
unification (`expand_alias`, 28 call sites in checker.tw), so the alias TypeId
survives in three channels that the checker never rewrites:

1. the final per-expression **TypeMap** (checker.tw ~5064),
2. **function signature** param/return types (resolver `FunctionSig`),
3. **record field / variant** types.

Lowering, monomorphization, and the backend have **zero** alias awareness
(`expand_alias` exists only in checker.tw), so they treat `Named(alias_tid)` as a
distinct nominal type. Non-generic → backend nominal verifier rejects the
mismatch; generic → monomorphization can't match the call's type args to a
specialization, leaving a reference to the dropped generic original (the
`unknown FuncId`).

**Fix (single source).** Expand aliases at `resolve_single_name`: when the
looked-up entry's `def` is `.Alias(_, type_params, target)`, substitute the
applied `args` into `target`'s `Var` placeholders and recursively expand,
returning the underlying type instead of `Named(alias_tid)`. This is the one
place type names become `MonoType`, so it feeds all three channels at once; no
alias TypeId ever enters resolved types.

**Why this is safe (ordering).** Pass 2 resolves type declarations in
**topological dependency order** (`topo_sort_type_decls`) and resolves function
signatures **after** all type decls. Aliases cannot be mutually recursive (Bug 1
guards that), so they form a DAG and a referenced alias's target def is always
already resolved when referenced. Cross-module targets are resolved at import
time. So eager expansion is always complete, with no forward-reference hazard.
Fall back to `Named(tid)` when a def is unavailable (only possible mid-resolution
of a genuine cycle, which Bug 1's detector then reports).

**Implementation notes.**
- `resolver.tw` already has `pub fn lookup_type_def(tid)`. It needs a small
  type-param substitution over `MonoType` (the alias target is stored with
  `Var("A")` placeholders), mirroring checker.tw's `subst_vars` + `build_var_map`.
  Add a local helper rather than reaching into the checker.
- Leave the checker's `expand_alias` calls in place — they become no-ops on
  already-expanded types and cost nothing.
- Minor, accepted UX change: diagnostics now show the underlying type rather than
  the alias name (already true after unification today).

### Bug 3 — re-exported alias does not carry the home module's inherent methods

Found while wiring Part B. Construction, field access, and equality through the
re-exported `Triple` alias all worked, but `${t}` / `t.to_string()` failed with
"does not satisfy `Stringify`" / "no method `to_string`" **unless the consumer
also wrote `use @std.tuple.triple`** — defeating the single-surface goal. Inherent
methods are merged into a consumer's env only from **directly-imported** modules;
re-exporting a type carried the type but not its home module's methods.

**Root cause.** Method tables key off a type's own `TypeId` (`methods_by_type`).
A transparent alias's own id has no methods — they live under the **target**
type's id. The export builder (`extract_exports_for_module`) collected a type's
methods via `lookup_type_methods(entry.id)`, so for the alias it found nothing;
and the import merge (`register_imported_interface_types`) registered methods
under the alias's remapped id, not the underlying type that alias-typed values
actually resolve to after Bug 2's expansion.

**Fix.** A `method_source_tid(env, entry)` helper follows an alias entry to its
underlying `TypeId`. Used at both ends: the export builder now collects the
re-exported type's methods from the home type, and the consumer registers them
under the underlying id (which is what `resolve_single_name`'s eager expansion
produces for alias-typed values). No per-method re-export syntax needed; it falls
out of the alias being transparent.

### Part A verification

- New repro files under `boot/repros/` (the project's regression-repro home),
  e.g. `alias_reexport_circular.tw` (Bug 1) and `alias_reexport_dispatch.tw`
  (Bug 2, covering both the non-generic verifier path and the generic
  monomorphization path).
- Resolver/checker suite coverage: alias name == target name compiles; a
  value typed via a (same-module and cross-module) alias passes to a function on
  the underlying type and dispatches an inherent method through the alias.
- `make boot-test`, then `make bundle-cli` (the self-host loop recompiles the
  boot sources, exercising the changed resolver on the whole compiler) — must be
  green before Part B.
- **Stage0 (`src/`) is not required for this feature.** Stage0 only compiles
  what bootstraps `boot/main.tw`, and the compiler never `use`s `@std.tuple`, so
  stage0 never compiles `tuple.tw`/`triple.tw`. `@std.tuple` is compiled solely
  by `target/twk` (the boot compiler) — for user programs and for the
  boot-compiled test suite (`target/twk run boot/tests/main.tw`). The fix reaches
  `target/twk` through the normal `make bundle-cli` self-host loop, which compiles
  Twinkle *source text* and so needs no prior understanding of alias re-export.
  Stage0 would only need the fix if the compiler's **own** sources (anything
  stage0 compiles) ever used cross-module alias re-export; they do not. Existing
  same-module aliases in the compiler (e.g. `type ItemParse = Parse<Item>` in
  `parser.tw`) work today only because alias and underlying are used
  consistently and never meet in a call position — so the resolver fix neither
  requires stage0 nor regresses it.

## Part B — `@std.tuple` module (after Part A is green)

Layout:

- `boot/stdlib/tuple.tw` — module `tuple`: `Pair`, `pair`, `swap`, `Pair`'s
  `to_string`; re-exports `Triple` via transparent alias and provides the
  `triple` constructor.
- `boot/stdlib/tuple/triple.tw` — module `triple`: nominal `Triple`, `Triple`'s
  `to_string`. (The core_lib generator already recurses into stdlib subdirs, and
  `resolve_module_path` already handles nested `@std.tuple.triple` paths — no
  wiring changes needed for the nested layout.)

`Pair` stays the primary, common case; `Triple` is the escalation. Both satisfy
`Stringify`, so `${p}` and `${t}` both work.

Rollout:

1. Add the two source files. Regenerate `core_lib` and `make bundle-cli`.
2. Add `boot/tests/suites/stdlib_tuple_suite.tw`; wire into `boot/tests/main.tw`.
   Cover: construction, field access, `swap`, structural `==` (records give it
   when fields satisfy `Eq`), `Stringify` for both arities, and `Triple` reached
   purely through `@std.tuple.{Pair, Triple}` (proving the re-export).
3. Document under `docs/API.md` Standard Library (`@std.tuple`) and add to the
   `use @std.*` list. Note the two-import-line shape, as for `@std.view`.
4. Migrate the in-tree pure-plumbing pairs (`Pop<T>`, `Probe` in
   `tools/leetcode/`) to `Pair`; leave genuine domain records alone. Note: these
   are evidence that the gap is **Pair-shaped** — no current site needs `Triple`
   or `swap`, so both are forward-looking.

## Open questions (carried forward)

- Promote `Pair` (and the `tuple` surface) to the prelude later? Still
  conservative: module first, promote on evidence. The single-import friction is
  the main pro-promotion signal.
- `map_first`/`map_second`/`map_both`: omitted until a caller needs them.
- Wider arities beyond `Triple`: resist; a 4-tuple is a record smell. If ever
  needed, it extends the same submodule-per-arity + alias-re-export pattern.
