# Tier-1 Stdlib API Additions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the two highest-frequency missing stdlib APIs surfaced by the API audit — `Int` base/radix conversion and `Vector` reductions (`sum`/`product`/`min`/`max` and their comparator/key variants).

**Architecture:** Both are pure prelude additions in Twinkle source (`boot/prelude/int.tw`, `boot/prelude/vector.tw`) — ordinary `pub fn`s auto-registered as inherent methods, exactly like the existing `Int.gcd` / `Vector.fold` / `Vector.sort`. No Rust/stage0 changes, no runtime-builtin FuncId wiring. Everything uses only integer, string, generic, and `Ord`-contract operations, all of which stage0 already lowers (verified: `Vector.sort` uses `T: Ord`/`.compare` and `Int.from_string`-style string parsing already ships in the eagerly-lowered prelude). No `Float` rounding intrinsics are involved, so none of the stage0 f64 constraints that shaped `Float.to_fixed` apply here.

**Tech Stack:** Twinkle prelude (`.tw`), the boot self-hosted compiler, the `@std.testing` suite runner.

## Global Constraints

- **Prelude is embedded in `boot.wasm`.** After editing any `boot/prelude/*.tw`, you MUST run `python3 tools/generate_core_lib.py` and then the **full** `make bundle-cli` (NOT `make quick-bundle-cli`, which skips the boot.wasm rebuild and leaves the old prelude in place). Only after `make bundle-cli` does `target/twk` see the new methods.
- **Range literals over `range(...)`:** write `1..n`, not `range_from(1, n)` / `range(n)`. Identical lowering; house style.
- **Format after editing:** run `target/twk fmt <file>` on every edited `.tw` file (idempotent, canonical style).
- **Lint after editing:** run `target/twk lint boot/main.tw` — report-only; resolve or justify findings.
- **Rebinding style:** use self-rebind sugar (`acc = acc + x`, `out = .append(y)`); never introduce numbered copies (`acc2`, `out3`).
- **Test suites already exist and are registered** in `boot/tests/main.tw`: add `Int` tests to `boot/tests/suites/api_int_suite.tw` and `Vector` tests to `boot/tests/suites/api_vector_suite.tw`. No new suite registration is needed.
- **Test gate:** `make boot-test` runs the whole boot suite (the correctness gate). A green run ends with `Ran N tests: N passed`.
- **Docs:** every new public function gets a row in `docs/API.md` in the matching section (`## Numeric (Int)` and `## Vector<T>`).
- **Commit-message style:** short imperative subject; body explains what/why/how, no line/count metrics. Only add a `Co-Authored-By` trailer when actually correct for the session.

---

## Component A — Int radix conversion

Two functions in `boot/prelude/int.tw`: render an `Int` in any base 2–36, and parse one back. `Int.from_string` today is base-10 only and there is no base rendering at all, so hex/binary/octal work is currently impossible without hand-rolling.

### Task A1: `Int.to_string_radix`

**Files:**
- Modify: `boot/prelude/int.tw` (add `to_string_radix`)
- Modify: `boot/tests/suites/api_int_suite.tw` (add a test block)
- Modify: `docs/API.md` (add a row under `## Numeric (Int)`)

**Interfaces:**
- Produces: `Int.to_string_radix(n: Int, radix: Int) String` — base-`radix` (2–36) rendering, lowercase digits, leading `-` for negatives, `"0"` for zero. Traps if `radix` is outside `2..=36`.

- [ ] **Step 1: Write the failing test**

Append a new `.test(...)` block inside the `api Int` suite builder chain in `boot/tests/suites/api_int_suite.tw` (before the final closing `}` of `suite()`):

