# Runtime trace polish: ANSI color + name demangling

Status: design approved, ready for implementation plan.

Two independent, renderer-only refinements to source-mapped runtime trap traces
(the feature whose capture/render/disk-backed-debug-info/user-frame/rich-OOB work
already landed on `main`). Neither changes an ABI or requires bootstrap
sequencing: both live in `boot/lib/debug/` and `boot/runtime_trace_renderer.tw`,
and are exercised by `target/twk run boot/tests/main.tw` and `cli.test.mjs`'s
`buildRenderer` without a full compiler rebuild. `make bundle-cli` is only needed
to ship `target/renderer.wasm` into the real `target/twk` for manual end-to-end
verification.

## Goals

- **Phase 4.4 — ANSI color.** Runtime traces render monochrome because the
  renderer forces `.{ color: false }`. Give them the same color behavior as
  compile diagnostics.
- **Phase 4.2 — name demangling.** Backtrace lines print mangled Wasm names like
  `user__$f351_cell` and `user__$f352__init`. Render readable names.

Non-goals (deferred): div0/rem0 guard, string/dict/slice OOB, public `rt.panic`,
`twk build --strip-debug` (4.5), docs (4.6).

## Background: the mangling scheme (verified in-tree)

Final Wasm function name = `<ns_prefix>__<sym>`:

- `ns_prefix` (`linker.tw` `ns_prefix`/`qualify`) is the module namespace with
  `.` → `_`. User code is namespace `"user"` (`emit.tw` builds the user module
  with `namespace: "user"`). `rt.*` runtime helpers keep an `rt_` prefix and are
  not user-qualified.
- `sym` (`emit.tw`): `"$f" + func_id + "_" + sanitize_name(source_name)`.
  `sanitize_name` (`emit/helpers.tw`) maps every non-`[A-Za-z0-9_]` byte to `_`.

Concrete forms (confirmed by running a trapping program):

| Source                    | `sym` tail after `$f<id>_` | Final name              |
|---------------------------|----------------------------|-------------------------|
| `fn cell(...)`            | `cell`                     | `user__$f350_cell`      |
| top-level body (`$init`)  | `_init` (`$`→`_`)          | `user__$f352__init`     |
| lambda (`__lambda`)       | `__lambda`                 | `user__$fN___lambda`    |
| generic `sort_by` @ `Int` | `sort_by__Int`             | `user__$f361_sort_by__Int` |

Monomorphized suffixes come from `monomorphize.tw` `type_key`/`join_parts` and are
always PascalCase-leading (`Int`, `Float`, `Vec_Int`, `T7_Int`, `Fn_..`,
`Dict_..`, `Opt_..`, `Res_..`, `Ext<n>`, `M<n>`, or a PascalCase type-param
`Var`). User function names are snake_case (lowercase first char), so a `__`
immediately followed by an uppercase ASCII letter reliably marks the start of a
monomorphization suffix.

The `$f<digits>_` marker is the reliable anchor: the only literal `$` in a final
name comes from the emit-level `$f`/`$extern` prefixes (source `$` is sanitized
away), so matching it also strips the namespace prefix for free.

## Part A — ANSI color (Phase 4.4)

`boot/runtime_trace_renderer.tw`: replace `render_config()`'s hardcoded
`.{ color: false }` with `report.default_config()` (add `default_config` to the
`use lib.source.report.{...}` import list), and delete the stale
"swap this for `report.default_config()`" comment.

`default_config()` (`report.tw`) returns `color: true` unless `NO_COLOR` is set
non-empty — byte-for-byte the rule every compile diagnostic already uses
(`commands/common.tw`, `fmt.tw`, `lint.tw`). No TTY gate: the compiler doesn't
TTY-gate either, so this is strict parity. The renderer lib is instantiated with
`env` wired (`runtime.mjs` `renderChildTrace`), so `proc.env("NO_COLOR")`
resolves inside it.

## Part B — name demangling (Phase 4.2)

New module `boot/lib/debug/demangle.tw` exposing one function:

