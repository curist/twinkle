# String `graphemes()` Plan

## References

* [UAX #29: Unicode Text Segmentation](https://www.unicode.org/reports/tr29/) — defines extended grapheme cluster boundaries
* [GraphemeBreakProperty.txt](https://www.unicode.org/Public/UCD/latest/ucd/auxiliary/GraphemeBreakProperty.txt) — official code point → GCB property mapping
* [GraphemeBreakTest.txt](https://www.unicode.org/Public/UCD/latest/ucd/auxiliary/GraphemeBreakTest.txt) — official conformance test vectors
* [UAX #44: Unicode Character Database](https://www.unicode.org/reports/tr44/) — property definitions (Extend, Extended_Pictographic, Regional_Indicator, etc.)
* [emoji-data.txt](https://www.unicode.org/Public/UCD/latest/ucd/emoji/emoji-data.txt) — Extended_Pictographic property assignments

## Goal

Add user-perceived character iteration for strings:

* `String.graphemes(s: String) Iterator<String>`
* `s.graphemes()`

This complements existing scalar-based `chars()` and enables correct handling of combining marks, ZWJ emoji sequences, and regional-indicator flags.

## Status: Implemented

Tasks A–C are complete. Task D (documentation) is pending.

## Design (as implemented)

The entire grapheme segmentation is implemented in **pure Twinkle** (`prelude/string.tw`), with no host intrinsics or runtime imports. This guarantees identical behavior across interpreter and Wasm backends by construction.

Internal helpers (all private to the prelude module):

| Helper | Purpose |
|---|---|
| `decode_cp(s, pos)` | Decode UTF-8 code point at byte position |
| `cp_len(s, pos)` | Byte length of UTF-8 sequence at position |
| `is_gcb_extend(cp)` | GCB=Extend property check (combining marks, variation selectors, emoji modifiers, tags, etc.) |
| `is_regional_indicator(cp)` | U+1F1E6..U+1F1FF |
| `is_extended_pictographic(cp)` | Major emoji blocks (for GB11) |
| `next_grapheme_end(s, pos)` | Simplified UAX #29 state machine returning next cluster boundary |

Public API:

* `pub fn graphemes(s: String) Iterator<String>` — uses `Iterator.unfold` with `next_grapheme_end` as the step function

### UAX #29 rules covered

| Rule | Description | Covered |
|---|---|---|
| GB3 | CR × LF | Yes |
| GB4/GB5 | Break around Control/CR/LF | Yes |
| GB9 | × (Extend \| ZWJ) | Yes |
| GB11 | ExtPict Extend* ZWJ × ExtPict | Yes (with `saw_extpict` tracking) |
| GB12/GB13 | RI × RI (pairs only) | Yes |
| GB6–GB8 | Hangul L/V/T syllable sequences | Partial (V/T treated as Extend) |
| GB9a | × SpacingMark | Partial (major ranges in `is_gcb_extend`) |
| GB9b | Prepend × | Not yet |
| GB9c | Indic conjunct break | Not yet |
| GB999 | Any ÷ Any (default break) | Yes |

### Property table coverage

The `is_gcb_extend` function covers the most commonly encountered Extend ranges (Latin, Cyrillic, Hebrew, Arabic, Devanagari, Thai/Lao, Myanmar, Mongolian, emoji modifiers, variation selectors, tags). It does **not** exhaustively cover all Unicode Extend code points — see Risks.

## Non-Goals

* Adding `grapheme_len` or `grapheme_slice` in this plan.
* Implementing terminal display-width semantics.
* Changing `chars()` behavior (it remains Unicode scalar iteration).

## Semantics Contract

For any valid `s: String`:

* `collect g in s.graphemes() { g }.join("") == s`
* Every yielded `g` is non-empty valid UTF-8.
* Boundary behavior follows a simplified subset of UAX #29 extended grapheme cluster rules (see rules table above).

## Implementation Tasks

### Task A: API Surface — Complete

* `pub fn graphemes(s: String) Iterator<String>` added to `prelude/string.tw`.
* Resolved as a prelude method on String (dot-call `s.graphemes()` works).
* No signature stub needed — implemented directly in prelude, not as a builtin intrinsic.

### Task B: Grapheme Boundary Logic — Complete

* Implemented entirely in Twinkle (`prelude/string.tw`) as private helper functions.
* No intrinsic IDs, registry entries, or host imports needed.
* Both backends execute the same Twinkle code, guaranteeing parity by construction.

### Task C: Tests — Complete

* `tests/run/string_graphemes.tw` — covers ASCII, combining marks, emoji+modifier, regional indicator flags, ZWJ family sequences, GB11 edge case (non-ExtPict + ZWJ + emoji), mixed text, empty string, and round-trip invariants.
* Registered in both `tests/run_test.rs` (interpreter) and `tests/run_wasm_test.rs` (Wasm).
* Both backends produce identical output.

### Task D: Documentation — Pending

* Update `docs/spec.md` and `docs/API.md`:
  * `chars()` = Unicode scalar values
  * `graphemes()` = extended grapheme clusters
* Add examples showing why scalar and grapheme iteration can differ.

## Validation

* `string_graphemes` fixture passes under both interpreter and Wasm with identical output.
* Existing `string_chars` and `string_code_point` fixtures remain unchanged.
* Round-trip invariant (`join(graphemes(s)) == s`) verified in tests.

## Risks

* **Property table completeness**: `is_gcb_extend` covers common ranges but not the full Unicode Extend set. Scripts with combining marks outside the covered ranges (e.g., Tibetan, Ethiopic, some CJK annotations) may over-segment. This can be incrementally fixed by expanding the ranges in `is_gcb_extend` against the official `GraphemeBreakProperty.txt`.
* **Missing GB9b (Prepend)**: Prepend characters (rare — mainly Indic virama-like code points) are not handled. Text with these characters may over-segment.
* **Missing GB9c (Indic conjunct break)**: Indic consonant clusters using the InCB property are not handled.
* **Grapheme segmentation is O(n) per cluster boundary** due to code point decoding and property checks. For very long strings with many combining marks, this is slower than scalar iteration.
* **No Unicode version pinning**: The property tables are hardcoded ranges, not generated from a specific Unicode version's data files. Future Unicode versions may add new Extend or Extended_Pictographic code points that would require manual updates.

## Future Work

* Expand `is_gcb_extend` and `is_extended_pictographic` tables for broader script coverage, ideally by code-generating from `GraphemeBreakProperty.txt` and `emoji-data.txt`.
* Add `grapheme_len(s)` convenience function.
* Consider conformance testing against `GraphemeBreakTest.txt`.
* Implement GB9b (Prepend) and GB9c (Indic conjunct break) if needed.
