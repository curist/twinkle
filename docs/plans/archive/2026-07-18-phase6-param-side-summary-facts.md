# Phase 6 Part 1 — Parameter-Side Generic-Pass Facts Implementation Plan

> **Status: archived.** Phase 6 analysis work landed; remaining variant generation, routing, and deeper path/variant machinery are tracked by the codegen/migration tracks.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reconcile the interprocedural `ParamSummary` schema to the canonical Phase 6 `base_role` / `in_place_paths` / `flows_to_return` model and populate + render the coarse (shell-`[]`) parameter-side ownership facts in the existing generic summary pass.

**Architecture:** Purely a change to the *generic* summary pass (`summarize_function` and its schema in `ownership.tw`, plus the seed/compare/render functions in `summary.tw`). We fold the existing `escape × capability` pair into the three-way `ParamRole = { Borrowed, Consumed, Published }` (D9 precedence: any leaked param is `Published`), derive `flows_to_return` from the already-computed `ret`/`ret_paths`, and populate `in_place_paths` at **shell granularity only** (`[[]]` for a `Consumed` param, `[]` otherwise). No variant machinery, no owned-entry re-analysis, no call-site decisions — those are Phase 6 Part 2 (see "Scope boundary" below). Analysis-only: `twk ir --census` stays **0 in-place**.

**Tech Stack:** Twinkle (`.tw`), boot compiler only. Tests via the boot suite (`boot/tests/suites/cfg_summary_suite.tw`). Build/verify with `make boot-test` and `make stage2`.

---

## Scope boundary (what this plan does NOT do)

This is **Part 1** of Phase 6 (design: `docs/plans/sound-uniqueness/analysis/phase6-design.md`). It delivers the generic-pass parameter facts — the substrate Part 2 builds on — and is independently shippable and testable. It satisfies design acceptance criteria **#1 (shell-granularity subset)**, **#9**, **#10**, and re-baselines **#16**.

**Deferred to Phase 6 Part 2 (a separate plan, written after this lands):**
- Field-granular `in_place_paths` (`add_type` → `paths{[],[.types]}`; Part 1 gives only `paths{[]}`). Requires path-level mutation-site tracking (design Blocker 2/3).
- `UniqueReq` / `UniqueKey` / `VariantId` / `CallDecision` / `SpecializationFacts` and their Int-key encoding (design "Data model").
- Owned-entry re-analysis `summarize_variant` (D10), call-site decision + `consume_dead` (D4/D6), `ConsumedPaths` in `ForwardState` (Blocker 3), the SCC variant fixpoint + cap (D7/D12), and cfg decision rendering.

Because Part 2's exact code depends on the concrete APIs this part introduces (`ParamRole`, `reconcile_role`, the finalized `ParamSummary`), it is deliberately left for a follow-on plan rather than fabricated here.

## Terminology mapping (old → new)

| Old (`escape × capability`) | New `base_role` | Rule |
|---|---|---|
| `escape=Retained` (any capability) | `Published` | D9 precedence: a leaked param never specializes; `in_place_paths=[]` |
| `escape=Borrowed`, `capability=Consumed`, flows to return | `Consumed` | mutated, not leaked, region reaches the return; `in_place_paths=[[]]` |
| `escape=Borrowed`, `capability=Consumed`, does **not** flow | `Borrowed` | mutation is caller-invisible → no specialization value; `in_place_paths=[]` |
| `escape=Borrowed`, `capability=NoCap` | `Borrowed` | read-only reference; `in_place_paths=[]` |

`EscapeEffect` and `ParamCapability` stay as **internal** enums (still computed by the forward fixpoint); only their *externally visible* pairing on `ParamSummary` is replaced.

## File structure

