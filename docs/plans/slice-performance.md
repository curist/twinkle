# Slice usage audit & performance

Status: findings + proposal. Companion to
[queue-deque.md](queue-deque.md) (Vector end-access) and
[rrb-vector-concat.md](rrb-vector-concat.md) (general O(log n) concat/slice).

This doc holds the **boot-compiler slice audit** and the **String-slice
performance** discussion. The actionable Vector work lives in the two companion
docs; this is the evidence behind them plus the String side they don't cover.

## Audit: how the boot compiler uses `slice`

A scan of `boot/` (excluding tests, the `arr.tw`/`str.tw` runtime impls, and the
`core_lib.tw` embedded-source string literals). Two dominant realities:

1. **The vast majority of `.slice(` is `String` substring** — lexer, paths,
   JSON, LSP framing, signatures/hover/doc-comment parsing. This is where slice
   performance actually matters at volume.
2. **`Vector` slice is comparatively rare and mostly LIFO stack pop**, not FIFO
   dequeue.

### Vector `.slice` sites

| Pattern | Sites | Notes |
|---|---|---|
| **LIFO pop** `xs.slice(0, len-1)` | `checker.tw:85` & `lower_core/context.tw:101` (`pop_scope`), `codegen/type_order.tw:209` (Tarjan SCC worklist), `fmt/layout.tw:224` (`fit_stack`), `fmt/printer.tw:118` (trivia), `lexer.tw:369/379/394` (`interp_depths`) | scope stacks hot but bounded-depth; **Tarjan worklist can be large → genuine O(n²)** |
| **FIFO head-drop** `xs.slice(1, len)` | `emit/match.tw` ×4 (**recursive** head/tail over arms → O(k²)), `fmt/printer.tw:1242/1273` (recursive doc parts) | k usually modest |
| one-shot drop-first | `loader.tw:74`, `checker.tw:1935/2006`, `run.tw`, `argv.tw` | harmless (not loops) |

**Takeaway**: a FIFO `Queue` would help almost none of these; the LIFO majority
wants an O(log n) `drop_last` vector op (and the match-arm recursion wants an
index instead of a slice). Full treatment in [queue-deque.md](queue-deque.md).
Arbitrary/left-drop slice → O(log n) only via RRB
([rrb-vector-concat.md](rrb-vector-concat.md)).

### String `.slice` sites — the volume case

Concentrated in `lexer.tw`, `signatures.tw`, `parser.tw`, `query/hover.tw`,
`lib/source/*`, `lib/lsp/*`, `path`/`json`. A recurring shape is **slice purely
to compare** — allocating a substring that is immediately thrown away:

```tw
trimmed.slice(0, 3) == "///"          // signatures.tw, parser.tw, hover.tw
input.slice(i, end) == text           // json.tw
s.slice(0, m) == prefix               // core_lib starts_with / strip_prefix
s.slice(n - m, n) == suffix           // core_lib ends_with / strip_suffix
```

Each allocates a fresh byte array just to byte-compare it. That is pure garbage.

## String representation today

`String` is `(array (mut i8))` — a flat, contiguous, logically-immutable byte
array. `slice`/`substring` (`str.rs:substring_fn`, `str.tw`) clamps the range,
`array.new`s a fresh `$String`, and `array.copy`s the bytes in:

- `slice(s, a, b)` → **O(m)** time + **O(m)** allocation, m = b−a. No sharing.
- `concat(a, b)` → **O(\|a\|+\|b\|)** time + allocation (fresh contiguous array).
- `s[i]` / `char_code_at` → O(1).

So substring is never free, and "slice-then-compare" pays an allocation for
nothing.

## Improving String slice — three tiers

### Tier 1 — Allocation-free compare/scan (recommended first; pure stdlib)

Most hot slices never need a `String` object — they need a comparison or a scan.
Compare the byte range in place via O(1) indexing instead of allocating a
substring just to compare it. O(m) time, **zero allocation**, pure Twinkle in
`prelude`/`stdlib` (no compiler or runtime change).