```tw
    .test(
      "to_string_radix renders in any base 2..36",
      fn() {
        try assert.equal(255.to_string_radix(16), "ff")
        try assert.equal(10.to_string_radix(2), "1010")
        try assert.equal(0.to_string_radix(16), "0")
        try assert.equal((-255).to_string_radix(16), "-ff")
        try assert.equal(35.to_string_radix(36), "z")
        try assert.equal(1000.to_string_radix(10), "1000")
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run the suite to verify it fails**

Run: `make bundle-cli && make boot-test`
Expected: FAIL — `to_string_radix` is unresolved (method not found on `Int`).

- [ ] **Step 3: Write the implementation**

Add to `boot/prelude/int.tw`. The loop works in **negative magnitude** so `Int.min` does not overflow when negated:

```tw
/// Render `n` in base `radix` (2..=36) with lowercase digits. Traps on an
/// out-of-range radix. Negative values get a leading `-`; zero renders "0".
pub fn to_string_radix(n: Int, radix: Int) String {
  if radix < 2 or radix > 36 {
    error("Int.to_string_radix: radix must be in 2..=36")
  }

  if n == 0 {
    return "0"
  }

  digits := "0123456789abcdefghijklmnopqrstuvwxyz"
  neg := n < 0
  // Work with a non-positive magnitude so Int.min has no positive counterpart.
  m := if neg {
    n
  } else {
    0 - n
  }
  out := ""

  for m != 0 {
    r := 0 - m % radix // 0..radix-1 (m <= 0, so m % radix <= 0)
    m = m / radix
    out = digits.slice(r, r + 1).concat(out)
  }

  if neg {
    "-".concat(out)
  } else {
    out
  }
}
```

- [ ] **Step 4: Format, regenerate, rebuild, and run the suite**

Run: `target/twk fmt boot/prelude/int.tw boot/tests/suites/api_int_suite.tw && python3 tools/generate_core_lib.py && make bundle-cli && make boot-test`
Expected: PASS — `Ran N tests: N passed`.

- [ ] **Step 5: Document**

Add under `## Numeric (Int)` in `docs/API.md`:

```markdown
| `Int.to_string_radix` | `fn(n: Int, radix: Int) String` | Render `n` in base `radix` (2–36), lowercase digits, leading `-` for negatives; traps if `radix` is outside `2..=36` |
```

- [ ] **Step 6: Commit**

```bash
git add boot/prelude/int.tw boot/tests/suites/api_int_suite.tw docs/API.md
git commit -m "feat(int): add Int.to_string_radix for base rendering"
```

### Task A2: `Int.from_string_radix`

**Files:**
- Modify: `boot/prelude/int.tw` (add `from_string_radix` and a private `radix_digit_value` helper)
- Modify: `boot/tests/suites/api_int_suite.tw` (add a test block)
- Modify: `docs/API.md` (add a row under `## Numeric (Int)`)

**Interfaces:**
- Consumes: nothing from A1 (independent).
- Produces: `Int.from_string_radix(s: String, radix: Int) Option<Int>` — parse `s` (optional leading `+`/`-`, digits valid for `radix`) into an `Int`; `.None` on empty input or any invalid digit. Traps if `radix` is outside `2..=36`.

- [ ] **Step 1: Write the failing test**

Append inside the `api Int` suite builder chain in `boot/tests/suites/api_int_suite.tw`:

```tw
    .test(
      "from_string_radix parses any base and rejects bad digits",
      fn() {
        try assert.equal(Int.from_string_radix("ff", 16), .Some(255))
        try assert.equal(Int.from_string_radix("1010", 2), .Some(10))
        try assert.equal(Int.from_string_radix("-ff", 16), .Some(-255))
        try assert.equal(Int.from_string_radix("Z", 36), .Some(35))
        try assert.equal(Int.from_string_radix("", 10), .None)
        try assert.equal(Int.from_string_radix("2", 2), .None)
        try assert.equal(Int.from_string_radix("-", 10), .None)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run the suite to verify it fails**

Run: `make bundle-cli && make boot-test`
Expected: FAIL — `from_string_radix` is unresolved.

- [ ] **Step 3: Write the implementation**

Add to `boot/prelude/int.tw`. The helper returns `Option<Int>` so the parse loop can `try` it (early-returning `.None` on the first invalid digit, since `from_string_radix` itself returns `Option<Int>`):

```tw
// Value of a single ASCII digit character for the given radix, or .None when
// the character is not a valid digit (0-9, a-z / A-Z) below `radix`.
fn radix_digit_value(ch: String, radix: Int) Option<Int> {
  b := ch.to_ascii_lower().char_code_at(0)
  v := if b >= 48 and b <= 57 {
    b - 48
  } else if b >= 97 and b <= 122 {
    b - 97 + 10
  } else {
    -1
  }

  if v < 0 or v >= radix {
    .None
  } else {
    .Some(v)
  }
}

