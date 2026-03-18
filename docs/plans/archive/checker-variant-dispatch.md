# Checker: Unify Optional/Result/Sum variant dispatch

## Problem

`check_variant_lit` and `check_variant_pattern` both follow the same
three-phase sequential dispatch pattern:

```tw
case zonked { .Optional(inner) => { ... return }, _ => {} }
case zonked { .Result(ok, err) => { ... return }, _ => {} }
case resolve_variant_info(name, zonked, ctx.env) { ... }
// fallback error
```

Each case block matches one variant kind, does all its work, and returns.
The `_ => {}` arms are pure fallthrough noise. The same logic structure
is duplicated between the "lit" (expression) and "pattern" (match arm)
sides, with only the inner action differing (synth+unify vs check_pattern).

## Proposed approach: normalize to VariantInfo first, dispatch once

The key insight is that all three paths ultimately need the same thing:
a list of field types for the named variant. Optional, Result, and
user-defined sums just resolve that list differently.

### Step 1: Extend `resolve_variant_info` to handle Optional and Result

Currently `resolve_variant_info` only handles `Named(tid, args)` user sums.
Extend it to also handle Optional and Result:

```tw
fn resolve_variant_info(name: String, zonked: MonoType, env: ResolvedEnv) Result<VariantInfo, String>? {
  case zonked {
    .Optional(inner) => {
      if name == "Some" { return .Some(.Ok(.{ field_types: [inner] })) }
      if name == "None" { return .Some(.Ok(.{ field_types: [] })) }
      return .Some(.Err("unknown variant .${name} for Optional"))
    },
    .Result(ok_ty, err_ty) => {
      if name == "Ok" { return .Some(.Ok(.{ field_types: [ok_ty] })) }
      if name == "Err" { return .Some(.Ok(.{ field_types: [err_ty] })) }
      return .Some(.Err("unknown variant .${name} for Result"))
    },
    .Named(tid, type_args) => {
      // ... existing user-sum logic unchanged ...
    },
    _ => .None,
  }
}
```

### Step 2: Collapse check_variant_lit to a single dispatch

```tw
fn check_variant_lit(name, args, expected, s, ctx, diags) CheckOut {
  zonked := expand_alias(zonk(expected, ctx.subst), ctx.env, 0)
  case resolve_variant_info(name, zonked, ctx.env) {
    .Some(.Ok(info)) => {
      // check arg count, synth+unify each arg against info.field_types
    },
    .Some(.Err(msg)) => .{ ctx, diags: diags.push(diag.error(s, msg)) },
    .None => .{ ctx, diags: diags.push(diag.error(s, "variant .${name} used where ${ty_to_string(zonked)} expected")) },
  }
}
```

### Step 3: Same for check_variant_pattern

```tw
fn check_variant_pattern(name, sub_pats, expected, s, ctx, diags) CheckOut {
  zonked := expand_alias(zonk(expected, ctx.subst), ctx.env, 0)
  case resolve_variant_info(name, zonked, ctx.env) {
    .Some(.Ok(info)) => {
      // check sub_pat count, check_pattern each sub_pat against info.field_types
    },
    .Some(.Err(msg)) => .{ ctx, diags: diags.push(diag.error(s, msg)) },
    .None => .{ ctx, diags: diags.push(diag.error(s, "cannot match .${name} against ${ty_to_string(zonked)}")) },
  }
}
```

## What changes

- `resolve_variant_info`: grows Optional/Result arms (~10 lines)
- `check_variant_lit`: shrinks from ~100 lines to ~25
- `check_variant_pattern`: shrinks from ~70 lines to ~20
- Net reduction: ~100 lines removed, zero behavior change

## Risks / things to verify

- **Nested variant patterns**: `case .Some(.Ok(info))` — verify Twinkle
  parser supports nested variant patterns in case arms. If not, use
  two-level case or a `case result { .Ok(info) => ..., .Err(msg) => ... }`
  inside the `.Some` arm (which is what the code already does today).
- **Error messages**: the current code has slightly different wording per
  type ("for Optional" vs "for Result"). The unified version routes all
  error messages through `resolve_variant_info`, so the messages stay
  specific — they're constructed in the Optional/Result arms there.
- **`get_variant_names`** (used by exhaustiveness checking) also has
  separate Optional/Result/Named arms. Could apply the same pattern,
  but it's simpler code so lower priority.
