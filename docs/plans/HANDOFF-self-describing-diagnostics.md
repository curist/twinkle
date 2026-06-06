# Handoff — self-describing diagnostics + the analyzer hang fix

Written 2026-06-06. Self-contained so a fresh session (any harness) can continue.

## TL;DR

Two workstreams on branch **`self-describing-diagnostics`** (based on `main` @ `22712a7`):

1. **Analyzer infinite-hang bug — DONE, committed, fully verified.** This was
   pulled in as higher priority mid-feature. `twk check`/`build` hung forever on
   *any* compile error in a large project (e.g. `boot/main.tw`). Root-caused,
   fixed, regression-tested, self-hosted green.
2. **Self-describing diagnostics + fix-its (feature) — Tasks 1–2 of 9 DONE.**
   The remaining Tasks 3–9 are fully specced in
   `docs/plans/self-describing-diagnostics.md`.

**Pending decision for the human:** do we (a) continue Tasks 3–9 here, (b) split
the hang fix into its own branch/PR first (it's independent and high-value), or
(c) pause.

## Commits on this branch (newest last)

```
7468524 Add SuggestedFix/FixEdit types for self-describing diagnostics   (feature Task 1)
b73492e Memoize failed module analyses to stop exponential re-walk        (HANG FIX — independent)
d9ce970 Enrich MissingVariants payload with variant arity and insert offset (feature Task 2)
c23c017 Add self-describing-diagnostics implementation plan                (the plan doc)
```

Working tree is clean except untracked `tools/leetcode/` (pre-existing, unrelated)
and this handoff file.

> Note `b73492e` (hang fix) is logically independent of the feature commits. If
> you want to PR it alone, cherry-pick it onto a fresh branch off `main`.

---

## Workstream 1: the hang fix (context, in case it needs revisiting)

**Symptom:** `target/twk check boot/main.tw` (and `build`) froze indefinitely
whenever the source had a compile error. `^C` required. Small projects were fine.

**Root cause:** `boot/compiler/query/analyze.tw` memoized *successful* modules
(they short-circuit at the top of `analyze_module_impl` via `state.exports[...]`)
but never memoized *failures*. In a large import graph, one broken module forced
re-analysis of every module on every path that reaches it — exponential in the
number of paths. With a near-universal dependency (like `diagnostics.tw`) the path
count is astronomical → effectively infinite. Healthy builds are linear (every
module memoizes); stage0 (Rust) never hung because its driver short-circuits
differently.

**Fix:** added `failed: Dict<String, Bool>` to `AnalysisState`, a top-of-function
short-circuit, and a `mark_failed(state, canonical)` helper applied at every
failure return in `analyze_module_impl`. `failed` is rebuilt per analysis run
(`new_state`), so LSP re-analysis after a fix is unaffected. Diagnostics now
report once (dedup bonus).

**How it was proven (reusable technique):** a synthetic exponential-path DAG
reproduced the hang on the *current* binary with **no rebuild** —
`n_i` imports `n_{i+1}` and `n_{i+2}` (Fibonacci paths) funneling to one broken
leaf; ~28 tiny files hung at `timeout 30`. After the fix it returns in 0s.

**Verification done:** synthetic DAG 30s+→0s; `boot/main.tw`+error ∞→1s; healthy
`boot/main.tw` still 2s; `make boot-test` = **2526/2526 pass** and the self-host
loop reaches a fixed point. Regression test added in
`boot/tests/suites/query_analyze_suite.tw` ("a failed module is analyzed once
across multiple paths") — a diamond `main → {a,b} → broken` that asserts exactly
one diagnostic.

---

## Workstream 2: the feature — what's done and what's next

Read `docs/plans/self-describing-diagnostics.md` for the full design + step-by-step
tasks. Quick status:

- **Task 1 DONE** — `FixEdit`/`SuggestedFix` types in `boot/lib/source/report.tw`.
- **Task 2 DONE** — `MissingVariants` payload now carries
  `missing: Vector<MissingVariant>` (named type `MissingVariant = .{name, arity}`
  in `diagnostics.tw`) + `insert_at: Int`. `get_variant_specs` (replaced
  `get_variant_names`) in `checker.tw` supplies arities; emission sets
  `insert_at: s.end - 1` (the byte just before the case's closing `}`, since the
  case `s` span covers the whole expression). `message()`/`help_lines()` updated.
- **Tasks 3–9 TODO**, in order:
  - **3** — `fixes(kind) Vector<SuggestedFix>` projection + `missing_arm_text` +
    `fix_preview_lines` in `diagnostics.tw`; new `boot/tests/suites/fix_suite.tw`.
  - **4** — render the fix preview into CLI `help_lines` in `diag_render.tw`
    (`to_report` `MissingVariants` arm). No `Report` struct change.
  - **5** — `fixes_to_json` in `analyze.tw`; attach generically in `wrap_diags`.
  - **6** — migrate unused-import onto the same `data.fixes` shape
    (`convert_unused_import_diags`).
  - **7** — generic `fix_actions` in `boot/lib/lsp/code_action.tw`; remove
    `unused_import_actions`.
  - **8** — wire `fix_actions` in `boot/lib/lsp/server_core.tw`
    (`handle_code_action`).
  - **9** — full `make boot-test` + `make bundle-cli` + manual CLI/LSP smoke + `make fmt`.

The plan doc's tasks contain exact code. Two corrections already baked in from
this session: `Vector` has **no** `flatten` (use an explicit loop); `analyze.tw`
must add `use lib.source.report as report`.

---

## Environment & workflow notes (IMPORTANT for a fresh session / other harness)

- **Shell is fish**; `${PIPESTATUS[0]}` etc. are bash-isms — prefer simple commands.
- **Fast error-check without a rebuild:** `./target/release/twk build boot/main.tw -o /tmp/x.wasm`
  (this is **stage0**, the Rust compiler — it reports boot type/resolve errors
  quickly and never hung even before the fix). Use this for tight edit loops.
- **To exercise boot *runtime* behavior changes** (anything in `boot/**`), you must
  rebuild the self-hosted CLI: `make bundle-cli` (full self-host loop, a few
  minutes) → rebuilds `target/twk`. `make boot-test` also rebuilds then runs tests.
- **Run one test:** `TWK_TEST_FILTER='<substring>' target/twk run boot/tests/main.tw`
  (matches test name or `suite::test`). Note this uses the *current* `target/twk`
  to compile the test source, so source-only changes to boot libs are picked up
  without a `bundle-cli` **only** for code paths the test imports and runs.
- **Twinkle language gotchas hit this session:**
  - Anonymous record types are **not** allowed in a type position like
    `Vector<.{...}>` — only as a single enum variant payload. Define a named type.
  - Record construction requires **all** fields (no defaults) — adding a field to
    a widely-constructed record (e.g. `Report`, 52 literals) is costly; prefer a
    separate projection function. This is why fixes live in `fixes(kind)`, not on
    `Report`.
  - Style: prefer `xs.map(fn(v) { v.name })` over `collect v in xs { v.name }`
    for simple projections (user preference).
  - `record.dictField[key] = value` rebinding sugar works (e.g.
    `state.failed[canonical] = true`).
- **This harness auto-backgrounds long Bash calls** and they can pile up; if
  `target/twk` seems "slow," check `ps aux | grep twk` for duplicates and
  `pkill -f "target/twk check"` (leave any `twk lsp` — that's the editor).

## Suggested first action in the new session

1. `git -C /Users/curist/playground/rust/twinkle log --oneline -6` to confirm state.
2. Read `docs/plans/self-describing-diagnostics.md` (the plan) and this file.
3. Decide the pending question (continue Tasks 3–9 / split hang fix / pause), then
   proceed from Task 3.