- **`boot/compiler/ownership.tw`** — owns the schema. Add `ParamRole`, `ParamPath`, `RawParam`, `reconcile_role`, `param_in_indices`; change `ParamSummary`; migrate the caller publish-gate in `transfer_summarized_call` (`:1134-1141`, reads `ps.escape`); restructure the tail of `summarize_function` so params are finalized after `ret`/`ret_paths`.
- **`boot/compiler/summary.tw`** — seed (`conservative_summary`), fixpoint compare (`param_summary_eq` via `same_summary`), and render (`render_summary`). Add `role_eq`, `same_param_paths`, `render_param_path`.
- **`boot/tests/suites/cfg_summary_suite.tw`** — migrate helpers (`p_escape`, `cap_tag`) to the new schema (compat shims keep existing escape assertions unchanged), re-baseline the two literal-construction tests, and add Part 1 fact tests.

---

## Task 1: Reconcile `ParamSummary` schema to `ParamRole` + `flows_to_return`

**Files:**
- Modify: `boot/compiler/ownership.tw:39-43` (types), `:1134-1141` (caller publish-gate), `:3271-3286` + `:3410` (construction)
- Modify: `boot/compiler/summary.tw:58-63` (seed), `:92-94` (compare), `:467-491` (render)
- Test: `boot/tests/suites/cfg_summary_suite.tw`

> **Note on monotonicity (preempts the reviewer question):** folding `capability` into `base_role` makes `param_summary_eq` blind to the consumed-but-not-flowing distinction (it maps to `Borrowed`, same as read-only). That collapses one lattice dimension, but the dropped bit is unused in Part 1 and is never read off a stored `Summary` downstream (`transfer_summarized_call` ignores capability — design "Current state"). The fixpoint stays sound and may converge one step sooner.

- [ ] **Step 1: Re-baseline the exact-format render test (write the failing expectation first)**

In `boot/tests/suites/cfg_summary_suite.tw`, find the test `"review: render_summary public format is exact"` (around line 1055). Replace its two literal `ParamSummary` constructions and the expected string:

```tw
      "review: render_summary public format is exact",
      fn() {
        consumed := ownership.Summary.{
          params: [
            ownership.ParamSummary.{ base_role: .Borrowed, in_place_paths: [], flows_to_return: false },
            ownership.ParamSummary.{ base_role: .Published, in_place_paths: [], flows_to_return: false },
          ],
          ret: .MayAliasParams([0, 1]),
          ret_paths: [],
        }
        fresh := ownership.Summary.{ params: [], ret: .OwnedFresh, ret_paths: [] }
        try assert.equal(
          summary.render_summary(consumed),
          "summary: p0=Borrowed p1=Published ret=alias(p0,p1)",
        )
        try assert.equal(summary.render_summary(fresh), "summary:  ret=fresh")
        .Ok({})
      },
```

- [ ] **Step 2: Run the suite to confirm it fails to compile (schema not migrated yet)**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -20`
Expected: a type error — `ParamSummary` has no field `base_role` (the schema still says `escape`/`capability`).

- [ ] **Step 3: Add the new types and the reconciliation helper in `ownership.tw`**

Immediately after the existing `ParamCapability` line (`ownership.tw:41`), add `ParamRole` and `ParamPath`, and replace the `ParamSummary` definition:

```tw
pub type ParamCapability = { NoCap, Consumed }

// Phase 6 (D9): the reconciled three-way role, folding escape × capability.
pub type ParamRole = { Borrowed, Consumed, Published }

// A parameter in-place path: [] = the shell / whole collection; [f, …] = a record
// field chain. FIELD-ONLY (design Blocker 1). Part 1 only ever emits [] (shell);
// field chains arrive in Phase 6 Part 2.
pub type ParamPath = Vector<Int>