```
pub fn demangle_frame_name(raw: String) String
```

`trace.tw` imports it dot-relative (`use .demangle.{demangle_frame_name}`) and
runs each resolved frame's `rf.frame.name` through it in `backtrace_lines` — the
only place a name renders (the primary caret/snippet carries no name). The
existing `.None` → `<anonymous>` fallback for nameless frames is preserved
(demangling only applies to `.Some(name)`).

### Algorithm

1. Locate the `$f` marker. Require a run of ASCII digits after it, then a `_`.
   If the shape doesn't match, return `raw` unchanged — externs (`$extern_...`),
   `rt_*` helpers, and raw exports degrade gracefully instead of being mangled
   further. A successful match also discards everything up to and including that
   `_`, dropping the namespace prefix (`user__`, `std_view__`, …).
2. Let `tail` be the substring after that underscore. Classify:
   - `tail == "_init"` → `"<script>"` (the synthetic top-level `$init`).
   - `tail` starts with `"__lambda"` → `"<closure>"` (covers both `__lambda` and
     monomorphized `__lambda__Int`).
   - otherwise strip the monomorphization suffix: find the first `__` whose next
     character is an uppercase ASCII letter (`A`–`Z`) and cut there, returning the
     base. `sort_by__Int` → `sort_by`. A user name with a genuine double
     underscore followed by lowercase (`my__thing`) is left intact.

### Readable-name choices (approved)

- top-level `$init` → `<script>`
- lambda `__lambda` → `<closure>`
- monomorphized `base__TypeArgs` → `base` (strip; `file:line:col` disambiguates
  which instantiation)

Known, accepted ambiguity: a user function literally named `_init` (leading
underscore) would also render as `<script>`. Display-only; acceptable.

## Testing

- **`demangle.tw` unit suite (new).** Direct `demangle_frame_name` cases:
  `user__$f350_cell` → `cell`; `user__$f352__init` → `<script>`;
  `user__$f7___lambda` → `<closure>`; `user__$f7___lambda__Int` → `<closure>`;
  `user__$f361_sort_by__Int` → `sort_by`; `user__$f5_my__thing` → `my__thing`
  (untouched); a non-`$f` name (e.g. `rt_arr__get` / `$extern_..`) → unchanged;
  a stdlib-namespaced `std_view__$f12_from` → `from`.
- **`trace_render_suite.tw`.** Add integration cases with realistic
  `user__$fN_` frame names, asserting the demangled `at cell (...)` /
  `at <script> (...)` lines under the existing `plain()` (`color: false`) config.
- **`runtime_trace_renderer_suite.tw` (color ripple).** Its `str_contains`
  assertions on `render_runtime_trace` output break once ANSI is injected (the
  boot test env has `NO_COLOR` unset → color on). Fix: add a local `strip_ansi`
  helper and assert content against the stripped output, so the suite passes
  whether or not `NO_COLOR` is set. Add a parity assertion driven by
  `report.default_config().color`: when true, assert `out.contains("\e[")`; when
  false, assert it does not — correct under both env states.

## Verification (sequential, foreground; never full `cargo test`)

1. `target/twk fmt <touched .tw>` then `target/twk lint boot/main.tw`
   (~4 pre-existing findings in section.tw/builtins.tw/census.tw are unrelated).
2. `target/twk run boot/tests/main.tw` (renderer changes picked up, no rebuild).
3. `make bundle-cli`, then manual `target/twk run` on a trap fixture
   (`xs[7]` on a length-3 vector) to confirm color + demangled backtrace in the
   shipped CLI.
4. `cp target/boot.wasm tools/js_runtime/boot.wasm`, then
   `deno test -A tools/js_runtime/` (a pre-existing web.test.mjs env-quirk
   failure is unrelated).

## Workflow

One feature branch (`runtime-trace-color-demangle`). Subagent-driven execution
with a task review after each part and a whole-branch review before finishing.
Commits: imperative subject, what/why/how, no line/count metrics, with the
`Co-Authored-By` / `Claude-Session` trailers.
