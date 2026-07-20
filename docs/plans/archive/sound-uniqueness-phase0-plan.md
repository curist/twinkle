# Phase 0 Baseline & Safety Rails — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the two latent Phase 0 safety rails — a negative-aliasing guard suite and a candidate-op census (behind `twk ir --census`) with an asserted regression gate — so later phases have a corruption net and an in-place-conversion signal.

**Architecture:** A reusable boot function walks the codegen-bound optimized ANF (`artifacts.opt`), emitting one `CensusSite` per COW-candidate op; a tally is a reduction over sites. Candidate/in-place detection **rides `OptimizerSemantics`** (`compiler/opt/semantics.tw`): a call is a candidate iff its `CallSemantics.effect` is `.Update`, an in-place call is one whose id is a registered `in_place_equivalent`, and every `ARecordUpdate` counts (its `in_place` bit read directly). A thin registry-built family map only *labels* families (`record_update`, `dict_set`/`remove`, `vector_set`, `vector_append`, `vector_builder`), with an `other` bucket for any `.Update` candidate it does not name — so the census cannot silently drop a candidate as boot grows. The `--census` flag and the gate test both call this function. The guard suite asserts existing persistent semantics on aliasing/publication patterns; each test flips red only if a future in-place lowering is unsound.

**Tech Stack:** Twinkle (`.tw`), boot self-hosted compiler, `@std.testing` runner, `pipeline.compile_source` for in-process lowering.

**Design spec:** `docs/plans/sound-uniqueness/phase0-baseline.md`. This plan contains no ownership analysis — both rails read only what ANF already carries.

---

## File structure

- **Create** `boot/compiler/census.tw` — the census: `CensusSite`, `CensusTally`, `census_sites`, `tally_from_sites`, `render_census`, `render_sites`. One responsibility: count candidate ops over ANF.
- **Modify** `boot/main.tw` — register `--census` / `--sites` flags on `ir_cmd`.
- **Modify** `boot/commands/ir.tw` — handle the census flags in `run_ir_command`.
- **Create** `boot/tests/suites/uniqueness_census_suite.tw` — census unit + gate tests (exact counts on inline fixtures; all-COW-floor invariant).
- **Create** `boot/tests/suites/uniqueness_guard_suite.tw` — the negative-aliasing behavioral guards + Case B/V positive anchors.
- **Modify** `boot/tests/main.tw` — register the two new suites.

## Conventions (read once)

- **CLI-flag / compiler-source changes need a self-hosted rebuild.** After editing `boot/` compiler or command code that the CLI itself runs (the new `--census` flag handler, or the `compiler.census` module reached through `twk ir`), rebuild with `make bundle-cli` — it runs `make stage2` to fold the change into `target/boot.wasm`, then rebuilds `target/twk`. `make quick-bundle-cli` only re-wraps an already-fresh `target/boot.wasm` and will **not** contain new boot source, so it is the wrong command after editing boot code. (`make stage2 && make quick-bundle-cli` is the same thing, split.)
- **Boot-test iteration needs no rebuild.** `target/twk run boot/tests/main.tw` compiles the suite — and any imported `compiler.*` module, including `compiler.census` — from source through the existing CLI. So the census gate/guard suites (Tasks 1, 3, 4) run without a bundle; only the `twk ir --census` flag verification (Task 2) requires `make bundle-cli`.
- Format after editing: `target/twk fmt <file>`. Lint: `target/twk lint <entry>`.
- Test one suite quickly by running the whole boot suite and grepping its output: `target/twk run boot/tests/main.tw 2>&1 | grep -iE 'census|uniqueness|FAIL'`.

---

## Task 1: Census core (`census.tw`)

**Files:**
- Create: `boot/compiler/census.tw`
- Test: `boot/tests/suites/uniqueness_census_suite.tw` (created here, extended in Task 3-gate)

- [ ] **Step 1: Write the failing test**

Create `boot/tests/suites/uniqueness_census_suite.tw`:

```tw
use @std.testing.assert as assert
use @std.testing as runner

use compiler.census
use compiler.pipeline

// Compile a snippet and return its optimized-ANF census tally.
fn tally(src: String) Result<census.CensusTally, String> {
  artifacts := try pipeline.compile_source(src)
  sites := census.census_sites(artifacts.opt, artifacts.builtins)
  .Ok(census.tally_from_sites(sites))
}

pub fn suite() runner.Suite {
  runner
    .suite("uniqueness census")
    .test(
      "counts two dict.set candidates, none in-place",
      fn() {
        t := try tally(
          "fn two_sets(seed: Int) Int {\n  d: Dict<String, Int> = Dict.new()\n  d[\"a\"] = seed\n  d[\"b\"] = seed\n  d.len()\n}\n",
        )
        try assert.equal(t.dict_set.candidates, 2)
        try assert.equal(t.dict_set.in_place, 0)
        try assert.equal(t.record_update.candidates, 0)
        .Ok({})
      },
    )
}
```