pub type ParamSummary = .{
  base_role: ParamRole,
  in_place_paths: Vector<ParamPath>,
  flows_to_return: Bool,
}
// Invariant (D9 precedence): base_role == Published  =>  in_place_paths == []
//                            in_place_paths != []     =>  base_role == Consumed && flows_to_return
```

Then, near the other summary helpers in `ownership.tw` (anywhere above `summarize_function`), add the intermediate type and helpers:

```tw
// Intermediate for the two-phase param finalization (D9): the forward fixpoint
// yields esc/cap, and the role is assigned only after ret/ret_paths are known.
// A NAMED nominal record — Twinkle has no anonymous structural record element type,
// so `Vector<.{…}>` is not writable; `Vector<RawParam>` is.
type RawParam = .{ esc: EscapeEffect, cap: ParamCapability }

// D9 precedence: a leaked (Retained) param is always Published, regardless of a
// consumed capability — a value the callee leaks cannot be handed back uniquely.
// Otherwise a Consumed-and-return-flowing param is Consumed; else Borrowed.
pub fn reconcile_role(esc: EscapeEffect, cap: ParamCapability, flows_to_return: Bool) ParamRole {
  case esc {
    .Retained => .Published,
    .Borrowed => case cap {
      .Consumed => if flows_to_return { .Consumed } else { .Borrowed },
      .NoCap => .Borrowed,
    },
  }
}

fn param_in_indices(k: Int, idxs: Vector<Int>) Bool {
  for x in idxs {
    if x == k {
      return true
    }
  }
  false
}

// flows_to_return (design "Requirement collection"): param k's region reaches the
// return iff the whole-return classification names it (MayAliasParams) or a
// ret_path hands it back (OwnedFromParam(k)). A projection of Phase 5 facts.
fn param_flows_to_return(k: Int, ret: ReturnEffect, ret_paths: Vector<ReturnPathOwn>) Bool {
  case ret {
    .MayAliasParams(idxs) => if param_in_indices(k, idxs) {
      return true
    },
    _ => {},
  }
  for rp in ret_paths {
    case rp.own {
      .OwnedFromParam(j) => if j == k {
        return true
      },
      _ => {},
    }
  }
  false
}
```

- [ ] **Step 4: Restructure `summarize_function` so params are finalized after `ret_paths`**

In `ownership.tw`, the param loop at `:3271-3286` currently builds `params` before `ret`/`ret_paths` exist. Change it to collect the raw `esc`/`cap` per param into an intermediate, then finalize after `ret_paths` is known.

Replace the `params` collect block at `:3271-3286` with a raw collection into the named `RawParam` type (roles are assigned at the end):

```tw
  raw_params: Vector<RawParam> = collect p in f.params {
    esc: EscapeEffect = .Borrowed
    cap: ParamCapability = .NoCap
    for blk in blocks {
      if own_is_shared(own_map_get(fx.exits, blk.id.id), p.id) {
        esc = .Retained
      }
      case valid_map_get(fx.exit_valid, blk.id.id).get(p.id) {
        .Some(v) => if !v {
          cap = .Consumed
        },
        .None => {},
      }
    }
    RawParam.{ esc, cap }
  }
```

Then migrate the **caller publish-gate** in `transfer_summarized_call` at `:1134-1141` — a live consumer that reads `ps.escape` to decide whether to publish each arg. Under D9, `Published` ⟺ old `Retained` (publish), and both `Borrowed` and `Consumed` must **not** publish (a consumed-but-not-leaked arg is not published — preserving the old behavior where only `escape` gated this). Replace:

```tw
  for ps, i in s.params {
    if i < args.len() {
      case ps.base_role {
        .Published => st = .publish_atom(args[i]),
        .Borrowed => {},
        .Consumed => {},
      }
    }
  }
```

Then change the final `Summary` construction at `:3410` from:

```tw
  Summary.{ params, ret, ret_paths: meet_ret_paths_tagged(ret_sites) }
```

to finalize params against the computed return facts:

```tw
  ret_paths_final := meet_ret_paths_tagged(ret_sites)
  params: Vector<ParamSummary> = collect rp, i in raw_params {
    flows := param_flows_to_return(i, ret, ret_paths_final)
    role := reconcile_role(rp.esc, rp.cap, flows)
    ParamSummary.{ base_role: role, in_place_paths: [], flows_to_return: flows }
  }
  Summary.{ params, ret, ret_paths: ret_paths_final }
