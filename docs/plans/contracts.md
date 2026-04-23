# Contracts Implementation Plan

## Goal

Introduce contracts as Twinkle's lightweight constrained-polymorphism model,
built on top of existing inherent method resolution.

The initial implementation should make builtin contracts such as `Stringify`
usable in generic bounds and selected syntax hooks, without introducing impl
blocks, instance search, or dynamic dispatch.

The design should be general enough that adding new builtin contracts later is
mostly a matter of defining new contract metadata and syntax hooks, rather than
threading ad-hoc special cases through the checker and lowering.

---

## Scope

### In scope

* builtin contracts, starting with `Stringify`
* single contract bound per type parameter in surface syntax
* bounds on function declarations
* bounds on type declarations
* method lookup on bounded type parameters
* contract satisfaction checks at generic instantiation sites
* contract satisfaction checks for bounded type declarations at type
  instantiation sites
* conditional satisfaction for user-defined generic types through constrained
  inherent methods
* builtin generic types satisfying contracts through the same contract model,
  including `Vector<T>: Stringify` when `T: Stringify`
* contract-backed interpolation
* a compiler-internal contract metadata model that can accommodate additional
  builtin contracts later

### Out of scope for the initial rollout

* user-defined contract declarations
* separate `impl Contract for Type` blocks
* dynamic dispatch through contract values
* associated types or associated constants
* default method bodies
* retroactive conformance for foreign or builtin types outside compiler-owned
  builtin contract metadata and inherent/prelude method rules
* multiple bounds in surface syntax

User-defined contracts may be added later, but they are not required to make the
contracts model useful.

---

## Required MVP Shape

The MVP must support all of the following.

### 1. Builtin contracts

At minimum:

* `Stringify`

with requirement:

```tw
to_string(self) -> String
```

The implementation should model this through compiler-owned contract metadata,
not through interpolation- or type-specific special cases.

### 2. Generic bounds

The checker must understand bounds such as:

```tw
fn show<T: Stringify>(x: T) String {
  x.to_string()
}
```

Inside the function body, the bound makes `to_string()` available on `T`.

### 3. Type declaration bounds

The checker must also understand bounded type declarations such as:

```tw
type Box<T: Stringify> = .{ value: T }
```

This means `Box<Foo>` is only a valid instantiation when `Foo` satisfies
`Stringify`.

This is distinct from conditional contract satisfaction for the enclosing type.
A type declaration bound constrains which type arguments are legal for the type
constructor at all. It does not by itself mean `Box<T>` satisfies `Stringify`.
That still comes from a matching inherent method.

### 4. Satisfaction checking at call sites

When a generic function is instantiated, the concrete type arguments must be
verified against the required builtin contracts.

### 5. Conditional satisfaction for user-defined generic types

This is required for MVP.

Twinkle must support user-defined generic wrappers and containers satisfying
builtin contracts through constrained inherent methods.

Example:

```tw
type Box<T> = .{ value: T }

fn to_string<T: Stringify>(b: Box<T>) String {
  "Box(${b.value.to_string()})"
}
```

The implementation must understand the resulting rule:

* `Box<T>` satisfies `Stringify` when its inherent `to_string` method matches
  the `Stringify` requirement and its own bounds can be satisfied.

Without this, builtin contracts would work only for builtin containers and
monomorphic user types. That would leave generic user-defined containers as
second-class citizens, which is not acceptable for the contracts model.

### 6. Builtin generic types must use the same model

One of the reasons to add contracts is to let builtin generic types participate
without feeling ad hoc.

In particular, the implementation should support cases such as:

```tw
xs: Vector<Int> = [1, 2, 3]
xs.to_string()
```

through the same contract machinery used for user-defined types.

That means `Vector<T>: Stringify` should be established because a matching
builtin or prelude-owned `to_string` method exists with the appropriate bound,
for example conceptually:

```tw
fn to_string<T: Stringify>(xs: Vector<T>) String { ... }
```

