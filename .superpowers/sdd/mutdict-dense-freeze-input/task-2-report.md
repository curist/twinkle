# Task 2 implementation report

Status: DONE_WITH_CONCERNS

## Implementation

Added the throwaway boot-only stable-ID arena measurement target described by the Task 2 brief:

- `MutDictArenaEntryI64` holds cached hash and unboxed Int key plus mutable unboxed value/liveness.
- Candidate M deep-clones mutable records, mutates values with `struct.set`, marks removals dead, appends reinsertions, and always converts/compacts into fresh immutable `HamtEntry` records.
- Candidate H stores exact immutable `HamtEntry` records with external `I32Array` liveness, shallow-clones references/liveness with `array.copy`, overwrites by replacing the record at the same stable ID, hands off its dense arena directly, and recreates corrected entries while compacting holes.
- Both outputs call the landed `freeze_dense` adapter and return ordinary persistent Dicts.
- Temporary builtin/signature surfaces expose exactly `Dict.bench_arena_layouts` and `Dict.bench_arena_timings` to the feature-named benchmark.
- The benchmark checks exact contents, absence, insertion order, clone isolation (runtime assertions), and persistent set/remove isolation after publication.

No open-addressing index, region selection, lifecycle poison, retained mutable ABI, or stage0 runtime implementation was added.

## TDD evidence

RED was established before runtime implementation:

```text
target/twk run boot/bench/mutdict_arena_layout_spike.tw
error: undefined variable `Dict`
error: no field `bench_arena_layouts`
error: no field `bench_arena_timings`
```

GREEN: the same benchmark now completes every dense/churn row with `parity=PASS`.

## Debugging note

The first generated module failed Wasm validation in function index 42, mapped through WAT import/function ordering to `rt_dict__bench_arena_layouts`. Its local declaration had only ten `f64` slots total for one timer scratch plus ten timing outputs, so intended timing local 24 was emitted as the following `Array` local. Adding the missing `f64` local fixed the exact `local.set` type mismatch. No workload or assertion was weakened.

## Validation

Commands run:

- `target/twk fmt boot/compiler/codegen/runtime/types.tw boot/compiler/codegen/runtime/dict.tw boot/compiler/builtins.tw boot/prelude/signatures/dict.tw boot/bench/mutdict_arena_layout_spike.tw` — passed; changed Twinkle files canonicalized.
- `make stage2` — passed; self-host fixed point reached.
- `make quick-bundle-cli` — passed.
- `target/twk run boot/bench/mutdict_arena_layout_spike.tw` — passed; all warmup, 65K, and 1M dense/churn rows reported `parity=PASS`.
- `target/twk test` — passed (`Ran 3506 tests: 3506 passed`).
- `target/twk lint boot/main.tw` — existing baseline only: five unrelated `record-copy-helper` findings; no new finding points at this spike.
- focused `target/twk wat` inspections — passed:
  - mutable overwrite contains `struct.set MutDictArenaEntryI64 2`;
  - canonical overwrite allocates `BoxedInt` + `HamtEntry` and uses `array.set`, with no entry-field mutation;
  - mutable seam has no `hash_*`, `node_get`, `node_set`, or Buffer call;
  - wrapper calls both candidate helpers and `freeze_dense` twice.
- `git diff --check` — passed.

## Initial measurement signal

Representative final 1M rows (single quick pass, after warmups):

- dense 1x: mutable clone 19.18 ms vs canonical 1.27 ms; mutable update 1.28 ms vs canonical 43.67 ms; mutable seam 38.04 ms vs canonical direct 0 ms.
- dense 4x: mutable clone 14.37 ms vs canonical 1.32 ms; mutable update 4.73 ms vs canonical 106.83 ms; mutable seam 84.08 ms vs canonical direct 0 ms.
- churn 1x: mutable clone 23.80 ms vs canonical 1.66 ms; mutable update 1.23 ms vs canonical 34.37 ms; mutable churn 10.50 ms vs canonical 7.92 ms; mutable seam 40.00 ms vs canonical compaction 27.76 ms.

This fortifies the expected tradeoff: unboxed mutable updates are dramatically cheaper, while canonical immutable storage dominates snapshot clone and dense publication preparation. It does not yet select a retained layout.

## Concerns / residual risks

- Timing phases are executed in a fixed candidate order and this quick pass reports one sample per row. GC placement visibly perturbs seam/freeze timings, especially allocation-heavy rows. `freeze_dense` results are semantically equivalent and use the same builder, so differing freeze times should be treated as order/GC noise, not a representation effect. A decision-grade pass must rotate candidate order and report ranges/medians.
- Construction remains untimed by the exact quick-spike contract, so canonical insertion boxing/allocation versus mutable record construction is not yet measured.
- Temporary source-callable APIs and the benchmark-only runtime type/function surface require cleanup after the representation decision.
- The benchmark covers Int→Int only and cannot quantify cached-hash savings for String keys.

## Changed files

- `boot/bench/mutdict_arena_layout_spike.tw`
- `boot/compiler/builtins.tw`
- `boot/compiler/codegen/runtime/dict.tw`
- `boot/compiler/codegen/runtime/types.tw`
- `boot/prelude/signatures/dict.tw`
- `docs/plans/mutdict-dense-freeze-input.md`
- `docs/plans/sound-uniqueness/storage/README.md`

## Fix round 1

Addressed both task-review findings without changing the two-result or ten-slot API.

### Clone-isolation guard

`bench_arena_layouts` now performs an exhaustive untimed scan over every retained
base entry after the working clones have been overwritten and churned. For each
stable ID it reconstructs the original key/hash and traps unless:

- Candidate M retains the original cached hash, unboxed key, original value, and
  `live == 1`;
- Candidate H retains the original cached hash, boxed key, boxed value, and
  insertion-order index; and
- every element of retained `h_live_base` remains `1`.

The scan is outside all ten timers and does not weaken or replace the published
Dict parity checks.

### Rotated timing and repeated sampling

Added the internal throwaway `arena_bench_rotation` global. Every call alternates
candidate order, and clone, overwrite, churn, seam preparation, and `freeze_dense`
all contain both mutable-first and canonical-first branches. Result order remains
`[mutable, canonical]`; timing slots remain the original fixed semantic order.
The global toggles only after metrics have been stored.

The benchmark now emits three explicitly labeled raw samples for every 65K and 1M
row after two warmups. Labels name both sample number and actual candidate order.
Because each row has an odd sample count, starting order also alternates between
rows. No layout-selection claim was added.

### Fix-round validation

- `target/twk fmt boot/compiler/codegen/runtime/dict.tw boot/bench/mutdict_arena_layout_spike.tw` — passed.
- `git diff --check` — passed.
- `make stage2` — passed; fixed point reached.
- `make quick-bundle-cli` — passed.
- `target/twk run boot/bench/mutdict_arena_layout_spike.tw` — passed; two warmups
  and all eighteen reported samples completed with `parity=PASS`, alternating
  `mutable-first` / `canonical-first` labels. Raw output saved at
  `/tmp/mutdict-arena-rotated-results.txt` for this run.
- `target/twk test` — passed (`Ran 3506 tests: 3506 passed`).
- `target/twk lint boot/main.tw` — unchanged baseline: five unrelated
  `record-copy-helper` findings.
- Focused `target/twk wat ... --func bench_arena_layouts` inspection — passed:
  six reads of `arena_bench_rotation`; both call orders are present for clone,
  overwrite, churn, seam, and freeze; `verify_base_loop` calls `hash_i64`, checks
  Candidate H's `I32Array`, and loops to `verify_base_exit`.

### Fix-round self-review

- Verified the internal rotation is read consistently throughout one sample and
  toggled only after all ten semantic timing locals are boxed.
- Verified canonical dense seam still records exactly `0.0` regardless of order.
- Verified base verification is outside timed phases and scans exactly `0..n`.
- Verified no builtin/signature/API slot changes were made in this round.
- Verified the benchmark still makes no final layout selection.

Residual measurement risk: even with rotation and three raw samples, allocation
and GC variance is substantial. Future decision work should compare ranges or
medians and should not interpret freeze-time differences as representation work,
since both paths call the same adapter on equivalent dense inputs.

## Final fix wave

Addressed the final review findings without selecting a layout or opening Task 4.

### Persistent-remove oracle

The benchmark now checks the persistent result after removal exhaustively: length
is exactly `n - 1`, `keys().len()` is exactly `n - 1`, and every remaining key is
in the expected sequence. The check runs for both candidate results outside all
timers. Churn removes stable ID 0, the first key of the reinserted suffix: its
physical appended index differs from its compacted published slot, so a stale
`order_index` changes the sequence and fails the oracle.

TDD sensitivity was demonstrated by initially expecting length `n`; the first
warmup reported `parity=FAIL` and trapped. Correcting the oracle to `n - 1` made
the complete benchmark pass.

### Evidence and historical wording corrections

The active gate, storage README, and Tier-0 spike now consistently say that the
reported clone is the dense pre-churn-base clone, not a clone of the 1.25n churned
physical state. The reopening inventory explicitly includes that missing clone,
k/n=1/8, construction, the same-session persistent control, and separate HAMT,
order, publication, and complete-workload totals. Task 3 no longer marks those
phase splits complete and remains **STOP WITHOUT SELECTION**.

The storage history now labels the insertion-order-sidecar and bulk-copy claims
as Tier-0 proxy results. Current layout C uses stable arena identity; Candidate M
deep-copies mutable records, while Candidate H bulk-copies immutable references
and external liveness.

### Final-fix validation

- `target/twk fmt boot/bench/mutdict_arena_layout_spike.tw` — passed.
- RED: `target/twk run boot/bench/mutdict_arena_layout_spike.tw` with the temporary
  incorrect length oracle — failed as intended on the first warmup with
  `parity=FAIL`.
- GREEN: `target/twk run boot/bench/mutdict_arena_layout_spike.tw` — passed; both
  warmups and all eighteen reported samples returned `parity=PASS`. Raw output is
  `/tmp/mutdict-arena-final-fix-results.txt` for this run.
- `target/twk test` — passed (`Ran 3506 tests: 3506 passed`).
- `target/twk lint boot/main.tw` — unchanged baseline only: the same five unrelated
  `record-copy-helper` findings.
- Local Markdown-target validation over the active gate, storage README, and
  Tier-0 spike — passed.
- `git diff --check` — passed.
- Focused WAT was not rerun because this wave changes no runtime/codegen shape;
  it adds only a source-level untimed correctness oracle and documentation.

### Final-fix self-review

- Verified the exact-key oracle covers both result slots and both dense/churn
  shapes, and executes only after timed metrics are returned.
- Verified churn ID 0 is physically appended but is first in the compacted
  reinserted suffix, making stale order identity observable.
- Verified all active/spike/storage descriptions use the same complete missing-
  evidence inventory and do not claim separate phase splits were measured.
- Verified Task 4 remains closed and no retained layout is selected.
