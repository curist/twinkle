# leetcode

An **API-ergonomics stress test** for Twinkle.

The goal is not the puzzles themselves but the experience of writing them:
each problem is solved as idiomatically as the language and standard library
allow, and anywhere that feels awkward becomes a data point for improving the
API (the friction it surfaced fed real compiler and stdlib work). The problems
are deliberately small and self-contained so the friction is about expression,
not architecture.

Each `problems/p*.tw` is a module exposing a `suite()` of test cases; `main.tw`
runs them all.

## Run

```bash
target/twk run examples/leetcode/main.tw
```

`assert.tw` and `runner.tw` are symlinks to the canonical harness in
`boot/tests/`.