The compiler may still source that method from builtin metadata or prelude
signatures, but satisfaction should be phrased in contract terms rather than as
an interpolation-only or container-only special case.

### 7. Interpolation through `Stringify`

Interpolation should typecheck in terms of `Stringify` and lower through the
resolved `to_string` method.

---

## Surface syntax

The MVP supports at most one bound per type parameter in source syntax.

Allowed forms:

```tw
fn id<T>(x: T) T { x }
fn show<T: Stringify>(x: T) String { x.to_string() }

type Box<T> = .{ value: T }
type PrintableBox<T: Stringify> = .{ value: T }
```

Not yet supported:

```tw
fn f<T: A + B>(x: T) { ... }
fn f<T: A, B>(x: T) { ... }
```

The internal representation should still be future-compatible with multiple
bounds even though the parser only accepts zero or one bound initially.

### Grammar sketch

```text
TypeParam     := Ident (":" TypePath)?
TypeParamList := "<" TypeParam ("," TypeParam)* ">"
```

For MVP, the contract name in a bound resolves only to compiler-known builtin
contracts.

---

## AST and resolved representation

### AST

Replace bare type-parameter names with structured type parameters.

Suggested AST shape:

```tw
pub type TypeParam = .{
  name: String,
  bounds: Vector<TypePath>,
  span: span.Span,
}
```

Then use:

```tw
pub type FunctionDecl = .{
  ...
  type_params: Vector<TypeParam>,
  ...
}

pub type TypeDecl = .{
  ...
  type_params: Vector<TypeParam>,
  ...
}
```

Even though MVP only accepts zero or one bound per type parameter, using
`Vector<TypePath>` avoids painting the compiler into a corner.

### Resolved representation

Bounds must survive resolution and module/signature loading.

Suggested resolved shape:

```tw
pub type ResolvedTypeParam = .{
  name: String,
  bounds: Vector<ResolvedContractRef>,
}

pub type ResolvedContractRef = {
  Builtin(BuiltinContract),
}
```

For the current boot compiler, this should be introduced in a way that keeps
most existing code mechanical to migrate:

* `ast.FunctionDecl.type_params` and `ast.TypeDecl.type_params` change from
  `Vector<String>` to `Vector<TypeParam>`
* `resolver.FunctionSig.type_params` changes from `Vector<String>` to
  `Vector<ResolvedTypeParam>`
* `resolver.ResolvedTypeDef` variants change from `Vector<String>` to
  `Vector<ResolvedTypeParam>`

The resolver should also expose helper queries such as:

* extracting only type-parameter names from `Vector<ResolvedTypeParam>`
* looking up a resolved type parameter by name

Those helpers keep existing type-expression resolution and checker code from
becoming unnecessarily noisy during the migration.

Update `FunctionSig` to carry resolved type parameters rather than only their
names:

```tw
pub type FunctionSig = .{
  name: String,
  type_params: Vector<ResolvedTypeParam>,
  param_names: Vector<String>,
  params: Vector<MonoType>,
  ret: MonoType?,
}
```

Likewise update resolved type definitions so type declaration bounds remain
available later:

```tw
pub type ResolvedTypeDef = {
  Record(String, Vector<ResolvedTypeParam>, Vector<ResolvedField>),
  Sum(String, Vector<ResolvedTypeParam>, Vector<ResolvedVariant>),
  Alias(String, Vector<ResolvedTypeParam>, MonoType),
}
```

---

## Contract metadata model

Builtin contracts should live in a dedicated compiler-owned model, not as loose
checker conventions.

Suggested new module:

* `boot/compiler/contracts.tw`

Suggested core types:

```tw
pub type BuiltinContract = {
  Stringify,
}

pub type ContractReturnShape = {
  String,
}

pub type ContractMethodRequirement = .{
  method_name: String,
  receiver_param_count: Int,
  arg_count_without_receiver: Int,
  ret: ContractReturnShape,
}

pub type ContractSpec = .{
  name: String,
  methods: Vector<ContractMethodRequirement>,
}
```

