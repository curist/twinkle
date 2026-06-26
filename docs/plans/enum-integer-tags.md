# Enum Integer Tags (`.tag` / `from_tag`) — Design

**Goal:** Give field-less enums an explicit, checked integer mapping so boundary
constant groups (LSP wire kinds, wasm section IDs, JSON-RPC error codes) can be
real nominal types instead of loose `Int` constants.

**Status:** Design settled. Bite-sized implementation plan is the next step
(needs resolver/lowering mapping; see *Implementation map* below).

---

## Motivation

Boundary code keeps reaching for "a cohesive group of named integers." Today
those are loose `pub … := <int>` constants (e.g. `boot/lib/lsp/kinds.tw`), which
are raw `Int` end-to-end — nothing stops `kind: Int = 999` anywhere, in-memory
or on the wire. The values also come in every shape: sequential-with-gaps
(`kinds.tw`: 1,2,3,5,6,7,9,…), negative (JSON-RPC `-32601`), out-of-order
(wasm sections emit datacount=12 before code=10), and bitmask
(semantic-token modifiers 1,2,4,8). Auto-increment alone can express none of the
last three, so values must be explicitly settable. (Bitmask motivates *per-flag*
values only; modeling flag *sets* is a Non-goal — see below.)

The pattern already exists internally: `encode_val_type_into`
(`boot/compiler/codegen/wasm.tw:432`) maps the `ValType` enum to wasm bytes with
a hand-written `case`. This feature generalizes that into a first-class,
bidirectional, checked mapping.

## Scope decision: field-less enums only

`.tag` / `from_tag` / explicit `= N` are available **only on enums where every
variant is field-less** (nullary). Payload-carrying enums (`Some(T)`,
`Circle(Float)`) are rejected. Rationale:

- Keeps "a constant group" and "a sum type" as distinct concepts; `Circle(1.0).tag`
  is simply not a thing.
- Every boundary we need is field-less, so we lose nothing.
- Payload enums still map to integers the explicit way — a `fn(E) Int { case … }`.

## Non-goals

- **Flag sets.** `.tag`/`from_tag` map a *single* variant ↔ a single int. They do
  not model OR-composition or membership over combinations: for modifiers
  `{ Declaration = 1, Definition = 2, … }`, `from_tag(3)` (`1|2`) is `.None`, and
  building/testing the combined mask is the caller's job (`a.tag | b.tag`, plain
  `Int`). A first-class flag-set type is a separate, larger feature.
- **Const-expression values.** Tag values are integer literals only (incl. a
  leading `-`); no `1 + 1` (see D5). Lift later only if a real need appears.

## The tag is decoupled from the dispatch discriminant

Enums already carry a dense `0..n` discriminant (`VariantRef.vid`,
`builtin_refs.tw:13`) that `case`/match lowering depends on. The integer **tag is
a separate value mapping**, *not* that discriminant. Match lowering is untouched.

Nice property on the **forward** direction: when no explicit values are given,
the tag mapping is exactly `0,1,2,…` and coincides with the discriminant, so
`.tag` is a zero-overhead discriminant read; with explicit holes it becomes a
`vid → tag` map. The **inverse** is never free (see *`from_tag` is a select, not
a cast* below) — but the per-variant lookup table it needs is only non-trivial
once explicit values introduce holes. **You pay for sparsity only when you ask
for it.**

## Value-assignment rules

- **Default:** first variant `= 0`; each subsequent `= previous + 1`.
- **Explicit `= N`** sets that variant; **unspecified variants resume from `N + 1`**
  (this is what produces holes).
- **`N` is an integer literal** (including negative). Not a const expression — for now.
- **Duplicate-rejection runs on the *resolved* set**, after auto-increment, so it
  catches explicit-vs-explicit *and* explicit-vs-auto collisions.

```tw
type X = { A, B = 5, C }        // A=0, B=5, C=6                 ✅
type X = { A = 2, B = 1, C }    // A=2, B=1, C=2 → C collides w/ A  ❌
type X = { A, B = 0 }           // A=0 (auto), B=0 (explicit)       ❌
```