```

(`in_place_paths: []` is populated in Task 2.)

- [ ] **Step 5: Update the conservative seed in `summary.tw`**

At `summary.tw:58-63`, change `conservative_summary` to seed the reconciled schema. The old seed `escape: .Retained, capability: .NoCap` maps to `Published` (the conservative top; a param is proven `Borrowed`/`Consumed` as the transfer sharpens):

```tw
pub fn conservative_summary(f: CfgFunction) Summary {
  params: Vector<ParamSummary> = collect _p in f.params {
    ParamSummary.{ base_role: .Published, in_place_paths: [], flows_to_return: false }
  }
  Summary.{ params, ret: .Shared, ret_paths: [] }
}
```

- [ ] **Step 6: Update the fixpoint comparison in `summary.tw`**

Replace `param_summary_eq` (`summary.tw:92-94`) and add the two comparison helpers above it. `escape_eq` (`:66-77`) and `capability_eq` (`:79-90`) are now dead (verified: their only caller was the old `param_summary_eq`) — **delete both**.

```tw
fn role_eq(a: ParamRole, b: ParamRole) Bool {
  case a {
    .Borrowed => case b { .Borrowed => true, _ => false },
    .Consumed => case b { .Consumed => true, _ => false },
    .Published => case b { .Published => true, _ => false },
  }
}

fn same_param_paths(a: Vector<ParamPath>, b: Vector<ParamPath>) Bool {
  if a.len() != b.len() {
    return false
  }
  for pa, i in a {
    pb := b[i]
    if pa.len() != pb.len() {
      return false
    }
    for seg, j in pa {
      if seg != pb[j] {
        return false
      }
    }
  }
  true
}

fn param_summary_eq(a: ParamSummary, b: ParamSummary) Bool {
  role_eq(a.base_role, b.base_role)
    and (a.flows_to_return == b.flows_to_return)
    and same_param_paths(a.in_place_paths, b.in_place_paths)
}
```

Also update the `use compiler.ownership.{...}` import at the top of `summary.tw` (`:19`) to bring `ParamRole` and `ParamPath` into scope alongside the existing `ParamSummary` etc.

- [ ] **Step 7: Update `render_summary` in `summary.tw`**

Replace the param-rendering portion of `render_summary` (`summary.tw:467-491`) to print the role and (when present) `paths{…}`:

```tw
fn render_param_path(p: ParamPath) String {
  if p.len() == 0 {
    "[]"
  } else {
    segs: Vector<String> = collect s in p {
      ".f${s}"
    }
    "[${segs.join(",")}]"
  }
}

fn render_role(r: ParamRole) String {
  case r {
    .Borrowed => "Borrowed",
    .Consumed => "Consumed",
    .Published => "Published",
  }
}

pub fn render_summary(s: Summary) String {
  parts: Vector<String> = []
  for ps, i in s.params {
    paths := if ps.in_place_paths.len() > 0 {
      rendered: Vector<String> = collect p in ps.in_place_paths {
        render_param_path(p)
      }
      " paths{${rendered.join(",")}}"
    } else {
      ""
    }
    parts = .append("p${i}=${render_role(ps.base_role)}${paths}")
  }
  ret := case s.ret {
    .OwnedFresh => "fresh",
    .Shared => "shared",
    .MayAliasParams(idxs) => {
      ps := collect k in idxs {
        "p${k}"
      }
      "alias(${ps.join(",")})"
    },
  }
  "summary: ${parts.join(" ")} ret=${ret}${render_ret_paths(s.ret_paths)}"
}
```

- [ ] **Step 8: Migrate the test suite helpers to compat shims**

In `boot/tests/suites/cfg_summary_suite.tw`, replace `p_escape` (`:103-105`) with a compat shim that derives the old escape tag from `base_role` (so every existing `escape_tag(...)` assertion keeps passing unchanged — escape only ever distinguished `Retained` vs not, which is exactly `Published` vs not), and replace the `cap_tag`-reading assertions:

```tw
fn p_escape(s: ownership.Summary, i: Int) Int {
  // compat: old escape tag is 1 (Retained) iff the reconciled role is Published.
  case s.params[i].base_role {
    .Published => 1,
    _ => 0,
  }
}

