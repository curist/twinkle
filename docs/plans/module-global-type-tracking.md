# Module-global identity and type tracking

## Status

Proposed. Motivated by an attempt to expose LSP `CompletionItemKind` values as
module-level `pub` value bindings (`kinds.constant`). That refactor shipped as
functions (`kinds.constant()`) instead, because adding a module of `pub` value
constants triggers the bug described here. See `boot/lib/lsp/kinds.tw`.

This plan supersedes an earlier draft that proposed an exports-only
`linked_global_types` map (keyed by linked global id, but sourced only from
exports). Three independent reviews converged on a deeper repair: the root cause
is that **module globals reuse `LocalId`**, so the fix is to give globals a
distinct identity through the IR, not to patch the lookup. The exports-only
source and the existing source-local-keyed resolution are recorded under
"Rejected alternatives".

## Symptom

Adding a new module whose only exports are module-level `pub` value bindings
(e.g. a dozen `pub constant := 21`) can make a previously-building program fail
to compile, with an error about an **unrelated** global:

```
backend verifier: function run_help_command (F310x): call arg 0 to format_help
  has mono Void, expected Named(T328)  [arg=AGlobalLocal(L59, mono=Void)]
```

`L59` here is the top-level `cli` record in `boot/main.tw` — nothing to do with
the new module. Adding the new globals shifted the global-id layout enough to
expose a latent defect.

## Root cause

Module globals reuse `LocalId` for their identity, so two distinct id spaces are
conflated:

- **Linked global ids** — program-wide unique, assigned by `core_linker`
  (`core_linker.tw`, "Step 2b: Assign globally unique GlobalLocal IDs"). After
  linking these ride inside a `LocalId` on `GlobalLocal(id)` reads and on the
  init-function `Assign` targets that store each global.
- **Per-module source LocalIds** — unique only *within* a module, carried by
  every prepared slot as `SlotInfo.source_local`. Ordinary (non-global) init
  locals keep these.

Because both are `LocalId`, the backend cannot tell them apart by type; it has to
*guess* structurally which slots are globals. Module-level initializers from
different modules are prepared together (each module keeps its own `is_init`
function; the linker builds a combined init wrapper, `CombinedInit` in
`core_linker.tw`). Their init-slot facts are then aggregated into one
module-global map keyed by `source_local.id`. Since each source module numbers
its locals independently before linking, an unrelated ordinary init local can
share a `source_local.id` with another module's linked global id.

### The exact injection point

The linker remaps init `Assign` *targets* through `global_remap`
(`core_linker.tw:779-784`, gated by `remap_assign_targets = is_init`), so a
global-write slot's `source_local.id` ends up *equal to* its linked global id.
That coincidence is the only reason the current scheme works at all.

It breaks in `wasm_plan_impl.tw:173-181`: a fallback pass walks **every** init
slot — including ordinary locals whose `source_local.id` was never remapped —
and, if that id appears in `scan.module_globals` (a linked-gid set populated from
`AGlobalLocal` reads, `wasm_plan_scan.tw:171`), records that local's `mono`
(often `Void`) as the global's type. First-writer-wins then picks the wrong slot.

Codegen tolerates the wrong type because module globals are stored **anyref**: a
`Void`/`i31` declared type still holds a record via boxing, and reads cast back.
The verifier is the first strict consumer, so it rejects first — but the
imprecise mono also drives the global's repr to `OpaqueAnyref`, so this is not
mere verifier strictness.

### Latent soundness concern

The real hazard is local/global collision, not two distinct globals aliasing.
After linking, genuine global stores are remapped to distinct linked ids, so two
actual globals do not share an id. But an *ordinary* init local can have
`source_local.id == some linked GlobalId`, and because the backend cannot tell
locals from globals by type, that collision **poisons** the global's metadata:

- the planner may record the ordinary local's mono (often `Void`) as the
  global's type;
- the emitter may treat that ordinary init assignment as a module-global write
  and emit a spurious `GlobalSet(global_local_<n>)` to an unrelated global.

The observed case stopped at metadata poisoning (the colliding local was not
live), but a spurious write to a live unrelated global is not precluded by the
identity scheme.

## Design principles

Three principles, in priority order:

1. **Globals get a distinct identity type.** Reusing `LocalId` is what made the
   bug easy to introduce; a distinct `GlobalId` makes any local/global mix a type
   error rather than a silent id collision.
2. **Codegen emits from a single source.** The repr and type a global is emitted
   with must come from one place, so a separate metadata table can never drift
   from what is actually emitted.
3. **The verifier checks against an independent oracle.** The verifier must not
   read the same value codegen used to emit, or it can only confirm consistency,
   not correctness — which is exactly the failure that started this.

