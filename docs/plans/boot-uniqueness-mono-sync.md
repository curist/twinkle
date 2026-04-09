# Boot uniqueness mono-map synchronization

This plan captures the next cleanup step after the recent self-host progress.

## Context

The backend/verifier work now gets self-host materially further:

- `AGlobalLocal` verification is tightened
- pattern-local mono inference now reaches both slot assignment and closure conversion
- the old `error_message` / `format_help` verifier failures are gone

The next failure exposed a different seam:

- optimizer rewrites can synthesize fresh ANF locals
- but the function's `op_result_mono` map is still inherited unchanged
- backend preparation then sees structurally valid locals whose mono metadata was never produced

A concrete example was the loop-builder rewrite in `uniqueness.tw`, which introduces:

- `builder_local`
- `freeze_local`
- `assign_local`

Those locals exist in the rewritten ANF body, but their monos are not authored at the rewrite site.

## Problem statement

A post-hoc repair pass that re-scans rewritten ANF and guesses types for synthesized locals is the wrong abstraction.

Why:

- it reconstructs metadata from syntax after the fact
- it hard-codes knowledge of one optimizer rewrite shape downstream
- it makes `op_result_mono` less authoritative
- every new optimizer rewrite that creates locals would need another repair path

The correct invariant is:

> any pass that creates fresh ANF locals must also create their mono metadata at the same time.

## Goal

Make optimizer-produced locals participate in the same explicit metadata contract as lowering-produced locals.

In practice, this means `uniqueness_rewrite` should return updated `op_result_mono` whenever it synthesizes locals, instead of relying on downstream repair logic.

## Scope

Primary target:

- `boot/compiler/opt/uniqueness.tw`

Likely touched helpers:

- `boot/compiler/opt/loop_builder.tw`
- `boot/compiler/opt/builder_region.tw`
- optimizer tests that assert builder rewrite behavior

## Proposed design

### 1. Make uniqueness rewrite return metadata, not just syntax

Refactor the rewrite entry points so they carry mono updates alongside rewritten expressions.

Possible shapes:

- `rewrite_expr(...) -> { expr, mono_map, next_local, ... }`
- or a smaller targeted result type for loop-builder rewrites:
  - `rewrite_loop_region(...) -> { expr, new_monos }`

Either is acceptable, but the transform that allocates locals must be the transform that assigns their monos.

### 2. Author builder-region monos at creation time

When loop-builder rewriting succeeds, it should explicitly assign:

- `builder_local` → base accumulator mono
- `freeze_local` → base accumulator mono
- `assign_local` → `Void`

Those entries should be merged into the function's `op_result_mono` before the rewritten function is returned.

### 3. Remove post-hoc mono repair logic

Any temporary repair logic that pattern-matches rewritten ANF to infer builder-local monos should be deleted once the rewrite itself produces the metadata.

The backend should consume explicit metadata, not reconstruct it.

## Implementation sketch

### Option A: targeted fix

Keep most of `uniqueness.tw` unchanged and make only loop-builder rewriting return extra metadata.

For example:

- introduce `LoopRewrite = .{ expr: AnfExpr, mono_updates: Dict<Int, MonoType> }`
- have `rewrite_loop_region(...)` populate `mono_updates`
- thread those updates back through `try_loop_rewrite` and `uniqueness_rewrite_with_semantics`

Pros:

- smallest change
- addresses the current blocker directly

Cons:

- still leaves uniqueness with split responsibilities if future rewrites add other fresh locals

### Option B: general rewrite result threading

Make uniqueness's recursive rewrite APIs return both rewritten ANF and accumulated mono updates.

Pros:

- cleaner long-term model
- naturally handles future synthetic locals

Cons:

- broader refactor

Recommended approach: start with Option A if it keeps the code straightforward, but structure it so moving to Option B later is easy.

## Tests to add

### A. Uniqueness builder rewrite preserves mono metadata

Add a focused test showing that after loop-builder rewriting:

- synthesized builder-region locals appear in `op_result_mono`
- `freeze_local` has the collection mono
- `assign_local` has `Void`

### B. End-to-end backend preparation regression

Exercise a function like `compute_line_starts` through optimization and preparation, and assert the prepared backend no longer sees:

- assign value mono `Void` where collection mono is expected

### C. Self-host regression note

After the fix lands, rerun:

- `cargo run --release -- run boot/main.tw -- build boot/main.tw`

and treat the next failure, if any, as the next real blocker.

## Success criteria

- `uniqueness_rewrite` updates `op_result_mono` for all locals it synthesizes
- no downstream mono-repair pass is needed for builder-region locals
- backend verifier no longer fails on optimizer-generated locals whose metadata drifted from structure
- `op_result_mono` remains the authoritative local-mono source for backend preparation
