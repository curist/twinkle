# Boot Signature Source of Truth — Session Handoff

## What was done this session

### Doc fix
- Updated `docs/plans/boot-signature-source-of-truth.md`: all `pipeline.tw` references
  corrected to `base_env.tw` (the actual file containing `builtin_env()`).

### Phase 0 complete — guardrail tests (green)
New file: `boot/tests/suites/base_env_guardrail_suite.tw`

Locks current behaviour before the refactor:
- `builtin_env()` has expected function signatures (string_len, vector_push, dict_new,
  range_from, cell_new, iterator_unfold, byte_to_int, ...)
- `register_builtin_methods()` wires up method→function mappings correctly
  (String.len→string_len, Vector.push→vector_push, Cell.set→cell_set, ...)

These must stay green throughout all subsequent phases.

### Phase 1 complete — signature-only loader (green)
New file: `boot/compiler/signatures.tw`

`load_signatures(signatures_dir: String) Result<Vector<SignatureGroup>, String>`
- Lists `prelude/signatures/*.tw`, sorts for determinism
- Parses each file with `compiler.parser`
- Resolves type expressions against `builtin_type_env()` (no func bodies, no lowering)
- Returns one `SignatureGroup` per file: `{ receiver: String?, sigs: Vector<FunctionSig> }`
- Receiver from filename: string→String, vector→Vector, range→None (free fns), etc.

New helper in `boot/compiler/base_env.tw`:
`pub fn builtin_type_env() ResolvedEnv` — types only, no functions.
Avoids the circular dep that Phase 2 would otherwise create when `base_env.tw` calls
`load_signatures` which calls back into base_env.

New test suite: `boot/tests/suites/signature_loader_suite.tw`
- Verifies groups present for all 10 receiver types
- Spot-checks String.len shape, Vector.push type params, range free fns, Cell, Byte

Both suites added to `boot/tests/test_frontend.tw` for focused runs:
```
cargo run --release -- run boot/tests/test_frontend.tw
```

### Test count
- 1303 total. Only 2 pre-existing failures: `checker::G13 if-else ...` (unrelated).
- All new tests green.

---

## What's next: Phase 2

**Goal:** Replace the hardcoded `builtin_sig(...)` table in `base_env.builtin_env()` with
signatures loaded from `prelude/signatures/*.tw` via `load_signatures`.

**Red tests to write first** (in `base_env_guardrail_suite.tw` or a new phase-2 suite):
- After the swap, `builtin_env()` should still contain all the same function signatures
  — the existing guardrail tests already cover this, so they act as the green bar.

**Key design question for Phase 2:**
The signature files use short names (`len`, `push`, `get`), but the current env uses
prefixed internal names (`string_len`, `vector_push`). Two options:

A. **Keep internal names**: signature loader produces sigs with internal names by applying
   a prefix derived from receiver (string_ prefix for String group, etc.). Exceptions
   exist (char_code_at, from_code_point don't get the string_ prefix), so this needs
   a per-entry mapping somewhere.

B. **Adopt canonical names**: rename env entries to canonical form (String.len, etc.) and
   update method registration and lowering to match. This is cleaner long-term but touches
   more code.

Option A is lower risk for Phase 2; Option B aligns with Phase 3's canonical/internal
mapping in `builtins.tw`. Discuss at session start.

**Also needed for Phase 2:**
- Derive method registrations from `SignatureGroup` data (receiver + sig names) to replace
  the hardcoded `register_methods(...)` calls in `resolver.register_builtin_methods()`.
- Some entries in `register_builtin_methods()` come from normal prelude modules, not
  signature files (map, filter, fold, compare, index_of, etc.) — these must stay hardcoded
  or come from a different source. The Phase 0 inventory distinguishes them.

**Phase 2 exit criteria** (already defined in the plan):
- Changing a sig in `prelude/signatures/*.tw` updates boot builtin env without editing
  `base_env.tw`.
- Changing a builtin method there updates boot method shape without editing `resolver.tw`.

---

## Key files

| File | Role |
|---|---|
| `boot/compiler/signatures.tw` | New loader (Phase 1 complete) |
| `boot/compiler/base_env.tw` | Has builtin_type_env() + builtin_env() to refactor |
| `boot/compiler/resolver.tw` | Has register_builtin_methods() to refactor |
| `boot/tests/suites/base_env_guardrail_suite.tw` | Phase 0 guardrails |
| `boot/tests/suites/signature_loader_suite.tw` | Phase 1 tests |
| `boot/tests/test_frontend.tw` | Focused test runner |
| `docs/plans/boot-signature-source-of-truth.md` | Full plan |
