# Runtime Trace: Rich Out-of-Bounds Messages (Phase 3)

Status: Design (approved)
Date: 2026-09-09

Phase 3 of the runtime source-mapped stack-trace feature
(`docs/plans/runtime-stack-traces.md`). Phases 0–2, disk-backed debug info
M1+M2, and Phase 4.1 (user-frame-as-primary) are landed on `main`. This phase
gives array/vector out-of-bounds traps a **rich, source-mapped message** with
the offending index and the actual length, replacing today's bare `unreachable`.

## Motivation

Today a user's out-of-bounds `xs[i]` traps with V8's generic `unreachable` (or a
native GC-array trap), so the rendered trace headline is `error: unreachable` —
technically source-mapped (Phase 4.1 puts the caret on the `xs[i]` site) but
uninformative. The single most common runtime error a Twinkle programmer hits
should say what actually went wrong:

```
error: index 5 out of bounds for length 3
 --> src/grid.tw:12:14
   |
12 |   cell := row[idx]
   |           ^^^^^^^^
```

## Scope (decided during brainstorming)

- **In scope:** rich messages for the out-of-bounds trap on user **indexed read
  (`xs[i]`)** and **indexed write (`xs[i] = v`)** of arrays/vectors.
- **Out of scope / non-goals:**
  - **div0/rem0 guard** — deliberately skipped. `Int` `/` and `%` keep the
    native wasm trap; it already renders `divide by zero` with a source-mapped
    location, and a guard would add a divisor-zero compare on *every* division
    (a real hot-path cost on a single-instruction op). Remains a documented
    future option, not part of this phase.
  - **Renderer kind-headline polish** (a "runtime error:" prefix or
    kind-specific hints) — Phase 4. This phase needs **no renderer change**: the
    formatted message flows through the existing host capture as the headline.
  - **dict / string / slice / builder OOB**, and internal `rt.arr` invariant
    checks — stay `unreachable`. Only the user `xs[i]` read/write paths are
    enriched.
  - Name demangling, ANSI parity, `--strip-debug` — other Phase-4 items.

## Key facts established by exploration

- The user-facing indexed **read** compiles to the `rt.arr` `pvec_get` family
  function (`boot/compiler/codegen/runtime/arr.tw`), which loads the vector
  length as its **first** instruction (to choose tail-vs-trie) and then walks
  the structure — it has **no explicit bounds check** and OOB currently traps
  *natively* inside the trie walk. The indexed **write** compiles to
  `set_in_place` (via `get_leaf`), which has **no bounds check at all**.
- The **mutvec** variants (`mutvec_get_i64`/`mutvec_set_i64`) already carry an
  explicit `.If(.None, [.Unreachable], [])` bounds arm — the cheap case.
- `rt.arr` runtime functions call their siblings by **name** — `.Call("vi_nav")`,
  `.Call("do_set…")` — resolved by the linker's `rename_func`
  (`linker.tw:146`, per-module prefix + import redirects). **No** runtime
  function today calls the string/error ops, so wiring a message-formatting
  helper is the one genuinely novel piece (Component 1 / the spike).
- Phase 2's host capture already turns an `error(msg)` throw into
  `err.twinkleMessage = msg`, which `render_runtime_trace` uses as the trace
  headline. So a formatted panic message needs **no** renderer or host change.

## Decisions

- **Enrich via an explicit entry guard, not by intercepting native traps.** A
  native GC-array trap cannot carry a custom message, and the index/length are
  not recoverable at render time. So each user-facing get / set-in-place runtime
  function gains an explicit `if idx u>= len { __panic_oob(idx, len) }` at entry.
  For the read path this reuses the already-loaded `len` (one `i32.ge_u` +
  branch); for the write path it adds one struct-load of `len`. This cost is
  negligible relative to the trie walk / leaf store it guards — a different cost
  profile from the skipped div0 guard (which would double a one-instruction op),
  so the two decisions are consistent, not contradictory. The guard also makes
  OOB **deterministic** across representations instead of depending on trie
  shape.
- **Hidden helper `__panic_oob(index, len)`.** A compiler-only panic entry
  point (no public API, mirroring the existing hidden `__error_string` sink). It
  formats `index {index} out of bounds for length {len}` and traps through the
  existing `error` path. Users never call it; there is no `rt.panic` public
  surface in this phase.
- **The message flows through the existing `error` capture.** `__panic_oob`
  ultimately routes to the same string-sink-then-throw path `error(...)` uses,
  so the host tags the throw with `twinkleMessage` and the renderer shows the
  formatted string as the headline. No renderer/host edits.
- **Wiring is spike-decided (see Components).** Because no runtime code calls the
  formatter/error ops today, the exact mechanism by which `__panic_oob` formats
  and traps — and how `rt.arr` reaches it — is proven by a spike before the
  bounds guards are wired in. Two candidate mechanisms are specified below; the
  spike picks one.
- **Indices are `i32` at the trap site; length too.** `rt.arr` works in `i32`
  index space. `__panic_oob` takes the two `i32`s (widened to `Int`/`i64` for
  formatting as needed). The comparison is **unsigned** (`i32.ge_u`) so a
  negative index (which arrives as a large unsigned `i32`) is caught too.

## Components

### 1. `__panic_oob(index, len)` — the hidden panic helper (spike-decided)

The helper must (a) format `index {index} out of bounds for length {len}` and
(b) trap via the existing `error` string-sink so the message reaches the
renderer. No runtime function calls the string/error ops today, so the first
implementation task is a **spike** that proves the reachable call path and
picks one of:

