# Host write_bytes First-Class Fixture Plan

## Repro

`repro_host_write_bytes_builtin_first_class_function_arg_uses_wrapper_trampoline`
in `boot/tests/suites/codegen_integration_suite.tw`.

```tw
fn apply(f: fn(String, Vector<Byte>) Void, path: String, bytes: Vector<Byte>) Void {
  f(path, bytes)
}
apply(host.write_bytes, "/tmp/twinkle.out", [1, 2, 3])
```

## Symptom

The resolver reports `host` as an undefined variable. This fails before codegen,
so the repro does not currently exercise first-class host builtin wrapping.

## Design question

The test is using `host.write_bytes` as if `host` were a source-level module
namespace. Twinkle already exposes file I/O through public stdlib APIs such as
`@std.fs.write_bytes`, while raw host helpers are compiler/runtime internals.

The proper fix depends on the intended public model:

- If raw host namespaces are not source syntax, this repro should be rewritten to
  use the supported stdlib API or moved to a lower-level backend fixture.
- If raw host namespaces are intended as a compiler feature, resolver and docs
  need to expose that namespace deliberately.

## Preferred direction

Keep host helpers internal unless a broader language design requires exposing
them. Source-level tests should use public APIs. Backend-level tests can still
construct direct host builtin function values when the point is wrapper/trampoline
emission rather than source resolver behavior.

## Proper fix

- Decide whether source code may name raw host helpers directly.
- If not, rewrite the codegen integration repro to use a supported public API, or
  move this coverage into a backend test that constructs the host builtin value
  directly.
- If yes, add resolver support for a `host` namespace, document the syntax, and
  define which host functions are stable source-level API.
- Ensure the selected test still validates wrapper/trampoline emission for a
  first-class host builtin with `Vector<Byte>` ABI adaptation.

## Validation

- Add source-level coverage only for public syntax.
- Add backend-level coverage for direct host builtin function values if the raw
  helper stays internal.
- Run `target/twk run boot/tests/main.tw`.

## Non-goals

- Do not add `host` as a magical variable only for this repro.
- Do not expose all runtime host imports as public language API accidentally.