fn role_tag(r: ownership.ParamRole) Int {
  case r {
    .Borrowed => 0,
    .Consumed => 1,
    .Published => 2,
  }
}

fn p_role(s: ownership.Summary, i: Int) Int {
  role_tag(s.params[i].base_role)
}
```

Then fix the two `cap_tag(... .capability)` assertions (lines ~610 and ~912). **Read the fixtures carefully:** both bodies end in `.Atom(.ALitInt(0))` — the consumed param is neither returned nor leaked, so `flows_to_return = false`. By D9 (`reconcile_role(Borrowed, Consumed, false) = Borrowed`), the reconciled role is **`.Borrowed`**, not `.Consumed`, and `in_place_paths` is **empty**. These fixtures now serve as coverage of the `flows_to_return` gate (mutated-but-caller-invisible ⇒ not specializable). Rewrite each:

```tw
        // was: try assert.equal(cap_tag(s.params[0].capability), cap_tag(.Consumed))
        // the param is consumed but NOT returned/leaked -> flows_to_return=false -> Borrowed
        try assert.equal(p_role(s, 0), role_tag(.Borrowed))
        try assert.equal(s.params[0].in_place_paths.len(), 0)
```

Also update those two tests' names/comments (`"p0 consumed"`) to reflect the new meaning, e.g. `"t4 consumed-but-not-flowing -> Borrowed, no in_place_paths"`.

(Apply the same rewrite at both sites. The companion `p_escape(...) == escape_tag(.Borrowed)` assertions in those tests stay as-is — a `Borrowed` role is not `Published`, so the compat shim returns `0`.)

Finally, three assertions read `.escape` **directly** off the summary (not via `p_escape`) and must route through the shim. At lines ~683, ~717, ~719, rewrite `escape_tag(summ_of(t, N).params[0].escape)` → `p_escape(summ_of(t, N), 0)`:

```tw
        // line ~683 (expects Borrowed):
        try assert.equal(p_escape(summ_of(t, 1), 0), escape_tag(.Borrowed))
        // lines ~717 / ~719 (mutual recursion, expect Retained → shim returns 1):
        try assert.equal(p_escape(summ_of(t, 1), 0), escape_tag(.Retained))
        try assert.equal(p_escape(summ_of(t, 2), 0), escape_tag(.Retained))
```

With the last `.capability` read gone, `cap_tag` (`:96-101`) is dead — **remove it**. (`escape_tag` stays: it is still used on the RHS of the migrated assertions, and `EscapeEffect` remains an internal enum.)

Grep to be sure none are left: `grep -n '\.escape\b\|\.capability\b\|cap_tag' boot/tests/suites/cfg_summary_suite.tw` should return **only** the `p_escape` shim body and the two migrated literal `ParamSummary.{ base_role: … }` constructions (no `.params[i].escape` / `.params[i].capability` reads, no `cap_tag`).

- [ ] **Step 9: Run the full boot suite to verify green**

Run: `make boot-test 2>&1 | tail -20`
Expected: all tests pass, including the re-baselined `"review: render_summary public format is exact"` test.

- [ ] **Step 10: Commit**

```bash
git add boot/compiler/ownership.tw boot/compiler/summary.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "phase6: reconcile ParamSummary to base_role + flows_to_return