## Surface

```tw
type CompletionKind = { Text = 1, Method = 2, Function = 3, Field = 5, … }

CompletionKind.Method.tag         // 2          (forward; field-like accessor)
CompletionKind.from_tag(5)        // Option<CompletionKind>   (inverse, checked)
```

- `e.tag : Int` — synthesized accessor on a field-less-enum value.
- `T.from_tag(n: Int) : Option<T>` — synthesized static; `.None` for any unmapped int.

Typing note: `.tag` erases to `Int` deliberately — that is the wire boundary.
Callers keep the enum type in their model and signatures and call `.tag` only at
the serialization edge (inside a function whose parameter is already the enum);
`from_tag` recovers the type on decode and forces handling of bad wire values via
`Option`. The enum shrinks the untyped surface from "everywhere" to one line at
the edge; it cannot make the wire itself typed (JSON has no `CompletionKind`).

---

## Diagnostics (must be crystal clear)

These are the whole point of the field-less restriction, so they must name the
*specific* disqualifying variant and, for collisions, explain how the colliding
value was derived. `diag` = `lib.source.diagnostics`.

### D1 — explicit `= N` on an enum that has a payload variant (declaration site)

```
error: integer tags require every variant to be field-less
 --> shape.tw:1:31
  |
1 | type Shape = { Circle(Float), Rect = 2 }
  |                ------------          ^^^ tag assigned here
  |                |
  |                `Circle` carries a payload, so `Shape` cannot have integer tags
  |
  = note: `= N`, `.tag`, and `from_tag` are only available when every variant is field-less
  = help: remove the payload from `Circle`, or drop the tags and map `Shape` with
          `fn shape_code(s: Shape) Int { case s { … } }`
```

