# Codegen Phase 7E (update-site slice) — Dry-Run Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend `twk ir --census --sites` so each **update candidate** (vector/dict/record update site) shows its `persistent → mutable` catalog target and an ownership dry-run verdict (`would-use` vs `persistent(<reason>)`), without changing any emitted code.

**Architecture:** The ownership analysis already stores per-site verdicts on `CfgBlock.exit.verdicts` (`Dict<Int,String>` keyed by ANF local id), rendered today by `twk ir --cfg` — but only for record-update / record-get / user-call sites; **vector/dict update-call sites get no verdict**. This slice (1) extends `ownership.tw`'s `block_verdicts` to verdict the vector/dict update-call families by reusing the existing `shell_verdict`, and (2) adds a `compiler.codegen.dry_run` renderer that joins census candidate sites with those verdicts and with the `in_place_equivalent` catalog pairing, surfaced through `twk ir --census --sites`.

**Tech Stack:** Twinkle boot compiler (`boot/compiler/**/*.tw`), optimized ANF, ownership CFG analysis, `twk ir` inspection command, boot tests.

---

## Scope — this is a SLICE of README Phase 7E, not the whole phase

README Phase 7E lists three bullets. This plan deliberately implements only the first, in the honest form the compiler can actually show today:

**IN scope:**
- **Bullet 1 (rewrite targets):** render `persistent → mutable` per update candidate.
- **Bullet 2, narrowed:** render the **ownership verdict** per candidate (`reuse(unique)` / `in-place(...)` → would-use; `persistent(<reason>)` → fallback). This is an *ownership-verdict dry-run*, NOT backend decision-table state.

**OUT of scope (leave the README boxes UNCHECKED / deferred):**
- **Bullet 3 (variant-routing dry-run):** requires generic callee + would-be clone name + exact `VariantId` + route site + fallback reason. Today `render_call_decision` renders only `-> f<id>[unique:...]` (2 of 5 fields), so `--cfg` does NOT actually cover this. Deferred to a later variant-routing task.
- **True decision-table dry-run** ("decision found but dry-run" vs "absent/stale"): there is no `MutableDecisionTable` producer yet (deferred post-7D follow-up). Rendering ownership verdicts is *not* the same as rendering decision-table state; do not claim it is.

Task 5 marks only the update-site dry-run done and rewords the README accordingly.

## Global constraints

- No emitted-code change. Inspection output only.
- `census.tw` stays ownership-free (its module doc promises "Contains NO ownership analysis"). The ownership/census/catalog join lives in the new `dry_run.tw`.
- Plain `twk ir --census` (the tally table) output must be **byte-unchanged**. Only `--census --sites` gains columns.
- After each task that edits `.tw` files: `target/twk fmt <changed files>`, `target/twk lint boot/main.tw` (must end clean; `target/twk lint boot/main.tw --fix` for auto-fixable findings — there is no `twk fix` subcommand), and the named tests.
- Do NOT run `make bundle-cli`/`make stage2`/`cargo` during tasks. One self-host verification runs in the final task.

## Verified facts about the current code (checked against `main` at f765af5a)

- `boot/compiler/opt/semantics.tw` `CallSemantics`: `effect` (`.Update` for updates), `cow_base_arg: Int?` (base operand index; `.Some(0)` for vector-set / dict-set / dict-remove **and** for at least one update family with **`in_place_equivalent: .None`** — e.g. a vector append/other), `in_place_equivalent: FuncId?`. `pub fn call_info(sem, fid) CallSemantics?`. **Consequence:** a `.Update` site with a `reuse(unique)` verdict may still have NO mutable target, so `would_use` must require a mutable target to exist (see Task 3).
- `boot/compiler/census.tw`: `census_sites(m, b)` walks `m.functions`; `walk_expr` sees `.Let(local, op, body)` (local id currently discarded); the `.ACall(.AGlobalFunc(fid), _)` case has `fid`; `.ARecordUpdate` has no call FuncId. `build_labels` builds `family_of` (persistent id → family) and `ip_family_of` (in-place id → family). Contains no ownership analysis.
- `boot/compiler/ownership.tw`:
  - `fn block_verdicts(...)` — the `.ACall(callee, args)` builtin arm is `case call_info(sem, fid) { .Some(_) => {}, ... }` (builtin update calls get no verdict; ~line 1976). `pre` (pre-instruction `ForwardState`) and `last` (`last_use_at(...)`) are in scope in the loop.
  - `fn shell_verdict(st: ForwardState, base: Atom, last) String` → `"reuse(unique)"` / `"persistent(base consumed|base still live|aliased shell)"`. Reused as-is for update bases. `atom_brief(a)` → `"L${id}"`.
  - Driver sets `blk.exit.verdicts` (~line 3277).
  - `pub fn analyze_with_summaries(view: CfgView, b: BuiltinRegistry, sem: OptimizerSemantics, table: SummaryTable) CfgView` (~line 2951).
  - `pub fn prune_dead_merge(view) CfgView`.
