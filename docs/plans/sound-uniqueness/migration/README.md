# Migration Track

**Status:** Planned; starts after existing-hook codegen lowering proves the
analysis/codegen seam.

This track owns cleanup and consolidation. It should not be the first mutable
lowering implementation. First, the codegen track should prove that ownership
facts can safely drive today's existing hooks.

## Track invariants

- Migration does not create a second legality path. Ownership facts and decisions
  remain the only source of mutability legality.
- Existing hooks become implementation details behind a shared internal contract.
- User-facing immutable APIs remain unchanged.
- `collect` and other semantic builder uses must keep working independent of
  optimization.

## Phase 7 — Mutable-intrinsic migration and hook cleanup *(architecture: 2D)*

- [ ] **Define the compiler-private intrinsic family.** Finalize internal
  operations such as `begin`/`read`/`write`/`append`/`remove`/`freeze`, operand
  encoding, ANF annotation vs side-table representation, and proof/debug ids.
  Details: [mutable-intrinsics.md](mutable-intrinsics.md).
- [ ] **Migrate existing hooks behind the intrinsic layer.** Route vector builder
  hooks, vector set helpers, dict in-place helpers, and record shell update hooks
  through the shared decision/intrinsic interface while preserving runtime
  implementations where possible.
- [ ] **Remove split-brain mutability decisions.** Delete or disable any ad hoc
  recognizer/legality path that can independently decide mutation. Hooks are
  implementation targets only.
- [ ] **Preserve non-optimizer builder uses.** `collect` and semantic builder
  lowering required independent of optimization must remain supported.
- [ ] **Update inspection output.** `twk ir` and census output should show both
  ownership proof and final intrinsic-or-hook lowering.

## Phase 8 — Optional precision recovery and end-of-track verification *(architecture: Follow-up)*

- [ ] **Extern copying-borrow precision.** Treat host imports as borrow with owned
  GC results only when the copying-marshalling contract is explicit.
- [ ] **Evaluate non-escaping closure recovery.** Keep escaping/unknown captures
  conservative unless real workloads justify recovery.
- [ ] **Evaluate advanced concurrency distinctions.** Refine serialized-copy vs
  shared-transfer cases only when the runtime contract is explicit.
- [ ] **Run end-of-track performance gates.** Compare ordinary AWFY variants to
  current `*_mut` workaround ceilings from the same machine/session.
- [ ] **Retire Buffer workaround usage when justified.** Only after ordinary code
  reaches the target class and `Vector<Byte>` covers crypto needs. Details:
  [buffer-cleanup.md](buffer-cleanup.md).

## Deferred-work ledger

| Deferred work | Phase |
|---|---|
| Compiler-private mutable intrinsic family | Phase 7 |
| Migration of existing builder/in-place hooks behind shared internals | Phase 7 |
| Removal of ad hoc mutability legality paths | Phase 7 |
| Extern copying-borrow precision | Phase 8 |
| Non-escaping closure recovery | Phase 8, optional based on workload evidence |
| Advanced concurrency copy/share refinement | Phase 8, optional based on runtime contract |
| Buffer retirement | Phase 8, after performance parity is demonstrated |