`ResolvedContractRef` should remain part of the resolver-owned resolved type
model rather than being owned by the contracts module. That keeps the contract
metadata layer simple and avoids creating a contracts↔resolver ownership cycle.

For MVP, `Stringify` has one requirement:

* method name `to_string`
* receiver param count `1`
* non-receiver param count `0`
* return type `String`

This representation does not need a full `Self`-type abstraction yet. For the
current MVP, it is enough that contract metadata can answer:

* what method name is required
* how many value parameters it must take
* what return type it must produce

That keeps the first implementation small while still providing a single,
general place to add future builtin contracts.

The contract module should expose queries such as:

* resolve builtin contract name from source spelling
* fetch a builtin contract's spec
* list method requirements for bounded type-parameter lookup
* translate contract-owned shapes into checker expectations where needed

---

## Why user-defined contracts are optional but conditional satisfaction is not

User-defined contract declarations are a language-surface extension. They can be
added later without changing the core semantics of builtin contracts plus bounds.

Conditional satisfaction for user-defined generic types is different. It is part
of the core usefulness of the model. Without it, contracts would not compose
through ordinary user-defined wrappers and containers.

That means the roadmap is:

1. builtin contracts,
2. contract bounds,
3. conditional satisfaction for user-defined and builtin generic types through
   matching methods,
4. user-defined contract declarations only if later needed.

---

## Satisfaction model

Contract satisfaction should be implemented as a generic checker operation, not
as a set of one-off checks.

Conceptually, contract checking must return evidence, not only yes/no success:

```tw
resolve_contract_method(ty, contract, method_name, ctx)
  -> Result<ContractWitness, ContractFailure>
```

or equivalently a contract-satisfaction query that returns structured evidence.

For the current compiler pipeline, that evidence needs to cover both:

* a concrete callable target that is already known now
* a deferred witness that can be finalized once generic specialization has made
  the receiver type concrete

The checker should prove satisfaction using the following evidence sources.

### 1. In-scope type-parameter assumptions

Inside a body such as:

```tw
fn show<T: Stringify>(x: T) String {
  x.to_string()
}
```

`T` is assumed to satisfy `Stringify`.

This is what makes bounded type-parameter method lookup work.

### 2. Builtin satisfaction rules

Compiler-known primitive cases should be expressed through the same contract API.
For MVP this includes at least:

* `Int: Stringify`
* `Float: Stringify`
* `Bool: Stringify`
* `Byte: Stringify`
* `String: Stringify`

These primitive cases may be represented internally either as explicit builtin
contract-membership facts or as compiler-owned witness methods, but all queries
should still go through the same contract-checking API.

Builtin generic types such as `Vector<T>` should also be handled through this
same satisfaction path, but established by a matching method witness rather than
by interpolation-specific special cases.

### 3. Inherent or registered method witnesses

A type satisfies a builtin contract when a matching method can witness the
required contract method and all instantiated bounds introduced by that witness
can themselves be satisfied.

Contract witness lookup should reuse the existing resolved single-method lookup
model. The checker should not introduce overload search, global instance search,
or backtracking across multiple candidates.

For `Stringify`, a witness method succeeds when:

* the resolved method name is `to_string`
* the method has exactly one value parameter, the receiver
* the method returns `String`
* the receiver parameter unifies with the type being checked
* any bounds introduced by method instantiation can be recursively satisfied

This is the crucial rule that allows both:

* `Box<T>: Stringify` when `fn to_string<T: Stringify>(Box<T>) String`
* `Vector<T>: Stringify` when the builtin or prelude-owned `Vector.to_string`
  method has the corresponding bound

The preferred implementation path for builtin generic conformance is an ordinary
registered method signature with bounds, such as a builtin- or prelude-owned
`Vector.to_string<T: Stringify>` witness, rather than a one-off hardcoded rule
for `Vector`.