/// Parse `s` in base `radix` (2..=36) into an Int. Accepts an optional leading
/// `+`/`-`. Returns `.None` on empty input or any invalid digit. Traps on an
/// out-of-range radix.
pub fn from_string_radix(s: String, radix: Int) Option<Int> {
  if radix < 2 or radix > 36 {
    error("Int.from_string_radix: radix must be in 2..=36")
  }

  if s.len() == 0 {
    return .None
  }

  first := s.slice(0, 1)
  neg := first == "-"
  start := if neg or first == "+" {
    1
  } else {
    0
  }

  if start >= s.len() {
    return .None
  }

  acc := 0
  i := start

  for i < s.len() {
    d := try radix_digit_value(s.slice(i, i + 1), radix)
    acc = acc * radix + d
    i = i + 1
  }

  if neg {
    .Some(0 - acc)
  } else {
    .Some(acc)
  }
}
```

- [ ] **Step 4: Format, regenerate, rebuild, and run the suite**

Run: `target/twk fmt boot/prelude/int.tw boot/tests/suites/api_int_suite.tw && python3 tools/generate_core_lib.py && make bundle-cli && make boot-test`
Expected: PASS.

- [ ] **Step 5: Document**

Add under `## Numeric (Int)` in `docs/API.md`:

```markdown
| `Int.from_string_radix` | `fn(s: String, radix: Int) Option<Int>` | Parse `s` in base `radix` (2–36); optional leading `+`/`-`; `.None` on empty or invalid digit; traps if `radix` is outside `2..=36` |
```

- [ ] **Step 6: Commit**

```bash
git add boot/prelude/int.tw boot/tests/suites/api_int_suite.tw docs/API.md
git commit -m "feat(int): add Int.from_string_radix for base parsing"
```

---

## Component B — Vector reductions

Six functions in `boot/prelude/vector.tw`. `sum`/`product` are `Vector<Int>`-monomorphic (following the `Vector.join` precedent of an element-type-specific reduction — there is no numeric contract, so `Vector<Float>` sums stay `fold`-based). `min`/`max` use the `Ord` contract; `min_by`/`max_by` take a comparator; `min_by_key`/`max_by_key` take a key projection. All empty-input cases return `.None` (or the identity for `sum`/`product`).

### Task B1: `Vector.sum` and `Vector.product`

**Files:**
- Modify: `boot/prelude/vector.tw` (add `sum`, `product`)
- Modify: `boot/tests/suites/api_vector_suite.tw` (add a test block)
- Modify: `docs/API.md` (add rows under `## Vector<T>`)

**Interfaces:**
- Produces:
  - `Vector.sum(xs: Vector<Int>) Int` — sum of elements; `0` for an empty vector.
  - `Vector.product(xs: Vector<Int>) Int` — product of elements; `1` for an empty vector.

- [ ] **Step 1: Write the failing test**

Append a new `.test(...)` block inside the `api Vector` suite builder chain in `boot/tests/suites/api_vector_suite.tw` (before the final closing `}` of `suite()`):

```tw
    .test(
      "sum and product reduce Int vectors with identity on empty",
      fn() {
        empty: Vector<Int> = []
        try assert.equal([1, 2, 3, 4].sum(), 10)
        try assert.equal(empty.sum(), 0)
        try assert.equal([1, 2, 3, 4].product(), 24)
        try assert.equal(empty.product(), 1)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run the suite to verify it fails**

Run: `make bundle-cli && make boot-test`
Expected: FAIL — `sum` is unresolved on `Vector<Int>`.

- [ ] **Step 3: Write the implementation**

Add to `boot/prelude/vector.tw`:

```tw
/// Sum of an Int vector; 0 for an empty vector.
pub fn sum(xs: Vector<Int>) Int {
  acc := 0

  for x in xs {
    acc = acc + x
  }

  acc
}