## Proposed fix

### 1. Introduce `GlobalId`

Add `pub type GlobalId = .{ id: Int }` to `core_ir.tw` and use it for global
identity throughout:

- `CoreExprKind.GlobalLocal(GlobalId)` (currently `LocalId`).
- The prepared/ANF read atom `AGlobalLocal(GlobalId)`.
- A new global-**store** form (below).
- The verifier context and the wasm registry global entries.

This is mechanical but wide; do it first so the type checker enforces the rest.

### 2. Give global stores a distinct node

Today a top-level value store is `Assign(LocalId, value)` whose target the linker
remaps to a global id — indistinguishable from an ordinary local store. Replace
it with an explicit store form keyed by `GlobalId`, mirroring the existing
`GlobalLocal` read, and carry that identity through the whole pipeline so codegen
never has to recover it:

- Core: `CoreExprKind.GlobalSet(GlobalId, CoreExpr)`
- ANF: `Atom.AGlobalLocal(GlobalId)` (read) and `AnfOp.AGlobalSet(GlobalId, Atom)`
  (store)
- Prepared: `PreparedAtom.AGlobalLocal(GlobalId)` and a prepared global-store op

This also removes a second heuristic: today ANF has no global read atom — global
reads ride as `Atom.ALocal(id)` and only become `PreparedAtom.AGlobalLocal`
during slot assignment, when no slot can be found for the id
(`slot_assign.tw:246`). The new design carries `AGlobalLocal` from lowering, so
slot assignment no longer infers globals from missing slots. The lowerer/linker
emits the store form for module-level value initializers; the linker assigns the
`GlobalId` at the point it already computes `global_remap`.

With reads and writes both keyed by `GlobalId`, the structural guessing layer
becomes dead and is **deleted**:

- `facts.module_global_assign_monos` and `collect_let_source_lids` (the
  let-source heuristic),
- the `wasm_plan_impl.tw:173-181` scan-membership fallback,
- the cross-init aggregation keyed by `source_local.id`,
- the `slot_assign.tw:246` "no slot found → it must be a global" inference.

The registry is built by scanning `GlobalSet` nodes, keyed by `GlobalId`, with
the type taken from the store's already-`type_remap`'d value mono. `emit.tw`'s
`GlobalSet` instruction keys off the node's `GlobalId` instead of
`tgt.source_local.id` (`emit.tw:955`).

This closes the runtime aliasing gap by construction: identity is the linked
`GlobalId` everywhere, so distinct globals can never share a Wasm global.

### 3. Build the verifier oracle in the linker

Have `core_linker` produce, **for all defined globals** (not just exports):

```tw
linked_global_types: Dict<Int /* GlobalId.id */, MonoType>
```

- Source it from `defined_global_ids[m.path]`, which covers
  `0..m.defined_global_count` — **not** from `value_exports`/`global_value_ids`,
  which hold only `pub` names. A private top-level value referenced elsewhere in
  its module still appears as a `GlobalLocal`; sourcing from exports would leave
  it with no entry (and regress it through the
  `"no init function has slot for module global"` path).
- Apply the module's `type_remap` (`remap_mono_type`, `core_linker.tw:596`) to
  every stored mono, or the map is correctly keyed by linked id but carries
  pre-link, module-local `TypeId`s.
- The per-global types come from each module-level value binding recorded during
  lowering/checking. If `CompiledModule` does not already carry them, add
  `module_global_types: Dict<Int /* module-local global id */, MonoType>` and
  populate it for public **and** private bindings.

Thread `linked_global_types` `link → monomorphize → lower_anf → prepare` onto
`PreparedModule`. Module-global values must be concrete at backend time; assert
this when threading through monomorphize rather than assuming it (see Validation).

### 4. Wire consumers per principle

- **Codegen** (`wasm_plan_impl.tw`, `emit.tw`): derive each global's repr and
  emitted type from its `GlobalSet` node — one source for what is emitted.
- **Verifier** (`verify.tw` `build_module_globals`): resolve the *expected* type
  from `linked_global_types` and check the `GlobalSet`/`AGlobalLocal` mono
  against it. The oracle is independent of the slot mono codegen used, so a wrong
  slot mono is caught instead of rubber-stamped.

When this lands, the planner and verifier share one helper for locating global
stores, but each pulls its type from the side appropriate to its role
(emit-source vs. oracle). This is the shared helper the original `kinds` review
asked for; the earlier extraction shared only control flow while still resolving
via `source_local.id`, so it resolved the wrong type.

## Rejected alternatives

