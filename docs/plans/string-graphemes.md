# String `graphemes()` Plan

## Goal

Add user-perceived character iteration for strings:

* `String.graphemes(s: String) Iterator<String>`
* `s.graphemes()`

This complements existing scalar-based `chars()` and enables correct handling of combining marks, ZWJ emoji sequences, and regional-indicator flags.

## Current State

* Core string operations are byte-oriented (`len`, `get`, `slice`, indexing).
* Unicode scalar helpers exist in prelude (`chars`, `char_len`, `code_point_at`).
* There is no grapheme-cluster API.

## Target State

* `graphemes()` yields **extended grapheme clusters** (UAX #29), each as a non-empty `String`.
* Iterator order is left-to-right over grapheme boundaries.
* Behavior is consistent between interpreter and Wasm backends.
* Existing byte/scalar APIs remain unchanged.

## Non-Goals

* Adding `grapheme_len` or `grapheme_slice` in this plan.
* Implementing terminal display-width semantics.
* Changing `chars()` behavior (it remains Unicode scalar iteration).

## Semantics Contract

For any valid `s: String`:

* `collect g in s.graphemes() { g }.join("") == s`
* Every yielded `g` is non-empty valid UTF-8.
* Boundary behavior follows UAX #29 extended grapheme cluster rules, pinned to a documented Unicode version.

## Proposed Design

Use a prelude-facing iterator API with an internal boundary primitive:

1. Add a boundary helper intrinsic (internal-use focused), e.g.:
   * `String._next_grapheme_boundary(s: String, byte_pos: Int) Int?`
2. Implement `pub fn graphemes(s: String) Iterator<String>` in `prelude/string.tw` via `Iterator.unfold`:
   * state = current byte position
   * ask next boundary
   * yield `s.slice(pos, next)` and advance to `next`

Rationale:

* Keeps the user surface minimal (`graphemes()` only).
* Reuses existing iterator and slice machinery.
* Avoids adding specialized iterator runtime representation.

## Implementation Tasks

### Task A: API Surface

* Add `graphemes` declaration to `prelude/signatures/string.tw`.
* Register method lookup for `String.graphemes` in `TypeEnv`.
* Add `pub fn graphemes(s: String) Iterator<String>` in `prelude/string.tw`.

### Task B: Grapheme Boundary Primitive

* Add intrinsic ID/registry/signature/contract entries for internal boundary lookup helper.
* Implement helper in interpreter and Wasm codegen paths.
* Keep helper undocumented in user API (internal support for `graphemes`).

### Task C: Backend Parity

* Ensure interpreter and Wasm use the same segmentation logic and Unicode version.
* Add parity tests for representative cases:
  * ASCII
  * combining mark (`"e\u{301}"`)
  * emoji modifier (`"👍🏽"`)
  * flag (`"🇺🇸"`)
  * ZWJ family sequence (`"👨‍👩‍👧‍👦"`)
  * mixed-language text

### Task D: Documentation

* Update `docs/spec.md` and `docs/API.md`:
  * `chars()` = Unicode scalar values
  * `graphemes()` = extended grapheme clusters
* Add examples showing why scalar and grapheme iteration can differ.

## Validation

* New run fixtures pass under interpreter and Wasm with identical output.
* Existing `string_chars` and `string_code_point` fixtures remain unchanged.
* Round-trip invariant (`join(graphemes(s)) == s`) holds in targeted tests.

## Risks

* Unicode-version drift across hosts can cause behavior mismatches if not pinned.
* Grapheme segmentation is more expensive than scalar iteration.
* Incorrect boundary handling can silently split or merge user-perceived characters.

## Rollout

1. Land internal boundary helper + prelude `graphemes`.
2. Land parity fixtures and API-suite coverage.
3. Update spec/API docs with explicit scalar vs grapheme guidance.
4. Add a short migration note encouraging `graphemes()` for user-facing character semantics.
