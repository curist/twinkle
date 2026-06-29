# Case-of-Known-Constructor Fusion

Status: **DROPPED (spike concluded 2026-06-29).** The box is a real cost, but the
rewrite can't fire on this compiler's IR without large missing infrastructure, and
the only cheap variant just re-delivers a win `@std.buffer.set_byte` already
provides. Finding recorded below; `set_byte` stays as the documented escape hatch.

## Spike finding (why dropped)

A microbench isolating the box (`Byte.from_int(x).unwrap().to_int()` vs a
hand-written range check, 20M iterations) confirmed the `StructNew` of
`Option<Byte>` + the `unwrap` match is **~44% of the boxed loop** (~16.4ms →
~9.3ms) — consistent with the ~13%-of-base64 figure. So the allocation is genuine.

But the optimized IR for `Byte.from_int(n).unwrap()` is:

```
let L2: Byte? = call Fn67(L1)     // from_int — an OPAQUE call, not a constructor node
let L3: Byte  = call Fn284(L2)    // unwrap — a SEPARATE function call
fn unwrap__Byte [Fn284] params=[L2: Byte?]
  body: match L2 { Some(L3) => L3, None => error(...) }
```

The plan rewrites `Match(MakeVariant(tag, args), arms)`. **Neither operand exists in
that form:**

- **No `MakeVariant` node.** `Byte.from_int` is a *backend intrinsic*
  (`emit_intrinsic_byte_from_int` in `codegen/emit.tw`); the `StructNew` is
  materialized only at WAT-emit time. At Core IR / ANF it's an opaque `Call`.
- **No adjacent `Match`.** `unwrap` is a normal prelude function whose `case` lives
  in its own body, and there is **no inliner** in the pipeline (even `--opt` keeps
  `unwrap__Byte` distinct). The construct and destruct are split by a call boundary.

So the general approach needs two pieces that don't exist — an inliner *and* a
Core-IR representation of intrinsic constructors — before the pass could match
anything; that's a large project at arguably the wrong layer (the box only ever
exists in the backend). The only cheap variant is a peephole keyed on specific
intrinsic FuncId *pairs* (`Call(unwrap, Call(byte$from_int, x))` → fused
range-check-and-trap), which is special-casing that must enumerate every
safe-constructor × `unwrap` combination, and it still won't punch through `set_u8`
to let `set_byte` be deleted. Absolute cost is ~0.36ns/iter, only relevant in
mill-of-bytes crypto/codec loops that already have `set_byte`. Not worth it.

---

Original proposal (kept for reference):

A general Core-IR simplification that
cancels a sum-value constructor immediately consumed by a destructor (`case` /
`unwrap` / `try`) in the same expression, so the intermediate boxed variant is
never allocated. Surfaced while optimizing `@std.crypto` base64
([archive/crypto-perf.md](archive/crypto-perf.md)): `Byte.from_int(n).unwrap()` in
the encode hot loop allocates an `Option<Byte>` per call purely to tear it open on
the next instruction.

## The problem, concretely

`Byte.from_int(n)` returns `Option<Byte>`, lowering to a `StructNew` of the GC
`Variant` struct on the `.Some` path (a heap allocation) or `.None`. `.unwrap()`
then immediately matches the tag, extracts the payload, and traps on `None`. The
`Option` exists for one instruction — it never escapes, is never stored, and is
observed only by the unwrap that follows it.

```tw
b := Byte.from_int(n).unwrap()   // allocates Option<Byte>, then destructures it
```

Measured cost: in the base64 encoder, replacing `byte()` + `set_u8` (which goes
through `Byte.from_int(..).unwrap()`) with a raw `buffer.set_byte(off, intVal)`
dropped encode ~34µs → ~30µs/op at 4 KiB — i.e. the per-store variant allocation
was ~13% of encode. That hand-fix is why `@std.buffer` now carries *both*
`set_u8(Byte)` and `set_byte(Int)`; with this fusion the raw variant would be
unnecessary.

## Why it's missing

The boot optimizer (`boot/compiler/opt/`) is `const_fold`, `dead_let`, `copy_prop`,
`semantics`, `loop_builder`, `uniqueness`, `liveness`. `const_fold` only evaluates
`ABinOp`/`AUnOp` with **literal** operands — nothing recognizes a `case`/`unwrap`
over a **freshly constructed** variant. So the box is always allocated, then torn
apart.