---

## Operational algorithm for contract satisfaction

The checker should use an explicit proof procedure.

Given `satisfies_contract(ty, Stringify)`:

1. If `ty` is a type variable with an in-scope `Stringify` assumption, succeed.
2. If `ty` matches a builtin compiler-known satisfied case, succeed.
3. Find a candidate registered method that could witness the contract method.
4. Load that method's `FunctionSig`.
5. Freshen the method's type parameters with fresh MetaVars.
6. Validate the method's shape against the contract requirement.
7. Unify the witness receiver parameter with `ty`.
8. For each bound on the witness method's instantiated type parameters,
   recursively prove that bound.
9. If all recursive proofs succeed, the contract is satisfied and yields a
   witness.
10. Otherwise, return a contract failure describing the first relevant reason.

### Witness forms

For the current compiler, contract proof should produce one of two witness
shapes:

* **concrete witness** — the target function is already known and can be stored
  the same way ordinary method calls are stored today
* **deferred witness** — the target depends on generic specialization and must
  be finalized once the receiver type has become concrete

This distinction is critical for method calls inside generic bodies such as:

```tw
fn show<T: Stringify>(x: T) String {
  x.to_string()
}
```

Inside `show`, the checker can prove that `T` supports `to_string`, but it
cannot yet choose a single final concrete function target. The plan therefore
must carry deferred witness information forward rather than forcing early
concrete resolution.

### Cycle handling

Recursive satisfaction needs a visited set keyed by the type and contract being
proved.

For MVP, it is acceptable to use a conservative rule:

* if the checker re-enters the same `(type, contract)` proof while it is already
  active, fail that proof rather than looping forever

This can be improved later if needed.

---

## Compiler Work

### Parser

Add parsing support for a single bound per type parameter in function and type
declarations.

Diagnostics should include at least:

* `expected contract name after ':'`
* `multiple bounds are not supported yet`

### Resolver and signature model

Add support for resolving builtin contract bounds on generic parameters.

The internal function-signature representation needs to carry:

* resolved type parameters
* bounds per type parameter

Resolved type definitions also need to carry bounds so type declaration
constraints remain available after resolution.

The resolver should reject unknown contract names and preserve bounds across
exports/imports and signature loading.

### Signature loading

`prelude/signatures/*.tw` and actual `prelude/*.tw` files are part of the same
story.

The signature loader must preserve bounded type parameters so builtin and
prelude-owned generic methods can witness contracts through the same mechanism
as user-defined methods.

This is especially important for cases like `Vector<T>: Stringify`, where the
method may be compiler-owned or prelude-owned but should still feed the same
contract satisfaction engine.

The first bounded builtin-generic witness may come from either signature files
or actual prelude modules, whichever already owns the method in the current
compiler. The key requirement is that both loading paths preserve bounds into
`FunctionSig`, and that the chosen path ultimately provides a real callable
implementation target rather than only a surface signature.

### Type checker

The checker needs five key additions.

#### Bounded type parameter method lookup

When checking code under a bound such as `T: Stringify`, method lookup on `T`
must succeed for the methods required by `Stringify`.

This lookup should come from contract metadata, not from pretending there is a
concrete receiver type in the global method registry.

#### Instantiation checks for bounded generic functions

At call sites, inferred or explicit type arguments must satisfy all required
contract bounds on the instantiated function type parameters.

Bound validation should happen after ordinary call-site unification has solved
instantiated type arguments as far as possible, so contract checks see the most
concrete type arguments available.

#### Bounded type declaration checks

When a type declaration has a bounded type parameter, each concrete type
instantiation of that declaration must satisfy the declared bound.

Example:

```tw
type Box<T: Stringify> = .{ value: T }
```

Then `Box<Foo>` is only legal when `Foo: Stringify`.

This is required MVP semantics: all concrete instantiations must be checked.