- `boot/commands/ir.tw` `render_cfg_artifacts` shows the exact analysis sequence to reuse:
  ```tw
  view := cfg.build_view(artifacts.opt, b)
  view = ownership.prune_dead_merge(view)
  s := semantics.make_prelude_optimizer_semantics(b)
  table := summary.compute(view, b, s)
  // summary.compute_variants(...) is ONLY for variant rendering — NOT needed here
  analyzed := ownership.analyze_with_summaries(view, b, s, table)
  ```
  The `--census` handling is:
  ```tw
  if parsed.has_flag("census") {
    sites := census.census_sites(artifacts.opt, artifacts.builtins)
    print(census.tally_from_sites(sites).render_census())
    if parsed.has_flag("sites") { print(census.render_sites(sites)) }
  }
  ```
- Builtin name lookup: `BuiltinRegistry` has `by_id: Dict<Int, BuiltinEntry>`; a `BuiltinEntry` carries the registered symbolic name (e.g. `vector$set_unsafe`). Confirm the exact field name by reading `boot/compiler/builtins.tw:BuiltinEntry` before use.
- Test suites: the census gate suite is `boot/tests/suites/uniqueness_census_suite.tw`. There is **no** `ir_command_suite` / stdout-capture harness — test pure functions, not CLI stdout.

## File structure

- Modify `boot/compiler/ownership.tw` — verdict vector/dict update-call sites in `block_verdicts`.
- Modify `boot/compiler/census.tw` — `CensusSite` gains `local: Int` and `actual_fid: Int` (the callee id; `-1` for record updates). Stays ownership-free.
- Create `boot/compiler/codegen/dry_run.tw` — the join + renderer. Owns `DryRunSite`, `dry_run_sites`, `render_dry_run`, and the target-name derivation.
- Modify `boot/commands/ir.tw` — a pure `render_census_report(artifacts, include_sites) String` helper, wired into `--census`/`--sites`.
- Modify `boot/tests/suites/uniqueness_census_suite.tw`; create `boot/tests/suites/dry_run_suite.tw` (register in `boot/tests/main.tw`).
- Modify `docs/plans/sound-uniqueness/codegen/README.md`.

---

### Task 1: Verdict vector/dict update-call sites in `block_verdicts`

The one new analysis piece. It also adds these verdicts to `twk ir --cfg`, so `--cfg` goldens may need updating.

**Files:**
- Modify: `boot/compiler/ownership.tw` (the `.ACall` arm in `block_verdicts`, ~line 1976)
- Test: the suite that asserts ownership verdicts — find it first.

- [ ] **Step 1: Find the verdict-assertion suite + helper**

Run: `rg -ln "exit\.verdicts|reuse\(unique\)|persistent\(base|block_verdicts" boot/tests -g '*.tw'`
Read the matching suite (e.g. `cfg_ownership_suite.tw` / `cfg_ownership_facts_suite.tw`) to reuse its "build view → analyze → read `blk.exit.verdicts`" helper.

- [ ] **Step 2: Write failing tests (owned → verdict present; aliased → persistent)**