/// Product of an Int vector; 1 for an empty vector.
pub fn product(xs: Vector<Int>) Int {
  acc := 1

  for x in xs {
    acc = acc * x
  }

  acc
}
```

- [ ] **Step 4: Format, regenerate, rebuild, and run the suite**

Run: `target/twk fmt boot/prelude/vector.tw boot/tests/suites/api_vector_suite.tw && python3 tools/generate_core_lib.py && make bundle-cli && make boot-test`
Expected: PASS.

- [ ] **Step 5: Document**

Add under `## Vector<T>` in `docs/API.md`:

```markdown
| `.sum()` | `fn(xs: Vector<Int>) Int` | Sum of elements; `0` for an empty vector |
| `.product()` | `fn(xs: Vector<Int>) Int` | Product of elements; `1` for an empty vector |
```

- [ ] **Step 6: Commit**

```bash
git add boot/prelude/vector.tw boot/tests/suites/api_vector_suite.tw docs/API.md
git commit -m "feat(vector): add Int sum and product reductions"
```

### Task B2: `Vector.min` and `Vector.max` (Ord)

**Files:**
- Modify: `boot/prelude/vector.tw` (add `min`, `max`)
- Modify: `boot/tests/suites/api_vector_suite.tw` (add a test block)
- Modify: `docs/API.md` (add rows under `## Vector<T>`)

**Interfaces:**
- Produces:
  - `Vector.min(xs: Vector<T>) Option<T>` for `T: Ord` — least element by the `Ord` contract, `.None` when empty.
  - `Vector.max(xs: Vector<T>) Option<T>` for `T: Ord` — greatest element, `.None` when empty.

- [ ] **Step 1: Write the failing test**

Append inside the `api Vector` suite builder chain in `boot/tests/suites/api_vector_suite.tw`:

```tw
    .test(
      "min and max find extremes via the Ord contract",
      fn() {
        empty: Vector<Int> = []
        try assert.equal([3, 1, 4, 1, 5].min(), .Some(1))
        try assert.equal([3, 1, 4, 1, 5].max(), .Some(5))
        try assert.equal(["pear", "apple", "fig"].min(), .Some("apple"))
        try assert.equal(empty.min(), .None)
        try assert.equal(empty.max(), .None)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run the suite to verify it fails**

Run: `make bundle-cli && make boot-test`
Expected: FAIL — `min` is unresolved.

- [ ] **Step 3: Write the implementation**

Add to `boot/prelude/vector.tw` (the `.compare` call is the `Ord` contract method, same mechanism `Vector.sort` uses):

```tw
/// Least element by the Ord contract, or .None when empty.
pub fn min<T: Ord>(xs: Vector<T>) Option<T> {
  if xs.is_empty() {
    return .None
  }

  best := xs[0]

  for i in 1..xs.len() {
    if xs[i].compare(best) == .Lt {
      best = xs[i]
    }
  }

  .Some(best)
}

/// Greatest element by the Ord contract, or .None when empty.
pub fn max<T: Ord>(xs: Vector<T>) Option<T> {
  if xs.is_empty() {
    return .None
  }

  best := xs[0]

  for i in 1..xs.len() {
    if xs[i].compare(best) == .Gt {
      best = xs[i]
    }
  }

  .Some(best)
}
```

- [ ] **Step 4: Format, regenerate, rebuild, and run the suite**

Run: `target/twk fmt boot/prelude/vector.tw boot/tests/suites/api_vector_suite.tw && python3 tools/generate_core_lib.py && make bundle-cli && make boot-test`
Expected: PASS.

- [ ] **Step 5: Document**

Add under `## Vector<T>` in `docs/API.md`:

```markdown
| `.min()` | `fn<T: Ord>(xs: Vector<T>) Option<T>` | Least element by the `Ord` contract; `.None` when empty |
| `.max()` | `fn<T: Ord>(xs: Vector<T>) Option<T>` | Greatest element by the `Ord` contract; `.None` when empty |
```