- [ ] **Step 2: Register the suite (so the red run actually compiles it)**

An unregistered suite file is never imported, so it is never compiled — the missing `compiler.census` module would raise no error and the run would go green without ever executing the new test. Register it *first* so the red run is genuine.

In `boot/tests/main.tw`, add the import alongside the other `use .suites.*` lines:

```tw
use .suites.uniqueness_census_suite
```

and add to the suite array (near the end of the list, before the closing `])`):

```tw
  uniqueness_census_suite.suite(),
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -iE 'census|uniqueness|error'`
Expected: a compile error — `compiler.census` does not exist yet, so the `use compiler.census` in the now-registered suite fails to resolve. This confirms the suite is wired and the test cannot pass until the module exists.

- [ ] **Step 4: Write the census module**

Create `boot/compiler/census.tw`:

```tw
//! Candidate-op census over optimized ANF.
//!
//! Counts COW-candidate update sites and how many are lowered in-place. Candidate
//! detection RIDES the optimizer semantics (`OptimizerSemantics`): a call is a
//! candidate iff its `CallSemantics.effect` is `.Update`, and an in-place call is
//! one whose id is a registered `in_place_equivalent`. Record updates are counted
//! via `ARecordUpdate` (the `in_place` bit). A thin family-label table — built
//! symbolically from the registry, mirroring `opt/semantics.tw`'s own
//! registrations — only *names* the reporting families; it never decides
//! candidate-ness, and any `.Update` candidate it does not name falls into
//! `other`, so the census stays honest as boot grows. Contains NO ownership
//! analysis. Shared by `twk ir --census` and the census gate suite.

use compiler.anf.{AnfExpr, AnfModule, AnfOp, Atom}
use compiler.builder_family.{vector_builder_config}
use compiler.builtins.{BuiltinRegistry}
use compiler.core_ir.{FuncId}
use compiler.opt.semantics.{
  OptimizerSemantics, call_info, make_prelude_optimizer_semantics,
}

pub type CensusSite = .{ func: String, family: String, in_place: Bool }

pub type FamilyCount = .{ name: String, candidates: Int, in_place: Int }

pub type CensusTally = .{
  record_update: FamilyCount,
  dict_set: FamilyCount,
  dict_remove: FamilyCount,
  vector_set: FamilyCount,
  vector_append: FamilyCount,
  vector_builder: FamilyCount,
  other: FamilyCount,
}

// Family labels, keyed by raw FuncId int. `family_of` names each persistent-form
// candidate op; `ip_family_of` names each in-place variant (derived by reversing
// the semantics' `in_place_equivalent` map). These tables only bucket sites into
// reporting families — detection rides the semantics, not this list. Building the
// reverse map iterates a Dict, but the result is order-independent (each in-place
// id maps to exactly one family), so the census stays deterministic.
type Labels = .{
  family_of: Dict<Int, String>,
  ip_family_of: Dict<Int, String>,
}

fn build_labels(b: BuiltinRegistry, sem: OptimizerSemantics) Labels {
  builder := vector_builder_config(b)
  fam: Dict<Int, String> = Dict.new()
  fam[b.method_id("Dict", "set").id] = "dict_set"
  fam[b.method_id("Dict", "remove").id] = "dict_remove"
  fam[b.id("vector$set_unsafe").id] = "vector_set"
  fam[builder.push_id.id] = "vector_append"
  fam[builder.builder_new_id.id] = "vector_builder"
  fam[builder.builder_from_id.id] = "vector_builder"
  fam[builder.builder_push_id.id] = "vector_builder"
  fam[builder.builder_freeze_id.id] = "vector_builder"

  ip: Dict<Int, String> = Dict.new()
  for base_id, cs in sem.call_semantics {
    ip = case cs.in_place_equivalent {
      .Some(ip_id) => case fam.get(base_id) {
        .Some(name) => ip.set(ip_id.id, name),
        .None => ip,
      },
      .None => ip,
    }
  }
  Labels.{ family_of: fam, ip_family_of: ip }
}

type FamIp = .{ family: String, in_place: Bool }

// Classify a called FuncId: in-place variant first, then a named persistent
// candidate, then the semantics backstop (any other `.Update` call → `other`).
fn classify_call(fid: FuncId, sem: OptimizerSemantics, labels: Labels) FamIp? {
  case labels.ip_family_of.get(fid.id) {
    .Some(name) => .Some(FamIp.{ family: name, in_place: true }),
    .None => case labels.family_of.get(fid.id) {
      .Some(name) => .Some(FamIp.{ family: name, in_place: false }),
      .None => case call_info(sem, fid) {
        .Some(cs) => case cs.effect {
          .Update => .Some(FamIp.{ family: "other", in_place: false }),
          _ => .None,
        },
        .None => .None,
      },
    },
  }
}

/// Every candidate site in the module, in deterministic traversal order.
pub fn census_sites(m: AnfModule, b: BuiltinRegistry) Vector<CensusSite> {
  sem := make_prelude_optimizer_semantics(b)
  labels := build_labels(b, sem)
  sites: Vector<CensusSite> = []
  for f in m.functions {
    sites = walk_expr(f.body, f.name, sites, sem, labels)
  }
  sites
}

fn walk_expr(
  expr: AnfExpr,
  func: String,
  sites: Vector<CensusSite>,
  sem: OptimizerSemantics,
  labels: Labels,
) Vector<CensusSite> {
  case expr {
    .Let(_, op, body) => {
      after := walk_op(op, func, sites, sem, labels)
      walk_expr(body, func, after, sem, labels)
    },
    _ => sites,
  }
}

fn walk_op(
  op: AnfOp,
  func: String,
  sites: Vector<CensusSite>,
  sem: OptimizerSemantics,
  labels: Labels,
) Vector<CensusSite> {
  case op {
    .ARecordUpdate(_, _, _, in_place, _) =>
      sites.append(CensusSite.{ func: func, family: "record_update", in_place: in_place }),
    .ACall(callee, _) => case callee {
      .AGlobalFunc(fid) => case classify_call(fid, sem, labels) {
        .Some(fi) =>
          sites.append(CensusSite.{ func: func, family: fi.family, in_place: fi.in_place }),
        .None => sites,
      },
      _ => sites,
    },
    .AIf(_, then_e, else_e) => {
      after_then := walk_expr(then_e, func, sites, sem, labels)
      walk_expr(else_e, func, after_then, sem, labels)
    },
    .AMatch(_, arms) => {
      acc := sites
      for arm in arms {
        acc = walk_expr(arm.body, func, acc, sem, labels)
      }
      acc
    },
    .ALoop(body) => walk_expr(body, func, sites, sem, labels),
    .ADefer(body) => walk_expr(body, func, sites, sem, labels),
    _ => sites,
  }
}

fn empty_family(name: String) FamilyCount {
  FamilyCount.{ name: name, candidates: 0, in_place: 0 }
}

fn empty_tally() CensusTally {
  CensusTally.{
    record_update: empty_family("record_update"),
    dict_set: empty_family("dict_set"),
    dict_remove: empty_family("dict_remove"),
    vector_set: empty_family("vector_set"),
    vector_append: empty_family("vector_append"),
    vector_builder: empty_family("vector_builder"),
    other: empty_family("other"),
  }
}

fn inc(fc: FamilyCount, in_place: Bool) FamilyCount {
  FamilyCount.{
    name: fc.name,
    candidates: fc.candidates + 1,
    in_place: fc.in_place + if in_place { 1 } else { 0 },
  }
}

/// Reduce sites into per-family totals.
pub fn tally_from_sites(sites: Vector<CensusSite>) CensusTally {
  t := empty_tally()
  for s in sites {
    t = cond {
      s.family == "record_update" => {
        t.record_update = inc(t.record_update, s.in_place)
        t
      },
      s.family == "dict_set" => {
        t.dict_set = inc(t.dict_set, s.in_place)
        t
      },
      s.family == "dict_remove" => {
        t.dict_remove = inc(t.dict_remove, s.in_place)
        t
      },
      s.family == "vector_set" => {
        t.vector_set = inc(t.vector_set, s.in_place)
        t
      },
      s.family == "vector_append" => {
        t.vector_append = inc(t.vector_append, s.in_place)
        t
      },
      s.family == "vector_builder" => {
        t.vector_builder = inc(t.vector_builder, s.in_place)
        t
      },
      _ => {
        t.other = inc(t.other, s.in_place)
        t
      },
    }
  }
  t
}

fn render_row(fc: FamilyCount) String {
  "${fc.name}\t${fc.candidates}\t${fc.in_place}\n"
}

/// Tab-separated population table.
pub fn render_census(t: CensusTally) String {
  rows := [
    t.record_update,
    t.dict_set,
    t.dict_remove,
    t.vector_set,
    t.vector_append,
    t.vector_builder,
    t.other,
  ]
  out := "family\tcandidates\tin_place\n"
  for fc in rows {
    out = out.concat(render_row(fc))
  }
  out
}

/// Per-site listing (for `--sites`).
pub fn render_sites(sites: Vector<CensusSite>) String {
  out := "func\tfamily\tin_place\n"
  for s in sites {
    out = out.concat("${s.func}\t${s.family}\t${s.in_place}\n")
  }
  out
}
```