- **(i) Twinkle prelude/runtime function with a reserved FuncId.** Write
  `__panic_oob` as an ordinary Twinkle function
  (`fn __panic_oob(index: Int, len: Int) Never { error("index ${index} out of bounds for length ${len}") }`)
  in a hidden module, assign it a reserved FuncId in `builtins.tw`
  (and the matching stage0 entry), and have `rt.arr` emit `.Call("__panic_oob")`
  resolved through the linker's rename/redirect. Cleanest if the name resolves
  across the runtime→prelude boundary; string interpolation lowers to
  `int_to_string` + `string_concat` for free.
- **(ii) Runtime wasm helper.** Emit `__panic_oob` as a `FuncDef` in the runtime
  layer (e.g. alongside `arr.tw` or a small `panic.tw` runtime module) whose body
  calls `int_to_string`, `string_concat`, and `__error_string` **by name** via
  the same `.Call("name")` mechanism `rt.arr` already uses for its siblings —
  keeping everything inside the runtime module namespace.

**Spike deliverable:** a minimal end-to-end proof — a single `rt.arr` site
calling the helper, built through `make bundle-cli`, that makes an OOB access
print the formatted message via `twk run`. The spike's outcome (i or ii) fixes
the mechanism the rest of the plan uses. If neither path is viable without
disproportionate work, the spike reports back and the phase is re-scoped (e.g.
to a renderer-only friendlier "index out of bounds" headline without the
numbers) rather than pushing a shaky mechanism.

### 2. Bounds guards at the user get / set-in-place paths (`arr.tw`)

Once Component 1's mechanism exists, add the explicit guard at the entry of the
functions a user `xs[i]` / `xs[i] = v` compile to:

- **Read (`pvec_get` family):** `len` is already loaded first; insert
  `if idx u>= len { __panic_oob(idx, len) }` before the tail-vs-trie branch.
- **Write (`set_in_place` family):** load `len` (StructGet `pv_LEN`) and insert
  the same guard before `get_leaf`.
- **mutvec variants** (`mutvec_get_i64`/`mutvec_set_i64`): they already branch to
  `.Unreachable` on OOB — replace that cold arm with the `__panic_oob(idx, len)`
  call (free; the compare already exists).

The set of functions is family-generated (`PVecFamily`: boxed / i64 / bool /
float / byte), so the guard is written once in the family builder and applies
across element types. The plan's Component-2 task enumerates the exact `FuncDef`
builders touched (confirmed against the spike).

### 3. Stage0 parity

`rt.arr` and the FuncId space are shared with the Rust stage0 bootstrap. If
Component 1 chooses mechanism (i) (a reserved FuncId), stage0's builtin registry
must register the same id, and stage0's `arr.rs` bounds-arm behavior must at
least remain **bootstrap-valid** — it may keep emitting `.Unreachable` (stage0
output is only used to bootstrap boot; the shipped compiler is boot-built), as
long as `make stage2` still converges. The plan verifies `make stage2` after the
FuncId change and fixes stage0 only as far as bootstrapping requires (per the
stage0-bootstrap-dependency rule). Mechanism (ii) (a pure runtime helper) likely
needs no stage0 FuncId change; the plan confirms this in the spike.

### 4. Renderer / host — no change

The formatted message reaches the renderer as the trap headline through Phase
2's existing `twinkleMessage` capture, and Phase 4.1 already selects the user
`xs[i]` frame as primary. This phase adds **no** code to `runtime_trace_renderer`,
`trace.tw`, `symbolicate.tw`, or `runtime.mjs`.

## Architecture (data flow)

```
user: xs[5]  (len 3)
  └─ lowers to rt.arr pvec_get(vec, 5)
       └─ entry guard: 5 u>= 3  → __panic_oob(5, 3)
            └─ format "index 5 out of bounds for length 3"
                 └─ __error_string(msg)  → host error import throws
                      └─ host: err.twinkleMessage = "index 5 out of bounds…"
                           └─ childTrapHandler → render_runtime_trace(bytes, stack,
                                message="index 5 out of bounds…", source_root)
                                └─ headline "error: index 5 out of bounds for length 3"
                                   + Phase 4.1 caret on the xs[i] user frame
```

## Testing

- **Spike (Task 1):** the minimal `twk run` proof that an OOB access prints the
  formatted message end-to-end (gates the mechanism choice).
- **Boot-level (codegen):** a `twk wat`/emit-level assertion that the user
  `pvec_get`/`set_in_place` family functions contain the `__panic_oob` call in
  their OOB arm (mirrors existing `codegen`/`arr` suite patterns). If mechanism
  (i), also a check that the reserved FuncId resolves.
- **e2e (`cli.test.mjs`, `twk run`):** a fixture that indexes past the end of a
  length-3 vector (read) and, separately, writes past the end (`xs[i] = v`),
  each asserting stderr shows `index 5 out of bounds for length 3`, a
  source-mapped snippet/caret on the index site, and exit 1. A negative-index
  read (`xs[-1]`) asserts the unsigned compare catches it. An in-bounds program
  still exits 0.
- **Self-host / suites:** `make stage2` converges; boot + JS suites green;
  confirm no measurable regression on an indexing-heavy path is introduced by
  the read guard (a quick before/after on an existing vector benchmark).

## Non-goals recap (deferred, tracked in `runtime-stack-traces.md`)

div0/rem0 guard; renderer kind-headline/ANSI polish; name demangling;
`--strip-debug`; dict/string/slice OOB; a public `rt.panic` surface.
