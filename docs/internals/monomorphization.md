# Monomorphization

This document explains what monomorphization means in Twinkle, where it sits in the
compiler pipeline, what guarantees it provides, and what it does **not** guarantee.

It is intentionally an internal compiler document, not a user-language spec.

---

## Summary

Monomorphization is the pass that turns generic, type-parameterized IR into concrete IR
instances.

After monomorphization:

* codegen should no longer see `MonoType::Var` or unresolved type metavariables
* generic functions should have been replaced by concrete specializations
* calls should target concrete specialized functions rather than generic originals

Example:

```tw
fn id<T>(x: T) T { x }

println("${id(42)}")
println("${id(3.14)}")
```

Conceptually becomes:

```tw
fn id__Int(x: Int) Int { x }
fn id__Float(x: Float) Float { x }
```

The important boundary is:

* before monomorphization: IR may still contain generic functions and generic types
* after monomorphization: IR should be concrete enough for backend-specific layout decisions

---

## Pipeline Position

The intended pipeline is:

```text
parse
→ resolve
→ typecheck
→ lower to Core IR
→ monomorphize
→ lower to ANF
→ optimize
→ emit Wasm IR
→ link / encode
```

The key design choice is to perform semantic reasoning on generic IR first, and specialize
late.

This keeps:

* type checking simple
* analysis shared across all instantiations
* specialization separate from backend layout/codegen concerns

This is broadly the same high-level idea used by compilers such as Rust, even though the
exact IR structure and implementation details differ.

---

## Why Twinkle Wants Late Monomorphization

Late monomorphization gives Twinkle the useful part of static generics:

* generic source programs
* concrete backend code
* no mandatory runtime type erasure at generic call boundaries

Without monomorphization, a generic function like:

```tw
fn id<T>(x: T) T { x }
```

would force an erased representation at the call boundary. For an `Int` call, that means:

* box `Int`
* pass erased value
* treat parameter as erased in the callee
* unbox on the way out

That adds runtime overhead and leaks backend erasure concerns into otherwise concrete code.

With monomorphization, `id<Int>` can just be:

* parameter type: `Int`
* return type: `Int`
* direct codegen to `i64` on Wasm

---

## Core Guarantees

Monomorphization is expected to provide these guarantees to later passes.

### 1. Codegen sees concrete types

After monomorphization, backend-facing IR should use concrete `MonoType`s:

* `Int`
* `Float`
* `Bool`
* `String`
* `Vector<Int>`
* `Cell<Int>`
* `fn(Int) Int`
* `Iterator<String>`

and so on.

It should not leave behind:

* `MonoType::Var("T")`
* unresolved metavariables

### 2. Generic functions are specialized per concrete instantiation

If a generic function is used at multiple concrete type arguments, the compiler should
create multiple specialized functions.

Conceptually:

```text
map<Int, String>
map<String, String>
fold<Int, Int>
```

are distinct monomorphized instances.

### 3. Call sites are rewritten to concrete instances

A generic call should not survive into codegen as “call generic function with type args”.
It should already be rewritten to the concrete specialized function.

### 4. The pass is whole-program with respect to the compiled unit

Monomorphization needs visibility over the reachable linked program, not just a single
source file in isolation. Cross-module generic uses must be visible so the concrete
instances can be collected and rewritten consistently.

---

## What Monomorphization Does Not Guarantee

This is the most common source of confusion.

Monomorphization guarantees concrete **types** in IR. It does **not** automatically
guarantee concrete **runtime layouts** in the backend.

Example:

* monomorphization may ensure codegen sees `Cell<Int>`
* but the backend might still choose to lower it to an erased runtime container

That means these are separate layers:

* monomorphization: “the type is concrete”
* backend layout/codegen: “the runtime representation is concrete”

This distinction is why Twinkle has both:

* this internal monomorphization design note
* [../plans/backend-pipeline-alignment.md](../plans/backend-pipeline-alignment.md)
* [../plans/wasm-type-erasure-reduction.md](../plans/wasm-type-erasure-reduction.md)

The pipeline-alignment plan is about making the backend-oriented pipeline match this design.
The Wasm plan is about taking advantage of that concreteness in codegen.

---

## Relation To `Anyref`

Monomorphization is necessary for eliminating unnecessary `Anyref`, but it is not
sufficient on its own.

After monomorphization, the backend can choose to:

* emit concrete Wasm layouts
* emit monomorphized helper families
* preserve concrete value types across helper boundaries

If it does not do that, `Anyref` can still remain in backend code even though the source
types are concrete.

Examples of backend follow-up work:

* typed `Cell<T>` layouts
* typed user-record fields
* monomorphized iterator helpers like `Iterator.next__T__S`
* specialized helper payload layouts for hot `Option` / `Result` / `UnfoldStep` paths

So the correct mental model is:

```text
monomorphization enables concrete backend code
monomorphization does not automatically produce concrete backend code
```

---

## Hidden Runtime Types

Some abstractions carry more runtime type information than is visible in their surface type.

`Iterator<T>` is the clearest example.

For `Iterator.unfold(seed, step)`:

* `T` is the yielded item type
* `S` is the hidden state type carried between steps

These can differ.

Examples:

```tw
Iterator<Int>    // yielded value is Int
```

may have:

* `S = Int` for a numeric counter iterator
* `S = String` for a string-processing iterator
* `S = Record` for a structured state machine

This matters because monomorphization can make the surface `Iterator<T>` concrete while the
backend still needs additional information to choose a fully concrete iterator-state layout.

That is a backend/interface problem layered on top of monomorphization, not a failure of
monomorphization itself.

---

## Specialization Strategy

The intended strategy is:

1. Type checking records solved concrete instantiations.
2. Monomorphization collects the reachable concrete instances.
3. The compiler clones and substitutes generic definitions into concrete ones.
4. Call sites are rewritten to the concrete instances.
5. Generic originals do not reach backend emission.

Important properties:

* only used instances are emitted
* specialization is driven by reachable concrete call sites
* the pass may need iterative/fixpoint collection because one specialization can reveal
  more specializations transitively

---

## Why Analysis Should Happen Before Specialization

The compiler should avoid duplicating expensive semantic analysis per instantiation.

That means:

* do type checking on generic IR
* do most semantic reasoning before specialization
* specialize only after the program is already well-typed

Benefits:

* simpler compiler architecture
* less repeated work
* clearer separation between “what the program means” and “how concrete instances are emitted”

This is the main pipeline lesson worth borrowing from Rust-like compilers.

---

## Monomorphized Helper Emission

Once monomorphization is in place, the backend should increasingly prefer monomorphized
helper emission for hot concrete paths.

Examples:

* `Cell<Int>` helper/layout paths
* `Iterator.next__T__S`
* concrete closure helper paths
* specialized helper payload handling for hot sum types

The desired end state is:

* concrete code uses concrete helpers by default
* erased/universal helpers exist only as fallback

This is the bridge between monomorphization and the Wasm type-erasure reduction work.

---

## Non-Goals

Monomorphization is not responsible for:

* eliminating every use of `Anyref`
* choosing final Wasm struct/array layouts
* removing all universal runtime helper paths
* solving existential-style runtime representation problems by itself

Those are backend/codegen responsibilities.

---

## Practical Rule Of Thumb

If a problem sounds like:

* “why does codegen still see `T`?”

that is a monomorphization problem.

If a problem sounds like:

* “codegen sees `Cell<Int>`, so why is it still using `Anyref`?”

that is a backend representation/helper problem.

Keeping that distinction explicit prevents plan drift and keeps ownership clear between IR
specialization and Wasm codegen follow-up work.
