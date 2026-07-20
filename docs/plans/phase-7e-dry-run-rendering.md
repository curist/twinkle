# Codegen Phase 7E — Dry-Run Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend `twk ir --census --sites` so each candidate update site also shows its `persistent → mutable` catalog target and the ownership dry-run verdict (`would-use` vs `persistent(<reason>)`), without changing any emitted code.

**Architecture:** The ownership analysis already computes per-site verdicts and stores them on `CfgBlock.exit.verdicts` (`Dict<Int,String>` keyed by ANF local id), rendered today by `twk ir --cfg`. Today only record-update / record-get / user-call sites get a verdict; **vector/dict update-call sites do not**. Phase 7E (1) extends `ownership.tw`'s `block_verdicts` to emit a verdict for the vector/dict update-call families by reusing the existing `shell_verdict`, and (2) adds a new `compiler.codegen.dry_run` renderer that joins census candidate sites with those verdicts and with the persistent→mutable catalog pairing (`in_place_equivalent`), surfaced through `twk ir --census --sites`. Variant-routing dry-runs (README bullet 3) already exist in `--cfg` via `render_call_decision` and are intentionally left there.

**Tech Stack:** Twinkle boot compiler (`boot/compiler/**/*.tw`), optimized ANF, ownership CFG analysis, `twk ir` inspection command, boot tests (`boot/tests/**/*.tw`).

---

## Why this shape

The user chose to (a) render dry-run info by **bridging the existing ownership verdicts** rather than building a real decision producer, and (b) surface it by **extending `twk ir --census --sites`**. The one genuinely new piece of analysis — a verdict for `vector$set_unsafe` / `Dict.set` / `Dict.remove` call sites — is the bulk of the value: it also seeds the deferred post-7D producer, which will consume exactly these verdicts. No `MutableDecisionTable` producer is built here; no codegen changes.

## Global constraints

- No emitted-code change. This is inspection output only.
- `census.tw` stays ownership-free (its module doc promises "Contains NO ownership analysis"). The join lives in the new `dry_run.tw`.
- Plain `twk ir --census` (the tally table) output must be **unchanged**. Only `--census --sites` gains columns.
- After each task that edits `.tw` files, run `target/twk fmt <changed files>`, `target/twk lint boot/main.tw` (must end clean; apply `target/twk lint boot/main.tw --fix` for auto-fixable findings — there is no `twk fix` subcommand), and the named tests.
- Do NOT run `make bundle-cli` / `make stage2` / `cargo` during tasks (heavy). A single self-host verification runs once, in the final task.

## Key facts about the current code (verified against `main` at f765af5a)

- `boot/compiler/census.tw`:
  - `pub type CensusSite = .{ func: String, family: String, in_place: Bool }`.
  - `pub fn census_sites(m: AnfModule, b: BuiltinRegistry) Vector<CensusSite>` walks `m.functions`; `walk_expr` sees `.Let(local, op, body)` (the ANF local id is available but currently discarded) and `walk_op` classifies each `.ACall(.AGlobalFunc(fid), _)` via `classify_call` (which has `fid`) and each `.ARecordUpdate(...)`.
  - `build_labels(b, sem) Labels` builds `family_of: Dict<Int,String>` (persistent FuncId → family) and `ip_family_of` (in-place FuncId → family, reversed from `in_place_equivalent`).
  - `pub fn render_sites(sites) String` prints `func\tfamily\tin_place\n` rows.
  - `pub fn tally_from_sites` / `render_census` produce the unchanged tally.
