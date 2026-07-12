# Design Rationale — Why Static, Annotation-Free Uniqueness

**Status:** Draft (rationale)

Why this project takes the shape it does: a static, annotation-free, no-runtime-
overhead ownership analysis, rather than runtime reference counting or user-
visible ownership types. This is the "why this approach / is it even possible"
doc; the *how* lives in [fact-lattice.md](fact-lattice.md) and
[summary-specialization.md](summary-specialization.md).

## Is sound uniqueness achievable, or a research-hard problem?

Achievable — because the hard-sounding part is not the part we actually need.

- **Soundness is the easy half, and it is structural.** The trivial sound
  analysis is "never mutate, always persistent": 100% sound, 0% coverage. Every
  rule we add is a *proof that lets us beat that floor*; a missing proof leaves us
  on the floor. So the worst failure mode of this design is **a slower program,
  never a wrong one.** Soundness is not the research problem.
- **Coverage is the real work, and it is a gradient, not a cliff.** "How much real
  code reaches the fast path" grows pattern by pattern, each sound. We are never
  asked to answer one undecidable yes/no before getting anything.
- **The undecidable core is not a prize problem.** *Perfect* alias/pointer
  analysis is a Rice's-theorem impossibility — but nobody needs perfect. Sound
  *conservative* approximation of aliasing/escape is routine, shipped compiler
  technology (JVM and Go do escape analysis in every build).

**It has shipped, repeatedly:** Clean (uniqueness typing), Koka (Perceus/FBIP),
Roc (opportunistic mutation), Lean 4 (RC-based in-place arrays), Rust (the borrow
checker as a sound static aliasing analysis), MLKit (region inference). "Can a
compiler soundly mutate immutable-looking data?" is answered yes many times over.

## Why static, and not runtime reference counting

Koka/Roc/Lean get high-coverage in-place reuse from **runtime reference counting**
— and the reason is that RC *is already their memory manager*. Every heap object
carries a refcount because that is how they decide when to free it. Given that,
the in-place trick is nearly free: at an update site, emit
`if refcount == 1 { mutate } else { copy }` (Perceus). They are reading a number
they already maintain, and checking it at runtime is *more* powerful than static
analysis (a value unique on this path but shared on another still gets mutated).

**Twinkle cannot copy this, because the platform removes the primitive it needs.**
Twinkle targets **WebAssembly GC** — a host-provided *tracing* collector. Wasm GC
objects carry **no refcount**, and there is **no way to ask "how many references
point at this object."** The runtime check their whole technique is built on does
not exist here. To imitate them, Twinkle would have to bolt its own reference
counting on top of the tracing GC — paying RC traffic *and* tracing GC, a large
runtime fighting the reason we chose Wasm GC (tiny runtime, no allocator to write,
host portability).

So static analysis is not us stubbornly ignoring an easy RC path. It is the route
the platform leaves open that keeps the runtime small and overhead-free. RC-based
reuse is off the menu; static inference is what is left.

## The tradeoff triangle

Three properties are all desirable, and every language sacrifices one:

- **annotation-free** — no user-written ownership types;
- **zero runtime overhead** — no refcounts, flags, or dynamic checks;
- **high/complete coverage** — most/all eligible updates go in place.

| Language | Sacrifices |
|---|---|
| Koka / Roc / Lean | zero runtime overhead (they pay RC) |
| Rust / Clean | annotation-free (ownership is in the types) |
| **Twinkle** | **complete coverage** (conservative static, sound fallback) |

Twinkle wants annotation-free *and* zero-overhead, so the property that gives is
coverage — paid down conservatively, backed by the persistent fallback. That is a
deliberate, coherent corner, not a mistake; the RC clan sits elsewhere because
their runtime already spent the overhead budget.

## What immutable value semantics buys us

Immutability is the single biggest thing working in our favor — it makes the
analysis *tractable*, and it is why the lattice is small.

In a language with mutable cells, the expensive, near-intractable part is
**may-alias-and-write**: two references to one mutable location, a store through
one observable through the other at some later time. Almost all of alias
analysis's difficulty lives there.

Twinkle's default surface is immutable — every "mutation" is a rebind producing a
new value — with mutation **confined to one explicit escape hatch, `Cell<T>`** (a
genuine mutable box: `Cell.set` overwrites in place). That confinement is the key:
because mutation exists only through a distinguished type with distinguished
operations, the may-alias-and-write hazard is **localized and syntactically
identifiable** (any `cell$*` call), not pervasive as in C/Java/ML where *any*
reference could be mutated. For everything that does not flow through a `Cell`, the
only aliasing is *sharing of immutable values*, which is **benign**: no one can
observe a change through an alias, because nothing changes. That collapses the
soundness condition to a single clean question: **will a live reference read the
pre-update version?** That is an **ownership + liveness** question — standard,
decidable, cheap dataflow — not a may-alias-and-write one.

`Cell` is handled conservatively so the hazard stays quarantined: **storing into a
Cell publishes the value** (`Cell.new`/`set`/`update` → the stored value becomes
`Shared`, since a Cell is aliasable and readable at arbitrary times), and **reading
from a Cell yields a non-owned value** (`Cell.get` → `Unowned`, the contents stay
aliased through the live cell). Because no Cell-content value can enter the
owned-mutable path without going through those two rules, Cell's time-varying
contents never contaminate the immutable ownership reasoning. Cell is *not* an
optimization target — it is already mutable by design; the analysis only models its
effects to stay sound around it. The advantage immutability buys is therefore
localization of the hazard, not its elimination — but a localized, identifiable
hazard is still a decisive edge over pervasive mutation.
Immutability is precisely why the fact lattice is small — roughly
owned / shared / moved rather than a heavyweight points-to graph (the precise
five-element lattice, incl. the `Unowned` top and the
`OwnedPersistent`/`OwnedMutable` split, is in [fact-lattice.md](fact-lattice.md)). (This is the classic uniqueness-typing
insight: purity is the precondition that makes "unique ⇒ safe to destroy" *true*.)

