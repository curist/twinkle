# Diagnostic Type Name Snapshots

## Goal

Prevent user-facing diagnostics from rendering internal type ids such as
`Named(446)` when a diagnostic contains a `MonoType.Named(TypeId, ...)` whose
name cannot be resolved by the environment available at final render time.

Diagnostics should render stable source-level type names, for example:

```text
missing field `diagnostics_dirty` in record literal for `server_core.State`
```

rather than:

```text
missing field `diagnostics_dirty` in record literal for `Named(446)`
```

## Problem

Boot diagnostics are structured. Many diagnostic variants carry `MonoType`, and
`MonoType.Named(TypeId, args)` stores only an internal `TypeId`. Human-readable
names are recovered later through `ResolvedEnv.ty_to_string_env`.

That late lookup is fragile because diagnostics can be rendered with an env that
is not the same env that produced the diagnostic. This happens naturally in the
query pipeline:

* an analysis result can contain diagnostics from multiple modules;
* imported modules can remap type ids/names into importer environments;
* `DiagnosticsError` currently carries one `ResolvedEnv`, but a diagnostic may
  have been emitted under a different module-local env;
* LSP diagnostics flow through another conversion layer where only protocol data
  should remain.

When the render env cannot resolve a diagnostic's `TypeId`, the fallback debug
printer in `compiler/mono_type.tw` emits `Named(<id>)`. That fallback is useful
for internal debugging, but it should not appear in normal CLI/LSP-facing
messages.

## Design

Attach a small type display snapshot to each analysis diagnostic at the point the
diagnostic is produced.

```tw
pub type TypeNameSnapshot = Dict<Int, String>

pub type AnalysisDiag = .{
  identity: identity.SourceIdentity,
  version: Int?,
  kind: diag.DiagKind,
  stage: String,
  data: json.Json?,
  type_names: TypeNameSnapshot,
}
```

The snapshot maps every known `TypeId.id` in the producing environment to the
best source-level display name for that diagnostic context.

```tw
fn type_name_snapshot(env: ResolvedEnv) Dict<Int, String> {
  names: Dict<Int, String> = Dict.new()
  for i in 0..env.types.len() {
    names[env.types[i].id.id] = env.type_names[i]
  }
  names
}
```

Then diagnostic rendering should use the diagnostic snapshot before falling back
to the render env or debug printer.

Recommended lookup order for `MonoType.Named(tid, args)`:

1. diagnostic-local `type_names[tid.id]`
2. render context `env.find_type_name(tid)` when available
3. debug fallback `Named(${tid.id})`

The debug fallback remains, but it becomes a last-resort internal escape hatch.

## Rendering API Shape

Extend `diag_render.RenderCtx` with an optional snapshot:

```tw
pub type RenderCtx = .{
  registry: FileRegistry,
  env: ResolvedEnv?,
  type_names: Dict<Int, String>?,
  module: Module?,
  config: RenderConfig,
}
```

Update `fmt_ty` to use a local helper instead of calling
`env.ty_to_string_env` directly. The helper should recursively render nested
`MonoType` values so type arguments also benefit from the same snapshot.

```tw
fn fmt_ty(ty: MonoType, ctx: RenderCtx) String
```

For `.Named(tid, args)`, resolve the base name using the lookup order above and
then recursively render `args`.

## Snapshot Production

Add helpers in `boot/compiler/query/analyze.tw`:

```tw
fn empty_type_names() Dict<Int, String>
fn type_name_snapshot(env: ResolvedEnv) Dict<Int, String>
fn wrap_diags_with_types(source, stage, diags, type_names) Vector<AnalysisDiag>
fn wrap_diags_from_env(source, stage, diags, env) Vector<AnalysisDiag>
```

Use these at diagnostic creation sites:

* parse diagnostics: empty snapshot is acceptable because parse diagnostics do
  not carry `MonoType`;
* import/source synthetic diagnostics: empty snapshot is acceptable;
* resolver diagnostics: snapshot the resolver env returned by the resolve stage;
* checker diagnostics: snapshot `checked.env`, not the pre-check env;
* unused import warnings: empty snapshot is acceptable;
* any future lowering/checking diagnostics that carry `MonoType`: snapshot the
  env used to produce them.

The checker case is especially important because type diagnostics are produced
after inference/zonking, and `checked.env` is the closest context to the emitted
`MonoType`s.

## LSP Diagnostic Conversion

`boot/compiler/query/diagnostics.tw` currently converts `AnalysisDiag` into an
LSP-facing diagnostic shape. LSP clients receive pre-rendered messages, not rich
`DiagKind` reports.

Two reasonable options:

* keep LSP messages from `kind_to_message` simple and not type-rich for now;
* or extend the LSP conversion path to format messages through `diag_render`
  using the diagnostic snapshot.

For this plan, prefer the minimal change: preserve existing LSP behavior, but
carry the snapshot through if a future LSP message renderer needs it. The primary
bug being fixed is terminal rendering of rich reports.

## Migration Steps

### Add snapshot field

Update `AnalysisDiag` in `boot/compiler/query/analyze.tw` with a
`type_names: Dict<Int, String>` field. Update all constructors:

* `synthetic_diag`
* `convert_unused_import_diags`
* parser/resolve/check wrapping helpers
* tests that construct `AnalysisDiag` directly

### Update diagnostic renderer

Extend `RenderCtx` with `type_names`. Update tests and call sites to pass
`.None` where no diagnostic-local snapshot is available.

Change `fmt_ty` to prefer the snapshot. Keep `env.ty_to_string_env` available
for non-diagnostic callers, but avoid using it directly inside rich diagnostic
rendering because it cannot see per-diagnostic snapshots.

### Pass snapshots at render sites

In `boot/commands/common.tw` and `boot/compiler/pipeline.tw`, construct
`RenderCtx` with:

```tw
type_names: .Some(d.type_names)
```

The `env` field can remain as an additional fallback.

### Prefer producing env for check diagnostics

In `resolve_and_check_local`, when `checked.diagnostics.len() > 0`, wrap them
with a snapshot of `checked.env`. Do not use the pre-check `env` for these
diagnostics.

### Add regression coverage

Add a diagnostic-rendering or query-pipeline regression that creates a record
literal error involving an imported record type. The rendered report should
contain the source-level imported type name and should not contain `Named(`.

A representative scenario:

```tw
// dep.tw
pub type State = .{ old_field: Int, diagnostics_dirty: Bool }

// main.tw
use dep
s: dep.State = dep.State.{ old_field: 1 }
```

The missing-field diagnostic should render `dep.State` or another stable
source-level spelling instead of a raw `Named(id)`.

## Non-goals

* Do not remove `MonoType.Named(TypeId, ...)`; the internal representation is
  still correct.
* Do not remove debug fallback rendering from `compiler/mono_type.tw`.
* Do not redesign type-id allocation or import remapping in this plan.
* Do not require LSP diagnostics to render full rich type messages immediately.

## Expected Outcome

Diagnostics become self-contained enough for type display. A diagnostic can be
rendered later, after module analysis or through CLI/LSP boundaries, without
requiring the caller to guess which module environment originally produced the
`MonoType`s it carries.