- `boot/compiler/opt/semantics.tw`: `CallSemantics` has `effect` (`.Update` for update ops), `cow_base_arg: Int?` (base operand index — `.Some(0)` for vector set / dict set / dict remove), and `in_place_equivalent: FuncId?` (persistent → mutable pairing). `pub fn call_info(sem, fid) CallSemantics?`.
- `boot/compiler/ownership.tw`:
  - `fn block_verdicts(blk, entry, prep, table, b, sem, suppress) Dict<Int,String>` — iterates `blk.instructions`, keeps `pre := st` (pre-instruction `ForwardState`), fills `verdicts[inst.anf_local.id]`. The `.ACall(callee, args)` arm currently does `case call_info(sem, fid) { .Some(_) => {}, ... }` — i.e. **builtin update calls get no verdict** (this is the line to change; it is around line 1976–1978).
  - `fn shell_verdict(st: ForwardState, base: Atom, last: Vector<Int>) String` returns `"reuse(unique)"` (owned + reusable + last-use), `"persistent(base consumed)"`, `"persistent(base still live)"`, or `"persistent(aliased shell)"`. This is exactly the would-use/fallback verdict for a vector/dict base.
  - `fn atom_brief(a: Atom) String` → `"L${id}"` / `"atom"`.
  - `last_use_at(inst.op, scan.live_after[i])` gives the `last: Vector<Int>` for a site (already computed in `block_verdicts`'s loop as `last`).
  - The driver (around line 3271) does `verdicts := block_verdicts(...)` then `blk.exit.verdicts = verdicts`. So after analysis, `CfgBlock.exit.verdicts` holds each block's verdicts.
  - `pub fn analyze_with_summaries(...)` runs the whole analysis and returns the populated `CfgView`. (Confirm its exact parameters by reading the signature at ~line 2951 before use — the `--cfg` path in `boot/commands/ir.tw:render_cfg_artifacts` shows a working call sequence: `cfg.build_view(artifacts.opt, b)` then the analyze call.)
- `boot/commands/ir.tw`: `--census` handling is:
  ```tw
  if parsed.has_flag("census") {
    sites := census.census_sites(artifacts.opt, artifacts.builtins)
    print(census.tally_from_sites(sites).render_census())
    if parsed.has_flag("sites") {
      print(census.render_sites(sites))
    }
  }
  ```
  `render_cfg_artifacts(artifacts)` (same file) shows how to build the CFG view + run ownership analysis for a whole entry.
- `boot/main.tw` registers `ir` with `.add_flag("census", ...)` and `.add_flag("sites", ...)`.

## File structure

- Modify `boot/compiler/census.tw`
  - Add `local: Int` and `persistent_fid: Int` to `CensusSite` (record updates use `persistent_fid = -1`). Capture the `.Let` local id and the classified call `fid` during the walk. Keep census ownership-free; `render_sites`/tally behavior unchanged in shape (they simply ignore the new fields, except the new fields are threaded through construction).
- Create `boot/compiler/codegen/dry_run.tw`
  - The dry-run join + render. Consumes `census_sites` + ownership verdicts (`CfgBlock.exit.verdicts`) + the persistent→mutable pairing. Owns `DryRunSite`, `dry_run_sites(...)`, `render_dry_run(...)`.
- Modify `boot/compiler/ownership.tw`
  - Extend `block_verdicts`'s `.ACall` builtin arm to emit a verdict for `.Update`-effect calls with a `cow_base_arg`, reusing `shell_verdict`.
- Modify `boot/commands/ir.tw`
  - When `--census --sites` is set, print the dry-run table (from `dry_run.tw`) instead of / in addition to the plain site list.
- Modify `boot/tests/suites/census_suite.tw` (or the existing census/cfg suites — confirm names) and add coverage in a dry-run suite.
- Modify `docs/plans/sound-uniqueness/codegen/README.md` — mark Phase 7E done.

---

### Task 1: Extend `block_verdicts` to verdict vector/dict update-call sites

This is the one new piece of analysis. It also changes `twk ir --cfg` output (those sites gain a verdict line), so `--cfg` golden expectations must be updated.

**Files:**
- Modify: `boot/compiler/ownership.tw` (the `.ACall` arm in `block_verdicts`, ~line 1976)
- Test: `boot/tests/suites/cfg_ownership_suite.tw` (or the suite that asserts `--cfg`/verdict output — confirm by `rg -l "exit.verdicts|block_verdicts|shell_verdict|reuse\(unique\)" boot/tests`)

- [ ] **Step 1: Find the verdict/cfg test suite and a place to assert on a vector-set verdict**

Run:
```bash
rg -ln "exit\.verdicts|reuse\(unique\)|persistent\(base|render_cfg|block_verdicts" boot/tests -g '*.tw'
```
Pick the suite that already exercises ownership verdicts (likely `cfg_ownership_suite.tw` or `cfg_ownership_facts_suite.tw`). Read how it drives the analysis and reads `blk.exit.verdicts` so your new test mirrors the existing helper style.

- [ ] **Step 2: Write a failing test asserting an owned vector-set site gets a verdict**

Add a test that analyzes this source and asserts the `vector$set_unsafe` site’s verdict is `reuse(unique)`:
```tw
fn set_owned(xs: Vector<Int>) Vector<Int> {
  xs[0] = 9
  xs
}
```
Use the suite’s existing "build view → analyze → read block.exit.verdicts" helper. Assert the verdict string for the update site contains `reuse(unique)` (the base `xs` is `Unique` + last-use). Before Task 1’s implementation this site has **no** verdict entry, so the test fails (missing key / empty).

If the suite has no such helper, add one that walks the analyzed `CfgView`’s blocks and collects `blk.exit.verdicts` into one `Dict<Int,String>`, then asserts on the value.

- [ ] **Step 3: Run the test, verify it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: the new assertion fails because the vector-set site currently has no verdict.

- [ ] **Step 4: Implement the verdict in `block_verdicts`**

In `boot/compiler/ownership.tw`, change the `.ACall` builtin arm from:
```tw
.ACall(callee, args) => case callee_func_id(callee) {
  .Some(fid) => case call_info(sem, fid) {
    .Some(_) => {}, // builtin semantics -> no user-call specialization verdict
    .None => case table.summary_get(fid.id) {
      // ... unchanged user-call specialization ...
    },
  },
  _ => {},
},
```
to compute a base-shell verdict for `.Update` builtins that name a `cow_base_arg`:
```tw
.ACall(callee, args) => case callee_func_id(callee) {
  .Some(fid) => case call_info(sem, fid) {
    .Some(cs) => case cs.effect {
      .Update => case cs.cow_base_arg {
        .Some(bi) => if bi >= 0 and bi < args.len() {
          base := args[bi]
          verdicts[inst.anf_local.id] = "L${inst.anf_local.id} = update ${atom_brief(base)} base=${pre.shell_verdict(
            base,
            last,
          )}"
        },
        .None => {},
      },
      _ => {},
    },
    .None => case table.summary_get(fid.id) {
      // ... unchanged user-call specialization block ...
    },
  },
  _ => {},
},
```
Notes:
- `pre` and `last` are already in scope in the loop (`pre := st`, `last := last_use_at(inst.op, scan.live_after[i])`).
- `shell_verdict` is an inherent method on `ForwardState` (`pre.shell_verdict(base, last)`), so this reuses the exact owned/consumed/aliased logic.
- Keep the existing `.None => case table.summary_get(...)` user-call branch **unchanged** — only the `.Some(_)` builtin branch changes (from `{}` to the effect/cow_base_arg match).

- [ ] **Step 5: Run the new test + full suite; update `--cfg` goldens**

Run: `target/twk run boot/tests/main.tw`
Expected: the new test passes. If any existing `--cfg`/verdict golden test now fails because vector/dict sites gained a verdict line, update those expected strings to include the new `update ... base=<verdict>` line (this is an intended inspection improvement, not a regression). Re-run until green.

- [ ] **Step 6: Format, lint**

Run:
```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_suite.tw
target/twk lint boot/main.tw
```

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_suite.tw
git commit -m "ownership: render dry-run verdict for vector/dict update-call sites"
```

---

### Task 2: Carry ANF local id + persistent FuncId on census sites

Keeps `census.tw` ownership-free but gives the dry-run join the keys it needs: the site’s ANF local id (to look up the verdict) and the persistent call FuncId (to look up the mutable target).

**Files:**
- Modify: `boot/compiler/census.tw`
- Test: `boot/tests/suites/census_suite.tw` (confirm the exact suite name via `rg -ln "census_sites|render_census|CensusSite" boot/tests`)

- [ ] **Step 1: Write a failing test for the new site fields**

In the census suite, add a test compiling:
```tw
fn f(xs: Vector<Int>) Vector<Int> {
  xs[0] = 9
  xs
}
```
Call `census.census_sites(artifacts.opt, artifacts.builtins)`, find the `vector_set` site, and assert its new fields: `site.local` equals the ANF local id of the `xs[0]=9` update binding (nonzero), and `site.persistent_fid` equals `artifacts.builtins.id("vector$set_unsafe").id`. This fails to compile until the fields exist.

- [ ] **Step 2: Run, verify failure**

Run: `target/twk run boot/tests/main.tw`
Expected: compile failure — `CensusSite` has no field `local` / `persistent_fid`.

- [ ] **Step 3: Add the fields and capture them in the walk**

In `boot/compiler/census.tw`:
- Extend the type:
```tw
pub type CensusSite = .{ func: String, family: String, in_place: Bool, local: Int, persistent_fid: Int }
```
- `walk_expr`’s `.Let(_, op, body)` currently discards the local. Thread it into `walk_op` so each constructed `CensusSite` carries it. Change `walk_expr`:
```tw
.Let(local, op, body) => {
  after := walk_op(op, local.id, func, sites, sem, labels)
  walk_expr(body, func, after, sem, labels)
},
```
- Update `walk_op`’s signature to accept `local_id: Int` and set it on every `CensusSite` it builds. For `.ARecordUpdate(...)`, use `persistent_fid: -1` (record updates have no call FuncId). For the `.ACall(.AGlobalFunc(fid), _)` case, use `persistent_fid: fid.id`. Example:
```tw
.ARecordUpdate(_, _, _, in_place, _) => sites.append(CensusSite.{
  func,
  family: "record_update",
  in_place,
  local: local_id,
  persistent_fid: 0 - 1,
}),
.ACall(callee, _) => case callee {
  .AGlobalFunc(fid) => case classify_call(fid, sem, labels) {
    .Some(fi) => sites.append(CensusSite.{
      func,
      family: fi.family,
      in_place: fi.in_place,
      local: local_id,
      persistent_fid: fid.id,
    }),
    .None => sites,
  },
  _ => sites,
},
```
- Nested `.AIf` / `.AMatch` / `.ALoop` / `.ADefer` arms in `walk_op` currently recurse with `walk_expr` — those recurse into bodies whose own `.Let` locals will be captured by `walk_expr`, so pass through the current `sites` accumulator unchanged (their own sites get correct locals from the inner `walk_expr`). Keep them structurally identical, just adapt to the new `walk_op` signature (they don’t construct `CensusSite`s directly).
- `render_sites` may keep printing `func\tfamily\tin_place` (the new fields are for the dry-run renderer, not the plain listing). Leaving `render_sites` as-is keeps any existing plain-listing tests valid.

- [ ] **Step 4: Run tests**

Run: `target/twk run boot/tests/main.tw`
Expected: the new field test passes; existing census tally/site tests unchanged (tally ignores the new fields).

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/census.tw boot/tests/suites/census_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/census.tw boot/tests/suites/census_suite.tw
git commit -m "census: carry anf local id and persistent func id on sites"
```

---

### Task 3: Add the dry-run join + renderer

**Files:**
- Create: `boot/compiler/codegen/dry_run.tw`
- Test: `boot/tests/suites/dry_run_suite.tw` (new; register in `boot/tests/main.tw`)

**Interfaces:**
- `pub type DryRunSite = .{ func: String, family: String, persistent: String, mutable: String, verdict: String, would_use: Bool, local: Int }`
- `pub fn dry_run_sites(opt: AnfModule, env: ResolvedEnv, b: BuiltinRegistry) Vector<DryRunSite>`
- `pub fn render_dry_run(sites: Vector<DryRunSite>) String`

- [ ] **Step 1: Write failing tests**

Create `boot/tests/suites/dry_run_suite.tw`. Register it in `boot/tests/main.tw`. Add tests over compiled sources:

Owned vector set → `would_use = true`, targets named:
```tw
fn set_owned(xs: Vector<Int>) Vector<Int> {
  xs[0] = 9
  xs
}
```
Assert the `vector_set` `DryRunSite` has `persistent == "vector$set_unsafe"`, `mutable == "vector$set_in_place"`, `would_use == true`, and `verdict` contains `reuse(unique)`.

Aliased vector set → `would_use = false` (base escapes, so persistent):
```tw
fn set_aliased(xs: Vector<Int>) Vector<Int> {
  ys := xs
  xs[0] = 9
  ys
}
```
Assert the `vector_set` site has `would_use == false` and `verdict` contains `persistent(`.

Dict set + dict remove: assert `persistent`/`mutable` names are `dict$set`/`dict$set_in_place` and `dict$remove`/`dict$remove_in_place` respectively (use `b.method_id("Dict","set")`/`b.id("dict$set_in_place")` to derive expected names — see Step 3 for how names are produced).

Record update: assert the `record_update` site’s `persistent`/`mutable` read `struct.new(copy)` / `struct.set(reuse)` (the record family has no call FuncId; special-cased).

- [ ] **Step 2: Run, verify failure**

Run: `target/twk run boot/tests/main.tw`
Expected: compile failure naming `compiler.codegen.dry_run`.

- [ ] **Step 3: Implement `boot/compiler/codegen/dry_run.tw`**

The module:
1. Enumerates candidate sites via `census.census_sites(opt, b)` (now carrying `local` + `persistent_fid`).
2. Runs the ownership analysis to collect per-`(func, local)` verdict strings.
3. Bridges each site’s `persistent_fid` → mutable target via `in_place_equivalent`, and formats builtin names.
4. Classifies `would_use` from the verdict prefix.

Concrete implementation notes:
- **Builtin name lookup:** find the helper that maps a `FuncId` back to its wasm/registry name (search `rg -n "fn .*name.*FuncId|builtin_name|fn name_of|by_id" boot/compiler/builtins.tw`). Use it to render `persistent`/`mutable`. If the registry exposes names keyed by id, build a small `Int -> String` lookup once. For the persistent name use `persistent_fid`; for the mutable name use `in_place_equivalent` of that id.
- **`in_place_equivalent`:** obtain via `make_prelude_optimizer_semantics(b)` then `call_info(sem, FuncId.{ id: persistent_fid })` → `cs.in_place_equivalent`. (Mirror `census.build_labels`, which already reads `sem.call_semantics[...].in_place_equivalent`.)
- **Record-update special case:** `persistent_fid == -1` → `persistent = "struct.new(copy)"`, `mutable = "struct.set(reuse)"`.
- **Verdict collection:** run the same view+analyze sequence `render_cfg_artifacts` uses:
  ```tw
  view := cfg.build_view(opt, b)
  analyzed := ownership.analyze_with_summaries(view, /* args per its signature */)
  ```
  Confirm `analyze_with_summaries`’s parameter list at `ownership.tw:~2951` and the `CfgView` shape (how to iterate its functions/blocks) at `boot/compiler/cfg.tw`. Then fold every block’s `blk.exit.verdicts` into a per-function map. **Local ids repeat across functions**, so key by `(func_name, local)` — e.g. `Dict<String, Dict<Int, String>>` keyed first by the block’s function name, or build one combined key string `"${func}#${local}"`. Match census’s `func` string to the CFG function’s name so the join keys line up (verify how the CFG view names functions).
- **would_use classification:** `verdict.contains("reuse(") or verdict.contains("in-place(")` → `true`; else `false`. Empty/missing verdict → `would_use = false`, `verdict = "-"`.
- **Render:** tab-separated table with a header:
  ```
  func\tfamily\tpersistent\tmutable\twould_use\tverdict
  ```

Skeleton:
```tw
//! Phase 7E dry-run rendering: joins census candidate sites with ownership
//! verdicts and the persistent->mutable catalog pairing. Inspection only —
//! no emitted-code change, no decision producer.

use compiler.anf.{AnfModule}
use compiler.builtins.{BuiltinRegistry}
use compiler.census
use compiler.cfg
use compiler.opt.semantics.{call_info, make_prelude_optimizer_semantics}
use compiler.ownership
use compiler.resolver.{ResolvedEnv}
use compiler.core_ir.{FuncId}

pub type DryRunSite = .{
  func: String,
  family: String,
  persistent: String,
  mutable: String,
  verdict: String,
  would_use: Bool,
  local: Int,
}

pub fn dry_run_sites(opt: AnfModule, env: ResolvedEnv, b: BuiltinRegistry) Vector<DryRunSite> {
  sem := make_prelude_optimizer_semantics(b)
  sites := census.census_sites(opt, b)
  verdicts := collect_verdicts(opt, env, b) // Dict<String, String>, key "${func}#${local}"

  out: Vector<DryRunSite> = []
  for s in sites {
    key := "${s.func}#${s.local}"
    v := case verdicts.get(key) {
      .Some(text) => text,
      .None => "-",
    }
    targets := targets_for(s, sem, b) // .{ persistent, mutable }
    out = out.append(DryRunSite.{
      func: s.func,
      family: s.family,
      persistent: targets.persistent,
      mutable: targets.mutable,
      verdict: v,
      would_use: v.contains("reuse(") or v.contains("in-place("),
      local: s.local,
    })
  }
  out
}
```
Implement `collect_verdicts` (build view + analyze + fold `blk.exit.verdicts` keyed by `"${func}#${local}"`), `targets_for` (name bridge + record-update special case), and `render_dry_run` following the notes above. Confirm every external signature (`analyze_with_summaries`, `cfg.build_view`, the CFG-view iteration, the builtin name lookup) by reading the cited files before finalizing.

- [ ] **Step 4: Run tests**

Run: `target/twk run boot/tests/main.tw`
Expected: dry-run tests pass (owned→would_use, aliased→persistent, dict set/remove names, record-update special case).

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/codegen/dry_run.tw boot/tests/suites/dry_run_suite.tw boot/tests/main.tw
target/twk lint boot/main.tw
git add boot/compiler/codegen/dry_run.tw boot/tests/suites/dry_run_suite.tw boot/tests/main.tw
git commit -m "codegen: add 7E dry-run site join and renderer"
```

---

### Task 4: Surface the dry-run table via `twk ir --census --sites`

**Files:**
- Modify: `boot/commands/ir.tw`
- Test: `boot/tests/suites/ir_command_suite.tw` (confirm name via `rg -ln "run_ir_command|--census|render_census" boot/tests`)

- [ ] **Step 1: Write a failing test for the `--census --sites` output**

Add a test that runs the ir census+sites path (reuse whatever harness the ir suite uses to invoke `run_ir_command` or the render helpers) over:
```tw
fn f(xs: Vector<Int>) Vector<Int> {
  xs[0] = 9
  xs
}
```
Assert the emitted `--census --sites` text contains the dry-run header (`persistent\tmutable\twould_use\tverdict` columns) and a row naming `vector$set_unsafe` and `vector$set_in_place`. Assert the plain `--census` tally text is unchanged (still `family\tcandidates\tin_place`).

- [ ] **Step 2: Run, verify failure**

Run: `target/twk run boot/tests/main.tw`
Expected: the assertion fails — current `--sites` prints only `func\tfamily\tin_place`.

- [ ] **Step 3: Wire the dry-run renderer into the census command**

In `boot/commands/ir.tw`, add `use compiler.codegen.dry_run`. Change the census block so `--sites` prints the dry-run table:
```tw
if parsed.has_flag("census") {
  sites := census.census_sites(artifacts.opt, artifacts.builtins)
  print(census.tally_from_sites(sites).render_census())
  if parsed.has_flag("sites") {
    dry := dry_run.dry_run_sites(artifacts.opt, artifacts.env, artifacts.builtins)
    print(dry_run.render_dry_run(dry))
  }
}
```
(Confirm `artifacts.env` is available on the ir command’s `PipelineArtifacts` — it is per the pipeline; if the census branch doesn’t already have `env` in scope, thread it from `artifacts`.) The plain tally print is untouched.

- [ ] **Step 4: Run tests**

Run: `target/twk run boot/tests/main.tw`
Expected: ir census+sites test passes; tally output unchanged.

- [ ] **Step 5: Manual smoke check**

Run:
```bash
printf 'fn f(xs: Vector<Int>) Vector<Int> {\n  xs[0] = 9\n  xs\n}\nprintln(f([1,2]).len().to_string())\n' > /tmp/7e.tw
target/twk ir /tmp/7e.tw --census --sites
```
Expected: the tally table, then a per-site table with `persistent=vector$set_unsafe mutable=vector$set_in_place would_use=... verdict=...` for the `vector_set` row. (This uses the current `target/twk`, i.e. the pre-change compiler payload, so the dry-run columns only appear after the final `make bundle-cli` in Task 5 — until then, rely on the boot-test assertions, which compile the new code from source.)

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/commands/ir.tw boot/tests/suites/ir_command_suite.tw
target/twk lint boot/main.tw
git add boot/commands/ir.tw boot/tests/suites/ir_command_suite.tw
git commit -m "ir: show dry-run targets and verdicts in census --sites"
```

---

### Task 5: Docs + self-host verification

**Files:**
- Modify: `docs/plans/sound-uniqueness/codegen/README.md`

- [ ] **Step 1: Mark Phase 7E done in the codegen README**

In `docs/plans/sound-uniqueness/codegen/README.md`, change the Phase 7E header to `## Codegen Phase 7E — Dry-run rendering ✅ done (<today>)` and check its boxes, describing the implemented surface:
- `[x]` Print dry-run rewrite targets — `twk ir --census --sites` now shows `persistent → mutable` per candidate site via `compiler.codegen.dry_run`.
- `[x]` Render consumed vs ignored decisions — each site shows `would_use` + the ownership `verdict` (`reuse(unique)` vs `persistent(<reason>)`), backed by the new vector/dict verdict coverage in `ownership.tw`.
- `[x]` Variant routing dry-runs — already served by `twk ir --cfg` (`render_call_decision`); cross-referenced, not duplicated into the update-op census.

Also update the top-of-file **Status** line’s "Next" pointer from "Phase 7E or the analysis-producer follow-up" to just the analysis-producer follow-up (Phase 8 remains the first emitted-code change).

- [ ] **Step 2: Full verification incl. self-host**

Run sequentially (never concurrently):
```bash
target/twk fmt boot/compiler/ownership.tw boot/compiler/census.tw boot/compiler/codegen/dry_run.tw boot/commands/ir.tw boot/tests/suites/dry_run_suite.tw boot/tests/main.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
make bundle-cli
make boot-test
git diff --check
```
Expected: fmt idempotent, lint clean, boot tests green, `make bundle-cli` reaches the `stage3 == stage4` fixed point (proving the new inspection code self-hosts), `make boot-test` green against the rebuilt payload, `git diff --check` prints nothing.

- [ ] **Step 3: Commit**

```bash
git add docs/plans/sound-uniqueness/codegen/README.md
git commit -m "docs: mark codegen 7E dry-run rendering done"
```

---

## Acceptance criteria

- `twk ir --census --sites` shows, per candidate update site: `persistent` and `mutable` catalog target names, a `would_use` bool, and the ownership `verdict` string.
- `vector$set_unsafe` / `Dict.set` / `Dict.remove` call sites now receive an ownership verdict (`reuse(unique)` when the base is owned + last-use; `persistent(<reason>)` otherwise), in both `--census --sites` and `--cfg`.
- Owned-base updates render `would_use = true`; aliased/borrowed-base updates render `would_use = false`.
- Plain `twk ir --census` (the tally) output is byte-unchanged.
- `census.tw` performs no ownership analysis; the join lives in `dry_run.tw`.
- No emitted-code change: `make bundle-cli` reaches the stage3 == stage4 fixed point; boot tests green.
- Variant-routing dry-run is left to `--cfg`; not duplicated here.

## Self-review notes

- Spec coverage: README bullets 1 (persistent→mutable targets) and 2 (consumed-vs-ignored via would_use/verdict) are implemented in `--census --sites`; bullet 3 (variant routing) is explicitly delegated to the existing `--cfg` renderer.
- Risk — two external surfaces need live-signature confirmation before coding: `ownership.analyze_with_summaries` (params) and the `CfgView` iteration/function-naming used by `collect_verdicts`. Both are read-and-confirm steps in Task 3, grounded by the working `render_cfg_artifacts` precedent in `boot/commands/ir.tw`.
- Risk — Task 1 changes `--cfg` output; Task 1 Step 5 updates affected goldens.
- Scope: no `MutableDecisionTable` producer, no codegen change, no variant-routing rework.