Two honest boundaries on that advantage:

1. **It is an edge over *impure* languages, not over the RC clan.** Koka/Roc/Lean
   are also pure/immutable — that is *why* their `refcount == 1` check is sound.
   Immutability is a shared precondition across the pure-FP family, table stakes,
   not a Twinkle-specific edge over them.
2. **It gives soundness cheaply, not coverage.** Immutability removes the alias
   nightmare but not the residual work — proving unique ownership / old-version-dead
   across branches, loops, and *calls without annotations*. Nested ownership also
   survives: a fresh immutable shell around a shared immutable inner is still
   "shell owned, inner shared" — immutability makes reading the shared inner
   harmless, but you still cannot destroy it.

## The two hard parts

There are two independent hard parts, and it clarifies everything to separate them:

- **the alias-analysis hard part** — reasoning about aliased writes;
- **the coverage hard part** — proving uniqueness precisely, annotation-free, with
  no runtime help.

| | removes alias part | removes coverage part |
|---|---|---|
| RC clan (Koka/Roc/Lean) | ✅ immutability | ✅ runtime refcount |
| impure + zero-overhead (hypothetical) | ❌ | ❌ |
| **Twinkle** | ✅ **immutability** | ❌ (static, no RC) |

**Immutability removes the first; RC removes the second.** The RC clan removed
both. An impure zero-overhead language would face both — the worst quadrant.
Twinkle removed the first via immutability and kept the second (static, no
refcount to lean on). (More precisely: immutability removes the alias-analysis
hazard for the immutable majority; `Cell<T>` reintroduces a *localized* mutable
hazard, handled conservatively — store publishes, read yields non-owned — so it
stays quarantined rather than reopening general alias analysis.) So we are
strictly better off than the impure-zero-overhead route, and exactly *one* hard
part behind the RC route — the coverage part — which
the rest of this plan is built to chip away at, soundness free the whole way.

## Honest framing

The difficulty of this project is partly **downstream of choosing Wasm GC.** That
choice bought real things — no GC/allocator to write, host-portable, small runtime
— and its bill is: no free RC-reuse trick, so in-place performance requires the
hard static route. It is a self-consistent bet, not a blunder, but the hardness is
a *consequence of the platform*, not free-standing ambition.

And the right-quadrant existence proof is real: **JVM and Go escape analysis** is
static, annotation-free, running on a tracing GC, and ships in every build. It is
more limited than our goal (mostly stack allocation and scalar replacement, not
destructive update of unique escaping heap objects), but it proves the "static +
inferred + tracing GC" quadrant is not empty — it is the quadrant where you win
*some* cases conservatively rather than *all* cases dynamically.

## Prior art and further reading

No single paper is a blueprint — the coverage set is Twinkle-specific (census,
not literature). But every sub-problem has a canonical, production-proven
technique, and each is already folded into the design under its own vocabulary.
So this is a short reading list, not a spec — one anchor per sub-problem, keyed
to where it lands:

| Sub-problem | Canonical source | Where it lands |
|---|---|---|
| Uniqueness / linearity — the "unique ⇒ safe to destroy" license | Clean uniqueness typing (Barendsen & Smetsers); Linear Haskell (POPL 2018) | the mutate-license soundness argument |
| Escape analysis on a tracing GC — the quadrant we live in | Choi et al. / Blanchet (OOPSLA 1999); Go's escape analysis | transfer function + lattice ([fact-lattice.md](fact-lattice.md)) |
| Region inference — static, inferred ownership | Tofte & Talpin (1997); MLKit | interprocedural summaries ([summary-specialization.md](summary-specialization.md)) |
| Borrowing / loop-carried lifetimes | Rust NLL / Polonius; RustBelt (POPL 2018) | back-edge fixpoint, borrow-vs-publish |
| RC-based reuse — why we *can't* copy it | Perceus (PLDI 2021), FBIP | the no-runtime-RC argument above |
| Build-in-place (destination-passing) | DPS (Shaikhha et al., FHPC 2017) | the freeze/accumulator shape |

Notably **absent: points-to / alias analysis** (Steensgaard, Andersen). That is
precisely the machinery immutability lets us skip — it would matter only for the
quarantined `Cell<T>` hazard, which two conservative rules handle instead of a
points-to graph. The give-up-and-stay-persistent default keeps the whole thing
conservative.

## Bottom line

Static, annotation-free uniqueness is real and tractable, not imaginary and not a
Clay problem. Soundness is structural here (the fallback guarantees it); the open
work is coverage, an ordinary if substantial dataflow-engineering gradient with
production existence proofs behind every ingredient. Immutability is what keeps us
out of the worst quadrant and makes the analysis ownership-and-liveness rather than
full alias analysis. The one genuinely ambitious bet — static, inferred,
annotation-free, no runtime RC — is forced by targeting Wasm GC, and its downside
is "less speedup than hoped," measurable in advance from printed facts, never wrong
answers.
