# fmt drops comments inside method chains

**Status:** Bug report + fix sketch (not started)

**Severity:** Correctness / silent data loss — `twk fmt` deletes user comments
placed between the links of a method chain. Formatting is idempotent, so the
comment is gone after the first format with no diagnostic.

## Symptom (verified minimal repro)

Input:

```tw
fn build() Int {
  x := foo()
    // comment A: between chain links
    .bar()
    // comment B: before another link
    .baz()
  x
}
```

After `twk fmt`:

```tw
fn build() Int {
  x := foo().bar().baz()
  x
}
```

Both interior comments (A, B) are **dropped**, and the multi-line chain is
collapsed to one line. A comment on the first line of a block body is unaffected
(it survives), so the loss is specific to the **inter-segment gaps of a chain**.

This was hit in practice while adding a comment between two chained `.test(...)`
calls in `boot/tests/suites/mutable_produce_suite.tw`; the workaround was to move
the comment inside the following `fn() { … }` body, where fmt preserves it.

## Root cause

Comments are represented as `n` nodes attached to token spans, stored in
`nMap`/`TriviaMap` (`boot/compiler/fmt/printer.tw`) as `leading`/`trailing` dicts
keyed by span offsets, and emitted by range collectors (`get_n_in_range`,
`get_trailing_n_in_range`).

The **binary/logical chain** printer threads interior trivia between pieces via a
trivia-aware separator: `format_chained_binary` (printer.tw ~1494) calls
`binary_chain_sep(prev_end, piece.expr.span.end)` (~1457) for each piece, which
collects and re-emits any `n` sitting in the gap between adjacent operands.

The **method-chain** printer has no equivalent. `collect_method_chain`
(printer.tw ~1664) walks a `Call(Field(obj, name), args)` spine into
`ChainSegment`s and `finish_method_chain` (~1648) / `format_call` (~1296) render
them, but nothing collects the `n` nodes that fall in the gaps between one
segment's end and the next segment's `.method` token. Those comments are never
looked up, so they are silently dropped, and the chain is free to collapse onto a
single line.

**Related, same class:** `project_fmt_trivia_bug.md` records comments in logical
`or`/`and` chains being dropped by `collect_logical_chain`. Both are "a chain
collector flattens its spine without threading interior trivia." A shared
trivia-carrying-separator approach could fix both; at minimum the fixes should be
designed together.

## Fix sketch

1. In the method-chain formatter, collect interior `n` for each segment gap —
   the range between the previous segment's `span.end` and the current segment's
   leading `.`/method token — mirroring `binary_chain_sep`.
2. When any interior comment exists in a chain, force the chain to break
   multi-line (one segment per line) and emit each collected comment on its own
   line ahead of its segment, instead of collapsing. When no interior comments
   exist, keep today's collapse behavior.
3. Preserve idempotency: re-running fmt on the broken-out form must be a no-op.

## Acceptance

- The repro above round-trips: comments A and B survive `twk fmt`, and a second
  `twk fmt` is a no-op (idempotent).
- New `boot/tests/suites/fmt_suite.tw` cases cover: (a) a method chain with an
  interior comment (forced multi-line, comment preserved), (b) a clean method
  chain with no comments (still collapses), and (c) a trailing comment on a chain
  segment.
- The related logical-`or`/`and` interior-comment case
  (`project_fmt_trivia_bug.md`) is covered by the same or a sibling test, whether
  or not both are fixed in one change.

## Non-goals / notes

- Not changing when chains collapse vs. break in the *comment-free* case; layout
  policy is untouched except that presence of an interior comment forces a break.
- Do not "fix" this by having fmt refuse to format files with chain comments;
  the format must be faithful, not skipped.
- After the fix, re-verify with the scratchpad repro and by re-adding the
  `mutable_produce_suite.tw` marker comment between chained `.test()` calls (it
  currently must live inside the `fn()` body as a workaround).