**Done so far** (`prelude/string.tw`, commit "Make string prefix/suffix checks
allocation-free"): added a private `region_eq(s, start, other)` helper and
rewired `starts_with` / `ends_with` / `strip_prefix` / `strip_suffix` to use it
instead of `s.slice(...) == other`. `strip_*` still allocate only the *remainder*
they return. Verified by self-host fixpoint + boot tests. `region_eq` is kept
**private** for now — the public surface is deferred pending the view direction
below.

**Remaining Tier-1 candidates** (optional, separable):

- The inline `s.slice(a, b) == lit` sites — e.g. `trimmed.slice(0, 3) == "///"`
  in `signatures.tw`, `parser.tw`, `query/hover.tw`; `input.slice(i, end) == text`
  in `json.tw`. **Decision: do not add a public compare API** (`equals_at` /
  `region_eq` / offset-arg `starts_with`) for these. Instead they go the
  **`StringView` route** (Tier 2 / Option V1) — `region_eq` stays a private
  internal helper. So these inline sites are left as-is until `StringView` lands.

Precedent for *why we're not* adding a public compare API: copying-slice
languages do (Java `regionMatches`, C `memcmp`/`strncmp`, C++ `string::compare`,
Python/JS offset-arg `startswith`), but **view-slice languages — Rust `&str`, Go,
C++ `string_view`, Swift `Substring` — skip all that because their slice is
zero-copy.** We're choosing that camp (Tier 2 / `StringView`), so we deliberately
avoid growing a parallel compare-primitive surface.

### Tier 2 — O(1) views (when Tier 1's tuple-threading gets unwieldy)

Tier 1 avoids allocation but threads `(s, start, end)` by hand, which gets
clumsy for *multi-step* scanning (advance, sub-slice, compare, repeat). A view
makes that composable while staying allocation-free. Two ways to get views:

**Option V1 — a separate `StringView` / `StringSlice` type (recommended view).**
A GC record over a backing string:

```tw
pub type StringView = .{ source: String, start: Int, len: Int }
pub fn view(s: String, start: Int, end: Int) StringView   // O(1), no copy
// view[i] → source[start+i]; view.slice(a,b) → O(1) sub-view;
// region_eq / starts_with / scan on the view; view.to_string() → O(m) materialize
```

This is the Twinkle analog of C++ `std::string_view`, Rust `&str`, Java
`CharSequence` — an explicit non-owning view.

- `String` **stays a flat array** — no global repr change, every existing string
  op untouched.
- The "tiny slice pins a big backing" retain hazard is **opt-in and localized**
  to where you take views, not forced on all strings.
- Natural fit for the **lexer/parser** (the volume consumers): scan and compare
  over views, materialize a `String` only for token text. The rest of the
  compiler keeps using `String` unchanged.
- **Cost**: no implicit coercion in Twinkle (no traits), so a `StringView` is not
  a `String`; handing a substring to a `String`-typed API needs `.to_string()`,
  and you maintain a parallel view-accepting surface. Manageable precisely
  because the heavy consumers (lexer/parser) are self-contained.
- Can start as a pure-stdlib type; **this is the chosen direction** for the
  zero-copy story, so it warrants its own implementation plan doc (as the queue
  type did).

**Option V2 — make `String` itself a view** (`{ bytes, start, len }`, Go-style).
Transparent everywhere (slice on any string is O(1), no API friction), but a
broad-but-mechanical change to every string op (`eq`/`cmp`/`utf8_bytes`/iteration
honor `start`), and the retain caveat applies to *all* strings (mitigate with a
copy-on-small-slice heuristic). Safe because strings are immutable.

Either way `concat` is **unchanged** (still copies into a fresh contiguous
backing) — a view helps slice, not concat.

Recommendation between them: **V1** — keep `String` simple and make views opt-in
where they pay off, rather than reshaping the core type for a localized win.

### Tier 3 — Rope / cord (O(log n) slice **and** concat)

Tree-structured string (essentially `Vector<Byte>` with the RRB treatment, or a
cord of chunks): O(log n) slice and concat, at the price of O(log n) indexing and
real complexity. Only justified by a genuine large-string manipulation workload —
the compiler is not one (it builds output via `Vector<Byte>` buffers + a single
`from_utf8`, e.g. `join`, avoiding repeated string concat). **Unlikely worth it.**

## Recommendation

1. **Tier 1** — *done* for prefix/suffix (private `region_eq`). **No public
   compare API** beyond that, by decision.
2. **Tier 2 / Option V1 (`StringView`)** — the **chosen direction** for the rest
   (the inline `slice(...) == lit` sites and zero-copy scanning generally),
   preferred over both a public compare API and reshaping `String` itself (V2).
   To be drafted as its own implementation plan.
3. **Tier 3** — defer indefinitely absent a big-string workload.

For `Vector` slice, see the companion docs (drop_last op; RRB for left-drop).

## Open questions

- ~~**Tier 1 surface**~~ — resolved: `region_eq` stays private; **no public
  compare API**. The remaining sites go the `StringView` route.
- **`StringView` scope (V1)** — for the dedicated plan: which ops does the view
  need (indexing, sub-view, `region_eq`, `starts_with`, `find`, `to_string`, byte
  iteration), and how far do the lexer/parser migrate to views?
- **V1 retain control**: V1 localizes backing-retention to explicit views (vs V2,
  which would need a copy-on-small-slice threshold across all strings).
- **Interaction with String interpolation / concat**: is repeated `"${...}"`
  building anywhere O(n²)? (Out of scope here, but worth a glance — concat, not
  slice, would be the culprit.)