- [ ] **Step 6: Commit**

```bash
git add boot/prelude/vector.tw boot/tests/suites/api_vector_suite.tw docs/API.md
git commit -m "feat(vector): add Ord-based min and max"
```

### Task B3: `Vector.min_by` and `Vector.max_by` (comparator)

**Files:**
- Modify: `boot/prelude/vector.tw` (add `min_by`, `max_by`)
- Modify: `boot/tests/suites/api_vector_suite.tw` (add a test block)
- Modify: `docs/API.md` (add rows under `## Vector<T>`)

**Interfaces:**
- Consumes: nothing from B2 (independent, though conceptually parallel).
- Produces:
  - `Vector.min_by(xs: Vector<T>, cmp: fn(T, T) Order) Option<T>` — least element by `cmp`, `.None` when empty.
  - `Vector.max_by(xs: Vector<T>, cmp: fn(T, T) Order) Option<T>` — greatest element by `cmp`, `.None` when empty.

- [ ] **Step 1: Write the failing test**

Append inside the `api Vector` suite builder chain in `boot/tests/suites/api_vector_suite.tw`:

```tw
    .test(
      "min_by and max_by use a comparator",
      fn() {
        empty: Vector<Int> = []
        try assert.equal([3, 1, 4, 1, 5].min_by(Int.compare), .Some(1))
        try assert.equal([3, 1, 4, 1, 5].max_by(Int.compare), .Some(5))
        try assert.equal(empty.min_by(Int.compare), .None)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run the suite to verify it fails**

Run: `make bundle-cli && make boot-test`
Expected: FAIL — `min_by` is unresolved.

- [ ] **Step 3: Write the implementation**

Add to `boot/prelude/vector.tw`:

```tw
/// Least element by a comparator, or .None when empty.
pub fn min_by<T>(xs: Vector<T>, cmp: fn(T, T) Order) Option<T> {
  if xs.is_empty() {
    return .None
  }

  best := xs[0]

  for i in 1..xs.len() {
    if cmp(xs[i], best) == .Lt {
      best = xs[i]
    }
  }

  .Some(best)
}

/// Greatest element by a comparator, or .None when empty.
pub fn max_by<T>(xs: Vector<T>, cmp: fn(T, T) Order) Option<T> {
  if xs.is_empty() {
    return .None
  }

  best := xs[0]

  for i in 1..xs.len() {
    if cmp(xs[i], best) == .Gt {
      best = xs[i]
    }
  }

  .Some(best)
}
```

- [ ] **Step 4: Format, regenerate, rebuild, and run the suite**

Run: `target/twk fmt boot/prelude/vector.tw boot/tests/suites/api_vector_suite.tw && python3 tools/generate_core_lib.py && make bundle-cli && make boot-test`
Expected: PASS.

- [ ] **Step 5: Document**

Add under `## Vector<T>` in `docs/API.md`:

```markdown
| `.min_by(cmp)` | `fn<T>(xs: Vector<T>, cmp: fn(T,T) Order) Option<T>` | Least element by comparator; `.None` when empty |
| `.max_by(cmp)` | `fn<T>(xs: Vector<T>, cmp: fn(T,T) Order) Option<T>` | Greatest element by comparator; `.None` when empty |
```

- [ ] **Step 6: Commit**

```bash
git add boot/prelude/vector.tw boot/tests/suites/api_vector_suite.tw docs/API.md
git commit -m "feat(vector): add comparator-based min_by and max_by"
```

### Task B4: `Vector.min_by_key` and `Vector.max_by_key`

**Files:**
- Modify: `boot/prelude/vector.tw` (add `min_by_key`, `max_by_key`)
- Modify: `boot/tests/suites/api_vector_suite.tw` (add a test block)
- Modify: `docs/API.md` (add rows under `## Vector<T>`)

**Interfaces:**
- Produces:
  - `Vector.min_by_key(xs: Vector<T>, f: fn(T) K) Option<T>` for `K: Ord` — element with the least projected key, `.None` when empty.
  - `Vector.max_by_key(xs: Vector<T>, f: fn(T) K) Option<T>` for `K: Ord` — element with the greatest projected key, `.None` when empty.