- [ ] **Step 5: Format and run the test**

Run (no CLI rebuild — `target/twk run` compiles `compiler.census` from source):
```bash
target/twk fmt boot/compiler/census.tw boot/tests/suites/uniqueness_census_suite.tw
target/twk run boot/tests/main.tw 2>&1 | grep -iE 'uniqueness census|FAIL'
```
Expected: the "uniqueness census" suite passes (`counts two dict.set candidates, none in-place`), no FAIL.

If the count is not 2, run `target/twk ir /tmp/two.tw --anf` on the snippet (write it to a file) to inspect the lowered calls and confirm `d[k]=v` lowers to a `Dict.set` `ACall`; the family ids are the fix point, not the assertion.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/census.tw boot/tests/suites/uniqueness_census_suite.tw boot/tests/main.tw
git commit -m "census: add candidate-op census over optimized ANF"
```

---

## Task 2: `twk ir --census` / `--sites` flags

**Files:**
- Modify: `boot/main.tw` (the `ir_cmd` definition)
- Modify: `boot/commands/ir.tw` (`run_ir_command`)

- [ ] **Step 1: Register the flags**

In `boot/main.tw`, extend the `ir_cmd` chain (it currently ends at `.add_flag("all", ...)`):

```tw
ir_cmd := file_command("ir", "Compile and print compiler IR")
  .add_flag("core", "Print lowered Core IR")
  .add_flag("mono", "Print monomorphized Core IR")
  .add_flag("anf", "Print unoptimized ANF IR")
  .add_flag("opt", "Print optimized ANF IR")
  .add_flag("wat", "Print final linked WAT")
  .add_flag("all", "Print all available IR stages")
  .add_flag("census", "Print the candidate-op census table (optimized ANF)")
  .add_flag("sites", "With --census, also list each candidate site")