- **Relax the verifier** (skip the mono check for global operands): the
  following repr check still fails (`OpaqueAnyref` into a `TypedRef` param),
  because codegen derives repr from the same wrong mono. Relaxing both masks
  genuinely malformed codegen — and removes the oracle (principle 3).
- **Prefer non-`Void` slots when disambiguating**: in the observed case *both*
  colliding slots are `Void`; the real type is in neither.
- **Functions instead of values** (`kinds.constant()`): the current workaround.
  Sidesteps module globals entirely; does not fix the defect or help genuine
  module-level `pub` value constants elsewhere.
- **Exports-only `linked_global_types`**: the earlier draft. Fixes the
  `kinds.constant` case but regresses private module globals (no entry → error).
  Superseded by sourcing from all defined globals (step 3).
- **`linked_global_types` as codegen's input, keyed by `source_local.id`**: keeps
  the local/global id overload and a second type source that can drift from the
  emitted repr. Superseded by `GlobalId` + emit-from-node (steps 1, 2, 4).

## Files (expected)

- `boot/compiler/core_ir.tw` — `GlobalId`; `GlobalLocal(GlobalId)`;
  `GlobalSet(GlobalId, CoreExpr)`; carry types on the linked module type.
- `boot/compiler/core_linker.tw` — assign `GlobalId`s; emit `GlobalSet`; build
  `linked_global_types` over all defined globals with `type_remap` applied.
- `boot/compiler/anf.tw` — `Atom.AGlobalLocal(GlobalId)` and
  `AnfOp.AGlobalSet(GlobalId, Atom)`.
- `boot/compiler/lower_anf.tw` — lower `GlobalLocal`/`GlobalSet` to the new ANF
  atom/op directly (instead of `ALocal`/`AAssign`).
- `boot/compiler/backend/prepared_ir.tw` — `PreparedAtom.AGlobalLocal(GlobalId)`
  and the prepared global-store op.
- `boot/compiler/backend/prepare.tw`, `boot/compiler/backend/slot_assign.tw` —
  thread `linked_global_types` to `PreparedModule`; carry `AGlobalLocal` through
  rather than recovering it from missing slots.
- `boot/compiler/codegen/wasm_plan_impl.tw`, `boot/compiler/codegen/wasm_plan_scan.tw`,
  `boot/compiler/codegen/emit.tw` — build the registry from global-store ops;
  delete the source-local fallback; emit by `GlobalId`.
- `boot/compiler/backend/facts.tw` — delete `module_global_assign_monos` and the
  let-source heuristic; add the shared global-store locator.
- `boot/compiler/backend/verify.tw` — resolve expected types from
  `linked_global_types`; check against the store/read mono.
- Rust stage0 (`src/`) — only if needed to keep `boot/main.tw` bootstrapping (see
  Bootstrap note).

## Validation

- **Reproduce first (regression gate):** switch `boot/lib/lsp/kinds.tw` back to
  `pub <name> := <n>` value bindings and update `completion.tw` to
  `kinds.constant` (no parens); confirm `target/twk build boot/main.tw` fails as
  in the symptom.
- **Concreteness check (resolved — keep the assert):** the checker has no
  let-generalization (no `generalize`/`Forall`/`Scheme`; `instantiate` runs only
  on explicit `FunctionSig` type params), so top-level `:=` bindings get a single
  monomorphic inferred type. Any binding whose type stays open (e.g. `pub xs := []`)
  is rejected at type-check time: the final zonk pass (`checker.tw:5114-5145`)
  emits `AmbiguousType` for any `type_map` entry still holding an unsolved
  `MetaVar`, before link/monomorphize/backend ever run. The "concrete at backend
  time" invariant is therefore guaranteed upstream — encode it as an assert, not
  as defaulting/error logic.
- **IR survival:** confirm `GlobalId`, `GlobalLocal`, and `GlobalSet` pass
  through monomorphize and `lower_anf` unchanged, like the current `GlobalLocal`
  read does.
- **Focused tests:**
  - a multi-module program reading a module-level `pub` *record* value from
    another module's top level (exported-global path);
  - a module with a *private* top-level record value used by a public function or
    another top-level initializer (private-global path);
  - another module with many `pub` constants to perturb the linked/global/local
    id layout (collision path).
- **Full loop:** self-host to fixed point (`make bundle-cli`), boot suite
  (`make boot-test`), Rust suite.
- Once green with value bindings, convert `kinds` back to the value form.

## Bootstrap note

`target/twk`'s embedded verifier rejects the value-binding program, so it cannot
build a fixed compiler from the new source (chicken-and-egg). Bootstrap through
Rust stage0: `cargo build --release` then
`./target/release/twk build boot/main.tw -o stage1.wasm` produces a compiler
carrying the fix, which then drives the self-host loop.