Primary span = the `= N` (what they tried); secondary span = the payload variant
(why it's rejected).

### D2 — `.tag` on a value of a payload-carrying enum (use site)

The case the design is built around: `fn x(shape: Shape) Int { shape.tag }` is
rejected.

```
error: `.tag` is only available on field-less enums
 --> x.tw:2:5
  |
2 |   shape.tag
  |   ^^^^^^^^^ `Shape` has a payload-carrying variant `Circle(Float)`, so it has no integer tag
  |
  = help: map a payload-carrying enum to integers explicitly:
          `fn shape_code(s: Shape) Int { case s { … } }`
```

### D3 — `from_tag` for a payload-carrying enum (use site)

```
error: `from_tag` is only available on field-less enums
 --> x.tw:7:11
  |
7 |   maybe := Shape.from_tag(n)
  |            ^^^^^^^^^^^^^^^^^ `Shape` has a payload-carrying variant `Circle(Float)`
  |
  = help: decode a payload-carrying enum explicitly with your own `Int -> Option<Shape>` function
```

### D4 — duplicate resolved tag (declaration site, with derivation)

The subtle one: when the collision is caused by auto-increment, the message must
explain *why* the variant resolved to the colliding value, or the user stares at
a variant that has no `= N` written.

```
error: tag value 2 is assigned to more than one variant of `X`
 --> x.tw:1:12
  |
1 | type X = { A = 2, B = 1, C }
  |            ^^^^^         ^ `C` also resolves to 2
  |            |
  |            `A` is set to 2 here
  |
  = note: `C` has no explicit tag, so it auto-increments to 2 (one past `B = 1`)
  = help: give `C` an explicit tag, or adjust the earlier values so every tag is distinct
```

For an explicit-vs-explicit collision, drop the `note` and label both spans with
"set to N here".

### D5 — non-literal tag value

```
error: an enum tag must be an integer literal
 --> x.tw:1:16
  |
1 | type X = { A = 1 + 1 }
  |                ^^^^^ expected an integer literal here
```

---

## Implementation map

Grounded in current structure; the bite-sized TDD task plan is the follow-up.

**Parser** — `boot/compiler/parser.tw`
- `parse_sum_variants` (`:634`): after the name and optional payload list, if the
  next token is `=`, parse the value. **Error-path matters:** after `=`, inspect
  the value tokens — accept `IntLit`, or `Minus` `IntLit` folded to a negative
  (the lexer emits `Minus` + `IntLit` separately, `lexer.tw:597`; there is no
  signed-literal token). If the first token is neither, span-cover from `=` up to
  the next `,`/`}` and emit **D5** there. This explicit check is required: a
  literal-only parser would consume `1` from `= 1 + 1`, land on `+`, and fall
  through to the generic `"expected ',' or '}' in sum type"` at `parser.tw:700`
  with the wrong span — *not* D5.
- `VariantDecl` (`boot/compiler/ast.tw:83`) gains `tag: Option<Int>`
  (the explicitly-written value, pre-resolution).

**Resolver / typecheck** — where `ResolvedVariant` is built (resolver module)
- Resolve final tags: fold over variants applying default/resume/explicit rules.
- Enforce field-less-only at the type level: if any variant has `fields.len() > 0`
  **and** any `tag` is `.Some`, emit **D1**.
- Distinctness on the resolved set → **D4**. The resolver must carry, per variant,
  enough provenance to render the precise note: whether its tag was explicit (and
  the `= N` span) or auto-derived (and the *id of the earlier variant* it
  incremented from). "auto" alone is not enough — D4 names the driving variant.
- Store resolved tags on the resolved enum so lowering and `from_tag` can read them.

**Member access `.tag`** — postfix field/method resolution (typecheck)
- On `e.tag` where `e`'s type is an enum: if the enum is field-less, type the
  expression `Int`; otherwise emit **D2**. Must resolve *before* the generic
  "unknown field/method" error so the message is specific.

**Static `from_tag`** — where `T.method(args)` static calls resolve
- `T.from_tag(Int) Option<T>` for field-less enums; **D3** otherwise.
- **Name precedence:** `from_tag` is reserved (synthesized) on field-less enums.
  Unlike `.tag` (a no-paren accessor, which can't collide with a `()` inherent
  method), `T.from_tag(n)` is syntactically identical to calling a user function
  `from_tag` as a static. If the enum's defining module also declares
  `fn from_tag(...)`, treat it as a name collision and error — mirroring the
  existing "field vs inherent name collision is illegal" rule — rather than
  silently shadowing either way.

**Lowering** — Core IR / ANF
- `e.tag`: read field 0 (the discriminant/`vid`) of the `rt_types__Variant`
  struct. Default `0..n` tags → that value directly; custom tags → a generated
  `vid → tag` map.
- `from_tag(n)` is a **select, not a cast.** Payload-free variants are singleton
  globals (`emit.tw:227`, registered in `variant_singletons`); field-less ⊂
  singleton-eligible (`wasm_layout.tw:545`). So `from_tag` lowers to a generated
  dispatch `tag → its singleton-global ref`, wrapped in `Option` (`.None` for any
  unmapped int). No new construction primitive is needed (the singletons already
  exist), but it is a generated lookup/switch — *not* a bounds-checked identity,
  even in the dense default case, because there is no int→ref cast.
- Decoupled from dispatch discriminant — match lowering unchanged.

**Migration (first adopters)**
- `boot/lib/lsp/kinds.tw` → `type CompletionKind = { … }` / `type SymbolKind = …`
  with explicit values; update call sites to hold the enum and `.tag` at the JSON
  edge; decode paths use `from_tag`.
- Then wasm section IDs (`wasm.tw:1432–1606`) and the JSON-RPC error codes
  (`server_core.tw:158`).

## Open (minor)

- Spelling: `.tag` + `from_tag` (chosen). Alternatives `.value`/`from_value`
  rejected to avoid clashing with record-field `value`.
- Const-expression tag values: deferred (literals only) until a real need appears.