Add tests analyzing these and asserting the `vector$set_unsafe` site’s verdict:
```tw
fn set_owned(xs: Vector<Int>) Vector<Int> {
  xs[0] = 9
  xs
}
```
Assert the update site’s verdict contains `reuse(unique)`.
```tw
fn set_aliased(xs: Vector<Int>) Vector<Int> {
  ys := xs
  xs[0] = 9
  ys
}
```
Assert its verdict contains `persistent(`. Before implementation the vector-set site has no verdict entry, so the first test fails.

- [ ] **Step 3: Run, verify failure** — `target/twk run boot/tests/main.tw` (owned-case assertion fails: missing verdict).

- [ ] **Step 4: Implement the verdict**

Change the `.ACall` builtin arm’s `.Some(_) => {}` to compute a base-shell verdict for `.Update` builtins that name a `cow_base_arg`:
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
      // ... existing user-call specialization block UNCHANGED ...
    },
  },
  _ => {},
},
```
Only the `.Some` builtin branch changes; leave the `.None` user-call branch untouched.

- [ ] **Step 5: Run new tests + full suite; update `--cfg` goldens**

`target/twk run boot/tests/main.tw`. If existing `--cfg`/verdict goldens now include the new `update ... base=...` lines, update those expected strings (intended inspection improvement). Re-run until green.

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw <the verdict suite file>
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw <the verdict suite file>
git commit -m "ownership: render dry-run verdict for vector/dict update-call sites"
```

---

### Task 2: Carry ANF local id + actual callee id on census sites

Gives the join its keys, keeping census ownership-free. Stores the **actual** callee id (not "persistent") so the derivation is correct even if a future compiler emits an in-place callee.

**Files:**
- Modify: `boot/compiler/census.tw`
- Test: `boot/tests/suites/uniqueness_census_suite.tw`

- [ ] **Step 1: Failing test for the new fields**

Compile `fn f(xs: Vector<Int>) Vector<Int> { xs[0] = 9  xs }`, find the `vector_set` site, assert `site.local` is the update binding’s ANF local id (nonzero) and `site.actual_fid == artifacts.builtins.id("vector$set_unsafe").id`.

- [ ] **Step 2: Run, verify failure** — compile error: no field `local` / `actual_fid`.

- [ ] **Step 3: Add fields + capture in the walk**

```tw
pub type CensusSite = .{ func: String, family: String, in_place: Bool, local: Int, actual_fid: Int }
```
Thread the `.Let` local into `walk_op`:
```tw
.Let(local, op, body) => {
  after := walk_op(op, local.id, func, sites, sem, labels)
  walk_expr(body, func, after, sem, labels)
},
```
Give `walk_op` a `local_id: Int` param; set it on every `CensusSite`. Record updates → `actual_fid: 0 - 1`; call sites → `actual_fid: fid.id`:
```tw
.ARecordUpdate(_, _, _, in_place, _) => sites.append(CensusSite.{
  func, family: "record_update", in_place, local: local_id, actual_fid: 0 - 1,
}),
.ACall(callee, _) => case callee {
  .AGlobalFunc(fid) => case classify_call(fid, sem, labels) {
    .Some(fi) => sites.append(CensusSite.{
      func, family: fi.family, in_place: fi.in_place, local: local_id, actual_fid: fid.id,
    }),
    .None => sites,
  },
  _ => sites,
},
```
Adapt the nested `.AIf/.AMatch/.ALoop/.ADefer` arms to the new `walk_op` signature (they recurse via `walk_expr`, which captures their inner `.Let` locals — no `CensusSite` built directly there). Leave `render_sites`, `tally_from_sites`, `render_census` behavior unchanged (they ignore the new fields).