```

- [ ] **Step 2: Handle the flags in `run_ir_command`**

In `boot/commands/ir.tw`, add the import near the other `use compiler.*` lines:

```tw
use compiler.census
```

Then, inside `pub fn run_ir_command(parsed)`, immediately after the `print_warnings(artifacts.warnings)` line and before `first := true`, insert:

```tw
  if parsed.has_flag("census") {
    sites := census.census_sites(artifacts.opt, artifacts.builtins)
    print(census.render_census(census.tally_from_sites(sites)))

    if parsed.has_flag("sites") {
      print(census.render_sites(sites))
    }

    return
  }
```

- [ ] **Step 3: Rebuild and verify the flag end-to-end**

Write a fixture and run the flag. The flag handler lives in the compiler itself, so `target/boot.wasm` must be rebuilt (`make bundle-cli`, which runs `make stage2`); `make quick-bundle-cli` would leave the CLI without the new flag:
```bash
printf 'fn f(d: Dict<String, Int>) Int {\n  d["a"] = 1\n  d.len()\n}\n' > /tmp/census_probe.tw
target/twk fmt boot/main.tw boot/commands/ir.tw
make bundle-cli
target/twk ir /tmp/census_probe.tw --census
```
Expected output (tab-separated; `dict_set` candidates 1, everything else 0, all `in_place` 0):
```
family	candidates	in_place
record_update	0	0
dict_set	1	0
dict_remove	0	0
vector_set	0	0
vector_append	0	0
vector_builder	0	0
other	0	0
```
Then verify `--sites`:
```bash
target/twk ir /tmp/census_probe.tw --census --sites
```
Expected: the same table, followed by a `func\tfamily\tin_place` listing with one `dict_set / false` row for function `f`.

- [ ] **Step 4: Verify the wide reference command runs**

Run:
```bash
target/twk ir boot/main.tw --census
```
Expected: the table prints with non-zero `candidates` across families and `in_place` all `0` (the all-COW floor of the real compiler). This is the documented wide reference — not asserted in CI.

- [ ] **Step 5: Commit**

```bash
git add boot/main.tw boot/commands/ir.tw
git commit -m "ir: add twk ir --census / --sites candidate-op flag"
```

---

## Task 3: Census gate tests (exact counts + all-COW floor)

**Files:**
- Modify: `boot/tests/suites/uniqueness_census_suite.tw`

- [ ] **Step 1: Add the failing gate tests**

In `boot/tests/suites/uniqueness_census_suite.tw`, add these `.test(...)` blocks to the suite chain (after the existing Task-1 test). They pin hand-verifiable exact counts and assert the all-COW floor:

```tw
    .test(
      "counts two record updates, none in-place",
      fn() {
        t := try tally(
          "type P = .{ x: Int, y: Int }\nfn move_xy(p: P) P {\n  p.x = 1\n  p.y = 2\n  p\n}\n",
        )
        try assert.equal(t.record_update.candidates, 2)
        try assert.equal(t.record_update.in_place, 0)
        .Ok({})
      },
    )
    .test(
      "counts two vector index-sets, none in-place",
      fn() {
        t := try tally(
          "fn set_two(xs: Vector<Int>) Vector<Int> {\n  xs[0] = 9\n  xs[1] = 8\n  xs\n}\n",
        )
        try assert.equal(t.vector_set.candidates, 2)
        try assert.equal(t.vector_set.in_place, 0)
        .Ok({})
      },
    )
    .test(
      "counts two vector appends as vector_append candidates",
      fn() {
        // `Vector.append` is an `.Update` candidate via `builder.push_id`; this
        // pins the family the semantics-based detector must not drop.
        t := try tally(
          "fn push_two(xs: Vector<Int>) Vector<Int> {\n  xs = xs.append(1)\n  xs = xs.append(2)\n  xs\n}\n",
        )
        try assert.equal(t.vector_append.candidates, 2)
        try assert.equal(t.vector_append.in_place, 0)
        .Ok({})
      },
    )
    .test(
      "descends into both branches of an if",
      fn() {
        t := try tally(
          "fn cond_set(xs: Vector<Int>, b: Bool) Vector<Int> {\n  if b {\n    xs[0] = 1\n  } else {\n    xs[1] = 2\n  }\n  xs\n}\n",
        )
        try assert.equal(t.vector_set.candidates, 2)
        .Ok({})
      },
    )
    .test(
      "descends into a loop body (one static site)",
      fn() {
        t := try tally(
          "fn loop_set(xs: Vector<Int>, n: Int) Vector<Int> {\n  for i in range(n) {\n    xs[i] = i\n  }\n  xs\n}\n",
        )
        try assert.equal(t.vector_set.candidates, 1)
        .Ok({})
      },
    )
    .test(
      "worked-example shapes stay on the all-COW floor",
      fn() {
        // Case B (add_type threading) + Case V (record quartet): candidates > 0,
        // zero in-place — the floor this gate guards until Phase 5 flips sites.
        t := try tally(
          "type Env = .{ types: Dict<String, Int> }\nfn add_type(e: Env, k: String, id: Int) Env {\n  e.types = .set(k, id)\n  e\n}\nfn build() Env {\n  e := Env.{ types: Dict.new() }\n  e = add_type(e, \"a\", 1)\n  e = add_type(e, \"b\", 2)\n  e\n}\n",
        )
        try assert.is_true(t.dict_set.candidates > 0)
        try assert.equal(t.dict_set.in_place, 0)
        try assert.equal(t.record_update.in_place, 0)
        try assert.equal(t.vector_set.in_place, 0)
        try assert.equal(t.vector_append.in_place, 0)
        try assert.equal(t.vector_builder.in_place, 0)
        try assert.equal(t.other.in_place, 0)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Rebuild and run**

Run:
```bash
target/twk fmt boot/tests/suites/uniqueness_census_suite.tw
target/twk run boot/tests/main.tw 2>&1 | grep -iE 'uniqueness census|FAIL'
```
Expected: all "uniqueness census" tests pass, no FAIL. (No CLI rebuild needed — only test source changed; `target/twk run` executes it directly.)

If a record-update or vector-set count differs, dump the snippet's ANF with `target/twk ir <file> --anf` and adjust: the counts are what the current lowering produces, and the gate pins them — do not weaken an assertion to `> 0` for the exact-count cases (those are the tight gate). The floor test intentionally uses `> 0` only for the size-dependent candidate count.

- [ ] **Step 3: Commit**

```bash
git add boot/tests/suites/uniqueness_census_suite.tw
git commit -m "census: assert candidate counts and all-COW floor on fixtures"
```

---

## Task 4: Negative-aliasing guard suite

**Files:**
- Create: `boot/tests/suites/uniqueness_guard_suite.tw`
- Modify: `boot/tests/main.tw`

- [ ] **Step 1: Write the guard suite**

Create `boot/tests/suites/uniqueness_guard_suite.tw`. Each test asserts existing persistent semantics on an aliasing/publication pattern; it flips red only if a future in-place lowering is unsound for that pattern. Names and comments cross-reference the worked-example case + fact-lattice rule guarded.

```tw
use @std.testing.assert as assert
use @std.testing as runner

// Module-global publication sink. Twinkle's only mutable global is a `Cell`, so a
// value stored here has escaped into a module-global, arbitrarily-read box.
gpub: Cell<Vector<Int>> = Cell.new([0])

// Helper for the call-boundary publication guard: the callee RETAINS the vector
// (returns it inside an aggregate), so a later rebind of the caller's binding must
// not corrupt what the callee kept. A read-only callee returning an Int would be
// vacuous — the Int is computed before the rebind and could never go red.
fn keep_in_box(v: Vector<Int>) Box {
  Box.{ items: v }
}

// Helper for the closure-capture guard.
fn make_reader(v: Vector<Int>) fn() Int {
  fn() Int { v[0] }
}

// Case B / V positive anchors.
type Env = .{ types: Dict<String, Int> }

fn add_type(e: Env, k: String, id: Int) Env {
  e.types = .set(k, id)
  e
}

type St = .{ seen: Dict<String, Int>, count: Int }

fn visit(s: St, k: String) St {
  s.seen = .set(k, s.count)
  s.count = s.count + 1
  s
}

// Case T: an owned handle live across a `try` error arm.
fn parse_step(ok: Bool) Result<Int, String> {
  if ok {
    .Ok(1)
  } else {
    .Err("no")
  }
}

fn chain(ok: Bool) Result<Int, String> {
  v := try parse_step(ok)
  .Ok(v + 1)
}

// Aggregate + unknown-call guard shape.
type Box = .{ items: Vector<Int> }

pub fn suite() runner.Suite {
  runner
    .suite("uniqueness guards")
    // Case C — AInit alias hinge: the old dict version stays observable.
    .test(
      "case C: aliased old dict version stays observable",
      fn() {
        d: Dict<String, Int> = Dict.new()
        d["a"] = 1
        old := d
        d["a"] = 2
        try assert.equal(old.get("a"), .Some(1))
        try assert.equal(d.get("a"), .Some(2))
        .Ok({})
      },
    )
    // Shared backing via slice/concat: originals are untouched.
    .test(
      "slice and concat share backing without mutating the source",
      fn() {
        xs: Vector<Int> = [1, 2, 3, 4]
        tail := xs.slice(1, xs.len())
        ys := xs.concat([5])
        try assert.equal(tail[0], 2)
        try assert.equal(ys[4], 5)
        try assert.equal(xs.len(), 4)
        try assert.equal(xs[0], 1)
        .Ok({})
      },
    )
    // Stored in an escaping aggregate before update (fact-lattice: escaping store).
    .test(
      "value stored in a record keeps its pre-update version",
      fn() {
        xs: Vector<Int> = [7, 7]
        b := Box.{ items: xs }
        xs[0] = 9
        try assert.equal(b.items[0], 7)
        try assert.equal(xs[0], 9)
        .Ok({})
      },
    )
    // Stored in a variant payload before update (sound-analysis: variant storage).
    .test(
      "value stored in a variant keeps its pre-update version",
      fn() {
        xs: Vector<Int> = [7, 7]
        boxed: Vector<Int>? = .Some(xs)
        xs[0] = 9
        case boxed {
          .Some(v) => try assert.equal(v[0], 7),
          .None => try assert.is_true(false),
        }
        try assert.equal(xs[0], 9)
        .Ok({})
      },
    )
    // Stored as a dict value before update (sound-analysis: dict storage).
    .test(
      "value stored as a dict value keeps its pre-update version",
      fn() {
        xs: Vector<Int> = [7, 7]
        d: Dict<String, Vector<Int>> = Dict.new()
        d["k"] = xs
        xs[0] = 9
        case d.get("k") {
          .Some(v) => try assert.equal(v[0], 7),
          .None => try assert.is_true(false),
        }
        try assert.equal(xs[0], 9)
        .Ok({})
      },
    )
    // Case Cell — store publishes; get yields the (unknown) stored value.
    .test(
      "case Cell: cell retains the value stored before a later rebind",
      fn() {
        xs: Vector<Int> = [1, 2]
        c := Cell.new(xs)
        xs[0] = 9
        got := c.get()
        try assert.equal(got[0], 1)
        try assert.equal(xs[0], 9)
        .Ok({})
      },
    )
    // Module-global publication: a value published into a module-global Cell keeps
    // its pre-rebind version (sound-analysis: module/global publication sink).
    .test(
      "module-global publication keeps the pre-rebind version",
      fn() {
        xs: Vector<Int> = [7, 7]
        gpub.set(xs)
        xs[0] = 9
        got := gpub.get()
        try assert.equal(got[0], 7)
        try assert.equal(xs[0], 9)
        .Ok({})
      },
    )
    // Closure capture publishes: the captured value is not corrupted by a later rebind.
    .test(
      "closure capture keeps the captured version",
      fn() {
        xs: Vector<Int> = [3, 3]
        reader := make_reader(xs)
        xs[0] = 9
        try assert.equal(reader(), 3)
        try assert.equal(xs[0], 9)
        .Ok({})
      },
    )
    // Task capture publishes (concurrency sink).
    .test(
      "task capture observes the pre-rebind version",
      fn() {
        xs: Vector<Int> = [7, 7]
        captured := xs
        t := Task.spawn(fn() Int { captured[0] })
        xs[0] = 9
        try assert.equal(t.await(), 7)
        try assert.equal(xs[0], 9)
        .Ok({})
      },
    )
    // Channel send publishes to another context (concurrency sink).
    .test(
      "channel send delivers the pre-rebind version",
      fn() {
        ch: Channel<Vector<Int>> = Channel.bounded(1)
        xs: Vector<Int> = [7, 7]
        try assert.is_true(ch.send(xs))
        xs[0] = 9
        case ch.recv() {
          .Some(v) => try assert.equal(v[0], 7),
          .None => try assert.is_true(false),
        }
        try assert.equal(xs[0], 9)
        .Ok({})
      },
    )
    // Call-boundary publication: the callee retains the vector, so a later rebind
    // must not corrupt the retained alias (sound-analysis: unknown/publishing callee).
    .test(
      "vector retained by a callee survives a later rebind of the caller",
      fn() {
        xs: Vector<Int> = [5, 5]
        held := keep_in_box(xs)
        xs[0] = 100
        try assert.equal(held.items[0], 5)
        try assert.equal(xs[0], 100)
        .Ok({})
      },
    )
    // Nested collection: outer holds the pre-update inner (shell-vs-deep).
    .test(
      "nested outer vector keeps the pre-update inner vector",
      fn() {
        inner: Vector<Int> = [1, 1]
        outer: Vector<Vector<Int>> = [inner, inner]
        inner[0] = 9
        try assert.equal(outer[0][0], 1)
        try assert.equal(inner[0], 9)
        .Ok({})
      },
    )
    // Case T — try/early-return publication path stays correct.
    .test(
      "case T: try propagates ok and error arms",
      fn() {
        try assert.equal(chain(true), .Ok(2))
        try assert.equal(chain(false), .Err("no"))
        .Ok({})
      },
    )
    // Case B positive anchor — owned record + dict threading.
    .test(
      "case B: owned env threading builds the expected dict",
      fn() {
        e := Env.{ types: Dict.new() }
        e = add_type(e, "a", 1)
        e = add_type(e, "b", 2)
        try assert.equal(e.types.get("a"), .Some(1))
        try assert.equal(e.types.get("b"), .Some(2))
        .Ok({})
      },
    )
    // Case V positive anchor — record quartet threaded through recursion-shaped update.
    .test(
      "case V: record quartet threads seen-dict and counter",
      fn() {
        s := St.{ seen: Dict.new(), count: 0 }
        s = visit(s, "a")
        s = visit(s, "b")
        try assert.equal(s.seen.get("a"), .Some(0))
        try assert.equal(s.seen.get("b"), .Some(1))
        try assert.equal(s.count, 2)
        .Ok({})
      },
    )
}
```

- [ ] **Step 2: Register the suite**

In `boot/tests/main.tw`, add the import:

```tw
use .suites.uniqueness_guard_suite
```

and the array entry near the other new suite:

```tw
  uniqueness_guard_suite.suite(),
```

- [ ] **Step 3: Format and run**

Run:
```bash
target/twk fmt boot/tests/suites/uniqueness_guard_suite.tw
target/twk run boot/tests/main.tw 2>&1 | grep -iE 'uniqueness guards|FAIL'
```
Expected: all "uniqueness guards" tests pass, no FAIL. (These assert current persistent behavior, so they are green today; the value is that they will catch an unsound Phase 5 in-place rewrite.)

If a test errors on an API mismatch (e.g. `Channel.bounded`, `.recv()` shape), reconcile with the live API in `boot/tests/suites/channel_suite.tw` / `task_suite.tw` / `api_set_suite.tw` — mirror their exact calls. `Cell.set` is a mutating (void) statement, mirrored from `api_cell_suite.tw`. If the top-level `gpub: Cell<Vector<Int>>` module-global is rejected in a suite module (module-global init), move that single guard into a tiny dedicated module rather than dropping it — the module/global publication sink is a required Phase 0 negative.

- [ ] **Step 4: Commit**

```bash
git add boot/tests/suites/uniqueness_guard_suite.tw boot/tests/main.tw
git commit -m "tests: add negative-aliasing uniqueness guard suite"
```

---

## Task 5: Wire tracking and mark Phase 0 progress

**Files:**
- Modify: `docs/plans/sound-uniqueness/README.md` (check off the delivered items)
- Modify: `docs/plans/sound-uniqueness/phase0-baseline.md` (record the resolved open questions)

- [ ] **Step 1: Check off the delivered Phase 0 items**

In `docs/plans/sound-uniqueness/README.md`, under `### Phase 0`, change the three delivered boxes from `- [ ]` to `- [x]` and append ` Done: <YYYY-MM-DD>.` to each of: "Define correctness guard programs", "Define inspection workflow", and "Stand up a boot-side ownership census harness". Leave the census-ceiling item as-is (already `[x]`).

- [ ] **Step 2: Resolve the doc's open questions**

Component 3 of `phase0-baseline.md` already states the corpus is inline worked-example snippets (aligned during the plan revision, so no AWFY contradiction remains). Replace the remaining "Open questions" list with the resolutions reached in implementation:

```markdown
## Resolved during implementation

- The asserted gate pins the exact per-family counts the current lowering produces
  for each inline fixture (record/dict/vector shapes + Cases B/V); those counts
  live in `uniqueness_census_suite.tw` and are reconciled against
  `twk ir <fixture> --anf` whenever the lowering shifts them.
- The wide `boot/main.tw` census stays **informational** (no CI assertion) — the
  inline gate carries the deterministic regression signal.
```

- [ ] **Step 3: Full suite green + commit**

Run:
```bash
target/twk run boot/tests/main.tw 2>&1 | tail -5
```
Expected: the suite summary reports all suites passing (no failures).

```bash
git add docs/plans/sound-uniqueness/README.md docs/plans/sound-uniqueness/phase0-baseline.md
git commit -m "docs/sound-uniqueness: mark Phase 0 rails delivered"
```

---

## Self-review

**Spec coverage** (against `phase0-baseline.md` + `sound-analysis.md`):
- Component 1 (guard suite) → Task 4: the required negatives — alias-old-version, slice/concat sharing, storage in record/variant/dict, Cell, module-global publication, closure/task/channel capture, retaining callee (call-boundary publication), nested collection, `try` — plus Case B/V positives. Each publication sink from `sound-analysis.md` has a red-able guard (the callee guard retains the alias; the global guard publishes into a module-global Cell). ✓
- Component 2 (`--census` reusable fn, optimized ANF, **detection rides `OptimizerSemantics` — `CallSemantics.effect == .Update` + `ARecordUpdate` + `in_place_equivalent`**, family labels + `other` backstop, default table + `--sites`) → Tasks 1–2. `Vector.append` (via `builder.push_id`) is its own `vector_append` family. ✓
- Component 3 (asserted fixture gate + wide reference) → Task 3 (gate) + Task 2 Step 4 / Task 5 (wide). ✓ (gate uses inline snippets; `phase0-baseline.md` Component 3 is aligned to match, so no AWFY contradiction remains.)
- Verified enablers (`compile_source`→`artifacts.opt`/`.builtins`, `anf_lower_suite` idiom, Task/Channel suites) → used directly in Tasks 1, 3, 4. ✓

**Placeholder scan:** No TBD/TODO. Every code step shows complete code; every run step shows the exact command and expected output. The one "discover the number" risk (exact candidate counts) is handled by pinning hand-verifiable snippets and giving an ANF-dump reconciliation step, not a placeholder.

**Type consistency:** `census_sites(AnfModule, BuiltinRegistry) → Vector<CensusSite>`, `tally_from_sites(Vector<CensusSite>) → CensusTally`, `render_census(CensusTally)`, `render_sites(Vector<CensusSite>)` are used identically in the census module (Task 1), the flag handler (Task 2), and the gate test's `tally` helper (Task 1/3). `CensusTally` field names (`record_update`, `dict_set`, `dict_remove`, `vector_set`, `vector_append`, `vector_builder`, `other`) and `FamilyCount` fields (`candidates`, `in_place`) match across the module, `empty_tally`, `tally_from_sites`, `render_census`, and every gate assertion. `classify_call` returns only these family strings, so the `tally_from_sites` `cond` (with `_ => other`) is total.

**Known residual risks (validated by the plan's own run steps, not assumed):**
- Exact candidate counts depend on lowering; Task 1 Step 5 and Task 3 Step 2 include ANF-dump reconciliation.
- Task/Channel API shape is mirrored from live suites; Task 4 Step 3 points at them for reconciliation.
- `d[k]=v` must lower to a `Dict.set` `ACall` for the dict_set family to catch it; Task 1 Step 5 verifies this against `--anf` before the count is trusted.