For the current boot compiler, the first implementation hook may begin at named
construction sites, annotations, and other explicit named-type instantiation
paths already owned by the checker, but the feature should not be considered
complete until all concrete instantiation paths are covered.

The important point for the plan is that the resolved type definition must
retain the bounds so this enforcement is possible without redesign.

#### Conditional satisfaction

When checking whether a concrete type satisfies a builtin contract, the checker
must use the generic satisfaction procedure described above.

That is the crucial step for cases like:

* `Box<T>: Stringify`
* `Vector<T>: Stringify`

The same witness logic should work for both user-defined and builtin generic
types.

For builtin generic types, the implementation must choose an actual witness
source. In particular, `Vector<T>: Stringify` should be backed by a real
`Vector.to_string<T: Stringify>` implementation path, either via prelude-owned
code plus signatures or via a compiler-owned builtin callable target. A
signature stub alone is not sufficient.

#### Interpolation checks

Interpolation should call the same contract satisfaction logic rather than using
its current ad-hoc type switch.

Interpolation is not the only lowering-sensitive use case. Ordinary method calls
on bounded type parameters in generic bodies need the same witness story.

### Lowering

Interpolation lowering should use the resolved `to_string` witness selected by
the checker.

Lowering should not re-derive contract satisfaction from the interpolated type.
Instead, the checker should record enough metadata for each interpolation
expression to let lowering emit the exact approved call target.

In the current boot compiler, this should mirror the existing method-call
metadata flow rather than inventing a second lowering-time contract resolver.
A small interpolation-witness table in the checker result is preferable to
reconstructing contract reasoning in Core lowering.

More generally, contract-backed method calls in generic bodies need a
pre-monomorphization representation. The current concrete `func_name`-only
method-call metadata is sufficient for already-resolved receivers, but not for
calls such as `x.to_string()` when `x: T` and `T: Stringify`.

For that case, the plan should carry deferred contract witnesses until generic
specialization makes the receiver concrete enough to choose the final target.
Resolving those deferred witnesses during monomorphization is the preferred MVP
fit for the current pipeline.

No separate runtime representation for contracts is needed.

### Monomorphization

No dynamic dispatch is introduced. Once bounds are checked, the existing
monomorphization pipeline should continue to generate concrete code for the
resolved functions.

---

## Lowering data flow

The current interpolation lowering uses ad-hoc primitive cases. That should be
replaced with checker-provided witness data.

Suggested approach:

* when checking interpolation of a non-`String` expression, prove
  `Stringify(expr_ty)`
* record the resolved witness function for that interpolation expression
* in Core lowering, emit a call to that recorded witness

This mirrors the existing method-call metadata flow and keeps lowering free of
contract reasoning.

The same principle applies to ordinary contract-backed method calls in generic
bodies. Those calls should lower through checker-provided contract witness data,
with deferred witnesses surviving until monomorphization if the final callee is
not yet concrete.

---

## Diagnostics

Diagnostics should be phrased in contract terms.

Contract-proof code should produce structured internal failure reasons and only
format them into user-facing diagnostic text at the reporting boundary, rather
than assembling ad-hoc strings throughout the proof logic.

Examples:

* `type Buffer does not satisfy Stringify: missing to_string(self) -> String`
* `type User has to_string(self) -> Byte, expected String for Stringify`
* `type Box<Foo> does not satisfy Stringify because Foo does not satisfy Stringify`
* `type argument Foo does not satisfy Stringify required by Box<T: Stringify>`

Diagnostics are especially important once conditional satisfaction enters the
picture.

---

## Tests

### Positive tests

* parser preserves bounded type parameters on functions and types
* primitive `Stringify` bounds
* bounded type-parameter method lookup inside generic functions
* user-defined monomorphic type satisfying `Stringify`
* user-defined generic wrapper satisfying `Stringify` under bounds
* builtin `Vector<T>` satisfying `Stringify` when `T: Stringify`
* legal instantiation of bounded type declarations
* interpolation of values whose types satisfy `Stringify`