- [ ] **Step 4: Run tests** — field test passes; tally/site tests unchanged.

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/census.tw boot/tests/suites/uniqueness_census_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/census.tw boot/tests/suites/uniqueness_census_suite.tw
git commit -m "census: carry anf local id and actual callee id on sites"
```

---

### Task 3: The dry-run join + renderer

**Files:**
- Create: `boot/compiler/codegen/dry_run.tw`
- Test: `boot/tests/suites/dry_run_suite.tw` (new; register in `boot/tests/main.tw`)

**Interfaces:**
- `pub type DryRunSite = .{ func: String, family: String, persistent: String, mutable: String, verdict: String, would_use: Bool, local: Int }`
- `pub fn dry_run_sites(opt: AnfModule, b: BuiltinRegistry) Vector<DryRunSite>`
- `pub fn render_dry_run(sites: Vector<DryRunSite>) String`

- [ ] **Step 1: Failing tests**

Create `dry_run_suite.tw`, register it. Tests over compiled sources:

Owned vector set → mutable target exists + owned ⇒ `would_use = true`:
```tw
fn set_owned(xs: Vector<Int>) Vector<Int> { xs[0] = 9  xs }
```
Assert `persistent == "vector$set_unsafe"`, `mutable == "vector$set_in_place"`, `would_use == true`, `verdict` contains `reuse(unique)`.

Aliased vector set → owned-check fails ⇒ `would_use = false`, `verdict` contains `persistent(`:
```tw
fn set_aliased(xs: Vector<Int>) Vector<Int> { ys := xs  xs[0] = 9  ys }
```

Dict set / remove → `persistent`/`mutable` are `dict$set`/`dict$set_in_place` and `dict$remove`/`dict$remove_in_place`.

Record update (owned) → `persistent == "struct.new(copy)"`, `mutable == "struct.set(reuse)"`, `would_use == true`.

**Update family with no mutable target** (`in_place_equivalent: .None`, e.g. vector append/other — find one via `rg -n "in_place_equivalent: .None" boot/compiler/opt/semantics.tw` and a source that emits it) → assert `mutable == "-"` and `would_use == false` **even if** the ownership verdict is `reuse(unique)`. This is the anti-regression for the would_use bug.

- [ ] **Step 2: Run, verify failure** — missing module `compiler.codegen.dry_run`.

- [ ] **Step 3: Implement `dry_run.tw`**

Analysis sequence — mirror `render_cfg_artifacts` exactly (verified):
```tw
view := cfg.build_view(opt, b)
view = ownership.prune_dead_merge(view)
sem := make_prelude_optimizer_semantics(b)
table := summary.compute(view, b, sem)
analyzed := ownership.analyze_with_summaries(view, b, sem, table)
```
Do NOT call `summary.compute_variants` (only for variant rendering).

**Verdict collection:** fold every block’s `blk.exit.verdicts` across `analyzed.functions` into a map keyed by `"${func}#${local}"`. Local ids repeat across functions, so the function name MUST be part of the key. Confirm how a `CfgFunction` exposes its name (read `boot/compiler/cfg.tw`) and that it matches census’s `func` string; if the block does not carry its function name directly, collect per-function during the `analyzed.functions` iteration.

**Target derivation (`targets_for`)** — build forward + reverse `in_place_equivalent` maps from `sem.call_semantics`, then, for a site’s `actual_fid`:
- `actual_fid == -1` (record) → `.{ persistent: "struct.new(copy)", mutable: "struct.set(reuse)", has_mutable: true }`.
- else if `call_info(sem, FuncId.{ id: actual_fid })` is `.Some(cs)`:
  - `cs.in_place_equivalent` is `.Some(mid)` → persistent = name(actual_fid), mutable = name(mid), has_mutable = true. (actual is a persistent-form op)
  - else if actual_fid is a value in the reverse map (actual is itself an in-place op) → persistent = name(reverse[actual_fid]), mutable = name(actual_fid), has_mutable = true.
  - else → persistent = name(actual_fid), mutable = "-", has_mutable = false. (persistent-form update, no mutable target)
- else → persistent = "?", mutable = "-", has_mutable = false.

`name(id)`: look up `b.by_id[id]`’s symbolic name (confirm the field on `BuiltinEntry`).

**would_use** = `(verdict.contains("reuse(") or verdict.contains("in-place("))` **AND** `has_mutable`. Missing verdict → `verdict = "-"`, `would_use = false`.

Skeleton:
```tw
//! Phase 7E update-site dry-run: joins census candidates with ownership
//! verdicts and the persistent->mutable catalog pairing. Inspection only.

use compiler.anf.{AnfModule}
use compiler.builtins.{BuiltinRegistry}
use compiler.census
use compiler.cfg
use compiler.core_ir.{FuncId}
use compiler.opt.semantics.{call_info, make_prelude_optimizer_semantics}
use compiler.ownership
use compiler.summary

pub type DryRunSite = .{
  func: String, family: String, persistent: String, mutable: String,
  verdict: String, would_use: Bool, local: Int,
}

type Targets = .{ persistent: String, mutable: String, has_mutable: Bool }

pub fn dry_run_sites(opt: AnfModule, b: BuiltinRegistry) Vector<DryRunSite> {
  sem := make_prelude_optimizer_semantics(b)
  sites := census.census_sites(opt, b)
  verdicts := collect_verdicts(opt, b) // Dict<String,String> key "${func}#${local}"

  out: Vector<DryRunSite> = []
  for s in sites {
    v := case verdicts.get("${s.func}#${s.local}") {
      .Some(text) => text,
      .None => "-",
    }
    t := targets_for(s.actual_fid, sem, b)
    reusable := v.contains("reuse(") or v.contains("in-place(")
    out = out.append(DryRunSite.{
      func: s.func, family: s.family, persistent: t.persistent, mutable: t.mutable,
      verdict: v, would_use: reusable and t.has_mutable, local: s.local,
    })
  }
  out
}
```
Implement `collect_verdicts`, `targets_for`, and `render_dry_run` (tab-separated: header `func\tfamily\tpersistent\tmutable\twould_use\tverdict`) per the notes. Confirm each external signature by reading the cited files first.

- [ ] **Step 4: Run tests** — all dry-run cases pass, including the no-mutable-target row (`mutable="-"`, `would_use=false`).

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/codegen/dry_run.tw boot/tests/suites/dry_run_suite.tw boot/tests/main.tw
target/twk lint boot/main.tw
git add boot/compiler/codegen/dry_run.tw boot/tests/suites/dry_run_suite.tw boot/tests/main.tw
git commit -m "codegen: add 7E update-site dry-run join and renderer"
```

---

### Task 4: Surface via `twk ir --census --sites` (through a testable pure helper)

No stdout-capture harness exists, so put the rendering in a pure helper and test that; leave the CLI a thin caller + smoke check.

**Files:**
- Modify: `boot/commands/ir.tw`
- Test: `boot/tests/suites/uniqueness_census_suite.tw`

- [ ] **Step 1: Failing test on the pure render helper**

Add a test that builds `PipelineArtifacts` for `fn f(xs: Vector<Int>) Vector<Int> { xs[0] = 9  xs }` (reuse the suite’s existing compile helper) and calls a new pure `ir.render_census_report(artifacts, true)`. Assert the returned string contains the tally header (`family\tcandidates\tin_place`) AND the dry-run header (`persistent\tmutable\twould_use\tverdict`) AND a row naming `vector$set_unsafe` and `vector$set_in_place`. Add a second assertion that `ir.render_census_report(artifacts, false)` contains the tally but NOT the dry-run header (plain `--census` unchanged).

- [ ] **Step 2: Run, verify failure** — no `render_census_report`.

- [ ] **Step 3: Add the pure helper + wire the command**

In `boot/commands/ir.tw` add `use compiler.codegen.dry_run` and:
```tw
pub fn render_census_report(artifacts: PipelineArtifacts, include_sites: Bool) String {
  sites := census.census_sites(artifacts.opt, artifacts.builtins)
  out := census.tally_from_sites(sites).render_census()
  if include_sites {
    dry := dry_run.dry_run_sites(artifacts.opt, artifacts.builtins)
    out = out.concat(dry_run.render_dry_run(dry))
  }
  out
}
```
Replace the inline `--census` block with a call:
```tw
if parsed.has_flag("census") {
  print(render_census_report(artifacts, parsed.has_flag("sites")))
}
```

- [ ] **Step 4: Run tests** — helper test passes; plain-census path unchanged.

- [ ] **Step 5: Smoke check** (uses the current, pre-change `target/twk`; dry-run columns only appear after Task 5’s `make bundle-cli` — rely on boot tests until then):
```bash
printf 'fn f(xs: Vector<Int>) Vector<Int> {\n  xs[0] = 9\n  xs\n}\nprintln(f([1,2]).len().to_string())\n' > /tmp/7e.tw
target/twk ir /tmp/7e.tw --census --sites
```

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/commands/ir.tw boot/tests/suites/uniqueness_census_suite.tw
target/twk lint boot/main.tw
git add boot/commands/ir.tw boot/tests/suites/uniqueness_census_suite.tw
git commit -m "ir: show update-site dry-run in census --sites via pure helper"
```

---

### Task 5: Docs + self-host verification

**Files:**
- Modify: `docs/plans/sound-uniqueness/codegen/README.md`

- [ ] **Step 1: Update the README honestly**

In the Phase 7E section:
- Add a first checked item: `[x] **Print dry-run rewrite targets (update sites).** twk ir --census --sites shows persistent → mutable per vector/dict/record update candidate, plus an ownership verdict and would_use (true only when the base is owned AND a mutable target exists), via compiler.codegen.dry_run + a new update-call verdict in ownership.tw.`
- Keep the remaining bullets UNCHECKED, annotated as deferred:
  - `[ ] Render consumed vs ignored *decisions* — deferred: needs a real MutableDecisionTable producer (post-7D follow-up); today only ownership-verdict dry-runs exist, not decision-table state.`
  - `[ ] Variant routing dry-runs — deferred: render_call_decision renders only \`-> f<id>[unique:...]\` (2 of 5 fields); a full generic/clone/VariantId/route/fallback renderer is a separate task.`
- Retitle the phase header to `## Codegen Phase 7E — Dry-run rendering 🚧 update-site slice done (<today>)` (NOT ✅ — the phase is not fully complete).
- Do NOT change the top-of-file Status "Next" pointer to imply 7E is finished; note the update-site slice landed and variant-routing / decision-table dry-run remain.

- [ ] **Step 2: Full verification incl. self-host** (sequential, never concurrent):
```bash
target/twk fmt boot/compiler/ownership.tw boot/compiler/census.tw boot/compiler/codegen/dry_run.tw boot/commands/ir.tw boot/tests/main.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
make bundle-cli
make boot-test
git diff --check
```
Expected: fmt idempotent, lint clean, boot tests green, `make bundle-cli` reaches `stage3 == stage4`, `make boot-test` green, `git diff --check` clean.

- [ ] **Step 3: Commit**
```bash
git add docs/plans/sound-uniqueness/codegen/README.md
git commit -m "docs: mark 7E update-site dry-run slice done"
```

---

## Acceptance criteria

- `twk ir --census --sites` shows, per update candidate: `persistent`, `mutable` (or `-`), `would_use`, and the ownership `verdict`.
- Vector/dict update-call sites receive an ownership verdict (`reuse(unique)` when owned + last-use; `persistent(<reason>)` otherwise) in both `--census --sites` and `--cfg`.
- `would_use = true` iff the base is ownership-reusable AND a mutable target exists. Update families with `in_place_equivalent: .None` render `mutable = "-"`, `would_use = false`, regardless of verdict.
- `census.actual_fid` holds the real callee id; target derivation handles persistent-form, in-place-form (reverse-mapped), no-mutable, and record cases.
- Plain `twk ir --census` output is byte-unchanged; `census.tw` does no ownership analysis.
- README marks ONLY the update-site slice done; variant-routing and decision-table dry-run stay explicitly deferred.
- No emitted-code change: `make bundle-cli` reaches the stage3 == stage4 fixed point; boot tests green.

## Self-review notes

- Rescoped to the update-site slice; overclaims removed (variant routing and decision-table dry-run are OUT and left unchecked).
- `would_use` now requires a mutable target (fixes the `reuse(unique)`-without-target false positive).
- Site model stores `actual_fid`; derivation is future-proof for in-place callees via forward+reverse `in_place_equivalent` maps.
- Analysis sequence matches the verified `--cfg` path (`prune_dead_merge` + `summary.compute` + `analyze_with_summaries(view,b,sem,table)`; no `compute_variants`).
- Testing targets pure functions (`render_census_report`, `render_dry_run`, `dry_run_sites`) since no stdout harness exists.
- One confirm-before-coding item remains: how a `CfgFunction` exposes its name for the `(func, local)` join key (Task 3, read `cfg.tw`).