- [ ] **Step 1: Write the failing test**

Append inside the `api Vector` suite builder chain in `boot/tests/suites/api_vector_suite.tw`:

```tw
    .test(
      "min_by_key and max_by_key project an Ord key",
      fn() {
        words := ["bb", "a", "cccc", "ddd"]
        try assert.equal(words.min_by_key(fn(w: String) Int { w.len() }), .Some("a"))
        try assert.equal(words.max_by_key(fn(w: String) Int { w.len() }), .Some("cccc"))
        empty: Vector<String> = []
        try assert.equal(empty.min_by_key(fn(w: String) Int { w.len() }), .None)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run the suite to verify it fails**

Run: `make bundle-cli && make boot-test`
Expected: FAIL — `min_by_key` is unresolved.

- [ ] **Step 3: Write the implementation**

Add to `boot/prelude/vector.tw` (the projected key is cached so `f` runs once per element):

```tw
/// Element with the least projected Ord key, or .None when empty.
pub fn min_by_key<T, K: Ord>(xs: Vector<T>, f: fn(T) K) Option<T> {
  if xs.is_empty() {
    return .None
  }

  best := xs[0]
  best_key := f(best)

  for i in 1..xs.len() {
    k := f(xs[i])

    if k.compare(best_key) == .Lt {
      best = xs[i]
      best_key = k
    }
  }

  .Some(best)
}

/// Element with the greatest projected Ord key, or .None when empty.
pub fn max_by_key<T, K: Ord>(xs: Vector<T>, f: fn(T) K) Option<T> {
  if xs.is_empty() {
    return .None
  }

  best := xs[0]
  best_key := f(best)

  for i in 1..xs.len() {
    k := f(xs[i])

    if k.compare(best_key) == .Gt {
      best = xs[i]
      best_key = k
    }
  }

  .Some(best)
}
```

- [ ] **Step 4: Format, regenerate, rebuild, and run the suite**

Run: `target/twk fmt boot/prelude/vector.tw boot/tests/suites/api_vector_suite.tw && python3 tools/generate_core_lib.py && make bundle-cli && make boot-test`
Expected: PASS.

- [ ] **Step 5: Document**

Add under `## Vector<T>` in `docs/API.md`:

```markdown
| `.min_by_key(f)` | `fn<T,K: Ord>(xs: Vector<T>, f: fn(T) K) Option<T>` | Element with the least projected key; `.None` when empty |
| `.max_by_key(f)` | `fn<T,K: Ord>(xs: Vector<T>, f: fn(T) K) Option<T>` | Element with the greatest projected key; `.None` when empty |
```

- [ ] **Step 6: Commit**

```bash
git add boot/prelude/vector.tw boot/tests/suites/api_vector_suite.tw docs/API.md
git commit -m "feat(vector): add key-projection min_by_key and max_by_key"
```

---

## Finalization

- [ ] **Run the full gate one last time.** `make bundle-cli && make boot-test` (green: `Ran N tests: N passed`), plus `cargo test --release` if any doubt about stage0 interplay (there should be none — no Rust changed).
- [ ] **Lint.** `target/twk lint boot/main.tw` — resolve or justify any findings on the edited prelude files.
- [ ] **Update the plan index.** On completion, remove this plan's row from `docs/plans/README.md` and move this doc to `docs/plans/archive/` (per the plan lifecycle: completed plans are archived, not marked "Done" in place).

## Notes on scope (YAGNI)

- **`sum`/`product` are `Int`-only** by design — there is no numeric contract, and `Vector.join` sets the precedent for element-type-specific reductions. `Vector<Float>` totals use `fold(0.0, fn(a, x) { a + x })`.
- **Deferred (Tier 2/3, not in this plan):** `Dict.update`/`get_or`/`merge`, `String.split_once`/`trim_start`/`trim_end`, public `Float.is_nan`/`is_finite`, and `Vector.partition`/`distinct`/`zip`. Each is its own future plan.