### Negative tests

* unknown builtin contract name in a bound
* bounded generic call with unsatisfied `Stringify`
* illegal instantiation of `type Box<T: Stringify>` with a non-`Stringify` type
* type with wrong `to_string` return type
* generic wrapper whose inner type does not satisfy `Stringify`
* builtin `Vector<T>` stringification when `T` does not satisfy `Stringify`
* interpolation of non-`Stringify` values
* recursive contract proof does not loop forever

---

## Suggested rollout order

### Step 1 — Add compiler-owned builtin contract metadata

Create a dedicated contract metadata module and define builtin `Stringify` with
requirement:

```tw
to_string(self) -> String
```

To avoid a contracts↔resolver ownership cycle, keep the metadata model minimal
for MVP. In particular, do not require the contracts module to depend on full
resolver-owned `MonoType` machinery if a smaller contract-owned shape is
sufficient.

Hook interpolation checking to this metadata rather than ad-hoc type cases.

### Step 2 — Add bounded type parameter syntax and AST support

Support:

```tw
fn show<T: Stringify>(x: T) String { ... }
type Box<T: Stringify> = .{ value: T }
```

The parser and AST should preserve structured type parameters with future-ready
bound storage.

### Step 3 — Preserve bounds in resolution and signature loading

Teach the resolver and signature loader to resolve builtin contract bounds and
carry them in function signatures and type definitions.

This step should also add migration helpers for the current compiler shape,
especially helpers that recover plain type-parameter names from structured
resolved type parameters so existing resolution and checker code can be updated
incrementally.

### Step 4 — Teach the checker bounded type-parameter lookup and function bound checks

Inside function bodies, `x.to_string()` should typecheck when `x: T` and
`T: Stringify`.

Calls to bounded generic functions should fail clearly when the instantiated
type does not satisfy the required contract.

For the current boot checker, this likely means replacing the existing
`type_var_scope: Vector<String>` view with a scope that preserves bounds, while
still exposing helper accessors for plain type-variable names where older code
expects them.

### Step 5 — Enforce bounded type declaration instantiation

Whenever a bounded type declaration is instantiated with concrete type
arguments, the checker must verify those arguments satisfy the declared bounds.

This is required MVP semantics. The first implementation may start from the
narrowest reliable named-type instantiation hooks in the current checker, but
MVP is not complete until all concrete instantiation paths are covered.

### Step 6 — Implement conditional satisfaction through witness methods

This step is mandatory for MVP, not a follow-up.

The checker must recognize that a generic method with matching bounds allows the
receiver type to satisfy the contract conditionally.

This is the step that makes both user-defined wrappers and builtin generic types
such as `Vector<T>` participate in the same contract model.

For builtin generic types, the intended shape is still a normal witness method
with bounds, not a special contract rule attached directly to the type.

This step should also introduce the checker and IR metadata needed to represent
contract witnesses before monomorphization, including deferred witnesses for
generic bodies where the final concrete callee is not yet known.

### Step 7 — Lower through resolved and deferred witnesses

Once checking is contract-based, lowering and monomorphization should emit calls
through the witness selected by the checker rather than using
container-specific or primitive-only special cases.

This applies both to interpolation and to ordinary contract-backed method calls
inside generic bodies.

---

## Bootstrap considerations

The boot compiler is the primary implementation target, but bounded generic
syntax and contract metadata affect parser, resolver, and signature-loading
behavior that may also matter for bootstrap.

If the implementation changes syntax or prelude/signature files in ways the Rust
stage0 must understand in order to keep bootstrapping working, update `src/`
accordingly.

---

## Follow-ups

Once the MVP is stable, Twinkle can evaluate:

* additional builtin contracts such as `Slice` or `IntoIterator`
* user-defined contract declarations
* better syntax for multiple bounds if needed
* richer cycle handling or proof caching for recursive contract satisfaction

These are follow-ups. They should not block the core contracts rollout.