Fold escape × capability into the three-way ParamRole (D9 precedence:
leaked params are Published), derive flows_to_return from ret/ret_paths,
and render the reconciled role. in_place_paths present but empty (Part 2
populates it). Generic pass only; no codegen change."
```

---

## Task 2: Populate + render coarse shell-`[]` `in_place_paths`

**Files:**
- Modify: `boot/compiler/ownership.tw:3410` region (param finalization)
- Test: `boot/tests/suites/cfg_summary_suite.tw`

- [ ] **Step 1: Write the failing test — a Consumed collection param prints `paths{[]}`**

Add to `cfg_summary_suite.tw`'s `suite()` a new test mirroring the existing "t4 capability" fixture (a param moved then threaded through `Dict.set`, i.e. consumed, and returned so it flows):

```tw
    .test(
      "phase6 p1: consumed+flowing param carries in_place_paths {[]}",
      fn() {
        b := b_reg()
        set_call: AnfOp = .ACall(
          .AGlobalFunc(b.method_id("Dict", "set")),
          [.ALocal(lid(2)), .ALitInt(1), .ALitInt(2)],
        )
        // w := <move x>; w2 := Dict.set(w,1,2); return w2  -> x consumed AND flows out
        body: AnfExpr = .Let(
          lid(2),
          .AInit(.ALocal(lid(0))),
          .Let(lid(3), set_call, .Atom(.ALocal(lid(3)))),
        )
        s := summ1("g", 1, body)
        try assert.equal(p_role(s, 0), role_tag(.Consumed))
        try assert.is_true(s.params[0].flows_to_return)
        try assert.equal(s.params[0].in_place_paths.len(), 1)
        try assert.equal(s.params[0].in_place_paths[0].len(), 0) // the shell path []
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run it to verify it fails**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -A3 "phase6 p1"`
Expected: FAIL — `in_place_paths.len()` is `0` (Task 1 seeds it empty).

- [ ] **Step 3: Populate `in_place_paths` at shell granularity**

In `ownership.tw`, in the final param `collect` from Task 1 Step 4 (`:3410` region), set `in_place_paths` from the role. `Consumed` implies mutated-and-flowing, so the shell `[]` is a sound (coarse) in-place candidate; `Borrowed`/`Published` carry none:

```tw
  params: Vector<ParamSummary> = collect rp, i in raw_params {
    flows := param_flows_to_return(i, ret, ret_paths_final)
    role := reconcile_role(rp.esc, rp.cap, flows)
    ipp: Vector<ParamPath> = case role {
      // shell / whole-collection candidate; Phase 6 Part 2 refines a record param
      // to add its consumed field paths ([.f]). Downward-closed under [].
      .Consumed => [[]],
      _ => [],
    }
    ParamSummary.{ base_role: role, in_place_paths: ipp, flows_to_return: flows }
  }
```

- [ ] **Step 4: Run the new test to verify it passes**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -A3 "phase6 p1"`
Expected: PASS.

- [ ] **Step 5: Add the negative tests — Published precedence (#10) and Borrowed (#9) carry no paths**

```tw
    .test(
      "phase6 p1: leaked+mutated param is Published with empty in_place_paths (#10)",
      fn() {
        b := b_reg()
        // w := <move x>; global_set G0 = w  -> x consumed AND leaked -> Published, no paths
        body: AnfExpr = .Let(
          lid(1),
          .AInit(.ALocal(lid(0))),
          .Let(
            lid(2),
            .AGlobalSet(GlobalId.{ id: 0 }, .ALocal(lid(1))),
            .Atom(.ALitInt(0)),
          ),
        )
        s := summ1("g", 1, body)
        try assert.equal(p_role(s, 0), role_tag(.Published))
        try assert.equal(s.params[0].in_place_paths.len(), 0)
        .Ok({})
      },
    )
    .test(
      "phase6 p1: read-only param is Borrowed with empty in_place_paths (#9)",
      fn() {
        b := b_reg()
        s := summ1("f", 1, .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1)))))
        try assert.equal(p_role(s, 0), role_tag(.Borrowed))
        try assert.equal(s.params[0].in_place_paths.len(), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 6: Add a render test for `paths{[]}`**

```tw
    .test(
      "phase6 p1: render shows paths{[]} for a Consumed param",
      fn() {
        s := ownership.Summary.{
          params: [
            ownership.ParamSummary.{ base_role: .Consumed, in_place_paths: [[]], flows_to_return: true },
          ],
          ret: .OwnedFresh,
          ret_paths: [],
        }
        try assert.equal(summary.render_summary(s), "summary: p0=Consumed paths{[]} ret=fresh")
        .Ok({})
      },
    )
```

- [ ] **Step 7: Run the full boot suite to verify green**

Run: `make boot-test 2>&1 | tail -20`
Expected: all tests pass.

- [ ] **Step 8: Commit**

```bash
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "phase6: populate coarse shell in_place_paths for Consumed params

A Consumed (mutated + return-flowing, not leaked) param carries the shell
path {[]}; Borrowed/Published carry none (D5/D9 precedence). Field-granular
paths ([.f]) are Phase 6 Part 2. Rendered as paths{...}. No codegen change."
```

---

## Task 3: Whole-phase verification

**Files:** none (verification only)

- [ ] **Step 1: Confirm no in-place codegen regression (acceptance #15)**

Run: `target/twk ir boot/main.tw --census 2>&1 | tail -5`
Expected: **0 in-place** (Part 1 changes no codegen).

- [ ] **Step 2: Run the full boot test suite**

Run: `make boot-test 2>&1 | tail -20`
Expected: all green.

- [ ] **Step 3: Reach the self-host fixed point (acceptance #16)**

Run: `make stage2 2>&1 | tail -20`
Expected: the self-host loop reaches its fixed point (Part 1 is boot-only and adds no stage0-parity construct). Run this **alone** (no other heavy `twk`/`make` in parallel — concurrent `twk` pegs CPU).

- [ ] **Step 4: Sanity-check the rendered summary on a real entry**

Run: `target/twk ir boot/main.tw --cfg 2>&1 | grep -m5 'summary:.*paths{'`
Expected: real functions that consume-and-return a collection param print `p{i}=Consumed paths{[]}`; no crash; output shape matches the render tests.

- [ ] **Step 5: Final commit (if any doc/expectations were re-baselined)**

```bash
git add -A
git commit -m "phase6: verify Part 1 (param-side generic facts); census still 0 in-place"
```

---

## Self-review notes (already applied)

- **Spec coverage:** Part 1 covers design acceptance #1 (shell-granularity subset — `set_at`/collection params print `paths{[]}`; field-granular `add_type` deferred to Part 2), #9 (read-only → Borrowed, empty), #10 (leaked+mutated → Published, empty), #15 (census 0), #16 (self-host). Criteria #2–#8, #11–#14 depend on the variant/decision machinery and are Part 2.
- **Type consistency:** `ParamRole`, `ParamPath`, `RawParam`, `ParamSummary`, `reconcile_role`, `param_in_indices`, `param_flows_to_return`, `role_eq`, `same_param_paths`, `render_role`, `render_param_path`, `p_role`, `role_tag` are used with identical signatures across all tasks.
- **All `ps.escape` / `ps.capability` consumers migrated:** the schema swap touches every reader — the caller publish-gate (`ownership.tw:1136`, migrated to `base_role`), the construction site (`:3271/:3410`), the seed/compare/render in `summary.tw`, and the suite (direct `.escape` reads at 683/717/719 + `.capability` reads at 610/912 + literal constructions at 1062). Dead helpers (`escape_eq`, `capability_eq`, `cap_tag`) removed.
- **Fixture role targets verified against the ANF bodies:** the two consumed fixtures (595/895) return `.ALitInt(0)` ⇒ `flows_to_return=false` ⇒ `Borrowed` (not `Consumed`); Task 2's positive fixture returns the consumed value ⇒ `Consumed` + `paths{[]}`.
- **No placeholders:** every code step shows the exact replacement; every run step names the command and expected result.