## The rewrite

Standard *case-of-known-constructor* / constructor–destructor cancellation:

```
case Ctor(args) { Ctor(binds) => body, _ => fallback }
    ⇒  body[binds := args]            (when Ctor matches the arm)

unwrap(Some(v))  ⇒  v
unwrap(None)     ⇒  trap
try Ok(v)        ⇒  v
try Err(e)       ⇒  early-return Err(e)
```

For `Byte.from_int(n).unwrap()` the net effect is:

```
if 0 <= n && n <= 255 { n_as_byte } else { trap }
```

a range check and a value — no `StructNew`, no variant, no payload array.

**Side condition (the only subtle part):** the constructed value must be
single-use — consumed *only* by the matching destructor, not also stored, returned,
or matched elsewhere. In the `X.from_y(..).unwrap()` / `try literal` shapes this is
syntactically obvious (the variant is born and consumed in one expression). A safe
v1 can restrict to exactly those local shapes and skip a general escape analysis.

## Where it fires beyond crypto

Any "build a sum value, immediately tear it apart" site — common with the
Option/Result-returning safe constructors when the caller knows the input is valid:

- `Byte.from_int(..).unwrap()`, `Int.from_string(..).unwrap()`,
  `String.from_utf8(..).unwrap()`, `Char`/byte conversions in lexers/codecs.
- `try .Ok(v)` / `try .Some(v)` written inline.
- `case .Some(x) { .Some(y) => .., .None => .. }` produced by desugaring.

So the payoff is language-wide allocation + branch reduction, not a crypto patch.

## Where it'd live

A Core-IR (or ANF) peephole pass, ideally hosted by the planned
[fold_core_expr](archive/) combinator traversal (the one exhaustive
`CoreExprKind` child-walk) so the recursion is sound-by-default. Match shapes:

- `Match(MakeVariant(tag, args), arms)` → select the arm whose pattern tag ==
  `tag`, substitute its binders with `args`, drop the construct. Wildcard arm is
  the fallback when no tag arm matches.
- The `unwrap`/`try` intrinsic forms after they lower to `Match`, so handling
  `Match`-on-construct covers them uniformly (verify the lowering order — do this
  *after* unwrap/try desugar to `case`).

Binding-awareness: the substitution must respect the optimizer's existing
copy-prop / liveness invariants (don't duplicate a non-atomic `arg` into multiple
binder uses without a `Let`; reuse copy_prop machinery).

## Plan (spike-first)

1. **Probe** — hand-write the fused form for `Byte.from_int(n).unwrap()` in a tiny
   bench and confirm the allocation delta matches the ~4µs/op base64 figure (sanity
   that the box is the cost, in isolation).
2. **v1 pass** — `Match(MakeVariant(..), arms)` cancellation in `boot/compiler/opt/`,
   single-use/local shapes only, behind the fixed-point pass manager. Cross-check
   against the full boot suite (variant/`case`/`try` heavy) for behavioral identity.
3. **Measure** — base64 encode with `byte()` + `set_u8` should match `set_byte`;
   if so, `@std.buffer.set_byte` can be **dropped** and crypto reverted to the
   typed store. Re-run the crypto bench + a couple of `try`/`unwrap`-heavy
   workloads (lexer, json) for incidental wins.
4. **Stage0 parity** — only if `boot/main.tw` itself comes to depend on the fused
   output (it won't for correctness; this is purely an optimization, so stage0 can
   lag — confirm the no-stage0-parity rule for pure boot-codegen opts applies).

## Success criteria / kill criteria

- **Keep** if v1 is behaviorally identical across the boot suite AND recovers the
  `set_byte` win on the typed path (lets us delete the raw setter), or shows a
  measurable win on another allocation-heavy `try`/`unwrap` workload.
- **Drop** if the single-use restriction makes it fire too rarely to matter, or the
  substitution interacts badly with COW/uniqueness invariants. In that case
  `set_byte` stays as the documented escape hatch and this doc is archived with the
  finding.

## Out of scope

- General partial-evaluation / full case-of-case. This is the narrow, high-value
  single-construct-single-destruct shape only.
- Escape analysis for non-local constructors; v1 is syntactic single-use.
